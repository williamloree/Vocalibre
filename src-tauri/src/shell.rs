//! Desktop side of the actor: tray, overlay, main window, global shortcuts
//! and autostart.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::Serialize;
use tauri::image::Image;
use tauri::menu::{CheckMenuItem, IsMenuItem, Menu, MenuItem, PredefinedMenuItem, Submenu};
use tauri::tray::TrayIconBuilder;
use tauri::{
    AppHandle, Emitter, Manager, Monitor, PhysicalPosition, WebviewUrl, WebviewWindow,
    WebviewWindowBuilder, Wry,
};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut};

use crate::app::actor::{Changed, Shell, View};
use crate::app::state::{AppState, Notice};
use crate::i18n::{self, Lang};
use crate::settings::OverlayPosition;

pub const OVERLAY: &str = "overlay";
pub const MAIN: &str = "main";
const TRAY: &str = "main";
const OVERLAY_W: f64 = 220.0;
const OVERLAY_H: f64 = 48.0;
/// Default distance between the overlay and the bottom of the screen.
const OVERLAY_MARGIN: f64 = 48.0;
/// How long an error stays on the overlay.
const NOTICE_FOR: Duration = Duration::from_secs(4);

const ICON_IDLE: &[u8] = include_bytes!("../icons/tray/idle.png");
const ICON_LOADING: &[u8] = include_bytes!("../icons/tray/loading.png");
const ICON_RECORDING: &[u8] = include_bytes!("../icons/tray/recording.png");
const ICON_TRANSCRIBING: &[u8] = include_bytes!("../icons/tray/transcribing.png");
const ICON_ERROR: &[u8] = include_bytes!("../icons/tray/error.png");

/// The registered shortcuts, read by the global shortcut handler. `None`
/// until a dictation shortcut was registered successfully.
#[derive(Default)]
pub struct Hotkeys(pub Mutex<Option<HotkeyPair>>);

#[derive(Clone, Copy)]
pub struct HotkeyPair {
    pub main: Shortcut,
    pub cancel: Shortcut,
}

#[derive(Clone, Serialize)]
struct NoticePayload {
    code: &'static str,
    detail: String,
}

/// Tray menu entries updated on state changes.
struct TrayItems {
    status: MenuItem<Wry>,
    toggle: MenuItem<Wry>,
    retry: MenuItem<Wry>,
}

/// Everything the shell remembers from the last [`View`].
struct Current {
    lang: Lang,
    hotkey: String,
    overlay_enabled: bool,
    overlay_positions: std::collections::HashMap<String, OverlayPosition>,
    autostart: Option<bool>,
    state: AppState,
    tray: Option<TrayItems>,
}

pub struct TauriShell {
    app: AppHandle,
    current: Mutex<Current>,
    /// Bumped on every show, so a pending hide only applies to its own show.
    overlay_gen: Arc<AtomicU64>,
    notice_until: Mutex<Option<Instant>>,
}

impl TauriShell {
    pub fn new(app: AppHandle) -> Self {
        Self {
            app,
            current: Mutex::new(Current {
                lang: Lang::En,
                hotkey: String::new(),
                overlay_enabled: true,
                overlay_positions: Default::default(),
                autostart: None,
                state: AppState::Onboarding,
                tray: None,
            }),
            overlay_gen: Arc::default(),
            notice_until: Mutex::new(None),
        }
    }

    fn overlay(&self) -> Option<WebviewWindow> {
        self.app.get_webview_window(OVERLAY)
    }

    fn show_overlay(&self) {
        self.overlay_gen.fetch_add(1, Ordering::SeqCst);
        let Some(w) = self.overlay() else { return };
        if !w.is_visible().unwrap_or(false) {
            let positions = self
                .current
                .lock()
                .map(|c| c.overlay_positions.clone())
                .unwrap_or_default();
            place_overlay(&self.app, &w, &positions);
        }
        if let Err(e) = w.show() {
            tracing::warn!("cannot show overlay: {e}");
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

    fn update_tray(&self, c: &Current) {
        let s = i18n::strings(c.lang);
        let status = match &c.state {
            AppState::Onboarding => s.no_model.to_string(),
            AppState::LoadingModel { .. } => s.loading.to_string(),
            AppState::Idle => s.ready.replace("{hotkey}", &c.hotkey),
            AppState::Recording { .. } => s.recording.to_string(),
            AppState::Transcribing { .. } => s.transcribing.to_string(),
            AppState::Error { error } => format!("{} : {}", s.error, error.detail),
        };
        let icon = match &c.state {
            AppState::Idle => ICON_IDLE,
            AppState::Recording { .. } => ICON_RECORDING,
            AppState::Transcribing { .. } => ICON_TRANSCRIBING,
            AppState::LoadingModel { .. } => ICON_LOADING,
            AppState::Onboarding | AppState::Error { .. } => ICON_ERROR,
        };
        if let Some(tray) = self.app.tray_by_id(TRAY) {
            let _ = tray.set_tooltip(Some(format!("Vocalibre · {status}")));
            match Image::from_bytes(icon) {
                Ok(i) => {
                    let _ = tray.set_icon(Some(i));
                }
                Err(e) => tracing::warn!("tray icon: {e}"),
            }
        }
        let Some(items) = &c.tray else { return };
        let _ = items.status.set_text(&status);
        let recording = matches!(c.state, AppState::Recording { .. });
        let _ = items
            .toggle
            .set_text(if recording { s.stop } else { s.start });
        let _ = items.toggle.set_enabled(matches!(
            c.state,
            AppState::Idle | AppState::Recording { .. }
        ));
        let _ = items
            .retry
            .set_enabled(matches!(c.state, AppState::Error { .. }));
    }

    fn apply_autostart(&self, wanted: bool) {
        // A debug build would register `target/debug` to start with the OS.
        if cfg!(debug_assertions) {
            return;
        }
        use tauri_plugin_autostart::ManagerExt;
        let launcher = self.app.autolaunch();
        let result = match launcher.is_enabled() {
            Ok(on) if on == wanted => Ok(()),
            _ if wanted => launcher.enable(),
            _ => launcher.disable(),
        };
        if let Err(e) = result {
            tracing::warn!("autostart: {e}");
        }
    }
}

impl Shell for TauriShell {
    fn state_changed(&self, state: &AppState) {
        let overlay_enabled = match self.current.lock() {
            Ok(mut c) => {
                c.state = state.clone();
                self.update_tray(&c);
                c.overlay_enabled
            }
            Err(_) => return,
        };
        let _ = self.app.emit("state", state);
        if !overlay_enabled {
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
        if !self.current.lock().is_ok_and(|c| c.overlay_enabled) {
            return;
        }
        if let Ok(mut g) = self.notice_until.lock() {
            *g = Some(Instant::now() + NOTICE_FOR);
        }
        self.show_overlay();
        self.hide_overlay_after(NOTICE_FOR);
    }

    fn set_cancel_key(&self, enabled: bool) {
        let state = self.app.state::<Hotkeys>();
        let Some(pair) = state.0.lock().ok().and_then(|g| *g) else {
            return;
        };
        let gs = self.app.global_shortcut();
        let result = if enabled {
            gs.register(pair.cancel)
        } else if gs.is_registered(pair.cancel) {
            gs.unregister(pair.cancel)
        } else {
            Ok(())
        };
        if let Err(e) = result {
            // Cancelling is a convenience: recording still works without it.
            tracing::warn!("cancel key: {e}");
        }
    }

    fn set_hotkeys(&self, main: &str, cancel: &str) -> Result<(), String> {
        let main: Shortcut = main.parse().map_err(|e| format!("{e}"))?;
        let cancel: Shortcut = cancel.parse().map_err(|e| format!("{e}"))?;
        let state = self.app.state::<Hotkeys>();
        let mut current = state.0.lock().map_err(|_| "hotkeys poisoned".to_string())?;
        let gs = self.app.global_shortcut();
        let previous = *current;
        if let Some(p) = previous {
            if p.main == main {
                *current = Some(HotkeyPair { main, cancel });
                return Ok(());
            }
            if gs.is_registered(p.main) {
                let _ = gs.unregister(p.main);
            }
        }
        match gs.register(main) {
            Ok(()) => {
                *current = Some(HotkeyPair { main, cancel });
                Ok(())
            }
            Err(e) => {
                // Roll back: the old shortcut keeps working.
                if let Some(p) = previous {
                    if let Err(e) = gs.register(p.main) {
                        tracing::error!("cannot restore previous hotkey: {e}");
                    }
                }
                Err(e.to_string())
            }
        }
    }

    fn suspend_hotkey(&self, suspended: bool) {
        let state = self.app.state::<Hotkeys>();
        let Some(pair) = state.0.lock().ok().and_then(|g| *g) else {
            return;
        };
        let gs = self.app.global_shortcut();
        let result = match (suspended, gs.is_registered(pair.main)) {
            (true, true) => gs.unregister(pair.main),
            (false, false) => gs.register(pair.main),
            _ => Ok(()),
        };
        if let Err(e) = result {
            tracing::warn!("suspend hotkey: {e}");
        }
    }

    fn refresh(&self, view: &View<'_>) {
        let Ok(mut c) = self.current.lock() else {
            return;
        };
        let lang_changed = c.lang != view.lang;
        c.lang = view.lang;
        c.hotkey = view.settings.hotkey.clone();
        c.overlay_enabled = view.settings.overlay_enabled;
        c.overlay_positions = view.settings.overlay_positions.clone();
        match build_menu(&self.app, view) {
            Ok((menu, items)) => {
                if let Some(tray) = self.app.tray_by_id(TRAY) {
                    let _ = tray.set_menu(Some(menu));
                }
                c.tray = Some(items);
            }
            Err(e) => tracing::error!("tray menu: {e}"),
        }
        self.update_tray(&c);
        if c.autostart != Some(view.settings.autostart) {
            c.autostart = Some(view.settings.autostart);
            self.apply_autostart(view.settings.autostart);
        }
        if !view.settings.overlay_enabled {
            if let Some(w) = self.overlay() {
                let _ = w.hide();
            }
        }
        drop(c);
        if lang_changed {
            let _ = self.app.emit("lang", view.lang);
        }
    }

    fn changed(&self, what: Changed) {
        let event = match what {
            Changed::Settings => "settings-changed",
            Changed::Models => "models-changed",
            Changed::History => "history-changed",
        };
        let _ = self.app.emit_to(MAIN, event, ());
    }
}

fn build_menu(app: &AppHandle, view: &View<'_>) -> tauri::Result<(Menu<Wry>, TrayItems)> {
    let s = i18n::strings(view.lang);
    let status = MenuItem::with_id(app, "status", s.starting, false, None::<&str>)?;
    let toggle = MenuItem::with_id(app, "toggle", s.start, false, None::<&str>)?;
    let retry = MenuItem::with_id(app, "retry", s.retry, false, None::<&str>)?;
    let models = view
        .installed
        .iter()
        .map(|m| {
            CheckMenuItem::with_id(
                app,
                format!("model:{}", m.id),
                &m.display_name,
                true,
                view.active_model == Some(m.id.as_str()),
                None::<&str>,
            )
        })
        .collect::<tauri::Result<Vec<_>>>()?;
    let model_refs: Vec<&dyn IsMenuItem<Wry>> =
        models.iter().map(|i| i as &dyn IsMenuItem<Wry>).collect();
    let model_menu = Submenu::with_items(app, s.model, !models.is_empty(), &model_refs)?;
    let item = |id: &str, text: &str| MenuItem::with_id(app, id, text, true, None::<&str>);
    let menu = Menu::with_items(
        app,
        &[
            &status,
            &PredefinedMenuItem::separator(app)?,
            &toggle,
            &retry,
            &model_menu,
            &PredefinedMenuItem::separator(app)?,
            &item("history", s.history)?,
            &item("settings", s.settings)?,
            &item("logs_dir", s.open_logs)?,
            &item("check_updates", s.check_updates)?,
            &PredefinedMenuItem::separator(app)?,
            &item("quit", s.quit)?,
        ],
    )?;
    Ok((
        menu,
        TrayItems {
            status,
            toggle,
            retry,
        },
    ))
}

/// Creates the tray icon; its menu is set by the first [`Shell::refresh`].
pub fn build_tray(
    app: &AppHandle,
    on_menu: impl Fn(&AppHandle, &str) + Send + Sync + 'static,
) -> tauri::Result<()> {
    TrayIconBuilder::with_id(TRAY)
        .icon(Image::from_bytes(ICON_LOADING)?)
        .tooltip("Vocalibre")
        .show_menu_on_left_click(true)
        .on_menu_event(move |app, event| on_menu(app, event.id().as_ref()))
        .build(app)?;
    Ok(())
}

/// Hidden, non-focusable pill: showing or clicking it never steals focus
/// from the app receiving the text.
pub fn build_overlay(app: &AppHandle) -> tauri::Result<WebviewWindow> {
    WebviewWindowBuilder::new(app, OVERLAY, WebviewUrl::App("index.html".into()))
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
        .visible(false)
        .build()
}

/// Shows the main window on `page`, creating it if needed.
pub fn open_main(app: &AppHandle, page: &str) {
    if let Some(w) = app.get_webview_window(MAIN) {
        let _ = app.emit_to(MAIN, "navigate", page);
        let _ = w.unminimize();
        let _ = w.show();
        let _ = w.set_focus();
        return;
    }
    let url = WebviewUrl::App(format!("index.html#/{page}").into());
    let built = WebviewWindowBuilder::new(app, MAIN, url)
        .title("Vocalibre")
        .inner_size(900.0, 640.0)
        .min_inner_size(760.0, 520.0)
        .center()
        .build();
    if let Err(e) = built {
        tracing::error!("cannot open main window: {e}");
    }
}

fn monitor_key(m: &Monitor) -> String {
    m.name().cloned().unwrap_or_else(|| "default".into())
}

/// Places the overlay on the screen under the mouse: at the position saved
/// for that screen if it still fits, otherwise bottom centre.
fn place_overlay(
    app: &AppHandle,
    w: &WebviewWindow,
    positions: &std::collections::HashMap<String, OverlayPosition>,
) {
    let monitor = app
        .cursor_position()
        .ok()
        .and_then(|p| app.monitor_from_point(p.x, p.y).ok().flatten())
        .or_else(|| app.primary_monitor().ok().flatten());
    let Some(m) = monitor else { return };
    let scale = m.scale_factor();
    let area = m.work_area();
    let (aw, ah) = (
        f64::from(area.size.width) / scale,
        f64::from(area.size.height) / scale,
    );
    let (x, y) = match positions.get(&monitor_key(&m)) {
        Some(p) if p.x >= 0.0 && p.y >= 0.0 && p.x + OVERLAY_W <= aw && p.y + OVERLAY_H <= ah => {
            (p.x, p.y)
        }
        _ => ((aw - OVERLAY_W) / 2.0, ah - OVERLAY_H - OVERLAY_MARGIN),
    };
    let pos = PhysicalPosition::new(
        area.position.x + (x * scale) as i32,
        area.position.y + (y * scale) as i32,
    );
    if let Err(e) = w.set_position(pos) {
        tracing::warn!("cannot place overlay: {e}");
    }
}

/// The overlay's position relative to the work area of the screen it is on,
/// after the user dragged it.
pub fn overlay_placement(app: &AppHandle) -> Option<(String, OverlayPosition)> {
    let w = app.get_webview_window(OVERLAY)?;
    let pos = w.outer_position().ok()?;
    let m = w.current_monitor().ok().flatten()?;
    let scale = m.scale_factor();
    let area = m.work_area();
    Some((
        monitor_key(&m),
        OverlayPosition {
            x: f64::from(pos.x - area.position.x) / scale,
            y: f64::from(pos.y - area.position.y) / scale,
        },
    ))
}
