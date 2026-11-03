use std::path::PathBuf;
use std::process::Command;

const SHARE_ASSETS_SUBDIR: &str = ".local/share/voicy/assets/ui";

fn get_sound_path(filename: &str) -> PathBuf {
    if let Ok(home) = std::env::var("HOME") {
        let path = PathBuf::from(home).join(SHARE_ASSETS_SUBDIR).join(filename);
        if path.exists() {
            return path;
        }
    }
    PathBuf::from("src/assets/ui").join(filename)
}

fn play_file(filename: &str) {
    let path = get_sound_path(filename);
    if !path.exists() {
        return;
    }

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

/// Plays the start sound cue as a Notification-class stream
pub fn play_start_sound() {
    std::thread::spawn(|| {
        play_file("agentPop.mp3");
    });
}

/// Plays the 30-second warning timeout beep
pub fn play_timeout_sound() {
    std::thread::spawn(|| {
        play_file("timeout.mp3");
    });
}

/// Plays the delivery confirmation chime
pub fn play_delivered_sound() {
    std::thread::spawn(|| {
        play_file("delivered.mp3");
    });
}
