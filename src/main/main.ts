import {
  app, BrowserWindow, dialog, globalShortcut, ipcMain, Menu, nativeImage, nativeTheme, Notification, screen, shell, Tray,
} from "electron";
import { execFile } from "node:child_process";
import { appendFileSync, cpSync, existsSync } from "node:fs";
import { join } from "node:path";
import { OpenAICompatibleEngine } from "./engine/openai";
import { FixFlow, type Status } from "./flow";
import { HelperClient, type HelperReply, type SelectionReply } from "./helper";
import { SettingsStore } from "./settings";
import { TypingBadge } from "./badge";
import { AutoChecker, type Issue, type SentenceFix, type Snapshot } from "./autocheck";
import { UnderlineOverlay } from "./underline";
import { InsightStore } from "./insights";
import { diffWords } from "../shared/diff";
import type { Tone } from "./engine/types";
import type { PopoverPayload, SettingsView, TestResult, TrayAction, TrayState } from "../shared/ipc";

const APP_NAME = "ugrammar";
const KEY_SERVICE = "Proofy";
const KEY_ACCOUNT = "api-key";

app.setName(APP_NAME);
if (process.env.UGRAMMAR_THEME === "dark" || process.env.UGRAMMAR_THEME === "light") nativeTheme.themeSource = process.env.UGRAMMAR_THEME;
if (!app.requestSingleInstanceLock()) app.quit();

const root = join(import.meta.dirname, "..");
const helperPath = app.isPackaged
  ? join(process.resourcesPath, "grammar-helper")
  : join(root, "helper/.build/release/grammar-helper");

// Renamed from Proofy: carry settings over once.
const settingsDir = join(app.getPath("appData"), APP_NAME);
const legacyDir = join(app.getPath("appData"), "Proofy");
if (!existsSync(settingsDir) && existsSync(legacyDir)) cpSync(legacyDir, settingsDir, { recursive: true });
const store = new SettingsStore(settingsDir);
const helper = new HelperClient(helperPath);
const insights = new InsightStore(settingsDir);

let tray: Tray | null = null;
let status: Status = { kind: "idle" };
let popover: BrowserWindow | null = null;
let popoverResolve: ((c: "accept" | "dismiss") => void) | null = null;
let messageTimer: ReturnType<typeof setTimeout> | null = null;

// ---------- Keychain ----------
async function getApiKey(): Promise<string | null> {
  const r = await helper.request<{ ok: boolean; value?: string | null }>("keychain_get", { service: KEY_SERVICE, account: KEY_ACCOUNT });
  return r.ok ? (r.value ?? null) : null;
}

async function makeEngine(minTimeoutMs = 0) {
  const s = store.get();
  return new OpenAICompatibleEngine({ endpoint: s.endpoint, model: s.model, timeoutMs: Math.max(s.timeoutMs, minTimeoutMs) }, { apiKey: await getApiKey() });
}

// ---------- Tray ----------
const STATUS_LABEL: Record<Status["kind"], string> = {
  idle: "Ready", checking: "Checking…", paused: "Paused", error: "Error", needs_permission: "Needs Accessibility permission",
};
const STATUS_TITLE: Record<Status["kind"], string> = { idle: "ug", checking: "ug…", paused: "ug⏸", error: "ug!", needs_permission: "ug?" };

const hotkeyLabel = () => {
  const { mods, key } = store.get().hotkey;
  return `${mods.map((m) => ({ cmd: "⌘", alt: "⌥", ctrl: "⌃", shift: "⇧" })[m]).join("")}${key}`;
};

/** Right-click fallback: the plain native menu. */
function trayMenu() {
  const s = store.get();
  return Menu.buildFromTemplate([
    { label: `Status: ${STATUS_LABEL[status.kind]}${status.message ? ` — ${status.message}` : ""}`, enabled: false },
    { label: `Fix selection: ${hotkeyLabel()}`, enabled: false },
    { type: "separator" },
    { label: "Pause", type: "checkbox", checked: s.paused, click: (i) => store.update({ paused: i.checked }) },
    ...(status.kind === "needs_permission"
      ? [{ label: "Open Accessibility Settings…", click: openAXSettings }]
      : []),
    { label: "Settings…", click: () => void openPanel() },
    { type: "separator" },
    { label: `Quit ${APP_NAME}`, accelerator: "Cmd+Q", click: () => app.quit() },
  ]);
}

function refreshTray() {
  if (!tray) return;
  tray.setTitle(STATUS_TITLE[status.kind]);
  tray.setToolTip(`${APP_NAME} — ${STATUS_LABEL[status.kind]}`);
  if (trayWin && !trayWin.isDestroyed() && trayWin.isVisible()) void trayState().then((st) => trayWin?.webContents.send("tray:state", st));
}

// ---------- Tray panel ----------
const TRAY_W = 344;
const TRAY_MAX_H = 600;
let trayWin: BrowserWindow | null = null;
let trayHiddenAt = 0;
let trayPinned = false; // a native dialog over the panel must not blur it closed

const trayState = async (): Promise<TrayState> => ({
  status: status.kind, message: status.message, settings: store.get(), hotkey: hotkeyLabel(), insights: insights.top(5),
  hasApiKey: !!(await getApiKey()),
});

function ensureTrayWin(): BrowserWindow {
  if (trayWin && !trayWin.isDestroyed()) return trayWin;
  trayWin = new BrowserWindow({
    width: TRAY_W, height: 520, show: false, frame: false, resizable: false, movable: false, fullscreenable: false,
    alwaysOnTop: true, skipTaskbar: true, hasShadow: true, transparent: true,
    webPreferences: { preload: join(root, "dist/preload.cjs"), sandbox: true, contextIsolation: true },
  });
  trayWin.setVisibleOnAllWorkspaces(true, { visibleOnFullScreen: true });
  trayWin.on("blur", () => trayPinned || hideTrayWin());
  trayWin.loadFile(join(root, "dist/renderer/tray.html"));
  return trayWin;
}

function hideTrayWin() {
  if (!trayWin || trayWin.isDestroyed() || !trayWin.isVisible()) return;
  trayWin.hide();
  trayHiddenAt = Date.now();
}

function placeTrayWin(win: BrowserWindow) {
  if (!tray) return;
  const b = tray.getBounds();
  const area = screen.getDisplayNearestPoint({ x: b.x, y: b.y }).workArea;
  const [w] = win.getSize();
  const x = Math.min(Math.max(b.x + b.width / 2 - w / 2, area.x + 8), area.x + area.width - w - 8);
  win.setPosition(Math.round(x), Math.round(area.y + 6));
}

async function toggleTrayWin() {
  if (trayWin?.isVisible()) return hideTrayWin();
  if (Date.now() - trayHiddenAt < 250) return; // this click is what blurred it closed
  await openPanel();
}

async function openPanel() {
  const win = ensureTrayWin();
  if (win.webContents.isLoading()) await new Promise<void>((r) => win.webContents.once("did-finish-load", () => r()));
  win.webContents.send("tray:state", await trayState());
  placeTrayWin(win);
  win.show();
  win.focus();
}

ipcMain.handle("tray:get", trayState);
ipcMain.on("tray:resize", (_e, h: number) => {
  if (!trayWin || trayWin.isDestroyed() || h <= 0) return;
  const area = screen.getDisplayMatching(trayWin.getBounds()).workArea;
  trayWin.setSize(TRAY_W, Math.min(Math.ceil(h), TRAY_MAX_H, area.height - 12)); // taller content scrolls inside
});
ipcMain.on("tray:action", async (_e, a: TrayAction) => {
  hideTrayWin();
  if (a === "quit") return app.quit();
  if (a === "ax") { await helper.request("prompt_ax"); return openAXSettings(); }
  // Tone: hand focus back to the app the user was writing in, then rewrite its selection.
  app.hide();
  await new Promise((r) => setTimeout(r, 200));
  debug(`tone=${a} outcome=${await runFlow({ tone: a })}`);
});

const badge = new TypingBadge(root, () => void onBadgeClick(), async () => {
  const r = await helper.request<any>("caret", { excluded: store.get().excludedApps }, 1500);
  return r.ok ? r : null;
});

function setStatus(s: Status) {
  status = s.kind === "idle" && store.get().paused ? { kind: "paused" } : s;
  refreshTray();
  if (status.kind === "paused" || status.kind === "needs_permission" || !store.get().showBadge) badge.stop();
  else { badge.start(); badge.setState(status.kind === "checking" ? "checking" : "idle"); }
}

function notify(body: string) {
  if (Notification.isSupported()) new Notification({ title: APP_NAME, body }).show();
}

function openAXSettings() {
  shell.openExternal("x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility");
}

// ---------- Popover ----------
function closePopover(choice: "accept" | "dismiss" = "dismiss") {
  if (messageTimer) clearTimeout(messageTimer);
  messageTimer = null;
  globalShortcut.unregister("CommandOrControl+Return");
  globalShortcut.unregister("Escape");
  if (popover && !popover.isDestroyed()) popover.hide();
  const r = popoverResolve;
  popoverResolve = null;
  r?.(choice);
}

const POPOVER_W = 460;  function ensurePopover(): BrowserWindow {
  if (popover && !popover.isDestroyed()) return popover;
  popover = new BrowserWindow({
    width: POPOVER_W, height: 200, show: false, frame: false, resizable: false, movable: true,
    alwaysOnTop: true, skipTaskbar: true, hasShadow: true, transparent: true, type: "panel", focusable: false,
    webPreferences: { preload: join(root, "dist/preload.cjs"), sandbox: true, contextIsolation: true },
  });
  popover.setAlwaysOnTop(true, "screen-saver");
  popover.setVisibleOnAllWorkspaces(true, { visibleOnFullScreen: true });
  popover.loadFile(join(root, "dist/renderer/popover.html"));
  return popover;
}

/** `y` = top edge when placed below, `altY` = bottom edge when placed above. */
interface Anchor { x: number; y: number; altY: number }
let popoverAnchor: Anchor | null = null;

type Rect = { x: number; y: number; w: number; h: number };
const belowRect = (r: Rect): Anchor => ({ x: r.x + r.w / 2, y: r.y + r.h + 6, altY: r.y - 6 });

function cursorAnchor(): Anchor {
  const pt = screen.getCursorScreenPoint();
  return { x: pt.x, y: pt.y + 20, altY: pt.y - 36 };
}

/** Below the anchor if it fits, else above it; always clamped inside the display. Re-run after each resize. */
function placePopover(win: BrowserWindow, fresh?: Anchor | true) {
  if (fresh) popoverAnchor = fresh === true ? cursorAnchor() : fresh;
  else if (!popoverAnchor) popoverAnchor = cursorAnchor();
  const a = popoverAnchor;
  const area = screen.getDisplayNearestPoint({ x: a.x, y: a.y }).workArea;
  const [w, h] = win.getSize();
  const x = Math.min(Math.max(a.x - w / 2, area.x + 8), area.x + area.width - w - 8);
  const y = a.y + h <= area.y + area.height - 8 ? a.y : a.altY - h;
  win.setPosition(Math.round(x), Math.round(Math.min(Math.max(y, area.y + 8), area.y + area.height - h - 8)));
}

async function showPopover(p: PopoverPayload, anchor?: Anchor) {
  const win = ensurePopover();
  if (win.webContents.isLoading()) await new Promise<void>((r) => win.webContents.once("did-finish-load", () => r()));
  win.setSize(POPOVER_W, win.getSize()[1]); // measure at full width; a pill shrinks it back
  win.webContents.send("popover:show", p);
  placePopover(win, anchor ?? true);
  win.showInactive(); // keep the target app focused so write-back lands there
}

function showDiff(p: PopoverPayload, anchor?: Anchor): Promise<"accept" | "dismiss"> {
  closePopover();
  return new Promise((resolve) => {
    popoverResolve = resolve;
    void showPopover(p, anchor);
    globalShortcut.register("CommandOrControl+Return", () => closePopover("accept"));
    globalShortcut.register("Escape", () => closePopover("dismiss"));
  });
}

function showMessage(message: string, anchor?: Anchor) {
  closePopover();
  void showPopover({ kind: "message", message }, anchor);
  messageTimer = setTimeout(() => closePopover(), 2200);
}

function showLoading(anchor?: Anchor) {
  closePopover();
  void showPopover({ kind: "loading", message: "Checking…" }, anchor);
}

ipcMain.on("popover:action", (_e, a: "accept" | "dismiss") => closePopover(a));
ipcMain.on("popover:resize", (_e, h: number, w?: number) => {
  if (popover && !popover.isDestroyed() && h > 0) {
    popover.setSize(w ? Math.min(Math.ceil(w), POPOVER_W) : POPOVER_W, Math.min(Math.ceil(h), 600));
    placePopover(popover);
  }
});

// ---------- Flow ----------
// Opt-in diagnostics (PROOFY_DEBUG=1): outcomes only, never text content.
const debug = (msg: string) => { if (process.env.PROOFY_DEBUG) appendFileSync("/tmp/proofy-debug.log", `${new Date().toISOString()} ${msg}\n`); };

const flow = new FixFlow({
  settings: () => store.get(),
  readSelection: async (excluded, selectAll) => {
    const r = await helper.request<SelectionReply>("read", { excluded, selectAll }, 8000);
    debug(`read app=${r.bundleId} ok=${r.ok} method=${r.method ?? "-"} err=${r.error ?? "-"}`);
    return r;
  },
  writeSelection: async (text, pid, method) => {
    const r = await helper.request("write", { text, pid, method }, 8000);
    debug(`write ok=${r.ok} method=${r.method ?? "-"} err=${r.error ?? "-"}`);
    return r;
  },
  makeEngine,
  setStatus,
  notify,
  showDiff: (p) => showDiff(p, flowAnchor),
  showMessage: (m) => showMessage(m, flowAnchor),
  showLoading: () => { loadingSeq++; loadingShown = false; showLoading(flowAnchor); }, // the manual check owns the popover now
  hideLoading: () => closePopover(),
  onApplied: (o, c, n) => insights.record(o, c, n),
});

let flowAnchor: Anchor | undefined;

/** Under the selection, else under the focused field; undefined leaves the popover at the mouse. */
async function selectionAnchor(): Promise<Anchor | undefined> {
  const r = await helper.request<HelperReply & Partial<Rect & { visible: boolean; precise: boolean }>>("caret", { excluded: store.get().excludedApps, deep: true }, 1500);
  debug(`anchor ${!r.ok || !r.visible ? "mouse" : r.precise ? "selection" : "field"}`);
  return r.ok && r.visible && r.x !== undefined && r.y !== undefined && r.w !== undefined && r.h !== undefined ? belowRect({ x: r.x, y: r.y, w: r.w, h: r.h }) : undefined;
}

/** Manual checks: popovers sit by the text being fixed. */
async function runFlow(opts?: Parameters<FixFlow["run"]>[0]) {
  flowAnchor = await selectionAnchor();
  try {
    return await flow.run(opts);
  } finally {
    flowAnchor = undefined;
  }
}

// ---------- Auto-check (underline in place + small card) ----------
let cardOpen = false;
const underlines = new UnderlineOverlay(root, (id) => void openIssueCard(id));
const auto = new AutoChecker({
  settings: () => store.get(),
  snapshot: (excluded, ranges) => helper.request<Snapshot>("snapshot", { excluded, ranges }, 3000),
  makeEngine: () => makeEngine(30000), // background: slow gateways need more than the 10s hotkey default
  log: (m) => debug(`auto ${m}`),
  onMarks: (marks) => (cardOpen ? undefined : underlines.set(marks)),
  onSentence: (fix) => void openSentencePopup(fix),
  onLoading: (at) => void autoLoading(at),
});

let loadingSeq = 0;
let loadingShown = false;

/** Popup mode: "Checking…" under the sentence; never over a card or a manual check. */
async function autoLoading(at: { start: number; length: number } | null) {
  const seq = ++loadingSeq;
  if (!at) {
    if (loadingShown && !popoverResolve) closePopover();
    loadingShown = false;
    return;
  }
  if (cardOpen || popoverResolve) return;
  const anchor = await rangeAnchor(at.start + Math.max(0, at.length - 1), Math.min(1, at.length)); // sentence end, by the caret
  if (seq !== loadingSeq || cardOpen || popoverResolve) return; // hidden or replaced meanwhile
  loadingShown = true;
  showLoading(anchor);
}

/** Anchor below a text range (above when there is no room), from fresh AX bounds. */
async function rangeAnchor(loc: number, len: number, known?: { x: number; y: number; w: number; h: number }) {
  let r = known ?? null;
  if (!r) {
    const snap = await helper.request<Snapshot>("snapshot", { excluded: store.get().excludedApps, ranges: [{ loc, len }] }, 3000);
    r = snap.ok && snap.visible ? snap.bounds?.[0] ?? null : null;
  }
  return r ? belowRect(r) : undefined;
}

const issueAnchor = (it: Issue) => rangeAnchor(it.start, it.end - it.start, underlines.bounds(it.id));

/** Popup mode: the whole sentence's fix with reasons, opened as soon as the check comes back. */
async function openSentencePopup(fix: SentenceFix) {
  const it = auto.issues.find((i) => i.id === fix.firstId);
  if (!it || cardOpen || popoverResolve) return; // never cover a card or a manual check
  cardOpen = true;
  loadingSeq++; // a pending hint must not land over the popup
  loadingShown = false;
  try {
    const choice = await showDiff({
      kind: "diff", original: fix.original, corrected: fix.corrected, ops: diffWords(fix.original, fix.corrected),
      appName: auto.appName, notes: fix.notes.length ? fix.notes : undefined,
    }, await issueAnchor(it));
    if (choice === "dismiss") return auto.resolveSentence(fix.start, fix.original.length);
    const w = await helper.request("replace_range", { loc: fix.start, len: fix.original.length, text: fix.corrected, expect: fix.original, pid: auto.pid }, 5000);
    debug(`auto popup write ok=${w.ok} method=${w.method ?? "-"} err=${w.error ?? "-"}`);
    auto.resolveSentence(fix.start, fix.original.length, w.ok ? fix.corrected : undefined);
    if (w.ok) insights.record(fix.original, fix.corrected, fix.notes);
    else notify(w.error === "stale" ? "Text changed; fix skipped." : `Could not replace (${w.error}). Try the hotkey instead.`);
  } finally {
    cardOpen = false;
  }
}

async function openIssueCard(id: number) {
  const it: Issue | undefined = auto.issues.find((i) => i.id === id);
  if (!it || cardOpen) return;
  cardOpen = true;
  try {
    const anchor = await issueAnchor(it);
    const ops = diffWords(it.original, it.replacement);
    const choice = await showDiff({
      kind: "diff", original: it.original, corrected: it.replacement, ops, appName: auto.appName,
      notes: it.reason ? [it.reason] : undefined,
    }, anchor);
    if (choice === "dismiss") return auto.dismiss(id);
    const w = await helper.request("replace_range", { loc: it.start, len: it.end - it.start, text: it.replacement, expect: it.original, pid: auto.pid }, 5000);
    if (w.ok) { insights.record(it.original, it.replacement, it.reason ? [it.reason] : []); auto.applied(id); }
    else {
      auto.dismiss(id);
      notify(w.error === "stale" ? "Text changed; fix skipped." : `Could not replace (${w.error}). Try the hotkey instead.`);
    }
  } finally {
    cardOpen = false;
  }
}

let resumeTimer: NodeJS.Timeout | null = null;

badge.onMenu = (bundleId) => {
  Menu.buildFromTemplate([
    { label: "Ẩn ở app này", enabled: !!bundleId, click: () => store.addExcludedApp(bundleId) },
    {
      label: "Tạm dừng 1 giờ",
      click: () => {
        store.update({ paused: true });
        if (resumeTimer) clearTimeout(resumeTimer);
        resumeTimer = setTimeout(() => store.update({ paused: false }), 3600_000);
      },
    },
    { type: "separator" },
    { label: "Settings…", click: () => void openPanel() },
  ]).popup();
};

async function onBadgeClick() {
  debug("badge click");
  closePopover();
  await new Promise((r) => setTimeout(r, 0));
  debug(`outcome=${await runFlow({ selectAll: true })} status=${status.kind}`);
}

async function onHotkey() {
  debug("hotkey");
  closePopover(); // a still-open popover counts as dismissed
  await new Promise((r) => setTimeout(r, 0));
  debug(`outcome=${await runFlow()} status=${status.kind}${status.message ? ` (${status.message})` : ""}`);
}

async function registerHotkey() {
  const { key, mods } = store.get().hotkey;
  const r = await helper.request("set_hotkey", { key, mods });
  if (!r.ok) notify(`Could not register hotkey (${r.error}). It may be in use by another app.`);
  // Tone menu: fixed ⌥⌘T. Failure (key in use) is not fatal.
  await helper.request("set_hotkey", { key: "T", mods: ["alt", "cmd"], slot: 2 });
}

/** Tone hotkey: remember the target app, then pick Formal / Friendly / Short from a menu. */
async function onHotkey2() {
  closePopover();
  const pick = (tone: Tone) => async () => {
    debug(`tone=${tone} outcome=${await runFlow({ tone })}`);
  };
  Menu.buildFromTemplate([
    { label: "Rewrite selection as…", enabled: false },
    { label: "Formal", click: pick("formal") },
    { label: "Friendly", click: pick("friendly") },
    { label: "Short", click: pick("short") },
  ]).popup();
}

// ---------- Settings window & IPC ----------
async function settingsView(): Promise<SettingsView> {
  const [key, st] = await Promise.all([getApiKey(), helper.request<{ ok: boolean; trusted?: boolean }>("status")]);
  return { settings: store.get(), hasApiKey: !!key, axTrusted: !!st.trusted, insights: insights.top(10) };
}

ipcMain.handle("settings:clearInsights", () => { insights.clear(); refreshTray(); return settingsView(); });
ipcMain.handle("settings:save", async (_e, patch) => {
  const before = store.get().hotkey;
  const after = store.update(patch).hotkey;
  if (JSON.stringify(before) !== JSON.stringify(after)) await registerHotkey();
  refreshTray();
  return settingsView();
});
ipcMain.handle("settings:setKey", async (_e, key: string) => {
  await helper.request(key ? "keychain_set" : "keychain_delete", { service: KEY_SERVICE, account: KEY_ACCOUNT, value: key });
  refreshTray();
  return settingsView();
});
ipcMain.handle("settings:test", async (_e, d: { endpoint: string; model: string; apiKey?: string }): Promise<TestResult> => {
  const apiKey = d.apiKey || (await getApiKey());
  const engine = new OpenAICompatibleEngine(
    { endpoint: d.endpoint.trim() || store.get().endpoint, model: d.model.trim() || store.get().model, timeoutMs: store.get().timeoutMs },
    { apiKey },
  );
  const r = await engine.testConnection();
  if (r.ok) setStatus({ kind: "idle" });
  return r;
});
ipcMain.handle("settings:pickApp", async () => {
  trayPinned = true;
  const r = await dialog.showOpenDialog({ defaultPath: "/Applications", properties: ["openFile"], filters: [{ name: "Applications", extensions: ["app"] }] })
    .finally(() => { trayPinned = false; trayWin?.focus(); });
  if (r.canceled || !r.filePaths[0]) return null;
  return new Promise<string | null>((resolve) =>
    execFile("/usr/bin/mdls", ["-raw", "-name", "kMDItemCFBundleIdentifier", r.filePaths[0]], (err, out) =>
      resolve(err || !out || out === "(null)" ? null : out.trim())));
});
ipcMain.handle("settings:openAX", async () => {
  await helper.request("prompt_ax");
  openAXSettings();
});

// ---------- Lifecycle ----------
app.whenReady().then(async () => {
  app.dock?.hide();
  tray = new Tray(nativeImage.createEmpty());
  tray.on("click", () => void toggleTrayWin());
  tray.on("right-click", () => tray?.popUpContextMenu(trayMenu()));
  helper.onHotkey = () => void onHotkey();
  helper.onHotkey2 = () => void onHotkey2();
  helper.onRestart = () => void registerHotkey();
  helper.start();
  store.onChange((cfg) => {
    setStatus(status.kind === "paused" || status.kind === "idle" ? { kind: "idle" } : status);
    if (cfg.autoCheck && !cfg.paused) auto.start(); else auto.stop();
  });
  if (store.get().autoCheck) auto.start();
  setStatus({ kind: "idle" });

  const st = await helper.request<{ ok: boolean; trusted?: boolean }>("status");
  await registerHotkey();

  if (!store.get().firstRunAcknowledged) {
    await dialog.showMessageBox({
      type: "info",
      message: `Welcome to ${APP_NAME}`,
      detail:
        "1. Grant Accessibility permission when macOS asks (System Settings → Privacy & Security → Accessibility). Needed to read and replace text.\n" +
        `2. Select text and press ${hotkeyLabel()} to proofread. ⌥⌘T rewrites in a tone.\n` +
        "3. Click the ug icon in the menu bar for everything else: auto-check, tone, engine, dictionary.\n\n" +
        `Privacy: checked text is sent to ${store.get().endpoint} (change it from the ug menu-bar panel). Password fields and excluded apps are never read. Text is never logged or stored.`,
      buttons: ["Get started"],
    });
    store.update({ firstRunAcknowledged: true });
  }
  if (!st.trusted) {
    setStatus({ kind: "needs_permission", message: "Grant Accessibility permission" });
    await helper.request("prompt_ax");
  }
});

app.on("window-all-closed", () => {}); // menu-bar app: stay alive
app.on("before-quit", () => { globalShortcut.unregisterAll(); helper.stop(); });
app.on("second-instance", () => void openPanel());
