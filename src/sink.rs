use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};

static WAS_MUTED_BY_US: AtomicBool = AtomicBool::new(false);

/// Checks if @DEFAULT_AUDIO_SINK@ is already muted
fn is_sink_muted() -> bool {
    if let Ok(output) = Command::new("wpctl")
        .arg("get-volume")
        .arg("@DEFAULT_AUDIO_SINK@")
        .output()
    {
        let text = String::from_utf8_lossy(&output.stdout);
        return text.contains("[MUTED]");
    }
    false
}

/// Mutes system sound when recording starts, remembering if user was already muted
pub fn mute_system_audio() {
    if is_sink_muted() {
        // User already muted audio themselves; don't touch on restore
        WAS_MUTED_BY_US.store(false, Ordering::SeqCst);
    } else {
        WAS_MUTED_BY_US.store(true, Ordering::SeqCst);
        let _ = Command::new("wpctl")
            .arg("set-mute")
            .arg("@DEFAULT_AUDIO_SINK@")
            .arg("1")
            .status();
    }
}

/// Restores system sound when recording stops or cancels, only if we muted it
pub fn restore_system_audio() {
    if WAS_MUTED_BY_US.swap(false, Ordering::SeqCst) {
        let _ = Command::new("wpctl")
            .arg("set-mute")
            .arg("@DEFAULT_AUDIO_SINK@")
            .arg("0")
            .status();
    }
}
