//! 应用数据目录解析。
//!
//! Windows 下固定使用 `%LOCALAPPDATA%\KanFanOS`，与设计文档 §5.4 的缓存布局一致。

use std::path::PathBuf;

use anyhow::Result;
use tauri::Manager;

/// 目录名作为显示名称，根目录（如 `C:\`）则退回完整路径。
pub fn display_name(path: &str) -> String {
    std::path::Path::new(path)
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.to_string())
}

pub fn app_data_root(app: &tauri::AppHandle) -> Result<PathBuf> {
    #[cfg(windows)]
    if let Ok(local_app_data) = std::env::var("LOCALAPPDATA")
        && !local_app_data.is_empty()
    {
        return Ok(PathBuf::from(local_app_data).join("KanFanOS"));
    }

    Ok(app.path().app_local_data_dir()?)
}
