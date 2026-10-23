//! The only owner of application state. Hotkey, tray, windows, audio and STT
//! all talk to it through one channel; it feeds the [`Machine`], runs the
//! effects and applies settings changes immediately.

use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::Serialize;
use tokio::sync::{mpsc, oneshot};
use tokio::task::JoinHandle;

use super::state::{AppState, Effect, Input, Machine, Notice};
use crate::audio::{Audio, AudioEvent};
use crate::history::{self, History};
use crate::i18n::{self, Lang};
use crate::injection::{self, Injector, LastInjection};
use crate::models::download::{self, DownloadError};
use crate::models::manifest::{Manifest, ModelInfo, RemoteFile};
use crate::models::{self};
use crate::settings::{self, Acceleration, OverlayPosition, Settings, Shortcuts};
use crate::sound::Player;
use crate::stt::postprocess;
use crate::stt::whisper::{
    self, LoadRequest, Loaded, SttError, SttWorker, TranscribeOptions, Transcript,
};

/// What the shell shows outside the state itself.
pub struct View<'a> {
    pub settings: &'a Settings,
    pub lang: Lang,
    pub installed: Vec<&'a ModelInfo>,
    pub active_model: Option<&'a str>,
    pub has_last_text: bool,
}

/// Window events telling the UI to refetch.
#[derive(Debug, Clone, Copy)]
pub enum Changed {
    Settings,
    Models,
    History,
}

/// Progress of the model download, for the UI.
#[derive(Debug, Clone, Serialize)]
pub struct DownloadEvent {
    pub model_id: String,
    pub downloaded: u64,
    pub total: u64,
    pub done: bool,
    /// Error code (see [`DownloadError::code`]) when `done` without success.
    pub error: Option<String>,
}

/// What the actor needs from the desktop shell (tray, windows, hotkeys).
pub trait Shell: Send + 'static {
    fn state_changed(&self, state: &AppState);
    fn level(&self, level: f32);
    fn notice(&self, notice: &Notice);
    fn set_cancel_key(&self, enabled: bool);
    /// Registers the shortcuts and remembers the cancel key. Each one that
    /// cannot be registered keeps its previous key; the error lists them.
    fn set_hotkeys(&self, keys: &Shortcuts) -> Result<(), String>;
    /// Releases the shortcuts while the UI captures a new one.
    fn suspend_hotkey(&self, suspended: bool);
    /// Settings, installed models or the active model changed.
    fn refresh(&self, view: &View<'_>);
    fn changed(&self, what: Changed);
    fn download(&self, event: &DownloadEvent);
}

#[derive(Debug, Clone, Serialize)]
pub struct ModelStatus {
    pub id: String,
    pub on_gpu: bool,
    pub fell_back_to_cpu: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct Status {
    pub state: AppState,
    pub lang: Lang,
    pub model: Option<ModelStatus>,
    pub hotkey_error: Option<String>,
    pub models_dir: PathBuf,
    pub vad_file: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ModelEntry {
    #[serde(flatten)]
    pub info: ModelInfo,
    pub installed: bool,
    pub active: bool,
    /// Bytes so far and total, while downloading.
    pub download: Option<(u64, u64)>,
}

#[derive(Debug, Clone, Serialize)]
pub struct BenchResult {
    pub audio_ms: u64,
    pub inference_ms: u64,
    pub text: String,
}

type Reply<T> = oneshot::Sender<T>;

#[derive(Debug)]
pub enum Msg {
    Input(Input),
    SelectModel(String),
    Loaded {
        model_id: String,
        result: Result<Loaded, SttError>,
    },
    Transcribed {
        job_id: u64,
        audio_ms: u64,
        result: Result<Transcript, SttError>,
    },
    GetStatus(Reply<Status>),
    GetSettings(Reply<Settings>),
    /// Errors are codes the UI translates: `invalid:<fields>`, `hotkey:<detail>`.
    UpdateSettings(Box<Settings>, Reply<Result<Settings, String>>),
    ResetSettings(Reply<Result<Settings, String>>),
    SuspendHotkey(bool),
    /// Pastes the last dictation again (its own shortcut).
    PasteLast,
    /// Puts the last dictation in the clipboard (tray).
    CopyLast,
    GetModels(Reply<Vec<ModelEntry>>),
    DeleteModel(String, Reply<Result<(), String>>),
    Benchmark(Reply<Result<BenchResult, String>>),
    GetHistory(Reply<Vec<history::Entry>>),
    ClearHistory,
    CopyText(String, Reply<Result<(), String>>),
    OverlayMoved {
        monitor: String,
        position: OverlayPosition,
    },
    /// Errors: `download_busy` (one download at a time), `unknown model`.
    Download(String, Reply<Result<(), String>>),
    CancelDownload,
    DownloadProgress {
        downloaded: u64,
    },
    DownloadFinished(Result<(), DownloadError>),
}

/// The model download in progress (one at a time: the VAD file is shared).
struct ActiveDownload {
    model_id: String,
    downloaded: u64,
    total: u64,
    cancel: Arc<AtomicBool>,
}

pub type ActorTx = mpsc::UnboundedSender<Msg>;

pub struct Paths {
    pub config_dir: PathBuf,
    pub data_dir: PathBuf,
}

impl Paths {
    fn models_dir(&self, settings: &Settings) -> PathBuf {
        settings
            .models_dir
            .clone()
            .unwrap_or_else(|| self.data_dir.join("models"))
    }
}

pub struct Actor<S: Shell> {
    tx: ActorTx,
    rx: mpsc::UnboundedReceiver<Msg>,
    audio_rx: mpsc::UnboundedReceiver<AudioEvent>,
    machine: Machine,
    settings: Settings,
    paths: Paths,
    manifest: Manifest,
    history: History,
    audio: Audio,
    stt: SttWorker,
    sounds: Player,
    injector: Arc<Mutex<Injector>>,
    model: Option<ModelStatus>,
    /// The model to load as soon as no audio is in flight.
    reload_pending: bool,
    hotkey_error: Option<String>,
    last_injection: Option<LastInjection>,
    /// Last dictated text, in memory only (kept even with history off).
    last_text: Option<String>,
    auto_stop: Option<JoinHandle<()>>,
    download: Option<ActiveDownload>,
    epoch: Instant,
    shell: S,
}

impl<S: Shell> Actor<S> {
    pub fn new(
        settings: Settings,
        paths: Paths,
        manifest: Manifest,
        shell: S,
    ) -> std::io::Result<(Self, ActorTx)> {
        let (tx, rx) = mpsc::unbounded_channel();
        let (audio_tx, audio_rx) = mpsc::unbounded_channel();
        let actor = Self {
            tx: tx.clone(),
            rx,
            audio_rx,
            machine: Machine::new(AppState::Onboarding),
            history: History::load(&paths.data_dir),
            settings,
            paths,
            manifest,
            audio: Audio::spawn(audio_tx)?,
            stt: SttWorker::spawn()?,
            sounds: Player::spawn()?,
            injector: Arc::default(),
            model: None,
            reload_pending: true,
            hotkey_error: None,
            last_injection: None,
            last_text: None,
            auto_stop: None,
            download: None,
            epoch: Instant::now(),
            shell,
        };
        Ok((actor, tx))
    }

    pub async fn run(mut self) {
        let models_dir = self.models_dir();
        if let Err(e) = std::fs::create_dir_all(&models_dir) {
            tracing::error!(dir = %models_dir.display(), "cannot create models directory: {e}");
        }
        self.hotkey_error = self.shell.set_hotkeys(&self.settings.shortcuts()).err();
        if let Some(e) = &self.hotkey_error {
            tracing::error!("cannot register shortcuts: {e}");
        }
        self.refresh_shell();
        self.shell.state_changed(self.machine.state());
        self.reload_if_pending().await;
        loop {
            tokio::select! {
                Some(msg) = self.rx.recv() => self.on_msg(msg).await,
                Some(ev) = self.audio_rx.recv() => self.on_audio(ev).await,
                else => return,
            }
        }
    }

    async fn on_msg(&mut self, msg: Msg) {
        match msg {
            Msg::Input(input) => self.dispatch(input).await,
            Msg::SelectModel(model_id) => self.dispatch(Input::LoadModel { model_id }).await,
            Msg::Loaded { model_id, result } => self.on_loaded(model_id, result).await,
            Msg::Transcribed {
                job_id,
                audio_ms,
                result,
            } => self.on_transcribed(job_id, audio_ms, result).await,
            Msg::GetStatus(reply) => {
                let _ = reply.send(self.status());
            }
            Msg::GetSettings(reply) => {
                let _ = reply.send(self.settings.clone());
            }
            Msg::UpdateSettings(new, reply) => {
                let result = self.update_settings(*new).await;
                let _ = reply.send(result);
            }
            Msg::ResetSettings(reply) => {
                // The models stay where they are; the active model and
                // other internal fields are kept by `update_settings`.
                let fresh = Settings {
                    models_dir: self.settings.models_dir.clone(),
                    ..Settings::default()
                };
                let result = self.update_settings(fresh).await;
                let _ = reply.send(result);
            }
            Msg::SuspendHotkey(suspended) => self.shell.suspend_hotkey(suspended),
            Msg::PasteLast => self.paste_last().await,
            Msg::CopyLast => {
                if let Some(text) = self.last_text.clone() {
                    let injector = self.injector.clone();
                    tokio::task::spawn_blocking(move || {
                        let copied = injector
                            .lock()
                            .map_err(|_| "injector poisoned".to_string())
                            .and_then(|mut i| i.copy(&text).map_err(|e| e.to_string()));
                        if let Err(e) = copied {
                            tracing::warn!("cannot copy last dictation: {e}");
                        }
                    });
                }
            }
            Msg::GetModels(reply) => {
                let _ = reply.send(self.model_entries());
            }
            Msg::DeleteModel(id, reply) => {
                let result = self.delete_model(&id).await;
                let _ = reply.send(result);
            }
            Msg::Benchmark(reply) => self.benchmark(reply),
            Msg::GetHistory(reply) => {
                let _ = reply.send(self.history.entries().iter().cloned().collect());
            }
            Msg::ClearHistory => {
                self.history.clear();
                self.save_history();
            }
            Msg::CopyText(text, reply) => {
                let injector = self.injector.clone();
                tokio::task::spawn_blocking(move || {
                    let result = injector
                        .lock()
                        .map_err(|_| "injector poisoned".to_string())
                        .and_then(|mut i| i.copy(&text).map_err(|e| e.to_string()));
                    let _ = reply.send(result);
                });
            }
            Msg::OverlayMoved { monitor, position } => {
                self.settings.overlay_positions.insert(monitor, position);
                self.save_settings();
                self.refresh_shell();
            }
            Msg::Download(id, reply) => {
                let _ = reply.send(self.start_download(&id));
            }
            Msg::CancelDownload => {
                if let Some(d) = &self.download {
                    d.cancel.store(true, Ordering::Relaxed);
                }
            }
            Msg::DownloadProgress { downloaded } => {
                if let Some(d) = &mut self.download {
                    d.downloaded = downloaded;
                    let event = DownloadEvent {
                        model_id: d.model_id.clone(),
                        downloaded,
                        total: d.total,
                        done: false,
                        error: None,
                    };
                    self.shell.download(&event);
                }
            }
            Msg::DownloadFinished(result) => self.on_download_finished(result).await,
        }
    }

    async fn on_audio(&mut self, ev: AudioEvent) {
        match ev {
            AudioEvent::Level(l) => {
                if matches!(self.machine.state(), AppState::Recording { .. }) {
                    self.shell.level(l);
                }
            }
            AudioEvent::DeviceLost => self.dispatch(Input::AudioDeviceLost).await,
            AudioEvent::Silence => {
                if self.settings.silence_auto_stop {
                    self.dispatch(Input::AutoStop).await;
                }
            }
        }
    }

    async fn on_loaded(&mut self, model_id: String, result: Result<Loaded, SttError>) {
        match result {
            Ok(info) => {
                if info.fell_back_to_cpu {
                    tracing::warn!("GPU unavailable, model runs on CPU");
                }
                self.model = Some(ModelStatus {
                    id: model_id.clone(),
                    on_gpu: info.on_gpu,
                    fell_back_to_cpu: info.fell_back_to_cpu,
                });
                if self.settings.active_model.as_deref() != Some(model_id.as_str()) {
                    self.settings.active_model = Some(model_id.clone());
                    self.save_settings();
                    self.shell.changed(Changed::Settings);
                }
                self.refresh_shell();
                self.shell.changed(Changed::Models);
                self.dispatch(Input::ModelLoaded { model_id }).await;
            }
            Err(e) => {
                let detail = e.to_string();
                self.dispatch(Input::ModelLoadFailed { model_id, detail })
                    .await;
            }
        }
    }

    async fn on_transcribed(
        &mut self,
        job_id: u64,
        audio_ms: u64,
        result: Result<Transcript, SttError>,
    ) {
        let current =
            matches!(self.machine.state(), AppState::Transcribing { job_id: j } if *j == job_id);
        let transcript = match result {
            Ok(t) => t,
            Err(e) => {
                let detail = e.to_string();
                return self
                    .dispatch(Input::TranscriptionFailed { job_id, detail })
                    .await;
            }
        };
        let text = postprocess::process(&transcript.text, &self.postprocess_options());
        tracing::info!(
            job_id,
            audio_ms,
            inference_ms = transcript.inference_ms,
            chars = text.chars().count(),
            language = ?transcript.language,
            "transcribed"
        );
        if self.settings.debug_log_text {
            tracing::debug!(job_id, %text, "transcription text");
        }
        let has_text = !text.is_empty();
        if current && has_text {
            if let Err(detail) = self.inject(text.clone()).await {
                return self
                    .dispatch(Input::TranscriptionFailed { job_id, detail })
                    .await;
            }
            if self.last_text.replace(text.clone()).is_none() {
                self.refresh_shell();
            }
            if self.settings.history_enabled {
                self.history.push(history::Entry {
                    id: 0,
                    text,
                    at: chrono::Local::now().to_rfc3339(),
                    duration_ms: audio_ms,
                    model_id: self
                        .model
                        .as_ref()
                        .map(|m| m.id.clone())
                        .unwrap_or_default(),
                });
                self.save_history();
            }
        }
        self.dispatch(Input::TranscriptionFinished { job_id, has_text })
            .await;
    }

    async fn inject(&mut self, text: String) -> Result<(), String> {
        let now = self.now_ms();
        let text = if self.settings.auto_space {
            injection::with_leading_space(&text, self.last_injection, now)
        } else {
            text
        };
        let ended_with_space = injection::ends_with_space(&text);
        self.inject_raw(text).await?;
        self.last_injection = Some(LastInjection {
            at_ms: self.now_ms(),
            ended_with_space,
        });
        Ok(())
    }

    /// Pastes the last dictation again, as it was (no automatic space),
    /// typically after it went to the wrong window.
    async fn paste_last(&mut self) {
        if !matches!(self.machine.state(), AppState::Idle) {
            return;
        }
        let Some(text) = self.last_text.clone() else {
            return;
        };
        if let Err(e) = self.inject_raw(text).await {
            tracing::warn!("cannot paste last dictation: {e}");
            self.shell
                .notice(&Notice::TranscriptionFailed(e.to_string()));
        }
    }

    async fn inject_raw(&self, text: String) -> Result<(), String> {
        let injector = self.injector.clone();
        let s = &self.settings;
        let (mode, delay, keep) = (s.injection_mode, s.restore_delay_ms, s.keep_in_clipboard);
        tokio::task::spawn_blocking(move || {
            let mut inj = injector
                .lock()
                .map_err(|_| "injector poisoned".to_string())?;
            inj.inject(&text, mode, delay, keep)
                .map_err(|e| e.to_string())
        })
        .await
        .map_err(|e| e.to_string())?
    }

    /// Validates, applies and saves `new`. Fields the UI does not own are
    /// kept from the current settings.
    async fn update_settings(&mut self, mut new: Settings) -> Result<Settings, String> {
        let reset = new.validate();
        if !reset.is_empty() {
            return Err(format!("invalid:{}", reset.join(",")));
        }
        let old = &self.settings;
        new.schema_version = old.schema_version;
        new.active_model = old.active_model.clone();
        new.overlay_positions = old.overlay_positions.clone();
        new.onboarding_done = old.onboarding_done;

        if new.shortcuts() != old.shortcuts() {
            if let Err(e) = self.shell.set_hotkeys(&new.shortcuts()) {
                // All or nothing: put back the keys that did change.
                if let Err(e) = self.shell.set_hotkeys(&old.shortcuts()) {
                    tracing::error!("cannot restore shortcuts: {e}");
                }
                return Err(format!("hotkey:{e}"));
            }
            self.hotkey_error = None;
        }
        if new.models_dir != old.models_dir && self.download.is_some() {
            return Err("busy".into());
        }
        let reload = new.acceleration != old.acceleration || new.models_dir != old.models_dir;
        self.settings = new;
        self.save_settings();
        self.refresh_shell();
        if reload {
            self.reload_pending = true;
            self.shell.changed(Changed::Models);
            self.reload_if_pending().await;
        }
        Ok(self.settings.clone())
    }

    /// Downloads the model and, if missing, the VAD file. The first model
    /// downloaded becomes the active one.
    fn start_download(&mut self, id: &str) -> Result<(), String> {
        if self.download.is_some() {
            return Err("download_busy".into());
        }
        let info = self.manifest.get(id).ok_or("unknown model")?;
        let dir = self.models_dir();
        let files: Vec<RemoteFile> = [&self.manifest.vad, &info.file]
            .into_iter()
            .filter(|f| !models::is_installed(&dir, f))
            .cloned()
            .collect();
        let total = files.iter().map(|f| f.size).sum();
        let cancel = Arc::new(AtomicBool::new(false));
        self.download = Some(ActiveDownload {
            model_id: id.to_string(),
            downloaded: 0,
            total,
            cancel: cancel.clone(),
        });
        let tx = self.tx.clone();
        tokio::spawn(async move {
            let result = fetch_all(&files, &dir, &cancel, &tx).await;
            let _ = tx.send(Msg::DownloadFinished(result));
        });
        self.shell.changed(Changed::Models);
        Ok(())
    }

    async fn on_download_finished(&mut self, result: Result<(), DownloadError>) {
        let Some(d) = self.download.take() else {
            return;
        };
        let error = match &result {
            Ok(()) => None,
            Err(e) => {
                tracing::warn!(model = %d.model_id, "download failed: {e}");
                Some(e.code())
            }
        };
        self.shell.download(&DownloadEvent {
            model_id: d.model_id.clone(),
            downloaded: d.downloaded,
            total: d.total,
            done: true,
            error,
        });
        self.shell.changed(Changed::Models);
        self.refresh_shell();
        if result.is_ok() && self.model.is_none() {
            self.settings.active_model = Some(d.model_id);
            self.save_settings();
            self.shell.changed(Changed::Settings);
            self.reload_pending = true;
            self.reload_if_pending().await;
        }
    }

    async fn delete_model(&mut self, id: &str) -> Result<(), String> {
        let info = self.manifest.get(id).ok_or("unknown model")?;
        if self.download.as_ref().is_some_and(|d| d.model_id == id) {
            return Err("busy".into());
        }
        let in_use = self.model.as_ref().is_some_and(|m| m.id == id);
        if in_use
            && matches!(
                self.machine.state(),
                AppState::Recording { .. } | AppState::Transcribing { .. }
            )
        {
            return Err("busy".into());
        }
        if in_use {
            self.stt.unload();
            self.model = None;
        }
        let path = self.models_dir().join(&info.file.file_name);
        let removed = std::fs::remove_file(&path).map_err(|e| e.to_string());
        if in_use || self.settings.active_model.as_deref() == Some(id) {
            self.settings.active_model = None;
            self.save_settings();
            self.reload_pending = true;
        }
        self.refresh_shell();
        self.shell.changed(Changed::Models);
        self.reload_if_pending().await;
        removed
    }

    fn benchmark(&self, reply: Reply<Result<BenchResult, String>>) {
        if self.model.is_none() || *self.machine.state() != AppState::Idle {
            let _ = reply.send(Err("busy".into()));
            return;
        }
        let samples = whisper::sample_speech();
        let audio_ms = samples.len() as u64 / 16;
        let (stt, opts, post) = (
            self.stt.clone(),
            self.transcribe_options(),
            self.postprocess_options(),
        );
        tokio::spawn(async move {
            let result = stt
                .transcribe(
                    samples,
                    TranscribeOptions {
                        language: Some("fr".into()),
                        ..opts
                    },
                )
                .await
                .map(|t| BenchResult {
                    audio_ms,
                    inference_ms: t.inference_ms,
                    text: postprocess::process(&t.text, &post),
                })
                .map_err(|e| e.to_string());
            let _ = reply.send(result);
        });
    }

    /// Feeds `first` and every input produced while running its effects.
    async fn dispatch(&mut self, first: Input) {
        let mut queue = VecDeque::from([first]);
        while let Some(input) = queue.pop_front() {
            let now = self.now_ms();
            match self.machine.handle(input.clone(), now) {
                Ok(effects) => {
                    tracing::debug!(?input, state = ?self.machine.state(), "transition");
                    self.shell.state_changed(self.machine.state());
                    for effect in effects {
                        if let Some(next) = self.run_effect(effect).await {
                            queue.push_back(next);
                        }
                    }
                }
                Err(why) => tracing::debug!(?input, ?why, "input ignored"),
            }
        }
        Box::pin(self.reload_if_pending()).await;
    }

    /// Loads the preferred model once no audio is in flight.
    async fn reload_if_pending(&mut self) {
        if !self.reload_pending
            || matches!(
                self.machine.state(),
                AppState::Recording { .. } | AppState::Transcribing { .. }
            )
        {
            return;
        }
        self.reload_pending = false;
        let preferred = self.settings.active_model.clone();
        let input = match models::pick(&self.manifest, &self.models_dir(), preferred.as_deref()) {
            Some(model_id) => Input::LoadModel { model_id },
            None => {
                tracing::warn!(dir = %self.models_dir().display(), "no model installed");
                self.stt.unload();
                self.model = None;
                Input::NoModel
            }
        };
        self.dispatch(input).await;
    }

    async fn run_effect(&mut self, effect: Effect) -> Option<Input> {
        match effect {
            Effect::StartRecording => {
                let silence = self
                    .settings
                    .silence_auto_stop
                    .then(|| u64::from(self.settings.silence_timeout_s) * 1000);
                let mic = self.settings.microphone.clone();
                match self.audio.start(mic, silence).await {
                    Ok(info) => {
                        if info.fell_back {
                            tracing::warn!(device = %info.device_name, "configured microphone missing, using default");
                        }
                        self.schedule_auto_stop();
                        None
                    }
                    Err(e) => Some(Input::RecordingFailed {
                        detail: e.to_string(),
                    }),
                }
            }
            Effect::StopAndTranscribe { job_id } => {
                self.cancel_auto_stop();
                match self.audio.stop().await {
                    Ok(samples) => {
                        self.spawn_transcription(job_id, samples);
                        None
                    }
                    Err(e) => Some(Input::TranscriptionFailed {
                        job_id,
                        detail: e.to_string(),
                    }),
                }
            }
            Effect::DiscardRecording => {
                self.cancel_auto_stop();
                self.audio.discard();
                None
            }
            Effect::LoadModel { model_id } => self.spawn_load(model_id),
            Effect::Play(sound) => {
                if self.settings.sounds_enabled {
                    self.sounds.play(sound, self.settings.sound_volume);
                }
                None
            }
            Effect::Notify(notice) => {
                tracing::warn!(?notice, "notice");
                self.shell.notice(&notice);
                None
            }
            Effect::EnableCancelKey(on) => {
                self.shell.set_cancel_key(on);
                None
            }
        }
    }

    fn spawn_load(&mut self, model_id: String) -> Option<Input> {
        let Some(info) = self.manifest.get(&model_id) else {
            return Some(Input::ModelLoadFailed {
                detail: format!("unknown model {model_id}"),
                model_id,
            });
        };
        let dir = self.models_dir();
        let req = LoadRequest {
            model_path: dir.join(&info.file.file_name),
            vad_path: dir.join(&self.manifest.vad.file_name),
            use_gpu: self.settings.acceleration != Acceleration::Cpu,
        };
        self.model = None;
        let (stt, tx) = (self.stt.clone(), self.tx.clone());
        tokio::spawn(async move {
            let result = stt.load(req).await;
            let _ = tx.send(Msg::Loaded { model_id, result });
        });
        None
    }

    fn transcribe_options(&self) -> TranscribeOptions {
        let s = &self.settings;
        let reduced_audio_ctx = self
            .model
            .as_ref()
            .and_then(|m| self.manifest.get(&m.id))
            .is_some_and(|m| m.reduced_audio_ctx);
        TranscribeOptions {
            language: (s.language != "auto").then(|| s.language.clone()),
            prompt: s.vocabulary.clone(),
            beam_size: s.beam_size,
            threads: s.threads,
            reduced_audio_ctx,
        }
    }

    fn postprocess_options(&self) -> postprocess::Options {
        postprocess::Options {
            capitalize_first: self.settings.capitalize_first,
            strip_final_period: self.settings.strip_final_period,
            replacements: self.settings.replacements.clone(),
        }
    }

    fn spawn_transcription(&self, job_id: u64, samples: Vec<f32>) {
        let audio_ms = samples.len() as u64 / 16;
        let opts = self.transcribe_options();
        let (stt, tx) = (self.stt.clone(), self.tx.clone());
        tokio::spawn(async move {
            let result = stt.transcribe(samples, opts).await;
            let _ = tx.send(Msg::Transcribed {
                job_id,
                audio_ms,
                result,
            });
        });
    }

    fn schedule_auto_stop(&mut self) {
        self.cancel_auto_stop();
        let max = Duration::from_secs(self.settings.max_duration_s.into());
        let tx = self.tx.clone();
        self.auto_stop = Some(tokio::spawn(async move {
            tokio::time::sleep(max).await;
            let _ = tx.send(Msg::Input(Input::AutoStop));
        }));
    }

    fn cancel_auto_stop(&mut self) {
        if let Some(t) = self.auto_stop.take() {
            t.abort();
        }
    }

    fn models_dir(&self) -> PathBuf {
        self.paths.models_dir(&self.settings)
    }

    fn model_entries(&self) -> Vec<ModelEntry> {
        let dir = self.models_dir();
        let vad = models::is_installed(&dir, &self.manifest.vad);
        self.manifest
            .models
            .iter()
            .map(|m| ModelEntry {
                info: m.clone(),
                installed: vad && models::is_installed(&dir, &m.file),
                active: self.model.as_ref().is_some_and(|a| a.id == m.id),
                download: self
                    .download
                    .as_ref()
                    .filter(|d| d.model_id == m.id)
                    .map(|d| (d.downloaded, d.total)),
            })
            .collect()
    }

    fn status(&self) -> Status {
        Status {
            state: self.machine.state().clone(),
            lang: i18n::resolve(self.settings.ui_language),
            model: self.model.clone(),
            hotkey_error: self.hotkey_error.clone(),
            models_dir: self.models_dir(),
            vad_file: self.manifest.vad.file_name.clone(),
        }
    }

    fn refresh_shell(&self) {
        let dir = self.models_dir();
        self.shell.refresh(&View {
            settings: &self.settings,
            lang: i18n::resolve(self.settings.ui_language),
            installed: models::installed(&self.manifest, &dir),
            active_model: self.model.as_ref().map(|m| m.id.as_str()),
            has_last_text: self.last_text.is_some(),
        });
    }

    fn save_settings(&self) {
        if let Err(e) = settings::save(&self.paths.config_dir, &self.settings) {
            tracing::error!("cannot save settings: {e}");
        }
    }

    fn save_history(&self) {
        if let Err(e) = self.history.save() {
            tracing::error!("cannot save history: {e}");
        }
        self.shell.changed(Changed::History);
    }

    fn now_ms(&self) -> u64 {
        self.epoch.elapsed().as_millis() as u64
    }
}

/// How often the download reports progress to the UI.
const PROGRESS_EVERY: Duration = Duration::from_millis(250);

/// Downloads `files` in order, reporting the bytes done over all of them.
async fn fetch_all(
    files: &[RemoteFile],
    dir: &std::path::Path,
    cancel: &AtomicBool,
    tx: &ActorTx,
) -> Result<(), DownloadError> {
    let client = download::client()?;
    let mut before = 0;
    let mut last = Instant::now();
    for file in files {
        let mut report = |n: u64| {
            if last.elapsed() >= PROGRESS_EVERY {
                last = Instant::now();
                let _ = tx.send(Msg::DownloadProgress {
                    downloaded: before + n,
                });
            }
        };
        download::fetch(&client, file, dir, cancel, &mut report).await?;
        before += file.size;
    }
    Ok(())
}
