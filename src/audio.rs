use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::Stream;
use rubato::{Resampler, SincFixedIn, SincInterpolationParameters, SincInterpolationType, WindowFunction};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

pub struct AudioRecorder {
    is_recording: Arc<AtomicBool>,
    buffer: Arc<Mutex<Vec<f32>>>,
    stream: Option<Stream>,
    sample_rate: u32,
}

impl AudioRecorder {
    pub fn new() -> Result<Self, Box<dyn std::error::Error>> {
        let host = cpal::default_host();
        let device = host
            .default_input_device()
            .ok_or("No audio input device found")?;

        let config = device.default_input_config()?;
        let sample_rate = config.sample_rate().0;

        Ok(Self {
            is_recording: Arc::new(AtomicBool::new(false)),
            buffer: Arc::new(Mutex::new(Vec::new())),
            stream: None,
            sample_rate,
        })
    }

    pub fn start(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        if self.is_recording.load(Ordering::SeqCst) {
            return Ok(());
        }

        // Clear existing buffer
        if let Ok(mut buf) = self.buffer.lock() {
            buf.clear();
        }

        let host = cpal::default_host();
        let device = host
            .default_input_device()
            .ok_or("No audio input device found")?;
        let config = device.default_input_config()?;
        let channels = config.channels() as usize;

        let buffer_clone = Arc::clone(&self.buffer);
        let is_recording_clone = Arc::clone(&self.is_recording);

        let err_fn = |err| eprintln!("Audio stream error: {}", err);

        let stream = match config.sample_format() {
            cpal::SampleFormat::F32 => device.build_input_stream(
                &config.into(),
                move |data: &[f32], _| {
                    if is_recording_clone.load(Ordering::SeqCst) {
                        if let Ok(mut buf) = buffer_clone.lock() {
                            for chunk in data.chunks(channels) {
                                let mono = chunk.iter().sum::<f32>() / channels as f32;
                                buf.push(mono);
                            }
                        }
                    }
                },
                err_fn,
                None,
            )?,
            cpal::SampleFormat::I16 => device.build_input_stream(
                &config.into(),
                move |data: &[i16], _| {
                    if is_recording_clone.load(Ordering::SeqCst) {
                        if let Ok(mut buf) = buffer_clone.lock() {
                            for chunk in data.chunks(channels) {
                                let mono_i16 = chunk.iter().map(|&x| x as f32).sum::<f32>() / channels as f32;
                                buf.push(mono_i16 / 32768.0);
                            }
                        }
                    }
                },
                err_fn,
                None,
            )?,
            cpal::SampleFormat::U16 => device.build_input_stream(
                &config.into(),
                move |data: &[u16], _| {
                    if is_recording_clone.load(Ordering::SeqCst) {
                        if let Ok(mut buf) = buffer_clone.lock() {
                            for chunk in data.chunks(channels) {
                                let mono_u16 = chunk.iter().map(|&x| x as f32 - 32768.0).sum::<f32>() / channels as f32;
                                buf.push(mono_u16 / 32768.0);
                            }
                        }
                    }
                },
                err_fn,
                None,
            )?,
            _ => return Err("Unsupported sample format".into()),
        };

        stream.play()?;
        self.stream = Some(stream);
        self.is_recording.store(true, Ordering::SeqCst);
        Ok(())
    }

    pub fn stop(&mut self) -> Vec<f32> {
        self.is_recording.store(false, Ordering::SeqCst);
        self.stream = None;

        let raw_samples = self.buffer.lock().map(|mut b| std::mem::take(&mut *b)).unwrap_or_default();
        if raw_samples.is_empty() {
            return Vec::new();
        }

        // Resample to 16,000 Hz if necessary
        if self.sample_rate == 16000 {
            raw_samples
        } else {
            resample_to_16k(&raw_samples, self.sample_rate)
        }
    }

    pub fn cancel(&mut self) {
        self.is_recording.store(false, Ordering::SeqCst);
        self.stream = None;
        if let Ok(mut buf) = self.buffer.lock() {
            buf.clear();
        }
    }

    pub fn is_recording(&self) -> bool {
        self.is_recording.load(Ordering::SeqCst)
    }
}

fn resample_to_16k(input: &[f32], source_rate: u32) -> Vec<f32> {
    if input.is_empty() {
        return Vec::new();
    }

    let params = SincInterpolationParameters {
        sinc_len: 64,
        f_cutoff: 0.95,
        interpolation: SincInterpolationType::Linear,
        oversampling_factor: 128,
        window: WindowFunction::Blackman,
    };

    let mut resampler = match SincFixedIn::<f32>::new(
        16000 as f64 / source_rate as f64,
        2.0,
        params,
        input.len(),
        1,
    ) {
        Ok(r) => r,
        Err(_) => return input.to_vec(),
    };

    let waves_in = vec![input.to_vec()];
    match resampler.process(&waves_in, None) {
        Ok(mut resampled) => resampled.pop().unwrap_or_default(),
        Err(_) => input.to_vec(),
    }
}
