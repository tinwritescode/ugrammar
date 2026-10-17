use tauri::{AppHandle, LogicalPosition, LogicalSize, Manager, WebviewUrl, WebviewWindow, WebviewWindowBuilder};
use tauri_nspanel::{tauri_panel, CollectionBehavior, ManagerExt, PanelBuilder, PanelLevel, StyleMask};

tauri_panel! {
    panel!(FloatPanel {
        config: {
            can_become_key_window: false,
            is_floating_panel: true
        }
    })
}

/// Logical points, top-left origin: the space AX bounds and window positions share.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Area {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

const INSET: f64 = 8.0;

impl Area {
    /// Left edge that keeps a `w`-wide window inset inside this area.
    pub fn fit_x(&self, x: f64, w: f64) -> f64 {
        x.max(self.x + INSET).min(self.x + self.w - w - INSET)
    }

    pub fn fit_y(&self, y: f64, h: f64) -> f64 {
        y.max(self.y + INSET).min(self.y + self.h - h - INSET)
    }

    fn distance2(&self, x: f64, y: f64) -> f64 {
        let dx = (self.x - x).max(x - (self.x + self.w)).max(0.0);
        let dy = (self.y - y).max(y - (self.y + self.h)).max(0.0);
        dx * dx + dy * dy
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Screen {
    pub bounds: Area,
    pub work: Area,
}

const FALLBACK: Area = Area { x: 0.0, y: 0.0, w: 1440.0, h: 900.0 };

// tao reports each monitor's CG point rect multiplied by that monitor's own scale.
fn screens(app: &AppHandle) -> Vec<Screen> {
    let monitors = app.available_monitors().unwrap_or_default();
    monitors
        .iter()
        .map(|m| {
            let s = m.scale_factor();
            let (p, z, wa) = (m.position(), m.size(), m.work_area());
            Screen {
                bounds: Area { x: p.x as f64 / s, y: p.y as f64 / s, w: z.width as f64 / s, h: z.height as f64 / s },
                work: Area {
                    x: wa.position.x as f64 / s,
                    y: wa.position.y as f64 / s,
                    w: wa.size.width as f64 / s,
                    h: wa.size.height as f64 / s,
                },
            }
        })
        .collect()
}

/// The display containing the point, else the closest one.
pub fn screen_near(app: &AppHandle, x: f64, y: f64) -> Screen {
    screens(app)
        .into_iter()
        .min_by(|a, b| a.bounds.distance2(x, y).total_cmp(&b.bounds.distance2(x, y)))
        .unwrap_or(Screen { bounds: FALLBACK, work: FALLBACK })
}

fn primary_scale(app: &AppHandle) -> f64 {
    app.primary_monitor().ok().flatten().map_or(1.0, |m| m.scale_factor())
}

// tao scales the cursor (and tray rects) by the primary monitor's factor, whatever display they are on.
pub fn cursor(app: &AppHandle) -> (f64, f64) {
    let p = app.cursor_position().unwrap_or_default();
    let s = primary_scale(app);
    (p.x / s, p.y / s)
}

pub fn tray_rect(app: &AppHandle) -> Option<Area> {
    let r = app.tray_by_id("main")?.rect().ok()??;
    let s = primary_scale(app);
    let p = r.position.to_logical::<f64>(s);
    let z = r.size.to_logical::<f64>(s);
    Some(Area { x: p.x, y: p.y, w: z.width, h: z.height })
}

/// Non-activating, never-key panel: clicks reach it while the app being typed in keeps focus.
/// Main thread only.
pub fn float_panel(app: &AppHandle, label: &str, page: &str, w: f64, h: f64, shadow: bool) -> tauri::Result<WebviewWindow> {
    PanelBuilder::<_, FloatPanel>::new(app, label)
        .url(WebviewUrl::App(page.into()))
        .size(tauri::Size::Logical(LogicalSize::new(w, h)))
        .level(PanelLevel::ScreenSaver)
        .with_window(|w| w.decorations(false).visible(false).transparent(true))
        .add_style_mask(StyleMask::empty().nonactivating_panel())
        .collection_behavior(CollectionBehavior::new().can_join_all_spaces().full_screen_auxiliary())
        .transparent(true)
        .has_shadow(shadow)
        .no_activate(true)
        .build()?;
    app.get_webview_window(label).ok_or(tauri::Error::WindowNotFound)
}

/// The focusable menu-bar panel (it has inputs). Main thread only.
pub fn tray_window(app: &AppHandle, w: f64, h: f64) -> tauri::Result<WebviewWindow> {
    WebviewWindowBuilder::new(app, "tray", WebviewUrl::App("tray.html".into()))
        .inner_size(w, h)
        .visible(false)
        .decorations(false)
        .resizable(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .transparent(true)
        .shadow(true)
        .visible_on_all_workspaces(true)
        .build()
}

/// Order a panel front without activating ugrammar.
pub fn show_panel(app: &AppHandle, label: &str) {
    let (h, label) = (app.clone(), label.to_string());
    let _ = app.run_on_main_thread(move || {
        if let Ok(p) = h.get_webview_panel(&label) {
            p.show();
        }
    });
}

pub fn is_visible(app: &AppHandle, label: &str) -> bool {
    app.get_webview_window(label).is_some_and(|w| w.is_visible().unwrap_or(false))
}

pub fn hide(app: &AppHandle, label: &str) {
    if let Some(w) = app.get_webview_window(label) {
        let _ = w.hide();
    }
}

pub fn place(app: &AppHandle, label: &str, x: f64, y: f64) {
    if let Some(w) = app.get_webview_window(label) {
        let _ = w.set_position(LogicalPosition::new(x.round(), y.round()));
    }
}

pub fn resize(app: &AppHandle, label: &str, w: f64, h: f64) {
    if let Some(win) = app.get_webview_window(label) {
        let _ = win.set_size(LogicalSize::new(w, h));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn distance_is_zero_inside_and_grows_outside() {
        let a = Area { x: 0.0, y: 0.0, w: 100.0, h: 50.0 };
        assert_eq!(a.distance2(10.0, 10.0), 0.0);
        assert_eq!(a.distance2(103.0, 54.0), 25.0);
        assert_eq!(a.distance2(-2.0, 10.0), 4.0);
    }
}
