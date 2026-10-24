//! 资源页命令：内嵌浏览器与 qBittorrent WebUI。

use anyhow::Result as AnyResult;
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};

use crate::{
    browser::{self, BrowserRect},
    qbittorrent::{self, Endpoint, ManagedQbittorrent, QbState, TorrentInfo},
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
    app_settings.qbittorrent = QbittorrentSettings {
        mode,
        url: url.trim().to_string(),
        username: username.trim().to_string(),
        password,
        save_path,
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
    state: State<'_, AppState>,
    qb_state: State<'_, QbState>,
    manager: State<'_, ManagedQbittorrent>,
    hash: String,
    running: bool,
) -> Result<(), String> {
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
    let state = app.state::<AppState>();
    let qb_state = app.state::<QbState>();
    let manager = app.state::<ManagedQbittorrent>();

    let config = load_config(state.inner())?;
    let endpoint = resolve_endpoint(state.inner(), manager.inner(), &config).await?;

    qbittorrent::add_url(
        qb_state.inner(),
        &endpoint,
        url,
        config.save_path.as_deref(),
    )
    .await
}

fn load_config(state: &AppState) -> AnyResult<QbittorrentSettings> {
    Ok(settings::load(&state.data_root)?.qbittorrent)
}

/// 根据配置解析要连接的 WebUI。内置模式负责启动 sidecar。
async fn resolve_endpoint(
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
