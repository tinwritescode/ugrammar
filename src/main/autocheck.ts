import { diffSpans } from "../shared/spans";
import { sentenceEndingAt, type SentenceRange } from "../shared/sentence";
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

/** A checked sentence with fixes, for the popup mode. Offsets are UTF-16, into the field text. */
export interface SentenceFix {
  start: number;
  original: string;
  corrected: string;
  notes: string[];
  /** Issue the popup anchors to (its underline bounds). */
  firstId: number;
}

export interface AutoDeps {
  settings(): Settings;
  snapshot(excluded: string[], ranges: { loc: number; len: number }[]): Promise<Snapshot>;
  makeEngine(): Promise<GrammarEngine>;
  /** Underline marks changed; empty = hide all. */
  onMarks(marks: { id: number; x: number; y: number; w: number; h: number }[]): void;
  now?(): number;
  /** Popup mode: show (range) / hide (null) a "Checking…" hint at the sentence while the request runs. */
  onLoading?(at: { start: number; length: number } | null): void;
  /** Popup mode: fires only while the user has not typed past the sentence. */
  onSentence?(fix: SentenceFix): void;
  /** Diagnostics: outcomes only, never text. */
  log?(msg: string): void;
}

const POLL_MS = 250;
const STABLE_MS = 500;
/** A typed . ? ! is a clear "sentence done" signal: wait only long enough to skip "..." / "?!" mid-burst. */
const PUNCT_STABLE_MS = 200;
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
  private loading = false;
  private nextId = 1;
  private field = "";
  private lastText = "";
  private loggedText = "";
  private loggedMarks = "";
  private changedAt = 0;
  /** Sentence seen ending at the caret; checked even if the user types past it before the text settles. */
  private pending: SentenceRange | null = null;
  /** Sentence → its spans (offsets within the sentence); null while in flight or after a failure. */
  private checked = new Map<string, Omit<Issue, "id">[] | null>();
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
    this.pending = null;
    this.d.onMarks([]);
  }

  dismiss(id: number) {
    const gone = this.issues.find((i) => i.id === id);
    this.issues = this.issues.filter((i) => i.id !== id);
    // A dismissed fix must not come back from the cache when the sentence is seen again.
    if (gone) for (const [k, spans] of this.checked) {
      if (spans) this.checked.set(k, spans.filter((sp) => sp.original !== gone.original || sp.replacement !== gone.replacement));
    }
  }

  /** Popup closed: `corrected` = the sentence was rewritten to it; omitted = dismissed, its fixes stay hidden. */
  resolveSentence(start: number, length: number, corrected?: string) {
    const inside = this.issues.filter((i) => i.start >= start && i.end <= start + length);
    for (const it of inside) this.dismiss(it.id);
    if (corrected === undefined) return;
    this.cacheClean(corrected, []); // the accepted text must not be sent back for another pass
    this.lastText = "";
  }

  /** After a range was replaced: forget that issue and let the next tick re-anchor the rest. */
  applied(id: number) {
    const it = this.issues.find((i) => i.id === id);
    if (it) for (const [sentence, spans] of [...this.checked]) {
      const sp = spans?.find((x) => x.original === it.original && x.replacement === it.replacement);
      if (!sp) continue;
      // The patched sentence keeps its other fixes, shifted, instead of a fresh request.
      const delta = sp.replacement.length - sp.original.length;
      const rest = spans!.filter((x) => x !== sp).map((x) => (x.start >= sp.end ? { ...x, start: x.start + delta, end: x.end + delta } : x));
      this.cacheClean(sentence.slice(0, sp.start) + sp.replacement + sentence.slice(sp.end), rest);
    }
    this.dismiss(id);
    this.lastText = "";
  }

  private cacheClean(sentence: string, spans: Omit<Issue, "id">[]) {
    this.checked.set(sentence.trim(), spans); // sentenceEndingAt never includes edge whitespace
    if (this.checked.size > MAX_CACHE) this.checked.delete(this.checked.keys().next().value!);
  }

  async tick() {
    if (this.busy) return;
    this.busy = true;
    try {
      const s = this.d.settings();
      if (!s.autoCheck || s.paused) return this.reset();
      const asked = this.issues;
      const snap = await this.d.snapshot(s.excludedApps, asked.map((i) => ({ loc: i.start, len: i.end - i.start })));
      if (!snap.ok || !snap.visible || typeof snap.text !== "string") {
        // Focus often blips to a button/toolbar for one poll: hide marks but keep issues, so the field comes back intact.
        this.d.log?.(`snap ok=${snap.ok} visible=${snap.visible} err=${snap.error ?? "-"} why=${String(snap.reason ?? "-")}`);
        return this.d.onMarks([]);
      }
      const text = snap.text;
      const field = `${snap.pid}:${snap.bundleId}`;
      const now = this.d.now?.() ?? Date.now();
      this.pid = snap.pid;
      this.appName = snap.appName;
      if (field !== this.field) { this.reset(); this.field = field; this.checked.clear(); }
      if (text !== this.lastText) {
        this.lastText = text;
        this.changedAt = now;
        this.setLoading(null); // typing on: the popup will not open, so drop its hint
        this.issues = revalidate(this.issues, text);
        this.pending = sentenceEndingAt(text, snap.caret ?? 0) ?? this.pending;
      }
      this.publish(snap, asked);
      if (this.checking || (snap.selLen ?? 0) > 0) return;
      const sent = sentenceEndingAt(text, snap.caret ?? 0);
      const p = this.pending;
      if (p && p.text !== sent?.text) {
        // Typed past it: the sentence is done, no need to wait for the text to settle.
        this.pending = null;
        if (text.slice(p.start, p.end) === p.text && !this.checked.has(p.text)) return this.begin(p, snap.bundleId, field);
      }
      if (now - this.changedAt < (sent && /[.?!]$/.test(sent.text) ? PUNCT_STABLE_MS : STABLE_MS)) return;
      if (sent && this.checked.has(sent.text)) return this.restore(sent.start, sent.text);
      if (!sent) {
        if (text !== this.loggedText) { this.loggedText = text; this.d.log?.(`no sentence app=${snap.bundleId} len=${text.length} caret=${snap.caret}`); }
        return;
      }
      this.begin(sent, snap.bundleId, field);
    } catch {
      this.d.onMarks([]);
    } finally {
      this.busy = false;
    }
  }

  private begin(sent: SentenceRange, app: string | undefined, field: string) {
    if (this.pending?.text === sent.text) this.pending = null;
    this.checked.set(sent.text, null);
    if (this.checked.size > MAX_CACHE) this.checked.delete(this.checked.keys().next().value!);
    this.d.log?.(`check start app=${app} len=${sent.text.length}`);
    void this.check(sent.start, sent.text, field);
  }

  /** Same sentence typed again (or retyped elsewhere in the field): re-add its cached issues without a new request. */
  private restore(offset: number, sentence: string) {
    const spans = this.checked.get(sentence);
    if (!spans?.length) return;
    const inside = (i: Issue) => i.start >= offset && i.end <= offset + sentence.length;
    if (this.issues.some(inside)) return;
    this.merge(offset, sentence, spans);
  }

  private merge(offset: number, sentence: string, spans: Omit<Issue, "id">[]) {
    const fresh = spans.map((sp): Issue => ({ ...sp, id: this.nextId++, start: offset + sp.start, end: offset + sp.end }));
    this.issues = revalidate([...this.issues.filter((i) => i.end <= offset || i.start >= offset + sentence.length), ...fresh], this.lastText);
  }

  /** Bounds come back in the order of `asked`; only issues that have not moved since keep their mark this tick. */
  private publish(snap: Snapshot, asked: Issue[]) {
    const marks = this.issues.flatMap((it) => {
      const i = asked.findIndex((a) => a.id === it.id && a.start === it.start);
      const b = i === -1 ? null : snap.bounds?.[i];
      return b ? [{ id: it.id, x: b.x, y: b.y, w: b.w, h: b.h }] : [];
    });
    const sum = `marks=${marks.length}/${this.issues.length} bounds=${snap.bounds?.length ?? "-"}`;
    if (this.issues.length && sum !== this.loggedMarks) { this.loggedMarks = sum; this.d.log?.(sum); }
    this.d.onMarks(marks);
  }

  private async check(offset: number, sentence: string, field: string) {
    this.checking = true;
    try {
      const s = this.d.settings();
      const popup = s.autoCheckMode === "popup" && !!this.d.onSentence;
      const before = this.lastText;
      if (popup) this.setLoading({ start: offset, length: sentence.length });
      const engine = await this.d.makeEngine();
      // Reasons cost ~1.5 s; only the popup shows them up front.
      const r = await engine.check({ text: sentence, dictionary: s.dictionary, explain: popup && s.explain, explainLang: s.explainLang });
      this.d.log?.(`check done changed=${r.corrected !== sentence} stale=${field !== this.field}`);
      if (field !== this.field || r.corrected === sentence) return;
      const notes = r.notes ?? [];
      const spans = diffSpans(sentence, r.corrected).filter((sp) => sp.original.trim()).map((sp, i) => {
        const w = sp.original.trim().toLowerCase();
        return {
          start: sp.start,
          end: sp.end,
          original: sp.original,
          replacement: sp.replacement,
          reason: notes.find((n) => n.toLowerCase().startsWith(w)) ?? (notes.length === 1 ? notes[0] : notes[i]),
        };
      });
      this.d.log?.(`issues=${spans.length}`);
      if (this.checked.has(sentence)) this.checked.set(sentence, spans);
      this.merge(offset, sentence, spans);
      const first = this.issues.find((i) => i.start >= offset && i.end <= offset + sentence.length);
      if (popup && first && this.lastText === before) {
        this.loading = false; // the popup takes over the hint's place
        this.d.onSentence!({ start: offset, original: sentence, corrected: r.corrected, notes, firstId: first.id });
      }
    } catch (e) {
      this.d.log?.(`check error ${(e as Error).message}`);
      // network/engine errors are silent in the background (sentence stays cached: no retry loop); the manual hotkey still reports them
    } finally {
      this.checking = false;
      this.setLoading(null);
    }
  }

  private setLoading(at: { start: number; length: number } | null) {
    if (!at && !this.loading) return;
    this.loading = !!at;
    this.d.onLoading?.(at);
  }
}
