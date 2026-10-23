//! Strings shown by the Rust side (tray). The windows translate themselves
//! with the same resolved [`Lang`].

use serde::Serialize;

use crate::settings::UiLanguage;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Lang {
    Fr,
    En,
}

pub fn resolve(ui: UiLanguage) -> Lang {
    match ui {
        UiLanguage::Fr => Lang::Fr,
        UiLanguage::En => Lang::En,
        UiLanguage::System => from_locale(sys_locale::get_locale().as_deref()),
    }
}

/// French for any French locale, English otherwise.
pub fn from_locale(locale: Option<&str>) -> Lang {
    match locale {
        Some(l) if l.to_ascii_lowercase().starts_with("fr") => Lang::Fr,
        _ => Lang::En,
    }
}

pub struct Strings {
    pub starting: &'static str,
    pub no_model: &'static str,
    pub loading: &'static str,
    /// `{hotkey}` is replaced.
    pub ready: &'static str,
    pub recording: &'static str,
    pub transcribing: &'static str,
    pub error: &'static str,
    /// `{hotkey}` is replaced.
    pub hotkey_unavailable: &'static str,
    pub start: &'static str,
    pub stop: &'static str,
    pub retry: &'static str,
    pub model: &'static str,
    pub history: &'static str,
    pub settings: &'static str,
    pub open_logs: &'static str,
    pub quit: &'static str,
}

const FR: Strings = Strings {
    starting: "Démarrage…",
    no_model: "Aucun modèle installé",
    loading: "Chargement du modèle…",
    ready: "Prêt · {hotkey}",
    recording: "Écoute… (Échap pour annuler)",
    transcribing: "Transcription…",
    error: "Erreur",
    hotkey_unavailable: "Raccourci {hotkey} indisponible",
    start: "Démarrer la dictée",
    stop: "Arrêter la dictée",
    retry: "Réessayer",
    model: "Modèle",
    history: "Historique",
    settings: "Paramètres",
    open_logs: "Ouvrir le dossier des journaux",
    quit: "Quitter",
};

const EN: Strings = Strings {
    starting: "Starting…",
    no_model: "No model installed",
    loading: "Loading model…",
    ready: "Ready · {hotkey}",
    recording: "Listening… (Esc to cancel)",
    transcribing: "Transcribing…",
    error: "Error",
    hotkey_unavailable: "Shortcut {hotkey} unavailable",
    start: "Start dictation",
    stop: "Stop dictation",
    retry: "Retry",
    model: "Model",
    history: "History",
    settings: "Settings",
    open_logs: "Open logs folder",
    quit: "Quit",
};

pub fn strings(lang: Lang) -> &'static Strings {
    match lang {
        Lang::Fr => &FR,
        Lang::En => &EN,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn locale_resolution() {
        assert_eq!(from_locale(Some("fr-FR")), Lang::Fr);
        assert_eq!(from_locale(Some("FR_ca")), Lang::Fr);
        assert_eq!(from_locale(Some("en-US")), Lang::En);
        assert_eq!(from_locale(Some("de-DE")), Lang::En);
        assert_eq!(from_locale(None), Lang::En);
        assert_eq!(resolve(UiLanguage::Fr), Lang::Fr);
        assert_eq!(resolve(UiLanguage::En), Lang::En);
    }
}
