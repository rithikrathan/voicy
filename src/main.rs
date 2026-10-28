mod audio;
mod config;
mod ipc;
mod output;
mod whisper;

use clap::{Parser, Subcommand};
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::Arc;
use std::time::Duration;

#[derive(Parser)]
#[command(name = "voicy")]
#[command(about = "Ultra-fast voice-to-text daemon and client for Wayland/Hyprland in pure Rust", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Start the background daemon
    Daemon {
        /// Optional path to whisper model directory
        #[arg(short, long)]
        model_dir: Option<PathBuf>,
    },
    /// Push-to-talk: start recording (safe & idempotent, ignores repeats)
    Start,
    /// Push-to-talk: stop recording, transcribe, and type out text
    Stop,
    /// Toggle recording: starts if idle, stops and types if recording
    Toggle,
    /// Cancel current recording without transcribing
    Cancel,
    /// Query daemon status
    Status,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();
    let socket_path = config::get_socket_path();

    match cli.command {
        Commands::Daemon { model_dir } => {
            run_daemon(model_dir, socket_path)?;
        }
        Commands::Start => {
            let res = ipc::send_command(&socket_path, "START")?;
            if !res.trim().is_empty() && res.trim() != "recording" && res.trim() != "already_recording" {
                println!("{}", res.trim());
            }
        }
        Commands::Stop => {
            let res = ipc::send_command(&socket_path, "STOP")?;
            if !res.trim().is_empty() && res.trim() != "queued" && res.trim() != "idle" {
                println!("{}", res.trim());
            }
        }
        Commands::Toggle => {
            let res = ipc::send_command(&socket_path, "TOGGLE")?;
            if !res.trim().is_empty() {
                println!("{}", res.trim());
            }
        }
        Commands::Cancel => {
            let res = ipc::send_command(&socket_path, "CANCEL")?;
            if !res.trim().is_empty() {
                println!("{}", res.trim());
            }
        }
        Commands::Status => {
            let res = ipc::send_command(&socket_path, "STATUS")?;
            println!("{}", res.trim());
        }
    }

    Ok(())
}

fn start_worker_thread(
    model_dir: PathBuf,
    rx: Receiver<Vec<f32>>,
) -> std::thread::JoinHandle<()> {
    std::thread::spawn(move || {
        println!("[voicy] [worker] Loading Whisper INT8 model from {}...", model_dir.display());
        let mut whisper_engine = match whisper::WhisperEngine::new(&model_dir) {
            Ok(engine) => {
                println!("[voicy] [worker] Whisper model loaded successfully!");
                engine
            }
            Err(e) => {
                eprintln!("[voicy] [worker] Failed to initialize Whisper model: {}", e);
                return;
            }
        };

        while let Ok(samples) = rx.recv() {
            // Safety: ignore accidental micro-taps under 0.25 seconds (< 4000 samples at 16kHz)
            if samples.len() < 4000 {
                println!(
                    "[voicy] [worker] Audio snippet too short ({:.2}s), ignoring tap.",
                    samples.len() as f32 / 16000.0
                );
                continue;
            }

            // Safety: calculate RMS energy to discard complete silence
            let sum_sq: f32 = samples.iter().map(|&s| s * s).sum();
            let rms = (sum_sq / samples.len() as f32).sqrt();
            if rms < 0.003 {
                println!(
                    "[voicy] [worker] Audio below silence threshold (RMS: {:.5}), skipping.",
                    rms
                );
                continue;
            }

            println!(
                "[voicy] [worker] Transcribing {} audio samples ({:.2}s)...",
                samples.len(),
                samples.len() as f32 / 16000.0
            );

            match whisper_engine.transcribe(&samples) {
                Ok(text) => {
                    let trimmed = text.trim();
                    // Filter common Whisper silence hallucinations
                    if trimmed.is_empty()
                        || trimmed.eq_ignore_ascii_case("you")
                        || trimmed.eq_ignore_ascii_case("you.")
                        || trimmed.eq_ignore_ascii_case("thank you.")
                        || trimmed.eq_ignore_ascii_case("[blank_audio]")
                    {
                        println!("[voicy] [worker] Discarded hallucination token: \"{}\"", trimmed);
                        continue;
                    }

                    println!("[voicy] [worker] Result: \"{}\"", trimmed);
                    output::deliver_text(trimmed);

                    std::thread::sleep(Duration::from_millis(150));
                }
                Err(e) => eprintln!("[voicy] [worker] Transcription error: {}", e),
            }
        }
        println!("[voicy] [worker] Inference queue channel closed. Worker exiting.");
    })
}

fn run_daemon(
    custom_model_dir: Option<PathBuf>,
    socket_path: PathBuf,
) -> Result<(), Box<dyn std::error::Error>> {
    println!("[voicy] Starting daemon with asynchronous voice queue...");

    let model_dir = custom_model_dir.unwrap_or_else(|| PathBuf::from(config::MODEL_DEFAULT_DIR));
    let (tx, rx): (Sender<Vec<f32>>, Receiver<Vec<f32>>) = mpsc::channel();

    let _worker_handle = start_worker_thread(model_dir, rx);

    println!("[voicy] Initializing audio recorder...");
    let mut recorder = audio::AudioRecorder::new()?;

    let listener = ipc::connect_or_clean(&socket_path)?;
    println!("[voicy] Listening on Unix socket: {}", socket_path.display());

    let sock_for_cleanup = socket_path.clone();
    let running = Arc::new(AtomicBool::new(true));
    let r = running.clone();

    ctrlc::set_handler(move || {
        println!("\n[voicy] Received termination signal. Cleaning up...");
        let _ = std::fs::remove_file(&sock_for_cleanup);
        r.store(false, Ordering::SeqCst);
        std::process::exit(0);
    })?;

    listener.set_nonblocking(false)?;

    for stream in listener.incoming() {
        if !running.load(Ordering::SeqCst) {
            break;
        }

        let mut stream = match stream {
            Ok(s) => s,
            Err(e) => {
                eprintln!("[voicy] Incoming connection error: {}", e);
                continue;
            }
        };

        let mut reader = BufReader::new(&stream);
        let mut line = String::new();
        if reader.read_line(&mut line).is_err() {
            continue;
        }

        let command = line.trim();
        match command {
            "START" => {
                if recorder.is_recording() {
                    let _ = stream.write_all(b"already_recording\n");
                } else {
                    println!("[voicy] Start recording (PTT)...");
                    match recorder.start() {
                        Ok(_) => {
                            let _ = stream.write_all(b"recording\n");
                        }
                        Err(e) => {
                            eprintln!("[voicy] Failed to start recording: {}", e);
                            let _ = stream.write_all(format!("error: {}\n", e).as_bytes());
                        }
                    }
                }
                let _ = stream.flush();
            }

            "STOP" => {
                if recorder.is_recording() {
                    println!("[voicy] Stop recording (PTT). Enqueueing audio buffer...");
                    let samples = recorder.stop();
                    let _ = tx.send(samples);
                    let _ = stream.write_all(b"queued\n");
                    let _ = stream.flush();
                } else {
                    let _ = stream.write_all(b"idle\n");
                    let _ = stream.flush();
                }
            }

            "TOGGLE" => {
                if recorder.is_recording() {
                    println!("[voicy] Stop recording requested (toggle). Enqueueing audio buffer...");
                    let samples = recorder.stop();
                    let _ = tx.send(samples);
                    let _ = stream.write_all(b"queued\n");
                    let _ = stream.flush();
                } else {
                    println!("[voicy] Start recording requested (toggle).");
                    match recorder.start() {
                        Ok(_) => {
                            let _ = stream.write_all(b"recording\n");
                        }
                        Err(e) => {
                            eprintln!("[voicy] Failed to start recording: {}", e);
                            let _ = stream.write_all(format!("error: {}\n", e).as_bytes());
                        }
                    }
                    let _ = stream.flush();
                }
            }

            "CANCEL" => {
                if recorder.is_recording() {
                    recorder.cancel();
                    println!("[voicy] Recording canceled and discarded.");
                    let _ = stream.write_all(b"canceled\n");
                } else {
                    let _ = stream.write_all(b"idle\n");
                }
                let _ = stream.flush();
            }

            "STATUS" => {
                let state = if recorder.is_recording() {
                    "recording\n"
                } else {
                    "idle\n"
                };
                let _ = stream.write_all(state.as_bytes());
                let _ = stream.flush();
            }

            other => {
                eprintln!("[voicy] Unknown command received: {}", other);
                let _ = stream.write_all(b"unknown command\n");
                let _ = stream.flush();
            }
        }
    }

    let _ = std::fs::remove_file(&socket_path);
    Ok(())
}
