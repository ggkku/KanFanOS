//! 内置浏览器：每个标签页一个 WebView2 子 webview。
//!
//! 标签页的 webview label 是 `browser-<id>`，由前端持有 id 并在切换标签时
//! 显示其中一个、隐藏其余。切换/离开资源页只移动和缩放，不销毁，页面状态
//! （滚动、表单、播放）都保留。
//!
//! 磁力链接和 `.torrent` 链接会交给 qBittorrent；`target=_blank` 通过
//! `browser-new-tab` 事件交给前端开新标签页。

use std::{
    collections::HashSet,
    sync::{
        atomic::{AtomicU64, Ordering},
        LazyLock, Mutex,
    },
};

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use tauri::{
    webview::{DownloadEvent, NewWindowResponse, PageLoadEvent},
    AppHandle, Emitter, LogicalPosition, LogicalSize, Manager, WebviewBuilder, WebviewUrl,
};

/// 标签页 webview label 前缀。
pub const TAB_PREFIX: &str = "browser-";

/// 内嵌网页的默认缩放。桌面宽度下 100% 显示偏大，默认缩到 90%。
const PAGE_ZOOM: f64 = 0.9;

static NEXT_TAB_ID: AtomicU64 = AtomicU64::new(1);

/// 隐藏中的标签页 id。
///
/// 隐藏后忽略边界同步：切页时布局变化会触发一次迟到的 `set_tab_bounds`，
/// 如果它排在 `hide_tab` 之后执行，会把 webview 重新定位回可见区域。
static HIDDEN_TABS: LazyLock<Mutex<HashSet<u64>>> =
    LazyLock::new(|| Mutex::new(HashSet::new()));

fn lock_hidden() -> Result<std::sync::MutexGuard<'static, HashSet<u64>>> {
    HIDDEN_TABS.lock().map_err(|_| anyhow::anyhow!("浏览器状态锁损坏"))
}

fn tab_label(tab_id: u64) -> String {
    format!("{TAB_PREFIX}{tab_id}")
}

fn parse_tab_id(label: &str) -> Option<u64> {
    label.strip_prefix(TAB_PREFIX)?.parse().ok()
}

/// 后端当前存在的标签页，用于前端核对是否有多余的 webview。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowserTabInfo {
    pub tab_id: u64,
    pub url: String,
}

pub fn list_tabs(app: &AppHandle) -> Result<Vec<BrowserTabInfo>> {
    let mut tabs: Vec<BrowserTabInfo> = app
        .webviews()
        .into_iter()
        .filter_map(|(label, webview)| {
            let tab_id = parse_tab_id(&label)?;
            let url = webview
                .url()
                .map(|url| url.to_string())
                .unwrap_or_default();
            Some(BrowserTabInfo { tab_id, url })
        })
        .collect();

    tabs.sort_by_key(|tab| tab.tab_id);
    Ok(tabs)
}

/// 关闭所有不在 `keep` 里的标签页 webview。
///
/// 前端状态丢失、重复实例等情况可能留下没人跟踪的 webview，它们会一直
/// 画在窗口最上层。离开/进入资源页时调用一次兜底清理。
pub fn close_untracked(app: &AppHandle, keep: &[u64]) -> Result<()> {
    for (label, webview) in app.webviews() {
        let Some(tab_id) = parse_tab_id(&label) else {
            continue;
        };
        if keep.contains(&tab_id) {
            continue;
        }

        if let Ok(mut hidden) = HIDDEN_TABS.lock() {
            hidden.remove(&tab_id);
        }
        let _ = webview.close();
    }
    Ok(())
}

/// 前端占位区域的矩形（CSS 像素，相对主 WebView 视口）。
#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowserRect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

impl BrowserRect {
    fn position(self) -> LogicalPosition<f64> {
        LogicalPosition::new(self.x.max(0.0), self.y.max(0.0))
    }

    fn size(self) -> LogicalSize<f64> {
        LogicalSize::new(self.width.max(1.0), self.height.max(1.0))
    }
}

/// 标签页标题或 URL 变化。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct TabUpdated {
    tab_id: u64,
    title: Option<String>,
    url: String,
}

/// 页面请求打开新窗口（对应前端开新标签页）。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct NewTabRequest {
    url: String,
}

/// 规范化地址栏输入：缺少协议时补 `https://`，只允许 http / https。
fn parse_url(input: &str) -> Result<tauri::Url> {
    let trimmed = input.trim();
    // 空白标签页：没有默认首页，新建标签时不加载任何网站。
    if trimmed.is_empty() {
        return tauri::Url::parse("about:blank").context("无法创建空白页");
    }

    let candidate = if trimmed.contains("://") {
        trimmed.to_string()
    } else {
        format!("https://{trimmed}")
    };

    let url = tauri::Url::parse(&candidate)
        .with_context(|| format!("无法解析地址：{trimmed}"))?;

    match url.scheme() {
        "http" | "https" => Ok(url),
        scheme => bail!("不支持打开 {scheme} 链接"),
    }
}

/// 新建标签页，返回标签页 id。
pub fn open_tab(app: &AppHandle, url: &str, rect: BrowserRect) -> Result<u64> {
    let url = parse_url(url)?;
    let tab_id = NEXT_TAB_ID.fetch_add(1, Ordering::Relaxed);
    let label = tab_label(tab_id);

    let window = app.get_window("main").context("找不到主窗口")?;

    let navigation_handle = app.clone();
    let new_window_handle = app.clone();
    let download_handle = app.clone();
    let title_handle = app.clone();
    let load_handle = app.clone();

    let builder = WebviewBuilder::new(&label, WebviewUrl::External(url))
        .on_navigation(move |url| {
            if is_torrent_target(url) {
                accept_torrent(url, &navigation_handle);
                return false;
            }
            true
        })
        .on_new_window(move |url, _features| {
            if is_torrent_target(&url) {
                accept_torrent(&url, &new_window_handle);
            } else {
                let _ = new_window_handle.emit(
                    "browser-new-tab",
                    NewTabRequest {
                        url: url.to_string(),
                    },
                );
            }
            NewWindowResponse::Deny
        })
        .on_download(move |_webview, event| {
            if let DownloadEvent::Requested { url, .. } = event
                && is_torrent_target(&url)
            {
                accept_torrent(&url, &download_handle);
                // 取消 WebView2 自己的下载，改由 qBittorrent 处理。
                return false;
            }
            true
        })
        .on_document_title_changed(move |webview, title| {
            let url = webview
                .url()
                .map(|url| url.to_string())
                .unwrap_or_default();
            let _ = title_handle.emit(
                "browser-tab-updated",
                TabUpdated {
                    tab_id,
                    title: Some(title),
                    url,
                },
            );
        })
        .on_page_load(move |_webview, payload| {
            if payload.event() == PageLoadEvent::Finished {
                let _ = load_handle.emit(
                    "browser-tab-updated",
                    TabUpdated {
                        tab_id,
                        title: None,
                        url: payload.url().to_string(),
                    },
                );
            }
        });

    let webview = window.add_child(builder, rect.position(), rect.size())?;
    webview.set_zoom(PAGE_ZOOM)?;
    // 调试用：可以通过 CDP 在页面里读到自己的标签 id，方便核对孤儿 webview。
    let _ = webview.eval(format!("window.__kanfanosTabId = {tab_id};"));
    Ok(tab_id)
}

/// 显示标签页并更新位置；返回 false 表示 webview 已不存在。
pub fn show_tab(app: &AppHandle, tab_id: u64, rect: BrowserRect) -> Result<bool> {
    let Some(webview) = app.get_webview(&tab_label(tab_id)) else {
        return Ok(false);
    };

    // 和 hide / set_bounds 用同一把锁串行化，避免迟到的定位把隐藏结果覆盖。
    let mut hidden = lock_hidden()?;
    hidden.remove(&tab_id);
    webview.set_position(rect.position())?;
    webview.set_size(rect.size())?;
    webview.set_zoom(PAGE_ZOOM)?;
    Ok(true)
}

/// 隐藏标签页但不销毁它（类似最小化标签页）。
pub fn hide_tab(app: &AppHandle, tab_id: u64) -> Result<()> {
    let Some(webview) = app.get_webview(&tab_label(tab_id)) else {
        return Ok(());
    };

    {
        let mut hidden = lock_hidden()?;
        hidden.insert(tab_id);

        // 移到客户区外并缩到 1x1，避免盖住其他页面；页面本身继续保留。
        // 两个操作都尝试一遍，其中一个失败也不会留下全尺寸的覆盖层。
        let position = webview.set_position(LogicalPosition::new(-20000.0, -20000.0));
        let size = webview.set_size(LogicalSize::new(1.0, 1.0));
        position?;
        size?;
    }

    // 焦点还给主界面，避免隐藏的页面继续接收键盘输入。
    if let Some(main) = app.get_webview("main") {
        let _ = main.set_focus();
    }
    Ok(())
}

/// 更新标签页的位置和大小。webview 不存在或正在隐藏时忽略。
pub fn set_tab_bounds(app: &AppHandle, tab_id: u64, rect: BrowserRect) -> Result<()> {
    let Some(webview) = app.get_webview(&tab_label(tab_id)) else {
        return Ok(());
    };

    let hidden = lock_hidden()?;
    if hidden.contains(&tab_id) {
        return Ok(());
    }

    webview.set_position(rect.position())?;
    webview.set_size(rect.size())?;
    Ok(())
}

/// 标签页导航到新地址。
pub fn navigate_tab(app: &AppHandle, tab_id: u64, url: &str) -> Result<()> {
    let url = parse_url(url)?;
    let webview = app
        .get_webview(&tab_label(tab_id))
        .context("标签页不存在")?;

    webview.set_zoom(PAGE_ZOOM)?;
    webview.navigate(url)?;
    Ok(())
}

/// 关闭标签页。
pub fn close_tab(app: &AppHandle, tab_id: u64) -> Result<()> {
    if let Ok(mut hidden) = HIDDEN_TABS.lock() {
        hidden.remove(&tab_id);
    }
    if let Some(webview) = app.get_webview(&tab_label(tab_id)) {
        webview.close()?;
    }
    Ok(())
}

/// 磁力链接或 `.torrent` 文件都视为种子入口。
fn is_torrent_target(url: &tauri::Url) -> bool {
    match url.scheme() {
        "magnet" => true,
        "http" | "https" => url.path().to_ascii_lowercase().ends_with(".torrent"),
        _ => false,
    }
}

fn accept_torrent(url: &tauri::Url, app: &AppHandle) {
    let kind = if url.scheme() == "magnet" {
        "magnet"
    } else {
        "torrent"
    };
    crate::commands::resources::accept_browser_torrent(
        app,
        url.as_str().to_string(),
        kind,
    );
}
