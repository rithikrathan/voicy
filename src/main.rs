mod audio;
mod config;
mod ipc;
mod output;
mod whisper;

use clap::{Parser, Subcommand};
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

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

fn run_daemon(
    custom_model_dir: Option<PathBuf>,
    socket_path: PathBuf,
) -> Result<(), Box<dyn std::error::Error>> {
    println!("[voicy] Starting daemon...");

    let model_dir = custom_model_dir.unwrap_or_else(|| PathBuf::from(config::MODEL_DEFAULT_DIR));
    println!("[voicy] Loading Whisper INT8 model from {}...", model_dir.display());
    let mut whisper_engine = whisper::WhisperEngine::new(&model_dir)?;
    println!("[voicy] Whisper model loaded successfully!");

    println!("[voicy] Initializing audio recorder...");
    let mut recorder = audio::AudioRecorder::new()?;

    let listener = ipc::connect_or_clean(&socket_path)?;
    println!("[voicy] Listening on Unix socket: {}", socket_path.display());

    // Clean socket on SIGINT / SIGTERM
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
            "TOGGLE" => {
                if recorder.is_recording() {
                    println!("[voicy] Stop recording requested. Extracting audio...");
                    let samples = recorder.stop();
                    let _ = stream.write_all(b"transcribing\n");
                    let _ = stream.flush();

                    if !samples.is_empty() {
                        println!("[voicy] Transcribing {} audio samples...", samples.len());
                        match whisper_engine.transcribe(&samples) {
                            Ok(text) => {
                                println!("[voicy] Result: \"{}\"", text);
                                if !text.is_empty() {
                                    output::deliver_text(&text);
                                }
                            }
                            Err(e) => eprintln!("[voicy] Transcription error: {}", e),
                        }
                    } else {
                        println!("[voicy] Audio buffer was empty.");
                    }
                } else {
                    println!("[voicy] Start recording requested.");
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
