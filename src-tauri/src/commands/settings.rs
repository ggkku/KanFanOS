use std::{
    path::PathBuf,
    sync::atomic::{AtomicBool, Ordering},
};

use serde::Serialize;
use tauri::{AppHandle, Manager, State};

use crate::database::libraries::LibrarySummary;
use crate::paths::display_name;
use crate::settings::{self, Settings};
use crate::state::AppState;
use crate::time::now_ms;
use crate::watcher;

/// 进入全屏前窗口是否处于最大化；退出全屏时据此恢复。
///
/// Windows 在最大化状态下直接切全屏时，客户区仍停留在工作区高度，
/// 全屏底部会留下任务栏高度的黑边，所以必须先还原窗口。
static RESTORE_MAXIMIZED: AtomicBool = AtomicBool::new(false);

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppSettings {
    pub library_root: Option<String>,
    pub library_id: Option<i64>,
    /// 应用数据根目录，前端用它拼接缩略图路径。
    pub data_root: String,
    pub auto_watch: bool,
    pub auto_start: bool,
    pub thumbnail_width: u32,
    /// 页面切换动画：`fade` 或 `slide`。
    pub page_transition: String,
    /// 打开番剧时的过渡动画：`rise` / `zoom` / `slide` / `flip`。
    pub series_transition: String,
    /// 主窗口模式：`windowed` / `borderless`。
    pub window_mode: String,
    /// FFprobe / FFmpeg 是否可用；前端据此提示缩略图与时长无法生成。
    pub tools_ready: bool,
}

#[tauri::command]
pub fn get_settings(state: State<'_, AppState>) -> Result<AppSettings, String> {
    let settings = settings::load(&state.data_root).map_err(to_message)?;
    to_app_settings(state.inner(), settings)
}

#[tauri::command]
pub fn set_library_root(
    app: AppHandle,
    state: State<'_, AppState>,
    path: String,
) -> Result<LibrarySummary, String> {
    let canonical = dunce::canonicalize(&path)
        .map_err(|error| format!("无法访问所选目录：{error}"))?;

    if !canonical.is_dir() {
        return Err("所选路径不是目录".to_string());
    }

    let root_path = canonical.to_string_lossy().into_owned();
    let library = state
        .db
        .add_library(&root_path, &display_name(&root_path), now_ms())
        .map_err(to_message)?;

    let mut settings = settings::load(&state.data_root).map_err(to_message)?;
    settings.library_root = Some(root_path.clone());
    settings::save(&state.data_root, &settings).map_err(to_message)?;

    // 允许前端读取新目录下的封面图片。
    if let Err(error) = app
        .asset_protocol_scope()
        .allow_directory(&root_path, true)
    {
        eprintln!("允许访问番剧目录失败: {error}");
    }

    // 目录变化后换到新的监听目标（自动同步关闭时保持关闭）。
    if settings.auto_watch {
        watcher::start(app, library.id, PathBuf::from(root_path));
    }

    Ok(library)
}

#[tauri::command]
pub fn set_auto_watch(
    app: AppHandle,
    state: State<'_, AppState>,
    enabled: bool,
) -> Result<AppSettings, String> {
    let mut settings = settings::load(&state.data_root).map_err(to_message)?;
    settings.auto_watch = enabled;
    settings::save(&state.data_root, &settings).map_err(to_message)?;

    if enabled {
        if let Some(root) = settings.library_root.as_deref() {
            let library = state
                .db
                .add_library(root, &display_name(root), now_ms())
                .map_err(to_message)?;
            watcher::start(app, library.id, PathBuf::from(root));
        }
    } else {
        watcher::stop(&app);
    }

    to_app_settings(state.inner(), settings)
}

#[tauri::command]
pub fn set_auto_start(
    state: State<'_, AppState>,
    enabled: bool,
) -> Result<AppSettings, String> {
    // 调试模式不写注册表，避免把 target\debug\kanfanos.exe 注册成开机启动；
    // 配置仍会保存，打包版启动时按配置同步到注册表。
    if !cfg!(debug_assertions) {
        crate::autostart::set_enabled(enabled).map_err(to_message)?;
    }

    let mut settings = settings::load(&state.data_root).map_err(to_message)?;
    settings.auto_start = enabled;
    settings::save(&state.data_root, &settings).map_err(to_message)?;

    to_app_settings(state.inner(), settings)
}

#[tauri::command]
pub fn quit_app(app: AppHandle) {
    app.exit(0);
}

#[tauri::command]
pub fn set_window_mode(
    app: AppHandle,
    state: State<'_, AppState>,
    mode: String,
) -> Result<AppSettings, String> {
    let mut settings = settings::load(&state.data_root).map_err(to_message)?;
    settings.window_mode = mode;
    settings::save(&state.data_root, &settings).map_err(to_message)?;

    // 窗口操作放在 Rust 侧执行，不依赖前端窗口权限配置。
    apply_window_mode(&app, &settings.window_mode)?;

    to_app_settings(state.inner(), settings)
}

pub(crate) fn apply_window_mode(app: &AppHandle, mode: &str) -> Result<(), String> {
    let Some(window) = app.get_webview_window("main") else {
        return Ok(());
    };

    if mode == settings::WINDOW_MODE_WINDOWED {
        window.set_fullscreen(false).map_err(|error| error.to_string())?;
        window
            .set_always_on_top(false)
            .map_err(|error| error.to_string())?;
        window
            .set_skip_taskbar(false)
            .map_err(|error| error.to_string())?;
        window
            .set_decorations(true)
            .map_err(|error| error.to_string())?;

        // 某些情况下全屏/装饰切换后窗口会落在屏幕外（Windows 用
        // -16000 表示隐藏窗口），这里检查一次并居中恢复。
        let offscreen = window
            .current_monitor()
            .ok()
            .flatten()
            .map(|monitor| {
                let area = monitor.work_area();
                let position = window.outer_position().unwrap_or_default();
                let size = window.outer_size().unwrap_or_default();
                let right = position.x.saturating_add(size.width as i32);
                let bottom = position.y.saturating_add(size.height as i32);
                let monitor_right = area.position.x.saturating_add(area.size.width as i32);
                let monitor_bottom = area.position.y.saturating_add(area.size.height as i32);
                right <= area.position.x
                    || bottom <= area.position.y
                    || position.x >= monitor_right
                    || position.y >= monitor_bottom
            })
            // 窗口完全在屏幕外时可能定位不到 monitor，按异常处理。
            .unwrap_or(true);

        if offscreen {
            window
                .set_size(tauri::LogicalSize::new(1180.0, 780.0))
                .map_err(|error| error.to_string())?;
            window.center().map_err(|error| error.to_string())?;
        }

        // 从全屏返回时恢复进入前的最大化状态。
        if RESTORE_MAXIMIZED.swap(false, Ordering::SeqCst) {
            window.maximize().map_err(|error| error.to_string())?;
        }
    } else {
        // 最大化窗口直接进入全屏会保留工作区高度，底部留出任务栏黑边。
        // 先还原窗口再全屏，并记住状态供返回窗口模式时恢复。
        if !window.is_fullscreen().unwrap_or(false) {
            let was_maximized = window.is_maximized().unwrap_or(false);
            if was_maximized {
                window.unmaximize().map_err(|error| error.to_string())?;
            }
            RESTORE_MAXIMIZED.store(was_maximized, Ordering::SeqCst);

            // 必须在进入全屏前去掉装饰。先全屏再 set_decorations
            // 在最大化状态下会让窗口丢失位置和尺寸（落到屏幕外）。
            window
                .set_decorations(false)
                .map_err(|error| error.to_string())?;
        }

        window.set_fullscreen(true).map_err(|error| error.to_string())?;
        window
            .set_always_on_top(false)
            .map_err(|error| error.to_string())?;
        window
            .set_skip_taskbar(false)
            .map_err(|error| error.to_string())?;
        let _ = window.set_focus();
    }

    Ok(())
}

#[tauri::command]
pub fn set_series_transition(
    state: State<'_, AppState>,
    transition: String,
) -> Result<AppSettings, String> {
    let mut settings = settings::load(&state.data_root).map_err(to_message)?;
    settings.series_transition = transition;
    settings::save(&state.data_root, &settings).map_err(to_message)?;

    to_app_settings(state.inner(), settings)
}

#[tauri::command]
pub fn set_page_transition(
    state: State<'_, AppState>,
    transition: String,
) -> Result<AppSettings, String> {
    let mut settings = settings::load(&state.data_root).map_err(to_message)?;
    settings.page_transition = transition;
    settings::save(&state.data_root, &settings).map_err(to_message)?;

    to_app_settings(state.inner(), settings)
}

#[tauri::command]
pub fn set_thumbnail_width(
    state: State<'_, AppState>,
    width: u32,
) -> Result<AppSettings, String> {
    let mut settings = settings::load(&state.data_root).map_err(to_message)?;
    settings.thumbnail_width =
        width.clamp(settings::MIN_THUMBNAIL_WIDTH, settings::MAX_THUMBNAIL_WIDTH);
    settings::save(&state.data_root, &settings).map_err(to_message)?;

    to_app_settings(state.inner(), settings)
}

fn to_app_settings(state: &AppState, settings: Settings) -> Result<AppSettings, String> {
    // 数据库重建后，配置里的目录仍然是权威来源。
    let library_id = match settings.library_root.as_deref() {
        Some(root) => Some(
            state
                .db
                .add_library(root, &display_name(root), now_ms())
                .map_err(to_message)?
                .id,
        ),
        None => None,
    };

    Ok(AppSettings {
        library_root: settings.library_root,
        library_id,
        data_root: state.data_root.display().to_string(),
        auto_watch: settings.auto_watch,
        auto_start: settings.auto_start,
        thumbnail_width: settings.thumbnail_width,
        page_transition: settings.page_transition,
        series_transition: settings.series_transition,
        window_mode: settings.window_mode,
        tools_ready: crate::tools::available("ffprobe") && crate::tools::available("ffmpeg"),
    })
}

fn to_message(error: anyhow::Error) -> String {
    format!("{error:#}")
}
