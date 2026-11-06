use ptts::checkpoint::read_config;
use ptts::preprocess::Lang;
use ptts::synth::Synth;
use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Instant;

fn get_rss_kb() -> usize {
    if let Ok(statm) = fs::read_to_string("/proc/self/statm") {
        if let Some(rss_pages) = statm.split_whitespace().nth(1) {
            if let Ok(pages) = rss_pages.parse::<usize>() {
                return pages * 4; // 4KB per page
            }
        }
    }
    0
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let model_dir = Path::new("/mnt/sda4/models/phonon-31m-en");
    let voices_dir = model_dir.join("voices");
    let samples_out_dir = model_dir.join("samples");
    let csv_path = model_dir.join("voice_performance_benchmark.csv");

    fs::create_dir_all(&samples_out_dir)?;

    println!("Loading base Phonon 31M model into memory...");
    let start_load = Instant::now();
    let cfg = read_config(model_dir.join("config.json"))?;
    let base_synth = Synth::builder(cfg, model_dir.join("model.q8.gguf"), Lang::En)
        .tokenizer_file(model_dir.join("tokenizer.json"));
    println!("Base configuration loaded in {:.2}s", start_load.elapsed().as_secs_f32());

    let mut csv_file = File::create(&csv_path)?;
    writeln!(csv_file, "voice_name,inference_time_ms,audio_duration_s,real_time_factor,ram_usage_mb")?;

    let mut entries: Vec<PathBuf> = fs::read_dir(&voices_dir)?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().and_then(|s| s.to_str()) == Some("safetensors"))
        .collect();
    entries.sort();

    println!("Beginning voice benchmark for {} voices...", entries.len());

    for (idx, voice_path) in entries.iter().enumerate() {
        let voice_name = voice_path.file_stem().unwrap().to_string_lossy().to_string();
        let prompt_text = format!("Hello, I am {}, and this is a demonstration of my voice.", voice_name.replace('_', " "));

        // Build synth with specific voice
        let cfg_clone = read_config(model_dir.join("config.json"))?;
        let synth = match Synth::builder(cfg_clone, model_dir.join("model.q8.gguf"), Lang::En)
            .tokenizer_file(model_dir.join("tokenizer.json"))
            .add_voice(&voice_name, voice_path)
            .build() {
                Ok(s) => s,
                Err(e) => {
                    eprintln!("[{}/{}] Failed to load voice {}: {}", idx + 1, entries.len(), voice_name, e);
                    continue;
                }
            };

        let start_time = Instant::now();
        let pcm = match synth.say(&prompt_text) {
            Ok(p) => p,
            Err(e) => {
                eprintln!("[{}/{}] Synthesis error for {}: {}", idx + 1, entries.len(), voice_name, e);
                continue;
            }
        };
        let elapsed = start_time.elapsed();
        let elapsed_ms = elapsed.as_millis();

        let sample_rate = synth.sample_rate();
        let audio_duration_s = pcm.len() as f32 / sample_rate as f32;
        let rtf = (elapsed.as_secs_f32()) / audio_duration_s;
        let ram_mb = get_rss_kb() as f32 / 1024.0;

        // Save audio sample
        let wav_file = samples_out_dir.join(format!("{}.wav", voice_name));
        let _ = ptts::wav::write_wav_file(&wav_file, &pcm, sample_rate);

        writeln!(
            csv_file,
            "{},{},{:.2},{:.3},{:.1}",
            voice_name, elapsed_ms, audio_duration_s, rtf, ram_mb
        )?;
        let _ = csv_file.flush();

        println!(
            "[{:>3}/{}] {:<25} | Latency: {:>5}ms | Duration: {:>4.2}s | RTF: {:>5.3}x | RAM: {:>6.1} MB",
            idx + 1,
            entries.len(),
            voice_name,
            elapsed_ms,
            audio_duration_s,
            rtf,
            ram_mb
        );
    }

    println!("Benchmark completed! Full performance table written to: {}", csv_path.display());
    Ok(())
}
