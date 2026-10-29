use std::collections::HashMap;
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

lazy_static::lazy_static! {
    static ref MUTED_STREAMS: Arc<Mutex<HashMap<u32, f32>>> = Arc::new(Mutex::new(HashMap::new()));
    static ref FADE_GENERATION: Arc<AtomicU64> = Arc::new(AtomicU64::new(0));
}

/// Discovers active playback audio stream IDs in PipeWire (e.g., Brave, Spotify, MPV, Discord)
/// excluding notification or audio cue sounds.
fn get_active_playback_streams() -> Vec<(u32, f32)> {
    let mut streams = Vec::new();
    if let Ok(output) = Command::new("pw-dump").output() {
        if let Ok(json_val) = serde_json::from_slice::<serde_json::Value>(&output.stdout) {
            if let Some(arr) = json_val.as_array() {
                for item in arr {
                    let id = item.get("id").and_then(|i| i.as_u64()).unwrap_or(0) as u32;
                    let props = item.get("info").and_then(|info| info.get("props"));
                    if let Some(p) = props {
                        let media_class = p.get("media.class").and_then(|m| m.as_str()).unwrap_or("");
                        let media_role = p.get("media.role").and_then(|r| r.as_str()).unwrap_or("");
                        let app_name = p.get("application.name").and_then(|a| a.as_str()).unwrap_or("");

                        // Target app playback streams, ignore alert/notification/cues and voicy itself
                        if media_class == "Stream/Output/Audio"
                            && media_role != "Notification"
                            && media_role != "Alert"
                            && !app_name.to_lowercase().contains("voicy")
                            && !app_name.to_lowercase().contains("pw-play")
                            && !app_name.to_lowercase().contains("paplay")
                        {
                            // Get current volume
                            let vol = get_node_volume(id).unwrap_or(1.0);
                            streams.push((id, vol));
                        }
                    }
                }
            }
        }
    }
    streams
}

fn get_node_volume(id: u32) -> Option<f32> {
    if let Ok(output) = Command::new("wpctl").arg("get-volume").arg(id.to_string()).output() {
        let text = String::from_utf8_lossy(&output.stdout);
        let parts: Vec<&str> = text.split_whitespace().collect();
        if parts.len() >= 2 {
            if let Ok(v) = parts[1].parse::<f32>() {
                return Some(v);
            }
        }
    }
    None
}

/// Smoothly fades out all active media/app playback streams to 0, then sets stream-level mute.
/// Leaves the master audio sink completely unmuted so the audio cue remains 100% audible!
pub fn fade_out_system_audio_sync() {
    let gen = FADE_GENERATION.fetch_add(1, Ordering::SeqCst) + 1;

    let active_streams = get_active_playback_streams();
    if active_streams.is_empty() {
        return;
    }

    {
        let mut map = MUTED_STREAMS.lock().unwrap();
        for (id, vol) in &active_streams {
            map.entry(*id).or_insert(*vol);
        }
    }

    let steps = 10;
    let step_delay = Duration::from_millis(15); // ~150ms fade

    for i in 1..=steps {
        if FADE_GENERATION.load(Ordering::SeqCst) != gen {
            return;
        }
        let factor = 1.0 - (i as f32 / steps as f32);
        for (id, orig_vol) in &active_streams {
            let target = orig_vol * factor;
            let _ = Command::new("wpctl")
                .arg("set-volume")
                .arg(id.to_string())
                .arg(format!("{:.2}", target))
                .status();
        }
        std::thread::sleep(step_delay);
    }

    if FADE_GENERATION.load(Ordering::SeqCst) == gen {
        for (id, _) in &active_streams {
            let _ = Command::new("wpctl")
                .arg("set-mute")
                .arg(id.to_string())
                .arg("1")
                .status();
        }
    }
}

/// Smoothly un-mutes and fades active media/app streams back in to their original volume levels.
pub fn fade_in_system_audio() {
    let gen = FADE_GENERATION.fetch_add(1, Ordering::SeqCst) + 1;

    let saved = {
        let mut map = MUTED_STREAMS.lock().unwrap();
        std::mem::take(&mut *map)
    };

    if saved.is_empty() {
        return;
    }

    std::thread::spawn(move || {
        for (id, _) in &saved {
            let _ = Command::new("wpctl")
                .arg("set-mute")
                .arg(id.to_string())
                .arg("0")
                .status();
        }

        let steps = 10;
        let step_delay = Duration::from_millis(15); // ~150ms fade back in
        for i in 1..=steps {
            if FADE_GENERATION.load(Ordering::SeqCst) != gen {
                return;
            }
            let factor = i as f32 / steps as f32;
            for (id, orig_vol) in &saved {
                let current = orig_vol * factor;
                let _ = Command::new("wpctl")
                    .arg("set-volume")
                    .arg(id.to_string())
                    .arg(format!("{:.2}", current))
                    .status();
            }
            std::thread::sleep(step_delay);
        }

        if FADE_GENERATION.load(Ordering::SeqCst) == gen {
            for (id, orig_vol) in &saved {
                let _ = Command::new("wpctl")
                    .arg("set-volume")
                    .arg(id.to_string())
                    .arg(format!("{:.2}", orig_vol))
                    .status();
            }
        }
    });
}
