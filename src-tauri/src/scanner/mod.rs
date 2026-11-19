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
    time::{Duration, Instant},
};

use anyhow::{bail, Context, Result};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};
use walkdir::WalkDir;

use crate::{
    catalog::{
        is_movie_category, schedule_label, MOVIE_CATEGORY, UNKNOWN_CATEGORY,
    },
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

/// 自动同步只收录已经静默这么久的文件。
///
/// qBittorrent 新建任务、外部下载器写文件、从 U 盘复制视频时，目标文件会先
/// 出现再持续写入。此时按扩展名就能匹配到视频，但元数据/缩略图一定会失败。
/// 自动同步跳过还在变化的文件，等 watcher 下一次补扫再收录；手动扫描仍然
/// 按磁盘当前状态执行。
pub(crate) const AUTO_SETTLE: Duration = Duration::from_secs(10);

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
    /// 自动同步时因文件仍在写入而跳过的数量。
    pub deferred: usize,
    /// 本次扫描因 qBittorrent 仍在下载而排除的路径数量。
    pub downloading: usize,
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

pub fn scan(
    app: &AppHandle,
    library_id: i64,
    downloading: &[PathBuf],
) -> Result<ScanSummary> {
    let state = app.state::<AppState>();

    scan_with_state(
        state.inner(),
        library_id,
        None,
        downloading,
        &mut |progress| {
            // 事件发送失败不应中断扫描（窗口可能已关闭）。
            let _ = app.emit(PROGRESS_EVENT, progress);
        },
    )
}

/// 文件监听触发的静默扫描：不发送逐条进度事件，避免界面闪烁。
///
/// 自动同步跳过仍在写入的文件：内置 qBittorrent 刚添加任务时文件已经存在，
/// 外部下载器/复制操作也会持续改同一个文件；先把它们留在索引外，等静默后
/// 由 watcher 补扫，避免半成品触发一串媒体处理失败。
///
/// `downloading` 是 qBittorrent 报告仍在下载的文件/目录路径。它们不仅不
/// 进入索引，已有的索引记录也会被标记为 missing，这样选集页面不会显示
/// 未下载完的文件；下载完成后再由下一次扫描恢复。
pub fn scan_quiet(
    app: &AppHandle,
    library_id: i64,
    downloading: &[PathBuf],
) -> Result<ScanSummary> {
    let state = app.state::<AppState>();
    scan_with_state(
        state.inner(),
        library_id,
        Some(AUTO_SETTLE),
        downloading,
        &mut |_| {},
    )
}

fn scan_with_state(
    state: &AppState,
    library_id: i64,
    settle: Option<Duration>,
    downloading: &[PathBuf],
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

    let result = run(state, library_id, settle, downloading, on_progress);
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
    /// 自动同步的文件静默期；`None` 表示手动扫描。
    settle: Option<Duration>,
    started_ms: i64,
    /// 仍在写入、需要等下一次扫描再收录的文件。
    deferred_paths: Vec<String>,
    /// 其中需要 watcher 定时补扫的数量（排除永久 0 字节文件）。
    deferred: usize,
    /// qBittorrent 仍在下载的路径（已统一分隔符、小写）。
    downloading: Vec<String>,
}

/// 路径比较键：统一斜杠、去掉末尾分隔符并转小写。
fn path_key(path: &str) -> String {
    path.replace('/', "\\")
        .trim_end_matches('\\')
        .to_ascii_lowercase()
}

/// 判断路径是否等于排除路径，或位于排除目录之下。
fn path_matches(path: &str, prefix: &str) -> bool {
    if path == prefix {
        return true;
    }
    path.strip_prefix(prefix)
        .is_some_and(|rest| rest.starts_with('\\'))
}

impl ScanContext<'_> {
    /// qBittorrent 报告该路径仍在下载：不收录，并把已有记录隐藏。
    fn is_downloading(&self, path: &Path) -> bool {
        if self.downloading.is_empty() {
            return false;
        }
        let key = path_key(&path.to_string_lossy());
        self.downloading
            .iter()
            .any(|prefix| path_matches(&key, prefix))
    }

    /// 自动同步时跳过仍在写入的文件。
    ///
    /// 返回 `true` 时调用方直接跳过这个文件，不写入索引；如果它已经在数据库
    /// 里，`run` 结束前会把状态恢复为 active，避免下载过程中短暂显示缺失。
    fn defer_if_unsettled(
        &mut self,
        path: &Path,
        metadata: &fs::Metadata,
    ) -> bool {
        let Some(settle) = self.settle else {
            return false;
        };

        let settle_ms = settle.as_millis() as i64;
        let started_ms = self.started_ms;
        // 时间在未来（复制工具保留时间戳、系统时钟偏差）视为已完成，
        // 否则会永远落在静默期内。
        let recent = |time: Option<i64>| {
            time.is_some_and(|time| {
                time <= started_ms && started_ms - time < settle_ms
            })
        };

        let unsettled = if metadata.len() == 0 {
            // 下载器/复制工具刚创建文件，还没写入数据。等后续写入事件即可，
            // 不安排重试，避免永久空文件造成反复扫描。
            true
        } else {
            // `modified` 覆盖下载器边下边写；`created` 覆盖复制工具保留源
            // 修改时间（目标文件刚创建但 mtime 可能是旧的）的情况。
            recent(metadata.modified().ok().and_then(system_time_to_ms))
                || recent(metadata.created().ok().and_then(system_time_to_ms))
        };

        if !unsettled {
            return false;
        }

        if metadata.len() > 0 {
            self.deferred += 1;
        }
        self.deferred_paths
            .push(path.to_string_lossy().into_owned());
        true
    }

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
    settle: Option<Duration>,
    downloading: &[PathBuf],
    on_progress: &mut dyn FnMut(ScanProgress),
) -> Result<ScanSummary> {
    let started = Instant::now();
    let started_ms = now_ms();
    let downloading: Vec<String> = downloading
        .iter()
        .map(|path| path_key(&path.to_string_lossy()))
        .filter(|path| !path.is_empty())
        .collect();
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
        settle,
        started_ms,
        deferred_paths: Vec::new(),
        deferred: 0,
        downloading,
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

        // 档期目录和「剧场版」都作为分类：子目录是番剧/影片，
        // 直接放在分类目录里的视频单独成一部。
        let category = schedule_label(&name).or_else(|| {
            is_movie_category(&name).then(|| MOVIE_CATEGORY.to_string())
        });

        if let Some(category) = category {
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

    // qBittorrent 仍在下载的文件不进入选集页面。它们可能已经在旧索引里，
    // 这里先标记为 missing；下载完成后的扫描会用 upsert 恢复为 active。
    if !ctx.downloading.is_empty() {
        state.db.hide_paths(library_id, &ctx.downloading)?;
    }

    // 仍在写入的文件没有进入本次扫描，但它们只是暂时跳过，不是被删除。
    // 先把它们的记录从 checking 恢复为 active，避免下载过程中被标记缺失。
    if !ctx.deferred_paths.is_empty() {
        state
            .db
            .keep_active_paths(library_id, &ctx.deferred_paths)?;
    }

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
        deferred: ctx.deferred,
        downloading: ctx.downloading.len(),
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
    // 评分以 kanfanos.toml 为准，扫描时同步到数据库索引。
    ctx.state.db.set_series_rating(series_id, config.rating)?;
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
        if ctx.is_downloading(&path) {
            continue;
        }
        if ctx.defer_if_unsettled(&path, &metadata) {
            continue;
        }

        let is_root_file = path.parent().map(|parent| parent == dir).unwrap_or(false);
        let relative = series_config::relative_path(dir, path);
        let kind = series_config::classify_kind(
            &config.episode_mode,
            &config.episodes,
            &relative,
            is_root_file,
        );

        ctx.push_file(
            ScannedFile {
                series_id,
                path: path.to_string_lossy().into_owned(),
                filename: entry.file_name().to_string_lossy().into_owned(),
                extension,
                file_size: metadata.len().min(i64::MAX as u64) as i64,
                modified_at: metadata.modified().ok().and_then(system_time_to_ms),
                kind: kind.to_string(),
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
        if ctx.is_downloading(&path) {
            continue;
        }
        if ctx.defer_if_unsettled(&path, &metadata) {
            continue;
        }
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
                kind: "main".to_string(),
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
        time::{Duration, SystemTime, UNIX_EPOCH},
    };

    use super::scan_with_state;
    use crate::{
        catalog::{MOVIE_CATEGORY, UNKNOWN_CATEGORY},
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
                player: std::sync::Mutex::new(None),
                video_surface: std::sync::Mutex::new(None),
                data_root: root.join("appdata"),
            },
            library.id,
        )
    }

    #[test]
    fn scans_movie_category_as_top_level_category() {
        let root = test_dir("scan-movies");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join(MOVIE_CATEGORY).join("你的颜色")).unwrap();
        fs::create_dir_all(root.join("2026年7月新番").join("测试番")).unwrap();

        fs::write(
            root.join(MOVIE_CATEGORY).join("你的颜色").join("movie.mp4"),
            b"1",
        )
        .unwrap();
        fs::write(
            root.join("2026年7月新番").join("测试番").join("第1集.mp4"),
            b"2",
        )
        .unwrap();

        let (state, library_id) = open_state(&root);
        let mut noop = |_| {};

        let stats = scan_with_state(&state, library_id, None, &[], &mut noop).unwrap();
        assert_eq!(stats.series_found, 2);

        let categories = state.db.list_categories(library_id).unwrap();
        assert_eq!(categories[0].label, MOVIE_CATEGORY);
        assert_eq!(categories[0].count, 1);
        assert_eq!(categories[1].label, "2026年7月");

        let movies = state
            .db
            .list_series_page(library_id, MOVIE_CATEGORY, "name", 100, 0)
            .unwrap()
            .items;
        assert_eq!(movies.len(), 1);
        assert_eq!(movies[0].name, "你的颜色");
        assert_eq!(movies[0].episode_count, 1);
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

        let first = scan_with_state(&state, library_id, None, &[], &mut noop).unwrap();
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

        let _ = scan_with_state(&state, library_id, None, &[], &mut noop).unwrap();

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

        let second = scan_with_state(&state, library_id, None, &[], &mut noop).unwrap();
        assert_eq!(second.files_found, 3);
        assert_eq!(second.files_added, 1);
        assert_eq!(second.missing, 1);

        let scheduled = state.db.list_series_page(library_id, "2026年7月", "name", 100, 0).unwrap().items;
        assert_eq!(scheduled[0].episode_count, 2);

        drop(state);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn automatic_scan_defers_files_still_being_written() {
        let root = test_dir("settle");
        let _ = fs::remove_dir_all(&root);
        let series_dir = root.join("番剧");
        fs::create_dir_all(&series_dir).unwrap();
        fs::write(series_dir.join("第1集.mp4"), b"partial").unwrap();

        let (state, library_id) = open_state(&root);
        let mut noop = |_| {};

        // 先按手动扫描收录，模拟文件已经在索引里。
        scan_with_state(&state, library_id, None, &[], &mut noop).unwrap();

        // 刚写入的文件还在自动同步的静默期内：跳过，但不能判定为缺失。
        let auto = scan_with_state(
            &state,
            library_id,
            Some(Duration::from_secs(10)),
            &[],
            &mut noop,
        )
        .unwrap();
        assert_eq!(auto.files_found, 0);
        assert_eq!(auto.deferred, 1);
        assert_eq!(auto.missing, 0);

        let page = state
            .db
            .list_series_page(library_id, UNKNOWN_CATEGORY, "name", 100, 0)
            .unwrap();
        assert_eq!(page.items.len(), 1);
        assert_eq!(page.items[0].episode_count, 1);

        drop(state);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn automatic_scan_hides_files_still_downloading() {
        let root = test_dir("downloading");
        let _ = fs::remove_dir_all(&root);
        let series_dir = root.join("番剧");
        fs::create_dir_all(&series_dir).unwrap();
        fs::write(series_dir.join("第1集.mp4"), b"done").unwrap();
        fs::write(series_dir.join("第2集.mp4"), b"partial").unwrap();

        let (state, library_id) = open_state(&root);
        let mut noop = |_| {};

        scan_with_state(&state, library_id, None, &[], &mut noop).unwrap();
        let page = state
            .db
            .list_series_page(library_id, UNKNOWN_CATEGORY, "name", 100, 0)
            .unwrap();
        let series_id = page.items[0].id;
        assert_eq!(state.db.list_episodes(series_id).unwrap().len(), 2);

        // qBittorrent 报告第 2 集仍在下载：本次扫描不收录它，并且已有记录
        // 要隐藏，选集页面只剩第 1 集。
        let downloading = [series_dir.join("第2集.mp4")];
        let hidden = scan_with_state(
            &state,
            library_id,
            None,
            &downloading,
            &mut noop,
        )
        .unwrap();
        assert_eq!(hidden.files_found, 1);
        assert_eq!(hidden.downloading, 1);
        let episodes = state.db.list_episodes(series_id).unwrap();
        assert_eq!(episodes.len(), 1);
        assert_eq!(episodes[0].filename, "第1集.mp4");

        // 下载完成后再扫描，记录恢复。
        scan_with_state(&state, library_id, None, &[], &mut noop).unwrap();
        assert_eq!(state.db.list_episodes(series_id).unwrap().len(), 2);

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

        let summary = scan_with_state(&state, library_id, None, &[], &mut |_| {}).unwrap();
        assert!(summary.cancelled);
        assert_eq!(summary.missing, 0);

        drop(state);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn separates_root_episodes_from_subfolder_extras() {
        let root = test_dir("extras");
        let _ = fs::remove_dir_all(&root);
        let series_dir = root.join("2026年7月新番").join("测试番");
        fs::create_dir_all(series_dir.join("PV")).unwrap();
        fs::write(series_dir.join("第1集.mp4"), b"main").unwrap();
        fs::write(series_dir.join("PV").join("特典.mp4"), b"extra").unwrap();

        let (state, library_id) = open_state(&root);
        let mut noop = |_| {};
        scan_with_state(&state, library_id, None, &[], &mut noop).unwrap();

        let page = state
            .db
            .list_series_page(library_id, "2026年7月", "name", 100, 0)
            .unwrap();
        assert_eq!(page.items.len(), 1);
        let series = &page.items[0];
        assert_eq!(series.episode_count, 1);

        let main = state.db.list_episodes(series.id).unwrap();
        let extras = state.db.list_extra_episodes(series.id).unwrap();
        assert_eq!(main.len(), 1);
        assert_eq!(main[0].filename, "第1集.mp4");
        assert_eq!(extras.len(), 1);
        assert_eq!(extras[0].filename, "特典.mp4");

        drop(state);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn manual_episode_mode_uses_selected_subfolder_files() {
        let root = test_dir("manual-episodes");
        let _ = fs::remove_dir_all(&root);
        let series_dir = root.join("2026年7月新番").join("测试番");
        fs::create_dir_all(series_dir.join("正片")).unwrap();
        fs::write(series_dir.join("下载的种子.mp4"), b"download").unwrap();
        fs::write(series_dir.join("正片").join("第1集.mp4"), b"episode").unwrap();
        fs::write(
            series_dir.join("kanfanos.toml"),
            "episode_mode = \"manual\"\nepisodes = [\"正片/第1集.mp4\"]\n",
        )
        .unwrap();

        let (state, library_id) = open_state(&root);
        let mut noop = |_| {};
        scan_with_state(&state, library_id, None, &[], &mut noop).unwrap();

        let page = state
            .db
            .list_series_page(library_id, "2026年7月", "name", 100, 0)
            .unwrap();
        let series = &page.items[0];
        assert_eq!(series.episode_count, 1);

        let main = state.db.list_episodes(series.id).unwrap();
        let extras = state.db.list_extra_episodes(series.id).unwrap();
        assert_eq!(main.len(), 1);
        assert_eq!(main[0].filename, "第1集.mp4");
        assert_eq!(extras.len(), 1);
        assert_eq!(extras[0].filename, "下载的种子.mp4");

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
        let summary = scan_with_state(&state, library_id, None, &[], &mut |_| {}).unwrap();
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
            player: std::sync::Mutex::new(None),
            video_surface: std::sync::Mutex::new(None),
            data_root: work.join("appdata"),
        };

        let summary = scan_with_state(&state, library.id, None, &[], &mut |_| {}).unwrap();
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
