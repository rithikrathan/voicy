use std::path::{Path, PathBuf};
use std::process::Command;

/// Locates a sound asset across installation and development paths
pub fn find_sound(names: &[&str]) -> Option<PathBuf> {
    let mut candidate_dirs = Vec::new();

    if let Ok(data_home) = std::env::var("XDG_DATA_HOME") {
        candidate_dirs.push(format!("{}/voicy/assets/ui", data_home));
    }
    if let Ok(home) = std::env::var("HOME") {
        candidate_dirs.push(format!("{}/.local/share/voicy/assets/ui", home));
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(parent) = exe.parent() {
            if let Some(share) = parent.parent().map(|p| p.join("share/voicy/assets/ui")) {
                if let Some(s) = share.to_str() {
                    candidate_dirs.push(s.to_string());
                }
            }
        }
    }
    candidate_dirs.push("src/assets/ui".to_string());
    candidate_dirs.push("/mnt/sda4/projects/voicy/src/assets/ui".to_string());

    for dir in &candidate_dirs {
        for name in names {
            let p = Path::new(dir).join(name);
            if p.exists() {
                return Some(p);
            }
        }
    }
    None
}

fn play_file(path: &Path) {
    let _ = Command::new("pw-play")
        .arg("--media-role=Notification")
        .arg(path)
        .status()
        .or_else(|_| {
            Command::new("paplay")
                .arg("--property=media.role=notification")
                .arg(path)
                .status()
        });
}

/// Plays the start sound cue as a Notification-class stream
pub fn play_start_sound() {
    std::thread::spawn(|| {
        if let Some(path) = find_sound(&["agentPop.mp3"]) {
            play_file(&path);
        }
    });
}

/// Plays the 30-second warning timeout beep as a subtle Notification-class stream
pub fn play_timeout_sound() {
    std::thread::spawn(|| {
        if let Some(path) = find_sound(&["timeout.mp3", "timeout.wav"]) {
            play_file(&path);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_find_start_sound() {
        let p = find_sound(&["agentPop.mp3"]);
        assert!(p.is_some(), "agentPop.mp3 sound asset should exist");
    }

    #[test]
    fn test_find_timeout_sound() {
        let p = find_sound(&["timeout.mp3", "timeout.wav"]);
        assert!(p.is_some(), "timeout sound asset should exist");
    }
}
