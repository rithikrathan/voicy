#![allow(dead_code)]

use crate::config::ModelProfile;
use sherpa_rs::whisper::{WhisperConfig, WhisperRecognizer};
use std::fs;
use std::path::{Path, PathBuf};

pub struct WhisperEngine {
    recognizer: WhisperRecognizer,
}

impl WhisperEngine {
    pub fn from_profile(profile: &ModelProfile) -> Result<Self, Box<dyn std::error::Error>> {
        let dir = &profile.path;
        if !dir.exists() {
            return Err(format!("Model directory does not exist: {}", dir.display()).into());
        }

        let encoder = if let Some(ref enc) = profile.encoder {
            dir.join(enc)
        } else {
            find_model_file(dir, "encoder")
                .ok_or_else(|| format!("Could not find encoder ONNX in {}", dir.display()))?
        };

        let decoder = if let Some(ref dec) = profile.decoder {
            dir.join(dec)
        } else {
            find_model_file(dir, "decoder")
                .ok_or_else(|| format!("Could not find decoder ONNX in {}", dir.display()))?
        };

        let tokens = if let Some(ref tok) = profile.tokens {
            dir.join(tok)
        } else {
            find_tokens_file(dir)
                .ok_or_else(|| format!("Could not find tokens.txt in {}", dir.display()))?
        };

        if !encoder.exists() || !decoder.exists() || !tokens.exists() {
            return Err(format!(
                "Whisper model files missing in {}:\n  encoder: {}\n  decoder: {}\n  tokens: {}",
                dir.display(),
                encoder.display(),
                decoder.display(),
                tokens.display()
            )
            .into());
        }

        println!("[voicy] Initializing Whisper engine with:");
        println!("  Encoder: {}", encoder.display());
        println!("  Decoder: {}", decoder.display());
        println!("  Tokens:  {}", tokens.display());

        let config = WhisperConfig {
            encoder: encoder.to_string_lossy().to_string(),
            decoder: decoder.to_string_lossy().to_string(),
            tokens: tokens.to_string_lossy().to_string(),
            language: profile.language.clone(),
            bpe_vocab: None,
            tail_paddings: Some(0),
            provider: None,
            num_threads: Some(profile.num_threads as i32),
            debug: false,
        };

        let recognizer = WhisperRecognizer::new(config)?;
        Ok(Self { recognizer })
    }

    pub fn new<P: AsRef<Path>>(model_dir: P) -> Result<Self, Box<dyn std::error::Error>> {
        let profile = ModelProfile {
            path: model_dir.as_ref().to_path_buf(),
            encoder: None,
            decoder: None,
            tokens: None,
            language: "en".to_string(),
            num_threads: 4,
        };
        Self::from_profile(&profile)
    }

    pub fn transcribe(&mut self, samples: &[f32]) -> Result<String, Box<dyn std::error::Error>> {
        if samples.is_empty() {
            return Ok(String::new());
        }
        // 16kHz audio samples
        let result = self.recognizer.transcribe(16000, samples);
        Ok(result.text.trim().to_string())
    }
}

fn find_model_file(dir: &Path, kind: &str) -> Option<PathBuf> {
    let entries = fs::read_dir(dir).ok()?;
    let mut best_int8 = None;
    let mut fallback = None;

    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_lowercase();
        if name.ends_with(".onnx") && name.contains(kind) {
            if name.contains("int8") {
                best_int8 = Some(entry.path());
            } else if fallback.is_none() {
                fallback = Some(entry.path());
            }
        }
    }

    best_int8.or(fallback)
}

fn find_tokens_file(dir: &Path) -> Option<PathBuf> {
    let entries = fs::read_dir(dir).ok()?;
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_lowercase();
        if name.ends_with(".txt") && (name.contains("tokens") || name.contains("vocab")) {
            return Some(entry.path());
        }
    }
    None
}
