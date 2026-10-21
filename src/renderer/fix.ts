import { el } from "./dom";
import type { DiffOp } from "../shared/ipc";

/** Engine notes read "old → new: reason"; anything else shows as plain text. */
export function note(n: string) {
  const m = /^(.+?)\s*(?:→|->)\s*(.+?):\s+(.+)$/s.exec(n);
  if (!m) return el("li", {}, n);
  const [, from, to, reason] = m;
  return el("li", {}, el("span", { className: "pair" }, el("del", {}, from), "→", el("ins", {}, to)), el("span", {}, reason));
}

/** Each run of deletes/inserts between equal text is one fix. */
export const fixCount = (ops: DiffOp[]) => ops.filter((op, i) => op.type !== "equal" && (i === 0 || ops[i - 1].type === "equal")).length;

/** The corrected text with removed words struck and added words marked. */
export const diffNodes = (ops: DiffOp[]) => ops.map((op) => (op.type === "equal" ? op.text : el(op.type === "insert" ? "ins" : "del", {}, op.text)));
