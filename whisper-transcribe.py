#!/usr/bin/env python3
import os
import sys
import wave
import numpy as np
import sherpa_onnx

if len(sys.argv) < 2:
    sys.exit(1)

audio_path = sys.argv[1]
if not os.path.exists(audio_path):
    sys.exit(1)

model_dir = "/mnt/sda4/models/sherpa-onnx/sherpa-onnx-whisper-small.en"

recognizer = sherpa_onnx.OfflineRecognizer.from_whisper(
    encoder=f"{model_dir}/small.en-encoder.int8.onnx",
    decoder=f"{model_dir}/small.en-decoder.int8.onnx",
    tokens=f"{model_dir}/small.en-tokens.txt",
    language="en",
    task="transcribe",
    num_threads=4,
)

with wave.open(audio_path) as f:
    n_frames = f.getnframes()
    rate = f.getframerate()
    raw_audio = f.readframes(n_frames)
    samples = np.frombuffer(raw_audio, dtype=np.int16).astype(np.float32) / 32768.0

stream = recognizer.create_stream()
stream.accept_waveform(rate, samples)
recognizer.decode_stream(stream)
print(stream.result.text.strip())
