use std::path::Path;

use tauri::{AppHandle, State};

use crate::database::history::RecentItem;
use crate::playback::mpv;
use crate::state::AppState;
use crate::time::now_ms;

#[tauri::command]
pub fn open_episode(
    app: AppHandle,
    state: State<'_, AppState>,
    episode_id: i64,
) -> Result<(), String> {
    let episode = state
        .db
        .playable_episode(episode_id)
        .map_err(to_message)?;

    if !Path::new(&episode.path).is_file() {
        return Err(format!("文件不存在：{}", episode.path));
    }

    let (last_position, completed) =
        state.db.resume_position(episode_id).map_err(to_message)?;

    // 看完过就从头播放，否则从上次位置继续。
    let start_ms = if completed { 0 } else { last_position };
    let pipe = mpv::pipe_name(episode_id);

    mpv::spawn(&episode.path, start_ms, &pipe).map_err(to_message)?;
    state
        .db
        .record_episode_opened(episode_id, now_ms())
        .map_err(to_message)?;
    mpv::track_progress(app, episode_id, episode.duration_ms, pipe);

    Ok(())
}

#[tauri::command]
pub fn list_recent(
    state: State<'_, AppState>,
    library_id: i64,
    limit: i64,
) -> Result<Vec<RecentItem>, String> {
    state
        .db
        .recent_items(library_id, limit.clamp(1, 200))
        .map_err(to_message)
}

fn to_message(error: anyhow::Error) -> String {
    format!("{error:#}")
}
