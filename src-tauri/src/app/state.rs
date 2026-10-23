//! Application state machine. Pure: no I/O, no clock, no threads.
//!
//! The actor feeds [`Input`]s with the current monotonic time and executes the
//! returned [`Effect`]s. Every (state × input) pair is covered by tests.

use serde::Serialize;

/// Presses closer than this to the previous accepted press are ignored.
pub const DEBOUNCE_MS: u64 = 250;
/// Recordings shorter than this are discarded without transcription.
pub const MIN_RECORDING_MS: u64 = 300;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RecordingMode {
    Toggle,
    PushToTalk,
}

/// A recoverable error shown to the user with a "retry" action.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ErrorInfo {
    /// Stable identifier, used by the UI to pick a translated message.
    pub code: &'static str,
    /// Technical detail, shown under the translated message.
    pub detail: String,
    /// Model to reload on retry, if the error came from loading it.
    pub retry_model: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AppState {
    /// First launch or no valid model installed.
    Onboarding,
    LoadingModel {
        model_id: String,
    },
    Idle,
    Recording {
        started_at_ms: u64,
        mode: RecordingMode,
    },
    Transcribing {
        job_id: u64,
    },
    Error {
        error: ErrorInfo,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Input {
    /// One of the two dictation shortcuts, with the mode it starts.
    HotkeyPressed(RecordingMode),
    /// The push-to-talk shortcut was released.
    HotkeyReleased,
    /// Escape during recording.
    Cancel,
    /// Tray "Start/Stop" item: behaves like a toggle press, without debounce.
    ToggleRequested,
    /// Maximum duration or silence auto-stop reached.
    AutoStop,
    /// Microphone unplugged or stream error during recording.
    AudioDeviceLost,
    /// The microphone could not be opened.
    RecordingFailed {
        detail: String,
    },
    LoadModel {
        model_id: String,
    },
    ModelLoaded {
        model_id: String,
    },
    ModelLoadFailed {
        model_id: String,
        detail: String,
    },
    /// No usable model on disk: go (back) to onboarding.
    NoModel,
    TranscriptionFinished {
        job_id: u64,
        /// Something was injected (no "done" sound for an empty result).
        has_text: bool,
    },
    TranscriptionFailed {
        job_id: u64,
        detail: String,
    },
    Retry,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sound {
    Start,
    Stop,
    Cancel,
    /// Text was transcribed and injected.
    Done,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Notice {
    DeviceLost,
    /// The recording is digital silence: the system gives the app a muted
    /// input, typically the microphone permission refused on macOS.
    MicSilent,
    RecordingFailed(String),
    TranscriptionFailed(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Effect {
    StartRecording,
    /// Stop capturing and send the audio to the STT worker under `job_id`.
    StopAndTranscribe {
        job_id: u64,
    },
    /// Stop capturing and drop the audio.
    DiscardRecording,
    LoadModel {
        model_id: String,
    },
    Play(Sound),
    Notify(Notice),
    /// Register Escape while recording only, so it is never stolen otherwise.
    EnableCancelKey(bool),
}

/// Why an input was not accepted. The actor logs it; nothing else happens.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ignored {
    NotApplicable,
    Debounced,
    StaleJob,
}

#[derive(Debug)]
pub struct Machine {
    state: AppState,
    last_press_ms: Option<u64>,
    next_job_id: u64,
}

impl Machine {
    pub fn new(initial: AppState) -> Self {
        Self {
            state: initial,
            last_press_ms: None,
            next_job_id: 1,
        }
    }

    pub fn state(&self) -> &AppState {
        &self.state
    }

    /// Applies `input` at monotonic time `now_ms`.
    pub fn handle(&mut self, input: Input, now_ms: u64) -> Result<Vec<Effect>, Ignored> {
        use AppState as S;
        use Input as I;

        if matches!(input, I::HotkeyPressed(_)) {
            if let Some(last) = self.last_press_ms {
                if now_ms.saturating_sub(last) < DEBOUNCE_MS {
                    return Err(Ignored::Debounced);
                }
            }
        }

        let (next, effects) = match (&self.state, input) {
            // Start recording.
            (S::Idle, I::HotkeyPressed(mode)) => self.start(now_ms, mode),
            (S::Idle, I::ToggleRequested) => self.start(now_ms, RecordingMode::Toggle),

            // Stop recording: a toggle recording stops on either shortcut,
            // a push-to-talk one only when its key is released.
            (
                S::Recording {
                    started_at_ms,
                    mode: RecordingMode::Toggle,
                },
                I::HotkeyPressed(_),
            )
            | (
                S::Recording {
                    started_at_ms,
                    mode: RecordingMode::PushToTalk,
                },
                I::HotkeyReleased,
            )
            | (S::Recording { started_at_ms, .. }, I::ToggleRequested | I::AutoStop) => {
                self.stop(*started_at_ms, now_ms, None)
            }
            (S::Recording { started_at_ms, .. }, I::AudioDeviceLost) => {
                self.stop(*started_at_ms, now_ms, Some(Notice::DeviceLost))
            }
            (S::Recording { .. }, I::Cancel) => (
                S::Idle,
                vec![
                    Effect::DiscardRecording,
                    Effect::EnableCancelKey(false),
                    Effect::Play(Sound::Cancel),
                ],
            ),
            (S::Recording { .. }, I::RecordingFailed { detail }) => (
                S::Idle,
                vec![
                    Effect::DiscardRecording,
                    Effect::EnableCancelKey(false),
                    Effect::Notify(Notice::RecordingFailed(detail)),
                ],
            ),

            // Transcription results; stale job ids are ignored.
            (
                S::Transcribing { job_id },
                I::TranscriptionFinished {
                    job_id: done,
                    has_text,
                },
            ) => {
                if *job_id != done {
                    return Err(Ignored::StaleJob);
                }
                let effects = if has_text {
                    vec![Effect::Play(Sound::Done)]
                } else {
                    vec![]
                };
                (S::Idle, effects)
            }
            (
                S::Transcribing { job_id },
                I::TranscriptionFailed {
                    job_id: done,
                    detail,
                },
            ) => {
                if *job_id != done {
                    return Err(Ignored::StaleJob);
                }
                (
                    S::Idle,
                    vec![Effect::Notify(Notice::TranscriptionFailed(detail))],
                )
            }

            // Model lifecycle. Loading is refused while audio is in flight.
            (
                S::Onboarding | S::Idle | S::Error { .. } | S::LoadingModel { .. },
                I::LoadModel { model_id },
            ) => (
                S::LoadingModel {
                    model_id: model_id.clone(),
                },
                vec![Effect::LoadModel { model_id }],
            ),
            (S::LoadingModel { model_id }, I::ModelLoaded { model_id: loaded })
                if *model_id == loaded =>
            {
                (S::Idle, vec![])
            }
            (
                S::LoadingModel { model_id },
                I::ModelLoadFailed {
                    model_id: failed,
                    detail,
                },
            ) if *model_id == failed => (
                S::Error {
                    error: ErrorInfo {
                        code: "model_load_failed",
                        detail,
                        retry_model: Some(failed),
                    },
                },
                vec![],
            ),
            (S::Idle | S::Error { .. } | S::LoadingModel { .. }, I::NoModel) => {
                (S::Onboarding, vec![])
            }
            (S::Error { error }, I::Retry) => match &error.retry_model {
                Some(model_id) => (
                    S::LoadingModel {
                        model_id: model_id.clone(),
                    },
                    vec![Effect::LoadModel {
                        model_id: model_id.clone(),
                    }],
                ),
                None => (S::Idle, vec![]),
            },

            _ => return Err(Ignored::NotApplicable),
        };

        self.state = next;
        Ok(effects)
    }

    fn start(&mut self, now_ms: u64, mode: RecordingMode) -> (AppState, Vec<Effect>) {
        self.last_press_ms = Some(now_ms);
        (
            AppState::Recording {
                started_at_ms: now_ms,
                mode,
            },
            vec![
                Effect::StartRecording,
                Effect::EnableCancelKey(true),
                Effect::Play(Sound::Start),
            ],
        )
    }

    fn stop(
        &mut self,
        started_at_ms: u64,
        now_ms: u64,
        notice: Option<Notice>,
    ) -> (AppState, Vec<Effect>) {
        self.last_press_ms = Some(now_ms);
        let mut effects = vec![Effect::EnableCancelKey(false)];
        let next = if now_ms.saturating_sub(started_at_ms) < MIN_RECORDING_MS {
            effects.push(Effect::DiscardRecording);
            effects.push(Effect::Play(Sound::Cancel));
            AppState::Idle
        } else {
            let job_id = self.next_job_id;
            self.next_job_id += 1;
            effects.push(Effect::StopAndTranscribe { job_id });
            effects.push(Effect::Play(Sound::Stop));
            AppState::Transcribing { job_id }
        };
        effects.extend(notice.map(Effect::Notify));
        (next, effects)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use RecordingMode::{PushToTalk, Toggle};

    fn err() -> ErrorInfo {
        ErrorInfo {
            code: "model_load_failed",
            detail: "x".into(),
            retry_model: Some("m".into()),
        }
    }

    fn all_states() -> Vec<AppState> {
        vec![
            AppState::Onboarding,
            AppState::LoadingModel {
                model_id: "m".into(),
            },
            AppState::Idle,
            AppState::Recording {
                started_at_ms: 0,
                mode: Toggle,
            },
            AppState::Recording {
                started_at_ms: 0,
                mode: PushToTalk,
            },
            AppState::Transcribing { job_id: 1 },
            AppState::Error { error: err() },
        ]
    }

    fn all_inputs() -> Vec<Input> {
        vec![
            Input::HotkeyPressed(Toggle),
            Input::HotkeyPressed(PushToTalk),
            Input::HotkeyReleased,
            Input::Cancel,
            Input::ToggleRequested,
            Input::AutoStop,
            Input::AudioDeviceLost,
            Input::RecordingFailed { detail: "d".into() },
            Input::LoadModel {
                model_id: "m".into(),
            },
            Input::ModelLoaded {
                model_id: "m".into(),
            },
            Input::ModelLoadFailed {
                model_id: "m".into(),
                detail: "d".into(),
            },
            Input::NoModel,
            Input::TranscriptionFinished {
                job_id: 1,
                has_text: true,
            },
            Input::TranscriptionFailed {
                job_id: 1,
                detail: "d".into(),
            },
            Input::Retry,
        ]
    }

    /// Expected next state for every pair, at t = 10 s (well past debounce
    /// and minimum duration). `None` = input ignored, state unchanged.
    fn expected(state: &AppState, input: &Input) -> Option<AppState> {
        use AppState as S;
        use Input as I;
        let rec = |mode| S::Recording {
            started_at_ms: 10_000,
            mode,
        };
        let loading = S::LoadingModel {
            model_id: "m".into(),
        };
        match (state, input) {
            (S::Idle, I::HotkeyPressed(mode)) => Some(rec(*mode)),
            (S::Idle, I::ToggleRequested) => Some(rec(Toggle)),
            (S::Recording { mode: Toggle, .. }, I::HotkeyPressed(_))
            | (
                S::Recording {
                    mode: PushToTalk, ..
                },
                I::HotkeyReleased,
            )
            | (S::Recording { .. }, I::ToggleRequested | I::AutoStop | I::AudioDeviceLost) => {
                Some(S::Transcribing { job_id: 1 })
            }
            (S::Recording { .. }, I::Cancel | I::RecordingFailed { .. }) => Some(S::Idle),
            (S::Transcribing { .. }, I::TranscriptionFinished { .. })
            | (S::Transcribing { .. }, I::TranscriptionFailed { .. }) => Some(S::Idle),
            (
                S::Onboarding | S::Idle | S::Error { .. } | S::LoadingModel { .. },
                I::LoadModel { .. },
            ) => Some(loading),
            (S::LoadingModel { .. }, I::ModelLoaded { .. }) => Some(S::Idle),
            (S::LoadingModel { .. }, I::ModelLoadFailed { .. }) => Some(S::Error { error: err() }),
            (S::Idle | S::Error { .. } | S::LoadingModel { .. }, I::NoModel) => Some(S::Onboarding),
            (S::Error { .. }, I::Retry) => Some(loading),
            _ => None,
        }
    }

    #[test]
    fn exhaustive_transition_table() {
        for state in all_states() {
            for input in all_inputs() {
                let mut m = Machine::new(state.clone());
                let result = m.handle(input.clone(), 10_000);
                let want = expected(&state, &input);
                match want {
                    Some(next) => {
                        assert!(result.is_ok(), "{state:?} × {input:?} should be accepted");
                        let got = match (m.state(), &next) {
                            // ErrorInfo detail comes from the input.
                            (AppState::Error { error: g }, AppState::Error { .. }) => {
                                assert_eq!(g.code, "model_load_failed");
                                assert_eq!(g.retry_model.as_deref(), Some("m"));
                                continue;
                            }
                            (g, _) => g.clone(),
                        };
                        assert_eq!(got, next, "{state:?} × {input:?}");
                    }
                    None => {
                        assert!(result.is_err(), "{state:?} × {input:?} should be ignored");
                        assert_eq!(m.state(), &state, "ignored input must not change state");
                    }
                }
            }
        }
    }

    #[test]
    fn toggle_full_cycle_emits_expected_effects() {
        let mut m = Machine::new(AppState::Idle);
        let fx = m.handle(Input::HotkeyPressed(Toggle), 1_000).unwrap();
        assert_eq!(
            fx,
            vec![
                Effect::StartRecording,
                Effect::EnableCancelKey(true),
                Effect::Play(Sound::Start)
            ]
        );
        let fx = m.handle(Input::HotkeyPressed(Toggle), 3_000).unwrap();
        assert_eq!(
            fx,
            vec![
                Effect::EnableCancelKey(false),
                Effect::StopAndTranscribe { job_id: 1 },
                Effect::Play(Sound::Stop)
            ]
        );
        let fx = m
            .handle(
                Input::TranscriptionFinished {
                    job_id: 1,
                    has_text: true,
                },
                3_500,
            )
            .unwrap();
        assert_eq!(fx, vec![Effect::Play(Sound::Done)]);
        assert_eq!(m.state(), &AppState::Idle);
    }

    #[test]
    fn empty_transcription_plays_no_done_sound() {
        let mut m = Machine::new(AppState::Transcribing { job_id: 1 });
        let fx = m
            .handle(
                Input::TranscriptionFinished {
                    job_id: 1,
                    has_text: false,
                },
                0,
            )
            .unwrap();
        assert!(fx.is_empty());
        assert_eq!(m.state(), &AppState::Idle);
    }

    #[test]
    fn press_during_transcription_is_ignored() {
        let mut m = Machine::new(AppState::Transcribing { job_id: 1 });
        assert_eq!(
            m.handle(Input::HotkeyPressed(Toggle), 10_000),
            Err(Ignored::NotApplicable)
        );
        assert_eq!(m.state(), &AppState::Transcribing { job_id: 1 });
    }

    #[test]
    fn presses_within_debounce_are_ignored() {
        let mut m = Machine::new(AppState::Idle);
        m.handle(Input::HotkeyPressed(Toggle), 1_000).unwrap();
        assert_eq!(
            m.handle(Input::HotkeyPressed(Toggle), 1_000 + DEBOUNCE_MS - 1),
            Err(Ignored::Debounced)
        );
        assert!(matches!(m.state(), AppState::Recording { .. }));
    }

    #[test]
    fn too_short_recording_is_discarded() {
        let mut m = Machine::new(AppState::Idle);
        m.handle(Input::HotkeyPressed(PushToTalk), 0).unwrap();
        let fx = m
            .handle(Input::HotkeyReleased, MIN_RECORDING_MS - 1)
            .unwrap();
        assert!(fx.contains(&Effect::DiscardRecording));
        assert_eq!(m.state(), &AppState::Idle);
    }

    #[test]
    fn push_to_talk_recording_stops_only_on_release() {
        let mut m = Machine::new(AppState::Idle);
        m.handle(Input::HotkeyPressed(PushToTalk), 0).unwrap();
        for mode in [PushToTalk, Toggle] {
            assert_eq!(
                m.handle(Input::HotkeyPressed(mode), 1_000),
                Err(Ignored::NotApplicable),
                "{mode:?} press while holding push-to-talk"
            );
        }
        m.handle(Input::HotkeyReleased, 2_000).unwrap();
        assert_eq!(m.state(), &AppState::Transcribing { job_id: 1 });
    }

    #[test]
    fn toggle_recording_ignores_release_and_stops_on_either_shortcut() {
        let mut m = Machine::new(AppState::Idle);
        m.handle(Input::HotkeyPressed(Toggle), 0).unwrap();
        assert_eq!(
            m.handle(Input::HotkeyReleased, 1_000),
            Err(Ignored::NotApplicable)
        );
        m.handle(Input::HotkeyPressed(PushToTalk), 2_000).unwrap();
        assert_eq!(m.state(), &AppState::Transcribing { job_id: 1 });
    }

    #[test]
    fn stale_transcription_result_is_ignored() {
        let mut m = Machine::new(AppState::Transcribing { job_id: 2 });
        assert_eq!(
            m.handle(
                Input::TranscriptionFinished {
                    job_id: 1,
                    has_text: true
                },
                0
            ),
            Err(Ignored::StaleJob)
        );
    }

    #[test]
    fn job_ids_increase() {
        let mut m = Machine::new(AppState::Idle);
        for expected in 1..=3 {
            let t = expected * 10_000;
            m.handle(Input::HotkeyPressed(Toggle), t).unwrap();
            m.handle(Input::HotkeyPressed(Toggle), t + 1_000).unwrap();
            assert_eq!(m.state(), &AppState::Transcribing { job_id: expected });
            m.handle(
                Input::TranscriptionFinished {
                    job_id: expected,
                    has_text: true,
                },
                t + 2_000,
            )
            .unwrap();
        }
    }

    #[test]
    fn load_error_then_retry_then_idle_without_restart() {
        let mut m = Machine::new(AppState::Idle);
        m.handle(
            Input::LoadModel {
                model_id: "a".into(),
            },
            0,
        )
        .unwrap();
        m.handle(
            Input::ModelLoadFailed {
                model_id: "a".into(),
                detail: "gpu".into(),
            },
            0,
        )
        .unwrap();
        let fx = m.handle(Input::Retry, 0).unwrap();
        assert_eq!(
            fx,
            vec![Effect::LoadModel {
                model_id: "a".into()
            }]
        );
        m.handle(
            Input::ModelLoaded {
                model_id: "a".into(),
            },
            0,
        )
        .unwrap();
        assert_eq!(m.state(), &AppState::Idle);
    }

    #[test]
    fn result_of_superseded_model_load_is_ignored() {
        let mut m = Machine::new(AppState::LoadingModel {
            model_id: "b".into(),
        });
        assert!(m
            .handle(
                Input::ModelLoaded {
                    model_id: "a".into()
                },
                0
            )
            .is_err());
    }

    #[test]
    fn device_lost_transcribes_and_notifies() {
        let mut m = Machine::new(AppState::Idle);
        m.handle(Input::HotkeyPressed(Toggle), 0).unwrap();
        let fx = m.handle(Input::AudioDeviceLost, 1_000).unwrap();
        assert!(fx.contains(&Effect::StopAndTranscribe { job_id: 1 }));
        assert!(fx.contains(&Effect::Notify(Notice::DeviceLost)));
    }
}
