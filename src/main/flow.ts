import { diffWords } from "../shared/diff";
import type { PopoverPayload } from "../shared/ipc";
import type { GrammarEngine } from "./engine/types";
import type { HelperReply, SelectionReply } from "./helper";
import type { Settings } from "./settings";

export type StatusKind = "idle" | "checking" | "paused" | "error" | "needs_permission";
export interface Status {
  kind: StatusKind;
  message?: string;
}

export type FixOutcome =
  | "paused" | "busy" | "skipped" | "no_selection" | "needs_permission"
  | "error" | "unchanged" | "too_short" | "dismissed" | "applied" | "write_failed";

export interface FlowDeps {
  settings(): Settings;
  readSelection(excluded: string[], selectAll?: boolean): Promise<SelectionReply>;
  writeSelection(text: string, pid: number | undefined, method: "ax" | "clipboard" | undefined): Promise<HelperReply>;
  makeEngine(): Promise<GrammarEngine>;
  setStatus(s: Status): void;
  notify(message: string): void;
  /** Resolves when the user picks, or when superseded via cancelPopover(). */
  showDiff(p: PopoverPayload): Promise<"accept" | "dismiss">;
  showMessage(message: string): void;
  /** Immediate "Checking…" feedback; replaced by showDiff/showMessage or removed by hideLoading. */
  showLoading(): void;
  hideLoading(): void;
}

const READ_ERRORS: Record<string, string> = {
  excluded_app: "Skipped: this app is on the exclusion list.",
  secure_field: "Skipped: secure/password field.",
  no_selection: "Select some text first.",
};

/** One hotkey press: read selection -> engine -> diff popover -> write. Original text is only touched on Accept. */
export class FixFlow {
  private busy = false;
  constructor(private d: FlowDeps) {}

  private idle() {
    this.d.setStatus(this.d.settings().paused ? { kind: "paused" } : { kind: "idle" });
  }

  /** `selectAll`: used by the badge click, checks the whole field when nothing is selected. */
  async run(opts: { selectAll?: boolean } = {}): Promise<FixOutcome> {
    const s = this.d.settings();
    if (s.paused) return "paused";
    if (this.busy) return "busy";
    this.busy = true;
    try {
      this.d.setStatus({ kind: "checking" });
      this.d.showLoading();
      const sel = await this.d.readSelection(s.excludedApps, opts.selectAll);
      if (!sel.ok || !sel.text) {
        if (sel.error === "accessibility_not_trusted") {
          this.d.hideLoading();
          this.d.setStatus({ kind: "needs_permission", message: "Grant Accessibility permission" });
          this.d.notify("ugrammar needs Accessibility permission (System Settings → Privacy & Security → Accessibility).");
          return "needs_permission";
        }
        const msg = READ_ERRORS[sel.error ?? ""] ?? `Could not read selection (${sel.error ?? "unknown"}).`;
        this.d.showMessage(msg);
        this.idle();
        return sel.error === "no_selection" ? "no_selection" : "skipped";
      }

      if (sel.text.trim().split(/\s+/).length < 3) {
        this.d.showMessage("Too short to check (under 3 words).");
        this.idle();
        return "too_short";
      }

      let result: { corrected: string; notes?: string[] };
      try {
        const engine = await this.d.makeEngine();
        result = (await engine.check({ text: sel.text, dictionary: s.dictionary, explain: s.explain }));
      } catch (e) {
        const msg = (e as Error).message || "Engine error";
        this.d.hideLoading();
        this.d.setStatus({ kind: "error", message: msg });
        this.d.notify(`Check failed: ${msg}. Your text was not changed.`);
        return "error";
      }

      const corrected = result.corrected;
      if (corrected === sel.text) {
        this.d.showMessage("No changes needed ✓");
        this.idle();
        return "unchanged";
      }

      this.idle();
      const choice = await this.d.showDiff({
        kind: "diff",
        original: sel.text,
        corrected,
        ops: diffWords(sel.text, corrected),
        appName: sel.appName,
        notes: result.notes,
      });
      if (choice !== "accept") return "dismissed";

      const w = await this.d.writeSelection(corrected, sel.pid, sel.method);
      if (!w.ok) {
        const msg = w.error === "focus_changed" ? "Focus changed; text not replaced." : `Could not replace text (${w.error}).`;
        this.d.notify(msg);
        return "write_failed";
      }
      return "applied";
    } finally {
      this.busy = false;
    }
  }
}
