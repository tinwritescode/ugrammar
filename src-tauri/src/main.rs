mod app;
mod autocheck;
mod commands;
mod deeplink;
mod flow;
mod helper;
mod ipc;
mod ui;

use app::{accept_shortcut, ctx, dismiss_shortcut, CTX, POPOVER_W, TRAY_W};
use ipc::Choice;
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{ActivationPolicy, Theme, WindowEvent};
use tauri_plugin_global_shortcut::ShortcutState;

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|_, _, _| {
            // A second launch can arrive before setup has built the context.
            if let Some(c) = CTX.get() {
                tauri::async_runtime::spawn(c.open_panel());
            }
        }))
        .plugin(tauri_plugin_deep_link::init())
        .plugin(tauri_nspanel::init())
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(|_, shortcut, event| {
                    if event.state != ShortcutState::Pressed {
                        return;
                    }
                    if *shortcut == accept_shortcut() {
                        ctx().close_popover(Choice::Accept);
                    } else if *shortcut == dismiss_shortcut() {
                        ctx().close_popover(Choice::Dismiss);
                    }
                })
                .build(),
        )
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .invoke_handler(tauri::generate_handler![
            commands::popover_action,
            commands::popover_resize,
            commands::popover_current,
            commands::check_update_now,
            commands::clear_insights,
            commands::save_settings,
            commands::set_api_key,
            commands::test_connection,
            commands::pick_app,
            commands::get_tray,
            commands::tray_action,
            commands::tray_resize,
        ])
        .setup(|app| {
            app.set_activation_policy(ActivationPolicy::Accessory);
            let h = app.handle();
            let events = app::init(h);
            tauri::async_runtime::spawn(ctx().helper_events(events));

            ui::float_panel(h, "popover", "popover.html", POPOVER_W, 200.0, true)?;
            let tray = ui::tray_window(h, TRAY_W, 400.0)?;
            tray.on_window_event(|e| {
                if let WindowEvent::Focused(false) = e {
                    ctx().on_tray_blur();
                }
            });
            let theme = match std::env::var("UGRAMMAR_THEME").as_deref() {
                Ok("dark") => Some(Theme::Dark),
                Ok("light") => Some(Theme::Light),
                _ => None,
            };
            if theme.is_some() {
                app.set_theme(theme);
            }

            TrayIconBuilder::with_id("main")
                .title("ug")
                .show_menu_on_left_click(false)
                .on_tray_icon_event(|_, e| {
                    if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = e {
                        tauri::async_runtime::spawn(ctx().toggle_tray());
                    }
                })
                .build(app)?;
            app.on_menu_event(|_, e| {
                let id = e.id().0.clone();
                tauri::async_runtime::spawn(async move { ctx().on_menu(&id).await });
            });
            tauri::async_runtime::spawn(ctx().start());
            deeplink::register(app);
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
