use crate::app::ctx;
use crate::ipc::{AppInfo, Choice, PermissionStatus, PopoverPayload, TrayState};
use crate::reactions::resting;
use serde::Deserialize;
use serde_json::Value;
use tauri::Manager;
use tauri_plugin_autostart::ManagerExt;
use tauri_plugin_dialog::DialogExt;
use tokio::sync::oneshot;
use ugrammar_core::engine::{ConnectionStatus, EngineSetup, Purpose};
use ugrammar_core::i18n::fill;
use ugrammar_core::settings::with_patch;

// Commands that touch windows are async so they run off the main thread.

#[tauri::command]
pub async fn popover_action(action: Choice) {
    ctx().close_popover(action);
}

/// Áp dụng / Bỏ qua on a fix in the tray's waiting list.
#[tauri::command]
pub async fn pending_action(id: u64, action: Choice) {
    ctx().pending_action(id, action).await;
}

/// The diff's fixes the user turned off; Accept writes only the rest.
#[tauri::command]
pub async fn popover_skip(fixes: Vec<usize>) {
    ctx().popover_skip(fixes);
}

#[tauri::command]
pub async fn add_word(word: String) {
    ctx().add_word(&word);
}

#[tauri::command]
pub async fn popover_resize(height: f64, width: Option<f64>) {
    ctx().popover_resize(height, width);
}

/// The popover's listener can register after the first show; it asks for what it missed.
#[tauri::command]
pub async fn popover_current() -> Option<PopoverPayload> {
    ctx().popover_payload()
}

#[tauri::command]
pub async fn check_update_now() {
    tauri::async_runtime::spawn(ctx().check_update_now());
}

#[tauri::command]
pub async fn clear_insights() {
    let c = ctx();
    c.clear_insights();
    c.refresh_tray();
}

#[tauri::command]
pub async fn save_settings(patch: Value) {
    let c = ctx();
    c.update_settings(patch);
    c.refresh_tray();
}

#[tauri::command]
pub async fn set_api_key(key: String) {
    let c = ctx();
    if let Err(e) = c.set_api_key(&key).await {
        c.notify(&fill(c.settings().strings().key_save_failed, &[("error", &e)]));
    }
    c.refresh_tray();
}

/// Engine fields typed in the setup view, not saved yet.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Draft {
    /// Settings keys (camelCase); a blank value keeps the saved one.
    #[serde(default)]
    settings: serde_json::Map<String, Value>,
    api_key: Option<String>,
}

#[tauri::command]
pub async fn test_connection(draft: Draft) -> ConnectionStatus {
    let c = ctx();
    let key = match draft.api_key.filter(|k| !k.is_empty()) {
        Some(k) => Some(k),
        None => c.api_key().await,
    };
    let typed = draft.settings.into_iter().filter(|(_, v)| v.as_str().is_none_or(|s| !s.trim().is_empty())).collect();
    let s = with_patch(&c.settings(), Value::Object(typed));
    let r = EngineSetup::from_settings(&s).engine(s.timeout_ms, Purpose::Hotkey, key).test_connection().await;
    if r.ok {
        c.set_status(resting(&c.settings()));
    }
    r
}

#[tauri::command]
pub async fn pick_app() -> Option<String> {
    let c = ctx();
    c.pin_tray(true);
    let (tx, rx) = oneshot::channel();
    c.app.dialog().file().set_directory("/Applications").add_filter("Applications", &["app"]).pick_file(move |f| {
        let _ = tx.send(f.and_then(|f| f.into_path().ok()));
    });
    let path = rx.await.ok().flatten();
    c.pin_tray(false);
    if let Some(w) = c.app.get_webview_window("tray") {
        let _ = w.set_focus();
    }
    let out = tokio::process::Command::new("/usr/bin/mdls").args(["-raw", "-name", "kMDItemCFBundleIdentifier"]).arg(path?).output().await.ok()?;
    let id = String::from_utf8_lossy(&out.stdout).trim().to_string();
    (out.status.success() && !id.is_empty() && id != "(null)").then_some(id)
}

#[tauri::command]
pub async fn app_info(ids: Vec<String>) -> Vec<AppInfo> {
    ctx().helper.app_info(&ids).await
}

#[tauri::command]
pub async fn get_tray() -> TrayState {
    ctx().tray_state().await
}

#[tauri::command]
pub async fn tray_action(action: String) {
    ctx().tray_action(&action).await;
}

#[tauri::command]
pub async fn tray_resize(height: f64) {
    ctx().tray_resize(height);
}

/// Adds or removes the login item. A failed toggle leaves it as it was, and the refreshed state shows that.
#[tauri::command]
pub async fn set_launch_at_login(enabled: bool) {
    let c = ctx();
    let launch = c.app.autolaunch();
    let _ = if enabled { launch.enable() } else { launch.disable() };
    c.refresh_tray();
}

/// Re-probes every permission; the permissions screen polls this while it's open.
#[tauri::command]
pub async fn check_permissions() -> Vec<PermissionStatus> {
    ctx().check_permissions().await
}

/// Keeps the panel up while the user works in System Settings; the permissions screen sets it while a grant is pending.
#[tauri::command]
pub async fn pin_tray(pinned: bool) {
    ctx().pin_tray(pinned);
}

/// Starts a native drag of this app's bundle, to drop on the Accessibility list in System Settings.
#[tauri::command]
pub fn drag_app(window: tauri::WebviewWindow) {
    let w = window.clone();
    let _ = window.run_on_main_thread(move || {
        let icon = drag::Image::Raw(include_bytes!("../../src/renderer/app-icon.png").to_vec());
        let _ = drag::start_drag(&w, drag::DragItem::Files(vec![crate::app::app_bundle()]), icon, |_, _| {}, drag::Options::default());
    });
}
