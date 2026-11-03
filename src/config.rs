#![allow(dead_code)]

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

pub const DEFAULT_SOCKET_NAME: &str = "voicy.sock";
pub const DEFAULT_CONFIG_FILENAME: &str = "config.toml";

pub fn get_default_model_dir(model_name: &str) -> PathBuf {
    // 1. If /mnt/sda4/models/sherpa-onnx/<name> exists, prefer it
    let host_path = PathBuf::from("/mnt/sda4/models/sherpa-onnx").join(model_name);
    if host_path.exists() {
        return host_path;
    }

    // 2. Standard XDG data directory (~/.local/share/voicy/models/<name>)
    if let Ok(data_home) = std::env::var("XDG_DATA_HOME") {
        PathBuf::from(data_home).join("voicy/models").join(model_name)
    } else if let Ok(home) = std::env::var("HOME") {
        PathBuf::from(home).join(".local/share/voicy/models").join(model_name)
    } else {
        PathBuf::from("models").join(model_name)
    }
}

pub fn get_default_log_file() -> Option<PathBuf> {
    let dev_log = PathBuf::from("/mnt/sda4/projects/tin/data/test_input.txt");
    if dev_log.parent().map(|p| p.exists()).unwrap_or(false) {
        Some(dev_log)
    } else {
        None
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelProfile {
    pub path: PathBuf,
    pub encoder: Option<String>,
    pub decoder: Option<String>,
    pub tokens: Option<String>,
    #[serde(default = "default_language")]
    pub language: String,
    #[serde(default = "default_num_threads")]
    pub num_threads: usize,
}

fn default_language() -> String {
    "en".to_string()
}

fn default_num_threads() -> usize {
    4
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AudioSettings {
    #[serde(default = "default_sample_rate")]
    pub sample_rate: u32,
    #[serde(default = "default_silence_threshold")]
    pub silence_threshold: f32,
    #[serde(default = "default_bool_true")]
    pub enable_high_pass: bool,
    #[serde(default = "default_high_pass_cutoff")]
    pub high_pass_cutoff: f32,
    #[serde(default = "default_bool_true")]
    pub enable_peak_norm: bool,
    #[serde(default = "default_peak_target")]
    pub peak_target: f32,
    #[serde(default = "default_timeout_seconds")]
    pub timeout_seconds: u64,
}

fn default_sample_rate() -> u32 {
    16000
}

fn default_silence_threshold() -> f32 {
    0.003
}

fn default_bool_true() -> bool {
    true
}

fn default_high_pass_cutoff() -> f32 {
    80.0
}

fn default_peak_target() -> f32 {
    0.90
}

fn default_timeout_seconds() -> u64 {
    30
}

impl Default for AudioSettings {
    fn default() -> Self {
        Self {
            sample_rate: default_sample_rate(),
            silence_threshold: default_silence_threshold(),
            enable_high_pass: default_bool_true(),
            high_pass_cutoff: default_high_pass_cutoff(),
            enable_peak_norm: default_bool_true(),
            peak_target: default_peak_target(),
            timeout_seconds: default_timeout_seconds(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutputSettings {
    #[serde(default = "default_typing_delay_ms")]
    pub typing_delay_ms: u64,
    #[serde(default = "default_bool_true")]
    pub copy_to_clipboard: bool,
    #[serde(default = "default_bool_true")]
    pub type_to_active_window: bool,
    #[serde(default = "default_bool_true")]
    pub append_to_file: bool,
    /// Path to append transcriptions for dataset collection. Set to "" or None to disable.
    pub log_file: Option<PathBuf>,
}

fn default_typing_delay_ms() -> u64 {
    10
}

impl Default for OutputSettings {
    fn default() -> Self {
        Self {
            typing_delay_ms: default_typing_delay_ms(),
            copy_to_clipboard: default_bool_true(),
            type_to_active_window: default_bool_true(),
            append_to_file: default_bool_true(),
            log_file: get_default_log_file(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SoundSettings {
    #[serde(default = "default_bool_true")]
    pub play_cues: bool,
    #[serde(default = "default_bool_true")]
    pub mute_external_audio: bool,
}

impl Default for SoundSettings {
    fn default() -> Self {
        Self {
            play_cues: default_bool_true(),
            mute_external_audio: default_bool_true(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VoicyConfig {
    #[serde(default = "default_active_model")]
    pub active_model: String,
    #[serde(default)]
    pub models: HashMap<String, ModelProfile>,
    #[serde(default)]
    pub audio: AudioSettings,
    #[serde(default)]
    pub output: OutputSettings,
    #[serde(default)]
    pub sound: SoundSettings,
}

fn default_active_model() -> String {
    "distil-small.en".to_string()
}

impl Default for VoicyConfig {
    fn default() -> Self {
        let mut models = HashMap::new();
        models.insert(
            "distil-small.en".to_string(),
            ModelProfile {
                path: get_default_model_dir("sherpa-onnx-whisper-distil-small.en"),
                encoder: None,
                decoder: None,
                tokens: None,
                language: "en".to_string(),
                num_threads: 4,
            },
        );
        models.insert(
            "small.en".to_string(),
            ModelProfile {
                path: get_default_model_dir("sherpa-onnx-whisper-small.en"),
                encoder: Some("small.en-encoder.int8.onnx".to_string()),
                decoder: Some("small.en-decoder.int8.onnx".to_string()),
                tokens: Some("small.en-tokens.txt".to_string()),
                language: "en".to_string(),
                num_threads: 4,
            },
        );

        Self {
            active_model: default_active_model(),
            models,
            audio: AudioSettings::default(),
            output: OutputSettings::default(),
            sound: SoundSettings::default(),
        }
    }
}

impl VoicyConfig {
    pub fn load() -> Self {
        let config_path = get_config_path();
        if config_path.exists() {
            match fs::read_to_string(&config_path) {
                Ok(content) => match toml::from_str::<VoicyConfig>(&content) {
                    Ok(cfg) => {
                        println!("[voicy] Loaded configuration from {}", config_path.display());
                        return cfg;
                    }
                    Err(e) => {
                        eprintln!(
                            "[voicy] Warning: Failed to parse {}, using defaults: {}",
                            config_path.display(),
                            e
                        );
                    }
                },
                Err(e) => {
                    eprintln!(
                        "[voicy] Warning: Could not read {}, using defaults: {}",
                        config_path.display(),
                        e
                    );
                }
            }
        } else {
            let default_cfg = VoicyConfig::default();
            if let Some(parent) = config_path.parent() {
                let _ = fs::create_dir_all(parent);
            }
            if let Ok(toml_str) = toml::to_string_pretty(&default_cfg) {
                let _ = fs::write(&config_path, toml_str);
                println!("[voicy] Created default configuration at {}", config_path.display());
            }
            return default_cfg;
        }

        VoicyConfig::default()
    }

    pub fn get_active_model_profile(&self) -> (String, ModelProfile) {
        if let Some(profile) = self.models.get(&self.active_model) {
            if profile.path.exists() {
                return (self.active_model.clone(), profile.clone());
            }
            eprintln!(
                "[voicy] Active model '{}' path '{}' does not exist yet.",
                self.active_model,
                profile.path.display()
            );
        }

        if let Some(profile) = self.models.get("small.en") {
            if profile.path.exists() {
                eprintln!("[voicy] Falling back to 'small.en' at {}", profile.path.display());
                return ("small.en".to_string(), profile.clone());
            }
        }

        for (name, profile) in &self.models {
            if profile.path.exists() {
                eprintln!("[voicy] Falling back to model '{}' at {}", name, profile.path.display());
                return (name.clone(), profile.clone());
            }
        }

        let fallback = self.models.get(&self.active_model).cloned().unwrap_or_else(|| ModelProfile {
            path: get_default_model_dir("sherpa-onnx-whisper-distil-small.en"),
            encoder: None,
            decoder: None,
            tokens: None,
            language: "en".to_string(),
            num_threads: 4,
        });

        (self.active_model.clone(), fallback)
    }
}

pub fn get_socket_path() -> PathBuf {
    if let Ok(runtime_dir) = std::env::var("XDG_RUNTIME_DIR") {
        PathBuf::from(runtime_dir).join(DEFAULT_SOCKET_NAME)
    } else {
        PathBuf::from("/tmp").join(DEFAULT_SOCKET_NAME)
    }
}

pub fn get_config_dir() -> PathBuf {
    if let Ok(config_home) = std::env::var("XDG_CONFIG_HOME") {
        PathBuf::from(config_home).join("voicy")
    } else if let Ok(home) = std::env::var("HOME") {
        PathBuf::from(home).join(".config").join("voicy")
    } else {
        PathBuf::from(".config/voicy")
    }
}

pub fn get_config_path() -> PathBuf {
    if let Ok(home) = std::env::var("HOME") {
        let voic_path = PathBuf::from(&home).join(".config").join("voic").join(DEFAULT_CONFIG_FILENAME);
        if voic_path.exists() {
            return voic_path;
        }
    }
    get_config_dir().join(DEFAULT_CONFIG_FILENAME)
}
