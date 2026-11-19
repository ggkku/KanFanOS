//! 文件系统监听：目录变化后自动做一次增量同步。
//!
//! 事件经过防抖合并；完整扫描仍是最终依据，因为系统通知可能丢失或合并。

use std::{
    path::{Path, PathBuf},
    sync::mpsc::{self, Receiver, RecvTimeoutError, Sender, TryRecvError},
    thread,
    time::{Duration, Instant},
};

use notify::{Event, RecursiveMode, Watcher};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

use crate::{
    media,
    qbittorrent::{self, Endpoint, ManagedQbittorrent, QbState},
    scanner,
    series_config,
    settings::{self, QBITTORRENT_MODE_EXTERNAL},
    state::AppState,
};

const IMAGE_EXTENSIONS: &[&str] =
    &["jpg", "jpeg", "png", "webp", "gif", "avif", "bmp"];

/// 安静这么久之后才触发同步，避开正在复制的大文件。
const DEBOUNCE: Duration = Duration::from_millis(1500);
const POLL: Duration = Duration::from_millis(250);
const RETRY: Duration = Duration::from_secs(5);
const HEALTH_CHECK: Duration = Duration::from_secs(30);
/// 有 qBittorrent 任务仍未完成时的兜底复查间隔。
///
/// 正常完成会有文件事件触发扫描；这个定时器只覆盖事件被合并/错过的情况。
/// 间隔放长一些，避免一个长期暂停的任务让整个库反复扫描。
const DOWNLOAD_RETRY: Duration = Duration::from_secs(180);
/// 查询 qBittorrent 下载状态的超时；WebUI 无响应时不能拖住自动同步。
const QB_QUERY_TIMEOUT: Duration = Duration::from_secs(3);

pub const SYNC_EVENT: &str = "library-sync";

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncEvent {
    pub library_id: i64,
    /// `watching` | `syncing` | `completed` | `offline` | `failed`
    pub phase: &'static str,
    pub series_found: Option<u64>,
    pub files_found: Option<u64>,
    pub missing: Option<usize>,
}

pub struct WatcherHandle {
    /// 线程通过通道断开感知停止，字段仅用于持有 Sender。
    _stop: Sender<()>,
}

/// 停止当前监听器。
pub fn stop(app: &AppHandle) {
    if let Ok(mut guard) = app.state::<AppState>().watcher.lock() {
        *guard = None;
    }
}

/// 启动或替换监听器。重复调用会先停止旧监听。
pub fn start(app: AppHandle, library_id: i64, root: PathBuf) {
    let (stop_tx, stop_rx) = mpsc::channel();

    {
        let state = app.state::<AppState>();
        let mut guard = match state.watcher.lock() {
            Ok(guard) => guard,
            Err(_) => {
                eprintln!("文件监听状态锁损坏，无法启动监听");
                return;
            }
        };

        // 停止旧监听，再挂上新句柄。
        *guard = None;
        *guard = Some(WatcherHandle { _stop: stop_tx });
    }

    thread::spawn(move || watcher_loop(app, library_id, root, stop_rx));
}

fn watcher_loop(app: AppHandle, library_id: i64, root: PathBuf, stop: Receiver<()>) {
    let (event_tx, event_rx) = mpsc::channel::<()>();

    let mut watcher = match notify::recommended_watcher(move |result: notify::Result<Event>| {
        match result {
            Ok(event) => {
                if is_relevant(&event) {
                    let _ = event_tx.send(());
                }
            }
            Err(error) => eprintln!("文件监听错误: {error}"),
        }
    }) {
        Ok(watcher) => watcher,
        Err(error) => {
            eprintln!("创建文件监听器失败: {error}");
            emit(&app, library_id, "failed", None);
            return;
        }
    };

    // 外接硬盘未接入时重试，期间不把索引标记为缺失。
    loop {
        if stopped(&stop) {
            return;
        }

        match watcher.watch(&root, RecursiveMode::Recursive) {
            Ok(()) => {
                // 目录恢复可用时重新授权封面图片访问（离线启动时可能失败过）。
                if let Err(error) = app
                    .asset_protocol_scope()
                    .allow_directory(&root, true)
                {
                    eprintln!("允许访问番剧目录失败: {error}");
                }

                emit(&app, library_id, "watching", None);
                break;
            }
            Err(error) => {
                eprintln!("监听目录失败 {}: {error}", root.display());
                emit(&app, library_id, "offline", None);
                if sleep_with_stop(&stop, RETRY) {
                    return;
                }
            }
        }
    }

    let mut pending = false;
    let mut deadline = Instant::now();
    let mut offline = false;
    let mut last_health = Instant::now();

    loop {
        if stopped(&stop) {
            break;
        }

        match event_rx.recv_timeout(POLL) {
            Ok(()) => {
                pending = true;
                // 已排定的补扫时间（文件静默期）只延后不提前：
                // 下载/复制期间的事件不应把扫描拉回几秒后。
                deadline = deadline.max(Instant::now() + DEBOUNCE);
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => break,
        }

        if last_health.elapsed() >= HEALTH_CHECK {
            last_health = Instant::now();
            let available = root.is_dir();
            if !available && !offline {
                offline = true;
                emit(&app, library_id, "offline", None);
            } else if available && offline {
                offline = false;
                emit(&app, library_id, "watching", None);
            }
        }

        if pending && Instant::now() >= deadline {
            pending = false;
            let retry = sync(&app, library_id, &root);
            // 扫描期间产生的事件已经包含在本次结果里。
            while event_rx.try_recv().is_ok() {}

            // 有文件仍在写入时，它们被本次扫描跳过；静默期结束后再补扫一次，
            // 不依赖下一次文件事件（下载刚完成时未必还有事件）。
            if let Some(delay) = retry {
                pending = true;
                deadline = Instant::now() + delay;
            }
        }
    }
}

/// 执行一次自动同步。
///
/// 返回值表示需要等待多久再补扫一次：扫描跳过了仍在写入的文件时，
/// 要等静默期结束把它们收录进来。
fn sync(app: &AppHandle, library_id: i64, root: &Path) -> Option<Duration> {
    if !root.is_dir() {
        emit(app, library_id, "offline", None);
        return None;
    }

    emit(app, library_id, "syncing", None);

    // qBittorrent 里未完成任务的文件不能进入选集页面；与文件静默期互补，
    // 能覆盖下载暂停、写入间隔很长等仅靠 mtime 判断不了的情况。
    let downloading = downloading_paths(app, root);

    match scanner::scan_quiet(app, library_id, &downloading) {
        Ok(summary) => {
            emit(app, library_id, "completed", Some(&summary));

            // 新文件需要补齐元数据和缩略图。
            if let Err(error) = media::start(app.clone(), library_id) {
                let message = format!("{error:#}");
                if !message.contains("已在进行中") {
                    eprintln!("自动同步后启动媒体处理失败: {message}");
                }
            }

            if summary.deferred > 0 {
                Some(scanner::AUTO_SETTLE)
            } else if summary.downloading > 0 {
                // 下载事件可能被合并/错过，定期复查完成状态。
                Some(DOWNLOAD_RETRY)
            } else {
                None
            }
        }
        Err(error) => {
            let message = format!("{error:#}");
            if message.contains("已有扫描任务") {
                // 手动扫描正在进行，它会覆盖这次变化。
                return None;
            }

            eprintln!("自动同步失败: {message}");
            let phase = if root.is_dir() { "failed" } else { "offline" };
            emit(app, library_id, phase, None);
            None
        }
    }
}

fn emit(
    app: &AppHandle,
    library_id: i64,
    phase: &'static str,
    summary: Option<&scanner::ScanSummary>,
) {
    let _ = app.emit(
        SYNC_EVENT,
        SyncEvent {
            library_id,
            phase,
            series_found: summary.map(|value| value.series_found),
            files_found: summary.map(|value| value.files_found),
            missing: summary.map(|value| value.missing),
        },
    );
}

fn stopped(stop: &Receiver<()>) -> bool {
    matches!(stop.try_recv(), Err(TryRecvError::Disconnected))
}

fn sleep_with_stop(stop: &Receiver<()>, duration: Duration) -> bool {
    let deadline = Instant::now() + duration;

    while Instant::now() < deadline {
        if stopped(stop) {
            return true;
        }
        thread::sleep(POLL.min(deadline.saturating_duration_since(Instant::now())));
    }

    false
}

/// 查询 qBittorrent 里仍在下载、且位于当前番剧目录内的路径。
///
/// 内置实例只在已经运行时查询，避免自动扫描顺手启动 qBittorrent；
/// 外部 WebUI 按配置连接。查询失败时返回空列表，退回文件静默期判断。
pub(crate) async fn downloading_paths_for(
    app: &AppHandle,
    root: &Path,
) -> Vec<PathBuf> {
    let state = app.state::<AppState>();
    let Ok(app_settings) = settings::load(&state.data_root) else {
        return Vec::new();
    };

    let endpoint =
        if app_settings.qbittorrent.mode == QBITTORRENT_MODE_EXTERNAL {
            Endpoint::external(
                app_settings.qbittorrent.url.clone(),
                app_settings.qbittorrent.username.clone(),
                app_settings.qbittorrent.password.clone(),
            )
        } else {
            let Some(base) = app
                .state::<ManagedQbittorrent>()
                .base_if_running()
            else {
                return Vec::new();
            };
            Endpoint::local(base)
        };

    let qb_state = app.state::<QbState>();
    let paths = match tokio::time::timeout(
        QB_QUERY_TIMEOUT,
        qbittorrent::downloading_files(qb_state.inner(), &endpoint),
    )
    .await
    {
        Ok(Ok(paths)) => paths,
        Ok(Err(error)) => {
            eprintln!("查询 qBittorrent 下载状态失败: {error:#}");
            Vec::new()
        }
        Err(_) => {
            eprintln!("查询 qBittorrent 下载状态超时");
            Vec::new()
        }
    };

    let root_key = path_key(root);
    paths
        .into_iter()
        .filter(|path| {
            let key = path_key(path);
            key == root_key
                || key
                    .strip_prefix(&root_key)
                    .is_some_and(|rest| rest.starts_with('\\'))
        })
        .collect()
}

/// 文件监听线程里的同步入口。
fn downloading_paths(app: &AppHandle, root: &Path) -> Vec<PathBuf> {
    tauri::async_runtime::block_on(downloading_paths_for(app, root))
}

/// 路径比较键：统一斜杠、去掉末尾分隔符并转小写。
fn path_key(path: &Path) -> String {
    path.to_string_lossy()
        .replace('/', "\\")
        .trim_end_matches('\\')
        .to_ascii_lowercase()
}

/// 目录本身、视频、封面图片或番剧配置的变化才触发同步。
fn is_relevant(event: &Event) -> bool {
    event.paths.iter().any(|path| {
        if path.file_name().and_then(|value| value.to_str())
            == Some(series_config::CONFIG_FILE)
        {
            return true;
        }

        match path.extension().and_then(|value| value.to_str()) {
            Some(extension) => {
                scanner::is_video_extension(extension)
                    || IMAGE_EXTENSIONS
                        .iter()
                        .any(|candidate| candidate.eq_ignore_ascii_case(extension))
            }
            // 目录没有扩展名。
            None => true,
        }
    })
}
