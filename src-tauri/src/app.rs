use crate::autocheck::{locate, AutoChecker, Effect, Found, Job, Outcome, Rect, Snapshot, POLL_MS};
use crate::flow::{FixFlow, FixOutcome, FlowDeps};
use crate::helper::{Helper, HelperEvent, Reply, Selection};
use crate::ipc::{Choice, PendingFix, PopoverKind, PopoverPayload, Status, StatusKind, TrayState, UpdateNote};
use crate::ui;
use serde_json::{json, Value};
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime};
use tauri::menu::{CheckMenuItem, IsMenuItem, Menu, MenuItem, PredefinedMenuItem};
use tauri::{AppHandle, Emitter, Manager, Wry};
use tauri_plugin_global_shortcut::{Code, GlobalShortcutExt, Modifiers, Shortcut};
use tauri_plugin_notification::NotificationExt;
use tauri_plugin_updater::UpdaterExt;
use tokio::sync::{mpsc, oneshot};
use ugrammar_core::engine::{CheckRequest, OpenAICompatibleEngine, OpenAIConfig, Tone};
use ugrammar_core::insights::InsightStore;
use ugrammar_core::settings::{Mod, Settings, SettingsStore};
use ugrammar_core::text::{to_u16, utf16_len};

pub const APP_NAME: &str = "ugrammar";
const REPO_URL: &str = "https://github.com/tinwritescode/ugrammar";
const LICENSE_URL: &str = "https://tyvn.online";
const AX_SETTINGS_URL: &str = "x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility";
const KEY_SERVICE: &str = "Proofy";
const KEY_ACCOUNT: &str = "api-key";
pub const POPOVER_W: f64 = 460.0;
pub const TRAY_W: f64 = 344.0;
const TRAY_MAX_H: f64 = 600.0;
const SECOND: Duration = Duration::from_secs(1);
const UPDATE_EVERY: Duration = Duration::from_secs(6 * 3600);
/// An untouched auto-check popup moves to the menu-bar list after this long.
const POPUP_IDLE: Duration = Duration::from_secs(8);
/// Taller fields scroll: below them is too far from the sentence.
const FIELD_MAX_H: f64 = 80.0;

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

fn open(target: &str) {
    let _ = std::process::Command::new("/usr/bin/open").arg(target).spawn();
}

fn parse_tone(id: &str) -> Option<Tone> {
    serde_json::from_value(json!(id)).ok()
}

fn outcome(ok: bool, method: &Option<String>, error: &Option<String>) -> String {
    format!("ok={ok} method={} err={}", method.as_deref().unwrap_or("-"), error.as_deref().unwrap_or("-"))
}

/// Ticks `tick` every `period` until `live` turns false; missed ticks are skipped.
fn spawn_poll<F>(period: Duration, live: impl Fn() -> bool + Send + 'static, tick: impl Fn() -> F + Send + 'static)
where
    F: std::future::Future<Output = ()> + Send,
{
    tauri::async_runtime::spawn(async move {
        let mut every = tokio::time::interval(period);
        every.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            every.tick().await;
            if !live() {
                break;
            }
            tick().await;
        }
    });
}

#[derive(Debug, Clone, Copy)]
pub enum Anchor {
    /// Centered on `x`; `y` = top edge when placed below, `alt_y` = bottom edge when placed above; `above` tries above first.
    Text { x: f64, y: f64, alt_y: f64, above: bool },
    /// The popover's top-right corner, for text without usable bounds.
    Corner { x: f64, y: f64 },
}

fn below_rect(r: Rect) -> Anchor {
    Anchor::Text { x: r.x + r.w / 2.0, y: r.y + r.h + 6.0, alt_y: r.y - 6.0, above: false }
}

#[derive(Default)]
struct Popover {
    resolve: Option<oneshot::Sender<Choice>>,
    anchor: Option<Anchor>,
    size: (f64, f64),
    /// Where it was last placed.
    rect: Option<ui::Area>,
    /// Bumped by every close, so a stale message timer cannot close a newer popover.
    generation: u64,
    payload: Option<PopoverPayload>,
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
    /// Fix id of the open auto-check popup.
    auto_popup: Mutex<Option<u64>>,
    /// Busy flag and inbox ids last shown in the menu bar.
    auto_seen: Mutex<(bool, Vec<u64>)>,
    flow: FixFlow,
    flow_anchor: Mutex<Option<Anchor>>,
    tray: Mutex<TrayPanel>,
    update: Mutex<Option<String>>,
    update_note: Mutex<Option<UpdateNote>>,
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
        auto_popup: Mutex::new(None),
        auto_seen: Mutex::default(),
        flow: FixFlow::default(),
        flow_anchor: Mutex::new(None),
        tray: Mutex::default(),
        update: Mutex::new(None),
        update_note: Mutex::new(None),
        started: Instant::now(),
    };
    let _ = CTX.set(ctx);
    rx
}

impl Ctx {
    pub fn settings(&self) -> Settings {
        self.store.lock().unwrap().get()
    }

    /// Every settings write goes through here so status and auto-check follow it.
    pub fn update_settings(&'static self, patch: Value) -> Settings {
        let before = self.settings();
        let cfg = {
            let mut s = self.store.lock().unwrap();
            s.update(patch).unwrap_or_else(|_| s.get())
        };
        let cur = self.status.lock().unwrap().clone();
        self.set_status(if matches!(cur.kind, StatusKind::Paused | StatusKind::Idle) { Status::of(StatusKind::Idle) } else { cur });
        let engine = |s: &Settings| (s.endpoint.clone(), s.model.clone(), s.dictionary.clone(), s.explain, s.explain_lang.clone());
        if engine(&before) != engine(&cfg) {
            self.auto.lock().unwrap().clear_cache(); // old answers came from another engine setup
        }
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

    async fn check_text(&self, text: String, explain: bool, tone: Option<Tone>, min_timeout_ms: u64) -> Result<(String, Vec<String>), String> {
        let s = self.settings();
        let cfg = OpenAIConfig { endpoint: s.endpoint, model: s.model, timeout_ms: s.timeout_ms.max(min_timeout_ms) };
        let req = CheckRequest { text, dictionary: s.dictionary, explain, explain_lang: Some(s.explain_lang), tone };
        let engine = OpenAICompatibleEngine::new(cfg, self.api_key().await);
        engine.check(&req).await.map(|r| (r.corrected, r.notes.unwrap_or_default())).map_err(|e| e.message)
    }

    pub fn notify(&self, body: &str) {
        let _ = self.app.notification().builder().title(APP_NAME).body(body).show();
    }

    async fn prompt_ax(&self) {
        self.helper.request("prompt_ax", json!({}), 10 * SECOND).await;
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
            update: self.update.lock().unwrap().clone(),
            update_note: *self.update_note.lock().unwrap(),
            pending: self
                .auto
                .lock()
                .unwrap()
                .inbox()
                .iter()
                .map(|f| PendingFix {
                    id: f.id,
                    app_name: f.app_name.clone(),
                    ops: ugrammar_core::diff::diff_words(&f.sentence, &f.corrected),
                    notes: f.notes.clone(),
                })
                .collect(),
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
            let _ = t.set_title(Some(self.tray_title(kind)));
            let _ = t.set_tooltip(Some(format!("{APP_NAME} — {}", STATUS_LABEL(kind))));
            if let Ok(m) = self.tray_menu() {
                let _ = t.set_menu(Some(m));
            }
        }
        if ui::is_visible(&self.app, "tray") {
            tauri::async_runtime::spawn(async move {
                let st = self.tray_state().await;
                let _ = self.app.emit_to("tray", "tray:state", st);
            });
        }
    }

    /// Status other than idle wins; then an auto-check in flight, then the count of fixes waiting.
    fn tray_title(&self, kind: StatusKind) -> String {
        let a = self.auto.lock().unwrap();
        match kind {
            StatusKind::Idle if a.busy() => STATUS_TITLE(StatusKind::Checking).into(),
            StatusKind::Idle if !a.inbox().is_empty() => format!("{} {}", STATUS_TITLE(kind), a.inbox().len()),
            _ => STATUS_TITLE(kind).into(),
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
        if !ui::is_visible(&self.app, "tray") {
            return;
        }
        ui::hide(&self.app, "tray");
        self.tray.lock().unwrap().hidden_at = Some(Instant::now());
        *self.update_note.lock().unwrap() = None;
    }

    pub async fn toggle_tray(&'static self) {
        if ui::is_visible(&self.app, "tray") {
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
            ui::place(&self.app, "tray", area.fit_x(b.x + b.w / 2.0 - TRAY_W / 2.0, TRAY_W), area.y + 6.0);
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
                self.prompt_ax().await;
                open(AX_SETTINGS_URL);
            }
            "github" => open(REPO_URL),
            "license" => open(LICENSE_URL),
            "restart" => self.app.restart(),
            tone => {
                let Some(tone) = parse_tone(tone) else { return };
                // Hand focus back to the app the user was writing in, then rewrite its selection.
                let _ = self.app.hide();
                tokio::time::sleep(Duration::from_millis(200)).await;
                self.rewrite(tone).await;
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

    pub fn popover_payload(&self) -> Option<PopoverPayload> {
        self.popover.lock().unwrap().payload.clone()
    }

    fn cursor_anchor(&self) -> Anchor {
        let (x, y) = ui::cursor(&self.app);
        Anchor::Text { x, y: y + 20.0, alt_y: y - 36.0, above: false }
    }

    /// On the anchor's preferred side if it fits, else the other; always clamped inside the display. Re-run after each resize.
    fn place_popover(&self) {
        let (a, (w, h)) = {
            let p = self.popover.lock().unwrap();
            (p.anchor.unwrap_or_else(|| self.cursor_anchor()), p.size)
        };
        let (x, y) = match a {
            Anchor::Text { x, y, alt_y, above } => {
                let area = ui::screen_near(&self.app, x, y).work;
                let fits_below = y + h <= area.y + area.h - 8.0;
                let top = if (above && alt_y - h >= area.y + 8.0) || !fits_below { alt_y - h } else { y };
                (area.fit_x(x - w / 2.0, w), area.fit_y(top, h))
            }
            Anchor::Corner { x, y } => {
                let area = ui::screen_near(&self.app, x - 1.0, y).work;
                (area.fit_x(x - w, w), area.fit_y(y, h))
            }
        };
        self.popover.lock().unwrap().rect = Some(ui::Area { x, y, w, h });
        ui::place(&self.app, "popover", x, y);
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

    /// `esc`: also take Esc globally; auto-check popups leave it to the app being typed in.
    pub async fn show_diff(&self, p: PopoverPayload, anchor: Option<Anchor>, esc: bool) -> Choice {
        let p = PopoverPayload { esc, ..p };
        self.close_popover(Choice::Later);
        let (tx, rx) = oneshot::channel();
        self.popover.lock().unwrap().resolve = Some(tx);
        self.show_popover(p, anchor);
        let gs = self.app.global_shortcut();
        let _ = gs.register(accept_shortcut());
        if esc {
            let _ = gs.register(dismiss_shortcut());
        }
        rx.await.unwrap_or(Choice::Dismiss)
    }

    pub fn show_message(&'static self, message: &str, anchor: Option<Anchor>) {
        self.close_popover(Choice::Later);
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
        self.close_popover(Choice::Later);
        self.show_popover(PopoverPayload::text(PopoverKind::Loading, "Đang kiểm tra…"), anchor);
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
    pub async fn run_flow(&'static self, tone: Option<Tone>) -> FixOutcome {
        let anchor = self.selection_anchor().await;
        *self.flow_anchor.lock().unwrap() = anchor;
        let outcome = self.flow.run(self, tone).await;
        *self.flow_anchor.lock().unwrap() = None;
        outcome
    }

    fn flow_anchor(&self) -> Option<Anchor> {
        *self.flow_anchor.lock().unwrap()
    }

    /// Hotkey checks the selection.
    pub async fn manual_check(&'static self, trigger: &str) {
        debug(trigger);
        // A still-open popover is set aside: an auto-check fix waits in the menu-bar list.
        self.close_popover(Choice::Later);
        tokio::task::yield_now().await;
        let outcome = self.run_flow(None).await;
        let st = self.status();
        debug(&format!("outcome={outcome:?} status={:?}{}", st.kind, st.message.map(|m| format!(" ({m})")).unwrap_or_default()));
    }

    async fn rewrite(&'static self, tone: Tone) {
        debug(&format!("tone={tone:?} outcome={:?}", self.run_flow(Some(tone)).await));
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
        self.close_popover(Choice::Later);
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
        self.popup_menu("popover", menu);
    }

    fn popup_menu(&self, window: &str, menu: tauri::Result<Menu<Wry>>) {
        if let (Ok(m), Some(w)) = (menu, self.app.get_webview_window(window)) {
            let _ = w.popup_menu(&m);
        }
    }

    pub async fn on_menu(&'static self, id: &str) {
        match id {
            "pause" => {
                let paused = !self.settings().paused;
                self.update_settings(json!({ "paused": paused }));
            }
            "ax" => open(AX_SETTINGS_URL),
            "settings" => self.open_panel().await,
            "quit" => self.app.exit(0),
            _ => {
                if let Some(tone) = id.strip_prefix("tone_").and_then(parse_tone) {
                    self.rewrite(tone).await;
                }
            }
        }
    }

    pub async fn helper_events(&'static self, mut rx: mpsc::UnboundedReceiver<HelperEvent>) {
        while let Some(e) = rx.recv().await {
            match e {
                HelperEvent::Hotkey => {
                    tauri::async_runtime::spawn(self.manual_check("hotkey"));
                }
                HelperEvent::Hotkey2 => {
                    let _ = self.app.run_on_main_thread(move || self.on_hotkey2());
                }
                HelperEvent::Restarted => self.register_hotkey().await,
            }
        }
    }

    // ---------- Auto-check (popup) ----------

    pub fn auto_start(&'static self) {
        if self.auto_running.swap(true, Ordering::SeqCst) {
            return;
        }
        let generation = self.auto_generation.fetch_add(1, Ordering::SeqCst) + 1;
        spawn_poll(Duration::from_millis(POLL_MS), move || self.auto_generation.load(Ordering::SeqCst) == generation, move || self.auto_tick());
    }

    pub fn auto_stop(&'static self) {
        if !self.auto_running.swap(false, Ordering::SeqCst) {
            return;
        }
        self.auto_generation.fetch_add(1, Ordering::SeqCst);
        self.auto.lock().unwrap().reset();
    }

    async fn snapshot(&self, excluded: &[String], ranges: Vec<Value>) -> Snapshot {
        self.helper.request_as("snapshot", json!({ "excluded": excluded, "ranges": ranges }), 3 * SECOND).await
    }

    fn now(&self) -> u64 {
        self.started.elapsed().as_millis() as u64
    }

    async fn auto_tick(&'static self) {
        let s = self.settings();
        if !s.auto_check || s.paused {
            return self.auto.lock().unwrap().reset();
        }
        let snap = self.snapshot(&s.excluded_apps, vec![]).await;
        let fx = self.auto.lock().unwrap().on_snapshot(snap, self.now());
        self.apply(fx);
    }

    fn apply(&'static self, effects: Vec<Effect>) {
        for e in effects {
            match e {
                Effect::Check(job) => {
                    tauri::async_runtime::spawn(async move {
                        let r = self.auto_check(&job).await;
                        let fx = self.auto.lock().unwrap().on_check_done(job, r, self.now());
                        self.apply(fx);
                    });
                }
                Effect::Show(f) => {
                    tauri::async_runtime::spawn(self.open_sentence_popup(f));
                }
                Effect::Hide(id) => {
                    let mine = self.auto_popup.lock().unwrap().take_if(|p| *p == id).is_some();
                    if mine {
                        self.close_popover(Choice::Later);
                    }
                }
                Effect::Log(m) => debug(&format!("auto {m}")),
            }
        }
        self.auto_changed();
    }

    /// The menu-bar title and the tray's list follow the checker's busy flag and inbox.
    fn auto_changed(&'static self) {
        let seen = {
            let a = self.auto.lock().unwrap();
            (a.busy(), a.inbox().iter().map(|f| f.id).collect::<Vec<_>>())
        };
        let changed = {
            let mut last = self.auto_seen.lock().unwrap();
            std::mem::replace(&mut *last, seen.clone()) != seen
        };
        if changed {
            self.refresh_tray();
        }
    }

    fn auto_resolve(&'static self, id: u64, outcome: Outcome) {
        self.auto.lock().unwrap().resolve(id, outcome, self.now());
        self.auto_changed();
    }

    async fn auto_check(&self, job: &Job) -> Result<(String, Vec<String>), String> {
        // Reasons cost ~1.5 s; the popup shows them up front.
        let explain = self.settings().explain;
        // Background: slow gateways need more than the 10s hotkey default.
        self.check_text(job.sentence.clone(), explain, None, 30000).await
    }

    /// Under a one-line field; else above the sentence's first line (else a line below its last); with no bounds,
    /// or the sentence scrolled away, the focused window's top-right corner, else the screen's.
    async fn sentence_anchor(&self, f: &Found) -> Anchor {
        let last = f.start + utf16_len(&f.sentence).saturating_sub(1);
        let snap = self.snapshot(&self.settings().excluded_apps, vec![json!({ "loc": f.start, "len": 1 }), json!({ "loc": last, "len": 1 })]).await;
        let here = snap.ok && snap.visible == Some(true) && snap.pid == f.pid;
        // A one-line field (Chrome's address bar, search boxes): right under it, bounds or not.
        if let (true, Some(r)) = (here, snap.field.filter(|r| r.h > 0.0 && r.h <= FIELD_MAX_H)) {
            debug("auto anchor field");
            return Anchor::Text { x: r.x + POPOVER_W / 2.0, y: r.y + r.h + 6.0, alt_y: r.y - 6.0, above: false };
        }
        let bounds = snap.bounds.unwrap_or_default();
        if let (true, Some(Some(a)), Some(Some(z))) = (here, bounds.first(), bounds.get(1)) {
            let shown = |r: &Rect| match snap.window {
                Some(w) => r.y >= w.y && r.y + r.h <= w.y + w.h,
                None => ui::screen_near(&self.app, r.x, r.y).bounds.contains(r.x, r.y),
            };
            if shown(a) && shown(z) {
                debug("auto anchor text");
                return Anchor::Text { x: a.x + POPOVER_W / 2.0 - 16.0, y: z.y + 2.0 * z.h + 6.0, alt_y: a.y - 6.0, above: true };
            }
        }
        if let (true, Some(w)) = (here, snap.window) {
            debug("auto anchor window");
            return Anchor::Corner { x: w.x + w.w - 12.0, y: w.y + 40.0 };
        }
        debug("auto anchor screen");
        let (x, y) = ui::cursor(&self.app);
        let area = ui::screen_near(&self.app, x, y).work;
        Anchor::Corner { x: area.x + area.w - 12.0, y: area.y + 12.0 }
    }

    /// Writes an accepted fix over its sentence and records it; says why when it cannot.
    async fn write_fix(&'static self, f: &Found) -> bool {
        let args = json!({ "loc": f.start, "len": utf16_len(&f.sentence), "text": f.corrected, "expect": f.sentence, "pid": f.pid });
        let r: Reply = self.helper.request_as("replace_range", args, 5 * SECOND).await;
        debug(&format!("auto write {}", outcome(r.ok, &r.method, &r.error)));
        if r.ok {
            self.insights.lock().unwrap().record(&f.sentence, &f.corrected, &f.notes);
        } else {
            self.show_message(&format!("Không sửa được, hãy chọn câu rồi bấm {}", self.hotkey_label()), None);
        }
        r.ok
    }

    /// Any popover up (a manual check, its message, another fix) keeps an auto-check popup away.
    fn popover_busy(&self) -> bool {
        self.popover.lock().unwrap().payload.is_some()
    }

    /// The sentence's fix with reasons. ⌘↩ applies; Esc stays with the app being typed in.
    async fn open_sentence_popup(&'static self, f: Found) {
        let free = !self.popover_busy() && {
            let mut p = self.auto_popup.lock().unwrap();
            p.is_none() && p.insert(f.id) == &f.id
        };
        if !free {
            return self.auto_resolve(f.id, Outcome::Later);
        }
        let anchor = self.sentence_anchor(&f).await;
        if *self.auto_popup.lock().unwrap() != Some(f.id) {
            return; // the user moved on meanwhile; the fix is in the list
        }
        // show_diff's own close bumps the generation once.
        let generation = self.popover.lock().unwrap().generation + 1;
        tauri::async_runtime::spawn(self.expire_popup(generation));
        let choice = self.show_diff(PopoverPayload::diff(&f.sentence, &f.corrected, f.app_name.clone(), f.notes.clone()), Some(anchor), false).await;
        let outcome = match choice {
            Choice::Accept if self.write_fix(&f).await => Outcome::Applied,
            Choice::Dismiss => Outcome::Dismissed,
            _ => Outcome::Later,
        };
        self.auto_popup.lock().unwrap().take_if(|p| *p == f.id);
        self.auto_resolve(f.id, outcome);
    }

    /// An untouched popup closes into the menu-bar list; the wait restarts while the mouse is over it.
    async fn expire_popup(&self, generation: u64) {
        loop {
            tokio::time::sleep(POPUP_IDLE).await;
            let hovered = {
                let p = self.popover.lock().unwrap();
                if p.generation != generation {
                    return;
                }
                let (x, y) = ui::cursor(&self.app);
                p.rect.is_some_and(|r| r.contains(x, y))
            };
            if !hovered {
                return self.close_popover(Choice::Later);
            }
        }
    }

    /// The tray list's Áp dụng / Bỏ qua. The fix is written into its own app, brought forward first.
    pub async fn pending_action(&'static self, id: u64, choice: Choice) {
        if choice != Choice::Accept {
            return self.auto_resolve(id, Outcome::Dismissed);
        }
        let Some(mut f) = self.auto.lock().unwrap().inbox_item(id).cloned() else { return };
        self.hide_tray();
        if let Some(pid) = f.pid {
            self.helper.request("activate", json!({ "pid": pid }), 3 * SECOND).await;
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
        let snap = self.snapshot(&self.settings().excluded_apps, vec![]).await;
        if let (Some(text), true) = (snap.text.as_deref(), snap.pid == f.pid) {
            f.start = locate(&to_u16(text), f.start, &f.sentence).unwrap_or(f.start);
        }
        if self.write_fix(&f).await {
            self.auto_resolve(id, Outcome::Applied);
        }
    }

    // ---------- Update ----------

    /// Checks, downloads and installs a newer release over this bundle; it runs from the next launch.
    /// `on_found` runs once a newer release is known, before its download starts.
    async fn install_update(&'static self, on_found: impl FnOnce()) -> Result<Option<String>, tauri_plugin_updater::Error> {
        let Some(u) = self.app.updater()?.check().await? else { return Ok(None) };
        on_found();
        u.download_and_install(|_, _| {}, || {}).await?;
        Ok(Some(u.version))
    }

    fn update_installed(&'static self, v: String) {
        debug(&format!("update installed {v}"));
        self.notify(&format!("Version {v} is installed. Restart {APP_NAME} from the menu bar to use it."));
        *self.update.lock().unwrap() = Some(v);
        self.refresh_tray();
    }

    fn set_update_note(&'static self, note: Option<UpdateNote>) {
        *self.update_note.lock().unwrap() = note;
        self.refresh_tray();
    }

    async fn check_update(&'static self) {
        // A dev binary is not inside an .app bundle to replace.
        if cfg!(debug_assertions) || self.update.lock().unwrap().is_some() {
            return;
        }
        match self.install_update(|| {}).await {
            Ok(Some(v)) => self.update_installed(v),
            Ok(None) => debug("update none"),
            Err(e) => debug(&format!("update failed: {e}")),
        }
    }

    /// The tray's "Check for updates" button: same install as the timer, with the outcome shown in the tray.
    pub async fn check_update_now(&'static self) {
        if cfg!(debug_assertions) {
            return self.set_update_note(Some(UpdateNote::Unsupported));
        }
        if self.update.lock().unwrap().is_some() {
            return; // already installed; the banner asks for a restart
        }
        self.set_update_note(Some(UpdateNote::Checking));
        let note = match self.install_update(|| self.set_update_note(Some(UpdateNote::Downloading))).await {
            Ok(Some(v)) => {
                self.update_installed(v);
                None
            }
            Ok(None) => Some(UpdateNote::UpToDate),
            Err(e) => {
                debug(&format!("update failed: {e}"));
                Some(UpdateNote::Failed)
            }
        };
        self.set_update_note(note);
    }

    // ---------- Lifecycle ----------

    pub async fn start(&'static self) {
        if self.settings().auto_check {
            self.auto_start();
        }
        spawn_poll(UPDATE_EVERY, || true, move || self.check_update());
        self.set_status(Status::of(StatusKind::Idle));
        let trusted = self.ax_trusted().await;
        self.register_hotkey().await;
        if !self.settings().first_run_acknowledged {
            self.first_run_dialog().await;
            self.update_settings(json!({ "firstRunAcknowledged": true }));
        }
        if !trusted {
            self.set_status(Status { kind: StatusKind::NeedsPermission, message: Some("Grant Accessibility permission".into()) });
            self.prompt_ax().await;
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

    async fn read_selection(&self, excluded: &[String]) -> Selection {
        let r: Selection = self.helper.request_as("read", json!({ "excluded": excluded }), 8 * SECOND).await;
        debug(&format!("read app={} {}", r.bundle_id.as_deref().unwrap_or("-"), outcome(r.ok, &r.method, &r.error)));
        r
    }

    async fn write_selection(&self, text: &str, pid: Option<i64>, method: Option<&str>) -> Reply {
        let r: Reply = self.helper.request_as("write", json!({ "text": text, "pid": pid, "method": method }), 8 * SECOND).await;
        debug(&format!("write {}", outcome(r.ok, &r.method, &r.error)));
        r
    }

    async fn check(&self, text: &str, tone: Option<Tone>) -> Result<(String, Vec<String>), String> {
        self.check_text(text.into(), Ctx::settings(self).explain, tone, 0).await
    }

    fn set_status(&self, s: Status) {
        ctx().set_status(s);
    }

    fn notify(&self, message: &str) {
        Ctx::notify(self, message);
    }

    async fn show_diff(&self, p: PopoverPayload) -> Choice {
        Ctx::show_diff(self, p, self.flow_anchor(), true).await
    }

    fn show_message(&self, message: &str) {
        ctx().show_message(message, self.flow_anchor());
    }

    // The manual check owns the popover now.
    fn show_loading(&self) {
        Ctx::show_loading(self, self.flow_anchor());
    }

    fn hide_loading(&self) {
        self.close_popover(Choice::Later);
    }

    fn on_applied(&self, original: &str, corrected: &str, notes: &[String]) {
        self.insights.lock().unwrap().record(original, corrected, notes);
    }
}
