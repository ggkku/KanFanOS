use std::{fs, path::Path};

use serde::Serialize;
use tauri::State;
use walkdir::WalkDir;

use crate::state::AppState;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CacheStats {
    pub files: u64,
    pub bytes: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CacheCleanup {
    pub removed_files: u64,
    pub removed_bytes: u64,
}

#[tauri::command]
pub fn get_cache_stats(state: State<'_, AppState>) -> Result<CacheStats, String> {
    Ok(stats(&thumbnail_dir(&state)))
}

#[tauri::command]
pub fn clear_thumbnail_cache(
    state: State<'_, AppState>,
) -> Result<CacheCleanup, String> {
    let dir = thumbnail_dir(&state);
    let before = stats(&dir);

    if dir.exists() {
        fs::remove_dir_all(&dir)
            .map_err(|error| format!("清理缩略图缓存失败：{error}"))?;
    }
    fs::create_dir_all(&dir)
        .map_err(|error| format!("重建缓存目录失败：{error}"))?;

    state
        .db
        .reset_thumbnails()
        .map_err(|error| format!("{error:#}"))?;

    Ok(CacheCleanup {
        removed_files: before.files,
        removed_bytes: before.bytes,
    })
}

fn thumbnail_dir(state: &AppState) -> std::path::PathBuf {
    state.data_root.join("thumbnails")
}

fn stats(dir: &Path) -> CacheStats {
    let mut result = CacheStats {
        files: 0,
        bytes: 0,
    };

    if !dir.exists() {
        return result;
    }

    for entry in WalkDir::new(dir).into_iter().flatten() {
        if entry.file_type().is_file() {
            result.files += 1;
            if let Ok(metadata) = entry.metadata() {
                result.bytes += metadata.len();
            }
        }
    }

    result
}
