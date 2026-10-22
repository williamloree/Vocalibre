//! Non-regression on WAV fixtures: silence, noise, short/long French, English.
//!
//! Needs `ggml-base-q5_1.bin` and `ggml-silero-v6.2.0.bin` in the directory
//! named by `VOCALIBRE_TEST_MODELS`; skipped otherwise.

use std::path::{Path, PathBuf};

use vocalibre_lib::stt::postprocess;
use vocalibre_lib::stt::whisper::{LoadRequest, SttWorker, TranscribeOptions};

const MAX_WER: f32 = 20.0;

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../tests/fixtures")
}

fn read_wav(path: &Path) -> Vec<f32> {
    let bytes = std::fs::read(path).unwrap();
    // 44-byte canonical PCM header, 16 kHz mono 16-bit (checked by the generator).
    bytes[44..]
        .chunks_exact(2)
        .map(|b| f32::from(i16::from_le_bytes([b[0], b[1]])) / 32768.0)
        .collect()
}

fn words(s: &str) -> Vec<String> {
    s.to_lowercase()
        .split(|c: char| !c.is_alphanumeric() && c != '\'' && c != '’')
        .filter(|w| !w.is_empty())
        .map(str::to_string)
        .collect()
}

fn wer(reference: &str, hypothesis: &str) -> f32 {
    let (r, h) = (words(reference), words(hypothesis));
    if r.is_empty() {
        return if h.is_empty() { 0.0 } else { 100.0 };
    }
    let mut prev: Vec<usize> = (0..=h.len()).collect();
    for (i, rw) in r.iter().enumerate() {
        let mut cur = vec![i + 1; h.len() + 1];
        for (j, hw) in h.iter().enumerate() {
            cur[j + 1] = (prev[j] + usize::from(rw != hw))
                .min(prev[j + 1] + 1)
                .min(cur[j] + 1);
        }
        prev = cur;
    }
    prev[h.len()] as f32 * 100.0 / r.len() as f32
}

#[tokio::test]
async fn fixtures_transcribe_within_wer_threshold() {
    let Ok(models) = std::env::var("VOCALIBRE_TEST_MODELS") else {
        eprintln!("VOCALIBRE_TEST_MODELS not set: skipping");
        return;
    };
    let models = PathBuf::from(models);
    let worker = SttWorker::spawn().unwrap();
    worker
        .load(LoadRequest {
            model_path: models.join("ggml-base-q5_1.bin"),
            vad_path: models.join("ggml-silero-v6.2.0.bin"),
            use_gpu: false,
        })
        .await
        .unwrap();

    for (name, lang) in [
        ("silence", "fr"),
        ("noise", "fr"),
        ("fr_short", "fr"),
        ("fr_long", "fr"),
        ("en", "en"),
    ] {
        let audio = read_wav(&fixtures().join(format!("{name}.wav")));
        let reference = std::fs::read_to_string(fixtures().join(format!("{name}.txt"))).unwrap();
        let out = worker
            .transcribe(
                audio,
                TranscribeOptions {
                    language: Some(lang.into()),
                    reduced_audio_ctx: true,
                    ..TranscribeOptions::default()
                },
            )
            .await
            .unwrap();
        let text = postprocess::process(&out.text, &postprocess::Options::default());
        let w = wer(&reference, &text);
        eprintln!("{name}: {w:.1}% «{text}»");
        assert!(w <= MAX_WER, "{name}: WER {w:.1}% > {MAX_WER}% («{text}»)");
    }
}
