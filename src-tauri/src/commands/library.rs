use std::path::Path;

use serde::Serialize;
use tauri::{AppHandle, State};

use crate::scanner::{self, ScanSummary};
use crate::state::AppState;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryStats {
    pub video_count: i64,
    pub library_count: i64,
    pub database_path: String,
    pub data_root: String,
}

#[tauri::command]
pub fn get_library_stats(state: State<'_, AppState>) -> Result<LibraryStats, String> {
    let db = &state.db;

    Ok(LibraryStats {
        video_count: db.video_count().map_err(to_message)?,
        library_count: db.library_count().map_err(to_message)?,
        database_path: db.path().display().to_string(),
        data_root: state.data_root.display().to_string(),
    })
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SeriesNameConflict {
    pub name: String,
    pub count: i64,
}

#[tauri::command]
pub fn series_name_conflicts(
    state: State<'_, AppState>,
    library_id: i64,
) -> Result<Vec<SeriesNameConflict>, String> {
    state
        .db
        .series_name_conflicts(library_id)
        .map(|rows| {
            rows.into_iter()
                .map(|(name, count)| SeriesNameConflict { name, count })
                .collect()
        })
        .map_err(to_message)
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InvalidSeries {
    pub series_id: i64,
    pub name: String,
    pub path: String,
    pub missing_videos: i64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InvalidSeriesCleanup {
    pub removed_series: usize,
    pub removed_videos: usize,
}

fn invalid_series_candidates(
    state: &AppState,
    library_id: i64,
) -> Result<Vec<InvalidSeries>, String> {
    let root = state
        .db
        .library_root(library_id)
        .map_err(to_message)?;
    if !Path::new(&root).is_dir() {
        return Err(format!("媒体库目录不可用，已取消清理：{root}"));
    }
    let rows = state
        .db
        .series_without_active_videos(library_id)
        .map_err(to_message)?;
    Ok(rows
        .into_iter()
        .filter(|(_, _, path, _)| !Path::new(path).is_dir())
        .map(|(series_id, name, path, missing_videos)| InvalidSeries {
            series_id,
            name,
            path,
            missing_videos,
        })
        .collect())
}

#[tauri::command]
pub fn list_invalid_series(
    state: State<'_, AppState>,
    library_id: i64,
) -> Result<Vec<InvalidSeries>, String> {
    invalid_series_candidates(state.inner(), library_id)
}

#[tauri::command]
pub fn cleanup_invalid_series(
    state: State<'_, AppState>,
    library_id: i64,
) -> Result<InvalidSeriesCleanup, String> {
    let candidates = invalid_series_candidates(state.inner(), library_id)?;
    let ids: Vec<i64> = candidates.iter().map(|item| item.series_id).collect();
    let (removed_series, removed_videos) = state
        .db
        .delete_series_batch(&ids)
        .map_err(to_message)?;
    Ok(InvalidSeriesCleanup {
        removed_series,
        removed_videos,
    })
}

/// 扫描是阻塞操作，放到后台线程执行；进度通过事件推送。
#[tauri::command(async)]
pub fn scan_library(app: AppHandle, library_id: i64) -> Result<ScanSummary, String> {
    scanner::scan(&app, library_id).map_err(to_message)
}

#[tauri::command]
pub fn cancel_scan(state: State<'_, AppState>) -> bool {
    state.scan.request_cancel()
}

fn to_message(error: anyhow::Error) -> String {
    format!("{error:#}")
}
