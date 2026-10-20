use std::{
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        Mutex,
    },
};

use crate::{database::Database, watcher::WatcherHandle};

/// 后台任务的运行与取消标记。扫描和媒体处理各自持有一份。
pub struct TaskState {
    pub running: AtomicBool,
    pub cancel: AtomicBool,
}

impl TaskState {
    pub fn new() -> Self {
        Self {
            running: AtomicBool::new(false),
            cancel: AtomicBool::new(false),
        }
    }

    pub fn request_cancel(&self) -> bool {
        if self.running.load(Ordering::Acquire) {
            self.cancel.store(true, Ordering::Release);
            true
        } else {
            false
        }
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancel.load(Ordering::Acquire)
    }

    pub fn reset_cancel(&self) {
        self.cancel.store(false, Ordering::Release);
    }
}

impl Default for TaskState {
    fn default() -> Self {
        Self::new()
    }
}

pub type ScanState = TaskState;
pub type MediaState = TaskState;

pub struct AppState {
    pub db: Database,
    pub scan: ScanState,
    pub media: MediaState,
    /// 当前番剧目录的文件监听器。
    pub watcher: Mutex<Option<WatcherHandle>>,
    /// 应用数据根目录（`%LOCALAPPDATA%\KanFanOS`）。
    pub data_root: PathBuf,
}
