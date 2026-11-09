use crate::placement::{Rect, Screen};
use tauri::window::{Effect, EffectState, EffectsBuilder};
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

// tao reports each monitor's CG point rect multiplied by that monitor's own scale.
pub fn screens(app: &AppHandle) -> Vec<Screen> {
    let monitors = app.available_monitors().unwrap_or_default();
    monitors
        .iter()
        .map(|m| {
            let s = m.scale_factor();
            let (p, z, wa) = (m.position(), m.size(), m.work_area());
            Screen {
                bounds: Rect { x: p.x as f64 / s, y: p.y as f64 / s, w: z.width as f64 / s, h: z.height as f64 / s },
                work: Rect {
                    x: wa.position.x as f64 / s,
                    y: wa.position.y as f64 / s,
                    w: wa.size.width as f64 / s,
                    h: wa.size.height as f64 / s,
                },
            }
        })
        .collect()
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

pub fn tray_rect(app: &AppHandle) -> Option<Rect> {
    let r = app.tray_by_id("main")?.rect().ok()??;
    let s = primary_scale(app);
    let p = r.position.to_logical::<f64>(s);
    let z = r.size.to_logical::<f64>(s);
    Some(Rect { x: p.x, y: p.y, w: z.width, h: z.height })
}

/// Matches the 12px corners of the tray panel and popover card in the web UI.
const CORNER_RADIUS: f64 = 18.0;

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
    let win = app.get_webview_window(label).ok_or(tauri::Error::WindowNotFound)?;
    vibrancy(&win)?;
    Ok(win)
}

/// The focusable menu-bar panel (it has inputs). Main thread only.
pub fn tray_window(app: &AppHandle, w: f64, h: f64) -> tauri::Result<WebviewWindow> {
    let win = WebviewWindowBuilder::new(app, "tray", WebviewUrl::App("tray.html".into()))
        .inner_size(w, h)
        .visible(false)
        .decorations(false)
        .resizable(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .transparent(true)
        .shadow(true)
        .visible_on_all_workspaces(true)
        .build()?;
    vibrancy(&win)?;
    Ok(win)
}

/// Native translucent material behind the page. Active even when unfocused: the popover never takes focus.
fn vibrancy(win: &WebviewWindow) -> tauri::Result<()> {
    win.set_effects(EffectsBuilder::new().effect(Effect::Popover).state(EffectState::Active).radius(CORNER_RADIUS).build())
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
