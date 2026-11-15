use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;
use tungstenite::protocol::WebSocket;
use tungstenite::{accept, Message};

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(tag = "type")]
pub enum ClientInMessage {
    #[serde(rename = "DELIVER_TEXT")]
    DeliverText {
        text: String,
        #[serde(default)]
        is_btw: bool,
    },
    #[serde(rename = "KEY_PRESS")]
    KeyPress {
        key: String,
    },
    #[serde(rename = "CLIENT_STATE")]
    ClientState {
        state: String,
    },
    #[serde(rename = "SET_TTS_ROUTING")]
    SetTtsRouting {
        route_to_phone: bool,
    },
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(tag = "type")]
pub enum DaemonOutMessage {
    #[serde(rename = "STATUS_UPDATE")]
    StatusUpdate {
        target: Option<String>,
        #[serde(rename = "is_btw")]
        is_btw: bool,
    },
    #[serde(rename = "CLIENT_STATUS")]
    ClientStatus {
        enabled: bool,
    },
    #[serde(rename = "STATE_UPDATE")]
    StateUpdate {
        state: String,
    },
    #[serde(rename = "TTS_PROMPT")]
    TtsPrompt {
        text: String,
    },
}

pub struct SharedDaemonState {
    pub client_mode_enabled: AtomicBool,
    pub client_connected: AtomicBool,
    pub route_tts_to_phone: AtomicBool,
    pub client_active_state: Mutex<String>,
    pub input_lock: Mutex<()>,
    pub active_target: Arc<Mutex<Option<String>>>,
    pub is_typing: Arc<AtomicBool>,
    pub log_file: Option<PathBuf>,
}

pub struct WsServer {
    clients: Arc<Mutex<HashMap<usize, WebSocket<TcpStream>>>>,
    state: Arc<SharedDaemonState>,
}

impl WsServer {
    pub fn new(state: Arc<SharedDaemonState>) -> Self {
        Self {
            clients: Arc::new(Mutex::new(HashMap::new())),
            state,
        }
    }

    pub fn start(&self, port: u16) -> Result<(), Box<dyn std::error::Error>> {
        let addr = format!("0.0.0.0:{}", port);
        let listener = TcpListener::bind(&addr)?;
        println!("[voicy] [ws] WebSocket server listening on {}", addr);

        let clients_map = Arc::clone(&self.clients);
        let state = Arc::clone(&self.state);

        thread::spawn(move || {
            let mut client_id_seq = 0usize;
            for stream in listener.incoming() {
                match stream {
                    Ok(tcp_stream) => {
                        let _ = tcp_stream.set_nodelay(true);
                        let _ = tcp_stream.set_read_timeout(Some(Duration::from_millis(200)));
                        let _ = tcp_stream.set_write_timeout(Some(Duration::from_millis(500)));

                        match accept(tcp_stream) {
                            Ok(mut ws) => {
                                client_id_seq += 1;
                                let id = client_id_seq;
                                println!("[voicy] [ws] New client #{} connected", id);
                                state.client_connected.store(true, Ordering::SeqCst);

                                // Send initial status
                                let initial_target = state.active_target.lock().ok().and_then(|t| t.clone());
                                let status_msg = DaemonOutMessage::StatusUpdate {
                                    target: initial_target,
                                    is_btw: false,
                                };
                                let client_status_msg = DaemonOutMessage::ClientStatus {
                                    enabled: state.client_mode_enabled.load(Ordering::SeqCst),
                                };

                                if let Ok(json) = serde_json::to_string(&status_msg) {
                                    let _ = ws.send(Message::Text(json.into()));
                                }
                                if let Ok(json) = serde_json::to_string(&client_status_msg) {
                                    let _ = ws.send(Message::Text(json.into()));
                                }

                                if let Ok(mut clients) = clients_map.lock() {
                                    clients.insert(id, ws);
                                }

                                let state_clone = Arc::clone(&state);
                                let clients_for_worker = Arc::clone(&clients_map);

                                thread::spawn(move || {
                                    loop {
                                        let msg_opt = {
                                            let mut map = match clients_for_worker.lock() {
                                                Ok(m) => m,
                                                Err(_) => break,
                                            };
                                            if let Some(ws_ref) = map.get_mut(&id) {
                                                match ws_ref.read() {
                                                    Ok(m) => Some(Ok(m)),
                                                    Err(tungstenite::Error::Io(ref e))
                                                        if e.kind() == std::io::ErrorKind::WouldBlock
                                                            || e.kind() == std::io::ErrorKind::TimedOut =>
                                                    {
                                                        None
                                                    }
                                                    Err(e) => Some(Err(e)),
                                                }
                                            } else {
                                                break;
                                            }
                                        };

                                        match msg_opt {
                                            Some(Ok(Message::Text(txt))) => {
                                                handle_client_message(&txt, &state_clone);
                                            }
                                            Some(Ok(Message::Close(_))) | Some(Err(_)) => {
                                                println!("[voicy] [ws] Client #{} disconnected", id);
                                                if let Ok(mut map) = clients_for_worker.lock() {
                                                    map.remove(&id);
                                                    if map.is_empty() {
                                                        state_clone.client_connected.store(false, Ordering::SeqCst);
                                                        if let Ok(mut st) = state_clone.client_active_state.lock() {
                                                            *st = "idle".to_string();
                                                        }
                                                    }
                                                }
                                                break;
                                            }
                                            Some(Ok(Message::Ping(p))) => {
                                                if let Ok(mut map) = clients_for_worker.lock() {
                                                    if let Some(ws_ref) = map.get_mut(&id) {
                                                        let _ = ws_ref.send(Message::Pong(p));
                                                    }
                                                }
                                            }
                                            _ => {
                                                thread::sleep(Duration::from_millis(50));
                                            }
                                        }
                                    }
                                });
                            }
                            Err(e) => {
                                eprintln!("[voicy] [ws] Handshake error: {}", e);
                            }
                        }
                    }
                    Err(e) => {
                        eprintln!("[voicy] [ws] Incoming TCP connection error: {}", e);
                    }
                }
            }
        });

        Ok(())
    }

    pub fn broadcast(&self, msg: &DaemonOutMessage) {
        if let Ok(json) = serde_json::to_string(msg) {
            if let Ok(mut clients) = self.clients.lock() {
                clients.retain(|_id, ws| ws.send(Message::Text(json.clone().into())).is_ok());
            }
        }
    }
}

fn handle_client_message(json_str: &str, state: &SharedDaemonState) {
    if let Ok(msg) = serde_json::from_str::<ClientInMessage>(json_str) {
        match msg {
            ClientInMessage::DeliverText { text, is_btw } => {
                println!("[voicy] [ws] Received text delivery from client (len: {}, btw: {})", text.len(), is_btw);
                let _lock = state.input_lock.lock();

                if let Ok(mut st) = state.client_active_state.lock() {
                    *st = "typing".to_string();
                }
                state.is_typing.store(true, Ordering::SeqCst);

                let target_opt = state.active_target.lock().ok().and_then(|t| t.clone());
                let target_str = target_opt.as_deref();

                crate::output::deliver_text_with_mode(
                    &text,
                    false, // targeted delivery
                    is_btw,
                    target_str,
                    state.log_file.as_deref(),
                    false,
                );

                crate::output::press_enter_key(false, target_str);
                crate::sound::play_delivered_sound();

                state.is_typing.store(false, Ordering::SeqCst);
                if let Ok(mut st) = state.client_active_state.lock() {
                    *st = "idle".to_string();
                }
            }
            ClientInMessage::KeyPress { key } => {
                println!("[voicy] [ws] Received key press: {}", key);
                let _lock = state.input_lock.lock();
                let target_opt = state.active_target.lock().ok().and_then(|t| t.clone());
                let target_str = target_opt.as_deref();

                if let Some(t) = target_str {
                    if t != "none" && !t.is_empty() {
                        let _ = crate::tmux::send_raw_keys(t, &key);
                        return;
                    }
                }
                // Fallback to desktop key simulation if needed
            }
            ClientInMessage::ClientState { state: client_st } => {
                println!("[voicy] [ws] Client state update: {}", client_st);
                if let Ok(mut st) = state.client_active_state.lock() {
                    *st = client_st.to_lowercase();
                }
            }
            ClientInMessage::SetTtsRouting { route_to_phone } => {
                println!("[voicy] [ws] Setting route_tts_to_phone: {}", route_to_phone);
                state.route_tts_to_phone.store(route_to_phone, Ordering::SeqCst);
            }
        }
    }
}
