use lazy_static::lazy_static;
use rodio::{Decoder, OutputStream, Sink};
use std::io::Cursor;
use std::path::PathBuf;
use std::sync::mpsc::{sync_channel, SyncSender};
use std::thread;

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

fn load_sound_bytes(filename: &str) -> Option<Vec<u8>> {
    let path = get_sound_path(filename);
    std::fs::read(&path).ok()
}

#[derive(Debug, Clone, Copy)]
pub enum SoundCue {
    Start,
    Timeout,
    Delivered,
}

struct SoundPlayer {
    sender: SyncSender<SoundCue>,
}

lazy_static! {
    static ref PLAYER: Option<SoundPlayer> = init_player();
}

fn play_fallback(cue: SoundCue) {
    let filename = match cue {
        SoundCue::Start => "agentPop.mp3",
        SoundCue::Timeout => "timeout.mp3",
        SoundCue::Delivered => "delivered.mp3",
    };
    let path = get_sound_path(filename);
    if path.exists() {
        let _ = std::process::Command::new("pw-play")
            .arg("--media-role=Notification")
            .arg(&path)
            .status()
            .or_else(|_| {
                std::process::Command::new("paplay")
                    .arg("--property=media.role=notification")
                    .arg(&path)
                    .status()
            });
    }
}

fn init_player() -> Option<SoundPlayer> {
    let (tx, rx) = sync_channel::<SoundCue>(16);

    thread::Builder::new()
        .name("sound-player".to_string())
        .spawn(move || {
            let (_stream, stream_handle) = match OutputStream::try_default() {
                Ok(res) => res,
                Err(e) => {
                    eprintln!(
                        "[voicy] [sound] Notice: Audio output stream init: {}. Using system fallback.",
                        e
                    );
                    while let Ok(cue) = rx.recv() {
                        play_fallback(cue);
                    }
                    return;
                }
            };

            while let Ok(cue) = rx.recv() {
                let filename = match cue {
                    SoundCue::Start => "agentPop.mp3",
                    SoundCue::Timeout => "timeout.mp3",
                    SoundCue::Delivered => "delivered.mp3",
                };

                if let Some(bytes) = load_sound_bytes(filename) {
                    let cursor = Cursor::new(bytes);
                    match Decoder::new(cursor) {
                        Ok(source) => {
                            match Sink::try_new(&stream_handle) {
                                Ok(sink) => {
                                    let volume = match cue {
                                        SoundCue::Start => 0.80,
                                        SoundCue::Timeout => 0.80,
                                        SoundCue::Delivered => 0.26,
                                    };
                                    sink.set_volume(volume);
                                    sink.append(source);
                                    sink.detach();
                                }
                                Err(_) => play_fallback(cue),
                            }
                        }
                        Err(_) => play_fallback(cue),
                    }
                } else {
                    play_fallback(cue);
                }
            }
        })
        .ok()?;

    Some(SoundPlayer { sender: tx })
}

/// Plays a sound cue non-blockingly via persistent in-daemon audio stream
pub fn play_cue(cue: SoundCue) {
    if let Some(player) = PLAYER.as_ref() {
        let _ = player.sender.try_send(cue);
    } else {
        std::thread::spawn(move || {
            play_fallback(cue);
        });
    }
}

/// Plays the start recording sound cue
pub fn play_start_sound() {
    play_cue(SoundCue::Start);
}

/// Plays the 30-second warning timeout beep
pub fn play_timeout_sound() {
    play_cue(SoundCue::Timeout);
}

/// Plays the delivery confirmation chime
pub fn play_delivered_sound() {
    play_cue(SoundCue::Delivered);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sound_paths_exist() {
        for name in &["agentPop.mp3", "timeout.mp3", "delivered.mp3"] {
            let p = get_sound_path(name);
            assert!(p.exists(), "Sound file {} should exist at {:?}", name, p);
        }
    }

    #[test]
    fn test_load_sound_bytes() {
        let bytes = load_sound_bytes("delivered.mp3");
        assert!(bytes.is_some(), "delivered.mp3 bytes should load successfully");
        let _ = PLAYER.as_ref();
    }
}
