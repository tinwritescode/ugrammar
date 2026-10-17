export interface CaretRect { x: number; y: number; w: number; h: number; precise: boolean }

export const BADGE_SIZE = 22;

/** Top-left of the badge in screen coords: just above-right of a real caret, else the bottom-right corner inside the field. */
export function badgePosition(r: CaretRect): { x: number; y: number } {
  if (r.precise) return { x: Math.round(r.x + r.w + 3), y: Math.round(r.y - BADGE_SIZE + 2) };
  return { x: Math.round(r.x + r.w - BADGE_SIZE - 4), y: Math.round(r.y + r.h - BADGE_SIZE - 4) };
}

/** Clamp into a display's work area so the badge never lands off-screen. */
export function clampToArea(p: { x: number; y: number }, a: { x: number; y: number; width: number; height: number }) {
  return {
    x: Math.min(Math.max(p.x, a.x), a.x + a.width - BADGE_SIZE),
    y: Math.min(Math.max(p.y, a.y), a.y + a.height - BADGE_SIZE),
  };
}
