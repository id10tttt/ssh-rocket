pub mod commands;
pub mod controller;
pub mod rules;
pub mod scanner;
pub mod traffic;
pub mod types;

use commands::*;
use controller::RuntimeController;
use tauri::{
    menu::{Menu, MenuItem},
    tray::TrayIconBuilder,
    Manager,
};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let controller = RuntimeController::default();

    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .manage(AppState {
            controller: controller.clone(),
        })
        .invoke_handler(tauri::generate_handler![
            get_config,
            save_config,
            get_runtime_status,
            start_service,
            stop_service,
            get_desktop_apps,
            import_rules,
            toggle_floating_window,
        ])
        .setup(|app| {
            // 系统托盘菜单
            let quit_i = MenuItem::with_id(app, "quit", "退出", true, None::<&str>)?;
            let show_i = MenuItem::with_id(app, "show", "显示主窗口", true, None::<&str>)?;
            let toggle_hud_i = MenuItem::with_id(app, "toggle_hud", "悬浮监控球", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&show_i, &toggle_hud_i, &quit_i])?;

            let _tray = TrayIconBuilder::new()
                .icon(app.default_window_icon().unwrap().clone())
                .menu(&menu)
                .on_menu_event(|app, event| match event.id.as_ref() {
                    "quit" => {
                        app.exit(0);
                    }
                    "show" => {
                        if let Some(window) = app.get_webview_window("main") {
                            let _ = window.show();
                            let _ = window.set_focus();
                        }
                    }
                    "toggle_hud" => {
                        if let Some(window) = app.get_webview_window("floating") {
                            if let Ok(vis) = window.is_visible() {
                                if vis {
                                    let _ = window.hide();
                                } else {
                                    let _ = window.show();
                                }
                            }
                        }
                    }
                    _ => {}
                })
                .build(app)?;

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
