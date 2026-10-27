//! 资源页命令：内嵌浏览器与 qBittorrent WebUI。

use std::{fs, path::{Path, PathBuf}, time::Duration};

use anyhow::{bail, Context, Result as AnyResult};
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};

use crate::{
    browser::{self, BrowserRect},
    qbittorrent::{self, Endpoint, ManagedQbittorrent, QbState, TorrentInfo},
    series_config::{self, SeriesConfig},
    settings::{self, QbittorrentSettings, QBITTORRENT_MODE_EXTERNAL},
    state::AppState,
};

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QbittorrentConfig {
    pub mode: String,
    pub url: String,
    pub username: String,
    pub password: String,
    pub save_path: Option<String>,
    pub advanced: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QbittorrentStatus {
    pub version: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QbittorrentPreferences {
    pub save_path: String,
}

/// 网页图片保存结果；子 webview 的 IPC 不可用时由代理导航触发。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImageSavedEvent {
    pub ok: bool,
    pub path: Option<String>,
    pub error: Option<String>,
}

/// 以网页选中文字创建文件夹的结果。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FolderCreatedEvent {
    pub ok: bool,
    pub path: Option<String>,
    pub error: Option<String>,
}

/// 下载目录被删除/移走时通知前端弹警告。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadDirMissingEvent {
    pub path: String,
}

#[tauri::command]
pub fn browser_create_folder(app: AppHandle, text: String) -> Result<String, String> {
    let path = create_download_folder(&app, &text)
        .map_err(to_message)?;
    let path_string = path.to_string_lossy().into_owned();
    let _ = app.emit(
        "browser-folder-created",
        FolderCreatedEvent {
            ok: true,
            path: Some(path_string.clone()),
            error: None,
        },
    );
    Ok(path_string)
}

/// 把浏览器里选中的文字交给主界面，打开预填好的追番弹窗。
#[tauri::command]
pub fn browser_add_follow_from_text(app: AppHandle, text: String) -> Result<(), String> {
    let text: String = text.trim().chars().take(4000).collect();
    if text.is_empty() {
        return Err("没有选中文字".to_string());
    }
    app.emit("rss-add-follow-from-text", text)
        .map_err(|error| error.to_string())
}

/// 供浏览器 on_navigation 的建文件夹代理调用。
pub(crate) fn spawn_browser_folder_create(app: AppHandle, text: String) {
    tauri::async_runtime::spawn(async move {
        let result = create_download_folder(&app, &text);
        let event = match result {
            Ok(path) => FolderCreatedEvent {
                ok: true,
                path: Some(path.to_string_lossy().into_owned()),
                error: None,
            },
            Err(error) => FolderCreatedEvent {
                ok: false,
                path: None,
                error: Some(format!("{error:#}")),
            },
        };
        let _ = app.emit("browser-folder-created", event);
    });
}

fn create_download_folder(app: &AppHandle, text: &str) -> AnyResult<PathBuf> {
    let directory = ensure_download_dir(app)?;

    let name = sanitize_folder_name(text).context("选中文字不能作为文件夹名")?;
    let path = unique_directory(&directory, &name);
    fs::create_dir(&path)
        .with_context(|| format!("创建文件夹失败：{}", path.display()))?;

    // 文件夹名做了 Windows 合法化；如果原文含非法字符，用配置里的
    // name 保留原始显示名（只去掉换行等控制字符）。没有非法字符时
    // name 留空，让程序直接使用文件夹名。
    let display_name = config_display_name(text);
    let config_name = if contains_invalid_filename_char(&display_name) {
        display_name
    } else {
        String::new()
    };
    let config = SeriesConfig {
        name: config_name,
        ..SeriesConfig::default()
    };
    if let Err(error) = series_config::save(&path, &config) {
        let _ = fs::remove_dir(&path);
        return Err(error)
            .with_context(|| format!("写入 {} 失败", series_config::CONFIG_FILE));
    }

    Ok(path)
}

pub(crate) fn sanitize_folder_name(text: &str) -> Option<String> {
    const RESERVED: [&str; 22] = [
        "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5",
        "COM6", "COM7", "COM8", "COM9", "LPT1", "LPT2", "LPT3", "LPT4",
        "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
    ];

    // 用一个内部标记表示“换行/制表符等硬空白”，它和旁边的普通空格
    // 一起整段删除，避免换行处留下空格。
    const HARD_SPACE: char = '\u{0}';

    let mut raw = String::new();
    for ch in text.chars() {
        if ch.is_control() || (ch.is_whitespace() && ch != ' ') {
            raw.push(HARD_SPACE);
        } else if ch == ' ' {
            raw.push(' ');
        } else if is_invalid_filename_char(ch) {
            // 非法文件名字符用普通空格替代。
            raw.push(' ');
        } else {
            raw.push(ch);
        }
    }

    // 只有“孤立的单个普通空格”保留；连续空格或含换行/制表符的空白段
    // 全部删除，不留空格。
    let mut cleaned = String::new();
    let mut spaces = 0usize;
    let mut hard = false;
    for ch in raw.chars() {
        if ch == HARD_SPACE {
            hard = true;
            continue;
        }
        if ch == ' ' {
            spaces += 1;
            continue;
        }
        if spaces == 1 && !hard {
            cleaned.push(' ');
        }
        spaces = 0;
        hard = false;
        cleaned.push(ch);
    }

    let trimmed = cleaned
        .trim_matches(|ch| ch == ' ' || ch == '.')
        .to_string();
    if trimmed.is_empty() {
        return None;
    }

    let mut name: String = trimmed.chars().take(80).collect();
    name = name
        .trim_end_matches(|ch| ch == ' ' || ch == '.')
        .to_string();
    if name.is_empty() {
        return None;
    }

    let stem = name
        .split('.')
        .next()
        .unwrap_or(&name)
        .to_ascii_uppercase();
    if RESERVED.contains(&stem.as_str()) {
        name.insert(0, '_');
    }
    Some(name)
}

fn is_invalid_filename_char(ch: char) -> bool {
    matches!(
        ch,
        '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*'
    )
}

/// 配置里的显示名：保留非法符号，只去掉换行/制表符等控制字符和
/// 非普通空格的空白。
fn config_display_name(text: &str) -> String {
    let mut display = String::new();
    let mut spaces = 0usize;
    let mut hard = false;

    for ch in text.chars() {
        if ch.is_control() || (ch.is_whitespace() && ch != ' ') {
            hard = true;
            continue;
        }
        if ch == ' ' {
            spaces += 1;
            continue;
        }
        if spaces == 1 && !hard && !display.is_empty() {
            display.push(' ');
        }
        spaces = 0;
        hard = false;
        display.push(ch);
    }

    display.trim().to_string()
}

fn contains_invalid_filename_char(text: &str) -> bool {
    text.chars().any(is_invalid_filename_char)
}

fn unique_directory(directory: &Path, name: &str) -> PathBuf {
    let candidate = directory.join(name);
    if !candidate.exists() {
        return candidate;
    }

    for index in 2..1000 {
        let candidate = directory.join(format!("{name} ({index})"));
        if !candidate.exists() {
            return candidate;
        }
    }
    directory.join(format!("{name} ({})", crate::time::now_ms()))
}

#[tauri::command]
pub async fn browser_save_image(
    app: AppHandle,
    url: String,
    referer: Option<String>,
    user_agent: Option<String>,
) -> Result<String, String> {
    let result = save_image_url(&app, &url, referer.as_deref(), user_agent.as_deref()).await;
    match &result {
        Ok(path) => {
            let _ = app.emit(
                "browser-image-saved",
                ImageSavedEvent {
                    ok: true,
                    path: Some(path.to_string_lossy().into_owned()),
                    error: None,
                },
            );
        }
        Err(error) => {
            let _ = app.emit(
                "browser-image-saved",
                ImageSavedEvent {
                    ok: false,
                    path: None,
                    error: Some(format!("{error:#}")),
                },
            );
        }
    }

    result
        .map(|path| path.to_string_lossy().into_owned())
        .map_err(to_message)
}

/// 供浏览器 on_navigation 的图片保存代理调用。
pub(crate) fn spawn_browser_image_save(
    app: AppHandle,
    url: String,
    referer: Option<String>,
) {
    tauri::async_runtime::spawn(async move {
        let result = save_image_url(&app, &url, referer.as_deref(), None).await;
        let event = match result {
            Ok(path) => ImageSavedEvent {
                ok: true,
                path: Some(path.to_string_lossy().into_owned()),
                error: None,
            },
            Err(error) => ImageSavedEvent {
                ok: false,
                path: None,
                error: Some(format!("{error:#}")),
            },
        };
        let _ = app.emit("browser-image-saved", event);
    });
}

async fn save_image_url(
    app: &AppHandle,
    url: &str,
    referer: Option<&str>,
    user_agent: Option<&str>,
) -> AnyResult<PathBuf> {
    let directory = ensure_download_dir(app)?;

    let (bytes, mime) = if url.starts_with("data:") {
        decode_image_data_url(url)?
    } else {
        download_image(url, referer, user_agent).await?
    };

    if !mime.starts_with("image/") {
        bail!("远程内容不是图片（Content-Type: {mime}）");
    }
    if bytes.is_empty() {
        bail!("图片内容为空");
    }

    let name = image_file_name(url, &mime);
    let path = unique_path(&directory, &name);
    fs::write(&path, bytes)
        .with_context(|| format!("写入图片失败：{}", path.display()))?;
    Ok(path)
}

async fn download_image(
    url: &str,
    referer: Option<&str>,
    user_agent: Option<&str>,
) -> AnyResult<(Vec<u8>, String)> {
    let parsed = tauri::Url::parse(url).context("图片地址无效")?;
    if parsed.scheme() != "http" && parsed.scheme() != "https" {
        bail!("只支持 http/https 图片地址");
    }

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(60))
        .user_agent(
            user_agent
                .filter(|value| !value.trim().is_empty())
                .unwrap_or("Mozilla/5.0 KanFanOS"),
        )
        .build()?;

    // 很多图床默认按 no-referrer 返回图片；带站点 Referer 反而会被防盗链
    // 返回 403（yuc.wiki 的 B 站图床就是这种情况）。先不带 Referer，
    // 遇到 403 再尝试带 Referer，兼容确实需要来源页的站点。
    let mut response = request_image(&client, parsed.clone(), None).await?;
    if response.status() == reqwest::StatusCode::FORBIDDEN
        && let Some(referer) = referer.filter(|value| !value.trim().is_empty())
    {
        response = request_image(&client, parsed, Some(referer)).await?;
    }

    if !response.status().is_success() {
        bail!("下载图片失败：HTTP {}", response.status());
    }

    let mime = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("")
        .split(';')
        .next()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();
    let bytes = response.bytes().await.context("读取图片内容失败")?;
    if bytes.len() > 50 * 1024 * 1024 {
        bail!("图片超过 50 MiB，已取消保存");
    }
    Ok((bytes.to_vec(), mime))
}

async fn request_image(
    client: &reqwest::Client,
    url: reqwest::Url,
    referer: Option<&str>,
) -> AnyResult<reqwest::Response> {
    let mut request = client.get(url);
    if let Some(referer) = referer.filter(|value| !value.trim().is_empty()) {
        request = request.header(reqwest::header::REFERER, referer);
    }
    request.send().await.context("下载图片失败")
}

fn decode_image_data_url(url: &str) -> AnyResult<(Vec<u8>, String)> {
    let rest = url.strip_prefix("data:").context("图片 data URL 无效")?;
    let (meta, data) = rest.split_once(',').context("图片 data URL 无效")?;
    let mime = meta
        .split(';')
        .next()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();
    if !mime.starts_with("image/") {
        bail!("data URL 不是图片");
    }

    let bytes = if meta
        .split(';')
        .any(|part| part.eq_ignore_ascii_case("base64"))
    {
        BASE64.decode(data).context("图片 base64 解码失败")?
    } else {
        data.as_bytes().to_vec()
    };
    Ok((bytes, mime))
}

fn image_download_dir(state: &AppState) -> AnyResult<PathBuf> {
    let settings = settings::load(&state.data_root)?;
    if let Some(path) = settings
        .qbittorrent
        .save_path
        .filter(|path| !path.trim().is_empty())
    {
        return Ok(PathBuf::from(path));
    }

    Ok(state
        .data_root
        .join("qbittorrent")
        .join("qBittorrent")
        .join("downloads"))
}

/// 当前「下载到」目录，不检查是否存在。
pub(crate) fn download_base_dir(app: &AppHandle) -> AnyResult<PathBuf> {
    let state = app.state::<AppState>();
    image_download_dir(state.inner())
}

/// 下载目录必须是已经存在的目录；不存在时只通知前端，不自动重建。
pub(crate) fn ensure_download_dir(app: &AppHandle) -> AnyResult<PathBuf> {
    let directory = download_base_dir(app)?;
    if !directory.is_dir() {
        let _ = app.emit(
            "download-dir-missing",
            DownloadDirMissingEvent {
                path: directory.to_string_lossy().into_owned(),
            },
        );
        bail!("选中的下载目录不存在：{}", directory.display());
    }
    Ok(directory)
}

fn image_file_name(url: &str, mime: &str) -> String {
    let raw = tauri::Url::parse(url)
        .ok()
        .and_then(|url| {
            url.path_segments()
                .and_then(|mut segments| segments.next_back())
                .map(|segment| segment.to_string())
        })
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| format!("image-{}", crate::time::now_ms()));

    let mut name: String = raw
        .chars()
        .map(|ch| {
            if ch.is_control() || matches!(ch, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*') {
                '_'
            } else {
                ch
            }
        })
        .collect();
    name = name.trim_matches(|ch| ch == ' ' || ch == '.').to_string();
    if name.is_empty() {
        name = format!("image-{}", crate::time::now_ms());
    }

    if Path::new(&name).extension().is_none() {
        let extension = match mime {
            "image/jpeg" => "jpg",
            "image/png" => "png",
            "image/gif" => "gif",
            "image/webp" => "webp",
            "image/avif" => "avif",
            "image/svg+xml" => "svg",
            "image/bmp" => "bmp",
            "image/x-icon" | "image/vnd.microsoft.icon" => "ico",
            _ => "img",
        };
        name.push('.');
        name.push_str(extension);
    }

    if name.len() > 120 {
        let extension = Path::new(&name)
            .extension()
            .and_then(|value| value.to_str())
            .unwrap_or("img")
            .to_string();
        let stem_len = 120usize.saturating_sub(extension.len() + 1);
        let stem: String = name.chars().take(stem_len).collect();
        name = format!("{stem}.{extension}");
    }
    name
}

fn unique_path(directory: &Path, name: &str) -> PathBuf {
    let candidate = directory.join(name);
    if !candidate.exists() {
        return candidate;
    }

    let path = Path::new(name);
    let stem = path
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("image");
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("img");

    for index in 1..1000 {
        let candidate = directory.join(format!("{stem}-{index}.{extension}"));
        if !candidate.exists() {
            return candidate;
        }
    }
    directory.join(format!("{stem}-{}.{}", crate::time::now_ms(), extension))
}

/// 浏览器里添加种子后通过事件通知前端。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TorrentAddedEvent {
    pub ok: bool,
    /// `magnet` 或 `torrent`。
    pub kind: String,
    pub url: String,
    pub error: Option<String>,
}

// 内嵌 webview 的创建/定位会阻塞等待主线程（`add_child` 内部同步等待），
// 必须在阻塞线程池里执行；直接放在同步命令里会占住主线程，界面卡在“打开中”。
async fn blocking_browser_task<T, F>(task: F) -> Result<T, String>
where
    T: Send + 'static,
    F: FnOnce() -> anyhow::Result<T> + Send + 'static,
{
    tauri::async_runtime::spawn_blocking(task)
        .await
        .map_err(|error| format!("浏览器操作失败：{error}"))?
        .map_err(to_message)
}

async fn blocking_browser<F>(task: F) -> Result<(), String>
where
    F: FnOnce() -> anyhow::Result<()> + Send + 'static,
{
    blocking_browser_task(task).await
}

#[tauri::command]
pub async fn browser_open_tab(
    app: AppHandle,
    url: String,
    rect: BrowserRect,
) -> Result<u64, String> {
    blocking_browser_task(move || browser::open_tab(&app, &url, rect)).await
}

/// 切换/恢复标签页并更新位置；返回 false 表示 webview 已不存在。
#[tauri::command]
pub async fn browser_show_tab(
    app: AppHandle,
    tab_id: u64,
    rect: BrowserRect,
) -> Result<bool, String> {
    blocking_browser_task(move || browser::show_tab(&app, tab_id, rect)).await
}

/// 隐藏标签页但保留页面状态。
#[tauri::command]
pub async fn browser_hide_tab(app: AppHandle, tab_id: u64) -> Result<(), String> {
    blocking_browser(move || browser::hide_tab(&app, tab_id)).await
}

/// 前端布局变化时同步标签页的位置和大小。
#[tauri::command]
pub async fn browser_set_tab_bounds(
    app: AppHandle,
    tab_id: u64,
    rect: BrowserRect,
) -> Result<(), String> {
    blocking_browser(move || browser::set_tab_bounds(&app, tab_id, rect)).await
}

#[tauri::command]
pub async fn browser_navigate_tab(
    app: AppHandle,
    tab_id: u64,
    url: String,
) -> Result<(), String> {
    blocking_browser(move || browser::navigate_tab(&app, tab_id, &url)).await
}

#[tauri::command]
pub async fn browser_go_back(app: AppHandle, tab_id: u64) -> Result<(), String> {
    blocking_browser(move || browser::go_back(&app, tab_id)).await
}

/// 供远程网页 webview 请求打开新标签页。
#[tauri::command]
pub fn browser_request_new_tab(app: AppHandle, url: String) -> Result<(), String> {
    browser::request_new_tab(&app, &url).map_err(to_message)
}

#[tauri::command]
pub async fn browser_reload_tab(app: AppHandle, tab_id: u64) -> Result<(), String> {
    blocking_browser(move || browser::reload_tab(&app, tab_id)).await
}

#[tauri::command]
pub async fn browser_get_zoom() -> Result<u32, String> {
    Ok(browser::zoom_percent())
}

#[tauri::command]
pub async fn browser_set_zoom(
    app: AppHandle,
    percent: u32,
) -> Result<u32, String> {
    blocking_browser_task(move || browser::set_zoom(&app, percent)).await
}

#[tauri::command]
pub async fn browser_close_tab(app: AppHandle, tab_id: u64) -> Result<(), String> {
    blocking_browser(move || browser::close_tab(&app, tab_id)).await
}

/// 列出后端实际存在的标签页 webview。
#[tauri::command]
pub async fn browser_list_tabs(
    app: AppHandle,
) -> Result<Vec<browser::BrowserTabInfo>, String> {
    blocking_browser_task(move || browser::list_tabs(&app)).await
}

/// 关闭不在 `keep` 里的标签页 webview，回收孤儿窗口。
#[tauri::command]
pub async fn browser_close_untracked(
    app: AppHandle,
    keep: Vec<u64>,
) -> Result<(), String> {
    blocking_browser(move || browser::close_untracked(&app, &keep)).await
}

#[tauri::command]
pub fn get_qbittorrent_config(
    state: State<'_, AppState>,
) -> Result<QbittorrentConfig, String> {
    let config = settings::load(&state.data_root)
        .map_err(to_message)?
        .qbittorrent;

    Ok(QbittorrentConfig {
        mode: config.mode,
        url: config.url,
        username: config.username,
        password: config.password,
        save_path: config.save_path,
        advanced: config.advanced,
    })
}

#[tauri::command]
pub fn set_qbittorrent_config(
    state: State<'_, AppState>,
    qb_state: State<'_, QbState>,
    manager: State<'_, ManagedQbittorrent>,
    mode: String,
    url: String,
    username: String,
    password: String,
) -> Result<(), String> {
    let mut app_settings = settings::load(&state.data_root).map_err(to_message)?;
    // 下载目录由单独的命令维护，这里不要覆盖。
    let save_path = app_settings.qbittorrent.save_path.take();
    let advanced = app_settings.qbittorrent.advanced;
    app_settings.qbittorrent = QbittorrentSettings {
        mode,
        url: url.trim().to_string(),
        username: username.trim().to_string(),
        password,
        save_path,
        advanced,
    };
    settings::save(&state.data_root, &app_settings).map_err(to_message)?;

    // 连接参数变了，旧会话立即失效。
    qb_state.invalidate();

    // 切到外部 WebUI 时不再需要内置实例，省下内存。
    if app_settings.qbittorrent.mode == QBITTORRENT_MODE_EXTERNAL {
        manager.stop();
    }

    Ok(())
}

/// 资源页是否显示 qBittorrent 高级连接设置。
#[tauri::command]
pub fn set_qbittorrent_advanced(
    state: State<'_, AppState>,
    advanced: bool,
) -> Result<QbittorrentConfig, String> {
    let mut app_settings = settings::load(&state.data_root).map_err(to_message)?;
    app_settings.qbittorrent.advanced = advanced;
    settings::save(&state.data_root, &app_settings).map_err(to_message)?;

    let config = app_settings.qbittorrent;
    Ok(QbittorrentConfig {
        mode: config.mode,
        url: config.url,
        username: config.username,
        password: config.password,
        save_path: config.save_path,
        advanced: config.advanced,
    })
}

#[tauri::command]
pub async fn get_qbittorrent_preferences(
    state: State<'_, AppState>,
    qb_state: State<'_, QbState>,
    manager: State<'_, ManagedQbittorrent>,
) -> Result<QbittorrentPreferences, String> {
    let config = load_config(state.inner()).map_err(to_message)?;
    let endpoint = resolve_endpoint(state.inner(), manager.inner(), &config)
        .await
        .map_err(to_message)?;

    let preferences = qbittorrent::preferences(qb_state.inner(), &endpoint)
        .await
        .map_err(to_message)?;

    Ok(QbittorrentPreferences {
        save_path: preferences.save_path,
    })
}

/// 保存下载目录。实例正在运行（或是外部 WebUI）时立即同步给 qBittorrent，
/// 否则只保存，下次启动内置实例时应用。
#[tauri::command]
pub async fn set_qbittorrent_save_path(
    state: State<'_, AppState>,
    qb_state: State<'_, QbState>,
    manager: State<'_, ManagedQbittorrent>,
    path: String,
) -> Result<QbittorrentPreferences, String> {
    let path = path.trim().to_string();

    let mut app_settings = settings::load(&state.data_root).map_err(to_message)?;
    app_settings.qbittorrent.save_path = (!path.is_empty()).then(|| path.clone());
    settings::save(&state.data_root, &app_settings).map_err(to_message)?;

    if !path.is_empty()
        && (app_settings.qbittorrent.mode == QBITTORRENT_MODE_EXTERNAL
            || manager.is_running())
    {
        let endpoint = resolve_endpoint(
            state.inner(),
            manager.inner(),
            &app_settings.qbittorrent,
        )
        .await
        .map_err(to_message)?;

        qbittorrent::set_save_path(qb_state.inner(), &endpoint, &path)
            .await
            .map_err(to_message)?;
    }

    Ok(QbittorrentPreferences { save_path: path })
}

/// 连接测试：内置模式会先启动 sidecar，再读取 qBittorrent 版本。
#[tauri::command]
pub async fn qbittorrent_test(
    state: State<'_, AppState>,
    qb_state: State<'_, QbState>,
    manager: State<'_, ManagedQbittorrent>,
) -> Result<QbittorrentStatus, String> {
    let config = load_config(state.inner()).map_err(to_message)?;
    let endpoint = resolve_endpoint(state.inner(), manager.inner(), &config)
        .await
        .map_err(to_message)?;

    let version = qbittorrent::version(qb_state.inner(), &endpoint)
        .await
        .map_err(to_message)?;

    Ok(QbittorrentStatus { version })
}

/// 启动或停止当前下载目录里的一个任务。
#[tauri::command]
pub async fn qbittorrent_set_torrent_running(
    app: AppHandle,
    state: State<'_, AppState>,
    qb_state: State<'_, QbState>,
    manager: State<'_, ManagedQbittorrent>,
    hash: String,
    running: bool,
) -> Result<(), String> {
    if running {
        ensure_download_dir(&app).map_err(to_message)?;
    }
    let config = load_config(state.inner()).map_err(to_message)?;
    let endpoint = resolve_endpoint(state.inner(), manager.inner(), &config)
        .await
        .map_err(to_message)?;

    qbittorrent::set_running(qb_state.inner(), &endpoint, &hash, running)
        .await
        .map_err(to_message)
}

/// 当前 WebUI 地址，用于在内嵌浏览器里打开 qBittorrent 自己的界面。
/// 内置模式会先确保 sidecar 已启动。
#[tauri::command]
pub async fn qbittorrent_webui_url(
    state: State<'_, AppState>,
    manager: State<'_, ManagedQbittorrent>,
) -> Result<String, String> {
    let config = load_config(state.inner()).map_err(to_message)?;
    let endpoint = resolve_endpoint(state.inner(), manager.inner(), &config)
        .await
        .map_err(to_message)?;

    Ok(endpoint.base)
}

/// 当前下载目录及其下的全部任务。
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TorrentListing {
    pub save_path: String,
    pub torrents: Vec<TorrentInfo>,
}

/// 读取当前下载目录下的全部任务，同时返回目录本身。
/// 前端据此在目录变化时先清空列表，避免旧目录的任务残留。
#[tauri::command]
pub async fn qbittorrent_list_torrents(
    state: State<'_, AppState>,
    qb_state: State<'_, QbState>,
    manager: State<'_, ManagedQbittorrent>,
) -> Result<TorrentListing, String> {
    let config = load_config(state.inner()).map_err(to_message)?;
    let endpoint = resolve_endpoint(state.inner(), manager.inner(), &config)
        .await
        .map_err(to_message)?;

    // 以 qBittorrent 当前的默认下载目录为准，避免前端状态过期。
    let preferences = qbittorrent::preferences(qb_state.inner(), &endpoint)
        .await
        .map_err(to_message)?;

    let torrents = qbittorrent::dir_torrents(
        qb_state.inner(),
        &endpoint,
        &preferences.save_path,
    )
    .await
    .map_err(to_message)?;

    Ok(TorrentListing {
        save_path: preferences.save_path,
        torrents,
    })
}

/// 内嵌浏览器捕获到磁力链接或 .torrent 链接后调用。
///
/// 在后台任务里添加种子，完成后通过 `qbittorrent-torrent` 事件通知前端，
/// 避免同步的导航回调里等网络请求。
pub fn accept_browser_torrent(app: &AppHandle, url: String, kind: &'static str) {
    let handle = app.clone();

    tauri::async_runtime::spawn(async move {
        let (ok, error) = match add_torrent_url(&handle, &url).await {
            Ok(()) => (true, None),
            Err(error) => (false, Some(format!("{error:#}"))),
        };

        let _ = handle.emit(
            "qbittorrent-torrent",
            TorrentAddedEvent {
                ok,
                kind: kind.to_string(),
                url,
                error,
            },
        );
    });
}

async fn add_torrent_url(app: &AppHandle, url: &str) -> AnyResult<()> {
    add_torrent_with_save_path(app, url, None).await
}

/// 添加种子；RSS 追番用 save_path 指定每部番的子目录。
pub(crate) async fn add_torrent_with_save_path(
    app: &AppHandle,
    url: &str,
    save_path: Option<&str>,
) -> AnyResult<()> {
    let state = app.state::<AppState>();
    let qb_state = app.state::<QbState>();
    let manager = app.state::<ManagedQbittorrent>();

    let config = load_config(state.inner())?;
    // 自定义保存目录不依赖全局「下载到」是否可用。
    if save_path.is_none() {
        ensure_download_dir(app)?;
    }
    let endpoint = resolve_endpoint(state.inner(), manager.inner(), &config).await?;

    qbittorrent::add_url(
        qb_state.inner(),
        &endpoint,
        url,
        save_path.or(config.save_path.as_deref()),
    )
    .await
}

/// 以文件内容添加种子；用于应用先抓取 .torrent、再交给 qBittorrent 的 RSS 流程。
pub(crate) async fn add_torrent_file_with_save_path(
    app: &AppHandle,
    bytes: &[u8],
    filename: &str,
    save_path: Option<&str>,
) -> AnyResult<()> {
    let state = app.state::<AppState>();
    let qb_state = app.state::<QbState>();
    let manager = app.state::<ManagedQbittorrent>();

    let config = load_config(state.inner())?;
    if save_path.is_none() {
        ensure_download_dir(app)?;
    }
    let endpoint = resolve_endpoint(state.inner(), manager.inner(), &config).await?;

    qbittorrent::add_file(
        qb_state.inner(),
        &endpoint,
        bytes,
        filename,
        save_path.or(config.save_path.as_deref()),
    )
    .await
}

fn load_config(state: &AppState) -> AnyResult<QbittorrentSettings> {
    Ok(settings::load(&state.data_root)?.qbittorrent)
}

/// 根据配置解析要连接的 WebUI。内置模式负责启动 sidecar。
pub(crate) async fn resolve_endpoint(
    state: &AppState,
    manager: &ManagedQbittorrent,
    config: &QbittorrentSettings,
) -> AnyResult<Endpoint> {
    if config.mode == QBITTORRENT_MODE_EXTERNAL {
        Ok(Endpoint::external(
            config.url.clone(),
            config.username.clone(),
            config.password.clone(),
        ))
    } else {
        let base = manager
            .ensure_started(&state.data_root, config.save_path.as_deref())
            .await?;
        Ok(Endpoint::local(base))
    }
}

fn to_message(error: anyhow::Error) -> String {
    format!("{error:#}")
}
