# Voicy

A lightweight, blazing-fast, and battery-friendly voice-to-text dictation tool designed specifically for Linux Wayland environments (Hyprland, Sway, etc.) written in pure Rust.

Voicy uses an offline quantized Whisper model (`sherpa-onnx`) running entirely in-memory with zero disk writes, native PipeWire audio streaming, Unix domain socket IPC, and Wayland virtual keyboard simulation.

---

## Features

- **Pure Rust Architecture**: Single unified binary handling both background daemon and CLI hotkey invocations.
- **Zero Disk I/O**: Captures microphone audio directly into RAM (`Vec<f32>`) and passes it straight to inference without touching the filesystem.
- **Robust Unix Domain Socket**: Located in `$XDG_RUNTIME_DIR/voicy.sock` (`tmpfs` RAM) with automated stale socket cleanup and PID/instance safety.
- **Dual Output**: Copies transcribed text to the system clipboard (`wl-copy`) and simulates real keystrokes (`wtype` / `ydotool`) into the active window, followed by `Enter`.
- **Offline & Private**: Runs completely local using ONNX Runtime quantized models (e.g. Whisper Small INT8).

---

## Architecture

```text
┌─────────────────────────────────────────────────────────────┐
│                       voicy (Rust binary)                   │
├──────────────────────────────┬──────────────────────────────┤
│      1. CLI Client           │        2. Background Daemon   │
│  - Super+D -> `voicy toggle` │  - Model kept warm in RAM    │
│  - Super+Shift+D -> `cancel` │  - In-memory mic recording   │
│  - Talk via Unix Socket      │  - sherpa-onnx Whisper INT8  │
│    (instant <1ms turnaround) │  - wl-copy + type text       │
└──────────────────────────────┴──────────────────────────────┘
```

---

## Prerequisites

1. **Rust toolchain** (`cargo`, `rustc`).
2. **Wayland typing and clipboard tools**:
   ```bash
   sudo pacman -S wl-clipboard wtype
   # or on Debian/Ubuntu:
   sudo apt install wl-clipboard wtype
   ```
3. **PipeWire / ALSA** audio subsystem.
4. **Sherpa ONNX Whisper model**:
   Download a quantized model (e.g., `sherpa-onnx-whisper-small.en`):
   - `small.en-encoder.int8.onnx`
   - `small.en-decoder.int8.onnx`
   - `small.en-tokens.txt`

---

## Installation

```bash
# Clone the repository
git clone https://github.com/your-username/voicy.git
cd voicy

# Build release binary
cargo build --release

# Install binary to ~/.local/bin
cp target/release/voicy ~/.local/bin/
```

---

## Usage

### 1. Run the Daemon
```bash
# Start the daemon pointing to your model folder
voicy daemon --model-dir /path/to/sherpa-onnx-whisper-small.en

# Or use the default location (/mnt/sda4/models/sherpa-onnx/sherpa-onnx-whisper-small.en)
voicy daemon
```

### 2. Control Commands (Hotkeys)
- **Toggle Recording:**
  ```bash
  voicy toggle
  ```
  *(Press once to start recording; press again to stop, transcribe, copy, and type).*
- **Cancel Recording:**
  ```bash
  voicy cancel
  ```
  *(Immediately stops and discards any audio without transcribing).*
- **Check Status:**
  ```bash
  voicy status
  ```

---

## Hyprland Configuration

Add the following keybindings to your `~/.config/hypr/hyprland.conf` (or `keybinds.conf`):

```ini
# Voice-to-text dictation
bind = Super, D, exec, voicy toggle
bind = Super+Shift, D, exec, voicy cancel
```

---

## Systemd User Service

To automatically start `voicy daemon` on login, create `~/.config/systemd/user/voicyd.service`:

```ini
[Unit]
Description=Voicy Rust Whisper STT Daemon
After=pipewire.service sound.target

[Service]
Type=simple
ExecStart=%h/.local/bin/voicy daemon
Restart=on-failure
RestartSec=1

[Install]
WantedBy=default.target
```

Enable and start the service:
```bash
systemctl --user daemon-reload
systemctl --user enable --now voicyd.service
```

---

## License

MIT
