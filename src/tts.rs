use ptts::checkpoint::read_config;
use ptts::preprocess::Lang;
use ptts::synth::{SpeechOptions, Synth};
use rodio::{OutputStream, Sink};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use crate::sink::{pause_media, resume_media};

struct SynthesisJob {
    id: u64,
    text: String,
    voice: Option<String>,
    volume: Option<f32>,
}

struct PlaybackJob {
    id: u64,
    samples: Vec<f32>,
    sample_rate: u32,
    volume: Option<f32>,
}

pub struct TtsManager {
    _synth: Arc<Mutex<Synth>>,
    default_voice: Arc<Mutex<String>>,
    _voices_dir: PathBuf,
    current_sink: Arc<Mutex<Option<Arc<Sink>>>>,
    is_speaking: Arc<AtomicBool>,
    job_counter: Arc<AtomicU64>,
    cancelled_before_id: Arc<AtomicU64>,
    synth_tx: Sender<SynthesisJob>,
}

impl TtsManager {
    pub fn new(model_dir: &Path) -> Result<Self, Box<dyn std::error::Error>> {
        let config_path = model_dir.join("config.json");
        let model_path = model_dir.join("model.q8.gguf");
        let tokenizer_path = model_dir.join("tokenizer.json");
        let default_voice_path = model_dir.join("default-voice.safetensors");
        let voices_dir = model_dir.join("voices");

        let cfg = read_config(&config_path)?;
        let mut builder = Synth::builder(cfg, &model_path, Lang::En)
            .tokenizer_file(&tokenizer_path);

        if default_voice_path.exists() {
            builder = builder.add_voice("default", &default_voice_path);
        }

        // Pre-register all available voices
        if voices_dir.exists() {
            if let Ok(entries) = fs::read_dir(&voices_dir) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.extension().and_then(|s| s.to_str()) == Some("safetensors") {
                        if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                            builder = builder.add_voice(stem, &path);
                        }
                    }
                }
            }
        }

        let synth = builder.build()?;
        let initial_voice = if default_voice_path.exists() {
            "default".to_string()
        } else {
            "Alfie".to_string()
        };

        let synth_arc = Arc::new(Mutex::new(synth));
        let default_voice_arc = Arc::new(Mutex::new(initial_voice));
        let current_sink_arc = Arc::new(Mutex::new(None));
        let is_speaking_arc = Arc::new(AtomicBool::new(false));
        let job_counter = Arc::new(AtomicU64::new(0));
        let cancelled_before_id = Arc::new(AtomicU64::new(0));

        // Pipeline channels:
        // Client -> synth_tx -> [Synthesis Thread] -> playback_tx -> [Playback Thread]
        let (synth_tx, synth_rx): (Sender<SynthesisJob>, Receiver<SynthesisJob>) = mpsc::channel();
        let (playback_tx, playback_rx): (Sender<PlaybackJob>, Receiver<PlaybackJob>) = mpsc::channel();

        // 1. Pipeline Stage 1: Synthesis Thread (Synthesizes ahead into playback channel)
        let s_clone = Arc::clone(&synth_arc);
        let dv_clone = Arc::clone(&default_voice_arc);
        let cancel_synth_clone = Arc::clone(&cancelled_before_id);

        std::thread::spawn(move || {
            while let Ok(job) = synth_rx.recv() {
                // Check if cancelled before starting synthesis
                if job.id <= cancel_synth_clone.load(Ordering::SeqCst) {
                    println!("[voicy] [tts] Skipping cancelled job #{} before synthesis.", job.id);
                    continue;
                }

                let cleaned = strip_markdown(&job.text);
                if cleaned.is_empty() {
                    continue;
                }

                let chosen_voice = match job.voice {
                    Some(ref v) if !v.trim().is_empty() => v.trim().to_string(),
                    _ => dv_clone.lock().unwrap().clone(),
                };

                let opts = SpeechOptions {
                    voice: Some(chosen_voice),
                    temperature: None,
                    seed: None,
                    cfg_coef: None,
                    max_tokens_per_chunk: None,
                    post_process: None,
                };

                println!("[voicy] [tts] [pipeline] Synthesizing speech #{} ahead: \"{}\"...", job.id, job.text);

                let synth_res: Result<(Vec<f32>, u32), String> = {
                    match s_clone.lock() {
                        Ok(synth) => {
                            let sr = synth.sample_rate();
                            synth.say_with(&cleaned, &opts)
                                .map(|pcm| (pcm, sr))
                                .map_err(|e| e.to_string())
                        }
                        Err(e) => Err(e.to_string()),
                    }
                };

                // Check again if cancelled while synthesizing
                if job.id <= cancel_synth_clone.load(Ordering::SeqCst) {
                    println!("[voicy] [tts] Discarding synthesized job #{} due to cancellation.", job.id);
                    continue;
                }

                let (mut samples, sample_rate) = match synth_res {
                    Ok((s, sr)) => (s, sr),
                    Err(e) => {
                        eprintln!("[voicy] [tts] Synthesis failed for #{}: {}", job.id, e);
                        continue;
                    }
                };

                if samples.is_empty() {
                    continue;
                }

                // Prepend 350ms of silence padding to prevent ALSA / DAC wake-up clipping
                let silence_samples = (sample_rate as f32 * 0.35) as usize;
                let mut padded_samples = Vec::with_capacity(silence_samples + samples.len());
                padded_samples.resize(silence_samples, 0.0f32);
                padded_samples.append(&mut samples);

                let _ = playback_tx.send(PlaybackJob {
                    id: job.id,
                    samples: padded_samples,
                    sample_rate,
                    volume: job.volume,
                });
            }
        });

        // 2. Pipeline Stage 2: Playback Thread (Plays audio in foreground while stage 1 prepares next)
        let cs_clone = Arc::clone(&current_sink_arc);
        let isp_clone = Arc::clone(&is_speaking_arc);
        let cancel_play_clone = Arc::clone(&cancelled_before_id);

        std::thread::spawn(move || {
            let (_stream, stream_handle) = match OutputStream::try_default() {
                Ok(res) => res,
                Err(e) => {
                    eprintln!("[voicy] [tts] Fatal: Could not open default audio output stream: {}", e);
                    return;
                }
            };

            let mut media_is_paused = false;

            while let Ok(job) = playback_rx.recv() {
                if job.id <= cancel_play_clone.load(Ordering::SeqCst) {
                    println!("[voicy] [tts] Discarding queued playback for job #{}.", job.id);
                    if media_is_paused && playback_rx.try_recv().is_err() {
                        resume_media();
                        media_is_paused = false;
                    }
                    continue;
                }

                if !media_is_paused {
                    pause_media();
                    media_is_paused = true;
                }

                isp_clone.store(true, Ordering::SeqCst);
                println!(
                    "[voicy] [tts] Playing job #{} ({} samples, {}Hz)...",
                    job.id,
                    job.samples.len(),
                    job.sample_rate
                );

                let sink = match Sink::try_new(&stream_handle) {
                    Ok(s) => Arc::new(s),
                    Err(e) => {
                        eprintln!("[voicy] [tts] Could not create rodio Sink: {}", e);
                        isp_clone.store(false, Ordering::SeqCst);
                        if media_is_paused && playback_rx.try_recv().is_err() {
                            resume_media();
                            media_is_paused = false;
                        }
                        continue;
                    }
                };

                if let Some(vol) = job.volume {
                    sink.set_volume(vol);
                }

                {
                    let mut guard = cs_clone.lock().unwrap();
                    *guard = Some(Arc::clone(&sink));
                }

                let source = rodio::buffer::SamplesBuffer::new(1, job.sample_rate, job.samples);
                sink.append(source);
                sink.sleep_until_end();

                {
                    let mut guard = cs_clone.lock().unwrap();
                    *guard = None;
                }
                isp_clone.store(false, Ordering::SeqCst);
                println!("[voicy] [tts] Finished playback for job #{}.", job.id);

                // If playback queue is drained, resume media playback
                if playback_rx.try_recv().is_err() {
                    if media_is_paused {
                        resume_media();
                        media_is_paused = false;
                    }
                }
            }

            if media_is_paused {
                resume_media();
            }
        });

        Ok(Self {
            _synth: synth_arc,
            default_voice: default_voice_arc,
            _voices_dir: voices_dir,
            current_sink: current_sink_arc,
            is_speaking: is_speaking_arc,
            job_counter,
            cancelled_before_id,
            synth_tx,
        })
    }

    pub fn set_default_voice(&self, voice: &str) {
        if let Ok(mut v) = self.default_voice.lock() {
            *v = voice.to_string();
        }
    }

    pub fn enqueue_speech(&self, text: String, voice: Option<String>, volume: Option<f32>) -> u64 {
        let id = self.job_counter.fetch_add(1, Ordering::SeqCst) + 1;
        let _ = self.synth_tx.send(SynthesisJob {
            id,
            text,
            voice,
            volume,
        });
        id
    }

    /// Stop ongoing playback immediately, without clearing the queued speech jobs behind it
    pub fn stop_current(&self) {
        let mut guard = self.current_sink.lock().unwrap();
        if let Some(sink) = guard.take() {
            sink.stop();
        }
        self.is_speaking.store(false, Ordering::SeqCst);
    }

    /// Stop ongoing playback AND cancel/clear all pending queued speech jobs
    pub fn clear_and_stop(&self) {
        let latest_id = self.job_counter.load(Ordering::SeqCst);
        self.cancelled_before_id.store(latest_id, Ordering::SeqCst);

        let mut guard = self.current_sink.lock().unwrap();
        if let Some(sink) = guard.take() {
            sink.stop();
        }
        self.is_speaking.store(false, Ordering::SeqCst);
        resume_media();
        println!("[voicy] [tts] Cancelled and cleared all TTS jobs up to #{}.", latest_id);
    }

    pub fn is_speaking(&self) -> bool {
        self.is_speaking.load(Ordering::SeqCst)
    }
}

pub fn strip_markdown(text: &str) -> String {
    let mut out = String::new();
    let mut in_code_block = false;

    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("```") {
            in_code_block = !in_code_block;
            continue;
        }
        if in_code_block {
            continue;
        }
        // Skip headers, blockquotes, bullets
        let mut clean_line = trimmed;
        while clean_line.starts_with('#') {
            clean_line = clean_line.trim_start_matches('#').trim_start();
        }
        if clean_line.starts_with('>') {
            clean_line = clean_line.trim_start_matches('>').trim_start();
        }
        if clean_line.starts_with("- ") || clean_line.starts_with("* ") || clean_line.starts_with("+ ") {
            clean_line = &clean_line[2..];
        }

        // Filter out formatting markers: ` * _ ~
        let mut line_buf = String::new();
        for c in clean_line.chars() {
            match c {
                '`' | '*' | '_' | '~' => continue,
                _ => line_buf.push(c),
            }
        }

        if !line_buf.trim().is_empty() {
            if !out.is_empty() {
                out.push(' ');
            }
            out.push_str(line_buf.trim());
        }
    }

    let final_text = out.trim().to_string();
    if !final_text.is_empty() && !final_text.ends_with(&['.', '!', '?']) {
        format!("{}.", final_text)
    } else {
        final_text
    }
}
