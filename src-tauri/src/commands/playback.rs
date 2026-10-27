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

#[tauri::command(async)]
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
pub fn set_player_cursor_visible(
    app: AppHandle,
    visible: bool,
) -> Result<(), String> {
    playback::set_cursor_visible(&app, visible).map_err(to_message)
}

#[tauri::command]
pub fn player_set_subtitle(
    app: AppHandle,
    subtitle_id: Option<i64>,
) -> Result<(), String> {
    let sid = match subtitle_id {
        Some(id) => json!(id),
        None => json!("no"),
    };

    playback::send(
        &app,
        json!({ "command": ["set_property", "sid", sid] }),
    )
    .map_err(to_message)?;

    // 之前可能通过别的途径关掉了字幕显示，选轨时一并打开。
    playback::send(
        &app,
        json!({ "command": ["set_property", "sub-visibility", true] }),
    )
    .map_err(to_message)
}

/// 正数让字幕整体延后，负数让字幕提前；单位秒。
#[tauri::command]
pub fn player_set_sub_delay(
    app: AppHandle,
    seconds: f64,
) -> Result<(), String> {
    let seconds = seconds.clamp(-60.0, 60.0);
    playback::send(
        &app,
        json!({ "command": ["set_property", "sub-delay", seconds] }),
    )
    .map_err(to_message)
}

#[tauri::command]
pub fn player_set_audio(
    app: AppHandle,
    audio_id: i64,
) -> Result<(), String> {
    playback::send(
        &app,
        json!({ "command": ["set_property", "aid", audio_id] }),
    )
    .map_err(to_message)
}

/// 切换播放器的音频输出设备。
#[tauri::command]
pub fn player_set_audio_device(
    app: AppHandle,
    device: String,
) -> Result<(), String> {
    playback::send(
        &app,
        json!({ "command": ["set_property", "audio-device", device] }),
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
