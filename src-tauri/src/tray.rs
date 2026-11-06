//! 系统托盘。
//!
//! 设置页的「最小化到托盘」只是隐藏主窗口，应用（含内置 qBittorrent 与
//! RSS 调度）继续在后台运行；用户通过托盘图标或菜单恢复窗口、退出应用。

use tauri::{
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    AppHandle,
};

const TRAY_ID: &str = "main-tray";
const MENU_SHOW: &str = "tray-show";
const MENU_QUIT: &str = "tray-quit";

/// 恢复并聚焦主窗口。
pub fn show_main_window(app: &AppHandle) {
    if let Some(window) = crate::window::main_window(app) {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    }
}

pub fn create(app: &AppHandle) -> tauri::Result<()> {
    let show = MenuItem::with_id(app, MENU_SHOW, "显示主窗口", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, MENU_QUIT, "退出 KanFanOS", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&show, &quit])?;

    // 优先用打包配置里的应用图标；取不到时退回编译进程序的图标，
    // 避免开发模式下托盘出现空白图标。
    let icon = app
        .default_window_icon()
        .cloned()
        .unwrap_or_else(|| tauri::include_image!("icons/128x128.png"));

    TrayIconBuilder::with_id(TRAY_ID)
        .icon(icon)
        .tooltip("KanFanOS")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id().as_ref() {
            MENU_SHOW => show_main_window(app),
            MENU_QUIT => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                show_main_window(tray.app_handle());
            }
        })
        .build(app)?;

    Ok(())
}
