use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

lazy_static::lazy_static! {
    static ref ORIGINAL_VOLUME: Arc<Mutex<Option<f32>>> = Arc::new(Mutex::new(None));
    static ref FADE_GENERATION: Arc<std::sync::atomic::AtomicU64> = Arc::new(std::sync::atomic::AtomicU64::new(0));
}

/// Reads the current sink volume (0.00 - 1.00+) and whether it is muted
fn get_sink_volume() -> Option<(f32, bool)> {
    if let Ok(output) = Command::new("wpctl")
        .arg("get-volume")
        .arg("@DEFAULT_AUDIO_SINK@")
        .output()
    {
        let text = String::from_utf8_lossy(&output.stdout);
        // Format: "Volume: 0.71" or "Volume: 0.71 [MUTED]"
        let parts: Vec<&str> = text.split_whitespace().collect();
        if parts.len() >= 2 {
            if let Ok(vol) = parts[1].parse::<f32>() {
                let muted = text.contains("[MUTED]");
                return Some((vol, muted));
            }
        }
    }
    None
}

/// Smoothly fades out system audio to 0.0, then sets mute
pub fn fade_out_system_audio() {
    let gen = FADE_GENERATION.fetch_add(1, Ordering::SeqCst) + 1;

    let (current_vol, is_muted) = match get_sink_volume() {
        Some(v) => v,
        None => return,
    };

    if is_muted {
        return;
    }

    // Save starting volume if not already saved
    {
        let mut orig = ORIGINAL_VOLUME.lock().unwrap();
        if orig.is_none() {
            *orig = Some(current_vol);
        }
    }

    std::thread::spawn(move || {
        let steps = 15;
        let step_delay = Duration::from_millis(15); // ~225ms total smooth fade
        for i in 1..=steps {
            if FADE_GENERATION.load(Ordering::SeqCst) != gen {
                return; // Another fade took over
            }
            let factor = 1.0 - (i as f32 / steps as f32);
            let target = current_vol * factor;
            let _ = Command::new("wpctl")
                .arg("set-volume")
                .arg("@DEFAULT_AUDIO_SINK@")
                .arg(format!("{:.2}", target))
                .status();
            std::thread::sleep(step_delay);
        }

        if FADE_GENERATION.load(Ordering::SeqCst) == gen {
            let _ = Command::new("wpctl")
                .arg("set-mute")
                .arg("@DEFAULT_AUDIO_SINK@")
                .arg("1")
                .status();
        }
    });
}

/// Smoothly un-mutes and fades in system audio back to original volume
pub fn fade_in_system_audio() {
    let gen = FADE_GENERATION.fetch_add(1, Ordering::SeqCst) + 1;

    let target_vol = {
        let mut orig = ORIGINAL_VOLUME.lock().unwrap();
        orig.take()
    };

    let target = match target_vol {
        Some(v) => v,
        None => return,
    };

    std::thread::spawn(move || {
        // Unmute first at 0 volume
        let _ = Command::new("wpctl")
            .arg("set-mute")
            .arg("@DEFAULT_AUDIO_SINK@")
            .arg("0")
            .status();

        let steps = 15;
        let step_delay = Duration::from_millis(15); // ~225ms total smooth fade
        for i in 1..=steps {
            if FADE_GENERATION.load(Ordering::SeqCst) != gen {
                return; // Another fade took over
            }
            let factor = i as f32 / steps as f32;
            let current = target * factor;
            let _ = Command::new("wpctl")
                .arg("set-volume")
                .arg("@DEFAULT_AUDIO_SINK@")
                .arg(format!("{:.2}", current))
                .status();
            std::thread::sleep(step_delay);
        }

        if FADE_GENERATION.load(Ordering::SeqCst) == gen {
            let _ = Command::new("wpctl")
                .arg("set-volume")
                .arg("@DEFAULT_AUDIO_SINK@")
                .arg(format!("{:.2}", target))
                .status();
        }
    });
}
