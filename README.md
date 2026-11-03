# Voicy

A lightweight, blazing-fast, and battery-friendly voice-to-text dictation and terminal agent router designed specifically for Linux Wayland environments (Hyprland, Sway, etc.) written in pure Rust.

Voicy uses an offline quantized Distil-Whisper model (`sherpa-onnx`) running entirely in-memory with zero disk writes during audio capture, native PipeWire audio streaming, Unix domain socket event broadcasting, active tmux session targeting, and Wayland virtual keyboard simulation.

---

## Key Features

- **Distil-Whisper INT8 Acceleration**: Transcribes speech at ~0.35x Real-Time Factor (up to 3-4x faster than real-time on CPU) with high technical vocabulary retention from Whisper Large distillation.
- **Configurable Model Switching (`~/.config/voicy/config.toml`)**: Switch between Distil-Whisper (`distil-small.en`) and vanilla Whisper Small (`small.en`) with automatic ONNX model discovery.
- **Active Tmux Session Targeting**: Injects transcribed text directly into tmux terminal panes via literal `tmux send-keys` without window-focus hijacking.
- **Effortless `voicy target here`**: Run `voicy target here` inside any terminal split to immediately lock Voicy's output onto that pane.
- **Dedicated Desktop Dictation (`--dictate`)**: Pass `--dictate` to type into whatever GUI window is currently focused (`wtype`).
- **Unix Domain Event Socket (`voicy-events.sock`)**: Broadcasts newline-delimited JSON events to all connected subscriber processes (home server agents, background listeners, or Python scripts) in real-time.
- **LIFO Stack Cancellation (`voicy cancel`)**: Pressing cancel pops the topmost active action: cancels the active recording buffer if recording (leaving background transcriptions undisturbed), or cancels in-flight transcription if idle.
- **Defensive Data Invariants**: Transcriptions are always saved to the Wayland clipboard (`wl-copy`) and appended to `/mnt/sda4/projects/tin/data/test_input.txt` and `input.txt`.

---

## Architecture Overview

```text
┌────────────────────────────────────────────────────────────────────────┐
│                          voicy (Unified Rust Binary)                   │
├──────────────────────────────────┬─────────────────────────────────────┤
│ 1. CLI Commands & Controls       │ 2. Background Daemon (voicyd)       │
│ - voicy toggle                   │ - In-memory cpal audio recording    │
│ - voicy toggle --dictate         │ - DSP Preprocessor (HPF + peak norm)│
│ - voicy target here / clear      │ - Sherpa-ONNX Distil-Whisper INT8   │
│ - voicy cancel (LIFO stack)      │ - Unix Event Broadcaster Socket     │
│ - voicy status / watch           │ - Multi-Channel Router (tmux/wtype) │
└──────────────────────────────────┴─────────────────────────────────────┘
```

---

## Prerequisites

1. **Rust Toolchain**: `cargo` and `rustc` (1.75+).
2. **Wayland & Audio Utilities**:
   ```bash
   # Arch Linux
   sudo pacman -S wl-clipboard wtype pipewire playerctl patchelf
   
   # Debian / Ubuntu
   sudo apt install wl-clipboard wtype pipewire playerctl patchelf
   ```
3. **Download Tools** (Optional for faster resumable downloads): `aria2c` or `wget`.

---

## Quick Installation

Run the automatic installer:

```bash
git clone https://github.com/rithikrathan/voicy.git
cd voicy
chmod +x install.sh
./install.sh
```

The installer will:
1. Verify system tools.
2. Auto-download and extract the pre-quantized Distil-Whisper model (~432MB) if missing.
3. Build the release binary with full optimizations and Link-Time Optimization (LTO).
4. Install the binary to `~/.local/bin/voicy`.
5. Install and start the systemd user service `voicyd.service`.

---

## Manual Model Download & Setup

If you prefer to download the models manually:

```bash
# Create models directory
mkdir -p /mnt/sda4/models/sherpa-onnx
cd /mnt/sda4/models/sherpa-onnx

# Download pre-quantized Distil-Whisper Small INT8
wget -c https://github.com/k2-fsa/sherpa-onnx/releases/download/asr-models/sherpa-onnx-whisper-distil-small.en.tar.bz2

# Extract
tar -xjf sherpa-onnx-whisper-distil-small.en.tar.bz2
```

---

## Configuration (`~/.config/voicy/config.toml`)

Voicy auto-generates a default configuration on first launch:

```toml
# Active model selection: matches profile key under [models]
active_model = "distil-small.en"

[models."distil-small.en"]
path = "/mnt/sda4/models/sherpa-onnx/sherpa-onnx-whisper-distil-small.en"
language = "en"
num_threads = 4

[models."small.en"]
path = "/mnt/sda4/models/sherpa-onnx/sherpa-onnx-whisper-small.en"
encoder = "small.en-encoder.int8.onnx"
decoder = "small.en-decoder.int8.onnx"
tokens = "small.en-tokens.txt"
language = "en"
num_threads = 4

[audio]
sample_rate = 16000
silence_threshold = 0.003
enable_high_pass = true
high_pass_cutoff = 80.0
enable_peak_norm = true
peak_target = 0.90
timeout_seconds = 30
```

---

## CLI Usage

### Recording & Dictation
```bash
# Toggle recording (routes to locked tmux target, or falls back to desktop dictation if null)
voicy toggle

# Force pure desktop dictation into the focused GUI window
voicy toggle --dictate

# Push-to-talk controls
voicy start
voicy stop

# Cancel topmost active task (LIFO: cancels active recording, or in-flight transcription)
voicy cancel
```

### Tmux Targeting
```bash
# Lock Voicy output onto the pane you are currently sitting in
voicy target here

# Lock to a specific pane or session
voicy target agent:0.1
voicy target %3

# Clear the target (revert to desktop dictation fallback)
voicy target clear
voicy target null

# Display the currently locked target
voicy target
```

---

## Unix Domain Event Socket (`voicy-events.sock`)

For external listener scripts, home server agent bridges, or terminal widgets:
- Socket location: `$XDG_RUNTIME_DIR/voicy-events.sock` (or `/tmp/voicy-events.sock`).
- Stream format: Newline-delimited JSON.

Example listener in Bash:
```bash
nc -U "$XDG_RUNTIME_DIR/voicy-events.sock" | while read -r line; do
    echo "Received transcription event: $line"
done
```

---

## Hyprland Keybindings

Add to `~/.config/hypr/keybinds.conf`:

```ini
# Voice-to-text toggle (Targeted / fallback)
bind = Super, D, exec, ~/.local/bin/md2tts-stop && ~/.local/bin/voicy toggle

# Pure desktop dictation
bind = Super+Alt, D, exec, ~/.local/bin/md2tts-stop && ~/.local/bin/voicy toggle --dictate

# LIFO Stack cancel
bind = Super+Shift, D, exec, ~/.local/bin/voicy cancel

# Push-to-talk on F12
bind = , F12, exec, ~/.local/bin/md2tts-stop && ~/.local/bin/voicy start
bindr = , F12, exec, ~/.local/bin/voicy stop
```

---

## License

MIT License.
