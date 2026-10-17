import { diffSpans } from "../shared/spans";
import { sentenceEndingAt } from "../shared/sentence";
import type { GrammarEngine } from "./engine/types";
import type { HelperReply } from "./helper";
import type { Settings } from "./settings";

export interface Issue {
  id: number;
  start: number;
  end: number;
  original: string;
  replacement: string;
  reason?: string;
}

export interface Snapshot extends HelperReply {
  visible?: boolean;
  text?: string;
  caret?: number;
  selLen?: number;
  pid?: number;
  bundleId?: string;
  appName?: string;
  bounds?: ({ x: number; y: number; w: number; h: number } | null)[];
}

export interface AutoDeps {
  settings(): Settings;
  snapshot(excluded: string[], ranges: { loc: number; len: number }[]): Promise<Snapshot>;
  makeEngine(): Promise<GrammarEngine>;
  /** Underline marks changed; empty = hide all. */
  onMarks(marks: { id: number; x: number; y: number; w: number; h: number }[]): void;
  now?(): number;
  /** Diagnostics: outcomes only, never text. */
  log?(msg: string): void;
}

const POLL_MS = 600;
const STABLE_MS = 500;
const MAX_CACHE = 200;

/** Keep issues whose text is still at (or has shifted near) its old place; drop the rest. */
export function revalidate(issues: Issue[], text: string): Issue[] {
  const out: Issue[] = [];
  for (const it of issues) {
    if (text.slice(it.start, it.end) === it.original) { out.push(it); continue; }
    let best = -1;
    for (let i = text.indexOf(it.original); i !== -1 && it.original; i = text.indexOf(it.original, i + 1)) {
      if (best === -1 || Math.abs(i - it.start) < Math.abs(best - it.start)) best = i;
    }
    if (best !== -1 && Math.abs(best - it.start) <= 200) out.push({ ...it, start: best, end: best + it.original.length });
  }
  return out;
}

/** Watches the focused field; when a sentence just ended and the text settled, checks only that sentence. */
export class AutoChecker {
  private timer: ReturnType<typeof setInterval> | null = null;
  private busy = false;
  private checking = false;
  private nextId = 1;
  private field = "";
  private lastText = "";
  private loggedText = "";
  private changedAt = 0;
  private checked = new Set<string>();
  issues: Issue[] = [];
  pid?: number;
  appName?: string;

  constructor(private d: AutoDeps) {}

  start() {
    if (this.timer) return;
    this.timer = setInterval(() => void this.tick(), POLL_MS);
  }

  stop() {
    if (this.timer) clearInterval(this.timer);
    this.timer = null;
    this.reset();
  }

  private reset() {
    this.issues = [];
    this.lastText = "";
    this.field = "";
    this.d.onMarks([]);
  }

  dismiss(id: number) {
    this.issues = this.issues.filter((i) => i.id !== id);
  }

  /** After a range was replaced: forget that issue and let the next tick re-anchor the rest. */
  applied(id: number) {
    this.dismiss(id);
    this.lastText = "";
  }

  private async tick() {
    if (this.busy) return;
    this.busy = true;
    try {
      const s = this.d.settings();
      if (!s.autoCheck || s.paused) return this.reset();
      const snap = await this.d.snapshot(s.excludedApps, this.issues.map((i) => ({ loc: i.start, len: i.end - i.start })));
      if (!snap.ok || !snap.visible || typeof snap.text !== "string") { this.d.log?.(`snap ok=${snap.ok} visible=${snap.visible} err=${snap.error ?? "-"}`); return this.reset(); }
      const text = snap.text;
      const field = `${snap.pid}:${snap.bundleId}`;
      const now = this.d.now?.() ?? Date.now();
      this.pid = snap.pid;
      this.appName = snap.appName;
      if (field !== this.field) { this.reset(); this.field = field; this.checked.clear(); }
      if (text !== this.lastText) {
        this.lastText = text;
        this.changedAt = now;
        this.issues = revalidate(this.issues, text);
      }
      this.publish(snap);
      if (this.checking || now - this.changedAt < STABLE_MS || (snap.selLen ?? 0) > 0) return;
      const sent = sentenceEndingAt(text, snap.caret ?? 0);
      if (!sent || this.checked.has(sent.text)) {
        if (text !== this.loggedText) { this.loggedText = text; this.d.log?.(`no sentence app=${snap.bundleId} len=${text.length} caret=${snap.caret} cached=${!!sent}`); }
        return;
      }
      this.checked.add(sent.text);
      if (this.checked.size > MAX_CACHE) this.checked.delete(this.checked.values().next().value!);
      this.d.log?.(`check start app=${snap.bundleId} len=${sent.text.length}`);
      void this.check(sent.start, sent.text, field);
    } catch {
      this.d.onMarks([]);
    } finally {
      this.busy = false;
    }
  }

  private publish(snap: Snapshot) {
    const marks = this.issues.flatMap((it, i) => {
      const b = snap.bounds?.[i];
      return b ? [{ id: it.id, x: b.x, y: b.y, w: b.w, h: b.h }] : [];
    });
    this.d.onMarks(marks);
  }

  private async check(offset: number, sentence: string, field: string) {
    this.checking = true;
    try {
      const s = this.d.settings();
      const engine = await this.d.makeEngine();
      const r = await engine.check({ text: sentence, dictionary: s.dictionary, explain: false });
      this.d.log?.(`check done changed=${r.corrected !== sentence} stale=${field !== this.field}`);
      if (field !== this.field || r.corrected === sentence) return;
      const notes = r.notes ?? [];
      const fresh = diffSpans(sentence, r.corrected).filter((sp) => sp.original.trim()).map((sp, i): Issue => {
        const w = sp.original.trim().toLowerCase();
        return {
          id: this.nextId++,
          start: offset + sp.start,
          end: offset + sp.end,
          original: sp.original,
          replacement: sp.replacement,
          reason: notes.find((n) => n.toLowerCase().startsWith(w)) ?? (notes.length === 1 ? notes[0] : notes[i]),
        };
      });
      this.d.log?.(`issues=${fresh.length}`);
      this.issues = revalidate([...this.issues.filter((i) => i.end <= offset || i.start >= offset + sentence.length), ...fresh], this.lastText);
    } catch (e) {
      this.d.log?.(`check error ${(e as Error).message}`);
      // network/engine errors are silent in the background (sentence stays cached: no retry loop); the manual hotkey still reports them
    } finally {
      this.checking = false;
    }
  }
}
