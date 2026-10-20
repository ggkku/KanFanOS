//! 目录扫描：按「档期 → 番剧 → 剧集」的层级建立索引。
//!
//! 支持的目录结构：
//! - `2026年7月新番/番剧名/视频`：第一层是档期，第二层是番剧；
//! - `番剧名/视频`：没有档期信息，归入「未知时间」；
//! - 档期目录下直接放视频时，这些视频归入以档期目录命名的番剧。

use std::{
    collections::HashSet,
    fs,
    path::{Path, PathBuf},
    sync::atomic::Ordering,
    time::Instant,
};

use anyhow::{bail, Context, Result};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};
use walkdir::WalkDir;

use crate::{
    catalog::{schedule_label, UNKNOWN_CATEGORY},
    database::videos::ScannedFile,
    series_config,
    state::{AppState, ScanState},
    time::{now_ms, system_time_to_ms},
};

/// 仅用于初步筛选，最终格式由 FFprobe 判定。
pub(crate) const VIDEO_EXTENSIONS: &[&str] = &[
    "mp4", "mkv", "mov", "avi", "wmv", "webm", "m4v", "ts", "mpg", "mpeg", "flv",
];

pub(crate) fn is_video_extension(extension: &str) -> bool {
    VIDEO_EXTENSIONS
        .iter()
        .any(|candidate| candidate.eq_ignore_ascii_case(extension))
}

/// 每批写入条数，见设计文档 §5.2。
const BATCH_SIZE: usize = 1000;

/// 小目录不足一批时，也保证进度事件不会长时间静默。
const PROGRESS_INTERVAL_MS: u128 = 300;

pub const PROGRESS_EVENT: &str = "scan-progress";

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanProgress {
    pub library_id: i64,
    /// `scanning` | `completed` | `cancelled`
    pub phase: &'static str,
    pub series_found: u64,
    pub files_found: u64,
    pub errors: u64,
    pub current_path: Option<String>,
    pub elapsed_ms: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanSummary {
    pub library_id: i64,
    pub series_found: u64,
    pub files_found: u64,
    pub files_added: u64,
    pub errors: u64,
    pub missing: usize,
    pub cancelled: bool,
    pub elapsed_ms: u64,
}

struct ScanGuard<'a> {
    scan: &'a ScanState,
}

impl Drop for ScanGuard<'_> {
    fn drop(&mut self) {
        self.scan.cancel.store(false, Ordering::Release);
        self.scan.running.store(false, Ordering::Release);
    }
}

pub fn scan(app: &AppHandle, library_id: i64) -> Result<ScanSummary> {
    let state = app.state::<AppState>();

    scan_with_state(state.inner(), library_id, &mut |progress| {
        // 事件发送失败不应中断扫描（窗口可能已关闭）。
        let _ = app.emit(PROGRESS_EVENT, progress);
    })
}

/// 文件监听触发的静默扫描：不发送逐条进度事件，避免界面闪烁。
pub fn scan_quiet(app: &AppHandle, library_id: i64) -> Result<ScanSummary> {
    let state = app.state::<AppState>();
    scan_with_state(state.inner(), library_id, &mut |_| {})
}

fn scan_with_state(
    state: &AppState,
    library_id: i64,
    on_progress: &mut dyn FnMut(ScanProgress),
) -> Result<ScanSummary> {
    if state
        .scan
        .running
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        bail!("已有扫描任务正在执行");
    }

    let _guard = ScanGuard {
        scan: &state.scan,
    };

    let result = run(state, library_id, on_progress);
    if result.is_err() {
        // 出错时不留下半截 checking 状态。
        let _ = state.db.abort_library_scan(library_id);
    }
    result
}

struct ScanContext<'a> {
    state: &'a AppState,
    library_id: i64,
    on_progress: &'a mut dyn FnMut(ScanProgress),
    started: Instant,
    last_emit: Instant,
    series_found: u64,
    files_found: u64,
    errors: u64,
    batch: Vec<ScannedFile>,
    seen_categories: HashSet<String>,
}

impl ScanContext<'_> {
    fn push_file(&mut self, file: ScannedFile, current: &Path) -> Result<()> {
        self.batch.push(file);
        self.files_found += 1;

        if self.batch.len() >= BATCH_SIZE {
            self.flush()?;
            self.emit("scanning", Some(current.to_string_lossy().into_owned()));
        } else if self.last_emit.elapsed().as_millis() >= PROGRESS_INTERVAL_MS {
            self.emit("scanning", None);
        }
        Ok(())
    }

    /// 记录本次扫描看到的档期，空目录也要保留在侧栏。
    fn note_category(&mut self, label: &str) -> Result<()> {
        if self.seen_categories.insert(label.to_string()) {
            self.state
                .db
                .upsert_category(self.library_id, label, now_ms())?;
        }
        Ok(())
    }

    fn flush(&mut self) -> Result<()> {
        if self.batch.is_empty() {
            return Ok(());
        }
        self.state
            .db
            .upsert_videos(self.library_id, &self.batch, now_ms())?;
        self.batch.clear();
        Ok(())
    }

    fn emit(&mut self, phase: &'static str, current_path: Option<String>) {
        self.last_emit = Instant::now();
        (self.on_progress)(ScanProgress {
            library_id: self.library_id,
            phase,
            series_found: self.series_found,
            files_found: self.files_found,
            errors: self.errors,
            current_path,
            elapsed_ms: self.started.elapsed().as_millis() as u64,
        });
    }

}

fn run(
    state: &AppState,
    library_id: i64,
    on_progress: &mut dyn FnMut(ScanProgress),
) -> Result<ScanSummary> {
    let started = Instant::now();
    let root = state.db.library_root(library_id)?;
    let root_path = PathBuf::from(&root);

    if !root_path.is_dir() {
        bail!("目录不存在或无法访问：{root}");
    }

    let rows_before = state.db.video_row_count(library_id)?;
    state.db.begin_library_scan(library_id)?;

    let mut ctx = ScanContext {
        state,
        library_id,
        on_progress,
        started,
        last_emit: Instant::now(),
        series_found: 0,
        files_found: 0,
        errors: 0,
        batch: Vec::with_capacity(BATCH_SIZE),
        seen_categories: HashSet::new(),
    };
    ctx.emit("scanning", None);

    let entries = fs::read_dir(&root_path)
        .with_context(|| format!("读取番剧目录失败: {}", root_path.display()))?;

    'entries: for entry in entries {
        if state.scan.is_cancelled() {
            break;
        }

        let entry = match entry {
            Ok(entry) => entry,
            Err(_) => {
                ctx.errors += 1;
                continue;
            }
        };

        let is_dir = match entry.file_type() {
            Ok(file_type) => file_type.is_dir(),
            Err(_) => {
                ctx.errors += 1;
                continue;
            }
        };
        if !is_dir {
            continue;
        }

        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') {
            continue;
        }
        let path = entry.path();

        if let Some(category) = schedule_label(&name) {
            // 时间分类目录：子目录是番剧，直接放在这里的视频单独成一部番剧。
            ctx.note_category(&category)?;

            let children = match fs::read_dir(&path) {
                Ok(children) => children,
                Err(_) => {
                    ctx.errors += 1;
                    continue;
                }
            };

            let mut loose_files = Vec::new();
            for child in children {
                if state.scan.is_cancelled() {
                    break 'entries;
                }

                let child = match child {
                    Ok(child) => child,
                    Err(_) => {
                        ctx.errors += 1;
                        continue;
                    }
                };

                let child_is_dir = match child.file_type() {
                    Ok(file_type) => file_type.is_dir(),
                    Err(_) => {
                        ctx.errors += 1;
                        continue;
                    }
                };

                if child_is_dir {
                    let series_name = child.file_name().to_string_lossy().into_owned();
                    if series_name.starts_with('.') {
                        continue;
                    }
                    scan_series(&mut ctx, &category, &series_name, &child.path())?;
                } else if video_extension(&child.path()).is_some() {
                    loose_files.push(child.path());
                }
            }

            if !loose_files.is_empty() {
                scan_loose_series(&mut ctx, &category, &name, &path, loose_files)?;
            }
        } else {
            // 没有档期信息，整个目录视作一部番剧。
            ctx.note_category(UNKNOWN_CATEGORY)?;
            scan_series(&mut ctx, UNKNOWN_CATEGORY, &name, &path)?;
        }
    }

    ctx.flush()?;
    let cancelled = state.scan.is_cancelled();

    let missing = if cancelled {
        state.db.abort_library_scan(library_id)?;
        0
    } else {
        // 完整扫描才清理已消失的档期目录。
        let keep: Vec<String> = ctx.seen_categories.iter().cloned().collect();
        state.db.prune_categories(library_id, &keep)?;
        state.db.finish_library_scan(library_id, now_ms())?
    };

    let rows_after = state.db.video_row_count(library_id)?;
    let summary = ScanSummary {
        library_id,
        series_found: ctx.series_found,
        files_found: ctx.files_found,
        files_added: (rows_after - rows_before).max(0) as u64,
        errors: ctx.errors,
        missing,
        cancelled,
        elapsed_ms: started.elapsed().as_millis() as u64,
    };

    ctx.emit(if cancelled { "cancelled" } else { "completed" }, None);
    Ok(summary)
}

/// 扫描一个番剧目录（含子目录）。
fn scan_series(
    ctx: &mut ScanContext,
    category: &str,
    name: &str,
    dir: &Path,
) -> Result<()> {
    let series_path = dir.to_string_lossy().into_owned();
    let config = series_config::resolve(dir, name);
    let aliases = config.aliases.join("\n");

    let series_id = ctx.state.db.upsert_series(
        ctx.library_id,
        category,
        &config.name,
        &series_path,
        &aliases,
        config.cover_path.as_deref(),
        config.cover_version,
        now_ms(),
    )?;
    ctx.series_found += 1;

    for entry in WalkDir::new(dir).follow_links(false) {
        if ctx.state.scan.is_cancelled() {
            break;
        }

        let entry = match entry {
            Ok(entry) => entry,
            Err(_) => {
                // 权限不足或目录读取失败：记录后继续，不中断整次扫描。
                ctx.errors += 1;
                continue;
            }
        };

        if !entry.file_type().is_file() {
            continue;
        }

        let Some(extension) = video_extension(entry.path()) else {
            continue;
        };

        let metadata = match entry.metadata() {
            Ok(metadata) => metadata,
            Err(_) => {
                ctx.errors += 1;
                continue;
            }
        };

        let path = entry.path();
        ctx.push_file(
            ScannedFile {
                series_id,
                path: path.to_string_lossy().into_owned(),
                filename: entry.file_name().to_string_lossy().into_owned(),
                extension,
                file_size: metadata.len().min(i64::MAX as u64) as i64,
                modified_at: metadata.modified().ok().and_then(system_time_to_ms),
            },
            path,
        )?;
    }

    ctx.flush()?;
    Ok(())
}

/// 档期目录下直接散放的文件。
fn scan_loose_series(
    ctx: &mut ScanContext,
    category: &str,
    name: &str,
    dir: &Path,
    files: Vec<PathBuf>,
) -> Result<()> {
    let series_path = dir.to_string_lossy().into_owned();
    let series_id = ctx.state.db.upsert_series(
        ctx.library_id,
        category,
        name,
        &series_path,
        "",
        None,
        None,
        now_ms(),
    )?;
    ctx.series_found += 1;

    for path in files {
        if ctx.state.scan.is_cancelled() {
            break;
        }

        let Some(extension) = video_extension(&path) else {
            continue;
        };
        let metadata = match fs::metadata(&path) {
            Ok(metadata) => metadata,
            Err(_) => {
                ctx.errors += 1;
                continue;
            }
        };
        let filename = path
            .file_name()
            .map(|value| value.to_string_lossy().into_owned())
            .unwrap_or_default();

        ctx.push_file(
            ScannedFile {
                series_id,
                path: path.to_string_lossy().into_owned(),
                filename,
                extension,
                file_size: metadata.len().min(i64::MAX as u64) as i64,
                modified_at: metadata.modified().ok().and_then(system_time_to_ms),
            },
            &path,
        )?;
    }

    ctx.flush()?;
    Ok(())
}

fn video_extension(path: &Path) -> Option<String> {
    let extension = path.extension()?.to_str()?.to_ascii_lowercase();
    is_video_extension(&extension).then_some(extension)
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        path::PathBuf,
        sync::atomic::Ordering,
        time::{SystemTime, UNIX_EPOCH},
    };

    use super::scan_with_state;
    use crate::{
        catalog::UNKNOWN_CATEGORY,
        database::Database,
        state::{AppState, ScanState},
        time::now_ms,
    };

    fn test_dir(name: &str) -> PathBuf {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("kanfanos-{name}-{stamp}"))
    }

    fn open_state(root: &std::path::Path) -> (AppState, i64) {
        let db = Database::open(&root.join("library.sqlite")).unwrap();
        let library = db
            .add_library(root.to_str().unwrap(), "test", now_ms())
            .unwrap();
        (
            AppState {
                db,
                scan: ScanState::new(),
                media: crate::state::MediaState::new(),
                watcher: std::sync::Mutex::new(None),
                data_root: root.join("appdata"),
            },
            library.id,
        )
    }

    #[test]
    fn scans_schedule_and_unknown_series() {
        let root = test_dir("scan");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("2026年7月新番").join("测试番")).unwrap();
        fs::create_dir_all(root.join("2025年10月新番")).unwrap();
        fs::create_dir_all(root.join("另一部番")).unwrap();

        fs::write(root.join("2026年7月新番").join("测试番").join("第1集.mp4"), b"1").unwrap();
        fs::write(root.join("2026年7月新番").join("测试番").join("第2集.mkv"), b"2").unwrap();
        fs::write(root.join("另一部番").join("第3集.mov"), b"3").unwrap();
        fs::write(root.join("note.txt"), b"ignored").unwrap();

        let (state, library_id) = open_state(&root);
        let mut noop = |_| {};

        let first = scan_with_state(&state, library_id, &mut noop).unwrap();
        assert_eq!(first.series_found, 2);
        assert_eq!(first.files_found, 3);
        assert_eq!(first.files_added, 3);
        assert_eq!(first.missing, 0);
        assert!(!first.cancelled);

        let categories = state.db.list_categories(library_id).unwrap();
        assert_eq!(categories.len(), 3);
        assert_eq!(categories[0].label, "2026年7月");
        assert_eq!(categories[0].count, 1);
        // 空档期目录也要保留在侧栏。
        assert_eq!(categories[1].label, "2025年10月");
        assert_eq!(categories[1].count, 0);
        assert_eq!(categories[2].label, UNKNOWN_CATEGORY);

        let scheduled = state.db.list_series_page(library_id, "2026年7月", "name", 100, 0).unwrap().items;
        assert_eq!(scheduled.len(), 1);
        assert_eq!(scheduled[0].name, "测试番");
        assert_eq!(scheduled[0].episode_count, 2);

        // 第一次扫描在每个番剧文件夹内生成配置。
        let test_config = root
            .join("2026年7月新番")
            .join("测试番")
            .join("kanfanos.toml");
        let other_config = root.join("另一部番").join("kanfanos.toml");
        assert!(test_config.is_file());
        assert!(other_config.is_file());

        // 手动编辑配置后重新扫描：显示名、别名和封面生效。
        fs::write(
            &test_config,
            "name = \"测试番（自定义）\"\naliases = [\"别名甲\", \"Alias B\"]\ncover = \"cover.jpg\"\n",
        )
        .unwrap();
        fs::write(
            root.join("2026年7月新番").join("测试番").join("cover.jpg"),
            b"jpg",
        )
        .unwrap();

        let _ = scan_with_state(&state, library_id, &mut noop).unwrap();

        let edited = state
            .db
            .list_series_page(library_id, "2026年7月", "name", 100, 0)
            .unwrap()
            .items;
        assert_eq!(edited[0].name, "测试番（自定义）");
        assert!(edited[0]
            .cover_path
            .as_deref()
            .unwrap()
            .ends_with("cover.jpg"));
        assert!(edited[0].cover_version.is_some());

        let by_alias = state.db.search_series(library_id, "别名甲", 10, 0).unwrap();
        assert_eq!(by_alias.total, 1);
        assert_eq!(by_alias.items[0].name, "测试番（自定义）");

        // 增量：删除一集、新增一集。
        fs::remove_file(root.join("2026年7月新番").join("测试番").join("第2集.mkv")).unwrap();
        fs::write(root.join("2026年7月新番").join("测试番").join("第3集.mp4"), b"3").unwrap();

        let second = scan_with_state(&state, library_id, &mut noop).unwrap();
        assert_eq!(second.files_found, 3);
        assert_eq!(second.files_added, 1);
        assert_eq!(second.missing, 1);

        let scheduled = state.db.list_series_page(library_id, "2026年7月", "name", 100, 0).unwrap().items;
        assert_eq!(scheduled[0].episode_count, 2);

        drop(state);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn cancelled_scan_does_not_mark_missing() {
        let root = test_dir("cancel");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("番剧")).unwrap();
        fs::write(root.join("番剧").join("第1集.mp4"), b"1").unwrap();

        let (state, library_id) = open_state(&root);
        state.scan.cancel.store(true, Ordering::Release);

        let summary = scan_with_state(&state, library_id, &mut |_| {}).unwrap();
        assert!(summary.cancelled);
        assert_eq!(summary.missing, 0);

        drop(state);
        let _ = fs::remove_dir_all(&root);
    }

    /// 性能基准：
    /// `KANFANOS_BENCH_FILES=10000 cargo test benchmark_scan_many_files -- --ignored --nocapture`
    #[test]
    #[ignore = "性能基准，需要手动指定文件数"]
    fn benchmark_scan_many_files() {
        use std::time::Instant;

        let total: usize = std::env::var("KANFANOS_BENCH_FILES")
            .ok()
            .and_then(|value| value.parse().ok())
            .unwrap_or(10_000);

        let root = test_dir("bench");
        let _ = fs::remove_dir_all(&root);

        let categories = 10;
        let series_per_category = 10;
        let episodes_per_series =
            (total / (categories * series_per_category)).max(1);

        for category_index in 0..categories {
            let category =
                root.join(format!("2025年{}月新番", category_index + 1));

            for series_index in 0..series_per_category {
                let series = category
                    .join(format!("番剧{category_index:02}-{series_index:02}"));
                fs::create_dir_all(&series).unwrap();

                for episode_index in 0..episodes_per_series {
                    fs::write(
                        series.join(format!("第{episode_index:03}集.mp4")),
                        b"",
                    )
                    .unwrap();
                }
            }
        }

        let (state, library_id) = open_state(&root);
        let expected = categories * series_per_category * episodes_per_series;

        let started = Instant::now();
        let summary = scan_with_state(&state, library_id, &mut |_| {}).unwrap();
        let elapsed = started.elapsed();

        eprintln!(
            "benchmark: {} files / {} series in {:?} ({:.0} files/s)",
            summary.files_found,
            summary.series_found,
            elapsed,
            summary.files_found as f64 / elapsed.as_secs_f64().max(0.001)
        );

        assert_eq!(summary.files_found as usize, expected);

        drop(state);
        let _ = fs::remove_dir_all(&root);
    }

    /// 手动验证真实目录结构：
    /// `KANFANOS_TEST_DIR=... cargo test scans_real_directory -- --ignored --nocapture`
    #[test]
    #[ignore = "需要 KANFANOS_TEST_DIR 指向真实番剧目录"]
    fn scans_real_directory() {
        let Ok(path) = std::env::var("KANFANOS_TEST_DIR") else {
            return;
        };
        let library_root = PathBuf::from(path);
        let work = test_dir("real");
        fs::create_dir_all(&work).unwrap();

        let db = Database::open(&work.join("library.sqlite")).unwrap();
        let library = db
            .add_library(library_root.to_str().unwrap(), "real", now_ms())
            .unwrap();
        let state = AppState {
            db,
            scan: ScanState::new(),
            media: crate::state::MediaState::new(),
            watcher: std::sync::Mutex::new(None),
            data_root: work.join("appdata"),
        };

        let summary = scan_with_state(&state, library.id, &mut |_| {}).unwrap();
        eprintln!("summary: {summary:#?}");

        for category in state.db.list_categories(library.id).unwrap() {
            eprintln!("category: {category:#?}");
            for series in state.db.list_series_page(library.id, &category.key, "name", 100, 0).unwrap().items {
                eprintln!("  series: {series:#?}");
            }
        }

        drop(state);
        let _ = fs::remove_dir_all(&work);
    }
}
