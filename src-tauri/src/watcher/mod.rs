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

use crate::{media, scanner, series_config, state::AppState};

const IMAGE_EXTENSIONS: &[&str] =
    &["jpg", "jpeg", "png", "webp", "gif", "avif", "bmp"];

/// 安静这么久之后才触发同步，避开正在复制的大文件。
const DEBOUNCE: Duration = Duration::from_millis(1500);
const POLL: Duration = Duration::from_millis(250);
const RETRY: Duration = Duration::from_secs(5);
const HEALTH_CHECK: Duration = Duration::from_secs(30);

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
                deadline = Instant::now() + DEBOUNCE;
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
            sync(&app, library_id, &root);
            // 扫描期间产生的事件已经包含在本次结果里。
            while event_rx.try_recv().is_ok() {}
        }
    }
}

fn sync(app: &AppHandle, library_id: i64, root: &Path) {
    if !root.is_dir() {
        emit(app, library_id, "offline", None);
        return;
    }

    emit(app, library_id, "syncing", None);

    match scanner::scan_quiet(app, library_id) {
        Ok(summary) => {
            emit(app, library_id, "completed", Some(&summary));

            // 新文件需要补齐元数据和缩略图。
            if let Err(error) = media::start(app.clone(), library_id) {
                let message = format!("{error:#}");
                if !message.contains("已在进行中") {
                    eprintln!("自动同步后启动媒体处理失败: {message}");
                }
            }
        }
        Err(error) => {
            let message = format!("{error:#}");
            if message.contains("已有扫描任务") {
                // 手动扫描正在进行，它会覆盖这次变化。
                return;
            }

            eprintln!("自动同步失败: {message}");
            let phase = if root.is_dir() { "failed" } else { "offline" };
            emit(app, library_id, phase, None);
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
