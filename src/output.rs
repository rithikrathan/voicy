use crate::tmux;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

fn get_wayland_display() -> Option<String> {
    if let Ok(disp) = std::env::var("WAYLAND_DISPLAY") {
        if !disp.is_empty() {
            return Some(disp);
        }
    }

    if let Ok(runtime_dir) = std::env::var("XDG_RUNTIME_DIR") {
        for name in &["wayland-1", "wayland-0"] {
            if PathBuf::from(&runtime_dir).join(name).exists() {
                return Some((*name).to_string());
            }
        }
    }

    None
}

fn apply_display_env(cmd: &mut Command) {
    if let Some(disp) = get_wayland_display() {
        cmd.env("WAYLAND_DISPLAY", disp);
    }
    if let Ok(runtime_dir) = std::env::var("XDG_RUNTIME_DIR") {
        cmd.env("XDG_RUNTIME_DIR", runtime_dir);
    }
    if let Ok(x11_disp) = std::env::var("DISPLAY") {
        cmd.env("DISPLAY", x11_disp);
    }
}

pub fn copy_to_clipboard(text: &str) {
    // 1. Try wl-copy (Wayland default)
    let mut wl_cmd = Command::new("wl-copy");
    apply_display_env(&mut wl_cmd);
    wl_cmd.stdin(Stdio::piped()).stdout(Stdio::null()).stderr(Stdio::null());

    if let Ok(mut child) = wl_cmd.spawn() {
        if let Some(mut stdin) = child.stdin.take() {
            let _ = stdin.write_all(text.as_bytes());
        }
        if let Ok(status) = child.wait() {
            if status.success() {
                return;
            }
        }
    }

    // 2. Fallback to xclip (X11 / universal clipboard)
    let mut xclip_cmd = Command::new("xclip");
    apply_display_env(&mut xclip_cmd);
    xclip_cmd.args(["-selection", "clipboard"])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null());

    if let Ok(mut child) = xclip_cmd.spawn() {
        if let Some(mut stdin) = child.stdin.take() {
            let _ = stdin.write_all(text.as_bytes());
        }
        let _ = child.wait();
    }
}

pub fn press_enter_key(is_dictate: bool, target: Option<&str>) {
    if !is_dictate {
        if let Some(t) = target {
            if t != "none" && !t.is_empty() {
                if tmux::send_keys_to_pane(t, "", true).is_ok() {
                    return;
                }
            }
        }
    }

    // Fallback to typing enter on active window
    type_text("", true);
}

pub fn type_text(text: &str, press_enter: bool) {
    // 1. Try wtype (Wayland wlroots compositor default: Hyprland, Sway, River)
    let mut wtype_cmd = Command::new("wtype");
    apply_display_env(&mut wtype_cmd);
    wtype_cmd.arg("-d").arg("10").arg(text);

    if let Ok(status) = wtype_cmd.status() {
        if status.success() {
            if press_enter {
                let mut ret_cmd = Command::new("wtype");
                apply_display_env(&mut ret_cmd);
                let _ = ret_cmd.arg("-k").arg("Return").status();
            }
            return;
        }
    }

    // 2. Fallback to ydotool (Universal Linux / uinput: works on GNOME Wayland, KDE, Sway, Hyprland)
    let mut ydo_cmd = Command::new("ydotool");
    apply_display_env(&mut ydo_cmd);
    ydo_cmd.arg("type").arg(text);

    if let Ok(status) = ydo_cmd.status() {
        if status.success() {
            if press_enter {
                let mut enter_cmd = Command::new("ydotool");
                apply_display_env(&mut enter_cmd);
                let _ = enter_cmd.arg("key").arg("28:1").arg("28:0").status();
            }
            return;
        }
    }

    // 3. Fallback to xdotool (X11 environments)
    let mut xdo_cmd = Command::new("xdotool");
    apply_display_env(&mut xdo_cmd);
    xdo_cmd.args(["type", "--clearmodifiers", text]);
    if let Ok(status) = xdo_cmd.status() {
        if status.success() {
            if press_enter {
                let mut enter_cmd = Command::new("xdotool");
                apply_display_env(&mut enter_cmd);
                let _ = enter_cmd.args(["key", "Return"]).status();
            }
        }
    }
}

pub fn append_to_dataset_log(text: &str, log_file: Option<&Path>) {
    let path = match log_file {
        Some(p) if !p.as_os_str().is_empty() => p,
        _ => return, // Logging disabled if None or empty path
    };

    let single_line = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if single_line.is_empty() {
        return;
    }

    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(path) {
        let _ = writeln!(file, "{}", single_line);
    }
}

/// Unified multi-channel delivery router
pub fn deliver_text(
    text: &str,
    is_dictate: bool,
    target: Option<&str>,
    log_file: Option<&Path>,
    press_enter: bool,
) {
    if text.trim().is_empty() {
        return;
    }

    append_to_dataset_log(text, log_file);

    // Channel 1: Dictation Mode (Active GUI Window typing)
    if is_dictate {
        println!("[voicy] [output] Delivering via desktop dictation (wtype/ydotool)...");
        type_text(text, press_enter);
        return;
    }

    // Channel 2: Targeted Mode (Tmux PTY injection with dictation fallback)
    if let Some(t) = target {
        if t != "none" && !t.is_empty() {
            println!("[voicy] [output] Delivering to tmux target '{}'...", t);
            match tmux::send_keys_to_pane(t, text, press_enter) {
                Ok(_) => {
                    return;
                }
                Err(e) => {
                    eprintln!(
                        "[voicy] [output] Tmux target '{}' failed ({}), falling back to desktop dictation.",
                        t, e
                    );
                }
            }
        }
    }

    // Target is None or Tmux failed -> Clean Fallback to Desktop Dictation
    println!("[voicy] [output] No active target set; falling back to desktop dictation...");
    type_text(text, press_enter);
}
