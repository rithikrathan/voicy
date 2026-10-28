use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};

fn get_wayland_display() -> Option<String> {
    if let Ok(disp) = std::env::var("WAYLAND_DISPLAY") {
        if !disp.is_empty() {
            return Some(disp);
        }
    }

    // Fallback: detect active wayland socket in XDG_RUNTIME_DIR
    if let Ok(runtime_dir) = std::env::var("XDG_RUNTIME_DIR") {
        for name in &["wayland-1", "wayland-0"] {
            if PathBuf::from(&runtime_dir).join(name).exists() {
                return Some((*name).to_string());
            }
        }
    }

    None
}

fn apply_wayland_env(cmd: &mut Command) {
    if let Some(disp) = get_wayland_display() {
        cmd.env("WAYLAND_DISPLAY", disp);
    }
    if let Ok(runtime_dir) = std::env::var("XDG_RUNTIME_DIR") {
        cmd.env("XDG_RUNTIME_DIR", runtime_dir);
    }
}

pub fn copy_to_clipboard(text: &str) {
    let mut cmd = Command::new("wl-copy");
    apply_wayland_env(&mut cmd);
    cmd.stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null());

    if let Ok(mut child) = cmd.spawn() {
        if let Some(mut stdin) = child.stdin.take() {
            let _ = stdin.write_all(text.as_bytes());
        }
        let _ = child.wait();
    }
}

pub fn type_text(text: &str) {
    // 1. Try wtype first with 10ms delay between keys
    let mut wtype_cmd = Command::new("wtype");
    apply_wayland_env(&mut wtype_cmd);
    wtype_cmd.arg("-d").arg("10").arg(text);

    if let Ok(status) = wtype_cmd.status() {
        if status.success() {
            let mut ret_cmd = Command::new("wtype");
            apply_wayland_env(&mut ret_cmd);
            let _ = ret_cmd.arg("-k").arg("Return").status();
            return;
        }
    }

    // 2. Fallback to ydotool
    let mut ydo_cmd = Command::new("ydotool");
    apply_wayland_env(&mut ydo_cmd);
    ydo_cmd.arg("type").arg(text);

    if let Ok(status) = ydo_cmd.status() {
        if status.success() {
            let mut enter_cmd = Command::new("ydotool");
            apply_wayland_env(&mut enter_cmd);
            let _ = enter_cmd.arg("key").arg("28:1").arg("28:0").status();
        }
    }
}

pub fn deliver_text(text: &str) {
    if text.trim().is_empty() {
        return;
    }
    copy_to_clipboard(text);
    type_text(text);
}
