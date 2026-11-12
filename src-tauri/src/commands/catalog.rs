use std::collections::HashMap;

use serde::Serialize;
use tauri::State;

use crate::database::series::{
    CategorySummary, EpisodeSummary, SearchPage, SeriesPage, SeriesSummary,
};
use crate::episode_names::{self, DuplicateEpisode};
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
pub fn get_series(
    state: State<'_, AppState>,
    series_id: i64,
) -> Result<SeriesSummary, String> {
    state
        .db
        .get_series(series_id)
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

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SeriesEpisodeIssues {
    pub series_id: i64,
    /// 同一集存在多个文件的集数。
    pub duplicate_episodes: Vec<DuplicateEpisode>,
    /// 最小集数与最大集数之间缺失的集数。
    pub missing_episodes: Vec<i64>,
}

/// 按文件名分析番剧的集数完整性（重复/中间缺集）。
///
/// 与 RSS 追番和自动下载完全无关，只看媒体库里已经存在的视频文件。
#[tauri::command(async)]
pub fn series_episode_issues(
    state: State<'_, AppState>,
    library_id: i64,
) -> Result<Vec<SeriesEpisodeIssues>, String> {
    let rows = state
        .db
        .library_main_video_filenames(library_id)
        .map_err(|error| format!("{error:#}"))?;
    let mut by_series: HashMap<i64, Vec<String>> = HashMap::new();
    for (series_id, filename) in rows {
        by_series.entry(series_id).or_default().push(filename);
    }

    let mut issues = Vec::new();
    for (series_id, files) in by_series {
        let names: Vec<&str> = files.iter().map(String::as_str).collect();
        let analysis = episode_names::analyze(names);
        if analysis.duplicates.is_empty() && analysis.missing.is_empty() {
            continue;
        }
        issues.push(SeriesEpisodeIssues {
            series_id,
            duplicate_episodes: analysis.duplicates,
            missing_episodes: analysis.missing,
        });
    }
    issues.sort_by_key(|item| item.series_id);
    Ok(issues)
}

#[tauri::command]
pub fn list_extra_episodes(
    state: State<'_, AppState>,
    series_id: i64,
) -> Result<Vec<EpisodeSummary>, String> {
    state
        .db
        .list_extra_episodes(series_id)
        .map_err(|error| format!("{error:#}"))
}
