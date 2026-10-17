import { expect, test } from "bun:test";
import { revalidate, type Issue } from "../src/main/autocheck";

const it = (start: number, original: string): Issue => ({ id: 1, start, end: start + original.length, original, replacement: "x" });

test("revalidate keeps, shifts, drops", () => {
  expect(revalidate([it(2, "has")], "I has a cat")).toEqual([it(2, "has")]);
  expect(revalidate([it(2, "has")], "Oh I has a cat")[0].start).toBe(5);
  expect(revalidate([it(2, "has")], "I have a cat")).toEqual([]);
});
