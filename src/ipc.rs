use std::fs;
use std::io::{Read, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::Path;
use std::time::Duration;

pub fn connect_or_clean(socket_path: &Path) -> Result<UnixListener, Box<dyn std::error::Error>> {
    if socket_path.exists() {
        // Ping check: see if a daemon is actively listening
        match UnixStream::connect(socket_path) {
            Ok(mut stream) => {
                stream.set_read_timeout(Some(Duration::from_millis(200)))?;
                let _ = stream.write_all(b"STATUS\n");
                let mut buf = [0u8; 64];
                if stream.read(&mut buf).is_ok() {
                    return Err(format!(
                        "Another active daemon is already listening at {}",
                        socket_path.display()
                    )
                    .into());
                }
                // If it connected but failed read, still treat as stale
                let _ = fs::remove_file(socket_path);
            }
            Err(_) => {
                // Connection refused or stale file -> remove it
                let _ = fs::remove_file(socket_path);
            }
        }
    }

    let listener = UnixListener::bind(socket_path)?;
    // Set 0600 permissions: user read/write only
    let _ = fs::set_permissions(socket_path, fs::Permissions::from_mode(0o600));
    Ok(listener)
}

pub fn send_command(socket_path: &Path, cmd: &str) -> Result<String, Box<dyn std::error::Error>> {
    let mut stream = UnixStream::connect(socket_path).map_err(|e| {
        format!(
            "Failed to connect to daemon at {}: {}. Is 'voicy daemon' running?",
            socket_path.display(),
            e
        )
    })?;

    stream.set_read_timeout(Some(Duration::from_secs(30)))?;
    stream.write_all(format!("{}\n", cmd.trim()).as_bytes())?;

    let mut response = String::new();
    stream.read_to_string(&mut response)?;
    Ok(response)
}
