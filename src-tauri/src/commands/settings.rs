use std::path::PathBuf;

use serde::Serialize;
use tauri::{AppHandle, Manager, State};

use crate::database::libraries::LibrarySummary;
use crate::paths::display_name;
use crate::settings::{self, Settings};
use crate::state::AppState;
use crate::time::now_ms;
use crate::watcher;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppSettings {
    pub library_root: Option<String>,
    pub library_id: Option<i64>,
    /// 应用数据根目录，前端用它拼接缩略图路径。
    pub data_root: String,
    pub auto_watch: bool,
    pub thumbnail_width: u32,
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
        thumbnail_width: settings.thumbnail_width,
    })
}

fn to_message(error: anyhow::Error) -> String {
    format!("{error:#}")
}
