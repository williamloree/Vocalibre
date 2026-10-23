pub mod app;
pub mod audio;
pub mod injection;
pub mod logging;
pub mod models;
pub mod settings;
pub mod shell;
pub mod sound;
pub mod stt;

use std::path::PathBuf;

use tauri::{AppHandle, Manager};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutEvent, ShortcutState};
use tauri_plugin_opener::OpenerExt;

use app::actor::{Actor, ActorTx, Msg, Paths};
use app::state::Input;
use models::manifest::Manifest;
use settings::{LoadOutcome, Settings};
use shell::{Hotkeys, TauriShell};

/// Folders the tray can open.
struct Dirs {
    models: PathBuf,
    logs: PathBuf,
}

/// Keeps the log file writer alive until exit.
struct LogGuard {
    _guard: Option<tracing_appender::non_blocking::WorkerGuard>,
}

pub fn run() {
    let app = tauri::Builder::default()
        // Must be first: a second launch exits before touching anything.
        .plugin(tauri_plugin_single_instance::init(|_, _, _| {
            tracing::info!("already running");
        }))
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(on_shortcut)
                .build(),
        )
        .plugin(tauri_plugin_opener::init())
        .setup(|app| setup(app.handle()))
        .build(tauri::generate_context!());
    match app {
        // Only the tray quits the app; it lives on without windows.
        Ok(app) => app.run(|_, event| {
            if let tauri::RunEvent::ExitRequested {
                api, code: None, ..
            } = event
            {
                api.prevent_exit();
            }
        }),
        Err(e) => {
            eprintln!("cannot start Vocalibre: {e}");
            std::process::exit(1);
        }
    }
}

fn setup(app: &AppHandle) -> Result<(), Box<dyn std::error::Error>> {
    let path = app.path();
    let config_dir = path.app_config_dir()?;
    let logs = path.app_log_dir()?;
    std::fs::create_dir_all(&config_dir)?;
    std::fs::create_dir_all(&logs)?;
    app.manage(LogGuard {
        _guard: logging::init(&logs),
    });
    tracing::info!(version = env!("CARGO_PKG_VERSION"), "Vocalibre starting");

    let settings = load_settings(&config_dir);
    let manifest = Manifest::embedded()?;
    let models_dir = match &settings.models_dir {
        Some(d) => d.clone(),
        None => path.app_data_dir()?.join("models"),
    };
    std::fs::create_dir_all(&models_dir)?;
    tracing::info!(dir = %models_dir.display(), "models directory");

    let hotkeys = Hotkeys {
        main: parse_shortcut(&settings.hotkey, &Settings::default().hotkey)?,
        cancel: parse_shortcut(&settings.cancel_key, &Settings::default().cancel_key)?,
    };
    let hotkey_error = app
        .global_shortcut()
        .register(hotkeys.main)
        .err()
        .map(|e| e.to_string());
    if let Some(e) = &hotkey_error {
        tracing::error!(hotkey = %settings.hotkey, "cannot register hotkey: {e}");
    }
    app.manage(hotkeys);

    let installed = models::installed(&manifest, &models_dir);
    let tray = shell::build_tray(app, &installed, on_menu)?;
    shell::build_overlay(app)?;
    let hotkey_label = match hotkey_error {
        None => settings.hotkey.clone(),
        Some(_) => format!("raccourci {} indisponible", settings.hotkey),
    };
    let shell = TauriShell::new(app.clone(), tray, hotkey_label, settings.overlay_enabled);

    app.manage(Dirs {
        models: models_dir.clone(),
        logs,
    });
    let paths = Paths {
        config_dir,
        models_dir,
    };
    let (actor, tx) = Actor::new(settings, paths, manifest, shell)?;
    app.manage(tx);
    tauri::async_runtime::spawn(actor.run());
    Ok(())
}

fn load_settings(dir: &std::path::Path) -> Settings {
    let (mut s, outcome) = settings::load(dir);
    let reset = s.validate();
    if !reset.is_empty() {
        tracing::warn!(?reset, "invalid settings reset to defaults");
    }
    let save = match &outcome {
        LoadOutcome::Loaded => !reset.is_empty(),
        LoadOutcome::Created => true,
        LoadOutcome::Corrupt { backup } => {
            tracing::warn!(backup = %backup.display(), "settings were corrupt, defaults used");
            true
        }
    };
    if save {
        if let Err(e) = settings::save(dir, &s) {
            tracing::error!("cannot save settings: {e}");
        }
    }
    s
}

/// Falls back to the default when the configured shortcut does not parse.
fn parse_shortcut(value: &str, default: &str) -> Result<Shortcut, Box<dyn std::error::Error>> {
    match value.parse() {
        Ok(s) => Ok(s),
        Err(e) => {
            tracing::warn!("invalid shortcut {value:?} ({e}), using {default}");
            Ok(default.parse()?)
        }
    }
}

fn send(app: &AppHandle, msg: Msg) {
    if let Some(tx) = app.try_state::<ActorTx>() {
        let _ = tx.send(msg);
    }
}

fn on_shortcut(app: &AppHandle, shortcut: &Shortcut, event: ShortcutEvent) {
    let Some(keys) = app.try_state::<Hotkeys>() else {
        return;
    };
    let pressed = event.state == ShortcutState::Pressed;
    let input = if *shortcut == keys.main {
        if pressed {
            Input::HotkeyPressed
        } else {
            Input::HotkeyReleased
        }
    } else if *shortcut == keys.cancel && pressed {
        Input::Cancel
    } else {
        return;
    };
    send(app, Msg::Input(input));
}

fn on_menu(app: &AppHandle, id: &str) {
    match id {
        "toggle" => send(app, Msg::Input(Input::ToggleRequested)),
        "retry" => send(app, Msg::Input(Input::Retry)),
        "models_dir" | "logs_dir" => {
            let Some(dirs) = app.try_state::<Dirs>() else {
                return;
            };
            let dir = if id == "models_dir" {
                &dirs.models
            } else {
                &dirs.logs
            };
            if let Err(e) = app.opener().open_path(dir.to_string_lossy(), None::<&str>) {
                tracing::warn!("cannot open {}: {e}", dir.display());
            }
        }
        "quit" => app.exit(0),
        other => {
            if let Some(model_id) = other.strip_prefix("model:") {
                send(app, Msg::SelectModel(model_id.to_string()));
            }
        }
    }
}
