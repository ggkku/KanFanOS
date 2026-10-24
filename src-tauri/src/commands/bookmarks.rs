//! 收藏夹命令。所有写操作都返回更新后的完整列表，前端直接替换状态。

use tauri::State;

use crate::{
    bookmarks::{self, Bookmark},
    state::AppState,
};

#[tauri::command]
pub fn list_bookmarks(state: State<'_, AppState>) -> Result<Vec<Bookmark>, String> {
    bookmarks::list(&state.data_root).map_err(to_message)
}

#[tauri::command]
pub fn add_bookmark(
    state: State<'_, AppState>,
    title: String,
    url: String,
) -> Result<Vec<Bookmark>, String> {
    bookmarks::add(&state.data_root, &title, &url).map_err(to_message)
}

#[tauri::command]
pub fn update_bookmark(
    state: State<'_, AppState>,
    id: u64,
    title: String,
    url: String,
) -> Result<Vec<Bookmark>, String> {
    bookmarks::update(&state.data_root, id, &title, &url).map_err(to_message)
}

#[tauri::command]
pub fn remove_bookmark(
    state: State<'_, AppState>,
    id: u64,
) -> Result<Vec<Bookmark>, String> {
    bookmarks::remove(&state.data_root, id).map_err(to_message)
}

fn to_message(error: anyhow::Error) -> String {
    format!("{error:#}")
}
