use ptts::checkpoint::read_config;
use ptts::preprocess::Lang;
use ptts::synth::{SpeechOptions, Synth};
use rodio::{OutputStream, Sink};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

pub struct TtsManager {
    synth: Arc<Mutex<Synth>>,
    default_voice: Arc<Mutex<String>>,
    _voices_dir: PathBuf,
    current_sink: Arc<Mutex<Option<Arc<Sink>>>>,
    is_speaking: Arc<AtomicBool>,
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

        Ok(Self {
            synth: Arc::new(Mutex::new(synth)),
            default_voice: Arc::new(Mutex::new(initial_voice)),
            _voices_dir: voices_dir,
            current_sink: Arc::new(Mutex::new(None)),
            is_speaking: Arc::new(AtomicBool::new(false)),
        })
    }

    pub fn set_default_voice(&self, voice: &str) {
        if let Ok(mut v) = self.default_voice.lock() {
            *v = voice.to_string();
        }
    }

    pub fn synthesize(&self, text: &str, voice: Option<&str>) -> Result<(Vec<f32>, u32), Box<dyn std::error::Error>> {
        let cleaned = strip_markdown(text);
        if cleaned.is_empty() {
            return Ok((Vec::new(), 24000));
        }

        let chosen_voice = match voice {
            Some(v) if !v.trim().is_empty() => v.trim().to_string(),
            _ => self.default_voice.lock().unwrap().clone(),
        };

        let opts = SpeechOptions {
            voice: Some(chosen_voice),
            temperature: None,
            seed: None,
            cfg_coef: None,
            max_tokens_per_chunk: None,
            post_process: None,
        };

        let synth = self.synth.lock().map_err(|e| e.to_string())?;
        let pcm = synth.say_with(&cleaned, &opts)?;
        let sample_rate = synth.sample_rate();
        Ok((pcm, sample_rate))
    }

    pub fn speak_async(
        self: &Arc<Self>,
        text: String,
        voice: Option<String>,
        volume: Option<f32>,
    ) {
        self.stop();

        let this = Arc::clone(self);
        std::thread::spawn(move || {
            this.is_speaking.store(true, Ordering::SeqCst);
            println!("[voicy] [tts] Synthesizing speech: \"{}\"...", text);
            let synth_res = this.synthesize(&text, voice.as_deref());

            let (samples, sample_rate) = match synth_res {
                Ok((s, sr)) => (s, sr),
                Err(e) => {
                    eprintln!("[voicy] [tts] Synthesis failed: {}", e);
                    this.is_speaking.store(false, Ordering::SeqCst);
                    return;
                }
            };

            if samples.is_empty() {
                println!("[voicy] [tts] Synthesized samples were empty.");
                this.is_speaking.store(false, Ordering::SeqCst);
                return;
            }

            println!(
                "[voicy] [tts] Synthesis complete ({} samples, {}Hz). Playing audio...",
                samples.len(),
                sample_rate
            );

            // Output via rodio
            let (_stream, stream_handle) = match OutputStream::try_default() {
                Ok(res) => res,
                Err(e) => {
                    eprintln!("[voicy] [tts] Could not open default audio output stream: {}", e);
                    this.is_speaking.store(false, Ordering::SeqCst);
                    return;
                }
            };

            let sink = match Sink::try_new(&stream_handle) {
                Ok(s) => Arc::new(s),
                Err(e) => {
                    eprintln!("[voicy] [tts] Could not create rodio Sink: {}", e);
                    this.is_speaking.store(false, Ordering::SeqCst);
                    return;
                }
            };

            if let Some(vol) = volume {
                sink.set_volume(vol);
            }

            // Store active sink so stop() can cancel playback immediately
            {
                let mut guard = this.current_sink.lock().unwrap();
                *guard = Some(Arc::clone(&sink));
            }

            let source = rodio::buffer::SamplesBuffer::new(1, sample_rate, samples);
            sink.append(source);
            sink.sleep_until_end();

            {
                let mut guard = this.current_sink.lock().unwrap();
                *guard = None;
            }
            this.is_speaking.store(false, Ordering::SeqCst);
            println!("[voicy] [tts] Playback finished.");
        });
    }

    pub fn stop(&self) {
        let mut guard = self.current_sink.lock().unwrap();
        if let Some(sink) = guard.take() {
            sink.stop();
        }
        self.is_speaking.store(false, Ordering::SeqCst);
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
