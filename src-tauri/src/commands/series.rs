//! 选集页「配置」弹窗：显示名、别名、上一季 / 下一季链接。
//!
//! 季链接是双向的：设置 A 的上一季为 B 时，同时把 B 的下一季设为 A；
//! 改动或清除链接时会解除旧配对。目标已有不同的反向链接时先由前端确认。

use std::path::{Path, PathBuf};

use serde::Serialize;
use tauri::State;

use crate::{
    catalog,
    database::series::SeriesOption,
    series_config::{self, SeriesConfig},
    state::AppState,
    time::now_ms,
};

#[derive(Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigRepairSummary {
    pub checked: usize,
    pub created: usize,
    pub updated: usize,
    pub names_cleared: usize,
    pub invalid: usize,
    pub errors: usize,
}

/// 目标的 `next` 字段需要与当前番剧同步（当前番剧设置的是「上一季」）。
const REVERSE_NEXT: &str = "next";
/// 目标的 `previous` 字段需要与当前番剧同步（当前番剧设置的是「下一季」）。
const REVERSE_PREVIOUS: &str = "previous";

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SeriesConfigView {
    /// 配置文件里的显示名称；空字符串表示使用文件夹名。
    pub name: String,
    pub aliases: Vec<String>,
    /// 0-10；None 表示未评分。
    pub rating: Option<i64>,
    pub awards: Vec<String>,
    pub previous: Option<SeriesOption>,
    pub next: Option<SeriesOption>,
    /// `auto` 或 `manual`。
    pub episode_mode: String,
    /// manual 模式下的选集文件（相对番剧目录）。
    pub episodes: Vec<String>,
}

/// 选集选择器里的一个视频。
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SeriesVideoView {
    pub path: String,
    pub filename: String,
    /// 相对番剧目录、`/` 分隔。
    pub relative_path: String,
    pub kind: String,
}

/// 自动同步反向链接时会覆盖目标已有的其它链接，需要用户确认。
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SeasonLinkConflict {
    /// 反向链接所在的番剧。
    pub target: SeriesOption,
    /// 需要修改目标哪一侧：`next`（同步「上一季」）或 `previous`（同步「下一季」）。
    pub direction: String,
    /// 目标当前指向的番剧；目标失效时为 None。
    pub current: Option<SeriesOption>,
}

#[tauri::command]
pub fn get_series_config(
    state: State<'_, AppState>,
    series_id: i64,
) -> Result<SeriesConfigView, String> {
    config_view(&state, series_id)
}

/// 季链接候选：按关键词搜索整个库，始终带 limit。
#[tauri::command]
pub fn search_series_options(
    state: State<'_, AppState>,
    library_id: i64,
    query: String,
    limit: i64,
) -> Result<Vec<SeriesOption>, String> {
    state
        .db
        .search_series_options(library_id, query.trim(), limit)
        .map_err(to_message)
}

/// 保存前检查：本次改动的季链接会不会覆盖目标已有的反向链接。
/// 返回空数组表示可以直接同步，不需要打扰用户。
#[tauri::command]
pub fn check_season_links(
    state: State<'_, AppState>,
    series_id: i64,
    previous_id: Option<i64>,
    next_id: Option<i64>,
) -> Result<Vec<SeasonLinkConflict>, String> {
    let series = state.db.get_series(series_id).map_err(to_message)?;
    let config = series_config::load(Path::new(&series.path));
    let new_previous = target_path(&state, series_id, previous_id)?;
    let new_next = target_path(&state, series_id, next_id)?;

    let mut conflicts = Vec::new();

    // 只有真正改动的链接才检查；重复保存不会反复弹窗。
    if !same_path(&new_previous, config.previous.trim()) {
        collect_conflict(
            &state,
            &new_previous,
            REVERSE_NEXT,
            &series.path,
            &mut conflicts,
        )?;
    }
    if !same_path(&new_next, config.next.trim()) {
        collect_conflict(
            &state,
            &new_next,
            REVERSE_PREVIOUS,
            &series.path,
            &mut conflicts,
        )?;
    }

    Ok(conflicts)
}

/// 设置个人评分；写入 kanfanos.toml，并同步数据库索引。
#[tauri::command]
pub fn set_series_rating(
    state: State<'_, AppState>,
    series_id: i64,
    rating: Option<i64>,
) -> Result<Option<i64>, String> {
    if let Some(value) = rating
        && !(0..=10).contains(&value)
    {
        return Err("评分必须在 0 到 10 之间".to_string());
    }

    let series = state.db.get_series(series_id).map_err(to_message)?;
    let dir = PathBuf::from(&series.path);
    let mut config = series_config::load(&dir);
    config.rating = rating.unwrap_or(series_config::UNRATED);

    series_config::save(&dir, &config)
        .map_err(|error| format!("写入番剧配置失败: {error}"))?;
    state
        .db
        .set_series_rating(series_id, rating)
        .map_err(to_message)
}

/// 保存奖项列表；奖项只存在 kanfanos.toml 中。
#[tauri::command]
pub fn set_series_awards(
    state: State<'_, AppState>,
    series_id: i64,
    awards: Vec<String>,
) -> Result<Vec<String>, String> {
    let series = state.db.get_series(series_id).map_err(to_message)?;
    let dir = PathBuf::from(&series.path);
    let mut config = series_config::load(&dir);

    let cleaned = series_config::clean_list(&awards);
    config.awards = cleaned.clone();

    series_config::save(&dir, &config)
        .map_err(|error| format!("写入番剧配置失败: {error}"))?;
    Ok(cleaned)
}

/// 卡片悬停时按需读取奖项；奖项只存在 kanfanos.toml，不额外建索引。
#[tauri::command]
pub fn get_series_awards(
    state: State<'_, AppState>,
    series_id: i64,
) -> Result<Vec<String>, String> {
    let series = state.db.get_series(series_id).map_err(to_message)?;
    let config = series_config::load(Path::new(&series.path));
    Ok(series_config::clean_list(&config.awards))
}

/// 批量检查并修复番剧配置：
/// 1. 名称与文件夹名相同时清空名称，让显示名跟随文件夹；
/// 2. 补齐旧配置缺少的字段；
/// 3. 解析失败的文件先备份再重建；
/// 4. 修复后同步数据库索引。
#[tauri::command(async)]
pub fn repair_series_configs(
    state: State<'_, AppState>,
    library_id: i64,
) -> Result<ConfigRepairSummary, String> {
    let root = PathBuf::from(state.db.library_root(library_id).map_err(to_message)?);
    let targets = state
        .db
        .list_series_config_targets(library_id)
        .map_err(to_message)?;
    let now = now_ms();
    let mut summary = ConfigRepairSummary::default();

    for target in targets {
        let dir = PathBuf::from(&target.path);

        // 散落在档期目录里的视频没有独立番剧文件夹，不生成配置。
        if is_loose_series_dir(&dir, &root) || !dir.is_dir() {
            continue;
        }

        summary.checked += 1;
        let folder_name = folder_name(&dir);

        let outcome = match series_config::repair_file(&dir, &folder_name) {
            Ok(outcome) => outcome,
            Err(error) => {
                eprintln!("修复番剧配置失败 {}: {error}", dir.display());
                summary.errors += 1;
                continue;
            }
        };

        if outcome.created {
            summary.created += 1;
        }
        if outcome.updated {
            summary.updated += 1;
        }
        if outcome.names_cleared {
            summary.names_cleared += 1;
        }
        if outcome.invalid {
            summary.invalid += 1;
        }

        // 把修复后的配置同步到数据库，名称/别名/评分立即生效。
        let resolved = series_config::resolve(&dir, &folder_name);
        match state.db.upsert_series(
            library_id,
            &target.category,
            &resolved.name,
            &target.path,
            &resolved.aliases.join("\n"),
            resolved.cover_path.as_deref(),
            resolved.cover_version,
            now,
        ) {
            Ok(series_id) => {
                if let Err(error) = state.db.set_series_rating(series_id, resolved.rating)
                {
                    eprintln!("同步评分失败 id={series_id}: {error:#}");
                    summary.errors += 1;
                }
            }
            Err(error) => {
                eprintln!("同步番剧配置失败 id={}: {error:#}", target.id);
                summary.errors += 1;
            }
        }
    }

    Ok(summary)
}

/// 保存名称、别名和季链接，并立即更新数据库索引。
/// `overwrite_inverse` 为 true 时，覆盖目标已有反向链接；为 false 时保留冲突项。
// 命令边界直接展开字段，前端调用更直观；参数过多是预期的。
#[allow(clippy::too_many_arguments)]
#[tauri::command]
pub fn update_series_config(
    state: State<'_, AppState>,
    series_id: i64,
    name: String,
    aliases: Vec<String>,
    previous_id: Option<i64>,
    next_id: Option<i64>,
    overwrite_inverse: bool,
    episode_mode: String,
    episodes: Vec<String>,
) -> Result<SeriesConfigView, String> {
    let series = state.db.get_series(series_id).map_err(to_message)?;
    let dir = PathBuf::from(&series.path);
    let mut config = series_config::load(&dir);
    let old_previous = config.previous.trim().to_string();
    let old_next = config.next.trim().to_string();

    config.name = name.trim().to_string();
    config.aliases = series_config::clean_aliases(&aliases);
    config.previous = target_path(&state, series_id, previous_id)?;
    config.next = target_path(&state, series_id, next_id)?;
    config.episode_mode = series_config::normalize_episode_mode(&episode_mode);
    config.episodes = series_config::clean_episodes(&episodes);

    series_config::save(&dir, &config)
        .map_err(|error| format!("写入番剧配置失败: {error}"))?;

    // 立即按新的选集配置修正这个番剧的视频分类，选集页无需重扫即可更新。
    state
        .db
        .reclassify_series_videos(series_id)
        .map_err(to_message)?;

    state
        .db
        .update_series_meta(
            series_id,
            &series_config::resolved_name(&config, &folder_name(&dir)),
            &config.aliases.join("\n"),
            now_ms(),
        )
        .map_err(to_message)?;

    sync_inverse_links(
        &state,
        &series.path,
        &old_previous,
        &old_next,
        &config.previous,
        &config.next,
        overwrite_inverse,
    )?;

    config_view(&state, series_id)
}

fn config_view(
    state: &State<'_, AppState>,
    series_id: i64,
) -> Result<SeriesConfigView, String> {
    let series = state.db.get_series(series_id).map_err(to_message)?;
    let dir = PathBuf::from(&series.path);
    let config = series_config::load(&dir);

    Ok(SeriesConfigView {
        name: config.name.trim().to_string(),
        aliases: series_config::clean_aliases(&config.aliases),
        rating: series_config::resolved_rating(&config),
        awards: series_config::clean_list(&config.awards),
        previous: target_option(state, series_id, &config.previous)?,
        next: target_option(state, series_id, &config.next)?,
        episode_mode: series_config::normalize_episode_mode(&config.episode_mode),
        episodes: series_config::clean_episodes(&config.episodes),
    })
}

/// 番剧目录下扫描到的全部视频，供选集选择器使用。
#[tauri::command]
pub fn list_series_videos(
    state: State<'_, AppState>,
    series_id: i64,
) -> Result<Vec<SeriesVideoView>, String> {
    let rows = state
        .db
        .series_videos(series_id)
        .map_err(to_message)?;

    Ok(rows
        .into_iter()
        .map(|row| SeriesVideoView {
            relative_path: series_config::relative_path(
                Path::new(&row.series_path),
                Path::new(&row.path),
            ),
            path: row.path,
            filename: row.filename,
            kind: row.kind,
        })
        .collect())
}

fn collect_conflict(
    state: &State<'_, AppState>,
    target_path: &str,
    reverse_field: &str,
    self_path: &str,
    conflicts: &mut Vec<SeasonLinkConflict>,
) -> Result<(), String> {
    let Some(target) = lookup_series(state, target_path)? else {
        return Ok(());
    };

    let target_config = series_config::load(Path::new(&target.path));
    let current_path = reverse_field_value(&target_config, reverse_field);
    if current_path.is_empty() || same_path(current_path, self_path) {
        return Ok(());
    }

    conflicts.push(SeasonLinkConflict {
        direction: reverse_field.to_string(),
        current: lookup_series(state, current_path)?,
        target,
    });
    Ok(())
}

/// 同步两个方向的反向链接，并拆除被替换的旧配对。
#[allow(clippy::too_many_arguments)]
fn sync_inverse_links(
    state: &State<'_, AppState>,
    self_path: &str,
    old_previous: &str,
    old_next: &str,
    new_previous: &str,
    new_next: &str,
    overwrite: bool,
) -> Result<(), String> {
    sync_reverse_link(
        state,
        self_path,
        old_previous,
        new_previous,
        REVERSE_NEXT,
        overwrite,
    )?;
    sync_reverse_link(
        state,
        self_path,
        old_next,
        new_next,
        REVERSE_PREVIOUS,
        overwrite,
    )
}

fn sync_reverse_link(
    state: &State<'_, AppState>,
    self_path: &str,
    old_target_path: &str,
    new_target_path: &str,
    reverse_field: &str,
    overwrite: bool,
) -> Result<(), String> {
    let old_target_path = old_target_path.trim();
    let new_target_path = new_target_path.trim();

    if same_path(old_target_path, new_target_path) {
        return Ok(());
    }

    // 1. 解除旧目标上指向自己的反向链接。
    if !old_target_path.is_empty()
        && !same_path(old_target_path, self_path)
        && let Some(old_target) = lookup_series(state, old_target_path)?
    {
        let mut config = series_config::load(Path::new(&old_target.path));
        if same_path(reverse_field_value(&config, reverse_field), self_path) {
            set_reverse_field(&mut config, reverse_field, "");
            save_config(&old_target.path, &config)?;
        }
    }

    if new_target_path.is_empty() || same_path(new_target_path, self_path) {
        return Ok(());
    }

    let Some(target) = lookup_series(state, new_target_path)? else {
        return Ok(());
    };

    let mut config = series_config::load(Path::new(&target.path));
    let current = reverse_field_value(&config, reverse_field).to_string();

    // 已经是正确配对。
    if same_path(&current, self_path) {
        return Ok(());
    }
    // 目标已有其它链接且用户选择保留。
    if !current.is_empty() && !overwrite {
        return Ok(());
    }

    // 覆盖前先拆掉旧的配对：C 指向目标 B 的那一侧。
    if !current.is_empty()
        && let Some(previous_target) = lookup_series(state, &current)?
    {
        let mut previous_config =
            series_config::load(Path::new(&previous_target.path));
        let opposite = opposite_field(reverse_field);
        if same_path(
            reverse_field_value(&previous_config, opposite),
            &target.path,
        ) {
            set_reverse_field(&mut previous_config, opposite, "");
            save_config(&previous_target.path, &previous_config)?;
        }
    }

    set_reverse_field(&mut config, reverse_field, self_path);
    save_config(&target.path, &config)
}

fn reverse_field_value<'a>(config: &'a SeriesConfig, field: &str) -> &'a str {
    if field == REVERSE_NEXT {
        config.next.trim()
    } else {
        config.previous.trim()
    }
}

fn set_reverse_field(config: &mut SeriesConfig, field: &str, value: &str) {
    if field == REVERSE_NEXT {
        config.next = value.to_string();
    } else {
        config.previous = value.to_string();
    }
}

fn opposite_field(field: &str) -> &'static str {
    if field == REVERSE_NEXT {
        REVERSE_PREVIOUS
    } else {
        REVERSE_NEXT
    }
}

fn save_config(series_path: &str, config: &SeriesConfig) -> Result<(), String> {
    series_config::save(Path::new(series_path), config)
        .map_err(|error| format!("写入番剧配置失败: {error}"))
}

fn lookup_series(
    state: &State<'_, AppState>,
    path: &str,
) -> Result<Option<SeriesOption>, String> {
    let path = path.trim();
    if path.is_empty() {
        return Ok(None);
    }

    state
        .db
        .series_option_by_path(path)
        .map_err(to_message)
}

/// Windows 路径大小写不敏感；统一按不区分大小写比较。
fn same_path(left: &str, right: &str) -> bool {
    left.trim().eq_ignore_ascii_case(right.trim())
}

fn target_option(
    state: &State<'_, AppState>,
    self_id: i64,
    path: &str,
) -> Result<Option<SeriesOption>, String> {
    let path = path.trim();
    if path.is_empty() {
        return Ok(None);
    }

    Ok(state
        .db
        .series_option_by_path(path)
        .map_err(to_message)?
        .filter(|option| option.id != self_id))
}

fn target_path(
    state: &State<'_, AppState>,
    self_id: i64,
    target_id: Option<i64>,
) -> Result<String, String> {
    let Some(target_id) = target_id else {
        return Ok(String::new());
    };
    if target_id == self_id {
        return Err("不能把番剧链接到自己".to_string());
    }

    Ok(state.db.get_series(target_id).map_err(to_message)?.path)
}

fn folder_name(dir: &Path) -> String {
    dir.file_name()
        .map(|value| value.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// 散落在库根目录或档期目录里的视频没有独立番剧文件夹，不应生成 kanfanos.toml。
fn is_loose_series_dir(dir: &Path, root: &Path) -> bool {
    let Ok(relative) = dir.strip_prefix(root) else {
        return false;
    };

    let mut components = relative.components();
    let Some(first) = components.next() else {
        return true;
    };
    if components.next().is_some() {
        return false;
    }

    let name = first.as_os_str().to_string_lossy();
    catalog::parse_schedule(&name).is_some() || catalog::is_movie_category(&name)
}

fn to_message(error: anyhow::Error) -> String {
    format!("{error:#}")
}
