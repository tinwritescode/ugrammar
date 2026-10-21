use crate::app::ctx;
use crate::ipc::{AppInfo, Choice, PopoverPayload, Status, StatusKind, TrayState};
use serde::Deserialize;
use serde_json::{json, Value};
use std::time::Duration;
use tauri::Manager;
use tauri_plugin_dialog::DialogExt;
use tokio::sync::oneshot;
use ugrammar_core::engine::{ConnectionStatus, OpenAICompatibleEngine, OpenAIConfig};

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
    let before = c.settings().hotkey;
    if c.update_settings(patch).hotkey != before {
        c.register_hotkey().await;
    }
    c.refresh_tray();
}

#[tauri::command]
pub async fn set_api_key(key: String) {
    let c = ctx();
    c.set_api_key(&key).await;
    c.refresh_tray();
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Draft {
    endpoint: String,
    model: String,
    api_key: Option<String>,
}

#[tauri::command]
pub async fn test_connection(draft: Draft) -> ConnectionStatus {
    let c = ctx();
    let s = c.settings();
    let key = match draft.api_key.filter(|k| !k.is_empty()) {
        Some(k) => Some(k),
        None => c.api_key().await,
    };
    let or = |v: &str, fallback: String| if v.trim().is_empty() { fallback } else { v.trim().to_string() };
    let cfg = OpenAIConfig { endpoint: or(&draft.endpoint, s.endpoint), model: or(&draft.model, s.model), timeout_ms: s.timeout_ms };
    let r = OpenAICompatibleEngine::new(cfg, key).test_connection().await;
    if r.ok {
        c.set_status(Status::of(StatusKind::Idle));
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
    let r = ctx().helper.request("app_info", json!({ "ids": ids }), Duration::from_secs(10)).await;
    serde_json::from_value(r["apps"].clone()).unwrap_or_default()
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
