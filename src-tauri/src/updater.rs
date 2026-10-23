//! Updates from the GitHub releases: the app fetches the signed
//! `latest.json`, offers the new version, then installs it and restarts.
//! The update is verified against the public key in `tauri.conf.json`.

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use tauri::{AppHandle, Manager};
use tauri_plugin_dialog::{DialogExt, MessageDialogButtons, MessageDialogKind};
use tauri_plugin_updater::UpdaterExt;
use tokio::sync::oneshot;

use crate::app::actor::{ActorTx, Msg};
use crate::app::state::AppState;
use crate::i18n::{self, Strings};

const FIRST_CHECK: Duration = Duration::from_secs(30);
const CHECK_EVERY: Duration = Duration::from_secs(24 * 60 * 60);
const BUSY_RETRY: Duration = Duration::from_secs(5);

/// One check at a time, so two dialogs never stack up.
static CHECKING: AtomicBool = AtomicBool::new(false);

/// Checks shortly after start, then once a day, while the setting allows it.
/// Development builds only check on demand: they must not offer to replace
/// themselves with a release.
pub fn spawn_periodic(app: AppHandle) {
    if cfg!(debug_assertions) {
        return;
    }
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(FIRST_CHECK).await;
        loop {
            check(&app, false).await;
            tokio::time::sleep(CHECK_EVERY).await;
        }
    });
}

/// Tray « Check for updates »: also reports « up to date » and errors.
pub fn check_now(app: &AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move { check(&app, true).await });
}

async fn check(app: &AppHandle, manual: bool) {
    if CHECKING.swap(true, Ordering::SeqCst) {
        return;
    }
    run(app, manual).await;
    CHECKING.store(false, Ordering::SeqCst);
}

async fn run(app: &AppHandle, manual: bool) {
    let Some(settings) = ask(app, Msg::GetSettings).await else {
        return;
    };
    if !manual && !settings.check_updates {
        return;
    }
    let s = i18n::strings(i18n::resolve(settings.ui_language));
    let found = match app.updater() {
        Ok(updater) => updater.check().await,
        Err(e) => Err(e),
    };
    let update = match found {
        Ok(Some(update)) => update,
        Ok(None) => {
            tracing::info!("no update available");
            if manual {
                let text = s.up_to_date.replace("{version}", env!("CARGO_PKG_VERSION"));
                message(app, s, &text, MessageDialogKind::Info).await;
            }
            return;
        }
        Err(e) => {
            tracing::warn!("update check failed: {e}");
            if manual {
                let text = s.update_failed.replace("{error}", &e.to_string());
                message(app, s, &text, MessageDialogKind::Error).await;
            }
            return;
        }
    };
    tracing::info!(version = %update.version, "update available");

    let text = s
        .update_available
        .replace("{version}", &update.version)
        .replace("{current}", &update.current_version);
    if !confirm(app, s, &text).await {
        return;
    }
    // The installer closes the app: never cut a dictation short.
    wait_until_idle(app).await;
    tracing::info!(version = %update.version, "installing update");
    match update.download_and_install(|_, _| {}, || {}).await {
        Ok(()) => app.restart(),
        Err(e) => {
            tracing::error!("update install failed: {e}");
            let text = s.update_failed.replace("{error}", &e.to_string());
            message(app, s, &text, MessageDialogKind::Error).await;
        }
    }
}

async fn ask<T>(app: &AppHandle, msg: impl FnOnce(oneshot::Sender<T>) -> Msg) -> Option<T> {
    let tx = app.try_state::<ActorTx>()?;
    let (reply, rx) = oneshot::channel();
    tx.send(msg(reply)).ok()?;
    rx.await.ok()
}

async fn wait_until_idle(app: &AppHandle) {
    while let Some(status) = ask(app, Msg::GetStatus).await {
        if !matches!(
            status.state,
            AppState::Recording { .. } | AppState::Transcribing { .. }
        ) {
            return;
        }
        tokio::time::sleep(BUSY_RETRY).await;
    }
}

async fn confirm(app: &AppHandle, s: &Strings, text: &str) -> bool {
    let (tx, rx) = oneshot::channel();
    app.dialog()
        .message(text)
        .title(s.update_title)
        .buttons(MessageDialogButtons::OkCancelCustom(
            s.update_install.to_string(),
            s.update_later.to_string(),
        ))
        .show(move |yes| {
            let _ = tx.send(yes);
        });
    rx.await.unwrap_or(false)
}

async fn message(app: &AppHandle, s: &Strings, text: &str, kind: MessageDialogKind) {
    let (tx, rx) = oneshot::channel();
    app.dialog()
        .message(text)
        .title(s.update_title)
        .kind(kind)
        .show(move |_| {
            let _ = tx.send(());
        });
    let _ = rx.await;
}
