use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
pub struct CaretRect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
    pub precise: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Point {
    pub x: f64,
    pub y: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
pub struct Area {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

pub const BADGE_SIZE: f64 = 22.0;

/// `Math.round`: halves go toward +∞, unlike `f64::round`.
fn js_round(v: f64) -> f64 {
    let f = v.floor();
    if v - f >= 0.5 {
        f + 1.0
    } else {
        f
    }
}

/// Top-left of the badge in screen coords: just above-right of a real caret, else the bottom-right corner inside the field.
pub fn badge_position(r: CaretRect) -> Point {
    if r.precise {
        return Point { x: js_round(r.x + r.w + 3.0), y: js_round(r.y - BADGE_SIZE + 2.0) };
    }
    Point { x: js_round(r.x + r.w - BADGE_SIZE - 4.0), y: js_round(r.y + r.h - BADGE_SIZE - 4.0) }
}

/// Clamp into a display's work area so the badge never lands off-screen.
pub fn clamp_to_area(p: Point, a: Area) -> Point {
    Point {
        x: p.x.max(a.x).min(a.x + a.width - BADGE_SIZE),
        y: p.y.max(a.y).min(a.y + a.height - BADGE_SIZE),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn precise_caret_badge_above_right() {
        let p = badge_position(CaretRect { x: 100.0, y: 200.0, w: 0.0, h: 16.0, precise: true });
        assert_eq!(p, Point { x: 103.0, y: 200.0 - BADGE_SIZE + 2.0 });
    }

    #[test]
    fn field_frame_badge_in_bottom_right_corner() {
        let p = badge_position(CaretRect { x: 100.0, y: 200.0, w: 300.0, h: 40.0, precise: false });
        assert_eq!(p, Point { x: 100.0 + 300.0 - BADGE_SIZE - 4.0, y: 200.0 + 40.0 - BADGE_SIZE - 4.0 });
    }

    #[test]
    fn clamp_keeps_badge_on_screen() {
        let a = Area { x: 0.0, y: 0.0, width: 800.0, height: 600.0 };
        assert_eq!(clamp_to_area(Point { x: 900.0, y: -50.0 }, a), Point { x: 800.0 - BADGE_SIZE, y: 0.0 });
    }

    #[test]
    fn rounds_like_math_round() {
        assert_eq!(js_round(-2.5), -2.0);
        assert_eq!(js_round(2.5), 3.0);
        assert_eq!(js_round(0.49999999999999994), 0.0);
    }
}
