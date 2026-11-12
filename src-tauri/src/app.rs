use crate::autocheck::{locate, AutoChecker, Effect, Found, Job, Outcome, POLL_MS};
use crate::flow::{FixFlow, FixOutcome, FlowDeps};
use crate::helper::{Helper, HelperEvent};
use crate::placement::{self, below_mouse, below_rect, Anchor};
pub use crate::placement::TRAY_W;
pub use crate::popover::POPOVER_W;
use crate::popover::{Closed, Expiry, Session, Ticket};
use crate::reactions::{react, resting};
use crate::helper_client::{HelperClient, HelperError, HotkeySlot, RangeEdit, Selection};
use crate::ipc::{Choice, PendingFix, PermissionId, PermissionStatus, PopoverKind, PopoverPayload, Status, StatusKind, TrayState, UpdateNote};
use crate::ui;
use serde_json::{json, Value};
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime};
use tauri::menu::{CheckMenuItem, IsMenuItem, Menu, MenuItem, PredefinedMenuItem};
use tauri::{AppHandle, Emitter, Manager, Wry};
use tauri_plugin_autostart::ManagerExt;
use tauri_plugin_global_shortcut::{Code, GlobalShortcutExt, Modifiers, Shortcut};
use tauri_plugin_notification::NotificationExt;
use tauri_plugin_updater::UpdaterExt;
use tokio::sync::{mpsc, oneshot};
use ugrammar_core::engine::{EngineSetup, Purpose, Tone};
use ugrammar_core::i18n::{fill, Lang, Strings};
use ugrammar_core::insights::InsightStore;
use ugrammar_core::settings::{Hotkey, Mod, Settings, SettingsStore};
use ugrammar_core::text::{to_u16, utf16_len};

pub const APP_NAME: &str = "ugrammar";
const REPO_URL: &str = "https://github.com/tinwritescode/ugrammar";
const LICENSE_URL: &str = "https://tyvn.online";
const AX_SETTINGS_URL: &str = "x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility";
/// Matches `identifier` in tauri.conf.json; needed to target this app with `tccutil`.
const BUNDLE_ID: &str = "dev.ugrammar.app";
const UPDATE_EVERY: Duration = Duration::from_secs(6 * 3600);
/// An untouched auto-check popup moves to the menu-bar list after this long.
const POPUP_IDLE: Duration = Duration::from_secs(8);

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

/// The `.app` a binary runs from, so the permissions screen can hand the user the thing macOS lists;
/// a dev run has no bundle and gets the bare executable.
fn bundle_of(exe: &Path) -> PathBuf {
    exe.ancestors().find(|p| p.extension().is_some_and(|e| e == "app")).unwrap_or(exe).to_path_buf()
}

pub fn app_bundle() -> PathBuf {
    bundle_of(&std::env::current_exe().unwrap_or_default())
}

fn parse_tone(id: &str) -> Option<Tone> {
    serde_json::from_value(json!(id)).ok()
}

/// Granted before but not now: System Settings still lists the app, so it needs a reset rather
/// than just the "open System Settings" flow a first-time grant uses.
fn permission_is_stale(granted: bool, ever_granted: bool) -> bool {
    !granted && ever_granted
}

fn outcome<T>(r: &Result<T, HelperError>, method: impl Fn(&T) -> Option<&str>) -> String {
    match r {
        Ok(v) => format!("ok method={}", method(v).unwrap_or("-")),
        Err(e) => format!("err={e}"),
    }
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


#[derive(Default)]
struct TrayPanel {
    hidden_at: Option<Instant>,
    /// A native dialog over the panel must not blur it closed.
    pinned: bool,
}

pub struct Ctx {
    pub app: AppHandle,
    pub helper: HelperClient,
    store: Mutex<SettingsStore>,
    insights: Mutex<InsightStore>,
    status: Mutex<Status>,
    popover: Mutex<Session>,
    auto: Mutex<AutoChecker>,
    auto_generation: AtomicU64,
    auto_running: AtomicBool,
    /// Busy flag and inbox ids last shown in the menu bar.
    auto_seen: Mutex<(bool, Vec<u64>)>,
    flow: FixFlow,
    flow_anchor: Mutex<Option<Anchor>>,
    tray: Mutex<TrayPanel>,
    update: Mutex<Option<String>>,
    update_note: Mutex<Option<UpdateNote>>,
    /// Whether the Keychain holds an API key; None until first read.
    has_api_key: Mutex<Option<bool>>,
    /// Last known Accessibility trust, refreshed on launch and by the permissions screen's polling.
    permission_trusted: Mutex<bool>,
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

fn status_label(t: &Strings, k: StatusKind) -> &'static str {
    match k {
        StatusKind::Idle => t.ready,
        StatusKind::Checking => t.checking_status,
        StatusKind::Paused => t.paused,
        StatusKind::Error => t.error,
        StatusKind::NeedsPermission => t.needs_permission,
    }
}

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
        helper: HelperClient::new(Helper::start(helper_path(), tx)),
        store: Mutex::new(SettingsStore::new(&dir)),
        insights: Mutex::new(InsightStore::new(&dir)),
        status: Mutex::new(Status::of(StatusKind::Idle)),
        popover: Mutex::default(),
        auto: Mutex::new(AutoChecker::new()),
        auto_generation: AtomicU64::new(0),
        auto_running: AtomicBool::new(false),
        auto_seen: Mutex::default(),
        flow: FixFlow::default(),
        flow_anchor: Mutex::new(None),
        tray: Mutex::default(),
        update: Mutex::new(None),
        update_note: Mutex::new(None),
        has_api_key: Mutex::new(None),
        // Optimistic default; `start()` probes the real value before this is ever read.
        permission_trusted: Mutex::new(true),
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
        let saved = self.store.lock().unwrap().update(patch);
        let cfg = saved.unwrap_or_else(|e| {
            self.notify(&fill(before.strings().settings_save_failed, &[("error", &e.to_string())]));
            before.clone()
        });
        let r = react(&before, &cfg, &self.status());
        if r.clear_cache {
            self.auto.lock().unwrap().clear_cache();
        }
        if r.auto_check {
            self.auto_start();
        } else {
            self.auto_stop();
        }
        if r.register_hotkey {
            tauri::async_runtime::spawn(self.register_hotkey());
        }
        if let Some(s) = r.status {
            self.set_status(s);
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
        self.helper.api_key().await.ok().flatten()
    }

    /// Saves (or, when empty, deletes) the Keychain key. Err carries the helper's error code.
    pub async fn set_api_key(&self, key: &str) -> Result<(), String> {
        self.helper.set_api_key(key).await.map_err(|e| e.to_string())?;
        *self.has_api_key.lock().unwrap() = Some(!key.is_empty());
        Ok(())
    }

    /// Read from the Keychain once; set_api_key keeps it current after that.
    async fn has_api_key(&self) -> bool {
        if let Some(known) = *self.has_api_key.lock().unwrap() {
            return known;
        }
        // A failed read (helper still starting) is asked again next time.
        let Ok(key) = self.helper.api_key().await else { return false };
        *self.has_api_key.lock().unwrap() = Some(key.is_some());
        key.is_some()
    }

    async fn check_text(&self, text: String, tone: Option<Tone>, purpose: Purpose) -> Result<(String, Vec<String>), String> {
        let s = self.settings();
        let setup = EngineSetup::from_settings(&s);
        let engine = setup.engine(s.timeout_ms, purpose, self.api_key().await);
        engine.check(&setup.request(text, tone)).await.map(|r| (r.corrected, r.notes.unwrap_or_default())).map_err(|e| e.message)
    }

    pub fn notify(&self, body: &str) {
        let _ = self.app.notification().builder().title(APP_NAME).body(body).show();
    }

    async fn prompt_ax(&self) {
        self.helper.prompt_ax().await;
    }

    pub async fn ax_trusted(&self) -> bool {
        self.helper.trusted().await
    }

    /// The permissions screen's current snapshot, from the last probe (see `check_permissions`).
    pub fn permissions(&self) -> Vec<PermissionStatus> {
        let granted = *self.permission_trusted.lock().unwrap();
        let stale = permission_is_stale(granted, self.settings().accessibility_ever_granted);
        vec![PermissionStatus { id: PermissionId::Accessibility, granted, stale }]
    }

    /// Re-probes Accessibility for real; the tray polls this while the permissions screen is open.
    /// Self-heals the tray status once granted, and remembers the grant so a later loss reads as
    /// stale (needs a reset) rather than "never granted".
    pub async fn check_permissions(&'static self) -> Vec<PermissionStatus> {
        let granted = self.ax_trusted().await;
        *self.permission_trusted.lock().unwrap() = granted;
        if granted {
            if !self.settings().accessibility_ever_granted {
                self.update_settings(json!({ "accessibilityEverGranted": true }));
            }
            if self.status().kind == StatusKind::NeedsPermission {
                self.set_status(resting(&self.settings()));
            }
        }
        self.permissions()
    }

    /// Clears the stale Accessibility entry System Settings keeps from a previous build (common
    /// after a rebuild/re-sign), then asks again.
    async fn reset_accessibility(&self) {
        let _ = tokio::process::Command::new("/usr/bin/tccutil").args(["reset", "Accessibility", BUNDLE_ID]).output().await;
        self.prompt_ax().await;
    }

    pub fn status(&self) -> Status {
        self.status.lock().unwrap().clone()
    }

    pub fn set_status(&'static self, s: Status) {
        *self.status.lock().unwrap() = s.clone();
        self.refresh_tray();
    }

    // ---------- Tray ----------

    pub async fn tray_state(&self) -> TrayState {
        let has_api_key = self.has_api_key().await;
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
            permissions: self.permissions(),
            launch_at_login: self.app.autolaunch().is_enabled().unwrap_or(false),
        }
    }

    pub fn clear_insights(&self) {
        self.insights.lock().unwrap().clear();
    }

    /// Right-click fallback: the plain native menu.
    fn tray_menu(&self) -> tauri::Result<Menu<Wry>> {
        let (st, cfg, a) = (self.status(), self.settings(), &self.app);
        let t = cfg.strings();
        let message = st.message.map(|m| format!(" — {m}")).unwrap_or_default();
        let status_text = format!("{}{message}", status_label(t, st.kind));
        let status = MenuItem::with_id(a, "status", fill(t.menu_status, &[("status", &status_text)]), false, None::<&str>)?;
        let hotkey = MenuItem::with_id(a, "hotkey", fill(t.menu_fix, &[("hotkey", &self.hotkey_label())]), false, None::<&str>)?;
        let pause = CheckMenuItem::with_id(a, "pause", t.menu_pause, true, cfg.paused, None::<&str>)?;
        let ax = MenuItem::with_id(a, "ax", t.menu_ax, true, None::<&str>)?;
        let settings = MenuItem::with_id(a, "settings", t.menu_settings, true, None::<&str>)?;
        let quit = MenuItem::with_id(a, "quit", t.menu_quit, true, Some("Cmd+Q"))?;
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
            let _ = t.set_tooltip(Some(format!("{APP_NAME} — {}", status_label(self.settings().strings(), kind))));
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
        self.open_panel_inner(false).await;
    }

    /// Launch's entry point when Accessibility isn't trusted: lands the user on the checklist
    /// instead of the main view, so they see the reason and the fix, not just a status dot.
    pub async fn open_panel_on_permissions(&'static self) {
        self.open_panel_inner(true).await;
    }

    async fn open_panel_inner(&'static self, to_permissions: bool) {
        let Some(w) = self.app.get_webview_window("tray") else { return };
        let _ = self.app.emit_to("tray", "tray:state", self.tray_state().await);
        if to_permissions {
            let _ = self.app.emit_to("tray", "tray:open_permissions", ());
        }
        if let Some(icon) = ui::tray_rect(&self.app) {
            let (x, y) = placement::tray_at(icon, &ui::screens(&self.app));
            ui::place(&self.app, "tray", x, y);
        }
        let _ = w.show();
        let _ = w.set_focus();
    }

    pub fn tray_resize(&self, h: f64) {
        if h <= 0.0 {
            return;
        }
        let h = placement::tray_height(h, ui::tray_rect(&self.app), &ui::screens(&self.app));
        ui::resize(&self.app, "tray", TRAY_W, h);
    }

    pub async fn tray_action(&'static self, a: &str) {
        // The permission steps stay on screen beside System Settings and Finder.
        if !matches!(a, "ax" | "reset_ax" | "reveal_app") {
            self.hide_tray();
        }
        match a {
            "quit" => self.app.exit(0),
            "ax" => {
                self.prompt_ax().await;
                open(AX_SETTINGS_URL);
            }
            "reset_ax" => self.reset_accessibility().await,
            "reveal_app" => {
                let _ = std::process::Command::new("/usr/bin/open").arg("-R").arg(app_bundle()).spawn();
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
        let closed = self.popover.lock().unwrap().close();
        self.popover_closed(closed, choice);
    }

    /// Window side of a close: shortcuts off, hidden, its waiter answered.
    fn popover_closed(&self, closed: Closed, choice: Choice) {
        let gs = self.app.global_shortcut();
        let _ = gs.unregister(accept_shortcut());
        let _ = gs.unregister(dismiss_shortcut());
        ui::hide(&self.app, "popover");
        closed.answer(choice);
    }

    pub fn popover_payload(&self) -> Option<PopoverPayload> {
        self.popover.lock().unwrap().payload()
    }

    /// Re-run after each resize.
    fn place_popover(&self) {
        let Some((a, size)) = ({
            let p = self.popover.lock().unwrap();
            p.anchor().map(|a| (a, p.size()))
        }) else {
            return;
        };
        let r = placement::popover_at(a, size, &ui::screens(&self.app));
        self.popover.lock().unwrap().placed(r);
        ui::place(&self.app, "popover", r.x, r.y);
    }

    /// Opens a popover over whatever was showing; `waiter` gets the user's choice.
    fn open_popover(&self, payload: PopoverPayload, anchor: Option<Anchor>, waiter: Option<oneshot::Sender<Choice>>) -> Ticket {
        let payload = PopoverPayload { lang: self.settings().lang(), ..payload };
        let anchor = anchor.unwrap_or_else(|| below_mouse(ui::cursor(&self.app)));
        let (ticket, closed, size) = {
            let mut p = self.popover.lock().unwrap();
            let (ticket, closed) = p.open(payload.clone(), anchor, waiter);
            (ticket, closed, p.size())
        };
        self.popover_closed(closed, Choice::Later);
        ui::resize(&self.app, "popover", size.0, size.1);
        let _ = self.app.emit_to("popover", "popover:show", payload);
        self.place_popover();
        // Keep the target app focused so write-back lands there.
        ui::show_panel(&self.app, "popover");
        ticket
    }

    pub fn popover_resize(&self, h: f64, w: Option<f64>) {
        let Some(size) = self.popover.lock().unwrap().resize(h, w) else { return };
        ui::resize(&self.app, "popover", size.0, size.1);
        self.place_popover();
    }

    /// Shows a fix; the receiver resolves with the user's choice. `esc`: also take Esc globally; auto-check popups leave it
    /// to the app being typed in.
    fn open_diff(&self, p: PopoverPayload, anchor: Option<Anchor>, esc: bool) -> (Ticket, oneshot::Receiver<Choice>) {
        let (tx, rx) = oneshot::channel();
        let ticket = self.open_popover(PopoverPayload { esc, ..p }, anchor, Some(tx));
        let gs = self.app.global_shortcut();
        let _ = gs.register(accept_shortcut());
        if esc {
            let _ = gs.register(dismiss_shortcut());
        }
        (ticket, rx)
    }

    pub fn show_message(&'static self, message: &str, anchor: Option<Anchor>) {
        let ticket = self.open_popover(PopoverPayload::text(PopoverKind::Message, message), anchor, None);
        tauri::async_runtime::spawn(async move {
            tokio::time::sleep(Duration::from_millis(2200)).await;
            let closed = {
                let mut p = self.popover.lock().unwrap();
                p.is_current(ticket).then(|| p.close())
            };
            if let Some(c) = closed {
                self.popover_closed(c, Choice::Dismiss);
            }
        });
    }

    fn show_loading(&self, anchor: Option<Anchor>) {
        self.open_popover(PopoverPayload::text(PopoverKind::Loading, self.settings().strings().checking), anchor, None);
    }

    // ---------- Flow ----------

    /// Under the selection, else under the focused field; None leaves the popover at the mouse.
    async fn selection_anchor(&self) -> Option<Anchor> {
        let c = self.helper.caret(&self.settings().excluded_apps).await;
        debug(&format!("anchor {}", c.map_or("mouse", |c| if c.precise { "selection" } else { "field" })));
        Some(below_rect(c?.rect))
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
        if let Err(e) = self.helper.set_hotkey(HotkeySlot::Fix, &h).await {
            self.notify(&fill(self.settings().strings().hotkey_failed, &[("error", &e.to_string())]));
        }
        // Tone menu: fixed ⌥⌘T. Failure (key in use) is not fatal.
        let tone = Hotkey { key: "T".into(), mods: vec![Mod::Alt, Mod::Cmd] };
        let _ = self.helper.set_hotkey(HotkeySlot::Tone, &tone).await;
    }

    /// Tone hotkey: pick Natural / Formal / Friendly / Short / Translate from a menu over the app being written in.
    pub fn on_hotkey2(&self) {
        self.close_popover(Choice::Later);
        let (a, t) = (&self.app, self.settings().strings());
        let menu = (|| {
            Menu::with_items(
                a,
                &[
                    &MenuItem::with_id(a, "tone_title", t.rewrite_as, false, None::<&str>)?,
                    &MenuItem::with_id(a, "tone_natural", t.natural, true, None::<&str>)?,
                    &MenuItem::with_id(a, "tone_formal", t.formal, true, None::<&str>)?,
                    &MenuItem::with_id(a, "tone_friendly", t.friendly, true, None::<&str>)?,
                    &MenuItem::with_id(a, "tone_short", t.short, true, None::<&str>)?,
                    &MenuItem::with_id(a, "tone_translate", t.translate, true, None::<&str>)?,
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
                HelperEvent::Hotkey(HotkeySlot::Fix) => {
                    tauri::async_runtime::spawn(self.manual_check("hotkey"));
                }
                HelperEvent::Hotkey(HotkeySlot::Tone) => {
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

    fn now(&self) -> u64 {
        self.started.elapsed().as_millis() as u64
    }

    async fn auto_tick(&'static self) {
        let s = self.settings();
        if !s.auto_check || s.paused {
            return self.auto.lock().unwrap().reset();
        }
        let snap = self.helper.snapshot(&s.excluded_apps, &[]).await;
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
                    let mine = self.popover.lock().unwrap().release_auto(id);
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
        self.check_text(job.sentence.clone(), None, Purpose::Background).await
    }

    async fn sentence_anchor(&self, f: &Found) -> Anchor {
        let last = f.start + utf16_len(&f.sentence).saturating_sub(1);
        let snap = self.helper.snapshot(&self.settings().excluded_apps, &[(f.start, 1), (last, 1)]).await;
        let (anchor, why) = placement::sentence_anchor(&snap, f.pid, &ui::screens(&self.app), ui::cursor(&self.app));
        debug(&format!("auto anchor {why}"));
        anchor
    }

    /// Writes an accepted fix over its sentence and records it; says why when it cannot.
    async fn write_fix(&'static self, f: &Found) -> bool {
        let edit = RangeEdit { loc: f.start, len: utf16_len(&f.sentence), text: &f.corrected, expect: &f.sentence, pid: f.pid };
        let r = self.helper.replace_range(edit).await;
        debug(&format!("auto write {}", outcome(&r, |m| m.as_deref())));
        if r.is_ok() {
            self.insights.lock().unwrap().record(&f.sentence, &f.corrected, &f.notes);
        } else {
            self.show_message(&fill(self.settings().strings().auto_write_failed, &[("hotkey", &self.hotkey_label())]), None);
        }
        r.is_ok()
    }

    /// The sentence's fix with reasons. ⌘↩ applies; Esc stays with the app being typed in. Any popover up (a manual
    /// check, its message, another fix) keeps it away.
    async fn open_sentence_popup(&'static self, f: Found) {
        if !self.popover.lock().unwrap().claim_auto(f.id) {
            return self.auto_resolve(f.id, Outcome::Later);
        }
        let anchor = self.sentence_anchor(&f).await;
        if !self.popover.lock().unwrap().holds_auto(f.id) {
            return; // the user moved on meanwhile; the fix is in the list
        }
        let (ticket, rx) = self.open_diff(PopoverPayload::diff(&f.sentence, &f.corrected, f.app_name.clone(), f.notes.clone()), Some(anchor), false);
        tauri::async_runtime::spawn(self.expire_popup(ticket));
        let outcome = match rx.await.unwrap_or(Choice::Dismiss) {
            Choice::Accept if self.write_fix(&f).await => Outcome::Applied,
            Choice::Dismiss => Outcome::Dismissed,
            _ => Outcome::Later,
        };
        self.popover.lock().unwrap().release_auto(f.id);
        self.auto_resolve(f.id, outcome);
    }

    /// An untouched popup closes into the menu-bar list; the wait restarts while the mouse is over it.
    async fn expire_popup(&self, ticket: Ticket) {
        loop {
            tokio::time::sleep(POPUP_IDLE).await;
            let closed = {
                let mut p = self.popover.lock().unwrap();
                match p.expiry(ticket, ui::cursor(&self.app)) {
                    Expiry::Stale => return,
                    Expiry::Keep => None,
                    Expiry::Close => Some(p.close()),
                }
            };
            if let Some(c) = closed {
                return self.popover_closed(c, Choice::Later);
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
            self.helper.activate(pid).await;
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
        let snap = self.helper.snapshot(&self.settings().excluded_apps, &[]).await;
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
        self.notify(&fill(self.settings().strings().update_installed, &[("version", &v)]));
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
        self.set_status(resting(&self.settings()));
        let trusted = self.check_permissions().await[0].granted;
        self.register_hotkey().await;
        if self.settings().language.is_none() {
            let lang = self.language_dialog().await;
            // A fresh install also explains fixes in that language.
            let explain = (!self.settings().first_run_acknowledged).then(|| lang.explain_lang());
            let mut patch = json!({ "language": lang });
            if let Some(e) = explain {
                patch["explainLang"] = json!(e);
            }
            self.update_settings(patch);
        }
        if !self.settings().first_run_acknowledged {
            self.first_run_dialog().await;
            self.update_settings(json!({ "firstRunAcknowledged": true }));
        }
        if !trusted {
            self.set_status(Status { kind: StatusKind::NeedsPermission, message: Some(self.settings().strings().grant_ax.into()) });
            // The checklist explains why and has the button; firing the native prompt unasked on
            // every launch (the old behavior) is the nagging this screen replaces.
            self.open_panel_on_permissions().await;
        }
    }

    /// Asked in both languages, before anything else is shown.
    async fn language_dialog(&self) -> Lang {
        use tauri_plugin_dialog::{DialogExt, MessageDialogButtons, MessageDialogKind};
        let (tx, rx) = oneshot::channel();
        self.app
            .dialog()
            .message("Choose the language for ugrammar. You can change it later in Settings.\n\nChọn ngôn ngữ cho ugrammar. Có thể đổi sau trong Cài đặt.")
            .title("Language · Ngôn ngữ")
            .kind(MessageDialogKind::Info)
            .buttons(MessageDialogButtons::OkCancelCustom("English".into(), "Tiếng Việt".into()))
            .show(move |english| {
                let _ = tx.send(if english { Lang::En } else { Lang::Vi });
            });
        rx.await.unwrap_or(Lang::En)
    }

    async fn first_run_dialog(&self) {
        use tauri_plugin_dialog::{DialogExt, MessageDialogButtons, MessageDialogKind};
        let cfg = self.settings();
        let t = cfg.strings();
        let detail = fill(t.welcome_body, &[("hotkey", &self.hotkey_label()), ("endpoint", &cfg.endpoint)]);
        let (tx, rx) = oneshot::channel();
        self.app
            .dialog()
            .message(detail)
            .title(t.welcome_title)
            .kind(MessageDialogKind::Info)
            .buttons(MessageDialogButtons::OkCustom(t.get_started.into()))
            .show(move |_| {
                let _ = tx.send(());
            });
        let _ = rx.await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stale_only_when_lost_after_being_granted() {
        assert!(!permission_is_stale(true, false));
        assert!(!permission_is_stale(true, true));
        assert!(!permission_is_stale(false, false));
        assert!(permission_is_stale(false, true));
        assert_eq!(bundle_of(Path::new("/Applications/ugrammar.app/Contents/MacOS/ugrammar")), Path::new("/Applications/ugrammar.app"));
        assert_eq!(bundle_of(Path::new("/repo/target/debug/ugrammar")), Path::new("/repo/target/debug/ugrammar"));
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

    async fn read_selection(&self, excluded: &[String]) -> Result<Selection, HelperError> {
        let r = self.helper.read(excluded).await;
        let app = r.as_ref().ok().and_then(|s| s.bundle_id.as_deref()).unwrap_or("-");
        debug(&format!("read app={app} {}", outcome(&r, |s| s.method.as_deref())));
        r
    }

    async fn write_selection(&self, text: &str, pid: Option<i64>, method: Option<&str>) -> Result<(), HelperError> {
        let r = self.helper.write(text, pid, method).await;
        debug(&format!("write {}", outcome(&r, |m| m.as_deref())));
        r.map(drop)
    }

    async fn copy_text(&self, text: &str) -> Result<(), HelperError> {
        let r = self.helper.copy(text).await;
        debug(&format!("copy {}", outcome(&r, |_| None))); // never the text
        r
    }

    async fn check(&self, text: &str, tone: Option<Tone>) -> Result<(String, Vec<String>), String> {
        self.check_text(text.into(), tone, Purpose::Hotkey).await
    }

    fn set_status(&self, s: Status) {
        ctx().set_status(s);
    }

    fn notify(&self, message: &str) {
        Ctx::notify(self, message);
    }

    async fn show_diff(&self, p: PopoverPayload) -> Choice {
        let (_, rx) = self.open_diff(p, self.flow_anchor(), true);
        rx.await.unwrap_or(Choice::Dismiss)
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

    fn note_accessibility_not_trusted(&self) {
        *ctx().permission_trusted.lock().unwrap() = false;
    }
}
