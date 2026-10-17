import { expect, test } from "bun:test";
import { BADGE_SIZE, badgePosition, clampToArea } from "../src/shared/badge";

test("precise caret: badge above-right", () => {
  expect(badgePosition({ x: 100, y: 200, w: 0, h: 16, precise: true })).toEqual({ x: 103, y: 200 - BADGE_SIZE + 2 });
});
test("field frame: badge in bottom-right corner", () => {
  expect(badgePosition({ x: 100, y: 200, w: 300, h: 40, precise: false })).toEqual({ x: 100 + 300 - BADGE_SIZE - 4, y: 200 + 40 - BADGE_SIZE - 4 });
});
test("clamp keeps badge on screen", () => {
  const a = { x: 0, y: 0, width: 800, height: 600 };
  expect(clampToArea({ x: 900, y: -50 }, a)).toEqual({ x: 800 - BADGE_SIZE, y: 0 });
});
