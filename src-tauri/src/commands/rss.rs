//! RSS 追番命令：订阅源、追番规则、抓取匹配和自动下载。

use std::{
    cmp::Reverse,
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, Ordering},
    time::Duration,
};

use anyhow::{bail, Context, Result as AnyResult};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};

use crate::{
    commands::resources::{
        add_torrent_file_with_save_path, add_torrent_with_save_path, download_base_dir,
        DownloadDirMissingEvent,
    },
    database::rss::{RssFeed, RssFollow, RssMatch},
    rss::{self, MatchScore},
    state::AppState,
    time::now_ms,
};

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RssRefreshResult {
    pub feed_id: i64,
    pub added_entries: usize,
    pub added_matches: usize,
    pub downloaded: usize,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RssDownloadRoot {
    pub path: String,
    pub exists: bool,
}

#[tauri::command]
pub fn rss_download_root(app: AppHandle) -> Result<RssDownloadRoot, String> {
    let path = download_base_dir(&app).map_err(to_message)?;
    let exists = path.is_dir();
    Ok(RssDownloadRoot {
        path: path.to_string_lossy().into_owned(),
        exists,
    })
}

#[tauri::command]
pub fn rss_parse_release_title(text: String) -> crate::rss::ParsedReleaseTitle {
    crate::rss::parse_release_title(&text)
}

#[tauri::command]
pub fn rss_list_feeds(state: State<'_, AppState>) -> Result<Vec<RssFeed>, String> {
    state.db.list_rss_feeds().map_err(to_message)
}

#[tauri::command]
pub fn rss_list_follows(state: State<'_, AppState>) -> Result<Vec<RssFollow>, String> {
    state.db.list_rss_follows().map_err(to_message)
}

#[tauri::command]
pub fn rss_list_matches(
    state: State<'_, AppState>,
    limit: Option<i64>,
    follow_ids: Option<Vec<i64>>,
) -> Result<Vec<RssMatch>, String> {
    let limit = limit.unwrap_or(80).clamp(1, 300);
    match follow_ids {
        Some(ids) => state
            .db
            .recent_rss_matches_for_follows(limit, &ids)
            .map_err(to_message),
        None => state.db.recent_rss_matches(limit).map_err(to_message),
    }
}

#[tauri::command]
pub fn rss_list_entries(
    state: State<'_, AppState>,
    feed_id: i64,
    limit: Option<i64>,
) -> Result<Vec<crate::database::rss::RssEntry>, String> {
    state
        .db
        .recent_rss_entries(feed_id, limit.unwrap_or(30).clamp(1, 200))
        .map_err(to_message)
}

#[tauri::command]
pub async fn rss_add_feed(app: AppHandle, url: String) -> Result<RssFeed, String> {
    let url = url.trim().to_string();
    let parsed = tauri::Url::parse(&url).map_err(|_| "RSS 地址无效".to_string())?;
    if parsed.scheme() != "http" && parsed.scheme() != "https" {
        return Err("RSS 地址只支持 http/https".to_string());
    }

    let state = app.state::<AppState>();
    let feed = state
        .db
        .add_rss_feed(&url, "", "", now_ms())
        .map_err(to_message)?;
    refresh_feed(&app, feed.id).await.map_err(to_message)?;
    state
        .db
        .get_rss_feed(feed.id)
        .map_err(to_message)?
        .context("RSS 源不存在")
        .map_err(to_message)
}

#[tauri::command]
pub async fn rss_refresh_feed(app: AppHandle, feed_id: i64) -> Result<RssRefreshResult, String> {
    refresh_feed(&app, feed_id).await.map_err(to_message)
}

#[tauri::command]
pub async fn rss_refresh_all(app: AppHandle) -> Result<Vec<RssRefreshResult>, String> {
    refresh_all(&app).await.map_err(to_message)
}

#[tauri::command]
pub fn rss_delete_feed(state: State<'_, AppState>, feed_id: i64) -> Result<(), String> {
    state.db.delete_rss_feed(feed_id).map_err(to_message)?;
    Ok(())
}

#[tauri::command]
pub async fn rss_add_follow(
    app: AppHandle,
    state: State<'_, AppState>,
    feed_id: i64,
    series_id: Option<i64>,
    title: String,
    aliases: String,
    preferred_groups: Option<String>,
    subtitle_packaging: Option<String>,
    resolution: Option<String>,
    save_path: Option<String>,
    auto_download: bool,
) -> Result<RssFollow, String> {
    let title = title.trim().to_string();
    if title.is_empty() {
        return Err("番剧名不能为空".to_string());
    }
    if state
        .db
        .get_rss_feed(feed_id)
        .map_err(to_message)?
        .is_none()
    {
        return Err("RSS 源不存在".to_string());
    }
    let aliases = rss::split_aliases(&aliases);
    let preferred_groups = rss::split_aliases(preferred_groups.as_deref().unwrap_or_default());
    let subtitle_packaging = subtitle_packaging.unwrap_or_default().trim().to_string();
    let save_path = normalize_save_path(save_path.as_deref()).map_err(to_message)?;
    let follow = state
        .db
        .add_rss_follow(
            feed_id,
            series_id,
            &title,
            &aliases,
            &preferred_groups,
            &subtitle_packaging,
            resolution.as_deref().unwrap_or("1080p"),
            save_path.as_deref(),
            auto_download,
            now_ms(),
        )
        .map_err(to_message)?;

    // 订阅源里可能已经有当前更新，立即匹配一次，用户不用再手动刷新。
    let _ = refresh_feed(&app, feed_id).await;
    Ok(follow)
}

#[tauri::command]
pub fn rss_update_follow(
    state: State<'_, AppState>,
    follow_id: i64,
    title: String,
    aliases: String,
    preferred_groups: Option<String>,
    subtitle_packaging: Option<String>,
    resolution: Option<String>,
    save_path: Option<String>,
    auto_download: bool,
    status: Option<String>,
) -> Result<(), String> {
    if title.trim().is_empty() {
        return Err("番剧名不能为空".to_string());
    }
    if state
        .db
        .get_rss_follow(follow_id)
        .map_err(to_message)?
        .is_none()
    {
        return Err("追番不存在".to_string());
    }
    let aliases = rss::split_aliases(&aliases);
    let preferred_groups = rss::split_aliases(preferred_groups.as_deref().unwrap_or_default());
    let subtitle_packaging = subtitle_packaging.unwrap_or_default().trim().to_string();
    let save_path = normalize_save_path(save_path.as_deref()).map_err(to_message)?;
    state
        .db
        .update_rss_follow(
            follow_id,
            title.trim(),
            &aliases,
            &preferred_groups,
            &subtitle_packaging,
            resolution.as_deref().unwrap_or("1080p"),
            save_path.as_deref(),
            auto_download,
            status.as_deref().unwrap_or("active"),
            now_ms(),
        )
        .map_err(to_message)
}

#[tauri::command]
pub fn rss_delete_follow(state: State<'_, AppState>, follow_id: i64) -> Result<(), String> {
    state.db.delete_rss_follow(follow_id).map_err(to_message)?;
    Ok(())
}

#[tauri::command]
pub async fn rss_download_match(app: AppHandle, match_id: i64) -> Result<(), String> {
    let state = app.state::<AppState>();
    let item = state
        .db
        .get_rss_match(match_id)
        .map_err(to_message)?
        .context("RSS 匹配记录不存在")
        .map_err(to_message)?;
    if item.status == "queued" {
        return Err("这条更新已经加入下载".to_string());
    }
    let follow = state
        .db
        .get_rss_follow(item.follow_id)
        .map_err(to_message)?
        .context("追番订阅不存在")
        .map_err(to_message)?;
    queue_download(&app, &item, &follow).await.map_err(to_message)
}

pub fn spawn_scheduler(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(Duration::from_secs(45)).await;
        loop {
            let has_feeds = app
                .state::<AppState>()
                .db
                .list_rss_feeds()
                .map(|feeds| !feeds.is_empty())
                .unwrap_or(false);
            if has_feeds {
                let _ = refresh_all(&app).await;
            }
            tokio::time::sleep(Duration::from_secs(15 * 60)).await;
        }
    });
}

async fn refresh_all(app: &AppHandle) -> AnyResult<Vec<RssRefreshResult>> {
    static REFRESHING: AtomicBool = AtomicBool::new(false);
    if REFRESHING.swap(true, Ordering::SeqCst) {
        bail!("RSS 正在刷新，请稍后再试");
    }

    let result = async {
        let feeds = app.state::<AppState>().db.list_rss_feeds()?;
        let mut results = Vec::new();
        for feed in feeds {
            // 单个源失败不阻塞其它源。
            if let Ok(result) = refresh_feed(app, feed.id).await {
                results.push(result);
            }
        }
        let _ = app.emit("rss-updated", ());
        Ok(results)
    }
    .await;

    REFRESHING.store(false, Ordering::SeqCst);
    result
}

async fn refresh_feed(app: &AppHandle, feed_id: i64) -> AnyResult<RssRefreshResult> {
    let state = app.state::<AppState>();
    let feed = state
        .db
        .get_rss_feed(feed_id)?
        .context("RSS 源不存在")?;

    let parsed = match rss::fetch(&feed.url).await {
        Ok(parsed) => parsed,
        Err(error) => {
            let _ = state
                .db
                .update_rss_feed_fetch(feed_id, "", "", now_ms(), Some(&format!("{error:#}")));
            return Err(error);
        }
    };

    state.db.update_rss_feed_fetch(
        feed_id,
        &parsed.title,
        &parsed.site,
        now_ms(),
        None,
    )?;

    let mut added_entries = 0usize;
    let mut added_matches = 0usize;
    let follows: Vec<RssFollow> = state
        .db
        .list_rss_follows()?
        .into_iter()
        .filter(|follow| follow.feed_id == feed_id && follow.status == "active")
        .collect();

    for entry in &parsed.entries {
        let Some(_entry_id) = state.db.insert_rss_entry(
            feed_id,
            &entry.guid,
            &entry.title,
            &entry.link,
            &entry.download_url,
            entry.published_at,
            now_ms(),
        )? else {
            continue;
        };
        added_entries += 1;
    }

    // 对最近的条目重新跑一遍匹配：新添加的追番也能匹配到订阅源里
    // 已经存在的更新，而不只是下一次抓取的新条目。
    let recent_entries = state.db.recent_rss_entries(feed_id, 200)?;
    for entry in &recent_entries {
        for follow in &follows {
            if let Some(MatchScore {
                episode,
                confidence,
            }) = rss::score_release(
                &follow.title,
                &follow.aliases,
                &follow.resolution,
                &entry.title,
            ) && state
                .db
                .insert_rss_match(entry.id, follow.id, episode, confidence, now_ms())?
            {
                added_matches += 1;
            }
        }
    }

    let downloaded = auto_download_latest(app).await;
    Ok(RssRefreshResult {
        feed_id,
        added_entries,
        added_matches,
        downloaded,
    })
}

/// 自动下载只处理每个追番当前最新的一集；更旧的 pending 直接忽略。
fn select_latest_candidate<'a>(
    pending: &'a [RssMatch],
    limited_groups: &[String],
    subtitle_packaging: &str,
    last_episode: i64,
) -> Option<&'a RssMatch> {
    let eligible: Vec<&RssMatch> = pending
        .iter()
        .filter(|item| {
            item.episode
                .is_some_and(|episode| episode > last_episode)
                && item.confidence >= 70
        })
        .filter(|item| {
            limited_groups.is_empty()
                || rss::preferred_group_rank(limited_groups, &item.entry_title) != usize::MAX
        })
        .filter(|item| rss::subtitle_packaging_matches(subtitle_packaging, &item.entry_title))
        .collect();
    let latest_episode = eligible.iter().filter_map(|item| item.episode).max()?;

    // 限定组/封装方式没有这一集时保持等待，不回退到其它组。
    eligible
        .into_iter()
        .filter(|item| item.episode == Some(latest_episode))
        .min_by_key(|item| (Reverse(item.confidence), Reverse(item.published_at)))
}

async fn auto_download_latest(app: &AppHandle) -> usize {
    let state = app.state::<AppState>();
    let follows = match state.db.list_rss_follows() {
        Ok(follows) => follows,
        Err(_) => return 0,
    };
    let mut downloaded = 0usize;

    for follow in follows {
        if !follow.auto_download || follow.status != "active" {
            continue;
        }
        let pending = match state.db.pending_rss_matches(follow.id) {
            Ok(pending) => pending,
            Err(_) => continue,
        };
        let last = follow.last_episode.unwrap_or(0);
        let Some(candidate) = select_latest_candidate(
            &pending,
            &follow.preferred_groups,
            &follow.subtitle_packaging,
            last,
        ) else {
            continue;
        };
        let candidate_episode = candidate.episode.unwrap_or(0);

        match queue_download(app, candidate, &follow).await {
            Ok(()) => {
                // 既然只补最新一集，下载成功后再把旧集从待确认里清掉。
                for item in pending.iter().filter(|item| {
                    item.id != candidate.id
                        && item.episode.is_some_and(|episode| episode < candidate_episode)
                }) {
                    let _ = state
                        .db
                        .set_rss_match_status(item.id, "ignored", None, now_ms());
                }
                downloaded += 1;
            }
            Err(error) => {
                let _ = state.db.set_rss_match_status(
                    candidate.id,
                    "failed",
                    Some(&format!("{error:#}")),
                    now_ms(),
                );
            }
        }
    }
    downloaded
}

fn normalize_save_path(value: Option<&str>) -> AnyResult<Option<String>> {
    let Some(value) = value.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok(None);
    };
    let path = PathBuf::from(value);
    if !Path::new(value).is_absolute() {
        bail!("保存目录必须是绝对路径");
    }
    let text = path.to_string_lossy();
    let trimmed = text.trim_end_matches(|ch| ch == '\\' || ch == '/');
    // 盘符根目录不能把末尾分隔符裁掉，否则 C:\ 会变成相对路径 C:。
    let normalized = if trimmed.is_empty() || trimmed.ends_with(':') {
        text.into_owned()
    } else {
        trimmed.to_string()
    };
    Ok(Some(normalized))
}

async fn queue_download(app: &AppHandle, item: &RssMatch, follow: &RssFollow) -> AnyResult<()> {
    let url = if item.download_url.trim().is_empty() {
        item.link.clone()
    } else {
        item.download_url.clone()
    };
    if url.trim().is_empty() {
        bail!("这条更新没有可用的种子或磁力地址");
    }

    let folder = match follow
        .save_path
        .as_deref()
        .map(str::trim)
        .filter(|path| !path.is_empty())
    {
        Some(path) => PathBuf::from(path),
        None => download_base_dir(app)?,
    };
    if !folder.is_absolute() {
        bail!("保存目录必须是绝对路径：{}", folder.display());
    }
    // 严格模式：保存目录必须事先存在，不在下载时自动创建。
    if !folder.is_dir() {
        let _ = app.emit(
            "download-dir-missing",
            DownloadDirMissingEvent {
                path: folder.to_string_lossy().into_owned(),
            },
        );
        bail!("保存目录不存在：{}", folder.display());
    }

    let folder_text = folder.to_string_lossy().into_owned();
    let lower = url.to_ascii_lowercase();
    if lower.starts_with("magnet:?") {
        // 磁力链接没有文件可预取，只能让 qBittorrent 自己解析元数据。
        add_torrent_with_save_path(app, &url, Some(folder_text.as_str())).await?;
    } else if lower.starts_with("http://") || lower.starts_with("https://") {
        let bytes = rss::fetch_torrent(&url).await?;
        let filename = format!("rss-{}-{}.torrent", item.follow_id, item.id);
        add_torrent_file_with_save_path(app, &bytes, &filename, Some(folder_text.as_str())).await?;
    } else {
        bail!("不支持的种子地址：{url}");
    }
    let db = &app.state::<AppState>().db;
    db.set_rss_match_status(item.id, "queued", None, now_ms())?;
    if let Some(episode) = item.episode
        && follow.last_episode.unwrap_or(0) < episode
    {
        db.set_rss_follow_last_episode(follow.id, episode)?;
    }
    Ok(())
}

fn to_message(error: anyhow::Error) -> String {
    format!("{error:#}")
}

#[cfg(test)]
mod tests {
    use super::{normalize_save_path, select_latest_candidate};
    use crate::database::rss::RssMatch;

    fn item(id: i64, title: &str, episode: i64, confidence: i64, published_at: i64) -> RssMatch {
        RssMatch {
            id,
            entry_id: id,
            follow_id: 1,
            follow_title: "测试番".to_string(),
            entry_title: title.to_string(),
            release_group: None,
            subtitle_packaging: None,
            link: String::new(),
            download_url: String::new(),
            published_at,
            episode: Some(episode),
            confidence,
            status: "pending".to_string(),
            error: None,
            created_at: 0,
        }
    }

    #[test]
    fn selects_latest_episode_then_preferred_group() {
        let ani = item(1, "[ANi] 测试番 - 04 [1080P]", 4, 100, 1);
        let black = item(2, "[黒ネズミたち] 测试番 - 04 [720P]", 4, 80, 2);
        let pending = vec![ani, black];

        assert_eq!(
            select_latest_candidate(&pending, &[], "", 3).map(|item| item.id),
            Some(1)
        );
        assert_eq!(
            select_latest_candidate(&pending, &["黒ネズミたち".to_string()], "", 3)
                .map(|item| item.id),
            Some(2)
        );
        // 限定组没有这一集时保持等待，不回退到其它组。
        assert_eq!(
            select_latest_candidate(&pending, &["不存在的组".to_string()], "", 3)
                .map(|item| item.id),
            None
        );
    }

    #[test]
    fn limited_group_selects_its_latest_episode() {
        let ani = item(1, "[ANi] 测试番 - 04 [1080P]", 4, 100, 1);
        let black = item(2, "[黒ネズミたち] 测试番 - 03 [1080P]", 3, 100, 2);
        let pending = vec![ani, black];

        assert_eq!(
            select_latest_candidate(&pending, &["黒ネズミたち".to_string()], "", 2)
                .map(|item| item.id),
            Some(2)
        );
    }

    #[test]
    fn subtitle_packaging_is_a_hard_filter() {
        let packaged = item(
            1,
            "[ANi] 测试番 - 04 [1080P][CHS_JP&CHT_JP][简繁日内封]",
            4,
            100,
            1,
        );
        let embedded = item(
            2,
            "[ANi] 测试番 - 04 [1080P][CHS_JP&CHT_JP][简日内嵌]",
            4,
            100,
            2,
        );
        let pending = vec![packaged, embedded];
        let groups = ["ANi".to_string()];

        assert_eq!(
            select_latest_candidate(&pending, &groups, "简繁日内封", 3).map(|item| item.id),
            Some(1)
        );
        assert_eq!(
            select_latest_candidate(&pending, &groups, "外挂", 3).map(|item| item.id),
            None
        );
    }

    #[test]
    fn normalizes_absolute_save_paths_and_rejects_relative_ones() {
        assert_eq!(
            normalize_save_path(Some(r"D:\Anime\Show\"))
                .unwrap()
                .as_deref(),
            Some(r"D:\Anime\Show")
        );
        assert_eq!(normalize_save_path(Some("  ")).unwrap(), None);
        assert!(normalize_save_path(Some(r"Anime\Show")).is_err());
    }
}
