mod audio;
mod audio_preprocessor;
mod broadcaster;
mod config;
mod ipc;
mod output;
mod sink;
mod sound;
mod tmux;
mod whisper;

use clap::{Parser, Subcommand};
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::Duration;

#[derive(Parser)]
#[command(name = "voicy")]
#[command(author = "Rithik")]
#[command(version = "0.2.0")]
#[command(
    about = "Ultra-fast voice-to-text daemon, dynamic tmux router, and dictation engine in pure Rust",
    long_about = None
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Start the background daemon
    Daemon {
        /// Optional path to whisper model directory (overrides config.toml)
        #[arg(short, long)]
        model_dir: Option<PathBuf>,
    },
    /// Push-to-talk: start recording (safe & idempotent, ignores repeats)
    Start {
        /// Pure desktop dictation mode (types into active Wayland window)
        #[arg(long)]
        dictate: bool,
    },
    /// Push-to-talk: stop recording, transcribe, and deliver text
    Stop,
    /// Toggle recording: starts if idle, stops and delivers if recording
    Toggle {
        /// Pure desktop dictation mode (types into active Wayland window)
        #[arg(long)]
        dictate: bool,
    },
    /// Cancel the topmost active task (LIFO: active recording first, or in-flight transcription)
    Cancel,
    /// Inspect or set the active tmux target pane ('here', 'clear', 'null', or '<target>')
    Target {
        /// Target destination e.g. 'here', 'clear', 'null', 'none', or 'session:window.pane'
        destination: Option<String>,
    },
    /// Query daemon status
    Status,
    /// Stream live JSON status updates for ashell / waybar widgets
    Watch,
}

struct AudioTask {
    samples: Vec<f32>,
    is_dictate: bool,
    task_id: u64,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();
    let socket_path = config::get_socket_path();

    match cli.command {
        Commands::Daemon { model_dir } => {
            run_daemon(model_dir, socket_path)?;
        }
        Commands::Start { dictate } => {
            let cmd = if dictate { "START:DICTATE" } else { "START" };
            let res = ipc::send_command(&socket_path, cmd)?;
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
        Commands::Toggle { dictate } => {
            let cmd = if dictate { "TOGGLE:DICTATE" } else { "TOGGLE" };
            let res = ipc::send_command(&socket_path, cmd)?;
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
        Commands::Target { destination } => {
            if let Some(dest) = destination {
                let target_val = match dest.to_lowercase().as_str() {
                    "here" => {
                        if let Some(pane) = tmux::get_env_tmux_pane() {
                            pane
                        } else if let Some(pane) = tmux::get_active_client_pane() {
                            pane
                        } else {
                            eprintln!("[voicy] Error: Not running inside tmux ($TMUX_PANE not found) and no attached tmux client detected.");
                            return Ok(());
                        }
                    }
                    "clear" | "null" | "none" | "0" | "" => "none".to_string(),
                    other => other.to_string(),
                };

                let res = ipc::send_command(&socket_path, &format!("TARGET:{}", target_val))?;
                println!("{}", res.trim());
            } else {
                let res = ipc::send_command(&socket_path, "TARGET")?;
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
    model_profile: config::ModelProfile,
    rx: Receiver<AudioTask>,
    is_transcribing: Arc<AtomicBool>,
    is_typing: Arc<AtomicBool>,
    canceled_task_id: Arc<AtomicU64>,
    active_target: Arc<Mutex<Option<String>>>,
    broadcaster: broadcaster::EventBroadcaster,
) -> std::thread::JoinHandle<()> {
    std::thread::spawn(move || {
        println!(
            "[voicy] [worker] Loading Whisper model from {}...",
            model_profile.path.display()
        );
        let mut whisper_engine = match whisper::WhisperEngine::from_profile(&model_profile) {
            Ok(engine) => {
                println!("[voicy] [worker] Whisper model loaded successfully!");
                engine
            }
            Err(e) => {
                eprintln!("[voicy] [worker] Failed to initialize Whisper model: {}", e);
                return;
            }
        };

        while let Ok(task) = rx.recv() {
            // LIFO cancellation check before preprocessing
            if task.task_id <= canceled_task_id.load(Ordering::SeqCst) {
                println!(
                    "[voicy] [worker] Task #{} canceled prior to transcription, skipping.",
                    task.task_id
                );
                continue;
            }

            let samples = audio_preprocessor::process_audio(&task.samples);
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
                "[voicy] [worker] Transcribing task #{} ({} samples, {:.2}s)...",
                task.task_id,
                samples.len(),
                samples.len() as f32 / 16000.0
            );

            let transcribe_result = whisper_engine.transcribe(&samples);
            is_transcribing.store(false, Ordering::SeqCst);

            // LIFO cancellation check after transcription before delivery
            if task.task_id <= canceled_task_id.load(Ordering::SeqCst) {
                println!(
                    "[voicy] [worker] Task #{} canceled during transcription, output discarded.",
                    task.task_id
                );
                continue;
            }

            match transcribe_result {
                Ok(text) => {
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

                        // Broadcast to Unix Domain Socket (for home agent & external listeners)
                        broadcaster.broadcast_transcription(trimmed);

                        // Deliver via Multi-channel Router (target or desktop dictation)
                        let target_lock = active_target.lock().ok();
                        let target_str = target_lock.as_deref().and_then(|opt| opt.as_deref());

                        is_typing.store(true, Ordering::SeqCst);
                        output::deliver_text(trimmed, task.is_dictate, target_str);
                        is_typing.store(false, Ordering::SeqCst);
                    }
                }
                Err(e) => {
                    eprintln!("[voicy] [worker] Transcription error: {}", e);
                }
            }

            std::thread::sleep(Duration::from_millis(100));
        }
    })
}

fn run_daemon(
    custom_model_dir: Option<PathBuf>,
    socket_path: PathBuf,
) -> Result<(), Box<dyn std::error::Error>> {
    println!("[voicy] Starting daemon with multi-channel routing & event streaming...");

    // 1. Load Configuration
    let voicy_cfg = config::VoicyConfig::load();
    let (model_name, mut model_profile) = voicy_cfg.get_active_model_profile();
    if let Some(dir) = custom_model_dir {
        println!("[voicy] Overriding model directory with CLI flag: {}", dir.display());
        model_profile.path = dir;
    }
    println!("[voicy] Active model selected: '{}'", model_name);

    // 2. Initialize Broadcaster Socket
    let broadcaster_socket = broadcaster::get_events_socket_path();
    let event_broadcaster = broadcaster::EventBroadcaster::new(&broadcaster_socket);
    if let Err(e) = event_broadcaster.start() {
        eprintln!("[voicy] Warning: Failed to start event broadcaster: {}", e);
    }

    // 3. Worker Channel & State
    let (tx, rx): (Sender<AudioTask>, Receiver<AudioTask>) = mpsc::channel();
    let is_transcribing = Arc::new(AtomicBool::new(false));
    let is_typing = Arc::new(AtomicBool::new(false));
    let task_counter = Arc::new(AtomicU64::new(0));
    let canceled_task_id = Arc::new(AtomicU64::new(0));
    let active_target = Arc::new(Mutex::new(None::<String>));

    let _worker_handle = start_worker_thread(
        model_profile,
        rx,
        Arc::clone(&is_transcribing),
        Arc::clone(&is_typing),
        Arc::clone(&canceled_task_id),
        Arc::clone(&active_target),
        event_broadcaster.clone(),
    );

    println!("[voicy] Initializing audio recorder...");
    let mut recorder = audio::AudioRecorder::new()?;

    let listener = ipc::connect_or_clean(&socket_path)?;
    println!(
        "[voicy] Control socket listening on: {}",
        socket_path.display()
    );

    let sock_for_cleanup = socket_path.clone();
    let running = Arc::new(AtomicBool::new(true));
    let r = running.clone();

    ctrlc::set_handler(move || {
        println!("\n[voicy] Received termination signal. Cleaning up...");
        sink::resume_media();
        let _ = std::fs::remove_file(&sock_for_cleanup);
        r.store(false, Ordering::SeqCst);
        std::process::exit(0);
    })?;

    listener.set_nonblocking(false)?;

    let mut timer_cancel: Option<std::sync::mpsc::Sender<()>> = None;
    let mut active_recording_is_dictate = false;

    let start_recording_timer = |cancel_slot: &mut Option<std::sync::mpsc::Sender<()>>| {
        if let Some(prev) = cancel_slot.take() {
            let _ = prev.send(());
        }
        let (tx, rx) = std::sync::mpsc::channel();
        *cancel_slot = Some(tx);
        std::thread::spawn(move || {
            if let Err(std::sync::mpsc::RecvTimeoutError::Timeout) =
                rx.recv_timeout(std::time::Duration::from_secs(30))
            {
                println!("[voicy] 30-second recording window reached. Playing timeout alert...");
                sound::play_timeout_sound();
            }
        });
    };

    let stop_recording_timer = |cancel_slot: &mut Option<std::sync::mpsc::Sender<()>>| {
        if let Some(prev) = cancel_slot.take() {
            let _ = prev.send(());
        }
    };

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
        let is_dictate_cmd = command.ends_with(":DICTATE");
        let base_cmd = if is_dictate_cmd {
            command.strip_suffix(":DICTATE").unwrap_or(command)
        } else {
            command
        };

        if base_cmd.starts_with("TARGET") {
            let mut parts = base_cmd.splitn(2, ':');
            let _ = parts.next();
            if let Some(new_target) = parts.next() {
                let trimmed = new_target.trim();
                let mut lock = active_target.lock().unwrap();
                if trimmed == "none" || trimmed.is_empty() {
                    *lock = None;
                    println!("[voicy] Active target cleared (null). Defaulting to dictation fallback.");
                    let _ = stream.write_all(b"Target cleared (null). Defaulting to dictation fallback.\n");
                } else {
                    *lock = Some(trimmed.to_string());
                    println!("[voicy] Active target set to: '{}'", trimmed);
                    let _ = stream.write_all(format!("Active target set to: '{}'\n", trimmed).as_bytes());
                }
            } else {
                let lock = active_target.lock().unwrap();
                match lock.as_deref() {
                    Some(t) => {
                        let _ = stream.write_all(format!("Target: {}\n", t).as_bytes());
                    }
                    None => {
                        let _ = stream.write_all(b"Target: null (dictation fallback)\n");
                    }
                }
            }
            let _ = stream.flush();
            continue;
        }

        match base_cmd {
            "START" => {
                if recorder.is_recording() {
                    let _ = stream.write_all(b"already_recording\n");
                } else {
                    active_recording_is_dictate = is_dictate_cmd;
                    println!(
                        "[voicy] Start recording (PTT, dictate={})...",
                        active_recording_is_dictate
                    );
                    sink::pause_media();
                    sound::play_start_sound();
                    match recorder.start() {
                        Ok(_) => {
                            start_recording_timer(&mut timer_cancel);
                            let _ = stream.write_all(b"recording\n");
                        }
                        Err(e) => {
                            sink::resume_media();
                            stop_recording_timer(&mut timer_cancel);
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
                    stop_recording_timer(&mut timer_cancel);
                    let samples = recorder.stop();
                    sink::resume_media();

                    let tid = task_counter.fetch_add(1, Ordering::SeqCst) + 1;
                    let _ = tx.send(AudioTask {
                        samples,
                        is_dictate: active_recording_is_dictate,
                        task_id: tid,
                    });

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
                    stop_recording_timer(&mut timer_cancel);
                    let samples = recorder.stop();
                    sink::resume_media();

                    let tid = task_counter.fetch_add(1, Ordering::SeqCst) + 1;
                    let _ = tx.send(AudioTask {
                        samples,
                        is_dictate: active_recording_is_dictate,
                        task_id: tid,
                    });

                    let _ = stream.write_all(b"queued\n");
                    let _ = stream.flush();
                } else {
                    active_recording_is_dictate = is_dictate_cmd;
                    println!(
                        "[voicy] Start recording requested (toggle, dictate={}).",
                        active_recording_is_dictate
                    );
                    sink::pause_media();
                    sound::play_start_sound();
                    match recorder.start() {
                        Ok(_) => {
                            start_recording_timer(&mut timer_cancel);
                            let _ = stream.write_all(b"recording\n");
                        }
                        Err(e) => {
                            sink::resume_media();
                            stop_recording_timer(&mut timer_cancel);
                            eprintln!("[voicy] Failed to start recording: {}", e);
                            let _ = stream.write_all(format!("error: {}\n", e).as_bytes());
                        }
                    }
                    let _ = stream.flush();
                }
            }

            "CANCEL" => {
                // LIFO Stack Cancellation Model
                if recorder.is_recording() {
                    // Top of stack: Active recording cancelled.
                    // Background worker task (if any) is PRESERVED and stays alive.
                    stop_recording_timer(&mut timer_cancel);
                    recorder.cancel();
                    sink::resume_media();
                    println!("[voicy] [cancel] Top of stack: Active recording canceled and discarded. Background transcription preserved.");
                    let _ = stream.write_all(b"canceled_recording\n");
                } else if is_transcribing.load(Ordering::SeqCst) {
                    // Second layer: No active recording, but worker is transcribing/queued.
                    // Pop and cancel the most recent transcription task.
                    let current_tid = task_counter.load(Ordering::SeqCst);
                    canceled_task_id.store(current_tid, Ordering::SeqCst);
                    println!(
                        "[voicy] [cancel] In-flight transcription task #{} marked canceled and output suppressed.",
                        current_tid
                    );
                    let _ = stream.write_all(b"canceled_transcription\n");
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

    sink::resume_media();
    let _ = std::fs::remove_file(&socket_path);
    let _ = std::fs::remove_file(&broadcaster_socket);
    Ok(())
}
