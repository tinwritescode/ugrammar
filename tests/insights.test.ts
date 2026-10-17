import { expect, test } from "bun:test";
import { mkdtempSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { InsightStore } from "../src/main/insights";

test("counts repeated fixes, hides singles, persists", () => {
  const dir = mkdtempSync(join(tmpdir(), "ins-"));
  const s = new InsightStore(dir);
  s.record("I has a cat", "I have a cat", ["has → have: subject-verb agreement"]);
  expect(s.top()).toEqual([]);
  s.record("She has two dogs", "She have two dogs".replace("She have", "She has"));
  s.record("We has fun", "We have fun");
  const top = new InsightStore(dir).top();
  expect(top.length).toBe(1);
  expect(top[0]).toMatchObject({ from: "has", to: "have", count: 2 });
  expect(top[0].note).toContain("agreement");
});
