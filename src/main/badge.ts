import { BrowserWindow, screen } from "electron";
import { BADGE_SIZE, badgePosition, clampToArea, type CaretRect } from "../shared/badge";

export type BadgeState = "idle" | "checking";

const PAGE = `<!doctype html><meta charset="utf-8"><style>
html,body{margin:0;background:transparent;overflow:hidden}
#d{width:14px;height:14px;margin:4px;border-radius:50%;background:#30d158;box-shadow:0 0 0 2px rgba(255,255,255,.85),0 1px 4px rgba(0,0,0,.3);transition:transform .15s}
.typing{transform:scale(1.25)}
.checking{background:transparent;border:2px solid rgba(0,0,0,.15);border-top-color:#0a84ff;box-sizing:border-box;animation:s .7s linear infinite}
@keyframes s{to{transform:rotate(360deg)}}
</style><div id="d"></div>`;

/** Small always-on-top dot beside the focused text field: shows Proofy is running and sees you typing. */
export class TypingBadge {
  private win: BrowserWindow | null = null;
  private timer: NodeJS.Timeout | null = null;
  private last = "";
  private state: BadgeState = "idle";
  private busy = false;

  constructor(private fetchCaret: () => Promise<(CaretRect & { visible: boolean }) | null>) {}

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
      webPreferences: { sandbox: true, contextIsolation: true },
    });
    this.win.setIgnoreMouseEvents(true);
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
      if (!r || !r.visible) return this.hide();
      const win = this.ensure();
      const pos = badgePosition(r);
      const area = screen.getDisplayNearestPoint(pos).workArea;
      const p = clampToArea(pos, area);
      const key = `${r.x},${r.y},${r.w},${r.h}`;
      const moved = key !== this.last;
      this.last = key;
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
