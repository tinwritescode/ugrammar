import { describe, expect, test } from "bun:test";
import { sentenceEndingAt } from "../src/shared/sentence";
import { diffSpans } from "../src/shared/spans";

describe("sentenceEndingAt", () => {
  test("detects a just-finished sentence", () => {
    const t = "Hello there. I has a cat.";
    const r = sentenceEndingAt(t, t.length)!;
    expect(r.text).toBe("I has a cat.");
    expect(t.slice(r.start, r.end)).toBe(r.text);
  });
  test("allows trailing space and newline", () => {
    expect(sentenceEndingAt("I has a cat. ", 13)?.text).toBe("I has a cat.");
    expect(sentenceEndingAt("I has a cat\n", 12)?.text).toBe("I has a cat");
  });
  test("no trigger mid-sentence or when too short", () => {
    expect(sentenceEndingAt("I has a cat", 11)).toBeNull();
    expect(sentenceEndingAt("Hi there.", 9)).toBeNull();
  });
  test("does not split on decimals", () => {
    const t = "The price is 3.5 dollars today.";
    expect(sentenceEndingAt(t, t.length)?.text).toBe(t);
  });
});

describe("diffSpans", () => {
  test("replacement spans point into the original", () => {
    const a = "Hi team, I has attached the report. Please to review it before monday.";
    const b = "Hi team, I have attached the report. Please review it before Monday.";
    const spans = diffSpans(a, b);
    expect(spans.map((s) => [s.original, s.replacement])).toEqual([["has", "have"], ["to ", ""], ["monday.", "Monday."]]);
    for (const s of spans) expect(a.slice(s.start, s.end)).toBe(s.original);
  });
  test("identical text has no spans", () => {
    expect(diffSpans("a b c", "a b c")).toEqual([]);
  });
});
