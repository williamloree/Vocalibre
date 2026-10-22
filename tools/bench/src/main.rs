//! Phase 0 benchmark: transcribe WAV fixtures with several GGML models and
//! report load time, latency, WER and RAM, on GPU and CPU.
//!
//! Usage: bench <fixtures_dir> <vad_model> <model.bin>... [--cpu]

use std::path::{Path, PathBuf};
use std::time::Instant;

use whisper_rs::{
    FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters, WhisperVadContext,
    WhisperVadContextParams, WhisperVadParams,
};

type Error = Box<dyn std::error::Error>;

const FIXTURES: [(&str, Option<&str>); 5] = [
    ("silence", None),
    ("noise", None),
    ("fr_short", Some("fr")),
    ("fr_long", Some("fr")),
    ("en", Some("en")),
];

fn main() -> Result<(), Error> {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let cpu = args.iter().any(|a| a == "--cpu");
    args.retain(|a| a != "--cpu");
    if args.len() < 3 {
        return Err("usage: bench <fixtures_dir> <vad_model> <model.bin>... [--cpu]".into());
    }
    let fixtures = PathBuf::from(&args[0]);
    let vad = args[1].clone();
    whisper_rs::install_logging_hooks();

    println!("device: {}", if cpu { "CPU" } else { "GPU" });
    eprintln!("system: {}", whisper_rs::print_system_info());
    println!("| model | load s | RAM MB | fixture | latency s | RTF | WER | output |");
    println!("|---|---|---|---|---|---|---|---|");
    for model in &args[2..] {
        bench_model(Path::new(model), &fixtures, &vad, cpu)?;
    }
    Ok(())
}

fn bench_model(model: &Path, fixtures: &Path, vad: &str, cpu: bool) -> Result<(), Error> {
    let name = model.file_stem().and_then(|s| s.to_str()).unwrap_or("?");
    let mut ctx_params = WhisperContextParameters::default();
    ctx_params
        .use_gpu(!cpu)
        .flash_attn(std::env::var("NO_FLASH").is_err());

    let t = Instant::now();
    let ctx = WhisperContext::new_with_params(model, ctx_params)?;
    let mut state = ctx.create_state()?;
    let mut vad_params = WhisperVadContextParams::new();
    // Silero is tiny: thread overhead dominates, 1 thread is ~8x faster than 8.
    vad_params.set_n_threads(1);
    let mut vad_ctx = WhisperVadContext::new(vad, vad_params)?;
    let load = t.elapsed().as_secs_f32();

    // Warm-up so the first measured run does not include shader/graph setup.
    let warm = read_wav(&fixtures.join("fr_short.wav"))?;
    transcribe(&mut state, &mut vad_ctx, &warm, Some("fr"))?;
    let ram = memory_stats::memory_stats().map_or(0, |m| m.physical_mem / 1_048_576);

    for (fixture, lang) in FIXTURES {
        let audio = read_wav(&fixtures.join(format!("{fixture}.wav")))?;
        let reference = std::fs::read_to_string(fixtures.join(format!("{fixture}.txt")))?;
        let t = Instant::now();
        let text = transcribe(&mut state, &mut vad_ctx, &audio, lang)?;
        let latency = t.elapsed().as_secs_f32();
        let rtf = latency / (audio.len() as f32 / 16_000.0);
        let wer = wer(&reference, &text);
        println!(
            "| {name} | {load:.2} | {ram} | {fixture} | {latency:.2} | {rtf:.3} | {wer:.1}% | {} |",
            text.replace('|', "/")
        );
    }
    Ok(())
}

fn transcribe(
    state: &mut whisper_rs::WhisperState,
    vad_ctx: &mut WhisperVadContext,
    audio: &[f32],
    lang: Option<&str>,
) -> Result<String, Error> {
    // Single VAD pass: keep only speech. No speech → never call the model
    // (Whisper hallucinates on silence).
    let t = Instant::now();
    let speech = speech_only(vad_ctx, audio)?;
    eprintln!("vad: {:.3}s", t.elapsed().as_secs_f32());
    if speech.is_empty() {
        return Ok(String::new());
    }

    let beam: i32 = std::env::var("BEAM")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(5);
    let mut p = FullParams::new(if beam > 1 {
        SamplingStrategy::BeamSearch {
            beam_size: beam,
            patience: -1.0,
        }
    } else {
        SamplingStrategy::Greedy { best_of: 1 }
    });
    p.set_language(lang);
    p.set_no_context(true);
    p.set_temperature(0.0);
    p.set_print_progress(false);
    p.set_print_realtime(false);
    p.set_print_special(false);
    p.set_print_timestamps(false);
    p.set_suppress_nst(true);
    p.set_n_threads(num_threads());
    if std::env::var("AUDIO_CTX").is_ok() {
        // Encoder context sized to the audio (1500 = 30 s) plus a margin.
        let ctx = (speech.len() as f32 / 16_000.0 * 50.0).ceil() as i32 + 64;
        p.set_audio_ctx(ctx.min(1500));
    }
    state.full(p, &speech)?;

    let mut out = String::new();
    for seg in state.as_iter() {
        if seg.no_speech_probability() > 0.6 {
            continue;
        }
        out.push_str(&seg.to_str_lossy()?);
    }
    Ok(out.trim().to_string())
}

/// Concatenates speech segments (timestamps in centiseconds), separated by
/// 100 ms of silence so Whisper keeps word boundaries.
fn speech_only(vad_ctx: &mut WhisperVadContext, audio: &[f32]) -> Result<Vec<f32>, Error> {
    let segments = vad_ctx.segments_from_samples(WhisperVadParams::new(), audio)?;
    let mut out = Vec::with_capacity(audio.len());
    for i in 0..segments.num_segments() {
        let (Some(t0), Some(t1)) = (
            segments.get_segment_start_timestamp(i),
            segments.get_segment_end_timestamp(i),
        ) else {
            continue;
        };
        let start = ((t0 * 160.0) as usize).min(audio.len());
        let end = ((t1 * 160.0) as usize).clamp(start, audio.len());
        if !out.is_empty() {
            out.extend(std::iter::repeat_n(0.0, 1_600));
        }
        out.extend_from_slice(&audio[start..end]);
    }
    Ok(out)
}

fn num_threads() -> i32 {
    std::env::var("THREADS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or_else(|| {
            std::thread::available_parallelism().map_or(4, |n| n.get().min(8) as i32)
        })
}

fn read_wav(path: &Path) -> Result<Vec<f32>, Error> {
    let mut r = hound::WavReader::open(path)?;
    let spec = r.spec();
    if spec.sample_rate != 16_000 || spec.channels != 1 || spec.bits_per_sample != 16 {
        return Err(format!("{}: expected 16 kHz mono 16-bit", path.display()).into());
    }
    r.samples::<i16>()
        .map(|s| Ok(f32::from(s?) / 32768.0))
        .collect()
}

fn words(s: &str) -> Vec<String> {
    s.to_lowercase()
        .split(|c: char| !c.is_alphanumeric() && c != '\'' && c != '’')
        .filter(|w| !w.is_empty())
        .map(str::to_string)
        .collect()
}

/// Word error rate in percent. Empty reference: 0 % if hypothesis empty, else 100 %.
fn wer(reference: &str, hypothesis: &str) -> f32 {
    let r = words(reference);
    let h = words(hypothesis);
    if r.is_empty() {
        return if h.is_empty() { 0.0 } else { 100.0 };
    }
    let mut prev: Vec<usize> = (0..=h.len()).collect();
    for (i, rw) in r.iter().enumerate() {
        let mut cur = vec![i + 1; h.len() + 1];
        for (j, hw) in h.iter().enumerate() {
            let sub = prev[j] + usize::from(rw != hw);
            cur[j + 1] = sub.min(prev[j + 1] + 1).min(cur[j] + 1);
        }
        prev = cur;
    }
    prev[h.len()] as f32 * 100.0 / r.len() as f32
}
