import { expect, test } from "bun:test";
import { revalidate, type Issue } from "../src/main/autocheck";

const it = (start: number, original: string): Issue => ({ id: 1, start, end: start + original.length, original, replacement: "x" });

test("revalidate keeps, shifts, drops", () => {
  expect(revalidate([it(2, "has")], "I has a cat")).toEqual([it(2, "has")]);
  expect(revalidate([it(2, "has")], "Oh I has a cat")[0].start).toBe(5);
  expect(revalidate([it(2, "has")], "I have a cat")).toEqual([]);
});

import { AutoChecker, type SentenceFix, type Snapshot } from "../src/main/autocheck";
import { DEFAULT_SETTINGS } from "../src/main/settings";

const S = "She dont like it.";

function rig(mode: "underline" | "popup" = "underline", typeDuringCheck = false) {
  let snap: Snapshot = { ok: true, visible: true, text: S, caret: S.length, selLen: 0, pid: 1, bundleId: "com.tinyspeck.slackmacgap" };
  let clock = 0;
  let calls = 0;
  let marks: { id: number }[] = [];
  const popups: SentenceFix[] = [];
  let explain: boolean | undefined;
  const loading: ({ start: number; length: number } | null)[] = [];
  const auto = new AutoChecker({
    settings: () => ({ ...DEFAULT_SETTINGS, autoCheck: true, autoCheckMode: mode }),
    snapshot: async (_x, ranges) => ({ ...snap, bounds: ranges.map(() => ({ x: 1, y: 2, w: 3, h: 4 })) }),
    makeEngine: async () => ({ testConnection: async () => ({ ok: true, message: "" }), check: async (req) => {
      calls++;
      explain = req.explain;
      if (typeDuringCheck) { snap = { ...snap, text: S + " Next", caret: S.length + 5 }; await auto.tick(); }
      return { original: S, corrected: "She doesn't like it." }; } }),
    onMarks: (m) => { marks = m; },
    onSentence: (f) => popups.push(f),
    onLoading: (at) => loading.push(at),
    now: () => clock,
  });
  const step = async (ms = 600) => { clock += ms; await auto.tick(); await Bun.sleep(0); };
  return { auto, step, set: (s: Partial<Snapshot>) => { snap = { ...snap, ...s }; }, marks: () => marks, calls: () => calls, popups, explain: () => explain, loading };
}

test("sentence end → issue underlined", async () => {
  const r = rig();
  await r.step(); await r.step(); await r.step();
  expect(r.auto.issues.map((i) => i.original)).toEqual(["dont"]);
  expect(r.marks().length).toBe(1);
});

test("one-poll focus blip hides marks but keeps issues", async () => {
  const r = rig();
  await r.step(); await r.step(); await r.step();
  r.set({ visible: false });
  await r.step();
  expect(r.marks()).toEqual([]);
  r.set({ visible: true });
  await r.step();
  expect(r.auto.issues.length).toBe(1);
  expect(r.marks().length).toBe(1);
});

test("retyped sentence reuses the cached result; dismissed fix stays gone", async () => {
  const r = rig();
  await r.step(); await r.step(); await r.step();
  r.set({ text: "", caret: 0 }); await r.step(); await r.step();
  expect(r.auto.issues).toEqual([]);
  r.set({ text: S, caret: S.length }); await r.step(); await r.step();
  expect(r.auto.issues.length).toBe(1);
  expect(r.calls()).toBe(1);
  r.auto.dismiss(r.auto.issues[0].id);
  await r.step(); await r.step();
  expect(r.auto.issues).toEqual([]);
});

test("popup mode: fix with reasons opens once the check returns", async () => {
  const r = rig("popup");
  await r.step(); await r.step(); await r.step();
  expect(r.popups.map((p) => [p.start, p.original, p.corrected])).toEqual([[0, S, "She doesn't like it."]]);
  expect(r.explain()).toBe(true);
  r.auto.resolveSentence(0, S.length);
  await r.step();
  expect(r.auto.issues).toEqual([]);
});

test("popup mode: accepted sentence is not checked again", async () => {
  const r = rig("popup");
  await r.step(); await r.step(); await r.step();
  const fixed = "She doesn't like it.";
  r.set({ text: fixed, caret: fixed.length });
  r.auto.resolveSentence(0, S.length, fixed);
  await r.step(); await r.step(); await r.step();
  expect(r.calls()).toBe(1);
  expect(r.popups.length).toBe(1);
});

test("underline fix applied: patched sentence is not checked again", async () => {
  const r = rig();
  await r.step(); await r.step(); await r.step();
  const fixed = "She doesn't like it.";
  r.set({ text: fixed, caret: fixed.length });
  r.auto.applied(r.auto.issues[0].id);
  await r.step(); await r.step(); await r.step();
  expect(r.calls()).toBe(1);
  expect(r.auto.issues).toEqual([]);
});

test("popup mode: no popup once the user typed past the sentence; underline stays", async () => {
  const r = rig("popup", true);
  await r.step(); await r.step(); await r.step();
  expect(r.popups).toEqual([]);
  expect(r.auto.issues.length).toBe(1);
});

test("underline mode skips reasons and never pops up", async () => {
  const r = rig();
  await r.step(); await r.step(); await r.step();
  expect(r.popups).toEqual([]);
  expect(r.explain()).toBe(false);
});

test("popup mode: loading shows at the sentence, then the popup takes its place", async () => {
  const r = rig("popup");
  await r.step(); await r.step(); await r.step();
  expect(r.loading).toEqual([{ start: 0, length: S.length }]);
  expect(r.popups.length).toBe(1);
});

test("popup mode: typing on hides the loading and skips the popup", async () => {
  const r = rig("popup", true);
  await r.step(); await r.step(); await r.step();
  expect(r.loading).toEqual([{ start: 0, length: S.length }, null]);
});

test("underline mode never shows loading", async () => {
  const r = rig();
  await r.step(); await r.step(); await r.step();
  expect(r.loading).toEqual([]);
});
