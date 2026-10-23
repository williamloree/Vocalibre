//! Desktop side of the actor: tray menu, overlay window and the cancel key.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::Serialize;
use tauri::menu::{CheckMenuItem, IsMenuItem, Menu, MenuItem, PredefinedMenuItem, Submenu};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Emitter, Manager, WebviewUrl, WebviewWindow, WebviewWindowBuilder, Wry};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut};

use crate::app::actor::Shell;
use crate::app::state::{AppState, Notice};
use crate::models::manifest::ModelInfo;

pub const OVERLAY: &str = "overlay";
const OVERLAY_W: f64 = 240.0;
const OVERLAY_H: f64 = 56.0;
/// How long an error stays on the overlay.
const NOTICE_FOR: Duration = Duration::from_secs(4);

/// The registered shortcuts, read by the global shortcut handler.
pub struct Hotkeys {
    pub main: Shortcut,
    pub cancel: Shortcut,
}

#[derive(Clone, Serialize)]
struct NoticePayload {
    code: &'static str,
    detail: String,
}

pub struct TrayItems {
    status: MenuItem<Wry>,
    toggle: MenuItem<Wry>,
    retry: MenuItem<Wry>,
    models: Vec<(String, CheckMenuItem<Wry>)>,
}

pub struct TauriShell {
    app: AppHandle,
    tray: TrayItems,
    hotkey_label: String,
    overlay_enabled: bool,
    /// Bumped on every show, so a pending hide only applies to its own show.
    overlay_gen: Arc<AtomicU64>,
    notice_until: Arc<Mutex<Option<Instant>>>,
}

impl TauriShell {
    pub fn new(
        app: AppHandle,
        tray: TrayItems,
        hotkey_label: String,
        overlay_enabled: bool,
    ) -> Self {
        Self {
            app,
            tray,
            hotkey_label,
            overlay_enabled,
            overlay_gen: Arc::default(),
            notice_until: Arc::default(),
        }
    }

    fn overlay(&self) -> Option<WebviewWindow> {
        self.app.get_webview_window(OVERLAY)
    }

    fn show_overlay(&self) {
        self.overlay_gen.fetch_add(1, Ordering::SeqCst);
        if let Some(w) = self.overlay() {
            if let Err(e) = w.show() {
                tracing::warn!("cannot show overlay: {e}");
            }
        }
    }

    fn hide_overlay_after(&self, delay: Duration) {
        let gen = self.overlay_gen.load(Ordering::SeqCst);
        let counter = self.overlay_gen.clone();
        let Some(w) = self.overlay() else { return };
        tauri::async_runtime::spawn(async move {
            tokio::time::sleep(delay).await;
            if counter.load(Ordering::SeqCst) == gen {
                let _ = w.hide();
            }
        });
    }

    fn remaining_notice(&self) -> Duration {
        let until = self.notice_until.lock().ok().and_then(|g| *g);
        until.map_or(Duration::ZERO, |t| {
            t.saturating_duration_since(Instant::now())
        })
    }

    fn status_text(&self, state: &AppState) -> String {
        match state {
            AppState::Onboarding => "Aucun modèle installé".into(),
            AppState::LoadingModel { .. } => "Chargement du modèle…".into(),
            AppState::Idle => format!("Prêt · {}", self.hotkey_label),
            AppState::Recording { .. } => "Écoute… (Échap pour annuler)".into(),
            AppState::Transcribing { .. } => "Transcription…".into(),
            AppState::Error { error } => format!("Erreur : {}", error.detail),
        }
    }
}

impl Shell for TauriShell {
    fn state_changed(&self, state: &AppState) {
        let status = self.status_text(state);
        let _ = self.tray.status.set_text(&status);
        if let Some(tray) = self.app.tray_by_id("main") {
            let _ = tray.set_tooltip(Some(format!("Vocalibre · {status}")));
        }
        let recording = matches!(state, AppState::Recording { .. });
        let _ = self.tray.toggle.set_text(if recording {
            "Arrêter la dictée"
        } else {
            "Démarrer la dictée"
        });
        let _ = self
            .tray
            .toggle
            .set_enabled(matches!(state, AppState::Idle | AppState::Recording { .. }));
        let _ = self
            .tray
            .retry
            .set_enabled(matches!(state, AppState::Error { .. }));

        let _ = self.app.emit_to(OVERLAY, "state", state);
        if !self.overlay_enabled {
            return;
        }
        match state {
            AppState::Recording { .. } | AppState::Transcribing { .. } => self.show_overlay(),
            AppState::Error { error } => {
                self.notice(&Notice::TranscriptionFailed(error.detail.clone()));
            }
            _ => self.hide_overlay_after(self.remaining_notice()),
        }
    }

    fn level(&self, level: f32) {
        let _ = self.app.emit_to(OVERLAY, "level", level);
    }

    fn notice(&self, notice: &Notice) {
        let (code, detail) = match notice {
            Notice::DeviceLost => ("device_lost", String::new()),
            Notice::RecordingFailed(d) => ("recording_failed", d.clone()),
            Notice::TranscriptionFailed(d) => ("transcription_failed", d.clone()),
        };
        let _ = self
            .app
            .emit_to(OVERLAY, "notice", NoticePayload { code, detail });
        if !self.overlay_enabled {
            return;
        }
        if let Ok(mut g) = self.notice_until.lock() {
            *g = Some(Instant::now() + NOTICE_FOR);
        }
        self.show_overlay();
        self.hide_overlay_after(NOTICE_FOR);
    }

    fn model_changed(&self, model_id: &str) {
        for (id, item) in &self.tray.models {
            let _ = item.set_checked(id == model_id);
        }
    }

    fn set_cancel_key(&self, enabled: bool) {
        let Some(keys) = self.app.try_state::<Hotkeys>() else {
            return;
        };
        let gs = self.app.global_shortcut();
        let result = if enabled {
            gs.register(keys.cancel)
        } else if gs.is_registered(keys.cancel) {
            gs.unregister(keys.cancel)
        } else {
            Ok(())
        };
        if let Err(e) = result {
            // Cancelling is a convenience: recording still works without it.
            tracing::warn!("cancel key: {e}");
        }
    }
}

/// Builds the tray. Menu events are handled by `on_menu`.
pub fn build_tray(
    app: &AppHandle,
    installed: &[&ModelInfo],
    on_menu: impl Fn(&AppHandle, &str) + Send + Sync + 'static,
) -> tauri::Result<TrayItems> {
    let status = MenuItem::with_id(app, "status", "Démarrage…", false, None::<&str>)?;
    let toggle = MenuItem::with_id(app, "toggle", "Démarrer la dictée", false, None::<&str>)?;
    let retry = MenuItem::with_id(app, "retry", "Réessayer", false, None::<&str>)?;
    let models = installed
        .iter()
        .map(|m| {
            let item = CheckMenuItem::with_id(
                app,
                format!("model:{}", m.id),
                &m.display_name,
                true,
                false,
                None::<&str>,
            )?;
            Ok((m.id.clone(), item))
        })
        .collect::<tauri::Result<Vec<_>>>()?;
    let model_refs: Vec<&dyn IsMenuItem<Wry>> = models
        .iter()
        .map(|(_, i)| i as &dyn IsMenuItem<Wry>)
        .collect();
    let model_menu = Submenu::with_items(app, "Modèle", !models.is_empty(), &model_refs)?;
    let menu = Menu::with_items(
        app,
        &[
            &status,
            &PredefinedMenuItem::separator(app)?,
            &toggle,
            &retry,
            &model_menu,
            &PredefinedMenuItem::separator(app)?,
            &MenuItem::with_id(
                app,
                "models_dir",
                "Ouvrir le dossier des modèles",
                true,
                None::<&str>,
            )?,
            &MenuItem::with_id(
                app,
                "logs_dir",
                "Ouvrir le dossier des journaux",
                true,
                None::<&str>,
            )?,
            &PredefinedMenuItem::separator(app)?,
            &MenuItem::with_id(app, "quit", "Quitter", true, None::<&str>)?,
        ],
    )?;
    let mut builder = TrayIconBuilder::with_id("main")
        .tooltip("Vocalibre")
        .menu(&menu)
        .show_menu_on_left_click(true)
        .on_menu_event(move |app, event| on_menu(app, event.id().as_ref()));
    if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());
    }
    builder.build(app)?;
    Ok(TrayItems {
        status,
        toggle,
        retry,
        models,
    })
}

/// Hidden, non-focusable pill at the bottom centre of the primary screen:
/// showing it must never steal focus from the app receiving the text.
pub fn build_overlay(app: &AppHandle) -> tauri::Result<WebviewWindow> {
    let mut builder = WebviewWindowBuilder::new(app, OVERLAY, WebviewUrl::App("index.html".into()))
        .title("Vocalibre")
        .inner_size(OVERLAY_W, OVERLAY_H)
        .resizable(false)
        .decorations(false)
        .transparent(true)
        .shadow(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .focusable(false)
        .focused(false)
        .visible(false);
    if let Some(m) = app.primary_monitor()? {
        let scale = m.scale_factor();
        let area = m.work_area();
        let (w, h) = (
            area.size.width as f64 / scale,
            area.size.height as f64 / scale,
        );
        let (x, y) = (
            area.position.x as f64 / scale,
            area.position.y as f64 / scale,
        );
        builder = builder.position(x + (w - OVERLAY_W) / 2.0, y + h - OVERLAY_H - 48.0);
    }
    let window = builder.build()?;
    // Clicks go through to the app underneath.
    window.set_ignore_cursor_events(true)?;
    Ok(window)
}
