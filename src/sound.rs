use std::path::{Path, PathBuf};
use std::process::Command;

pub fn play_start_sound() {
    std::thread::spawn(|| {
        // Try known asset locations
        let candidates = [
            "/home/rithik/.local/share/voicy/assets/ui/agentPop.mp3",
            "/mnt/sda4/projects/voicy/src/assets/ui/agentPop.mp3",
        ];

        let mut sound_path: Option<PathBuf> = None;
        for c in &candidates {
            let p = Path::new(c);
            if p.exists() {
                sound_path = Some(p.to_path_buf());
                break;
            }
        }

        if let Some(path) = sound_path {
            // Use pw-play or paplay
            let _ = Command::new("pw-play")
                .arg(&path)
                .status()
                .or_else(|_| Command::new("paplay").arg(&path).status());
        }
    });
}
