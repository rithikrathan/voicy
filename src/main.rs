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
    /// Stream live JSON status updates for ashell / waybar widgets
    Watch,
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
            if !res.trim().is_empty()
                && res.trim() != "recording"
                && res.trim() != "already_recording"
            {
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
        Commands::Watch => {
            run_watch(&socket_path)?;
        }
    }

    Ok(())
}

fn run_watch(socket_path: &std::path::Path) -> Result<(), Box<dyn std::error::Error>> {
    let mut last_state = String::new();

    loop {
        let current_state = match ipc::send_command(socket_path, "STATUS") {
            Ok(s) => s.trim().to_string(),
            Err(_) => "offline".to_string(),
        };

        if current_state != last_state {
            match current_state.as_str() {
                "recording" => {
                    println!("{{\"text\": \" Listening...\", \"alt\": \"recording\", \"class\": \"recording\"}}");
                }
                "transcribing" => {
                    println!("{{\"text\": \" Transcribing...\", \"alt\": \"transcribing\", \"class\": \"transcribing\"}}");
                }
                "typing" => {
                    println!(
                        "{{\"text\": \" Typing...\", \"alt\": \"typing\", \"class\": \"typing\"}}"
                    );
                }
                _ => {
                    println!("{{\"text\": \"\", \"alt\": \"idle\", \"class\": \"idle\"}}");
                }
            }
            let _ = std::io::stdout().flush();
            last_state = current_state;
        }

        std::thread::sleep(Duration::from_millis(100));
    }
}

fn start_worker_thread(
    model_dir: PathBuf,
    rx: Receiver<Vec<f32>>,
    is_transcribing: Arc<AtomicBool>,
    is_typing: Arc<AtomicBool>,
) -> std::thread::JoinHandle<()> {
    std::thread::spawn(move || {
        println!(
            "[voicy] [worker] Loading Whisper INT8 model from {}...",
            model_dir.display()
        );
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
            if samples.len() < 4000 {
                continue;
            }

            let sum_sq: f32 = samples.iter().map(|&s| s * s).sum();
            let rms = (sum_sq / samples.len() as f32).sqrt();
            if rms < 0.003 {
                continue;
            }

            is_transcribing.store(true, Ordering::SeqCst);
            println!(
                "[voicy] [worker] Transcribing {} audio samples ({:.2}s)...",
                samples.len(),
                samples.len() as f32 / 16000.0
            );

            match whisper_engine.transcribe(&samples) {
                Ok(text) => {
                    is_transcribing.store(false, Ordering::SeqCst);
                    let trimmed = text.trim();
                    if trimmed.is_empty()
                        || trimmed.eq_ignore_ascii_case("you")
                        || trimmed.eq_ignore_ascii_case("you.")
                        || trimmed.eq_ignore_ascii_case("thank you.")
                        || trimmed.eq_ignore_ascii_case("[blank_audio]")
                    {
                        println!(
                            "[voicy] [worker] Discarded hallucination token: \"{}\"",
                            trimmed
                        );
                    } else {
                        println!("[voicy] [worker] Result: \"{}\"", trimmed);
                        is_typing.store(true, Ordering::SeqCst);
                        output::deliver_text(trimmed);
                        is_typing.store(false, Ordering::SeqCst);
                    }
                }
                Err(e) => {
                    is_transcribing.store(false, Ordering::SeqCst);
                    eprintln!("[voicy] [worker] Transcription error: {}", e);
                }
            }

            std::thread::sleep(Duration::from_millis(150));
        }
    })
}

fn run_daemon(
    custom_model_dir: Option<PathBuf>,
    socket_path: PathBuf,
) -> Result<(), Box<dyn std::error::Error>> {
    println!("[voicy] Starting daemon with asynchronous voice queue...");

    let model_dir = custom_model_dir.unwrap_or_else(|| PathBuf::from(config::MODEL_DEFAULT_DIR));
    let (tx, rx): (Sender<Vec<f32>>, Receiver<Vec<f32>>) = mpsc::channel();
    let is_transcribing = Arc::new(AtomicBool::new(false));
    let is_typing = Arc::new(AtomicBool::new(false));

    let _worker_handle = start_worker_thread(
        model_dir,
        rx,
        Arc::clone(&is_transcribing),
        Arc::clone(&is_typing),
    );

    println!("[voicy] Initializing audio recorder...");
    let mut recorder = audio::AudioRecorder::new()?;

    let listener = ipc::connect_or_clean(&socket_path)?;
    println!(
        "[voicy] Listening on Unix socket: {}",
        socket_path.display()
    );

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
                    println!(
                        "[voicy] Stop recording requested (toggle). Enqueueing audio buffer..."
                    );
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
                } else if is_transcribing.load(Ordering::SeqCst) {
                    "transcribing\n"
                } else if is_typing.load(Ordering::SeqCst) {
                    "typing\n"
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
