import { BrowserWindow, ipcMain, screen } from "electron";
import { join } from "node:path";
import { BADGE_SIZE, badgePosition, clampToArea, type CaretRect } from "../shared/badge";

const KEEP_MS = 8000;

export type BadgeState = "idle" | "checking";

const PAGE = `<!doctype html><meta charset="utf-8"><style>
html,body{margin:0;background:transparent;overflow:hidden;cursor:pointer;user-select:none}
#d{position:relative;width:22px;height:22px;transition:transform .15s}
#d svg{display:block;width:22px;height:22px;filter:drop-shadow(0 1px 3px rgba(0,0,0,.35))}
#d:hover{transform:scale(1.12)}
.typing{transform:scale(1.18)}
.ring{display:none;position:absolute;inset:-2px;border-radius:50%;border:2px solid transparent;border-top-color:#fff;animation:s .7s linear infinite}
.checking .ring{display:block}
@keyframes s{to{transform:rotate(360deg)}}
</style>
<div id="d" title="Check grammar">
<svg viewBox="0 0 22 22"><defs><linearGradient id="g" x1="0" y1="0" x2="1" y2="1"><stop offset="0" stop-color="#4f9bff"/><stop offset="1" stop-color="#6a4cff"/></linearGradient></defs>
<rect width="22" height="22" rx="6" fill="url(#g)"/>
<path d="M7.5 16.5V5.5h4.1a3.1 3.1 0 0 1 0 6.2H7.5" fill="none" stroke="#fff" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"/>
<path d="M12.2 15.6l1.7 1.7 3.3-3.6" fill="none" stroke="#9ff0b4" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round"/></svg>
<span class="ring"></span></div>
<script>document.addEventListener("mousedown",e=>{if(e.button===0)proofy.badgeClick()});document.addEventListener("contextmenu",e=>{e.preventDefault();proofy.badgeMenu()})</script>`;

/** Small always-on-top dot beside the focused text field: shows ugrammar is running and sees you typing. */
export class TypingBadge {
  private win: BrowserWindow | null = null;
  private timer: NodeJS.Timeout | null = null;
  private last = "";
  private state: BadgeState = "idle";
  private busy = false;
  /** Bundle id of the app the badge is currently beside (for the right-click menu). */
  bundleId = "";
  private lastMove = 0;
  private seen = false;

  onMenu?: (bundleId: string) => void;

  constructor(
    private root: string,
    private onClick: () => void,
    private fetchCaret: () => Promise<(CaretRect & { visible: boolean; selLen?: number; bundleId?: string }) | null>,
  ) {
    ipcMain.on("badge:click", () => this.onClick());
    ipcMain.on("badge:menu", () => this.onMenu?.(this.bundleId));
  }

  start() {
    if (this.timer) return;
    this.timer = setInterval(() => void this.tick(), 400);
  }

  stop() {
    if (this.timer) clearInterval(this.timer);
    this.timer = null;
    this.hide();
  }

  setState(s: BadgeState) {
    this.state = s;
    this.paint(false);
  }

  private hide() {
    if (this.win && !this.win.isDestroyed() && this.win.isVisible()) this.win.hide();
  }

  private ensure(): BrowserWindow {
    if (this.win && !this.win.isDestroyed()) return this.win;
    this.win = new BrowserWindow({
      width: BADGE_SIZE, height: BADGE_SIZE, show: false, frame: false, resizable: false, transparent: true,
      alwaysOnTop: true, skipTaskbar: true, hasShadow: false, type: "panel", focusable: false,
      webPreferences: { preload: join(this.root, "dist/preload.cjs"), sandbox: true, contextIsolation: true },
    });
    this.win.setAlwaysOnTop(true, "screen-saver");
    this.win.setVisibleOnAllWorkspaces(true, { visibleOnFullScreen: true });
    void this.win.loadURL("data:text/html;charset=utf-8," + encodeURIComponent(PAGE));
    return this.win;
  }

  private paint(typing: boolean) {
    if (!this.win || this.win.isDestroyed() || this.win.webContents.isLoading()) return;
    const cls = this.state === "checking" ? "checking" : typing ? "typing" : "";
    void this.win.webContents.executeJavaScript(`document.getElementById("d").className=${JSON.stringify(cls)}`).catch(() => {});
  }

  private async tick() {
    if (this.busy) return;
    this.busy = true;
    try {
      const r = await this.fetchCaret();
      if (!r || !r.visible) { this.seen = false; return this.hide(); }
      this.bundleId = r.bundleId ?? "";
      const win = this.ensure();
      const pos = badgePosition(r);
      const area = screen.getDisplayNearestPoint(pos).workArea;
      const p = clampToArea(pos, area);
      const key = `${r.x},${r.y},${r.w},${r.h}`;
      const moved = key !== this.last;
      this.last = key;
      const now = Date.now();
      // Show only after the first change in a field (i.e. typing), and for a few seconds after the last one.
      if ((moved && this.seen) || (r.selLen ?? 0) > 0) this.lastMove = now; // typing or a live selection
      this.seen = true;
      const cur = screen.getCursorScreenPoint();
      const b = win.getBounds();
      const hovering = cur.x >= b.x && cur.x <= b.x + b.width && cur.y >= b.y && cur.y <= b.y + b.height;
      if (this.state !== "checking" && !hovering && now - this.lastMove > KEEP_MS) return this.hide();
      win.setPosition(p.x, p.y);
      if (!win.isVisible()) win.showInactive();
      this.paint(moved);
    } catch {
      this.hide();
    } finally {
      this.busy = false;
    }
  }
}
