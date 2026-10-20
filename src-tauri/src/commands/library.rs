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
