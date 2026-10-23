//! The only owner of application state. Hotkey, tray, audio and STT all talk
//! to it through one channel; it feeds the [`Machine`] and runs the effects.

use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use tokio::sync::mpsc;
use tokio::task::JoinHandle;

use super::state::{AppState, Effect, Input, Machine, Notice};
use crate::audio::{Audio, AudioEvent};
use crate::injection::{self, Injector, LastInjection};
use crate::models::{self, manifest::Manifest};
use crate::settings::{self, Acceleration, Settings};
use crate::sound::Player;
use crate::stt::postprocess;
use crate::stt::whisper::{
    LoadRequest, Loaded, SttError, SttWorker, TranscribeOptions, Transcript,
};

/// What the actor needs from the desktop shell (tray, overlay, hotkeys).
pub trait Shell: Send + 'static {
    fn state_changed(&self, state: &AppState);
    fn level(&self, level: f32);
    fn notice(&self, notice: &Notice);
    fn model_changed(&self, model_id: &str);
    fn set_cancel_key(&self, enabled: bool);
}

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
        result: Result<Transcript, SttError>,
    },
}

pub type ActorTx = mpsc::UnboundedSender<Msg>;

pub struct Paths {
    pub config_dir: PathBuf,
    pub models_dir: PathBuf,
}

pub struct Actor<S: Shell> {
    tx: ActorTx,
    rx: mpsc::UnboundedReceiver<Msg>,
    audio_rx: mpsc::UnboundedReceiver<AudioEvent>,
    machine: Machine,
    settings: Settings,
    paths: Paths,
    manifest: Manifest,
    audio: Audio,
    stt: SttWorker,
    sounds: Player,
    injector: Arc<Mutex<Injector>>,
    loaded_model: Option<String>,
    last_injection: Option<LastInjection>,
    auto_stop: Option<JoinHandle<()>>,
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
            settings,
            paths,
            manifest,
            audio: Audio::spawn(audio_tx)?,
            stt: SttWorker::spawn()?,
            sounds: Player::spawn()?,
            injector: Arc::default(),
            loaded_model: None,
            last_injection: None,
            auto_stop: None,
            epoch: Instant::now(),
            shell,
        };
        Ok((actor, tx))
    }

    pub async fn run(mut self) {
        let preferred = self.settings.active_model.clone();
        match models::pick(&self.manifest, &self.paths.models_dir, preferred.as_deref()) {
            Some(model_id) => self.dispatch(Input::LoadModel { model_id }).await,
            None => {
                tracing::warn!(dir = %self.paths.models_dir.display(), "no model installed");
                self.shell.state_changed(self.machine.state());
            }
        }
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
            Msg::Loaded { model_id, result } => match result {
                Ok(info) => {
                    if info.fell_back_to_cpu {
                        tracing::warn!("GPU unavailable, model runs on CPU");
                    }
                    self.loaded_model = Some(model_id.clone());
                    self.shell.model_changed(&model_id);
                    if self.settings.active_model.as_deref() != Some(model_id.as_str()) {
                        self.settings.active_model = Some(model_id.clone());
                        self.save_settings();
                    }
                    self.dispatch(Input::ModelLoaded { model_id }).await;
                }
                Err(e) => {
                    let detail = e.to_string();
                    self.dispatch(Input::ModelLoadFailed { model_id, detail })
                        .await;
                }
            },
            Msg::Transcribed { job_id, result } => self.on_transcribed(job_id, result).await,
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

    async fn on_transcribed(&mut self, job_id: u64, result: Result<Transcript, SttError>) {
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
        let opts = postprocess::Options {
            capitalize_first: self.settings.capitalize_first,
            strip_final_period: self.settings.strip_final_period,
            replacements: self.settings.replacements.clone(),
        };
        let text = postprocess::process(&transcript.text, &opts);
        tracing::info!(
            job_id,
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
            if let Err(detail) = self.inject(text).await {
                return self
                    .dispatch(Input::TranscriptionFailed { job_id, detail })
                    .await;
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
        .map_err(|e| e.to_string())??;
        self.last_injection = Some(LastInjection {
            at_ms: self.now_ms(),
            ended_with_space,
        });
        Ok(())
    }

    /// Feeds `first` and every input produced while running its effects.
    async fn dispatch(&mut self, first: Input) {
        let mut queue = VecDeque::from([first]);
        while let Some(input) = queue.pop_front() {
            let now = self.now_ms();
            let mode = self.settings.recording_mode;
            match self.machine.handle(input.clone(), now, mode) {
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
        let dir = &self.paths.models_dir;
        let req = LoadRequest {
            model_path: dir.join(&info.file.file_name),
            vad_path: dir.join(&self.manifest.vad.file_name),
            use_gpu: self.settings.acceleration != Acceleration::Cpu,
        };
        self.loaded_model = None;
        let (stt, tx) = (self.stt.clone(), self.tx.clone());
        tokio::spawn(async move {
            let result = stt.load(req).await;
            let _ = tx.send(Msg::Loaded { model_id, result });
        });
        None
    }

    fn spawn_transcription(&self, job_id: u64, samples: Vec<f32>) {
        let s = &self.settings;
        let reduced_audio_ctx = self
            .loaded_model
            .as_deref()
            .and_then(|id| self.manifest.get(id))
            .is_some_and(|m| m.reduced_audio_ctx);
        let opts = TranscribeOptions {
            language: (s.language != "auto").then(|| s.language.clone()),
            prompt: s.vocabulary.clone(),
            beam_size: s.beam_size,
            threads: s.threads,
            reduced_audio_ctx,
        };
        let (stt, tx) = (self.stt.clone(), self.tx.clone());
        tokio::spawn(async move {
            let result = stt.transcribe(samples, opts).await;
            let _ = tx.send(Msg::Transcribed { job_id, result });
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

    fn save_settings(&self) {
        if let Err(e) = settings::save(&self.paths.config_dir, &self.settings) {
            tracing::error!("cannot save settings: {e}");
        }
    }

    fn now_ms(&self) -> u64 {
        self.epoch.elapsed().as_millis() as u64
    }
}
