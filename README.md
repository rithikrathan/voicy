# Voicy

Voicy is a voice-to-text daemon for Linux Wayland desktops. It records on demand, transcribes speech locally with Whisper, and sends the text where you want it: into the focused window, into a specific tmux pane, to the clipboard, or out over a Unix socket for other programs to consume.

No cloud service, no audio written to disk, no window stealing. Built in Rust against PipeWire, `sherpa-onnx`, and standard Wayland tools.

---

## What it does

- **Local transcription** with a quantized Distil-Whisper model via `sherpa-onnx`. Runs on CPU at roughly 0.3–0.4× real-time, with the vocab coverage of Whisper Large's distillation.
- **Pick your model** in `~/.config/voicy/config.toml`: `distil-small.en` by default, `small.en` also supported. Model files are discovered automatically.
- **tmux routing.** `voicy target here` locks output to the pane you're in. Text is injected with `tmux send-keys`; focus never moves.
- **Desktop dictation.** `voicy toggle --dictate` types into whatever Wayland window currently has focus using `wtype`.
- **Event socket.** `voicy-events.sock` streams newline-delimited JSON to any number of listeners, so status bars, Python scripts, or home-server agents can react in real time.
- **LIFO cancel.** `voicy cancel` stops the active recording if there is one. Otherwise it cancels an in-flight transcription. Background work is left alone.
- **Output guarantees.** Every result is copied to the clipboard and appended to a transcript log, regardless of which delivery channel is used.

---

## How it fits together

```text
┌────────────────────────────────────────────────────────────────────────┐
│                          voicy (single binary)                         │
├──────────────────────────────────┬─────────────────────────────────────┤
│ 1. CLI                           │ 2. Daemon (voicyd)                  │
│ - voicy toggle                   │ - cpal audio capture, in memory     │
│ - voicy toggle --dictate         │ - DSP: high-pass + peak normalize   │
│ - voicy target here / clear      │ - Sherpa-ONNX Distil-Whisper INT8   │
│ - voicy cancel                   │ - Unix event broadcaster            │
│ - voicy status / watch           │ - Router: tmux, wtype, clipboard    │
└──────────────────────────────────┴─────────────────────────────────────┘
```

The CLI talks to the daemon over a Unix socket. Nothing is transcribed inside the CLI process.

---

## Requirements

- Rust 1.75 or newer (`cargo`, `rustc`).
- A Wayland compositor.
- `wl-clipboard`, `wtype`, `pipewire`, `playerctl`, and `patchelf` for install patching.
- `aria2c` or `wget` if you want resumable model downloads.

```bash
# Arch
sudo pacman -S wl-clipboard wtype pipewire playerctl patchelf

# Debian / Ubuntu
sudo apt install wl-clipboard wtype pipewire playerctl patchelf
```

---

## Install

```bash
git clone https://github.com/rithikrathan/voicy.git
cd voicy
chmod +x install.sh
./install.sh
```

The installer checks dependencies, downloads the quantized Distil-Whisper model (~432 MB) if it isn't already present, builds with LTO, installs the binary to `~/.local/bin/voicy`, and enables the `voicyd` systemd user service.

### Model download by hand

```bash
mkdir -p /mnt/sda4/models/sherpa-onnx
cd /mnt/sda4/models/sherpa-onnx
wget -c https://github.com/k2-fsa/sherpa-onnx/releases/download/asr-models/sherpa-onnx-whisper-distil-small.en.tar.bz2
tar -xjf sherpa-onnx-whisper-distil-small.en.tar.bz2
```

If you put models elsewhere, update the paths in the config.

---

## Configuration

Voicy writes a default config to `~/.config/voicy/config.toml` on first run.

```toml
# Must match a profile key under [models]
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

## Usage

### Recording and dictation

```bash
# Start if idle, stop and deliver if recording.
# Goes to the locked tmux target, or the focused window if none is set.
voicy toggle

# Ignore tmux entirely; type into the focused GUI window.
voicy toggle --dictate

# Push-to-talk
voicy start
voicy stop

# Cancel the topmost task: active recording first, otherwise in-flight transcription.
voicy cancel
```

### tmux targeting

```bash
# Lock output to the pane you are currently in.
voicy target here

# Lock to a specific session, window, or pane.
voicy target agent:0.1
voicy target %3

# Clear the lock; output falls back to desktop dictation.
voicy target clear
voicy target null

# Show the current target.
voicy target
```

### Status

```bash
voicy status   # one-shot state
voicy watch    # live JSON updates for waybar / ashell
```

---

## Event socket

Socket path: `$XDG_RUNTIME_DIR/voicy-events.sock`, with `/tmp/voicy-events.sock` as fallback.

Events are newline-delimited JSON. Any number of clients can connect at once.

```bash
nc -U "$XDG_RUNTIME_DIR/voicy-events.sock" | while read -r line; do
    echo "$line"
done
```

---

## Hyprland keybindings

Add to `~/.config/hypr/keybinds.conf`:

```ini
# Toggle recording (targeted, or dictation fallback)
bind = Super, D, exec, ~/.local/bin/md2tts-stop && ~/.local/bin/voicy toggle

# Desktop-only dictation
bind = Super+Alt, D, exec, ~/.local/bin/md2tts-stop && ~/.local/bin/voicy toggle --dictate

# Cancel
bind = Super+Shift, D, exec, ~/.local/bin/voicy cancel

# Push-to-talk
bind = , F12, exec, ~/.local/bin/md2tts-stop && ~/.local/bin/voicy start
bindr = , F12, exec, ~/.local/bin/voicy stop
```

The `md2tts-stop` prefix stops any currently playing TTS before recording starts. Drop it if you don't use that tool.

---

## License

MIT.
