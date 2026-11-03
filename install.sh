#!/usr/bin/env bash
set -e

echo "=== Voicy Automatic Installer ==="

# 1. Check prerequisites
MISSING_DEPS=()
for cmd in cargo rustc wl-copy wtype pw-play playerctl; do
    if ! command -v "$cmd" >/dev/null 2>&1; then
        MISSING_DEPS+=("$cmd")
    fi
done

if [ ${#MISSING_DEPS[@]} -gt 0 ]; then
    echo "⚠️ Warning: The following dependencies were not found in PATH:"
    for dep in "${MISSING_DEPS[@]}"; do
        echo "   - $dep"
    done
    echo "Please ensure they are installed for full functionality (wl-clipboard, wtype, pipewire, playerctl)."
fi

# 2. Build release binary
echo "Building Voicy in release mode..."
cargo build --release

# 3. Create destination directories
BIN_DIR="$HOME/.local/bin"
LIB_DIR="$HOME/.local/lib/voicy"
SHARE_DIR="$HOME/.local/share/voicy/assets"
SERVICE_DIR="$HOME/.config/systemd/user"

mkdir -p "$BIN_DIR" "$LIB_DIR" "$SHARE_DIR" "$SERVICE_DIR"

# 4. Install audio assets
echo "Installing UI audio assets to $SHARE_DIR..."
mkdir -p "$SHARE_DIR/ui"
cp -a src/assets/ui/* "$SHARE_DIR/ui/" 2>/dev/null || true

# 5. Copy shared libraries if present in cache/vendor
echo "Setting up runtime shared libraries..."
CACHE_LIB_DIR=$(find "$HOME/.cache/sherpa-rs" -name "libsherpa-onnx-c-api.so" 2>/dev/null | head -n 1 | xargs -r dirname)
if [ -n "$CACHE_LIB_DIR" ] && [ -d "$CACHE_LIB_DIR" ]; then
    cp -a "$CACHE_LIB_DIR"/* "$LIB_DIR/"
fi

# 5. Install binary and patch rpath
echo "Installing binary to $BIN_DIR/voicy..."
systemctl --user stop voicyd.service 2>/dev/null || true
install -m 755 target/release/voicy "$BIN_DIR/voicy"

if command -v patchelf >/dev/null 2>&1; then
    patchelf --set-rpath '$ORIGIN/../lib/voicy' "$BIN_DIR/voicy" 2>/dev/null || true
fi

# 7. Install and enable systemd user service
echo "Configuring systemd user service..."
cat << 'EOF' > "$SERVICE_DIR/voicyd.service"
[Unit]
Description=Voicy Rust Whisper STT Daemon
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
echo "   voicy toggle   # Toggle recording"
echo "   voicy cancel   # Discard active recording"
echo "   voicy status   # View current state"
