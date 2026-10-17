export interface SentenceRange {
  start: number;
  end: number;
  text: string;
}

const TERMINATORS = /[.?!]/;

/** Indices are UTF-16 units, matching Accessibility ranges. */
export function sentenceEndingAt(text: string, caret: number, minWords = 3): SentenceRange | null {
  if (caret <= 0 || caret > text.length) return null;
  // Terminator is the last non-space char before the caret, or a newline was just typed.
  let i = caret;
  let sawNewline = false;
  while (i > 0 && /[ \t\r\n]/.test(text[i - 1])) {
    if (text[i - 1] === "\n") sawNewline = true;
    i--;
  }
  if (i === 0) return null;
  const endsWithPunct = TERMINATORS.test(text[i - 1]);
  if (!endsWithPunct && !sawNewline) return null;
  const end = i;
  // Walk back to the previous terminator followed by whitespace (or text start / newline).
  let start = 0;
  for (let k = end - 2; k >= 0; k--) {
    const c = text[k];
    if (c === "\n" || (TERMINATORS.test(c) && /\s/.test(text[k + 1] ?? ""))) { start = k + 1; break; }
  }
  while (start < end && /\s/.test(text[start])) start++;
  const sentence = text.slice(start, end);
  if (sentence.trim().split(/\s+/).length < minWords) return null;
  return { start, end, text: sentence };
}
