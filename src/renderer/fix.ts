import { el } from "./dom";
import type { FixKind } from "./i18n";
import type { DiffOp } from "../shared/ipc";

const isFixKind = (k: string, kinds: Record<FixKind, string>): k is FixKind => Object.hasOwn(kinds, k);

/** Engine notes read "old → new: [kind] reason"; the kind shows as a tag, anything else as plain text. */
export function note(n: string, kinds: Record<FixKind, string>) {
  const m = /^(.+?)\s*(?:→|->)\s*(.+?):\s+(?:\[(\w+)\]\s*)?(.+)$/s.exec(n);
  if (!m) return el("li", {}, n);
  const [, from, to, kind, reason] = m;
  const k = kind?.toLowerCase();
  const tag = k && isFixKind(k, kinds) ? [el("span", { className: `kind ${k}` }, kinds[k])] : [];
  return el("li", {}, el("span", { className: "pair" }, ...tag, el("del", {}, from), "→", el("ins", {}, to)), el("span", {}, reason));
}

/** Each run of deletes/inserts between equal text is one fix. */
export const fixCount = (ops: DiffOp[]) => ops.filter((op, i) => op.type !== "equal" && (i === 0 || ops[i - 1].type === "equal")).length;

/** Like `diffNodes`, but each fix is a span the user clicks to skip or keep. Fixes are numbered the way core's
 * `apply_fixes` counts them; `onToggle` gets the skipped numbers after every click. `originals[n]` is fix n's old text. */
export function fixToggles(ops: DiffOp[], titles: { skip: string; keep: string }, onToggle: (skipped: number[]) => void) {
  const skipped = new Set<number>();
  let fixes = 0;
  let fix: HTMLSpanElement | undefined;
  const nodes: (Node | string)[] = [];
  const originals: string[] = [];
  for (const op of ops) {
    if (op.type === "equal") {
      fix = undefined;
      nodes.push(op.text);
      continue;
    }
    if (!fix) {
      const n = fixes++;
      const span = el("span", { className: "fix", title: titles.skip });
      span.onclick = () => {
        const off = span.classList.toggle("skipped");
        if (off) skipped.add(n);
        else skipped.delete(n);
        span.title = off ? titles.keep : titles.skip;
        onToggle([...skipped]);
      };
      fix = span;
      nodes.push(span);
      originals.push("");
    }
    if (op.type === "delete") originals[fixes - 1] += op.text;
    fix.append(el(op.type === "insert" ? "ins" : "del", {}, op.text));
  }
  return { nodes, originals };
}

/** The word a fix replaced, without surrounding punctuation; null when it was not one word. */
export const dictionaryWord = (original: string) => {
  const w = original.trim().replace(/^\p{P}+|\p{P}+$/gu, "");
  return w && !/\s/.test(w) ? w : null;
};

/** The corrected text with removed words struck and added words marked. */
export const diffNodes = (ops: DiffOp[]) => ops.map((op) => (op.type === "equal" ? op.text : el(op.type === "insert" ? "ins" : "del", {}, op.text)));
