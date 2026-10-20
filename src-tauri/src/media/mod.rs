//! 元数据与缩略图后台处理。
//!
//! 队列由数据库的 media_status 驱动，限制并发进程数量，
//! 失败的任务最多重试 3 次。

mod probe;
mod thumbnail;

use std::sync::{atomic::Ordering, Arc};

use anyhow::{bail, Result};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};
use tokio::{sync::Semaphore, task::JoinSet};

use crate::{
    database::videos::MediaJob,
    state::AppState,
    time::now_ms,
};

/// 同时运行的 FFprobe/FFmpeg 进程上限。
const CONCURRENCY: usize = 3;

/// 每轮领取的任务数。
const BATCH_SIZE: usize = 12;

pub const PROGRESS_EVENT: &str = "media-progress";

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaProgress {
    pub library_id: i64,
    /// `processing` | `completed` | `cancelled`
    pub phase: &'static str,
    pub processed: u64,
    pub failed: u64,
    pub total: u64,
}

pub(crate) fn command(program: &str) -> tokio::process::Command {
    #[allow(unused_mut)]
    let mut command = tokio::process::Command::new(program);

    #[cfg(windows)]
    {
        // 避免打包后的应用在后台弹出控制台窗口。
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }

    command
}

struct MediaGuard {
    app: AppHandle,
}

impl Drop for MediaGuard {
    fn drop(&mut self) {
        let state = self.app.state::<AppState>();
        state.media.reset_cancel();
        state.media.running.store(false, Ordering::Release);
    }
}

/// 启动后台处理并立即返回；进度通过 `media-progress` 事件推送。
pub fn start(app: AppHandle, library_id: i64) -> Result<()> {
    {
        let state = app.state::<AppState>();
        if state
            .media
            .running
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            bail!("媒体处理已在进行中");
        }
        state.db.reset_media_processing()?;
    }

    tauri::async_runtime::spawn(async move {
        let _guard = MediaGuard { app: app.clone() };

        if let Err(error) = run(&app, library_id).await {
            eprintln!("媒体处理失败: {error:#}");
            let state = app.state::<AppState>();
            let _ = state.db.reset_media_processing();
        }
    });

    Ok(())
}

async fn run(app: &AppHandle, library_id: i64) -> Result<()> {
    let state = app.state::<AppState>();
    let initial = state.db.media_stats(library_id)?;
    let total = (initial.pending + initial.processing).max(0) as u64;

    let mut processed = 0u64;
    let mut failed = 0u64;
    emit(app, library_id, "processing", processed, failed, total);

    loop {
        if state.media.is_cancelled() {
            break;
        }

        let jobs = state.db.claim_media_jobs(library_id, BATCH_SIZE)?;
        if jobs.is_empty() {
            break;
        }

        let semaphore = Arc::new(Semaphore::new(CONCURRENCY));
        let mut tasks = JoinSet::new();

        for job in jobs {
            let permit = semaphore.clone().acquire_owned().await?;
            let app = app.clone();

            tasks.spawn(async move {
                let state = app.state::<AppState>();
                let result = process_job(state.inner(), &job).await;
                drop(permit);
                (job.id, result)
            });
        }

        while let Some(joined) = tasks.join_next().await {
            match joined {
                Ok((_, Ok(()))) => processed += 1,
                Ok((id, Err(error))) => {
                    failed += 1;
                    let state = app.state::<AppState>();
                    if let Err(record_error) =
                        state.db.mark_media_failed(id, &format!("{error:#}"))
                    {
                        eprintln!("记录媒体任务失败状态出错: {record_error:#}");
                    }
                }
                Err(error) => eprintln!("媒体任务异常: {error}"),
            }

            emit(app, library_id, "processing", processed, failed, total);
        }
    }

    let cancelled = state.media.is_cancelled();
    emit(
        app,
        library_id,
        if cancelled { "cancelled" } else { "completed" },
        processed,
        failed,
        total,
    );
    Ok(())
}

async fn process_job(state: &AppState, job: &MediaJob) -> Result<()> {
    let probe = probe::probe(&job.path).await?;
    state.db.save_probe(job.id, &probe, now_ms())?;

    let key = thumbnail::key(job.id, job.updated_at);
    let output = state.data_root.join("thumbnails").join(&key);
    let width = crate::settings::load(&state.data_root)
        .map(|settings| settings.thumbnail_width)
        .unwrap_or(480);
    thumbnail::generate(
        &job.path,
        probe.duration_ms.unwrap_or(0),
        &output,
        width,
    )
    .await?;
    state.db.save_thumbnail(job.id, &key, now_ms())?;

    Ok(())
}

fn emit(
    app: &AppHandle,
    library_id: i64,
    phase: &'static str,
    processed: u64,
    failed: u64,
    total: u64,
) {
    let _ = app.emit(
        PROGRESS_EVENT,
        MediaProgress {
            library_id,
            phase,
            processed,
            failed,
            total,
        },
    );
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        time::{SystemTime, UNIX_EPOCH},
    };

    use crate::{
        database::{videos::ScannedFile, Database},
        state::{AppState, TaskState},
        time::now_ms,
    };

    use super::process_job;

    /// 手动运行：
    /// `cargo test processes_real_video -- --ignored --nocapture`
    #[tokio::test]
    #[ignore = "需要 ffmpeg / ffprobe 在 PATH 中"]
    async fn processes_real_video() {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let work = std::env::temp_dir().join(format!("kanfanos-media-{stamp}"));
        fs::create_dir_all(&work).unwrap();

        let video = work.join("sample.mp4");
        let status = std::process::Command::new("ffmpeg")
            .args([
                "-v",
                "error",
                "-f",
                "lavfi",
                "-i",
                "testsrc=duration=4:size=320x240:rate=25",
                "-pix_fmt",
                "yuv420p",
                "-y",
                video.to_str().unwrap(),
            ])
            .status()
            .expect("需要 ffmpeg 在 PATH 中");
        assert!(status.success());

        let metadata = fs::metadata(&video).unwrap();
        let db = Database::open(&work.join("library.sqlite")).unwrap();
        let library = db
            .add_library(work.to_str().unwrap(), "test", now_ms())
            .unwrap();
        let series = db
            .upsert_series(
                library.id,
                "未知时间",
                "sample",
                work.join("series").to_str().unwrap(),
                "",
                None,
                None,
                now_ms(),
            )
            .unwrap();

        db.upsert_videos(
            library.id,
            &[ScannedFile {
                series_id: series,
                path: video.to_string_lossy().into_owned(),
                filename: "sample.mp4".to_string(),
                extension: "mp4".to_string(),
                file_size: metadata.len() as i64,
                modified_at: None,
            }],
            now_ms(),
        )
        .unwrap();

        let state = AppState {
            db,
            scan: TaskState::new(),
            media: TaskState::new(),
            watcher: std::sync::Mutex::new(None),
            data_root: work.join("appdata"),
        };

        let jobs = state.db.claim_media_jobs(library.id, 10).unwrap();
        assert_eq!(jobs.len(), 1);
        process_job(&state, &jobs[0]).await.unwrap();

        let episodes = state.db.list_episodes(series).unwrap();
        assert_eq!(episodes.len(), 1);
        assert!(episodes[0].duration_ms.unwrap_or(0) >= 3900);
        assert_eq!(episodes[0].width, Some(320));
        assert_eq!(episodes[0].height, Some(240));

        let key = episodes[0]
            .thumbnail_key
            .as_deref()
            .expect("应生成缩略图");
        let thumb = state.data_root.join("thumbnails").join(key);
        assert!(thumb.exists(), "缩略图文件不存在: {}", thumb.display());

        let stats = state.db.media_stats(library.id).unwrap();
        assert_eq!(stats.ready, 1);

        drop(state);
        let _ = fs::remove_dir_all(&work);
    }
}
