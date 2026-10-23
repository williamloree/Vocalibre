//! Whisper inference on a dedicated thread (CPU-heavy, never on the async
//! runtime). One job at a time; the queue holds at most one pending request.

use std::path::PathBuf;
use std::sync::mpsc;
use std::time::Instant;

use serde::Serialize;
use tokio::sync::oneshot;

use crate::hardware;
use whisper_rs::{
    FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters, WhisperState,
    WhisperVadContext, WhisperVadContextParams, WhisperVadParams,
};

#[derive(Debug, thiserror::Error)]
pub enum SttError {
    #[error("model file not found: {0}")]
    MissingFile(PathBuf),
    #[error("cannot load model: {0}")]
    Load(String),
    #[error("no model loaded")]
    NotLoaded,
    #[error("transcription failed: {0}")]
    Inference(String),
    #[error("transcription worker is busy")]
    Busy,
    #[error("transcription worker stopped")]
    WorkerGone,
}

#[derive(Debug, Clone)]
pub struct LoadRequest {
    pub model_path: PathBuf,
    pub vad_path: PathBuf,
    pub use_gpu: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct Loaded {
    /// GPU was requested but none is usable, or loading on it failed:
    /// running on CPU.
    pub fell_back_to_cpu: bool,
    pub on_gpu: bool,
    /// The GPU in use.
    pub gpu_name: Option<String>,
    pub load_ms: u64,
}

#[derive(Debug, Clone, Default)]
pub struct TranscribeOptions {
    /// `None` = automatic detection.
    pub language: Option<String>,
    /// Custom vocabulary, passed as Whisper's initial prompt.
    pub prompt: String,
    /// 0 = automatic: 5 on GPU, greedy on CPU (beam search is ~2× slower there).
    pub beam_size: u32,
    /// 0 = automatic.
    pub threads: u32,
    /// Shrink the encoder context to the audio length (see manifest).
    pub reduced_audio_ctx: bool,
}

#[derive(Debug, Clone)]
pub struct Transcript {
    pub text: String,
    /// Detected or forced language code; `None` when no speech was found.
    pub language: Option<String>,
    pub inference_ms: u64,
}

enum Cmd {
    Load(LoadRequest, oneshot::Sender<Result<Loaded, SttError>>),
    Transcribe(
        Vec<f32>,
        TranscribeOptions,
        oneshot::Sender<Result<Transcript, SttError>>,
    ),
    Unload,
}

#[derive(Clone)]
pub struct SttWorker {
    tx: mpsc::SyncSender<Cmd>,
}

impl SttWorker {
    pub fn spawn() -> std::io::Result<Self> {
        let (tx, rx) = mpsc::sync_channel(1);
        std::thread::Builder::new()
            .name("stt".into())
            .spawn(move || run(rx))?;
        Ok(Self { tx })
    }

    pub async fn load(&self, req: LoadRequest) -> Result<Loaded, SttError> {
        let (reply, rx) = oneshot::channel();
        self.send(Cmd::Load(req, reply))?;
        rx.await.map_err(|_| SttError::WorkerGone)?
    }

    pub async fn transcribe(
        &self,
        audio: Vec<f32>,
        opts: TranscribeOptions,
    ) -> Result<Transcript, SttError> {
        let (reply, rx) = oneshot::channel();
        self.send(Cmd::Transcribe(audio, opts, reply))?;
        rx.await.map_err(|_| SttError::WorkerGone)?
    }

    pub fn unload(&self) {
        let _ = self.tx.try_send(Cmd::Unload);
    }

    fn send(&self, cmd: Cmd) -> Result<(), SttError> {
        self.tx.try_send(cmd).map_err(|e| match e {
            mpsc::TrySendError::Full(_) => SttError::Busy,
            mpsc::TrySendError::Disconnected(_) => SttError::WorkerGone,
        })
    }
}

struct Engine {
    // Fields drop in declaration order: the state must go before its context.
    state: WhisperState,
    _ctx: WhisperContext,
    vad: WhisperVadContext,
    on_gpu: bool,
}

fn run(rx: mpsc::Receiver<Cmd>) {
    let mut engine: Option<Engine> = None;
    while let Ok(cmd) = rx.recv() {
        match cmd {
            Cmd::Load(req, reply) => {
                engine = None;
                let result = load(&req).map(|(e, info)| {
                    engine = Some(e);
                    info
                });
                let _ = reply.send(result);
            }
            Cmd::Transcribe(audio, opts, reply) => {
                let result = match &mut engine {
                    Some(e) => transcribe(e, &audio, &opts),
                    None => Err(SttError::NotLoaded),
                };
                let _ = reply.send(result);
            }
            Cmd::Unload => engine = None,
        }
    }
}

fn load(req: &LoadRequest) -> Result<(Engine, Loaded), SttError> {
    for path in [&req.model_path, &req.vad_path] {
        if !path.is_file() {
            return Err(SttError::MissingFile(path.clone()));
        }
    }
    let t = Instant::now();
    // Without a usable GPU, whisper.cpp would silently run on the CPU while
    // asked for the GPU: ask for the CPU instead, so the settings fit it.
    let hw = hardware::detect();
    let gpu = if req.use_gpu { hw.best_gpu() } else { None };
    let (ctx, gpu_name, fell_back) = match gpu {
        Some(device) => match context(req, Some(device)) {
            Ok(ctx) => (ctx, hw.gpus.get(device).map(|g| g.name.clone()), false),
            Err(e) => {
                tracing::warn!("GPU load failed, falling back to CPU: {e}");
                (context(req, None)?, None, true)
            }
        },
        None => (context(req, None)?, None, req.use_gpu),
    };
    let on_gpu = gpu_name.is_some();
    let mut state = ctx
        .create_state()
        .map_err(|e| SttError::Load(e.to_string()))?;
    if on_gpu {
        warm_up(&mut state)?;
    }
    let mut vad_params = WhisperVadContextParams::new();
    // Silero is tiny: one thread is ~8× faster than eight (thread overhead).
    // GPU VAD aborts in whisper.cpp, so it always runs on CPU.
    vad_params.set_n_threads(1);
    vad_params.set_use_gpu(false);
    let vad_path = req.vad_path.to_string_lossy();
    let vad = WhisperVadContext::new(&vad_path, vad_params)
        .map_err(|e| SttError::Load(format!("VAD: {e}")))?;
    let info = Loaded {
        fell_back_to_cpu: fell_back,
        on_gpu,
        gpu_name,
        load_ms: t.elapsed().as_millis() as u64,
    };
    tracing::info!(model = %req.model_path.display(), ?info, "model loaded");
    Ok((
        Engine {
            state,
            _ctx: ctx,
            vad,
            on_gpu,
        },
        info,
    ))
}

/// `gpu`: whisper.cpp's `gpu_device`, `None` for the CPU.
fn context(req: &LoadRequest, gpu: Option<usize>) -> Result<WhisperContext, SttError> {
    let mut params = WhisperContextParameters::default();
    params
        .use_gpu(gpu.is_some())
        .flash_attn(gpu.is_some())
        .gpu_device(gpu.unwrap_or(0) as i32);
    WhisperContext::new_with_params(&req.model_path, params)
        .map_err(|e| SttError::Load(e.to_string()))
}

fn transcribe(
    e: &mut Engine,
    audio: &[f32],
    opts: &TranscribeOptions,
) -> Result<Transcript, SttError> {
    let t = Instant::now();
    let speech = speech_only(&mut e.vad, audio)?;
    if speech.is_empty() {
        // No speech: never call the model, it hallucinates on silence.
        return Ok(Transcript {
            text: String::new(),
            language: None,
            inference_ms: t.elapsed().as_millis() as u64,
        });
    }

    let beam = match opts.beam_size {
        0 if e.on_gpu => GPU_BEAM,
        0 => 1,
        n => n,
    };
    let mut p = params(beam);
    if opts.reduced_audio_ctx {
        // 1500 frames = 30 s; add a margin so the last word is not clipped.
        let frames = (speech.len() as f32 / 16_000.0 * 50.0).ceil() as i32 + 64;
        p.set_audio_ctx(frames.min(1500));
    }
    p.set_language(Some(opts.language.as_deref().unwrap_or("auto")));
    p.set_n_threads(threads(opts.threads, e.on_gpu));
    if !opts.prompt.trim().is_empty() {
        p.set_initial_prompt(opts.prompt.trim());
    }
    e.state
        .full(p, &speech)
        .map_err(|err| SttError::Inference(err.to_string()))?;

    let mut text = String::new();
    for seg in e.state.as_iter() {
        if seg.no_speech_probability() > 0.6 || avg_logprob(&seg) < -1.0 {
            continue;
        }
        let s = seg
            .to_str_lossy()
            .map_err(|err| SttError::Inference(err.to_string()))?;
        text.push_str(&s);
    }
    let language = whisper_rs::get_lang_str(e.state.full_lang_id_from_state()).map(str::to_string);
    Ok(Transcript {
        text,
        language,
        inference_ms: t.elapsed().as_millis() as u64,
    })
}

/// Beam width used on GPU when the setting is automatic.
const GPU_BEAM: u32 = 5;

fn params(beam: u32) -> FullParams<'static, 'static> {
    let mut p = FullParams::new(if beam > 1 {
        SamplingStrategy::BeamSearch {
            beam_size: beam as i32,
            patience: -1.0,
        }
    } else {
        SamplingStrategy::Greedy { best_of: 1 }
    });
    p.set_no_context(true);
    p.set_temperature(0.0);
    p.set_suppress_nst(true);
    p.set_print_progress(false);
    p.set_print_realtime(false);
    p.set_print_special(false);
    p.set_print_timestamps(false);
    p
}

/// 4 s of French speech (16 kHz mono 16-bit WAV): GPU warm-up and the
/// speed test in the settings.
const SAMPLE_WAV: &[u8] = include_bytes!("../../../tests/fixtures/fr_short.wav");

/// [`SAMPLE_WAV`] as 16 kHz samples.
pub fn sample_speech() -> Vec<f32> {
    // Canonical 44-byte header (checked when the fixture was generated).
    SAMPLE_WAV
        .get(44..)
        .unwrap_or_default()
        .chunks_exact(2)
        .map(|b| f32::from(i16::from_le_bytes([b[0], b[1]])) / 32768.0)
        .collect()
}

/// Vulkan compiles its shaders on first use: up to 30 s the first time a
/// build runs (then cached by the driver), still ~0.4 s per process. Real
/// speech is needed to reach every shader; silence leaves most of them out.
fn warm_up(state: &mut WhisperState) -> Result<(), SttError> {
    let t = Instant::now();
    let mut p = params(GPU_BEAM);
    p.set_language(Some("fr"));
    p.set_n_threads(threads(0, true));
    state
        .full(p, &sample_speech())
        .map_err(|e| SttError::Load(format!("warm-up: {e}")))?;
    tracing::info!(ms = t.elapsed().as_millis() as u64, "GPU warmed up");
    Ok(())
}

fn avg_logprob(seg: &whisper_rs::WhisperSegment<'_>) -> f32 {
    let n = seg.n_tokens();
    if n == 0 {
        return 0.0;
    }
    let sum: f32 = (0..n)
        .filter_map(|i| seg.get_token(i))
        .map(|t| t.token_probability().max(1e-6).ln())
        .sum();
    sum / n as f32
}

/// GPU inference is fastest with few CPU threads (measured: 2–4); CPU
/// inference uses up to 8.
fn threads(configured: u32, on_gpu: bool) -> i32 {
    if configured > 0 {
        return configured as i32;
    }
    let max = if on_gpu { 4 } else { 8 };
    std::thread::available_parallelism().map_or(4, |n| n.get().min(max) as i32)
}

/// Keeps only speech (timestamps in centiseconds), segments separated by
/// 100 ms of silence so word boundaries survive.
fn speech_only(vad: &mut WhisperVadContext, audio: &[f32]) -> Result<Vec<f32>, SttError> {
    let segments = vad
        .segments_from_samples(WhisperVadParams::new(), audio)
        .map_err(|e| SttError::Inference(format!("VAD: {e}")))?;
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
