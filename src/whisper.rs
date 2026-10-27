use sherpa_rs::whisper::{WhisperConfig, WhisperRecognizer};
use std::path::Path;

pub struct WhisperEngine {
    recognizer: WhisperRecognizer,
}

impl WhisperEngine {
    pub fn new<P: AsRef<Path>>(model_dir: P) -> Result<Self, Box<dyn std::error::Error>> {
        let dir = model_dir.as_ref();
        let encoder = dir.join("small.en-encoder.int8.onnx");
        let decoder = dir.join("small.en-decoder.int8.onnx");
        let tokens = dir.join("small.en-tokens.txt");

        if !encoder.exists() || !decoder.exists() || !tokens.exists() {
            return Err(format!(
                "Whisper model files missing in {}",
                dir.display()
            )
            .into());
        }

        let config = WhisperConfig {
            encoder: encoder.to_string_lossy().to_string(),
            decoder: decoder.to_string_lossy().to_string(),
            tokens: tokens.to_string_lossy().to_string(),
            language: "en".to_string(),
            bpe_vocab: None,
            tail_paddings: Some(0),
            provider: None,
            num_threads: Some(4),
            debug: false,
        };

        let recognizer = WhisperRecognizer::new(config)?;
        Ok(Self { recognizer })
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
