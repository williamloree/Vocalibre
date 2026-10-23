/// Every app command, so each gets an `allow-<name>` permission that the
/// capabilities grant per window. Keep in sync with `generate_handler!`.
const COMMANDS: &[&str] = &[
    "get_status",
    "get_lang",
    "get_settings",
    "update_settings",
    "reset_settings",
    "suspend_hotkey",
    "list_models",
    "select_model",
    "delete_model",
    "download_model",
    "cancel_download",
    "benchmark",
    "list_microphones",
    "get_history",
    "clear_history",
    "copy_text",
    "open_logs_dir",
    "open_models_dir",
    "overlay_moved",
    "app_info",
];

fn main() {
    let attributes = tauri_build::Attributes::new()
        .app_manifest(tauri_build::AppManifest::new().commands(COMMANDS));
    if let Err(e) = tauri_build::try_build(attributes) {
        panic!("tauri build failed: {e:#}");
    }
}
