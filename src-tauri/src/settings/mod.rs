//! Persistent settings: versioned schema, migrations, strict validation and
//! atomic writes. Debounced saving lives in the actor.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::stt::postprocess::Replacement;

pub const SCHEMA_VERSION: u32 = 2;
pub const FILE_NAME: &str = "settings.json";

#[derive(Debug, thiserror::Error)]
pub enum SettingsError {
    #[error("cannot write settings: {0}")]
    Io(#[from] std::io::Error),
    #[error("cannot serialize settings: {0}")]
    Serialize(#[from] serde_json::Error),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UiLanguage {
    System,
    Fr,
    En,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Acceleration {
    Auto,
    Cpu,
    Gpu,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InjectionMode {
    Paste,
    Type,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct OverlayPosition {
    pub x: f64,
    pub y: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub schema_version: u32,

    // General
    pub ui_language: UiLanguage,
    pub autostart: bool,
    pub sounds_enabled: bool,
    /// 0.0 – 1.0
    pub sound_volume: f32,
    pub overlay_enabled: bool,
    pub history_enabled: bool,
    pub onboarding_done: bool,

    // Models
    pub active_model: Option<String>,
    /// `None` = `<app_data>/models`.
    pub models_dir: Option<PathBuf>,

    // Transcription
    /// ISO 639-1 code, or "auto".
    pub language: String,
    pub vocabulary: String,
    pub replacements: Vec<Replacement>,
    pub capitalize_first: bool,
    pub strip_final_period: bool,
    pub auto_space: bool,
    /// 0 = automatic (5 on GPU, greedy on CPU).
    pub beam_size: u32,

    // Recording / hardware
    /// `None` = system default device.
    pub microphone: Option<String>,
    pub acceleration: Acceleration,
    /// 0 = automatic.
    pub threads: u32,
    pub max_duration_s: u32,
    pub silence_auto_stop: bool,
    pub silence_timeout_s: u32,

    // Shortcuts. `None` = disabled; at least one dictation shortcut is set.
    /// Press to start, press to stop.
    pub hotkey: Option<String>,
    /// Hold to talk.
    pub ptt_hotkey: Option<String>,
    /// Pastes the last dictation again.
    pub paste_last_hotkey: Option<String>,
    pub cancel_key: String,

    // Injection
    pub injection_mode: InjectionMode,
    pub restore_delay_ms: u32,
    pub keep_in_clipboard: bool,

    // Advanced
    pub check_updates: bool,
    /// Logs transcribed text. Shown as a visible warning in the UI.
    pub debug_log_text: bool,

    /// Overlay position per monitor name.
    pub overlay_positions: HashMap<String, OverlayPosition>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            ui_language: UiLanguage::System,
            autostart: true,
            sounds_enabled: true,
            sound_volume: 0.5,
            overlay_enabled: true,
            history_enabled: true,
            onboarding_done: false,
            active_model: None,
            models_dir: None,
            language: "auto".into(),
            vocabulary: String::new(),
            replacements: Vec::new(),
            capitalize_first: true,
            strip_final_period: false,
            auto_space: true,
            beam_size: 0,
            microphone: None,
            acceleration: Acceleration::Auto,
            threads: 0,
            max_duration_s: 300,
            silence_auto_stop: false,
            silence_timeout_s: 3,
            hotkey: Some(DEFAULT_HOTKEY.into()),
            ptt_hotkey: Some(DEFAULT_PTT_HOTKEY.into()),
            paste_last_hotkey: None,
            cancel_key: "Escape".into(),
            injection_mode: InjectionMode::Paste,
            restore_delay_ms: 300,
            keep_in_clipboard: true,
            check_updates: true,
            debug_log_text: false,
            overlay_positions: HashMap::new(),
        }
    }
}

pub const DEFAULT_HOTKEY: &str = "Ctrl+Space";
pub const DEFAULT_PTT_HOTKEY: &str = "F8";

/// The global shortcuts to register. `cancel` is only active while recording.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Shortcuts {
    pub toggle: Option<String>,
    pub push_to_talk: Option<String>,
    pub paste_last: Option<String>,
    pub cancel: String,
}

/// Languages accepted for `language` (Whisper codes), besides "auto".
pub const LANGUAGES: &[&str] = &[
    "fr", "en", "de", "es", "it", "pt", "nl", "pl", "ru", "uk", "cs", "sv", "da", "no", "fi", "el",
    "tr", "ro", "hu", "bg", "hr", "sk", "sl", "lt", "lv", "et", "ca", "eu", "gl", "ar", "he", "fa",
    "hi", "bn", "ja", "ko", "zh", "vi", "th", "id", "ms",
];

impl Settings {
    pub fn shortcuts(&self) -> Shortcuts {
        Shortcuts {
            toggle: self.hotkey.clone(),
            push_to_talk: self.ptt_hotkey.clone(),
            paste_last: self.paste_last_hotkey.clone(),
            cancel: self.cancel_key.clone(),
        }
    }

    /// Replaces out-of-range values by their default. Returns the names of
    /// the fields that were reset.
    pub fn validate(&mut self) -> Vec<&'static str> {
        let d = Settings::default();
        let mut reset = Vec::new();
        let mut check = |ok: bool, name: &'static str, fix: &mut dyn FnMut()| {
            if !ok {
                fix();
                reset.push(name);
            }
        };
        check(
            (0.0..=1.0).contains(&self.sound_volume),
            "sound_volume",
            &mut || self.sound_volume = d.sound_volume,
        );
        check(
            self.language == "auto" || LANGUAGES.contains(&self.language.as_str()),
            "language",
            &mut || self.language = d.language.clone(),
        );
        check(self.beam_size <= 10, "beam_size", &mut || {
            self.beam_size = d.beam_size
        });
        check(self.threads <= 64, "threads", &mut || self.threads = 0);
        check(
            (5..=3600).contains(&self.max_duration_s),
            "max_duration_s",
            &mut || self.max_duration_s = d.max_duration_s,
        );
        check(
            (1..=60).contains(&self.silence_timeout_s),
            "silence_timeout_s",
            &mut || self.silence_timeout_s = d.silence_timeout_s,
        );
        check(
            self.restore_delay_ms <= 5000,
            "restore_delay_ms",
            &mut || self.restore_delay_ms = d.restore_delay_ms,
        );
        let valid = |k: &Option<String>| k.as_deref().is_none_or(is_shortcut);
        check(valid(&self.hotkey), "hotkey", &mut || self.hotkey = None);
        check(valid(&self.ptt_hotkey), "ptt_hotkey", &mut || {
            self.ptt_hotkey = None
        });
        check(
            valid(&self.paste_last_hotkey),
            "paste_last_hotkey",
            &mut || self.paste_last_hotkey = None,
        );
        check(is_shortcut(&self.cancel_key), "cancel_key", &mut || {
            self.cancel_key = d.cancel_key.clone()
        });
        // Each key does one thing: a later duplicate of an earlier key is
        // turned off (the cancel key goes back to its default instead).
        let same = |a: &Option<String>, b: &str| a.as_deref().is_some_and(|a| same_key(a, b));
        let toggle = self.hotkey.clone();
        check(
            !self.ptt_hotkey.as_deref().is_some_and(|p| same(&toggle, p)),
            "ptt_hotkey",
            &mut || self.ptt_hotkey = None,
        );
        let dictation = [self.hotkey.clone(), self.ptt_hotkey.clone()];
        check(
            !self
                .paste_last_hotkey
                .as_deref()
                .is_some_and(|k| dictation.iter().any(|d| same(d, k))),
            "paste_last_hotkey",
            &mut || self.paste_last_hotkey = None,
        );
        let taken = [
            self.hotkey.clone(),
            self.ptt_hotkey.clone(),
            self.paste_last_hotkey.clone(),
        ];
        check(
            !taken.iter().any(|k| same(k, &self.cancel_key)),
            "cancel_key",
            &mut || self.cancel_key = d.cancel_key.clone(),
        );
        check(
            self.hotkey.is_some() || self.ptt_hotkey.is_some(),
            "hotkey",
            &mut || self.hotkey = d.hotkey.clone(),
        );
        check(
            self.models_dir.as_ref().is_none_or(|p| p.is_absolute()),
            "models_dir",
            &mut || self.models_dir = None,
        );
        check(self.vocabulary.len() <= 2000, "vocabulary", &mut || {
            self.vocabulary = d.vocabulary.clone()
        });
        reset
    }
}

/// Parses with the global shortcut syntax ("Ctrl+Shift+Space", "F9").
pub fn is_shortcut(s: &str) -> bool {
    s.parse::<tauri_plugin_global_shortcut::Shortcut>().is_ok()
}

/// Same key combination, whatever the spelling ("ctrl+space", "Control+Space").
fn same_key(a: &str, b: &str) -> bool {
    use tauri_plugin_global_shortcut::Shortcut;
    match (a.parse::<Shortcut>(), b.parse::<Shortcut>()) {
        (Ok(a), Ok(b)) => a == b,
        _ => a.eq_ignore_ascii_case(b),
    }
}

/// What happened while loading, for logging and user notification.
#[derive(Debug, PartialEq)]
pub enum LoadOutcome {
    Loaded,
    /// No file yet (first launch).
    Created,
    /// File unreadable: moved aside to this path, defaults used.
    Corrupt {
        backup: PathBuf,
    },
}

/// Loads settings from `dir`. Never fails: worst case yields defaults.
pub fn load(dir: &Path) -> (Settings, LoadOutcome) {
    let path = dir.join(FILE_NAME);
    let raw = match std::fs::read_to_string(&path) {
        Ok(raw) => raw,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return (Settings::default(), LoadOutcome::Created)
        }
        Err(e) => {
            tracing::error!("cannot read settings: {e}");
            return corrupt(&path);
        }
    };
    let parsed = serde_json::from_str::<Value>(&raw)
        .ok()
        .and_then(migrate)
        .and_then(|v| serde_json::from_value::<Settings>(v).ok());
    let Some(mut settings) = parsed else {
        return corrupt(&path);
    };
    for field in settings.validate() {
        tracing::warn!(field, "invalid setting reset to default");
    }
    (settings, LoadOutcome::Loaded)
}

fn corrupt(path: &Path) -> (Settings, LoadOutcome) {
    let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S");
    let backup = path.with_file_name(format!("settings.corrupt-{stamp}.json"));
    if let Err(e) = std::fs::rename(path, &backup) {
        tracing::error!("cannot move corrupt settings aside: {e}");
    }
    tracing::warn!(backup = %backup.display(), "settings file corrupt, defaults loaded");
    (Settings::default(), LoadOutcome::Corrupt { backup })
}

/// Upgrades an older JSON document to [`SCHEMA_VERSION`]. `None` if the
/// document is not an object or comes from a newer version.
fn migrate(mut v: Value) -> Option<Value> {
    let obj = v.as_object_mut()?;
    let version = obj
        .get("schema_version")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    if version > u64::from(SCHEMA_VERSION) {
        return None;
    }
    if version == 0 {
        // Pre-release files had no version and a boolean `push_to_talk`.
        if let Some(ptt) = obj.remove("push_to_talk").and_then(|p| p.as_bool()) {
            let mode = if ptt { "push_to_talk" } else { "toggle" };
            obj.insert("recording_mode".into(), mode.into());
        }
    }
    if version < 2 {
        // v1 had one shortcut and a mode; v2 has one shortcut per mode.
        // Hold-to-talk users keep their key for it and get the default
        // toggle shortcut; others get the default hold-to-talk key.
        let ptt = obj
            .remove("recording_mode")
            .and_then(|m| m.as_str().map(str::to_owned))
            == Some("push_to_talk".into());
        let hotkey = obj
            .remove("hotkey")
            .and_then(|h| h.as_str().map(str::to_owned));
        let hotkey = hotkey.unwrap_or_else(|| DEFAULT_HOTKEY.into());
        let (toggle, hold) = if ptt {
            let toggle = (!same_key(&hotkey, DEFAULT_HOTKEY)).then(|| DEFAULT_HOTKEY.to_owned());
            (toggle, Some(hotkey))
        } else {
            let hold =
                (!same_key(&hotkey, DEFAULT_PTT_HOTKEY)).then(|| DEFAULT_PTT_HOTKEY.to_owned());
            (Some(hotkey), hold)
        };
        obj.insert("hotkey".into(), toggle.into());
        obj.insert("ptt_hotkey".into(), hold.into());
    }
    obj.insert("schema_version".into(), SCHEMA_VERSION.into());
    Some(v)
}

/// Writes atomically: temp file in the same directory, then rename.
pub fn save(dir: &Path, settings: &Settings) -> Result<(), SettingsError> {
    let json = serde_json::to_vec_pretty(settings)?;
    crate::fsutil::write_atomic(&dir.join(FILE_NAME), &json)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_file_gives_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let (s, outcome) = load(dir.path());
        assert_eq!(s, Settings::default());
        assert_eq!(outcome, LoadOutcome::Created);
    }

    #[test]
    fn save_then_load_round_trips() {
        let dir = tempfile::tempdir().unwrap();
        let mut s = Settings {
            language: "fr".into(),
            ptt_hotkey: None,
            paste_last_hotkey: Some("Ctrl+Shift+V".into()),
            ..Settings::default()
        };
        s.overlay_positions
            .insert("DISPLAY1".into(), OverlayPosition { x: 10.0, y: 20.0 });
        save(dir.path(), &s).unwrap();
        assert!(!dir.path().join("settings.json.tmp").exists());
        let (loaded, outcome) = load(dir.path());
        assert_eq!(outcome, LoadOutcome::Loaded);
        assert_eq!(loaded, s);
    }

    #[test]
    fn corrupt_file_is_moved_aside() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(FILE_NAME), "{ not json").unwrap();
        let (s, outcome) = load(dir.path());
        assert_eq!(s, Settings::default());
        let LoadOutcome::Corrupt { backup } = outcome else {
            panic!("expected corrupt outcome");
        };
        assert!(backup.exists());
        assert!(!dir.path().join(FILE_NAME).exists());
    }

    #[test]
    fn newer_schema_is_treated_as_corrupt() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(FILE_NAME), r#"{"schema_version": 99}"#).unwrap();
        assert!(matches!(load(dir.path()).1, LoadOutcome::Corrupt { .. }));
    }

    #[test]
    fn out_of_range_values_reset_individually() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join(FILE_NAME),
            r#"{"schema_version":2,"sound_volume":7,"language":"klingon","beam_size":42,"hotkey":"Alt+D"}"#,
        )
        .unwrap();
        let (s, outcome) = load(dir.path());
        assert_eq!(outcome, LoadOutcome::Loaded);
        assert_eq!(s.sound_volume, 0.5);
        assert_eq!(s.language, "auto");
        assert_eq!(s.beam_size, 0);
        assert_eq!(s.hotkey.as_deref(), Some("Alt+D"), "valid values are kept");
    }

    #[test]
    fn invalid_shortcuts_and_relative_models_dir_are_reset() {
        let mut s = Settings {
            hotkey: Some("Ctrl+Nope".into()),
            paste_last_hotkey: Some("Nope".into()),
            cancel_key: "".into(),
            models_dir: Some(PathBuf::from("relative/dir")),
            ..Settings::default()
        };
        let mut reset = s.validate();
        reset.sort_unstable();
        assert_eq!(
            reset,
            ["cancel_key", "hotkey", "models_dir", "paste_last_hotkey"]
        );
        assert_eq!(s.hotkey, None, "an invalid key is turned off");
        assert_eq!(s.ptt_hotkey.as_deref(), Some("F8"));
        assert_eq!(s.paste_last_hotkey, None);
        assert_eq!(s.cancel_key, "Escape");
        assert_eq!(s.models_dir, None);
    }

    #[test]
    fn a_key_does_one_thing() {
        let mut s = Settings {
            ptt_hotkey: Some("control+space".into()),
            paste_last_hotkey: Some("Ctrl+Space".into()),
            cancel_key: "ctrl+space".into(),
            ..Settings::default()
        };
        let mut reset = s.validate();
        reset.sort_unstable();
        assert_eq!(reset, ["cancel_key", "paste_last_hotkey", "ptt_hotkey"]);
        assert_eq!(s.hotkey.as_deref(), Some("Ctrl+Space"));
        assert_eq!(s.ptt_hotkey, None);
        assert_eq!(s.paste_last_hotkey, None);
        assert_eq!(s.cancel_key, "Escape");
    }

    #[test]
    fn at_least_one_dictation_shortcut() {
        let mut s = Settings {
            hotkey: None,
            ptt_hotkey: None,
            ..Settings::default()
        };
        assert_eq!(s.validate(), ["hotkey"]);
        assert_eq!(s.hotkey.as_deref(), Some("Ctrl+Space"));

        let mut hold_only = Settings {
            hotkey: None,
            ..Settings::default()
        };
        assert!(hold_only.validate().is_empty());
    }

    #[test]
    fn wrong_type_is_corrupt() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join(FILE_NAME),
            r#"{"schema_version":2,"autostart":"yes"}"#,
        )
        .unwrap();
        assert!(matches!(load(dir.path()).1, LoadOutcome::Corrupt { .. }));
    }

    #[test]
    fn migrates_v0_push_to_talk_flag() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join(FILE_NAME),
            r#"{"push_to_talk":true,"language":"fr"}"#,
        )
        .unwrap();
        let (s, outcome) = load(dir.path());
        assert_eq!(outcome, LoadOutcome::Loaded);
        assert_eq!(s.schema_version, SCHEMA_VERSION);
        assert_eq!(s.ptt_hotkey.as_deref(), Some("Ctrl+Space"));
        assert_eq!(s.hotkey, None);
        assert_eq!(s.language, "fr");
    }

    fn migrated(v1: &str) -> Settings {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(FILE_NAME), v1).unwrap();
        let (s, outcome) = load(dir.path());
        assert_eq!(outcome, LoadOutcome::Loaded, "{v1}");
        s
    }

    #[test]
    fn migrates_v1_single_shortcut() {
        let s = migrated(r#"{"schema_version":1,"hotkey":"Alt+D","recording_mode":"toggle"}"#);
        assert_eq!(s.hotkey.as_deref(), Some("Alt+D"));
        assert_eq!(s.ptt_hotkey.as_deref(), Some("F8"));

        let s =
            migrated(r#"{"schema_version":1,"hotkey":"Alt+D","recording_mode":"push_to_talk"}"#);
        assert_eq!(s.hotkey.as_deref(), Some("Ctrl+Space"));
        assert_eq!(s.ptt_hotkey.as_deref(), Some("Alt+D"));

        let s = migrated(r#"{"schema_version":1,"hotkey":"F8","recording_mode":"toggle"}"#);
        assert_eq!(s.hotkey.as_deref(), Some("F8"));
        assert_eq!(s.ptt_hotkey, None, "no clash with the old key");

        let s = migrated(r#"{"schema_version":1}"#);
        assert_eq!(s.hotkey.as_deref(), Some("Ctrl+Space"));
        assert_eq!(s.ptt_hotkey.as_deref(), Some("F8"));
    }

    #[test]
    fn unknown_fields_are_ignored() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join(FILE_NAME),
            r#"{"schema_version":2,"removed_option":3}"#,
        )
        .unwrap();
        assert_eq!(load(dir.path()).1, LoadOutcome::Loaded);
    }
}
