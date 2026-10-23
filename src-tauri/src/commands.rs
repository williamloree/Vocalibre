//! Commands called by the windows. Each one is a message to the actor; the
//! permission to call it is granted per window in `capabilities/`. A new
//! command must also be listed in `build.rs`.

use tauri::{AppHandle, Manager, State};
use tauri_plugin_opener::OpenerExt;
use tokio::sync::oneshot;

use crate::app::actor::{ActorTx, BenchResult, ModelEntry, Msg, Status};
use crate::audio::{self, MicInfo};
use crate::history::Entry;
use crate::i18n::{self, Lang};
use crate::settings::Settings;
use crate::shell;

const GONE: &str = "the application is shutting down";

async fn ask<T>(tx: &ActorTx, msg: impl FnOnce(oneshot::Sender<T>) -> Msg) -> Result<T, String> {
    let (reply, rx) = oneshot::channel();
    tx.send(msg(reply)).map_err(|_| GONE.to_string())?;
    rx.await.map_err(|_| GONE.to_string())
}

fn tell(tx: &ActorTx, msg: Msg) -> Result<(), String> {
    tx.send(msg).map_err(|_| GONE.to_string())
}

#[tauri::command]
pub async fn get_status(tx: State<'_, ActorTx>) -> Result<Status, String> {
    ask(&tx, Msg::GetStatus).await
}

#[tauri::command]
pub async fn get_lang(tx: State<'_, ActorTx>) -> Result<Lang, String> {
    let s = ask(&tx, Msg::GetSettings).await?;
    Ok(i18n::resolve(s.ui_language))
}

#[tauri::command]
pub async fn get_settings(tx: State<'_, ActorTx>) -> Result<Settings, String> {
    ask(&tx, Msg::GetSettings).await
}

#[tauri::command]
pub async fn update_settings(
    tx: State<'_, ActorTx>,
    settings: Settings,
) -> Result<Settings, String> {
    ask(&tx, |r| Msg::UpdateSettings(Box::new(settings), r)).await?
}

#[tauri::command]
pub async fn reset_settings(tx: State<'_, ActorTx>) -> Result<Settings, String> {
    ask(&tx, Msg::ResetSettings).await?
}

#[tauri::command]
pub fn suspend_hotkey(tx: State<'_, ActorTx>, suspended: bool) -> Result<(), String> {
    tell(&tx, Msg::SuspendHotkey(suspended))
}

#[tauri::command]
pub async fn list_models(tx: State<'_, ActorTx>) -> Result<Vec<ModelEntry>, String> {
    ask(&tx, Msg::GetModels).await
}

#[tauri::command]
pub fn select_model(tx: State<'_, ActorTx>, id: String) -> Result<(), String> {
    tell(&tx, Msg::SelectModel(id))
}

#[tauri::command]
pub async fn delete_model(tx: State<'_, ActorTx>, id: String) -> Result<(), String> {
    ask(&tx, |r| Msg::DeleteModel(id, r)).await?
}

#[tauri::command]
pub async fn download_model(tx: State<'_, ActorTx>, id: String) -> Result<(), String> {
    ask(&tx, |r| Msg::Download(id, r)).await?
}

#[tauri::command]
pub fn cancel_download(tx: State<'_, ActorTx>) -> Result<(), String> {
    tell(&tx, Msg::CancelDownload)
}

#[tauri::command]
pub async fn benchmark(tx: State<'_, ActorTx>) -> Result<BenchResult, String> {
    ask(&tx, Msg::Benchmark).await?
}

#[tauri::command]
pub async fn list_microphones() -> Result<Vec<MicInfo>, String> {
    // Device enumeration can take a moment (WASAPI); keep it off the runtime.
    tokio::task::spawn_blocking(audio::list_inputs)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn get_history(tx: State<'_, ActorTx>) -> Result<Vec<Entry>, String> {
    ask(&tx, Msg::GetHistory).await
}

#[tauri::command]
pub fn clear_history(tx: State<'_, ActorTx>) -> Result<(), String> {
    tell(&tx, Msg::ClearHistory)
}

#[tauri::command]
pub async fn copy_text(tx: State<'_, ActorTx>, text: String) -> Result<(), String> {
    ask(&tx, |r| Msg::CopyText(text, r)).await?
}

#[tauri::command]
pub fn open_logs_dir(app: AppHandle) -> Result<(), String> {
    let dir = app.path().app_log_dir().map_err(|e| e.to_string())?;
    open(&app, &dir)
}

#[tauri::command]
pub async fn open_models_dir(app: AppHandle, tx: State<'_, ActorTx>) -> Result<(), String> {
    let status = ask(&tx, Msg::GetStatus).await?;
    open(&app, &status.models_dir)
}

#[tauri::command]
pub fn overlay_moved(app: AppHandle, tx: State<'_, ActorTx>) -> Result<(), String> {
    match shell::overlay_placement(&app) {
        Some((monitor, position)) => tell(&tx, Msg::OverlayMoved { monitor, position }),
        None => Ok(()),
    }
}

#[derive(serde::Serialize)]
pub struct AppInfo {
    version: &'static str,
    config_dir: String,
    log_dir: String,
}

#[tauri::command]
pub fn app_info(app: AppHandle) -> Result<AppInfo, String> {
    let path = app.path();
    let s = |p: tauri::Result<std::path::PathBuf>| {
        p.map(|p| p.display().to_string())
            .map_err(|e| e.to_string())
    };
    Ok(AppInfo {
        version: env!("CARGO_PKG_VERSION"),
        config_dir: s(path.app_config_dir())?,
        log_dir: s(path.app_log_dir())?,
    })
}

fn open(app: &AppHandle, dir: &std::path::Path) -> Result<(), String> {
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    app.opener()
        .open_path(dir.to_string_lossy(), None::<&str>)
        .map_err(|e| e.to_string())
}
