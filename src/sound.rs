use std::path::{Path, PathBuf};
use std::process::Command;

/// Plays the start sound cue as a Notification-class stream
pub fn play_start_sound() {
    std::thread::spawn(|| {
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
            let _ = Command::new("pw-play")
                .arg("--media-role=Notification")
                .arg(&path)
                .status()
                .or_else(|_| {
                    Command::new("paplay")
                        .arg("--property=media.role=notification")
                        .arg(&path)
                        .status()
                });
        }
    });
}
