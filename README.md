# Voicy

> High-performance voice-to-text (STT) daemon and on-device lookahead speech synthesis (TTS) for Linux Wayland desktops.

> [!NOTE]
> **Dataset Logging Notice**: The daemon can automatically log all transcriptions to a file for dataset collection and future fine-tuning. This path is fully configurable in `~/.config/voicy/config.toml` under `log_file`. If set to `""` or omitted, transcription logging is completely disabled.
>
> **Uncapped Continuous Dictation**: Voicy features a zero-overhead dynamic arithmetic audio chunker. Dead silence is stripped in real time, and active speech is cleanly sliced at natural speech pauses into $\le 24$-second dense chunks, eliminating Whisper's 30-second window limit. You can dictate indefinitely.
>
> **On-Device Lookahead TTS**: Voicy embeds a pure-Rust Gradium Phonon-31M engine with 102 offline voices. The speech engine operates as a two-stage lookahead pipeline: foreground audio plays without delay while background synthesis prepares subsequent items ahead of time. Zero temp disk files or trash are produced.

Voicy is an all-in-one voice-to-text and text-to-speech daemon for Linux Wayland desktops. It records on demand, streams and transcribes speech locally with Whisper, and incrementally injects text where you want it: into the focused window, into a specific tmux pane, to the clipboard, or out over a Unix socket for other programs to consume. It also serves on-device speech synthesis directly from the background daemon.

No cloud service, no audio written to disk, no window stealing. Built in Rust against PipeWire, `sherpa-onnx`, `ptts` (Phonon 31M), and standard Wayland tools.

---

## What it does

- **Real-time streaming dictation.** Speak continuously without a 30-second cap. Speech chunks are processed and typed sequentially into your prompt, with an automatic Enter key submitted upon session finish.
- **Local transcription** with a quantized Distil-Whisper model via `sherpa-onnx`. Runs on CPU at roughly 0.3–0.4× real-time, with the vocab coverage of Whisper Large's distillation.
- **On-device lookahead TTS pipeline.** Native local speech synthesis with 102 offline voices (`Alba` default, `Noa`, `Teddy`, `Spencer`, `Alfie`, `Edwin`, etc.). Synthesis prepares upcoming speech in the background while audio streams without clipping or hesitation.
- **Dynamic arithmetic silence-valley chunking.** Slices audio at natural pauses with zero neural network overhead (<0.1% CPU), retaining decay buffers to prevent clipped words.
- **Pick your model** in `~/.config/voicy/config.toml`: `distil-small.en` by default for STT, `small.en` also supported. Phonon-31M for TTS. Model files are discovered automatically.
- **tmux routing.** `voicy target here` locks output to the pane you're in. Text is injected with `tmux send-keys`; focus never moves.
- **Desktop dictation.** `voicy toggle --dictate` types into whatever Wayland window currently has focus using `wtype`.
- **Event socket & Clipboard consolidation.** `voicy-events.sock` streams consolidated JSON at session completion, and the entire recorded transcript is copied to the system clipboard upon finish.
- **LIFO cancel & TTS Queue purge.** `voicy cancel` stops the active recording if there is one. `voicy tts --queue-free` cancels current playback and instantly flushes all pending TTS jobs.

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
│ - voicy tts "<text>"             │ - Gradium Phonon 31M Pure-Rust TTS  │
│ - voicy tts --stop / --queue-free│ - Pipelined lookahead audio player  │
│ - voicy cancel                   │ - Unix event broadcaster            │
│ - voicy status / watch           │ - Router: tmux, wtype, clipboard    │
└──────────────────────────────────┴─────────────────────────────────────┘
```

The CLI talks to the daemon over a Unix socket. Nothing is transcribed or synthesized inside the CLI process.

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

### Text-to-Speech (TTS)

```bash
# Speak text with the default voice (Alba)
voicy tts "Hello! System status is completely normal."

# Speak text with a specific voice (e.g. Noa, Teddy, Spencer, Alfie)
voicy tts "Good evening, sir." --voice Teddy

# Halt current speech playback
voicy tts --stop

# Stop playback and clear all pending speech jobs
voicy tts --queue-free
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
voicy status   # one-shot state: idle, recording, transcribing, typing, speaking
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
