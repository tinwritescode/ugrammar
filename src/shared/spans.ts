import { diffWords } from "./diff";

export interface Span {
  /** Offsets into the original text (UTF-16 units). */
  start: number;
  end: number;
  original: string;
  replacement: string;
}

/** One span per changed region: adjacent delete/insert pairs merge into a replacement. */
export function diffSpans(original: string, corrected: string): Span[] {
  const spans: Span[] = [];
  let pos = 0;
  let cur: Span | null = null;
  const flush = () => { if (cur) { spans.push(cur); cur = null; } };
  for (const op of diffWords(original, corrected)) {
    if (op.type === "equal") {
      // A lone whitespace run between two changes belongs to one phrase-level change.
      flush();
      pos += op.text.length;
    } else if (op.type === "delete") {
      cur ??= { start: pos, end: pos, original: "", replacement: "" };
      cur.original += op.text;
      cur.end = pos + op.text.length;
      pos += op.text.length;
    } else {
      cur ??= { start: pos, end: pos, original: "", replacement: "" };
      cur.replacement += op.text;
    }
  }
  flush();
  return spans.map(trimSpan);
}

/** Drop shared leading/trailing whitespace so underlines hug the words. */
function trimSpan(s: Span): Span {
  let { start, end, original, replacement } = s;
  const lead = /^\s*/.exec(original)![0].length;
  if (lead && replacement.startsWith(original.slice(0, lead))) {
    start += lead; original = original.slice(lead); replacement = replacement.slice(lead);
  }
  const trail = /\s*$/.exec(original)![0].length;
  if (trail && replacement.endsWith(original.slice(original.length - trail))) {
    end -= trail; original = original.slice(0, original.length - trail); replacement = replacement.slice(0, replacement.length - trail);
  }
  return { start, end, original, replacement };
}
