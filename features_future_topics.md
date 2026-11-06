# Voicy: Future Roadmap & Features

## 1. Dynamic Audio Chunking (Removing the 30-Second Limit)

> **Status**: Completed (v1.2)  
> **Priority**: Resolved

Implemented in v1.2 using an arithmetic RMS silence-valley chunker in `src/audio.rs`:
- Strips long dead silence gaps in real time.
- Accumulates active voice into dense chunks and slices at natural speech pauses ($\ge 12\text{s}$ speech with $\ge 600\text{ms}$ pause).
- Enforces a 24-second hard safety ceiling to stay well inside Whisper's 30.0s embedding window.
- Automatically submits an Enter keystroke to tmux or active Wayland window when recording finishes, and consolidates the entire session into the system clipboard and `voicy-events.sock`.

---

## 2. Upcoming Features & Exploration

### A. Ambient Noise Floor Auto-Calibration
- **Status**: Backlog
- **Description**: On daemon startup or mic activation, sample 200–500ms of ambient room audio to dynamically calibrate `silence_threshold` instead of requiring manual configuration tuning in noisy or quiet environments.

### B. Optional Lightweight Neural VAD Alternative Evaluation
- **Status**: Backlog
- **Description**: Benchmark external non-Sherpa VAD alternatives (e.g. lightweight WebRTC VAD or Ten-VAD) against arithmetic RMS profiling for high-noise coffee shop environments, provided they do not introduce false phoneme truncation.

### C. Streaming Transducer Engine (Zipformer / Next-gen ASR)
- **Status**: Research
- **Description**: Support an optional true online streaming transducer backend for token-by-token character typing with sub-100ms latency alongside the Whisper offline engine.
