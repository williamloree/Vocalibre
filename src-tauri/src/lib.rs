pub mod app;
pub mod audio;
pub mod commands;
pub mod fsutil;
pub mod hardware;
pub mod history;
pub mod i18n;
pub mod injection;
pub mod logging;
pub mod models;
pub mod settings;
pub mod shell;
pub mod sound;
pub mod stt;
pub mod updater;

use tauri::{AppHandle, Manager};
use tauri_plugin_autostart::MacosLauncher;
use tauri_plugin_global_shortcut::{Shortcut, ShortcutEvent, ShortcutState};

use app::actor::{Actor, ActorTx, Msg, Paths};
use app::state::{Input, RecordingMode};
use models::manifest::Manifest;
use settings::{LoadOutcome, Settings};
use shell::{Hotkeys, TauriShell};

/// Keeps the log file writer alive until exit.
struct LogGuard {
    _guard: Option<tracing_appender::non_blocking::WorkerGuard>,
}

pub fn run() {
    let app = tauri::Builder::default()
        // Must be first: a second launch exits before touching anything,
        // and opens the settings of the running instance instead.
        .plugin(tauri_plugin_single_instance::init(|app, _, _| {
            shell::open_main(app, "general");
        }))
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(on_shortcut)
                .build(),
        )
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_autostart::init(
            MacosLauncher::LaunchAgent,
            None,
        ))
        .manage(Hotkeys::default())
        .invoke_handler(tauri::generate_handler![
            commands::get_status,
            commands::get_lang,
            commands::get_settings,
            commands::update_settings,
            commands::reset_settings,
            commands::suspend_hotkey,
            commands::list_models,
            commands::select_model,
            commands::delete_model,
            commands::download_model,
            commands::cancel_download,
            commands::benchmark,
            commands::hardware,
            commands::finish_onboarding,
            commands::list_microphones,
            commands::get_history,
            commands::clear_history,
            commands::copy_text,
            commands::open_logs_dir,
            commands::open_models_dir,
            commands::overlay_moved,
            commands::app_info,
        ])
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
    let data_dir = path.app_data_dir()?;
    let logs = path.app_log_dir()?;
    for dir in [&config_dir, &data_dir, &logs] {
        std::fs::create_dir_all(dir)?;
    }
    app.manage(LogGuard {
        _guard: logging::init(&logs),
    });
    tracing::info!(version = env!("CARGO_PKG_VERSION"), "Vocalibre starting");

    let settings = load_settings(&config_dir);
    let manifest = Manifest::embedded()?;
    shell::build_tray(app, on_menu)?;
    shell::build_overlay(app)?;

    let paths = Paths {
        config_dir,
        data_dir,
    };
    let (actor, tx) = Actor::new(settings, paths, manifest, TauriShell::new(app.clone()))?;
    app.manage(tx);
    tauri::async_runtime::spawn(actor.run());
    updater::spawn_periodic(app.clone());
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

fn send(app: &AppHandle, msg: Msg) {
    if let Some(tx) = app.try_state::<ActorTx>() {
        let _ = tx.send(msg);
    }
}

fn on_shortcut(app: &AppHandle, shortcut: &Shortcut, event: ShortcutEvent) {
    let state = app.state::<Hotkeys>();
    let Some(keys) = state.0.lock().ok().map(|b| *b) else {
        return;
    };
    let is = |key: Option<Shortcut>| key == Some(*shortcut);
    let pressed = event.state == ShortcutState::Pressed;
    let msg = match pressed {
        true if is(keys.toggle) => Msg::Input(Input::HotkeyPressed(RecordingMode::Toggle)),
        true if is(keys.push_to_talk) => {
            Msg::Input(Input::HotkeyPressed(RecordingMode::PushToTalk))
        }
        false if is(keys.push_to_talk) => Msg::Input(Input::HotkeyReleased),
        true if is(keys.paste_last) => Msg::PasteLast,
        true if is(keys.cancel) => Msg::Input(Input::Cancel),
        _ => return,
    };
    send(app, msg);
}

fn on_menu(app: &AppHandle, id: &str) {
    match id {
        "toggle" => send(app, Msg::Input(Input::ToggleRequested)),
        "retry" => send(app, Msg::Input(Input::Retry)),
        "history" => shell::open_main(app, "history"),
        "copy_last" => send(app, Msg::CopyLast),
        "settings" => shell::open_main(app, "general"),
        "logs_dir" => {
            if let Err(e) = commands::open_logs_dir(app.clone()) {
                tracing::warn!("cannot open logs: {e}");
            }
        }
        "check_updates" => updater::check_now(app),
        "quit" => app.exit(0),
        other => {
            if let Some(model_id) = other.strip_prefix("model:") {
                send(app, Msg::SelectModel(model_id.to_string()));
            }
        }
    }
}
