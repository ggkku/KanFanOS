use tauri::State;

use crate::database::series::{
    CategorySummary, EpisodeSummary, SearchPage, SeriesPage,
};
use crate::state::AppState;

#[tauri::command]
pub fn list_categories(
    state: State<'_, AppState>,
    library_id: i64,
) -> Result<Vec<CategorySummary>, String> {
    state
        .db
        .list_categories(library_id)
        .map_err(|error| format!("{error:#}"))
}

#[tauri::command]
pub fn list_series(
    state: State<'_, AppState>,
    library_id: i64,
    category: String,
    sort: String,
    offset: i64,
    limit: i64,
) -> Result<SeriesPage, String> {
    state
        .db
        .list_series_page(library_id, &category, &sort, limit, offset)
        .map_err(|error| format!("{error:#}"))
}

#[tauri::command]
pub fn search_series(
    state: State<'_, AppState>,
    library_id: i64,
    query: String,
    offset: i64,
    limit: i64,
) -> Result<SearchPage, String> {
    state
        .db
        .search_series(library_id, &query, limit, offset)
        .map_err(|error| format!("{error:#}"))
}

#[tauri::command]
pub fn list_episodes(
    state: State<'_, AppState>,
    series_id: i64,
) -> Result<Vec<EpisodeSummary>, String> {
    state
        .db
        .list_episodes(series_id)
        .map_err(|error| format!("{error:#}"))
}
