use std::path::PathBuf;

pub const DEFAULT_SOCKET_NAME: &str = "voicy.sock";
pub const MODEL_DEFAULT_DIR: &str = "/mnt/sda4/models/sherpa-onnx/sherpa-onnx-whisper-small.en";

pub fn get_socket_path() -> PathBuf {
    if let Ok(runtime_dir) = std::env::var("XDG_RUNTIME_DIR") {
        PathBuf::from(runtime_dir).join(DEFAULT_SOCKET_NAME)
    } else {
        PathBuf::from("/tmp").join(DEFAULT_SOCKET_NAME)
    }
}
