# Build & Installation Notes

## Dynamic Linker RUNPATH Requirement

The `voicy` binary dynamically links against `libsherpa-onnx-c-api.so`, which resides in `~/.local/lib/`.

When installed to `~/.local/bin/voicy`, the binary requires an embedded ELF `RUNPATH` of `$ORIGIN/../lib` so keybindings and desktop environments can execute it directly without an explicit `LD_LIBRARY_PATH` environment variable.

### Option 1: Compile with Linker Flag (Recommended)
```bash
RUSTFLAGS="-C link-args=-Wl,-rpath,\$ORIGIN/../lib" cargo build --release
```

Or configure in `.cargo/config.toml`:
```toml
[target.x86_64-unknown-linux-gnu]
rustflags = ["-C", "link-args=-Wl,-rpath,$ORIGIN/../lib"]
```

### Option 2: Patch Existing Binary
```bash
patchelf --set-rpath '$ORIGIN/../lib' ~/.local/bin/voicy
```

### Installation Workflow
```bash
systemctl --user stop voicyd
install -m 755 ./target/release/voicy ~/.local/bin/voicy
patchelf --set-rpath '$ORIGIN/../lib' ~/.local/bin/voicy
systemctl --user start voicyd
```
