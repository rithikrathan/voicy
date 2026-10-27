use std::io::Write;
use std::process::{Command, Stdio};

pub fn copy_to_clipboard(text: &str) {
    if let Ok(mut child) = Command::new("wl-copy")
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    {
        if let Some(mut stdin) = child.stdin.take() {
            let _ = stdin.write_all(text.as_bytes());
        }
        let _ = child.wait();
    }
}

pub fn type_text(text: &str) {
    // 1. Try wtype first with 10ms delay between keys
    let wtype_res = Command::new("wtype")
        .arg("-d")
        .arg("10")
        .arg(text)
        .status();

    if let Ok(status) = wtype_res {
        if status.success() {
            let _ = Command::new("wtype").arg("-k").arg("Return").status();
            return;
        }
    }

    // 2. Fallback to ydotool
    let ydo_type = Command::new("ydotool")
        .arg("type")
        .arg(text)
        .status();

    if let Ok(status) = ydo_type {
        if status.success() {
            // keycode 28 is Enter (press: 28:1, release: 28:0)
            let _ = Command::new("ydotool")
                .arg("key")
                .arg("28:1")
                .arg("28:0")
                .status();
        }
    }
}

pub fn deliver_text(text: &str) {
    if text.trim().is_empty() {
        return;
    }
    // Copy to clipboard
    copy_to_clipboard(text);
    // Type it out into active window and press Return
    type_text(text);
}
