#![allow(dead_code)]

use std::process::Command;

/// Checks if tmux server is running and accessible
pub fn is_tmux_running() -> bool {
    Command::new("tmux")
        .arg("ls")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Detects the current pane ID from the calling process environment ($TMUX_PANE)
pub fn get_env_tmux_pane() -> Option<String> {
    if let Ok(pane) = std::env::var("TMUX_PANE") {
        let trimmed = pane.trim();
        if !trimmed.is_empty() {
            return Some(trimmed.to_string());
        }
    }
    None
}

/// Detects the active pane across attached clients
pub fn get_active_client_pane() -> Option<String> {
    // Queries all panes across sessions formatted with client activity and pane ID
    let output = Command::new("tmux")
        .args([
            "list-panes",
            "-a",
            "-F",
            "#{?session_attached,1,0} #{client_activity} #{pane_id}",
        ])
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    // Find the attached pane with the highest activity timestamp
    let mut best_pane: Option<(u64, String)> = None;

    for line in stdout.lines() {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() >= 3 {
            let is_attached = parts[0] == "1";
            let activity = parts[1].parse::<u64>().unwrap_or(0);
            let pane_id = parts[2].to_string();

            if is_attached {
                if let Some((best_act, _)) = best_pane {
                    if activity > best_act {
                        best_pane = Some((activity, pane_id));
                    }
                } else {
                    best_pane = Some((activity, pane_id));
                }
            }
        }
    }

    best_pane.map(|(_, pane_id)| pane_id)
}

/// Verifies whether a specific pane or session target currently exists
pub fn is_pane_alive(target: &str) -> bool {
    Command::new("tmux")
        .args(["has-session", "-t", target])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
        || Command::new("tmux")
            .args(["display-message", "-t", target, "-p", "#{pane_id}"])
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
}

/// Injects literal text and optionally Enter into the target tmux pane
pub fn send_keys_to_pane(target: &str, text: &str, press_enter: bool) -> Result<(), String> {
    if text.trim().is_empty() {
        return Ok(());
    }

    // 1. Send literal characters without tmux key name lookup (-l)
    let status = Command::new("tmux")
        .args(["send-keys", "-t", target, "-l", "--", text])
        .status()
        .map_err(|e| format!("Failed to invoke tmux send-keys: {}", e))?;

    if !status.success() {
        return Err(format!("tmux send-keys returned exit code {:?}", status.code()));
    }

    // 2. Send Enter key if requested
    if press_enter {
        let enter_status = Command::new("tmux")
            .args(["send-keys", "-t", target, "Enter"])
            .status()
            .map_err(|e| format!("Failed to send Enter key: {}", e))?;

        if !enter_status.success() {
            return Err("tmux send-keys Enter failed".to_string());
        }
    }

    Ok(())
}
