use std::path::Path;

use serde_json::json;
use tauri::{AppHandle, Manager, State};

use crate::database::history::RecentItem;
use crate::playback::{self, PlayerRect, PlayerStarted};
use crate::state::AppState;

#[tauri::command(async)]
pub fn start_player(
    app: AppHandle,
    episode_id: i64,
    rect: PlayerRect,
) -> Result<PlayerStarted, String> {
    let state = app.state::<AppState>();
    let episode = state
        .db
        .playable_episode(episode_id)
        .map_err(to_message)?;

    if !Path::new(&episode.path).is_file() {
        return Err(format!("文件不存在：{}", episode.path));
    }

    playback::start(&app, &episode, episode_id, rect).map_err(to_message)
}

#[tauri::command]
pub fn stop_player(app: AppHandle) {
    playback::stop(&app);
}

#[tauri::command]
pub fn resize_player(
    app: AppHandle,
    rect: PlayerRect,
) -> Result<(), String> {
    playback::resize(&app, rect).map_err(to_message)
}

#[tauri::command]
pub fn player_set_paused(
    app: AppHandle,
    paused: bool,
) -> Result<(), String> {
    playback::send(
        &app,
        json!({ "command": ["set_property", "pause", paused] }),
    )
    .map_err(to_message)
}

#[tauri::command]
pub fn player_seek(
    app: AppHandle,
    position_ms: i64,
) -> Result<(), String> {
    let seconds = position_ms.max(0) as f64 / 1000.0;
    playback::send(
        &app,
        json!({ "command": ["seek", seconds, "absolute"] }),
    )
    .map_err(to_message)
}

#[tauri::command]
pub fn player_set_volume(
    app: AppHandle,
    volume: f64,
) -> Result<(), String> {
    playback::send(
        &app,
        json!({ "command": ["set_property", "volume", volume.clamp(0.0, 100.0)] }),
    )
    .map_err(to_message)
}

#[tauri::command]
pub fn player_set_muted(
    app: AppHandle,
    muted: bool,
) -> Result<(), String> {
    playback::send(
        &app,
        json!({ "command": ["set_property", "mute", muted] }),
    )
    .map_err(to_message)
}

#[tauri::command]
pub fn player_set_speed(
    app: AppHandle,
    speed: f64,
) -> Result<(), String> {
    playback::send(
        &app,
        json!({ "command": ["set_property", "speed", speed.clamp(0.25, 4.0)] }),
    )
    .map_err(to_message)
}

#[tauri::command]
pub fn set_player_fullscreen(
    app: AppHandle,
    fullscreen: bool,
) -> Result<(), String> {
    let window = app
        .get_webview_window("main")
        .ok_or_else(|| "找不到主窗口".to_string())?;

    window
        .set_fullscreen(fullscreen)
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn player_toggle_subtitles(app: AppHandle) -> Result<(), String> {
    playback::send(
        &app,
        json!({ "command": ["cycle", "sub-visibility"] }),
    )
    .map_err(to_message)
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
