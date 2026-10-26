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

/// 内嵌网页的缩放百分比。
///
/// 默认 80%，让较小的浏览器区域也能得到接近桌面宽度的 CSS 视口；
/// 前端工具栏可以继续增减，后端对所有已创建标签页统一应用。
static PAGE_ZOOM_PERCENT: AtomicU64 = AtomicU64::new(80);

fn page_zoom() -> f64 {
    PAGE_ZOOM_PERCENT.load(Ordering::Relaxed) as f64 / 100.0
}

pub fn zoom_percent() -> u32 {
    PAGE_ZOOM_PERCENT.load(Ordering::Relaxed) as u32
}

/// 设置所有浏览器标签页的缩放比例，返回实际生效的百分比。
pub fn set_zoom(app: &AppHandle, percent: u32) -> Result<u32> {
    let percent = percent.clamp(30, 150);
    PAGE_ZOOM_PERCENT.store(percent as u64, Ordering::Relaxed);

    let zoom = percent as f64 / 100.0;
    let zoom_script = format!(
        "window.__kanfanosZoom = {zoom}; window.dispatchEvent(new Event('kanfanos-zoom'));"
    );
    for (label, webview) in app.webviews() {
        if label.starts_with(TAB_PREFIX) {
            webview.set_zoom(zoom)?;
            let _ = webview.eval(&zoom_script);
        }
    }
    Ok(percent)
}

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

    let window = crate::window::main_window(app).context("找不到主窗口")?;

    let navigation_handle = app.clone();
    let new_window_handle = app.clone();
    let download_handle = app.clone();
    let title_handle = app.clone();
    let load_handle = app.clone();

    let builder = WebviewBuilder::new(&label, WebviewUrl::External(url))
        // 右键菜单/Toast 需要按当前网页缩放反向补偿，保持 100% 大小。
        .initialization_script_for_all_frames(&format!(
            "window.__kanfanosZoom = {};",
            page_zoom()
        ))
        // 内嵌网页和 iframe 都不弹浏览器右键菜单。
        .initialization_script_for_all_frames(
            "window.addEventListener('contextmenu', function (event) { event.preventDefault(); }, true);",
        )
        // 兼容 target=_blank 图片/链接：不依赖 WebView2 的 on_new_window，
        // 直接把新地址交给主界面的标签栏。
        .initialization_script_for_all_frames(
            r#"
(function () {
  function proxy(href) {
    window.location.href =
      "https://kanfanos.invalid/open?url=" + encodeURIComponent(href);
  }
  document.addEventListener("click", function (event) {
    var node = event.target;
    if (!(node instanceof Element)) return;
    var link = node.closest('a[target]');
    if (!link || String(link.target).toLowerCase() !== "_blank") return;
    var href = link.href;
    if (!/^https?:\/\//i.test(href)) return;
    if (/^magnet:/i.test(href) || /\.torrent(?:[?#]|$)/i.test(href)) return;
    event.preventDefault();
    event.stopPropagation();
    var internals = window.__TAURI_INTERNALS__;
    if (internals && internals.invoke) {
      try {
        Promise.resolve(
          internals.invoke("browser_request_new_tab", { url: href })
        ).catch(function () { proxy(href); });
        return;
      } catch (error) {
        proxy(href);
        return;
      }
    }
    proxy(href);
  }, true);
})();
"#,
        )
        .initialization_script_for_all_frames(
            r#"
(function () {
  var host = null;
  var menu = null;
  var toast = null;
  var toastTimer = 0;
  var currentUrl = null;
  var currentText = null;

  function applyZoomScale() {
    if (!host) return;
    var zoom = Number(window.__kanfanosZoom) || 1;
    host.style.setProperty("--kf-inv-zoom", String(1 / zoom));
  }

  function ensureUi() {
    if (host) return;
    host = document.createElement("div");
    host.style.cssText =
      "position:fixed;left:0;top:0;width:100%;height:100%;" +
      "z-index:2147483647;pointer-events:none;";
    var root = host.attachShadow({ mode: "closed" });
    root.innerHTML =
      '<style>' +
      '.menu{position:fixed;display:none;min-width:230px;padding:5px;' +
      'transform:scale(var(--kf-inv-zoom,1));transform-origin:top left;' +
      'background:#18181b;color:#e5e5e5;border:1px solid #3f3f46;' +
      'border-radius:8px;box-shadow:0 10px 30px rgba(0,0,0,.45);' +
      'font:12px/1.5 "Microsoft YaHei",system-ui,sans-serif;' +
      'pointer-events:auto;user-select:none;}' +
      '.item{display:block;width:100%;padding:7px 10px;border:0;' +
      'border-radius:5px;background:transparent;color:#e5e5e5;' +
      'text-align:left;font:inherit;cursor:pointer;white-space:nowrap;}' +
      '.item:hover{background:#27272a;color:#fff;}' +
      '.toast{position:fixed;right:18px;bottom:22px;' +
      'transform:scale(var(--kf-inv-zoom,1));transform-origin:bottom right;' +
      'max-width:70vw;padding:8px 14px;border-radius:8px;display:none;' +
      'background:#18181b;color:#e5e5e5;border:1px solid #3f3f46;' +
      'box-shadow:0 8px 24px rgba(0,0,0,.4);font:12px/1.5 ' +
      '"Microsoft YaHei",system-ui,sans-serif;pointer-events:none;' +
      'white-space:nowrap;overflow:hidden;text-overflow:ellipsis;}' +
      '.toast.error{border-color:#7f1d1d;color:#fca5a5;}' +
      '</style>' +
      '<div class="menu" role="menu">' +
      '<button class="item" data-action="image" type="button">图片另存为至「下载到」目录</button>' +
      '<button class="item" data-action="folder" type="button">以选中文字创建番剧目录及配置文件</button>' +
      '</div>' +
      '<div class="toast"></div>';
    document.documentElement.appendChild(host);
    applyZoomScale();
    menu = root.querySelector(".menu");
    toast = root.querySelector(".toast");
    var imageButton = menu.querySelector('[data-action="image"]');
    var folderButton = menu.querySelector('[data-action="folder"]');
    imageButton.addEventListener("click", function (event) {
      event.preventDefault();
      event.stopPropagation();
      var url = currentUrl;
      hide();
      if (url) saveImage(url);
    });
    folderButton.addEventListener("click", function (event) {
      event.preventDefault();
      event.stopPropagation();
      var text = currentText;
      hide();
      if (text) createFolder(text);
    });
  }

  function hide() {
    currentUrl = null;
    currentText = null;
    if (menu) menu.style.display = "none";
  }

  function show(x, y, url, text) {
    ensureUi();
    currentUrl = url;
    currentText = text;
    var imageButton = menu.querySelector('[data-action="image"]');
    var folderButton = menu.querySelector('[data-action="folder"]');
    imageButton.style.display = url ? "block" : "none";
    folderButton.style.display = text ? "block" : "none";
    if (!url && !text) {
      menu.style.display = "none";
      return;
    }
    menu.style.left = "0px";
    menu.style.top = "0px";
    menu.style.display = "block";
    var rect = menu.getBoundingClientRect();
    var left = Math.max(6, Math.min(x, window.innerWidth - rect.width - 6));
    var top = Math.max(6, Math.min(y, window.innerHeight - rect.height - 6));
    menu.style.left = left + "px";
    menu.style.top = top + "px";
  }

  function showToast(text, ok) {
    ensureUi();
    toast.textContent = text;
    toast.classList.toggle("error", !ok);
    toast.style.display = "block";
    window.clearTimeout(toastTimer);
    toastTimer = window.setTimeout(function () {
      toast.style.display = "none";
    }, 4000);
  }

  function imageUrlFor(img) {
    var raw =
      img.currentSrc ||
      img.src ||
      img.getAttribute("data-src") ||
      img.getAttribute("data-original") ||
      "";
    if (!raw) return null;
    try {
      return new URL(raw, document.baseURI).href;
    } catch (error) {
      return null;
    }
  }

  function selectedText() {
    var selection = window.getSelection();
    if (!selection || selection.isCollapsed) return null;
    var text = selection.toString().trim();
    if (!text) return null;
    return text.slice(0, 300);
  }

  function proxyCreateFolder(text) {
    try {
      window.location.href =
        "https://kanfanos.invalid/create-folder?text=" +
        encodeURIComponent(text);
    } catch (error) {
      showToast("创建文件夹失败：" + error, false);
    }
  }

  function createFolder(text) {
    var internals = window.__TAURI_INTERNALS__;
    if (!internals || !internals.invoke) {
      proxyCreateFolder(text);
      return;
    }

    var request;
    try {
      request = Promise.resolve(
        internals.invoke("browser_create_folder", { text: text })
      );
    } catch (error) {
      proxyCreateFolder(text);
      return;
    }

    request.catch(function (error) {
      var message = String(error);
      if (message.indexOf("选中的下载目录不存在") >= 0) {
        // 主界面统一用右下角红色 Toast 提示。
        return;
      }
      if (/not allowed|not found|unknown command|acl|ipc/i.test(message)) {
        proxyCreateFolder(text);
      } else {
        showToast("创建文件夹失败：" + message, false);
      }
    });
  }

  function proxySave(url) {
    try {
      window.location.href =
        "https://kanfanos.invalid/save-image?url=" +
        encodeURIComponent(url) +
        "&referer=" +
        encodeURIComponent(location.href);
    } catch (error) {
      showToast("保存失败：" + error, false);
    }
  }

  function saveImage(url) {
    var internals = window.__TAURI_INTERNALS__;
    if (!internals || !internals.invoke) {
      proxySave(url);
      return;
    }

    var request;
    try {
      request = Promise.resolve(
        internals.invoke("browser_save_image", {
          url: url,
          referer: location.href,
          userAgent: navigator.userAgent
        })
      );
    } catch (error) {
      proxySave(url);
      return;
    }

    request.catch(function (error) {
      var message = String(error);
      if (message.indexOf("选中的下载目录不存在") >= 0) {
        // 主界面统一用右下角红色 Toast 提示。
        return;
      }
      if (/not allowed|not found|unknown command|acl|ipc/i.test(message)) {
        proxySave(url);
      } else {
        showToast("保存失败：" + message, false);
      }
    });
  }

  document.addEventListener(
    "contextmenu",
    function (event) {
      event.preventDefault();
      var node = event.target;
      if (!(node instanceof Element)) {
        hide();
        return;
      }
      var img = node.closest("img");
      var url = img ? imageUrlFor(img) : null;
      var text = selectedText();
      if (!url && !text) {
        hide();
        return;
      }
      event.stopPropagation();
      show(event.clientX, event.clientY, url, text);
    },
    true
  );

  document.addEventListener(
    "mousedown",
    function (event) {
      if (!currentUrl && !currentText) return;
      var path = event.composedPath ? event.composedPath() : [];
      if (host && path.indexOf(host) >= 0) return;
      hide();
    },
    true
  );

  document.addEventListener("scroll", hide, true);
  document.addEventListener(
    "keydown",
    function (event) {
      if (event.key === "Escape") hide();
    },
    true
  );
  window.addEventListener("blur", hide);
  window.addEventListener("kanfanos-zoom", applyZoomScale);
})();
"#,
        )
        .on_navigation(move |url| {
            if let Some(text) = folder_create_proxy_target(url) {
                crate::commands::resources::spawn_browser_folder_create(
                    navigation_handle.clone(),
                    text,
                );
                return false;
            }
            if let Some((target, referer)) = image_save_proxy_target(url) {
                crate::commands::resources::spawn_browser_image_save(
                    navigation_handle.clone(),
                    target,
                    referer,
                );
                return false;
            }
            if let Some(target) = new_tab_proxy_target(url) {
                let _ = navigation_handle.emit(
                    "browser-new-tab",
                    NewTabRequest {
                        url: target.to_string(),
                    },
                );
                return false;
            }
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
    webview.set_zoom(page_zoom())?;
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
    webview.set_zoom(page_zoom())?;
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

/// 子 webview 自己请求打开新标签页。
///
/// WebView2 的 `target=_blank` 在部分页面/子窗口组合下不会触发
/// `on_new_window`，所以初始化脚本会拦截这类点击，通过 IPC 回到这里。
pub fn request_new_tab(app: &AppHandle, url: &str) -> Result<()> {
    let url = parse_url(url)?;
    app.emit(
        "browser-new-tab",
        NewTabRequest {
            url: url.to_string(),
        },
    )?;
    Ok(())
}

/// 标签页导航到新地址。
pub fn navigate_tab(app: &AppHandle, tab_id: u64, url: &str) -> Result<()> {
    let url = parse_url(url)?;
    let webview = app
        .get_webview(&tab_label(tab_id))
        .context("标签页不存在")?;

    webview.set_zoom(page_zoom())?;
    webview.navigate(url)?;
    Ok(())
}

/// 标签页后退。
pub fn go_back(app: &AppHandle, tab_id: u64) -> Result<()> {
    let webview = app
        .get_webview(&tab_label(tab_id))
        .context("标签页不存在")?;
    webview.eval("history.back()")?;
    Ok(())
}

/// 标签页重新加载。
pub fn reload_tab(app: &AppHandle, tab_id: u64) -> Result<()> {
    let webview = app
        .get_webview(&tab_label(tab_id))
        .context("标签页不存在")?;
    webview.set_zoom(page_zoom())?;
    webview.reload()?;
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

/// 主 webview 通过这个保留域名请求新标签页，避免依赖远程 webview 的 IPC。
fn new_tab_proxy_target(url: &tauri::Url) -> Option<tauri::Url> {
    if url.scheme() != "https"
        || url.host_str() != Some("kanfanos.invalid")
        || url.path() != "/open"
    {
        return None;
    }

    let (_, value) = url
        .query_pairs()
        .find(|(key, _)| key == "url")?;
    let target = tauri::Url::parse(value.as_ref()).ok()?;
    if target.scheme() != "http" && target.scheme() != "https" {
        return None;
    }
    Some(target)
}

/// 选中文字建文件夹代理：`/create-folder?text=...`。
fn folder_create_proxy_target(url: &tauri::Url) -> Option<String> {
    if url.scheme() != "https"
        || url.host_str() != Some("kanfanos.invalid")
        || url.path() != "/create-folder"
    {
        return None;
    }

    let (_, value) = url
        .query_pairs()
        .find(|(key, _)| key == "text")?;
    let text = value.trim();
    if text.is_empty() {
        return None;
    }
    Some(text.to_string())
}

/// 图片保存代理：`/save-image?url=...&referer=...`。
fn image_save_proxy_target(url: &tauri::Url) -> Option<(String, Option<String>)> {
    if url.scheme() != "https"
        || url.host_str() != Some("kanfanos.invalid")
        || url.path() != "/save-image"
    {
        return None;
    }

    let mut target = None;
    let mut referer = None;
    for (key, value) in url.query_pairs() {
        match key.as_ref() {
            "url" => target = Some(value.into_owned()),
            "referer" => referer = Some(value.into_owned()),
            _ => {}
        }
    }

    let target = target?;
    if !target.starts_with("http://") && !target.starts_with("https://") {
        return None;
    }
    Some((target, referer))
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
