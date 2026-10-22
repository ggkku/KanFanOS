use tauri::State;

use crate::state::AppState;
use crate::time::now_ms;

/// 一键把整部番剧（含二级选集）标记为已看完。
#[tauri::command]
pub fn mark_series_watched(
    state: State<'_, AppState>,
    series_id: i64,
) -> Result<usize, String> {
    state
        .db
        .mark_series_completed(series_id, now_ms())
        .map_err(|error| format!("{error:#}"))
}

/// 一键清除整部番剧（含二级选集）的观看记录。
#[tauri::command]
pub fn clear_series_history(
    state: State<'_, AppState>,
    series_id: i64,
) -> Result<usize, String> {
    state
        .db
        .clear_series_history(series_id)
        .map_err(|error| format!("{error:#}"))
}

/// 手动设置「看过遍数」，clamp 到 >= 0。
#[tauri::command]
pub fn set_series_watch_count(
    state: State<'_, AppState>,
    series_id: i64,
    count: i64,
) -> Result<i64, String> {
    state
        .db
        .set_series_watch_count(series_id, count)
        .map_err(|error| format!("{error:#}"))
}
