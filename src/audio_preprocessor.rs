/// Master switch to enable or disable audio pre-processing.
/// Set to `false` to completely bypass all pre-processing.
pub const ENABLE_PREPROCESSING: bool = true;

// Individual stage toggles (active only when ENABLE_PREPROCESSING is true)
pub const ENABLE_HIGH_PASS: bool = true;
pub const ENABLE_SILENCE_TRIM: bool = true;
pub const ENABLE_PEAK_NORM: bool = true;
pub const ENABLE_PRE_EMPHASIS: bool = false; // Off by default as Whisper models have native acoustic filters

/// Runs the modular pre-processing pipeline on 16kHz mono audio samples.
/// Returns the processed samples.
pub fn process_audio(samples: &[f32]) -> Vec<f32> {
    if !ENABLE_PREPROCESSING || samples.is_empty() {
        return samples.to_vec();
    }

    let mut audio = samples.to_vec();

    // 1. High-Pass Filter (80 Hz cutoff at 16kHz) to eliminate DC offset & sub-bass rumble
    if ENABLE_HIGH_PASS {
        high_pass_80hz(&mut audio, 16000.0);
    }

    // 2. Leading & Trailing Silence Trimming (keeps 100ms safety padding)
    if ENABLE_SILENCE_TRIM {
        audio = trim_silence(&audio, 16000, 0.005, 100);
    }

    if audio.is_empty() {
        return audio;
    }

    // 3. Peak Normalization (Gain staging to -1.0 dBFS / 0.90 amplitude)
    if ENABLE_PEAK_NORM {
        normalize_peak(&mut audio, 0.90);
    }

    // 4. Pre-Emphasis Filter (optional high-frequency boost for fricatives)
    if ENABLE_PRE_EMPHASIS {
        pre_emphasis(&mut audio, 0.97);
    }

    audio
}

/// Single-pole high-pass filter with cutoff frequency (e.g. 80 Hz at 16kHz sample rate).
/// y[n] = alpha * (y[n-1] + x[n] - x[n-1])
fn high_pass_80hz(samples: &mut [f32], sample_rate: f32) {
    if samples.is_empty() {
        return;
    }

    let cutoff = 80.0;
    let dt = 1.0 / sample_rate;
    let rc = 1.0 / (2.0 * std::f32::consts::PI * cutoff);
    let alpha = rc / (rc + dt);

    let mut prev_x = samples[0];
    let mut prev_y = samples[0];

    for s in samples.iter_mut() {
        let x = *s;
        let y = alpha * (prev_y + x - prev_x);
        prev_x = x;
        prev_y = y;
        *s = y;
    }
}

/// Trims leading and trailing silence using windowed RMS energy with margin padding.
fn trim_silence(samples: &[f32], sample_rate: usize, threshold: f32, pad_ms: usize) -> Vec<f32> {
    let frame_size = (sample_rate * 30) / 1000; // 30ms window (480 samples @ 16kHz)
    if samples.len() <= frame_size {
        return samples.to_vec();
    }

    let pad_samples = (sample_rate * pad_ms) / 1000; // e.g. 100ms = 1600 samples
    let num_frames = samples.len() / frame_size;

    let mut first_speech_frame = None;
    let mut last_speech_frame = None;

    for i in 0..num_frames {
        let start = i * frame_size;
        let end = start + frame_size;
        let frame = &samples[start..end];

        let sum_sq: f32 = frame.iter().map(|&x| x * x).sum();
        let rms = (sum_sq / frame.len() as f32).sqrt();

        if rms >= threshold {
            if first_speech_frame.is_none() {
                first_speech_frame = Some(i);
            }
            last_speech_frame = Some(i);
        }
    }

    match (first_speech_frame, last_speech_frame) {
        (Some(start_f), Some(end_f)) => {
            let start_sample = (start_f * frame_size).saturating_sub(pad_samples);
            let end_sample = std::cmp::min(samples.len(), (end_f + 1) * frame_size + pad_samples);
            samples[start_sample..end_sample].to_vec()
        }
        _ => Vec::new(), // Entire audio was silent
    }
}

/// Normalizes audio buffer so the highest peak matches the target amplitude (e.g. 0.90 = -1 dBFS).
fn normalize_peak(samples: &mut [f32], target_peak: f32) {
    let max_peak = samples.iter().map(|s| s.abs()).fold(0.0f32, f32::max);

    // Only boost if there is actual signal above minimum noise floor
    if max_peak > 0.005 && max_peak < target_peak {
        let gain = (target_peak / max_peak).min(10.0); // Maximum 10x (+20dB) gain safety limit
        for s in samples.iter_mut() {
            *s *= gain;
        }
    } else if max_peak > 1.0 {
        // Prevent clipping if signal exceeds 1.0
        let gain = target_peak / max_peak;
        for s in samples.iter_mut() {
            *s *= gain;
        }
    }
}

/// Pre-emphasis filter to boost high frequency consonants: y[n] = x[n] - alpha * x[n-1]
fn pre_emphasis(samples: &mut [f32], alpha: f32) {
    if samples.len() < 2 {
        return;
    }
    let mut prev = samples[0];
    for s in samples.iter_mut().skip(1) {
        let current = *s;
        *s = current - alpha * prev;
        prev = current;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_high_pass_removes_dc() {
        let mut dc_signal = vec![1.0; 16000]; // 1 second of constant DC bias 1.0
        high_pass_80hz(&mut dc_signal, 16000.0);
        // By end of 1 second, DC should have decayed near 0
        assert!(dc_signal.last().unwrap().abs() < 0.01);
    }

    #[test]
    fn test_peak_normalization() {
        let mut quiet_signal = vec![0.1; 1000];
        normalize_peak(&mut quiet_signal, 0.90);
        assert!((quiet_signal[0] - 0.90).abs() < 1e-4);
    }

    #[test]
    fn test_silence_trimming() {
        let mut audio = vec![0.0; 8000]; // 0.5s silence
        audio.extend(vec![0.5; 8000]); // 0.5s speech
        audio.extend(vec![0.0; 8000]); // 0.5s silence

        let trimmed = trim_silence(&audio, 16000, 0.01, 100);
        assert!(trimmed.len() < audio.len());
        assert!(trimmed.len() >= 8000);
    }
}
