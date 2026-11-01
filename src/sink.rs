use std::process::Command;
use std::sync::{Arc, Mutex};

lazy_static::lazy_static! {
    static ref PAUSED_PLAYERS: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
}

/// Identifies active MPRIS media players that are currently "Playing" and pauses them.
pub fn pause_media() {
    let mut paused = Vec::new();
    let output = match Command::new("playerctl").arg("-l").output() {
        Ok(out) if out.status.success() => out,
        _ => return,
    };

    let text = String::from_utf8_lossy(&output.stdout);
    for line in text.lines() {
        let player = line.trim();
        if player.is_empty() {
            continue;
        }

        if let Ok(status_out) = Command::new("playerctl")
            .arg("-p")
            .arg(player)
            .arg("status")
            .output()
        {
            let status = String::from_utf8_lossy(&status_out.stdout).trim().to_lowercase();
            if status == "playing" {
                if let Ok(res) = Command::new("playerctl")
                    .arg("-p")
                    .arg(player)
                    .arg("pause")
                    .status()
                {
                    if res.success() {
                        paused.push(player.to_string());
                    }
                }
            }
        }
    }

    let mut lock = PAUSED_PLAYERS.lock().unwrap();
    *lock = paused;
}

/// Resumes any MPRIS media players that were paused when recording started.
pub fn resume_media() {
    let players = {
        let mut lock = PAUSED_PLAYERS.lock().unwrap();
        std::mem::take(&mut *lock)
    };

    for player in players {
        let _ = Command::new("playerctl")
            .arg("-p")
            .arg(&player)
            .arg("play")
            .status();
    }
}
