import { existsSync, mkdirSync, readFileSync, renameSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { diffSpans } from "../shared/spans";

export interface Insight {
  from: string;
  to: string;
  count: number;
  note?: string;
}

const MAX_ENTRIES = 500;

/** Local-only counters of fixes the user accepted; never leaves the machine. Only short word-level pairs are kept, not sentences. */
export class InsightStore {
  private data: Record<string, Insight> = {};
  private file: string;

  constructor(private dir: string) {
    this.file = join(dir, "insights.json");
    try {
      if (existsSync(this.file)) this.data = JSON.parse(readFileSync(this.file, "utf8"));
    } catch {}
  }

  record(original: string, corrected: string, notes: string[] = []) {
    for (const sp of diffSpans(original, corrected)) {
      const from = sp.original.trim();
      const to = sp.replacement.trim();
      if (!from && !to) continue;
      if (from.length > 40 || to.length > 40) continue;
      const key = `${from.toLowerCase()}→${to.toLowerCase()}`;
      const note = notes.find((n) => from && n.toLowerCase().startsWith(from.toLowerCase()));
      const cur = this.data[key] ?? { from, to, count: 0 };
      cur.count++;
      if (note) cur.note = note;
      this.data[key] = cur;
    }
    const keys = Object.keys(this.data);
    if (keys.length > MAX_ENTRIES) {
      keys.sort((a, b) => this.data[a].count - this.data[b].count).slice(0, keys.length - MAX_ENTRIES).forEach((k) => delete this.data[k]);
    }
    this.save();
  }

  top(n = 10): Insight[] {
    return Object.values(this.data).filter((i) => i.count >= 2).sort((a, b) => b.count - a.count).slice(0, n);
  }

  clear() {
    this.data = {};
    this.save();
  }

  private save() {
    try {
      mkdirSync(this.dir, { recursive: true });
      const tmp = this.file + ".tmp";
      writeFileSync(tmp, JSON.stringify(this.data));
      renameSync(tmp, this.file);
    } catch {}
  }
}
