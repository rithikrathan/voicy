use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::Stream;
use rubato::{FastFixedIn, PolynomialDegree, Resampler};
use sherpa_rs::silero_vad::{SileroVad, SileroVadConfig};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};
use std::time::Instant;

pub struct AudioRecorder {
    is_recording: Arc<AtomicBool>,
    stream: Option<Stream>,
    sample_rate: u32,
    vad_config: SileroVadConfig,
    sender: Option<Sender<Vec<f32>>>,
    vad_worker_handle: Option<std::thread::JoinHandle<()>>,
    raw_buffer: Arc<Mutex<Vec<f32>>>,
}

impl AudioRecorder {
    pub fn new(
        vad_model_path: &Path,
        min_silence_duration: f32,
        min_speech_duration: f32,
        threshold: f32,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let host = cpal::default_host();
        let device = host
            .default_input_device()
            .ok_or("No audio input device found")?;

        let config = device.default_input_config()?;
        let sample_rate = config.sample_rate().0;

        let mut vad_config = SileroVadConfig::default();
        vad_config.model = vad_model_path.to_string_lossy().to_string();
        vad_config.min_silence_duration = min_silence_duration;
        vad_config.min_speech_duration = min_speech_duration;
        vad_config.threshold = threshold;
        vad_config.sample_rate = 16000;
        vad_config.window_size = 512;

        Ok(Self {
            is_recording: Arc::new(AtomicBool::new(false)),
            stream: None,
            sample_rate,
            vad_config,
            sender: None,
            vad_worker_handle: None,
            raw_buffer: Arc::new(Mutex::new(Vec::new())),
        })
    }

    pub fn start_streaming(
        &mut self,
        chunk_sender: Sender<Vec<f32>>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        if self.is_recording.load(Ordering::SeqCst) {
            return Ok(());
        }

        if let Ok(mut buf) = self.raw_buffer.lock() {
            buf.clear();
        }

        let host = cpal::default_host();
        let device = host
            .default_input_device()
            .ok_or("No audio input device found")?;
        let config = device.default_input_config()?;
        let channels = config.channels() as usize;
        let source_rate = self.sample_rate;

        let (pcm_tx, pcm_rx) = std::sync::mpsc::channel::<Vec<f32>>();
        self.sender = Some(pcm_tx.clone());

        let is_recording = Arc::clone(&self.is_recording);
        is_recording.store(true, Ordering::SeqCst);

        let vad_config = SileroVadConfig {
            model: self.vad_config.model.clone(),
            min_silence_duration: self.vad_config.min_silence_duration,
            min_speech_duration: self.vad_config.min_speech_duration,
            max_speech_duration: 25.0,
            threshold: self.vad_config.threshold,
            sample_rate: 16000,
            window_size: 512,
            provider: None,
            num_threads: Some(1),
            debug: false,
        };

        let raw_buffer_clone = Arc::clone(&self.raw_buffer);

        // Background VAD Processor thread
        let vad_handle = std::thread::spawn(move || {
            let mut vad = match SileroVad::new(vad_config, 30.0) {
                Ok(v) => v,
                Err(e) => {
                    eprintln!("[voicy] [vad] Failed to initialize SileroVad: {}", e);
                    return;
                }
            };

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
            let mut active_phrase_samples: Vec<f32> = Vec::new();
            let mut last_chunk_time = Instant::now();

            while let Ok(raw_block) = pcm_rx.recv() {
                // Resample to 16kHz
                let samples_16k = if let Some(ref mut r) = resampler {
                    input_resample_buf.extend_from_slice(&raw_block);
                    let mut out_samples = Vec::new();
                    while input_resample_buf.len() >= 1024 {
                        let block = input_resample_buf.drain(..1024).collect::<Vec<f32>>();
                        if let Ok(resampled) = r.process(&[block], None) {
                            if let Some(ch0) = resampled.first() {
                                out_samples.extend_from_slice(ch0);
                            }
                        }
                    }
                    out_samples
                } else {
                    raw_block
                };

                if samples_16k.is_empty() {
                    continue;
                }

                if let Ok(mut buf) = raw_buffer_clone.lock() {
                    buf.extend_from_slice(&samples_16k);
                }

                active_phrase_samples.extend_from_slice(&samples_16k);
                vad.accept_waveform(samples_16k);

                // Check if VAD has popped out completed speech segments
                while !vad.is_empty() {
                    let segment = vad.front();
                    vad.pop();
                    let seg_samples = segment.samples;
                    if !seg_samples.is_empty() {
                        let _ = chunk_sender.send(seg_samples);
                        active_phrase_samples.clear();
                        last_chunk_time = Instant::now();
                    }
                }

                // Safety guard: If continuous speech exceeds 25 seconds without pause, flush to avoid 30s limit
                if active_phrase_samples.len() > 16000 * 25
                    || (active_phrase_samples.len() > 16000 * 3
                        && last_chunk_time.elapsed().as_secs() >= 8)
                {
                    vad.flush();
                    while !vad.is_empty() {
                        let segment = vad.front();
                        vad.pop();
                        let seg_samples = segment.samples;
                        if !seg_samples.is_empty() {
                            let _ = chunk_sender.send(seg_samples);
                        }
                    }
                    active_phrase_samples.clear();
                    last_chunk_time = Instant::now();
                }
            }

            // Flush remaining resampler input buffer if any
            if let Some(ref mut r) = resampler {
                if !input_resample_buf.is_empty() {
                    let needed = 1024 - input_resample_buf.len();
                    input_resample_buf.extend(std::iter::repeat(0.0).take(needed));
                    let block = input_resample_buf.drain(..1024).collect::<Vec<f32>>();
                    if let Ok(resampled) = r.process(&[block], None) {
                        if let Some(ch0) = resampled.first() {
                            vad.accept_waveform(ch0.clone());
                        }
                    }
                }
            }

            // Flush remaining speech upon stop
            vad.flush();
            while !vad.is_empty() {
                let segment = vad.front();
                vad.pop();
                let seg_samples = segment.samples;
                if !seg_samples.is_empty() {
                    let _ = chunk_sender.send(seg_samples);
                }
            }
        });

        self.vad_worker_handle = Some(vad_handle);

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
        self.sender = None;

        if let Some(handle) = self.vad_worker_handle.take() {
            let _ = handle.join();
        }

        self.raw_buffer
            .lock()
            .map(|mut b| std::mem::take(&mut *b))
            .unwrap_or_default()
    }

    pub fn cancel(&mut self) {
        self.is_recording.store(false, Ordering::SeqCst);
        self.stream = None;
        self.sender = None;
        if let Some(handle) = self.vad_worker_handle.take() {
            let _ = handle.join();
        }
        if let Ok(mut buf) = self.raw_buffer.lock() {
            buf.clear();
        }
    }

    pub fn is_recording(&self) -> bool {
        self.is_recording.load(Ordering::SeqCst)
    }
}
