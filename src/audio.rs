use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::Stream;
use rubato::{FastFixedIn, PolynomialDegree, Resampler};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};

pub struct AudioRecorder {
    is_recording: Arc<AtomicBool>,
    stream: Option<Stream>,
    sample_rate: u32,
    silence_threshold: f32,
    pcm_sender: Option<Sender<Vec<f32>>>,
    chunker_handle: Option<std::thread::JoinHandle<()>>,
    raw_session_buffer: Arc<Mutex<Vec<f32>>>,
}

impl AudioRecorder {
    pub fn new(silence_threshold: f32) -> Result<Self, Box<dyn std::error::Error>> {
        let host = cpal::default_host();
        let device = host
            .default_input_device()
            .ok_or("No audio input device found")?;

        let config = device.default_input_config()?;
        let sample_rate = config.sample_rate().0;

        Ok(Self {
            is_recording: Arc::new(AtomicBool::new(false)),
            stream: None,
            sample_rate,
            silence_threshold,
            pcm_sender: None,
            chunker_handle: None,
            raw_session_buffer: Arc::new(Mutex::new(Vec::new())),
        })
    }

    pub fn start_streaming(
        &mut self,
        chunk_sender: Sender<Vec<f32>>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        if self.is_recording.load(Ordering::SeqCst) {
            return Ok(());
        }

        if let Ok(mut buf) = self.raw_session_buffer.lock() {
            buf.clear();
        }

        let host = cpal::default_host();
        let device = host
            .default_input_device()
            .ok_or("No audio input device found")?;
        let config = device.default_input_config()?;
        let channels = config.channels() as usize;
        let source_rate = self.sample_rate;
        let silence_thresh = self.silence_threshold;

        let (pcm_tx, pcm_rx) = std::sync::mpsc::channel::<Vec<f32>>();
        self.pcm_sender = Some(pcm_tx.clone());

        self.is_recording.store(true, Ordering::SeqCst);

        let raw_session_clone = Arc::clone(&self.raw_session_buffer);

        // Arithmetic Deterministic Chunker thread
        let handle = std::thread::spawn(move || {
            let mut resampler = if source_rate != 16000 {
                FastFixedIn::<f32>::new(
                    16000 as f64 / source_rate as f64,
                    2.0,
                    PolynomialDegree::Linear,
                    1024,
                    1,
                )
                .ok()
            } else {
                None
            };

            let mut input_resample_buf: Vec<f32> = Vec::with_capacity(2048);
            let mut active_chunk: Vec<f32> = Vec::with_capacity(16000 * 25);
            let mut trailing_silence_count: usize = 0;

            // Frame size for RMS calculation: 30ms @ 16kHz = 480 samples
            const FRAME_SIZE: usize = 480;
            // Pad silence allowance around words: 200ms = ~6-7 frames
            const SILENCE_PAD_FRAMES: usize = 7;
            // Dispatch target: between 15s (240k samples) and max 24s (384k samples)
            const MIN_CHUNK_DISPATCH_SAMPLES: usize = 16000 * 12; // 12 seconds
            const MAX_CHUNK_SAMPLES: usize = 16000 * 24;          // 24 seconds cap

            let mut frame_acc: Vec<f32> = Vec::with_capacity(FRAME_SIZE);

            while let Ok(raw_block) = pcm_rx.recv() {
                // Resample block to 16kHz
                let samples_16k = if let Some(ref mut r) = resampler {
                    input_resample_buf.extend_from_slice(&raw_block);
                    let mut out = Vec::new();
                    while input_resample_buf.len() >= 1024 {
                        let block = input_resample_buf.drain(..1024).collect::<Vec<f32>>();
                        if let Ok(res) = r.process(&[block], None) {
                            if let Some(ch0) = res.first() {
                                out.extend_from_slice(ch0);
                            }
                        }
                    }
                    out
                } else {
                    raw_block
                };

                if samples_16k.is_empty() {
                    continue;
                }

                // Append to raw session recorder
                if let Ok(mut buf) = raw_session_clone.lock() {
                    buf.extend_from_slice(&samples_16k);
                }

                // Process in 30ms frames for silence detection
                for &s in &samples_16k {
                    frame_acc.push(s);
                    if frame_acc.len() == FRAME_SIZE {
                        // Calculate RMS energy of this 30ms frame
                        let sum_sq: f32 = frame_acc.iter().map(|&x| x * x).sum();
                        let rms = (sum_sq / FRAME_SIZE as f32).sqrt();

                        let is_speech = rms >= silence_thresh;

                        if is_speech {
                            trailing_silence_count = 0;
                            active_chunk.extend_from_slice(&frame_acc);
                        } else {
                            // Silence frame
                            if !active_chunk.is_empty() {
                                trailing_silence_count += 1;
                                // Keep up to 200ms trailing padding to avoid clipping word decay
                                if trailing_silence_count <= SILENCE_PAD_FRAMES {
                                    active_chunk.extend_from_slice(&frame_acc);
                                }
                            }
                        }

                        // Check if we should dispatch the chunk
                        // 1. Natural pause slice: chunk has >= 12s of speech AND silence lasting >= 600ms (20 frames)
                        let natural_pause = active_chunk.len() >= MIN_CHUNK_DISPATCH_SAMPLES
                            && trailing_silence_count >= 20;

                        // 2. Hard safety ceiling: reached 24 seconds of accumulated speech
                        let hard_ceiling = active_chunk.len() >= MAX_CHUNK_SAMPLES;

                        if natural_pause || hard_ceiling {
                            let chunk_to_send = std::mem::replace(
                                &mut active_chunk,
                                Vec::with_capacity(16000 * 25),
                            );
                            if chunk_to_send.len() >= 16000 {
                                // >= 1.0 second of audio
                                let _ = chunk_sender.send(chunk_to_send);
                            }
                            trailing_silence_count = 0;
                        }

                        frame_acc.clear();
                    }
                }
            }

            // Flush remaining frame
            if !frame_acc.is_empty() && !active_chunk.is_empty() {
                active_chunk.extend_from_slice(&frame_acc);
            }

            // Flush final accumulated chunk on session stop
            if !active_chunk.is_empty() {
                // Must have at least 0.25 seconds of speech
                if active_chunk.len() >= 4000 {
                    let _ = chunk_sender.send(active_chunk);
                }
            }
        });

        self.chunker_handle = Some(handle);

        let is_recording_capture = Arc::clone(&self.is_recording);
        let err_fn = |err| eprintln!("Audio stream error: {}", err);

        let stream = match config.sample_format() {
            cpal::SampleFormat::F32 => device.build_input_stream(
                &config.into(),
                move |data: &[f32], _| {
                    if is_recording_capture.load(Ordering::SeqCst) {
                        let mut block = Vec::with_capacity(data.len() / channels);
                        for chunk in data.chunks(channels) {
                            let mono = chunk.iter().sum::<f32>() / channels as f32;
                            block.push(mono);
                        }
                        let _ = pcm_tx.send(block);
                    }
                },
                err_fn,
                None,
            )?,
            cpal::SampleFormat::I16 => device.build_input_stream(
                &config.into(),
                move |data: &[i16], _| {
                    if is_recording_capture.load(Ordering::SeqCst) {
                        let mut block = Vec::with_capacity(data.len() / channels);
                        for chunk in data.chunks(channels) {
                            let mono_i16 =
                                chunk.iter().map(|&x| x as f32).sum::<f32>() / channels as f32;
                            block.push(mono_i16 / 32768.0);
                        }
                        let _ = pcm_tx.send(block);
                    }
                },
                err_fn,
                None,
            )?,
            cpal::SampleFormat::U16 => device.build_input_stream(
                &config.into(),
                move |data: &[u16], _| {
                    if is_recording_capture.load(Ordering::SeqCst) {
                        let mut block = Vec::with_capacity(data.len() / channels);
                        for chunk in data.chunks(channels) {
                            let mono_u16 = chunk.iter().map(|&x| x as f32 - 32768.0).sum::<f32>()
                                / channels as f32;
                            block.push(mono_u16 / 32768.0);
                        }
                        let _ = pcm_tx.send(block);
                    }
                },
                err_fn,
                None,
            )?,
            _ => return Err("Unsupported sample format".into()),
        };

        stream.play()?;
        self.stream = Some(stream);
        Ok(())
    }

    pub fn stop(&mut self) -> Vec<f32> {
        self.is_recording.store(false, Ordering::SeqCst);
        self.stream = None;
        self.pcm_sender = None;

        if let Some(handle) = self.chunker_handle.take() {
            let _ = handle.join();
        }

        self.raw_session_buffer
            .lock()
            .map(|mut b| std::mem::take(&mut *b))
            .unwrap_or_default()
    }

    pub fn cancel(&mut self) {
        self.is_recording.store(false, Ordering::SeqCst);
        self.stream = None;
        self.pcm_sender = None;
        if let Some(handle) = self.chunker_handle.take() {
            let _ = handle.join();
        }
        if let Ok(mut buf) = self.raw_session_buffer.lock() {
            buf.clear();
        }
    }

    pub fn is_recording(&self) -> bool {
        self.is_recording.load(Ordering::SeqCst)
    }
}
