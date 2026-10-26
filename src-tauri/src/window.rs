//! 主窗口查找的兜底。
//!
//! 正常情况主窗口标签固定为 `main`。Windows 切换用户/桌面、Win 键导致
//! WebView2 重建时，Tauri 的 webview 窗口表里偶尔会短暂拿不到 `main`；
//! 这里优先用旧的 `get_window`（浏览器/播放器一直可用），再回退到
//! webview 窗口表，最后按“唯一一个非浏览器子 webview 的顶层窗口”兜底。

use tauri::{AppHandle, Manager, Window};

pub fn main_window(app: &AppHandle) -> Option<Window> {
    if let Some(window) = app.get_window("main") {
        return Some(window);
    }

    if let Some(window) = app.get_webview_window("main") {
        return Some(window.as_ref().window());
    }

    let mut candidates = app
        .webview_windows()
        .into_values()
        .filter(|window| !window.label().starts_with("browser-"));

    let first = candidates.next()?;
    if candidates.next().is_some() {
        return None;
    }
    Some(first.as_ref().window())
}
