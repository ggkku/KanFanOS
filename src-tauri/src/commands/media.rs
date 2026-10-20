use tauri::{AppHandle, Manager, State};

use crate::database::videos::MediaStats;
use crate::media;
use crate::state::AppState;

#[tauri::command]
pub fn start_media_processing(
    app: AppHandle,
    library_id: i64,
) -> Result<MediaStats, String> {
    media::start(app.clone(), library_id).map_err(to_message)?;

    let state = app.state::<AppState>();
    state.db.media_stats(library_id).map_err(to_message)
}

#[tauri::command]
pub fn cancel_media_processing(state: State<'_, AppState>) -> bool {
    state.media.request_cancel()
}

#[tauri::command]
pub fn get_media_stats(
    state: State<'_, AppState>,
    library_id: i64,
) -> Result<MediaStats, String> {
    state.db.media_stats(library_id).map_err(to_message)
}

fn to_message(error: anyhow::Error) -> String {
    format!("{error:#}")
}
