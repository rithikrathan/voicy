use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

lazy_static::lazy_static! {
    static ref ORIGINAL_VOLUME: Arc<Mutex<Option<f32>>> = Arc::new(Mutex::new(None));
    static ref FADE_GENERATION: Arc<AtomicU64> = Arc::new(AtomicU64::new(0));
}

/// Reads the current sink volume (0.00 - 1.00+) and whether it is muted
fn get_sink_volume() -> Option<(f32, bool)> {
    if let Ok(output) = Command::new("wpctl")
        .arg("get-volume")
        .arg("@DEFAULT_AUDIO_SINK@")
        .output()
    {
        let text = String::from_utf8_lossy(&output.stdout);
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

/// Synchronously and smoothly fades out system audio to 0.0, then sets mute.
/// Blocks until system audio is 100% silent so mic recording captures NO fading audio.
pub fn fade_out_system_audio_sync() {
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

    let steps = 12;
    let step_delay = Duration::from_millis(15); // ~180ms total fade duration

    for i in 1..=steps {
        if FADE_GENERATION.load(Ordering::SeqCst) != gen {
            return;
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

    // Extra 20ms settling pause to guarantee soundcard DAC silence before mic opens
    std::thread::sleep(Duration::from_millis(20));
}

/// Asynchronously and smoothly fades system audio back in to original volume after mic stops
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
        let _ = Command::new("wpctl")
            .arg("set-mute")
            .arg("@DEFAULT_AUDIO_SINK@")
            .arg("0")
            .status();

        let steps = 12;
        let step_delay = Duration::from_millis(15); // ~180ms total smooth restore
        for i in 1..=steps {
            if FADE_GENERATION.load(Ordering::SeqCst) != gen {
                return;
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
