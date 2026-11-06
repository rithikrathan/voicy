#!/usr/bin/env bash
set -e

echo "=== Voicy Automatic Installer ==="

# 1. Check prerequisites
MISSING_DEPS=()
for cmd in cargo rustc wl-copy wtype pw-play playerctl python3; do
    if ! command -v "$cmd" >/dev/null 2>&1; then
        MISSING_DEPS+=("$cmd")
    fi
done

if [ ${#MISSING_DEPS[@]} -gt 0 ]; then
    echo "⚠️ Warning: The following dependencies were not found in PATH:"
    for dep in "${MISSING_DEPS[@]}"; do
        echo "   - $dep"
    done
    echo "Please ensure they are installed for full functionality (wl-clipboard, wtype, pipewire, playerctl, python3)."
fi

# 2. Destination directories
BIN_DIR="$HOME/.local/bin"
LIB_DIR="$HOME/.local/lib/voicy"
SHARE_DIR="$HOME/.local/share/voicy/assets"
MODELS_DIR="$HOME/.local/share/voicy/models"
SERVICE_DIR="$HOME/.config/systemd/user"
CONFIG_DIR="$HOME/.config/voicy"
WIREPLUMBER_DIR="$HOME/.config/wireplumber/wireplumber.conf.d"

mkdir -p "$BIN_DIR" "$LIB_DIR" "$SHARE_DIR" "$MODELS_DIR" "$SERVICE_DIR" "$CONFIG_DIR" "$WIREPLUMBER_DIR"

# 3. Prevent Linux audio DAC wake-up clipping (disable WirePlumber ALSA suspend)
echo "Configuring WirePlumber to prevent audio power-saving clipping..."
cat << 'EOF' > "$WIREPLUMBER_DIR/51-disable-suspend.conf"
monitor.alsa.rules = [
  {
    matches = [
      { node.name = "~alsa_output.*" }
    ],
    actions = {
      update-props = {
        session.suspend-timeout-seconds = 0
      }
    }
  }
]
EOF
systemctl --user restart wireplumber pipewire 2>/dev/null || true

# 4. Whisper STT Models
DISTIL_HOST_DIR="/mnt/sda4/models/sherpa-onnx/sherpa-onnx-whisper-distil-small.en"
FALLBACK_STT_DIR="$MODELS_DIR/sherpa-onnx-whisper-distil-small.en"

if [ -d "$DISTIL_HOST_DIR" ]; then
    echo "Found Distil-Whisper model in $DISTIL_HOST_DIR"
elif [ -d "$FALLBACK_STT_DIR" ]; then
    echo "Found Distil-Whisper model in $FALLBACK_STT_DIR"
else
    echo "Distil-Whisper model not found locally. Downloading pre-quantized INT8 model (~432MB)..."
    MODEL_URL="https://github.com/k2-fsa/sherpa-onnx/releases/download/asr-models/sherpa-onnx-whisper-distil-small.en.tar.bz2"
    ARCHIVE_PATH="$MODELS_DIR/sherpa-onnx-whisper-distil-small.en.tar.bz2"

    if command -v aria2c >/dev/null 2>&1; then
        aria2c -c -x 4 -s 4 --auto-file-renaming=false -d "$MODELS_DIR" -o "sherpa-onnx-whisper-distil-small.en.tar.bz2" "$MODEL_URL"
    elif command -v wget >/dev/null 2>&1; then
        wget -c -O "$ARCHIVE_PATH" "$MODEL_URL"
    else
        curl -L -C - -o "$ARCHIVE_PATH" "$MODEL_URL"
    fi

    echo "Extracting model archive to $MODELS_DIR..."
    tar -xjf "$ARCHIVE_PATH" -C "$MODELS_DIR"
    rm -f "$ARCHIVE_PATH"
    echo "Model ready at $FALLBACK_STT_DIR"
fi

# Silero VAD verification
VAD_MODEL_PATH="/mnt/sda4/models/sherpa-onnx/silero_vad.onnx"
FALLBACK_VAD_PATH="$MODELS_DIR/silero_vad.onnx"
if [ -f "$VAD_MODEL_PATH" ]; then
    echo "Found Silero VAD model in $VAD_MODEL_PATH"
elif [ -f "$FALLBACK_VAD_PATH" ]; then
    echo "Found Silero VAD model in $FALLBACK_VAD_PATH"
else
    echo "Silero VAD model not found locally. Downloading (~640KB)..."
    VAD_URL="https://github.com/k2-fsa/sherpa-onnx/releases/download/asr-models/silero_vad.onnx"
    curl -L -o "$FALLBACK_VAD_PATH" "$VAD_URL"
    echo "Silero VAD ready at $FALLBACK_VAD_PATH"
fi

# 5. Phonon TTS Models & 102 Voices
PHONON_HOST_DIR="/mnt/sda4/models/phonon-31m-en"
FALLBACK_TTS_DIR="$MODELS_DIR/phonon-31m-en"

TARGET_TTS_DIR="$FALLBACK_TTS_DIR"
if [ -d "$PHONON_HOST_DIR" ]; then
    TARGET_TTS_DIR="$PHONON_HOST_DIR"
fi

mkdir -p "$TARGET_TTS_DIR/voices"
echo "Verifying Phonon-31M TTS base models in $TARGET_TTS_DIR..."

BASE_HF_URL="https://huggingface.co/gradium/phonon-31m-en/resolve/main"
for file in config.json default-voice.safetensors model.q8.gguf tokenizer.json; do
    if [ ! -f "$TARGET_TTS_DIR/$file" ]; then
        echo "Downloading $file from $BASE_HF_URL/$file..."
        curl -L -o "$TARGET_TTS_DIR/$file" "$BASE_HF_URL/$file"
    fi
done

# Download all 102 catalog voices if empty
VOICE_COUNT=$(find "$TARGET_TTS_DIR/voices" -name "*.safetensors" | wc -l)
if [ "$VOICE_COUNT" -lt 50 ]; then
    echo "Downloading 102 Phonon voice safetensors into $TARGET_TTS_DIR/voices/..."
    cd "$TARGET_TTS_DIR/voices"
    curl -s https://huggingface.co/gradium/phonon-voices/raw/main/voices.jsonl | python3 -c "
import sys, json, urllib.request, os
voices = []
for line in sys.stdin:
    if not line.strip(): continue
    d = json.loads(line)
    voices.append((d['name'], d['voice_id']))

print(f'Downloading {len(voices)} voices...')
for name, vid in voices:
    safe_name = name.replace('/', '_').replace(' ', '_')
    fn = f'{safe_name}.safetensors'
    if not os.path.exists(fn):
        url = f'https://huggingface.co/gradium/phonon-voices/resolve/main/voices/{vid}.safetensors'
        try:
            urllib.request.urlretrieve(url, fn)
            print(f'Downloaded {safe_name}')
        except Exception as e:
            print(f'Failed {safe_name}: {e}')
"
    cd - >/dev/null
    echo "Voices download complete ($(find "$TARGET_TTS_DIR/voices" -name "*.safetensors" | wc -l) voices installed)."
else
    echo "Found $VOICE_COUNT voices already installed in $TARGET_TTS_DIR/voices/."
fi

# 6. Build Voicy Release Binary
echo "Building Voicy in release mode..."
cargo build --release

# 7. Install audio assets
echo "Installing UI audio assets to $SHARE_DIR..."
mkdir -p "$SHARE_DIR/ui"
cp -a src/assets/ui/* "$SHARE_DIR/ui/" 2>/dev/null || true

# 8. Copy shared libraries
echo "Setting up runtime shared libraries..."
CACHE_LIB_DIR=$(find "$HOME/.cache/sherpa-rs" -name "libsherpa-onnx-c-api.so" 2>/dev/null | head -n 1 | xargs -r dirname)
if [ -n "$CACHE_LIB_DIR" ] && [ -d "$CACHE_LIB_DIR" ]; then
    cp -a "$CACHE_LIB_DIR"/* "$LIB_DIR/"
fi

# 9. Install binary and patch rpath
echo "Installing binary to $BIN_DIR/voicy..."
systemctl --user stop voicyd.service 2>/dev/null || true
fuser -k "$BIN_DIR/voicy" 2>/dev/null || true
install -m 755 target/release/voicy "$BIN_DIR/voicy"

if command -v patchelf >/dev/null 2>&1; then
    patchelf --set-rpath '$ORIGIN/../lib/voicy:$ORIGIN' "$BIN_DIR/voicy" 2>/dev/null || true
fi

# 10. Install and enable systemd user service
echo "Configuring systemd user service..."
cat << 'EOF' > "$SERVICE_DIR/voicyd.service"
[Unit]
Description=Voicy Rust Whisper STT & Phonon TTS Daemon
After=pipewire.service sound.target graphical-session.target
PartOf=graphical-session.target

[Service]
Type=simple
ExecStart=%h/.local/bin/voicy daemon
Restart=on-failure
RestartSec=1
PassEnvironment=WAYLAND_DISPLAY XDG_RUNTIME_DIR

[Install]
WantedBy=default.target
EOF

systemctl --user daemon-reload
systemctl --user enable --now voicyd.service

echo ""
echo "✅ Voicy installed and daemon started successfully!"
echo "Daemon status: $(systemctl --user is-active voicyd.service)"
echo ""
echo "Quick start:"
echo "   voicy toggle                 # Toggle live recording (routes to locked tmux target or dictation fallback)"
echo "   voicy toggle --dictate       # Pure desktop dictation into focused Wayland window"
echo "   voicy target here            # Lock speech output directly into current tmux pane"
echo "   voicy target clear           # Clear target (revert to dictation fallback)"
echo "   voicy cancel                 # LIFO cancel (cancels active recording, or in-flight transcription)"
echo "   voicy tts \"Hello world\"     # Pipelined lookahead TTS"
echo "   voicy tts --stop             # Stop currently playing speech"
echo "   voicy tts --queue-free       # Halt speech and purge upcoming queue"
echo "   voicy status                 # View current state"
