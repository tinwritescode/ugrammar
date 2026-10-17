// Marketing screenshot renderer: `bun run shots`. Renders the real popover/tray panel/badge renderer code
// with sample data and saves dark-mode PNGs @2x to marketing/. No Accessibility or Screen Recording needed.
import { app, BrowserWindow, ipcMain, nativeTheme } from "electron";
import { mkdirSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { diffWords } from "../shared/diff";
import type { PopoverPayload, SettingsView, TrayState } from "../shared/ipc";
import { BADGE_PAGE } from "./badge";
import { DEFAULT_SETTINGS } from "./settings";

const root = join(import.meta.dirname, "..");
const out = join(root, "marketing");
mkdirSync(out, { recursive: true });
nativeTheme.themeSource = "dark";
app.dock?.hide();

const ORIGINAL = "Hi team, I has attached the report. Please to review it before monday.";
const CORRECTED = "Hi team, I have attached the report. Please review it before Monday.";
const diff: PopoverPayload = {
  kind: "diff", original: ORIGINAL, corrected: CORRECTED, ops: diffWords(ORIGINAL, CORRECTED), appName: "Mail",
  notes: [
    "has → have: chủ ngữ “I” đi với “have”.",
    "Please to review → Please review: sau “Please” dùng động từ nguyên mẫu, không có “to”.",
    "monday → Monday: tên thứ trong tuần phải viết hoa.",
  ],
};
const settingsView: SettingsView = {
  settings: { ...DEFAULT_SETTINGS, endpoint: "http://localhost:11434/v1", model: "llama3.1:8b", dictionary: ["Xpiki", "Vercel", "Supabase"] },
  hasApiKey: false, axTrusted: true, insights: [{ from: "has", to: "have", count: 3, note: "has → have: subject-verb agreement" }],
};

ipcMain.handle("settings:save", () => settingsView);
ipcMain.handle("settings:setKey", () => settingsView);
ipcMain.handle("settings:test", () => ({ ok: true, message: "Connected (llama3.1:8b)" }));
ipcMain.handle("settings:pickApp", () => null);
ipcMain.handle("settings:openAX", () => undefined);
ipcMain.on("popover:action", () => {});
const trayState: TrayState = {
  status: "idle", settings: { ...settingsView.settings, autoCheck: true, autoCheckMode: "popup" }, hotkey: "⌥⌘G",
  insights: [{ from: "has", to: "have", count: 3 }, { from: "monday", to: "Monday", count: 2 }], hasApiKey: false,
};
ipcMain.handle("tray:get", () => trayState);

const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms));
const webPreferences = { preload: join(root, "dist/preload.cjs"), sandbox: true, contextIsolation: true, backgroundThrottling: false };

async function shot(name: string, win: BrowserWindow, keep = false) {
  await sleep(500);
  const img = await win.webContents.capturePage();
  writeFileSync(join(out, name), img.toPNG());
  const { width, height } = img.getSize();
  console.log(`${name} ${width}x${height}`);
  if (!keep) win.destroy();
}

async function popover(name: string, p: PopoverPayload) {
  const win = new BrowserWindow({ width: 460, height: 260, show: false, frame: false, transparent: true, webPreferences });
  ipcMain.once("popover:resize", (_e, h: number, w?: number) => win.setSize(Math.ceil(w ?? 460), Math.ceil(h)));
  await win.loadFile(join(root, "dist/renderer/popover.html"));
  win.webContents.send("popover:show", p);
  await shot(name, win);
}

app.on("window-all-closed", () => {}); // keep running between scenes

app.whenReady().then(async () => {
  await popover("popover-diff.png", diff);
  await popover("popover-loading.png", { kind: "loading", message: "Checking…" });
  await popover("popover-clean.png", { kind: "message", message: "No changes needed ✓" });

  const tray = new BrowserWindow({ width: 344, height: 2400, show: false, frame: false, transparent: true, webPreferences });
  ipcMain.on("tray:resize", (_e, h: number) => tray.isDestroyed() || tray.setSize(344, Math.ceil(h))); // uncapped: the shot shows every section
  await tray.loadFile(join(root, "dist/renderer/tray.html"));
  await shot("tray.png", tray, true);
  await tray.webContents.executeJavaScript(`document.getElementById("toSetup").click()`);
  await shot("tray-setup.png", tray);

  const badge = new BrowserWindow({ width: 22, height: 22, show: false, frame: false, transparent: true, webPreferences });
  await badge.loadURL("data:text/html;charset=utf-8," + encodeURIComponent(BADGE_PAGE));
  badge.webContents.setZoomFactor(8);
  badge.setSize(176, 176);
  await shot("badge-logo.png", badge);

  app.quit();
});
