# Voicy: Dynamic Audio Chunking (Removing the 30-Second Limit)

> **Status**: Backlog  
> **Priority**: Low

This document specifies the technical design for audio chunking to eliminate Whisper's 30-second window truncation.

---

## 1. Problem Statement

Whisper models use positional embeddings constrained to a maximum of 30.0 seconds (480,000 samples at 16kHz). In `sherpa-onnx`, passing an audio buffer exceeding 30 seconds triggers truncation:

```text
Only waves less than 30 seconds are supported. We process only the first 30 seconds and discard the remaining data
```

Any speech spoken past the 30-second mark is permanently lost.

---

## 2. Proposed Architecture: Native Silence-Valley Splitting

Rather than introducing an external neural VAD model, Voicy will perform native silence-valley detection directly in `src/audio_preprocessor.rs` using RMS energy profiling:

### A. Short Audio Bypass ($\le 28$ seconds)
- When recorded audio is under 28 seconds, it passes directly to Distil-Whisper in a single shot with zero overhead or latency change.

### B. Dynamic Silence-Valley Slicing ($> 28$ seconds)
- When audio exceeds 28 seconds, Voicy scans a candidate window (between 22.0s and 27.5s) for a natural speech pause (an RMS energy valley lasting $> 150\text{ms}$).
- Slices the audio cleanly at the lowest energy point in that valley, ensuring no spoken word or phoneme is cut in half.
- If the remaining audio buffer is still longer than 28 seconds, the slicing repeats recursively.

### C. Multi-Segment Sequential Decoding
- Each $\le 28$-second segment is passed sequentially through `WhisperEngine::transcribe()`.
- The resulting text transcriptions are stitched together with single spacing, preserving native punctuation and capitalization across segments.
- The unified, complete text is delivered in one single operation to the target destination (tmux pane or active window).

### D. Timeout Adjustment
- The 30-second recording safety alert timer (`timeout.mp3`) is extended to a longer threshold (e.g. 2 to 5 minutes) or made configurable in `config.toml`.
