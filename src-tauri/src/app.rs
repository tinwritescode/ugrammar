use crate::autocheck::{AutoChecker, Effect, Issue, Job, Mark, Rect, SentenceFix, Snapshot, POLL_MS};
use crate::flow::{FixFlow, FixOutcome, FlowDeps};
use crate::helper::{Helper, HelperEvent, Reply, Selection};
use crate::ipc::{Choice, PopoverKind, PopoverPayload, Status, StatusKind, TrayState};
use crate::ui;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime};
use tauri::menu::{CheckMenuItem, IsMenuItem, Menu, MenuItem, PredefinedMenuItem};
use tauri::{AppHandle, Emitter, Manager, Wry};
use tauri_plugin_global_shortcut::{Code, GlobalShortcutExt, Modifiers, Shortcut};
use tauri_plugin_notification::NotificationExt;
use tokio::sync::{mpsc, oneshot};
use ugrammar_core::badge::{badge_position, clamp_to_area, CaretRect, BADGE_SIZE};
use ugrammar_core::engine::{CheckRequest, OpenAICompatibleEngine, OpenAIConfig, Tone};
use ugrammar_core::insights::InsightStore;
use ugrammar_core::settings::{AutoCheckMode, Mod, Settings, SettingsStore};

pub const APP_NAME: &str = "ugrammar";
const REPO_URL: &str = "https://github.com/tinwritescode/ugrammar";
const KEY_SERVICE: &str = "Proofy";
const KEY_ACCOUNT: &str = "api-key";
pub const POPOVER_W: f64 = 460.0;
pub const TRAY_W: f64 = 344.0;
const TRAY_MAX_H: f64 = 600.0;
const KEEP_MS: u128 = 8000;
const SECOND: Duration = Duration::from_secs(1);

pub static CTX: OnceLock<Ctx> = OnceLock::new();

pub fn ctx() -> &'static Ctx {
    CTX.get().expect("app context is set in setup")
}

/// Opt-in diagnostics (PROOFY_DEBUG=1): outcomes only, never text content.
pub fn debug(msg: &str) {
    if std::env::var_os("PROOFY_DEBUG").is_none() {
        return;
    }
    let t = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).unwrap_or_default();
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open("/tmp/proofy-debug.log") {
        let _ = writeln!(f, "{}.{:03} {msg}", t.as_secs(), t.subsec_millis());
    }
}

/// `y` = top edge when placed below, `alt_y` = bottom edge when placed above.
#[derive(Debug, Clone, Copy)]
pub struct Anchor {
    x: f64,
    y: f64,
    alt_y: f64,
}

fn below_rect(r: Rect) -> Anchor {
    Anchor { x: r.x + r.w / 2.0, y: r.y + r.h + 6.0, alt_y: r.y - 6.0 }
}

#[derive(Default)]
struct Popover {
    resolve: Option<oneshot::Sender<Choice>>,
    anchor: Option<Anchor>,
    size: (f64, f64),
    /// Bumped by every close, so a stale message timer cannot close a newer popover.
    generation: u64,
    payload: Option<PopoverPayload>,
}

/// One pooled strip window per slot; slot i is the window `underline-i`.
#[derive(Default)]
struct Underlines {
    slots: Vec<Option<u64>>,
    rects: HashMap<u64, Rect>,
}

#[derive(Default)]
struct Badge {
    generation: u64,
    running: bool,
    checking: bool,
    visible: bool,
    last: String,
    pos: (f64, f64),
    bundle_id: String,
    last_move: Option<Instant>,
    seen: bool,
}

#[derive(Default)]
struct TrayPanel {
    hidden_at: Option<Instant>,
    /// A native dialog over the panel must not blur it closed.
    pinned: bool,
}

pub struct Ctx {
    pub app: AppHandle,
    pub helper: Helper,
    store: Mutex<SettingsStore>,
    insights: Mutex<InsightStore>,
    status: Mutex<Status>,
    popover: Mutex<Popover>,
    auto: Mutex<AutoChecker>,
    auto_generation: AtomicU64,
    auto_running: AtomicBool,
    underlines: Mutex<Underlines>,
    card_open: AtomicBool,
    loading_seq: AtomicU64,
    loading_shown: AtomicBool,
    flow: FixFlow,
    flow_anchor: Mutex<Option<Anchor>>,
    badge: Mutex<Badge>,
    tray: Mutex<TrayPanel>,
    resume_generation: AtomicU64,
    started: Instant,
}

fn copy_dir(from: &Path, to: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(to)?;
    for e in std::fs::read_dir(from)? {
        let e = e?;
        let dest = to.join(e.file_name());
        if e.file_type()?.is_dir() {
            copy_dir(&e.path(), &dest)?;
        } else {
            std::fs::copy(e.path(), dest)?;
        }
    }
    Ok(())
}

fn helper_path() -> PathBuf {
    let bundled = std::env::current_exe().ok().and_then(|e| Some(e.parent()?.join("grammar-helper")));
    match bundled {
        Some(p) if p.exists() => p,
        _ => PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../helper/.build/release/grammar-helper"),
    }
}

const STATUS_LABEL: fn(StatusKind) -> &'static str = |k| match k {
    StatusKind::Idle => "Ready",
    StatusKind::Checking => "Checking…",
    StatusKind::Paused => "Paused",
    StatusKind::Error => "Error",
    StatusKind::NeedsPermission => "Needs Accessibility permission",
};

const STATUS_TITLE: fn(StatusKind) -> &'static str = |k| match k {
    StatusKind::Idle => "ug",
    StatusKind::Checking => "ug…",
    StatusKind::Paused => "ug⏸",
    StatusKind::Error => "ug!",
    StatusKind::NeedsPermission => "ug?",
};

pub fn init(app: &AppHandle) -> mpsc::UnboundedReceiver<HelperEvent> {
    let data = app.path().data_dir().unwrap_or_else(|_| PathBuf::from("."));
    let dir = data.join(APP_NAME);
    // Renamed from Proofy: carry settings over once.
    let legacy = data.join("Proofy");
    if !dir.exists() && legacy.exists() {
        let _ = copy_dir(&legacy, &dir);
    }
    let (tx, rx) = mpsc::unbounded_channel();
    let ctx = Ctx {
        app: app.clone(),
        helper: Helper::start(helper_path(), tx),
        store: Mutex::new(SettingsStore::new(&dir)),
        insights: Mutex::new(InsightStore::new(&dir)),
        status: Mutex::new(Status::of(StatusKind::Idle)),
        popover: Mutex::default(),
        auto: Mutex::new(AutoChecker::new()),
        auto_generation: AtomicU64::new(0),
        auto_running: AtomicBool::new(false),
        underlines: Mutex::default(),
        card_open: AtomicBool::new(false),
        loading_seq: AtomicU64::new(0),
        loading_shown: AtomicBool::new(false),
        flow: FixFlow::default(),
        flow_anchor: Mutex::new(None),
        badge: Mutex::default(),
        tray: Mutex::default(),
        resume_generation: AtomicU64::new(0),
        started: Instant::now(),
    };
    let _ = CTX.set(ctx);
    rx
}

impl Ctx {
    pub fn settings(&self) -> Settings {
        self.store.lock().unwrap().get()
    }

    /// Every settings write goes through here so status, auto-check and the badge follow it.
    pub fn update_settings(&'static self, patch: Value) -> Settings {
        let cfg = {
            let mut s = self.store.lock().unwrap();
            s.update(patch).unwrap_or_else(|_| s.get())
        };
        let cur = self.status.lock().unwrap().clone();
        self.set_status(if matches!(cur.kind, StatusKind::Paused | StatusKind::Idle) { Status::of(StatusKind::Idle) } else { cur });
        if cfg.auto_check && !cfg.paused {
            self.auto_start();
        } else {
            self.auto_stop();
        }
        cfg
    }

    pub fn hotkey_label(&self) -> String {
        let h = self.settings().hotkey;
        let mods: String = h
            .mods
            .iter()
            .map(|m| match m {
                Mod::Cmd => "⌘",
                Mod::Alt => "⌥",
                Mod::Ctrl => "⌃",
                Mod::Shift => "⇧",
            })
            .collect();
        format!("{mods}{}", h.key)
    }

    pub async fn api_key(&self) -> Option<String> {
        let r = self.helper.request("keychain_get", json!({ "service": KEY_SERVICE, "account": KEY_ACCOUNT }), 10 * SECOND).await;
        if r["ok"] != true {
            return None;
        }
        r["value"].as_str().map(String::from)
    }

    pub async fn set_api_key(&self, key: &str) {
        let cmd = if key.is_empty() { "keychain_delete" } else { "keychain_set" };
        self.helper.request(cmd, json!({ "service": KEY_SERVICE, "account": KEY_ACCOUNT, "value": key }), 10 * SECOND).await;
    }

    async fn engine(&self, min_timeout_ms: u64) -> OpenAICompatibleEngine {
        let s = self.settings();
        let cfg = OpenAIConfig { endpoint: s.endpoint, model: s.model, timeout_ms: s.timeout_ms.max(min_timeout_ms) };
        OpenAICompatibleEngine::new(cfg, self.api_key().await)
    }

    pub fn notify(&self, body: &str) {
        let _ = self.app.notification().builder().title(APP_NAME).body(body).show();
    }

    pub fn open_ax_settings(&self) {
        let _ = std::process::Command::new("/usr/bin/open").arg("x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility").spawn();
    }

    pub async fn ax_trusted(&self) -> bool {
        self.helper.request("status", json!({}), 10 * SECOND).await["trusted"] == true
    }

    pub fn status(&self) -> Status {
        self.status.lock().unwrap().clone()
    }

    pub fn set_status(&'static self, s: Status) {
        let cfg = self.settings();
        let s = if s.kind == StatusKind::Idle && cfg.paused { Status::of(StatusKind::Paused) } else { s };
        *self.status.lock().unwrap() = s.clone();
        self.refresh_tray();
        if matches!(s.kind, StatusKind::Paused | StatusKind::NeedsPermission) || !cfg.show_badge {
            self.badge_stop();
        } else {
            self.badge_start();
            self.badge.lock().unwrap().checking = s.kind == StatusKind::Checking;
            self.badge_paint(false);
        }
    }

    // ---------- Tray ----------

    pub async fn tray_state(&self) -> TrayState {
        let has_api_key = self.api_key().await.is_some();
        let st = self.status();
        TrayState {
            status: st.kind,
            message: st.message,
            settings: self.settings(),
            hotkey: self.hotkey_label(),
            insights: self.insights.lock().unwrap().top(5),
            has_api_key,
        }
    }

    pub fn clear_insights(&self) {
        self.insights.lock().unwrap().clear();
    }

    /// Right-click fallback: the plain native menu.
    fn tray_menu(&self) -> tauri::Result<Menu<Wry>> {
        let (st, cfg, a) = (self.status(), self.settings(), &self.app);
        let message = st.message.map(|m| format!(" — {m}")).unwrap_or_default();
        let status = MenuItem::with_id(a, "status", format!("Status: {}{message}", STATUS_LABEL(st.kind)), false, None::<&str>)?;
        let hotkey = MenuItem::with_id(a, "hotkey", format!("Fix selection: {}", self.hotkey_label()), false, None::<&str>)?;
        let pause = CheckMenuItem::with_id(a, "pause", "Pause", true, cfg.paused, None::<&str>)?;
        let ax = MenuItem::with_id(a, "ax", "Open Accessibility Settings…", true, None::<&str>)?;
        let settings = MenuItem::with_id(a, "settings", "Settings…", true, None::<&str>)?;
        let quit = MenuItem::with_id(a, "quit", format!("Quit {APP_NAME}"), true, Some("Cmd+Q"))?;
        let (sep1, sep2) = (PredefinedMenuItem::separator(a)?, PredefinedMenuItem::separator(a)?);
        let mut items: Vec<&dyn IsMenuItem<Wry>> = vec![&status, &hotkey, &sep1, &pause];
        if st.kind == StatusKind::NeedsPermission {
            items.push(&ax);
        }
        items.extend([&settings as &dyn IsMenuItem<Wry>, &sep2, &quit]);
        Menu::with_items(a, &items)
    }

    pub fn refresh_tray(&'static self) {
        if let Some(t) = self.app.tray_by_id("main") {
            let kind = self.status().kind;
            let _ = t.set_title(Some(STATUS_TITLE(kind)));
            let _ = t.set_tooltip(Some(format!("{APP_NAME} — {}", STATUS_LABEL(kind))));
            if let Ok(m) = self.tray_menu() {
                let _ = t.set_menu(Some(m));
            }
        }
        if self.app.get_webview_window("tray").is_some_and(|w| w.is_visible().unwrap_or(false)) {
            tauri::async_runtime::spawn(async move {
                let st = self.tray_state().await;
                let _ = self.app.emit_to("tray", "tray:state", st);
            });
        }
    }

    pub fn pin_tray(&self, pinned: bool) {
        self.tray.lock().unwrap().pinned = pinned;
    }

    pub fn on_tray_blur(&self) {
        if !self.tray.lock().unwrap().pinned {
            self.hide_tray();
        }
    }

    pub fn hide_tray(&self) {
        let Some(w) = self.app.get_webview_window("tray") else { return };
        if !w.is_visible().unwrap_or(false) {
            return;
        }
        let _ = w.hide();
        self.tray.lock().unwrap().hidden_at = Some(Instant::now());
    }

    pub async fn toggle_tray(&'static self) {
        if self.app.get_webview_window("tray").is_some_and(|w| w.is_visible().unwrap_or(false)) {
            return self.hide_tray();
        }
        // This click is what blurred it closed.
        if self.tray.lock().unwrap().hidden_at.is_some_and(|t| t.elapsed() < Duration::from_millis(250)) {
            return;
        }
        self.open_panel().await;
    }

    pub async fn open_panel(&'static self) {
        let Some(w) = self.app.get_webview_window("tray") else { return };
        let _ = self.app.emit_to("tray", "tray:state", self.tray_state().await);
        if let Some(b) = ui::tray_rect(&self.app) {
            let area = ui::screen_near(&self.app, b.x, b.y).work;
            let x = (b.x + b.w / 2.0 - TRAY_W / 2.0).max(area.x + 8.0).min(area.x + area.w - TRAY_W - 8.0);
            ui::place(&self.app, "tray", x, area.y + 6.0);
        }
        let _ = w.show();
        let _ = w.set_focus();
    }

    pub fn tray_resize(&self, h: f64) {
        if h <= 0.0 {
            return;
        }
        let (x, y) = ui::tray_rect(&self.app).map_or((0.0, 0.0), |b| (b.x, b.y));
        let area = ui::screen_near(&self.app, x, y).work;
        // Taller content scrolls inside.
        ui::resize(&self.app, "tray", TRAY_W, h.ceil().min(TRAY_MAX_H).min(area.h - 12.0));
    }

    pub async fn tray_action(&'static self, a: &str) {
        self.hide_tray();
        match a {
            "quit" => self.app.exit(0),
            "ax" => {
                self.helper.request("prompt_ax", json!({}), 10 * SECOND).await;
                self.open_ax_settings();
            }
            "github" => {
                let _ = std::process::Command::new("/usr/bin/open").arg(REPO_URL).spawn();
            }
            tone => {
                let Ok(tone) = serde_json::from_value::<Tone>(json!(tone)) else { return };
                // Hand focus back to the app the user was writing in, then rewrite its selection.
                let _ = self.app.hide();
                tokio::time::sleep(Duration::from_millis(200)).await;
                debug(&format!("tone={tone:?} outcome={:?}", self.run_flow(false, Some(tone)).await));
            }
        }
    }

    // ---------- Popover ----------

    pub fn close_popover(&self, choice: Choice) {
        let resolve = {
            let mut p = self.popover.lock().unwrap();
            p.generation += 1;
            p.payload = None;
            p.resolve.take()
        };
        let gs = self.app.global_shortcut();
        let _ = gs.unregister(accept_shortcut());
        let _ = gs.unregister(dismiss_shortcut());
        ui::hide(&self.app, "popover");
        if let Some(r) = resolve {
            let _ = r.send(choice);
        }
    }

    pub fn popover_open(&self) -> bool {
        self.popover.lock().unwrap().resolve.is_some()
    }

    pub fn popover_payload(&self) -> Option<PopoverPayload> {
        self.popover.lock().unwrap().payload.clone()
    }

    fn cursor_anchor(&self) -> Anchor {
        let (x, y) = ui::cursor(&self.app);
        Anchor { x, y: y + 20.0, alt_y: y - 36.0 }
    }

    /// Below the anchor if it fits, else above it; always clamped inside the display. Re-run after each resize.
    fn place_popover(&self) {
        let (a, (w, h)) = {
            let p = self.popover.lock().unwrap();
            (p.anchor.unwrap_or_else(|| self.cursor_anchor()), p.size)
        };
        let area = ui::screen_near(&self.app, a.x, a.y).work;
        let x = (a.x - w / 2.0).max(area.x + 8.0).min(area.x + area.w - w - 8.0);
        let y = if a.y + h <= area.y + area.h - 8.0 { a.y } else { a.alt_y - h };
        ui::place(&self.app, "popover", x, y.max(area.y + 8.0).min(area.y + area.h - h - 8.0));
    }

    fn show_popover(&self, payload: PopoverPayload, anchor: Option<Anchor>) {
        {
            let mut p = self.popover.lock().unwrap();
            p.anchor = Some(anchor.unwrap_or_else(|| self.cursor_anchor()));
            // Measure at full width; a pill shrinks it back.
            p.size.0 = POPOVER_W;
            if p.size.1 == 0.0 {
                p.size.1 = 200.0;
            }
            p.payload = Some(payload.clone());
            ui::resize(&self.app, "popover", p.size.0, p.size.1);
        }
        let _ = self.app.emit_to("popover", "popover:show", payload);
        self.place_popover();
        // Keep the target app focused so write-back lands there.
        ui::show_panel(&self.app, "popover");
    }

    pub fn popover_resize(&self, h: f64, w: Option<f64>) {
        if h <= 0.0 {
            return;
        }
        let size = (w.map_or(POPOVER_W, |w| w.ceil().min(POPOVER_W)), h.ceil().min(600.0));
        self.popover.lock().unwrap().size = size;
        ui::resize(&self.app, "popover", size.0, size.1);
        self.place_popover();
    }

    pub async fn show_diff(&self, p: PopoverPayload, anchor: Option<Anchor>) -> Choice {
        self.close_popover(Choice::Dismiss);
        let (tx, rx) = oneshot::channel();
        self.popover.lock().unwrap().resolve = Some(tx);
        self.show_popover(p, anchor);
        let gs = self.app.global_shortcut();
        let _ = gs.register(accept_shortcut());
        let _ = gs.register(dismiss_shortcut());
        rx.await.unwrap_or(Choice::Dismiss)
    }

    pub fn show_message(&'static self, message: &str, anchor: Option<Anchor>) {
        self.close_popover(Choice::Dismiss);
        self.show_popover(PopoverPayload::text(PopoverKind::Message, message), anchor);
        let generation = self.popover.lock().unwrap().generation;
        tauri::async_runtime::spawn(async move {
            tokio::time::sleep(Duration::from_millis(2200)).await;
            if self.popover.lock().unwrap().generation == generation {
                self.close_popover(Choice::Dismiss);
            }
        });
    }

    fn show_loading(&self, anchor: Option<Anchor>) {
        self.close_popover(Choice::Dismiss);
        self.show_popover(PopoverPayload::text(PopoverKind::Loading, "Checking…"), anchor);
    }

    // ---------- Flow ----------

    /// Under the selection, else under the focused field; None leaves the popover at the mouse.
    async fn selection_anchor(&self) -> Option<Anchor> {
        let r = self.helper.request("caret", json!({ "excluded": self.settings().excluded_apps, "deep": true }), Duration::from_millis(1500)).await;
        let visible = r["ok"] == true && r["visible"] == true;
        debug(&format!("anchor {}", if !visible { "mouse" } else if r["precise"] == true { "selection" } else { "field" }));
        if !visible {
            return None;
        }
        let rect = Rect { x: r["x"].as_f64()?, y: r["y"].as_f64()?, w: r["w"].as_f64()?, h: r["h"].as_f64()? };
        Some(below_rect(rect))
    }

    /// Manual checks: popovers sit by the text being fixed.
    pub async fn run_flow(&'static self, select_all: bool, tone: Option<Tone>) -> FixOutcome {
        let anchor = self.selection_anchor().await;
        *self.flow_anchor.lock().unwrap() = anchor;
        let outcome = self.flow.run(self, select_all, tone).await;
        *self.flow_anchor.lock().unwrap() = None;
        outcome
    }

    fn flow_anchor(&self) -> Option<Anchor> {
        *self.flow_anchor.lock().unwrap()
    }

    pub async fn on_hotkey(&'static self) {
        debug("hotkey");
        // A still-open popover counts as dismissed.
        self.close_popover(Choice::Dismiss);
        tokio::task::yield_now().await;
        let outcome = self.run_flow(false, None).await;
        let st = self.status();
        debug(&format!("outcome={outcome:?} status={:?}{}", st.kind, st.message.map(|m| format!(" ({m})")).unwrap_or_default()));
    }

    pub async fn on_badge_click(&'static self) {
        debug("badge click");
        self.close_popover(Choice::Dismiss);
        tokio::task::yield_now().await;
        let outcome = self.run_flow(true, None).await;
        debug(&format!("outcome={outcome:?} status={:?}", self.status().kind));
    }

    pub async fn register_hotkey(&self) {
        let h = self.settings().hotkey;
        let r: Reply = self.helper.request_as("set_hotkey", json!({ "key": h.key, "mods": h.mods }), 10 * SECOND).await;
        if !r.ok {
            self.notify(&format!("Could not register hotkey ({}). It may be in use by another app.", r.error.unwrap_or_default()));
        }
        // Tone menu: fixed ⌥⌘T. Failure (key in use) is not fatal.
        self.helper.request("set_hotkey", json!({ "key": "T", "mods": ["alt", "cmd"], "slot": 2 }), 10 * SECOND).await;
    }

    /// Tone hotkey: pick Formal / Friendly / Short from a menu over the app being written in.
    pub fn on_hotkey2(&self) {
        self.close_popover(Choice::Dismiss);
        let a = &self.app;
        let menu = (|| {
            Menu::with_items(
                a,
                &[
                    &MenuItem::with_id(a, "tone_title", "Rewrite selection as…", false, None::<&str>)?,
                    &MenuItem::with_id(a, "tone_formal", "Formal", true, None::<&str>)?,
                    &MenuItem::with_id(a, "tone_friendly", "Friendly", true, None::<&str>)?,
                    &MenuItem::with_id(a, "tone_short", "Short", true, None::<&str>)?,
                ],
            )
        })();
        if let (Ok(m), Some(w)) = (menu, a.get_webview_window("popover")) {
            let _ = w.popup_menu(&m);
        }
    }

    pub fn on_badge_menu(&self) {
        let bundle = self.badge.lock().unwrap().bundle_id.clone();
        let a = &self.app;
        let menu = (|| {
            Menu::with_items(
                a,
                &[
                    &MenuItem::with_id(a, "badge_hide", "Ẩn ở app này", !bundle.is_empty(), None::<&str>)?,
                    &MenuItem::with_id(a, "badge_pause", "Tạm dừng 1 giờ", true, None::<&str>)?,
                    &PredefinedMenuItem::separator(a)?,
                    &MenuItem::with_id(a, "settings", "Settings…", true, None::<&str>)?,
                ],
            )
        })();
        if let (Ok(m), Some(w)) = (menu, a.get_webview_window("badge")) {
            let _ = w.popup_menu(&m);
        }
    }

    pub async fn on_menu(&'static self, id: &str) {
        match id {
            "pause" => {
                let paused = !self.settings().paused;
                self.update_settings(json!({ "paused": paused }));
            }
            "ax" => self.open_ax_settings(),
            "settings" => self.open_panel().await,
            "quit" => self.app.exit(0),
            "badge_hide" => {
                let bundle = self.badge.lock().unwrap().bundle_id.clone();
                let mut apps = self.settings().excluded_apps;
                if !bundle.is_empty() && !apps.contains(&bundle) {
                    apps.push(bundle);
                    self.update_settings(json!({ "excludedApps": apps }));
                }
            }
            "badge_pause" => {
                self.update_settings(json!({ "paused": true }));
                let generation = self.resume_generation.fetch_add(1, Ordering::SeqCst) + 1;
                tokio::time::sleep(Duration::from_secs(3600)).await;
                if self.resume_generation.load(Ordering::SeqCst) == generation {
                    self.update_settings(json!({ "paused": false }));
                }
            }
            "tone_formal" | "tone_friendly" | "tone_short" => {
                let tone = match id {
                    "tone_formal" => Tone::Formal,
                    "tone_friendly" => Tone::Friendly,
                    _ => Tone::Short,
                };
                debug(&format!("tone={tone:?} outcome={:?}", self.run_flow(false, Some(tone)).await));
            }
            _ => {}
        }
    }

    pub async fn helper_events(&'static self, mut rx: mpsc::UnboundedReceiver<HelperEvent>) {
        while let Some(e) = rx.recv().await {
            match e {
                HelperEvent::Hotkey => {
                    tauri::async_runtime::spawn(self.on_hotkey());
                }
                HelperEvent::Hotkey2 => {
                    let _ = self.app.run_on_main_thread(move || self.on_hotkey2());
                }
                HelperEvent::Restarted => self.register_hotkey().await,
            }
        }
    }

    // ---------- Auto-check (underline in place + small card) ----------

    pub fn auto_start(&'static self) {
        if self.auto_running.swap(true, Ordering::SeqCst) {
            return;
        }
        let generation = self.auto_generation.fetch_add(1, Ordering::SeqCst) + 1;
        tauri::async_runtime::spawn(async move {
            let mut every = tokio::time::interval(Duration::from_millis(POLL_MS));
            every.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            loop {
                every.tick().await;
                if self.auto_generation.load(Ordering::SeqCst) != generation {
                    break;
                }
                self.auto_tick().await;
            }
        });
    }

    pub fn auto_stop(&'static self) {
        if !self.auto_running.swap(false, Ordering::SeqCst) {
            return;
        }
        self.auto_generation.fetch_add(1, Ordering::SeqCst);
        let fx = self.auto.lock().unwrap().reset();
        self.apply(fx);
    }

    async fn snapshot(&self, excluded: &[String], ranges: Vec<Value>) -> Snapshot {
        self.helper.request_as("snapshot", json!({ "excluded": excluded, "ranges": ranges }), 3 * SECOND).await
    }

    async fn auto_tick(&'static self) {
        let s = self.settings();
        if !s.auto_check || s.paused {
            let fx = self.auto.lock().unwrap().reset();
            return self.apply(fx);
        }
        let asked = self.auto.lock().unwrap().asked();
        let ranges = asked.iter().map(|i| json!({ "loc": i.fix.start, "len": i.fix.end - i.fix.start })).collect();
        let snap = self.snapshot(&s.excluded_apps, ranges).await;
        let now = self.started.elapsed().as_millis() as u64;
        let fx = self.auto.lock().unwrap().on_snapshot(snap, &asked, now, s.auto_check_mode == AutoCheckMode::Popup);
        self.apply(fx);
    }

    fn apply(&'static self, effects: Vec<Effect>) {
        for e in effects {
            match e {
                Effect::Marks(m) => {
                    if !self.card_open.load(Ordering::SeqCst) {
                        self.set_underlines(m);
                    }
                }
                Effect::Loading(at) => {
                    let seq = self.loading_seq.fetch_add(1, Ordering::SeqCst) + 1;
                    match at {
                        None => {
                            if self.loading_shown.swap(false, Ordering::SeqCst) && !self.popover_open() {
                                self.close_popover(Choice::Dismiss);
                            }
                        }
                        Some(at) => {
                            tauri::async_runtime::spawn(self.auto_loading(at, seq));
                        }
                    }
                }
                Effect::Check(job) => {
                    tauri::async_runtime::spawn(async move {
                        let r = self.auto_check(&job).await;
                        let fx = self.auto.lock().unwrap().on_check_done(job, r);
                        self.apply(fx);
                    });
                }
                Effect::Sentence(fix) => {
                    tauri::async_runtime::spawn(self.open_sentence_popup(fix));
                }
                Effect::Log(m) => debug(&format!("auto {m}")),
            }
        }
    }

    async fn auto_check(&self, job: &Job) -> Result<(String, Vec<String>), String> {
        let s = self.settings();
        // Background: slow gateways need more than the 10s hotkey default.
        let engine = self.engine(30000).await;
        // Reasons cost ~1.5 s; only the popup shows them up front.
        let req = CheckRequest {
            text: job.sentence.clone(),
            dictionary: s.dictionary,
            explain: job.popup && s.explain,
            explain_lang: Some(s.explain_lang),
            tone: None,
        };
        engine.check(&req).await.map(|r| (r.corrected, r.notes.unwrap_or_default())).map_err(|e| e.message)
    }

    fn blocked(&self) -> bool {
        self.card_open.load(Ordering::SeqCst) || self.popover_open()
    }

    /// Popup mode: "Checking…" under the sentence; never over a card or a manual check.
    async fn auto_loading(&'static self, (start, length): (usize, usize), seq: u64) {
        if self.blocked() {
            return;
        }
        // Sentence end, by the caret.
        let anchor = self.range_anchor(start + length.saturating_sub(1), length.min(1), None).await;
        if seq != self.loading_seq.load(Ordering::SeqCst) || self.blocked() {
            return;
        }
        self.loading_shown.store(true, Ordering::SeqCst);
        self.show_loading(anchor);
    }

    /// Anchor below a text range (above when there is no room), from fresh AX bounds.
    async fn range_anchor(&self, loc: usize, len: usize, known: Option<Rect>) -> Option<Anchor> {
        let r = match known {
            Some(r) => Some(r),
            None => {
                let snap = self.snapshot(&self.settings().excluded_apps, vec![json!({ "loc": loc, "len": len })]).await;
                if snap.ok && snap.visible == Some(true) {
                    snap.bounds.and_then(|b| b.into_iter().next().flatten())
                } else {
                    None
                }
            }
        };
        r.map(below_rect)
    }

    async fn issue_anchor(&self, it: &Issue) -> Option<Anchor> {
        let known = self.underlines.lock().unwrap().rects.get(&it.id).copied();
        self.range_anchor(it.fix.start, it.fix.end - it.fix.start, known).await
    }

    async fn replace_range(&self, loc: usize, len: usize, text: &str, expect: &str) -> Reply {
        let pid = self.auto.lock().unwrap().pid;
        self.helper.request_as("replace_range", json!({ "loc": loc, "len": len, "text": text, "expect": expect, "pid": pid }), 5 * SECOND).await
    }

    fn replace_failed(&self, r: &Reply) {
        let e = r.error.as_deref().unwrap_or_default();
        self.notify(&if e == "stale" { "Text changed; fix skipped.".into() } else { format!("Could not replace ({e}). Try the hotkey instead.") });
    }

    /// Popup mode: the whole sentence's fix with reasons, opened as soon as the check comes back.
    async fn open_sentence_popup(&'static self, fix: SentenceFix) {
        let (it, app_name) = {
            let a = self.auto.lock().unwrap();
            (a.issue(fix.first_id).cloned(), a.app_name.clone())
        };
        // Never cover a card or a manual check.
        let Some(it) = it else { return };
        if self.blocked() || self.card_open.swap(true, Ordering::SeqCst) {
            return;
        }
        // A pending hint must not land over the popup.
        self.loading_seq.fetch_add(1, Ordering::SeqCst);
        self.loading_shown.store(false, Ordering::SeqCst);
        let anchor = self.issue_anchor(&it).await;
        let choice = self.show_diff(PopoverPayload::diff(&fix.original, &fix.corrected, app_name, fix.notes.clone()), anchor).await;
        let len = fix.original.encode_utf16().count();
        if choice == Choice::Dismiss {
            self.auto.lock().unwrap().resolve_sentence(fix.start, len, None);
        } else {
            let w = self.replace_range(fix.start, len, &fix.corrected, &fix.original).await;
            debug(&format!("auto popup write ok={} method={} err={}", w.ok, w.method.as_deref().unwrap_or("-"), w.error.as_deref().unwrap_or("-")));
            self.auto.lock().unwrap().resolve_sentence(fix.start, len, w.ok.then_some(fix.corrected.as_str()));
            if w.ok {
                self.insights.lock().unwrap().record(&fix.original, &fix.corrected, &fix.notes);
            } else {
                self.replace_failed(&w);
            }
        }
        self.card_open.store(false, Ordering::SeqCst);
    }

    pub async fn open_issue_card(&'static self, id: u64) {
        let (it, app_name) = {
            let a = self.auto.lock().unwrap();
            (a.issue(id).cloned(), a.app_name.clone())
        };
        let Some(it) = it else { return };
        if self.card_open.swap(true, Ordering::SeqCst) {
            return;
        }
        let f = &it.fix;
        let notes: Vec<String> = f.reason.iter().cloned().collect();
        let anchor = self.issue_anchor(&it).await;
        let choice = self.show_diff(PopoverPayload::diff(&f.original, &f.replacement, app_name, notes.clone()), anchor).await;
        if choice == Choice::Dismiss {
            self.auto.lock().unwrap().dismiss(id);
        } else {
            let w = self.replace_range(f.start, f.end - f.start, &f.replacement, &f.original).await;
            if w.ok {
                self.insights.lock().unwrap().record(&f.original, &f.replacement, &notes);
                self.auto.lock().unwrap().applied(id);
            } else {
                self.auto.lock().unwrap().dismiss(id);
                self.replace_failed(&w);
            }
        }
        self.card_open.store(false, Ordering::SeqCst);
    }

    /// Show exactly these marks (screen coords of the word); every other strip is hidden.
    fn set_underlines(&'static self, marks: Vec<Mark>) {
        let plan: Vec<(usize, Option<Rect>)> = {
            let mut u = self.underlines.lock().unwrap();
            u.rects = marks.iter().map(|m| (m.id, m.rect)).collect();
            for s in u.slots.iter_mut() {
                if s.is_some_and(|id| !marks.iter().any(|m| m.id == id)) {
                    *s = None;
                }
            }
            for m in &marks {
                if u.slots.contains(&Some(m.id)) {
                    continue;
                }
                match u.slots.iter().position(Option::is_none) {
                    Some(i) => u.slots[i] = Some(m.id),
                    None => u.slots.push(Some(m.id)),
                }
            }
            u.slots.iter().enumerate().map(|(i, s)| (i, s.and_then(|id| u.rects.get(&id).copied()))).collect()
        };
        let _ = self.app.run_on_main_thread(move || {
            for (i, rect) in plan {
                let label = format!("underline-{i}");
                let Some(r) = rect else {
                    ui::hide(&self.app, &label);
                    continue;
                };
                if self.app.get_webview_window(&label).is_none() && ui::float_panel(&self.app, &label, "underline.html", 40.0, 8.0, false).is_err() {
                    continue;
                }
                const STRIP: f64 = 8.0;
                let area = ui::screen_near(&self.app, r.x, r.y).bounds;
                let width = r.w.round().min(area.x + area.w - r.x).max(12.0);
                ui::resize(&self.app, &label, width, STRIP);
                ui::place(&self.app, &label, r.x, r.y + r.h - STRIP + 1.0);
                ui::show_panel(&self.app, &label);
            }
        });
    }

    pub fn underline_clicked(&'static self, label: &str) {
        let slot = label.strip_prefix("underline-").and_then(|s| s.parse::<usize>().ok());
        let id = slot.and_then(|i| self.underlines.lock().unwrap().slots.get(i).copied().flatten());
        if let Some(id) = id {
            tauri::async_runtime::spawn(self.open_issue_card(id));
        }
    }

    // ---------- Badge ----------

    fn badge_start(&'static self) {
        let generation = {
            let mut b = self.badge.lock().unwrap();
            if b.running {
                return;
            }
            b.running = true;
            b.generation += 1;
            b.generation
        };
        tauri::async_runtime::spawn(async move {
            let mut every = tokio::time::interval(Duration::from_millis(400));
            every.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            loop {
                every.tick().await;
                if self.badge.lock().unwrap().generation != generation {
                    break;
                }
                self.badge_tick().await;
            }
        });
    }

    fn badge_stop(&self) {
        let mut b = self.badge.lock().unwrap();
        if b.running {
            b.running = false;
            b.generation += 1;
        }
        drop(b);
        self.badge_hide();
    }

    fn badge_hide(&self) {
        let mut b = self.badge.lock().unwrap();
        if b.visible {
            b.visible = false;
            ui::hide(&self.app, "badge");
        }
    }

    fn badge_paint(&self, typing: bool) {
        let cls = {
            let b = self.badge.lock().unwrap();
            if !b.visible {
                return;
            }
            if b.checking {
                "checking"
            } else if typing {
                "typing"
            } else {
                ""
            }
        };
        if let Some(w) = self.app.get_webview_window("badge") {
            let _ = w.eval(format!("document.getElementById(\"d\").className={}", json!(cls)));
        }
    }

    async fn badge_tick(&self) {
        let r = self.helper.request("caret", json!({ "excluded": self.settings().excluded_apps }), Duration::from_millis(1500)).await;
        let caret = (r["ok"] == true && r["visible"] == true)
            .then(|| serde_json::from_value::<CaretRect>(r.clone()).ok())
            .flatten();
        let Some(caret) = caret else {
            self.badge.lock().unwrap().seen = false;
            return self.badge_hide();
        };
        let pos = badge_position(caret);
        let area = ui::screen_near(&self.app, pos.x, pos.y).work;
        let area = serde_json::from_value(json!({ "x": area.x, "y": area.y, "width": area.w, "height": area.h })).expect("area shape");
        let p = clamp_to_area(pos, area);
        let (cx, cy) = ui::cursor(&self.app);
        let (moved, show) = {
            let mut b = self.badge.lock().unwrap();
            b.bundle_id = r["bundleId"].as_str().unwrap_or_default().to_string();
            let key = format!("{},{},{},{}", caret.x, caret.y, caret.w, caret.h);
            let moved = key != b.last;
            b.last = key;
            // Show only after the first change in a field (i.e. typing), and for a few seconds after the last one.
            if (moved && b.seen) || r["selLen"].as_f64().unwrap_or(0.0) > 0.0 {
                b.last_move = Some(Instant::now());
            }
            b.seen = true;
            let (bx, by) = b.pos;
            let hovering = b.visible && cx >= bx && cx <= bx + BADGE_SIZE && cy >= by && cy <= by + BADGE_SIZE;
            let recent = b.last_move.is_some_and(|t| t.elapsed().as_millis() <= KEEP_MS);
            let show = b.checking || hovering || recent;
            if show {
                b.pos = (p.x, p.y);
            }
            (moved, show)
        };
        if !show {
            return self.badge_hide();
        }
        ui::place(&self.app, "badge", p.x, p.y);
        let was = std::mem::replace(&mut self.badge.lock().unwrap().visible, true);
        if !was {
            ui::show_panel(&self.app, "badge");
        }
        self.badge_paint(moved);
    }

    // ---------- Lifecycle ----------

    pub async fn start(&'static self) {
        if self.settings().auto_check {
            self.auto_start();
        }
        self.set_status(Status::of(StatusKind::Idle));
        let trusted = self.ax_trusted().await;
        self.register_hotkey().await;
        if !self.settings().first_run_acknowledged {
            self.first_run_dialog().await;
            self.update_settings(json!({ "firstRunAcknowledged": true }));
        }
        if !trusted {
            self.set_status(Status { kind: StatusKind::NeedsPermission, message: Some("Grant Accessibility permission".into()) });
            self.helper.request("prompt_ax", json!({}), 10 * SECOND).await;
        }
    }

    async fn first_run_dialog(&self) {
        use tauri_plugin_dialog::{DialogExt, MessageDialogButtons, MessageDialogKind};
        let detail = format!(
            "1. Grant Accessibility permission when macOS asks (System Settings → Privacy & Security → Accessibility). Needed to read and replace text.\n\
             2. Select text and press {} to proofread. ⌥⌘T rewrites in a tone.\n\
             3. Click the ug icon in the menu bar for everything else: auto-check, tone, engine, dictionary.\n\n\
             Privacy: checked text is sent to {} (change it from the ug menu-bar panel). Password fields and excluded apps are never read. Text is never logged or stored.",
            self.hotkey_label(),
            self.settings().endpoint
        );
        let (tx, rx) = oneshot::channel();
        self.app
            .dialog()
            .message(detail)
            .title(format!("Welcome to {APP_NAME}"))
            .kind(MessageDialogKind::Info)
            .buttons(MessageDialogButtons::OkCustom("Get started".into()))
            .show(move |_| {
                let _ = tx.send(());
            });
        let _ = rx.await;
    }
}

pub fn accept_shortcut() -> Shortcut {
    Shortcut::new(Some(Modifiers::SUPER), Code::Enter)
}

pub fn dismiss_shortcut() -> Shortcut {
    Shortcut::new(None, Code::Escape)
}

impl FlowDeps for Ctx {
    fn settings(&self) -> Settings {
        Ctx::settings(self)
    }

    async fn read_selection(&self, excluded: &[String], select_all: bool) -> Selection {
        let r: Selection = self.helper.request_as("read", json!({ "excluded": excluded, "selectAll": select_all }), 8 * SECOND).await;
        debug(&format!(
            "read app={} ok={} method={} err={}",
            r.bundle_id.as_deref().unwrap_or("-"),
            r.ok,
            r.method.as_deref().unwrap_or("-"),
            r.error.as_deref().unwrap_or("-")
        ));
        r
    }

    async fn write_selection(&self, text: &str, pid: Option<i64>, method: Option<&str>) -> Reply {
        let r: Reply = self.helper.request_as("write", json!({ "text": text, "pid": pid, "method": method }), 8 * SECOND).await;
        debug(&format!("write ok={} method={} err={}", r.ok, r.method.as_deref().unwrap_or("-"), r.error.as_deref().unwrap_or("-")));
        r
    }

    async fn check(&self, text: &str, tone: Option<Tone>) -> Result<(String, Vec<String>), String> {
        let s = Ctx::settings(self);
        let req = CheckRequest { text: text.into(), dictionary: s.dictionary, explain: s.explain, explain_lang: Some(s.explain_lang), tone };
        self.engine(0).await.check(&req).await.map(|r| (r.corrected, r.notes.unwrap_or_default())).map_err(|e| e.message)
    }

    fn set_status(&self, s: Status) {
        ctx().set_status(s);
    }

    fn notify(&self, message: &str) {
        Ctx::notify(self, message);
    }

    async fn show_diff(&self, p: PopoverPayload) -> Choice {
        Ctx::show_diff(self, p, self.flow_anchor()).await
    }

    fn show_message(&self, message: &str) {
        ctx().show_message(message, self.flow_anchor());
    }

    // The manual check owns the popover now.
    fn show_loading(&self) {
        self.loading_seq.fetch_add(1, Ordering::SeqCst);
        self.loading_shown.store(false, Ordering::SeqCst);
        Ctx::show_loading(self, self.flow_anchor());
    }

    fn hide_loading(&self) {
        self.close_popover(Choice::Dismiss);
    }

    fn on_applied(&self, original: &str, corrected: &str, notes: &[String]) {
        self.insights.lock().unwrap().record(original, corrected, notes);
    }
}
