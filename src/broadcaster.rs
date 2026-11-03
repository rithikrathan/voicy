use std::fs;
use std::io::Write;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

pub const DEFAULT_EVENTS_SOCKET: &str = "voicy-events.sock";

#[derive(Clone)]
pub struct EventBroadcaster {
    clients: Arc<Mutex<Vec<UnixStream>>>,
    socket_path: PathBuf,
}

impl EventBroadcaster {
    pub fn new<P: AsRef<Path>>(socket_path: P) -> Self {
        let path = socket_path.as_ref().to_path_buf();
        let clients = Arc::new(Mutex::new(Vec::new()));

        Self {
            clients,
            socket_path: path,
        }
    }

    pub fn start(&self) -> Result<(), Box<dyn std::error::Error>> {
        if self.socket_path.exists() {
            let _ = fs::remove_file(&self.socket_path);
        }

        if let Some(parent) = self.socket_path.parent() {
            let _ = fs::create_dir_all(parent);
        }

        let listener = UnixListener::bind(&self.socket_path)?;
        println!(
            "[voicy] [broadcaster] Event stream socket listening at {}",
            self.socket_path.display()
        );

        let clients_clone = Arc::clone(&self.clients);
        thread::spawn(move || {
            for stream in listener.incoming() {
                match stream {
                    Ok(client) => {
                        let _ = client.set_nonblocking(true);
                        let _ = client.set_write_timeout(Some(Duration::from_millis(500)));
                        if let Ok(mut clients) = clients_clone.lock() {
                            println!("[voicy] [broadcaster] New subscriber connected (total: {})", clients.len() + 1);
                            clients.push(client);
                        }
                    }
                    Err(e) => {
                        eprintln!("[voicy] [broadcaster] Accept error: {}", e);
                    }
                }
            }
        });

        Ok(())
    }

    pub fn broadcast_transcription(&self, text: &str) {
        if text.trim().is_empty() {
            return;
        }

        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        let payload = serde_json::json!({
            "event": "transcription",
            "text": text.trim(),
            "timestamp": timestamp
        });

        let mut message = payload.to_string();
        message.push('\n');

        self.broadcast_raw(&message);
    }

    pub fn broadcast_raw(&self, message: &str) {
        let bytes = message.as_bytes();
        let mut clients = match self.clients.lock() {
            Ok(c) => c,
            Err(_) => return,
        };

        if clients.is_empty() {
            return;
        }

        let mut alive = Vec::with_capacity(clients.len());
        for mut client in clients.drain(..) {
            if client.write_all(bytes).is_ok() && client.flush().is_ok() {
                alive.push(client);
            }
        }
        *clients = alive;
    }
}

impl Drop for EventBroadcaster {
    fn drop(&mut self) {
        if self.socket_path.exists() {
            let _ = fs::remove_file(&self.socket_path);
        }
    }
}

pub fn get_events_socket_path() -> PathBuf {
    if let Ok(runtime_dir) = std::env::var("XDG_RUNTIME_DIR") {
        PathBuf::from(runtime_dir).join(DEFAULT_EVENTS_SOCKET)
    } else {
        PathBuf::from("/tmp").join(DEFAULT_EVENTS_SOCKET)
    }
}
