#!/usr/bin/env python3
import os
import sys
import wave
import numpy as np
import sherpa_onnx
from http.server import HTTPServer, BaseHTTPRequestHandler

MODEL_DIR = "/mnt/sda4/models/sherpa-onnx/sherpa-onnx-whisper-small.en"
PORT = 29541

print("Loading Whisper Small INT8 model into memory...", flush=True)
recognizer = sherpa_onnx.OfflineRecognizer.from_whisper(
    encoder=f"{MODEL_DIR}/small.en-encoder.int8.onnx",
    decoder=f"{MODEL_DIR}/small.en-decoder.int8.onnx",
    tokens=f"{MODEL_DIR}/small.en-tokens.txt",
    language="en",
    task="transcribe",
    num_threads=4,
)
print(f"Daemon ready! Listening on 127.0.0.1:{PORT}", flush=True)

class TranscribeHandler(BaseHTTPRequestHandler):
    def log_message(self, format, *args):
        pass  # Silent, no stderr spam

    def do_POST(self):
        try:
            content_length = int(self.headers.get('Content-Length', 0))
            audio_path = self.rfile.read(content_length).decode('utf-8').strip()

            if not audio_path or not os.path.exists(audio_path):
                self.send_response(400)
                self.end_headers()
                return

            with wave.open(audio_path) as f:
                n_frames = f.getnframes()
                rate = f.getframerate()
                raw_audio = f.readframes(n_frames)
                samples = np.frombuffer(raw_audio, dtype=np.int16).astype(np.float32) / 32768.0

            stream = recognizer.create_stream()
            stream.accept_waveform(rate, samples)
            recognizer.decode_stream(stream)
            text = stream.result.text.strip()

            self.send_response(200)
            self.send_header('Content-Type', 'text/plain; charset=utf-8')
            self.end_headers()
            self.wfile.write(text.encode('utf-8'))
        except Exception:
            self.send_response(500)
            self.end_headers()

httpd = HTTPServer(('127.0.0.1', PORT), TranscribeHandler)
httpd.serve_forever()
