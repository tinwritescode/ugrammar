//! Where popovers and the tray panel go. Pure: anchors, sizes, screens and rects in, positions out; `ui` measures the
//! screens and moves the windows.

use crate::helper_client::Snapshot;
use crate::popover::POPOVER_W;
use serde::{Deserialize, Serialize};

/// Logical points, top-left origin: the space AX bounds and window positions share.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

const INSET: f64 = 8.0;

impl Rect {
    /// Left edge that keeps a `w`-wide window inset inside this area.
    pub fn fit_x(&self, x: f64, w: f64) -> f64 {
        x.max(self.x + INSET).min(self.x + self.w - w - INSET)
    }

    pub fn fit_y(&self, y: f64, h: f64) -> f64 {
        y.max(self.y + INSET).min(self.y + self.h - h - INSET)
    }

    pub fn contains(&self, x: f64, y: f64) -> bool {
        self.distance2(x, y) == 0.0
    }

    fn distance2(&self, x: f64, y: f64) -> f64 {
        let dx = (self.x - x).max(x - (self.x + self.w)).max(0.0);
        let dy = (self.y - y).max(y - (self.y + self.h)).max(0.0);
        dx * dx + dy * dy
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Screen {
    pub bounds: Rect,
    /// Without the menu bar and Dock.
    pub work: Rect,
}

const FALLBACK: Rect = Rect { x: 0.0, y: 0.0, w: 1440.0, h: 900.0 };

/// The display containing the point, else the closest one.
pub fn screen_near(screens: &[Screen], x: f64, y: f64) -> Screen {
    screens
        .iter()
        .copied()
        .min_by(|a, b| a.bounds.distance2(x, y).total_cmp(&b.bounds.distance2(x, y)))
        .unwrap_or(Screen { bounds: FALLBACK, work: FALLBACK })
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Anchor {
    /// Centered on `x`; `y` = top edge when placed below, `alt_y` = bottom edge when placed above; `above` tries above first.
    Text { x: f64, y: f64, alt_y: f64, above: bool },
    /// The popover's top-right corner, for text without usable bounds.
    Corner { x: f64, y: f64 },
}

pub fn below_rect(r: Rect) -> Anchor {
    Anchor::Text { x: r.x + r.w / 2.0, y: r.y + r.h + 6.0, alt_y: r.y - 6.0, above: false }
}

pub fn below_mouse((x, y): (f64, f64)) -> Anchor {
    Anchor::Text { x, y: y + 20.0, alt_y: y - 36.0, above: false }
}

/// On the anchor's preferred side if it fits, else the other; always clamped inside the display.
pub fn popover_at(a: Anchor, (w, h): (f64, f64), screens: &[Screen]) -> Rect {
    let (x, y) = match a {
        Anchor::Text { x, y, alt_y, above } => {
            let area = screen_near(screens, x, y).work;
            let fits_below = y + h <= area.y + area.h - INSET;
            let top = if (above && alt_y - h >= area.y + INSET) || !fits_below { alt_y - h } else { y };
            (area.fit_x(x - w / 2.0, w), area.fit_y(top, h))
        }
        Anchor::Corner { x, y } => {
            let area = screen_near(screens, x - 1.0, y).work;
            (area.fit_x(x - w, w), area.fit_y(y, h))
        }
    };
    Rect { x, y, w, h }
}

/// Taller than this is a text area, not a one-line field.
const FIELD_MAX_H: f64 = 80.0;

/// For an auto-check fix in app `pid`, with the snapshot that asked for its first and last character's bounds: under a
/// one-line field; else above the sentence's first line (else a line below its last); with no bounds, or the sentence
/// scrolled away, the focused window's top-right corner, else the screen's. The &str names the choice, for the log.
pub fn sentence_anchor(snap: &Snapshot, pid: Option<i64>, screens: &[Screen], mouse: (f64, f64)) -> (Anchor, &'static str) {
    let here = snap.ok && snap.visible == Some(true) && snap.pid == pid;
    if !here {
        return (screen_corner(screens, mouse), "screen");
    }
    if let Some(r) = snap.field.filter(|r| r.h > 0.0 && r.h <= FIELD_MAX_H) {
        return (Anchor::Text { x: r.x + POPOVER_W / 2.0, y: r.y + r.h + 6.0, alt_y: r.y - 6.0, above: false }, "field");
    }
    let bounds = snap.bounds.as_deref().unwrap_or_default();
    if let (Some(Some(a)), Some(Some(z))) = (bounds.first(), bounds.get(1)) {
        let shown = |r: &Rect| match snap.window {
            Some(w) => r.y >= w.y && r.y + r.h <= w.y + w.h,
            None => screen_near(screens, r.x, r.y).bounds.contains(r.x, r.y),
        };
        if shown(a) && shown(z) {
            return (Anchor::Text { x: a.x + POPOVER_W / 2.0 - 16.0, y: z.y + 2.0 * z.h + 6.0, alt_y: a.y - 6.0, above: true }, "text");
        }
    }
    match snap.window {
        Some(w) => (Anchor::Corner { x: w.x + w.w - 12.0, y: w.y + 40.0 }, "window"),
        None => (screen_corner(screens, mouse), "screen"),
    }
}

fn screen_corner(screens: &[Screen], (x, y): (f64, f64)) -> Anchor {
    let area = screen_near(screens, x, y).work;
    Anchor::Corner { x: area.x + area.w - 12.0, y: area.y + 12.0 }
}

pub const TRAY_W: f64 = 344.0;
const TRAY_MAX_H: f64 = 600.0;

/// Centered under the menu-bar icon, kept on its display.
pub fn tray_at(icon: Rect, screens: &[Screen]) -> (f64, f64) {
    let area = screen_near(screens, icon.x, icon.y).work;
    (area.fit_x(icon.x + icon.w / 2.0 - TRAY_W / 2.0, TRAY_W), area.y + 6.0)
}

/// The page's height, capped by the display under the icon; taller content scrolls inside.
pub fn tray_height(h: f64, icon: Option<Rect>, screens: &[Screen]) -> f64 {
    let (x, y) = icon.map_or((0.0, 0.0), |b| (b.x, b.y));
    h.ceil().min(TRAY_MAX_H).min(screen_near(screens, x, y).work.h - 12.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r(x: f64, y: f64, w: f64, h: f64) -> Rect {
        Rect { x, y, w, h }
    }

    /// 1440×900 with a 25pt menu bar, and a second display to its right.
    fn screens() -> Vec<Screen> {
        vec![
            Screen { bounds: r(0.0, 0.0, 1440.0, 900.0), work: r(0.0, 25.0, 1440.0, 875.0) },
            Screen { bounds: r(1440.0, 0.0, 1920.0, 1080.0), work: r(1440.0, 25.0, 1920.0, 1055.0) },
        ]
    }

    #[test]
    fn distance_is_zero_inside_and_grows_outside() {
        let a = r(0.0, 0.0, 100.0, 50.0);
        assert_eq!(a.distance2(10.0, 10.0), 0.0);
        assert_eq!(a.distance2(103.0, 54.0), 25.0);
        assert_eq!(a.distance2(-2.0, 10.0), 4.0);
    }

    #[test]
    fn nearest_screen_or_a_fallback() {
        assert_eq!(screen_near(&screens(), 2000.0, 10.0).bounds.x, 1440.0);
        assert_eq!(screen_near(&screens(), -50.0, 10.0).bounds.x, 0.0);
        assert_eq!(screen_near(&[], 0.0, 0.0).bounds, FALLBACK);
    }

    #[test]
    fn popover_goes_below_and_flips_above_when_it_does_not_fit() {
        let below = Anchor::Text { x: 500.0, y: 300.0, alt_y: 280.0, above: false };
        assert_eq!(popover_at(below, (200.0, 100.0), &screens()), r(400.0, 300.0, 200.0, 100.0));
        let low = Anchor::Text { x: 500.0, y: 850.0, alt_y: 830.0, above: false };
        assert_eq!(popover_at(low, (200.0, 100.0), &screens()).y, 730.0);
        let prefers_above = Anchor::Text { x: 500.0, y: 300.0, alt_y: 280.0, above: true };
        assert_eq!(popover_at(prefers_above, (200.0, 100.0), &screens()).y, 180.0);
        let no_room_above = Anchor::Text { x: 500.0, y: 300.0, alt_y: 60.0, above: true };
        assert_eq!(popover_at(no_room_above, (200.0, 100.0), &screens()).y, 300.0);
    }

    #[test]
    fn popover_is_clamped_inside_its_display() {
        let edge = Anchor::Text { x: 1430.0, y: 300.0, alt_y: 280.0, above: false };
        assert_eq!(popover_at(edge, (200.0, 100.0), &screens()).x, 1440.0 - 200.0 - INSET);
        let corner = Anchor::Corner { x: 3360.0 - 12.0, y: 37.0 };
        assert_eq!(popover_at(corner, (460.0, 100.0), &screens()), r(3360.0 - 12.0 - 460.0, 37.0, 460.0, 100.0));
    }

    fn snap(pid: i64) -> Snapshot {
        Snapshot { ok: true, visible: Some(true), pid: Some(pid), window: Some(r(100.0, 100.0, 800.0, 600.0)), ..Default::default() }
    }

    #[test]
    fn sentence_anchor_falls_back_field_text_window_screen() {
        let s = screens();
        let field = Snapshot { field: Some(r(10.0, 50.0, 300.0, 24.0)), ..snap(7) };
        assert_eq!(sentence_anchor(&field, Some(7), &s, (0.0, 0.0)).1, "field");

        let a = Some(r(200.0, 300.0, 8.0, 16.0));
        let text = Snapshot { bounds: Some(vec![a, Some(r(400.0, 320.0, 8.0, 16.0))]), ..snap(7) };
        let (anchor, why) = sentence_anchor(&text, Some(7), &s, (0.0, 0.0));
        assert_eq!(why, "text");
        assert_eq!(anchor, Anchor::Text { x: 200.0 + POPOVER_W / 2.0 - 16.0, y: 320.0 + 32.0 + 6.0, alt_y: 294.0, above: true });

        let scrolled = Snapshot { bounds: Some(vec![a, Some(r(400.0, 900.0, 8.0, 16.0))]), ..snap(7) };
        assert_eq!(sentence_anchor(&scrolled, Some(7), &s, (0.0, 0.0)), (Anchor::Corner { x: 888.0, y: 140.0 }, "window"));

        let other_app = snap(8);
        assert_eq!(sentence_anchor(&other_app, Some(7), &s, (2000.0, 10.0)), (Anchor::Corner { x: 3348.0, y: 37.0 }, "screen"));
    }

    #[test]
    fn tray_sits_under_its_icon_and_fits_the_display() {
        let icon = r(1300.0, 0.0, 30.0, 24.0);
        assert_eq!(tray_at(icon, &screens()), (1440.0 - TRAY_W - INSET, 31.0));
        assert_eq!(tray_height(450.3, Some(icon), &screens()), 451.0);
        assert_eq!(tray_height(5000.0, Some(icon), &screens()), TRAY_MAX_H);
    }
}
