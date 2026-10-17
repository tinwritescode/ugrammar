import { spawn, type ChildProcessWithoutNullStreams } from "node:child_process";
import { createInterface } from "node:readline";

export interface HelperReply {
  ok: boolean;
  error?: string;
  [k: string]: unknown;
}

export interface SelectionReply extends HelperReply {
  text?: string;
  method?: "ax" | "clipboard";
  bundleId?: string;
  appName?: string;
  pid?: number;
}

/** Talks JSON lines to the Swift helper. Respawns it if it dies. */
export class HelperClient {
  private proc: ChildProcessWithoutNullStreams | null = null;
  private nextId = 1;
  private pending = new Map<number, { resolve: (r: HelperReply) => void; timer: ReturnType<typeof setTimeout> }>();
  private stopped = false;
  onHotkey: () => void = () => {};
  onHotkey2: () => void = () => {};
  onRestart: () => void = () => {};

  constructor(private path: string) {}

  start() {
    this.stopped = false;
    const p = spawn(this.path, [], { stdio: ["pipe", "pipe", "pipe"] });
    this.proc = p;
    createInterface({ input: p.stdout }).on("line", (line) => this.onLine(line));
    p.stderr.on("data", () => {}); // never log content
    p.on("error", () => {});
    p.on("exit", () => {
      for (const [id, w] of this.pending) {
        clearTimeout(w.timer);
        w.resolve({ ok: false, error: "helper_exited" });
        this.pending.delete(id);
      }
      if (this.proc === p) this.proc = null;
      if (!this.stopped) setTimeout(() => { if (!this.stopped) { this.start(); this.onRestart(); } }, 1000);
    });
  }

  stop() {
    this.stopped = true;
    this.proc?.kill();
    this.proc = null;
  }

  private onLine(line: string) {
    let msg: HelperReply & { id?: number; event?: string };
    try { msg = JSON.parse(line); } catch { return; }
    if (msg.event === "hotkey") return this.onHotkey();
    if (msg.event === "hotkey2") return this.onHotkey2();
    if (typeof msg.id === "number") {
      const w = this.pending.get(msg.id);
      if (w) { clearTimeout(w.timer); this.pending.delete(msg.id); w.resolve(msg); }
    }
  }

  request<T extends HelperReply = HelperReply>(cmd: string, args: Record<string, unknown> = {}, timeoutMs = 5000): Promise<T> {
    return new Promise((resolve) => {
      if (!this.proc) return resolve({ ok: false, error: "helper_not_running" } as T);
      const id = this.nextId++;
      const timer = setTimeout(() => {
        this.pending.delete(id);
        resolve({ ok: false, error: "helper_timeout" } as T);
      }, timeoutMs);
      this.pending.set(id, { resolve: resolve as (r: HelperReply) => void, timer });
      this.proc.stdin.write(JSON.stringify({ id, cmd, ...args }) + "\n");
    });
  }
}
