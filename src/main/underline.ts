import { BrowserWindow, ipcMain, screen } from "electron";
import { join } from "node:path";

export interface UnderlineMark {
  id: number;
  x: number;
  y: number;
  w: number;
  h: number;
}

const STRIP = 8;

const PAGE = `<!doctype html><meta charset="utf-8"><style>
html,body{margin:0;height:100%;background:transparent;overflow:hidden;cursor:pointer}
#u{position:absolute;left:0;right:0;bottom:1px;height:4px;background:repeating-linear-gradient(135deg,#ff5d5d 0 2px,transparent 2px 4px);opacity:.9;border-radius:2px}
body:hover #u{height:5px;opacity:1}
</style><div id="u"></div>
<script>document.addEventListener("mousedown",e=>{if(e.button===0)proofy.underlineClick()})</script>`;

/** One tiny non-focusable strip per issue, drawn under the word. Clicking it asks main to open the card. */
export class UnderlineOverlay {
  private wins = new Map<number, BrowserWindow>();
  private rects = new Map<number, UnderlineMark>();

  constructor(private root: string, private onClick: (id: number) => void) {
    ipcMain.on("underline:click", (e) => {
      for (const [id, w] of this.wins) if (!w.isDestroyed() && w.webContents === e.sender) return this.onClick(id);
    });
  }

  private make(): BrowserWindow {
    const w = new BrowserWindow({
      width: 40, height: STRIP, show: false, frame: false, resizable: false, transparent: true, alwaysOnTop: true,
      skipTaskbar: true, hasShadow: false, type: "panel", focusable: false,
      webPreferences: { preload: join(this.root, "dist/preload.cjs"), sandbox: true, contextIsolation: true },
    });
    w.setAlwaysOnTop(true, "screen-saver");
    w.setVisibleOnAllWorkspaces(true, { visibleOnFullScreen: true });
    void w.loadURL("data:text/html;charset=utf-8," + encodeURIComponent(PAGE));
    return w;
  }

  /** Show exactly these marks (screen coords of the word); anything else is hidden. */
  set(marks: UnderlineMark[]) {
    const keep = new Set(marks.map((m) => m.id));
    this.rects = new Map(marks.map((m) => [m.id, m]));
    for (const [id, w] of this.wins) {
      if (keep.has(id)) continue;
      if (!w.isDestroyed()) w.destroy();
      this.wins.delete(id);
    }
    for (const m of marks) {
      let w = this.wins.get(m.id);
      if (!w || w.isDestroyed()) { w = this.make(); this.wins.set(m.id, w); }
      const area = screen.getDisplayNearestPoint({ x: m.x, y: m.y }).bounds;
      const width = Math.max(12, Math.min(Math.round(m.w), area.x + area.width - m.x));
      w.setBounds({ x: Math.round(m.x), y: Math.round(m.y + m.h - STRIP + 1), width, height: STRIP });
      if (!w.isVisible()) w.showInactive();
    }
  }

  /** Word rect (screen coords) of a mark, for placing the card. */
  bounds(id: number) {
    return this.rects.get(id);
  }

  clear() {
    this.set([]);
  }
}
