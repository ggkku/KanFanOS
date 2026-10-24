use std::{collections::HashMap, path::Path};

use anyhow::Result;
use rusqlite::params;

use super::Database;
use crate::series_config;

/// 扫描阶段能获得的最小文件信息，元数据由后续步骤补齐。
#[derive(Debug, Clone)]
pub struct ScannedFile {
    pub series_id: i64,
    pub path: String,
    pub filename: String,
    pub extension: String,
    pub file_size: i64,
    pub modified_at: Option<i64>,
    /// `main` = 选集；`extra` = 其它视频。
    pub kind: String,
}

/// 选集选择器使用的番剧视频行。
#[derive(Debug, Clone)]
pub struct SeriesVideoRow {
    pub path: String,
    pub filename: String,
    pub kind: String,
    pub series_path: String,
}

/// 同一路径只保留一条记录；文件大小或修改时间变化时清空派生元数据，
/// 让后续的元数据/缩略图任务重新处理。
const UPSERT_VIDEO: &str = r#"
INSERT INTO videos (
    path, filename, extension, file_size, modified_at,
    status, library_id, series_id, created_at, updated_at, kind
) VALUES (?1, ?2, ?3, ?4, ?5, 'active', ?6, ?7, ?8, ?8, ?9)
ON CONFLICT(path) DO UPDATE SET
    filename   = excluded.filename,
    extension  = excluded.extension,
    status     = 'active',
    library_id = COALESCE(videos.library_id, excluded.library_id),
    series_id  = excluded.series_id,
    kind       = excluded.kind,
    file_size  = excluded.file_size,
    modified_at = excluded.modified_at,
    updated_at = excluded.updated_at,
    duration_ms = CASE WHEN videos.file_size <> excluded.file_size
                        OR COALESCE(videos.modified_at, -1) <> COALESCE(excluded.modified_at, -1)
                       THEN NULL ELSE videos.duration_ms END,
    width = CASE WHEN videos.file_size <> excluded.file_size
                  OR COALESCE(videos.modified_at, -1) <> COALESCE(excluded.modified_at, -1)
                 THEN NULL ELSE videos.width END,
    height = CASE WHEN videos.file_size <> excluded.file_size
                   OR COALESCE(videos.modified_at, -1) <> COALESCE(excluded.modified_at, -1)
                  THEN NULL ELSE videos.height END,
    video_codec = CASE WHEN videos.file_size <> excluded.file_size
                        OR COALESCE(videos.modified_at, -1) <> COALESCE(excluded.modified_at, -1)
                       THEN NULL ELSE videos.video_codec END,
    audio_codec = CASE WHEN videos.file_size <> excluded.file_size
                        OR COALESCE(videos.modified_at, -1) <> COALESCE(excluded.modified_at, -1)
                       THEN NULL ELSE videos.audio_codec END,
    frame_rate = CASE WHEN videos.file_size <> excluded.file_size
                       OR COALESCE(videos.modified_at, -1) <> COALESCE(excluded.modified_at, -1)
                      THEN NULL ELSE videos.frame_rate END,
    thumbnail_key = CASE WHEN videos.file_size <> excluded.file_size
                          OR COALESCE(videos.modified_at, -1) <> COALESCE(excluded.modified_at, -1)
                         THEN NULL ELSE videos.thumbnail_key END,
    media_status = CASE WHEN videos.file_size <> excluded.file_size
                         OR COALESCE(videos.modified_at, -1) <> COALESCE(excluded.modified_at, -1)
                        THEN 'pending' ELSE videos.media_status END,
    media_attempts = CASE WHEN videos.file_size <> excluded.file_size
                           OR COALESCE(videos.modified_at, -1) <> COALESCE(excluded.modified_at, -1)
                          THEN 0 ELSE videos.media_attempts END,
    media_error = CASE WHEN videos.file_size <> excluded.file_size
                        OR COALESCE(videos.modified_at, -1) <> COALESCE(excluded.modified_at, -1)
                       THEN NULL ELSE videos.media_error END
"#;

/// 后台媒体任务领取的一条记录。
#[derive(Debug, Clone)]
pub struct MediaJob {
    pub id: i64,
    pub path: String,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Default)]
pub struct ProbeData {
    pub duration_ms: Option<i64>,
    pub width: Option<i64>,
    pub height: Option<i64>,
    pub video_codec: Option<String>,
    pub audio_codec: Option<String>,
    pub frame_rate: Option<f64>,
}

#[derive(Debug, Clone, Default, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaStats {
    pub pending: i64,
    pub processing: i64,
    pub ready: i64,
    pub failed: i64,
}

impl Database {
    /// 扫描开始前把该目录的记录置为 checking。
    pub fn begin_library_scan(&self, library_id: i64) -> Result<()> {
        let conn = self.conn()?;
        // 上一次扫描中断而残留的 checking 记录先恢复，避免误判为缺失。
        conn.execute(
            "UPDATE videos SET status = 'active'
              WHERE library_id = ?1 AND status = 'checking'",
            [library_id],
        )?;
        conn.execute(
            "UPDATE videos SET status = 'checking'
              WHERE library_id = ?1 AND status <> 'missing'",
            [library_id],
        )?;
        Ok(())
    }

    /// 在一个事务里批量写入。调用方控制每批 500～2000 条。
    pub fn upsert_videos(&self, library_id: i64, files: &[ScannedFile], now: i64) -> Result<()> {
        if files.is_empty() {
            return Ok(());
        }

        let mut conn = self.conn()?;
        let tx = conn.transaction()?;
        {
            let mut stmt = tx.prepare_cached(UPSERT_VIDEO)?;
            for file in files {
                stmt.execute(params![
                    file.path,
                    file.filename,
                    file.extension,
                    file.file_size,
                    file.modified_at,
                    library_id,
                    file.series_id,
                    now,
                    file.kind,
                ])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    /// 扫描正常结束：仍处于 checking 的记录即文件已不存在。
    /// 返回被标记为 missing 的数量。
    pub fn finish_library_scan(&self, library_id: i64, now: i64) -> Result<usize> {
        let conn = self.conn()?;
        let missing = conn.execute(
            "UPDATE videos SET status = 'missing', updated_at = ?2
              WHERE library_id = ?1 AND status = 'checking'",
            params![library_id, now],
        )?;
        conn.execute(
            "UPDATE libraries SET last_scan_at = ?2 WHERE id = ?1",
            params![library_id, now],
        )?;
        Ok(missing)
    }

    /// 扫描被取消或失败：不做缺失判定，把检查中的记录恢复为 active。
    pub fn abort_library_scan(&self, library_id: i64) -> Result<()> {
        self.conn()?.execute(
            "UPDATE videos SET status = 'active'
              WHERE library_id = ?1 AND status = 'checking'",
            [library_id],
        )?;
        Ok(())
    }

    pub fn video_row_count(&self, library_id: i64) -> Result<i64> {
        Ok(self.conn()?.query_row(
            "SELECT COUNT(*) FROM videos WHERE library_id = ?1",
            [library_id],
            |row| row.get(0),
        )?)
    }

    /// 领取一批待处理的媒体任务，并标记为 processing。
    pub fn claim_media_jobs(&self, library_id: i64, limit: usize) -> Result<Vec<MediaJob>> {
        let mut conn = self.conn()?;
        let tx = conn.transaction()?;

        let jobs = {
            let mut stmt = tx.prepare(
                "SELECT id, path, updated_at
                   FROM videos
                  WHERE library_id = ?1
                    AND status <> 'missing'
                    AND media_status = 'pending'
                  ORDER BY id
                  LIMIT ?2",
            )?;

            let rows = stmt.query_map(params![library_id, limit as i64], |row| {
                Ok(MediaJob {
                    id: row.get(0)?,
                    path: row.get(1)?,
                    updated_at: row.get(2)?,
                })
            })?;

            let mut jobs = Vec::new();
            for row in rows {
                jobs.push(row?);
            }
            jobs
        };

        for job in &jobs {
            tx.execute(
                "UPDATE videos SET media_status = 'processing' WHERE id = ?1",
                [job.id],
            )?;
        }

        tx.commit()?;
        Ok(jobs)
    }

    /// 应用启动或任务取消后，把中断的任务放回队列。
    pub fn reset_media_processing(&self) -> Result<()> {
        self.conn()?.execute(
            "UPDATE videos SET media_status = 'pending' WHERE media_status = 'processing'",
            [],
        )?;
        Ok(())
    }

    /// 把因外部工具不可用而耗尽重试的任务重新放回队列。
    ///
    /// 典型场景：旧安装包没有内置 FFprobe/FFmpeg，扫描时所有任务都会失败并
    /// 停在 `failed`；补上工具后必须重新排队，否则这些视频永远没有时长和
    /// 缩略图（`claim_media_jobs` 只领取 `pending`）。
    ///
    /// 不重排 `ffprobe 失败: ...` / `ffmpeg 失败: ...`：这两个前缀说明工具
    /// 已经运行，只是文件本身无法解析，每次启动重试没有意义。旧版本在
    /// FFprobe 缺失时只记录原始 IO 错误（如 `(os error 2)`），所以其余
    /// 错误都视为「工具没跑起来」。
    pub fn requeue_failed_media(&self, library_id: i64) -> Result<usize> {
        let updated = self.conn()?.execute(
            "UPDATE videos
                SET media_status = 'pending',
                    media_attempts = 0,
                    media_error = NULL
              WHERE library_id = ?1
                AND status <> 'missing'
                AND media_status = 'failed'
                AND (
                    media_error IS NULL
                    OR (
                        media_error NOT LIKE 'ffprobe 失败:%'
                        AND media_error NOT LIKE 'ffmpeg 失败:%'
                    )
                )",
            [library_id],
        )?;
        Ok(updated)
    }

    pub fn save_probe(&self, id: i64, probe: &ProbeData, now: i64) -> Result<()> {
        self.conn()?.execute(
            "UPDATE videos
                SET duration_ms = ?2,
                    width = ?3,
                    height = ?4,
                    video_codec = ?5,
                    audio_codec = ?6,
                    frame_rate = ?7,
                    last_scanned_at = ?8
              WHERE id = ?1",
            params![
                id,
                probe.duration_ms,
                probe.width,
                probe.height,
                probe.video_codec,
                probe.audio_codec,
                probe.frame_rate,
                now
            ],
        )?;
        Ok(())
    }

    pub fn save_thumbnail(&self, id: i64, key: &str, now: i64) -> Result<()> {
        self.conn()?.execute(
            "UPDATE videos
                SET thumbnail_key = ?2,
                    media_status = 'ready',
                    media_error = NULL,
                    last_scanned_at = ?3
              WHERE id = ?1",
            params![id, key, now],
        )?;
        Ok(())
    }

    /// 失败后累加次数；达到 3 次标记为 failed，否则回到队列重试。
    pub fn mark_media_failed(&self, id: i64, error: &str) -> Result<()> {
        self.conn()?.execute(
            "UPDATE videos
                SET media_attempts = media_attempts + 1,
                    media_status = CASE WHEN media_attempts + 1 >= 3
                                        THEN 'failed' ELSE 'pending' END,
                    media_error = ?2
              WHERE id = ?1",
            params![id, error],
        )?;
        Ok(())
    }

    /// 清空缩略图引用，让所有未失效的视频重新排队。
    ///
    /// 不能只重置 `thumbnail_key IS NOT NULL` 的记录：探测失败的任务从未写入
    /// 缩略图键，却能停在三连失败里，只重置有键的行会让它们永远卡住。
    pub fn reset_thumbnails(&self) -> Result<usize> {
        let updated = self.conn()?.execute(
            "UPDATE videos
                SET thumbnail_key = NULL,
                    media_status = 'pending',
                    media_attempts = 0,
                    media_error = NULL
              WHERE status <> 'missing'",
            [],
        )?;
        Ok(updated)
    }

    /// 按每个番剧的 `kanfanos.toml` 修正 `kind`。
    ///
    /// auto 模式保持旧规则（根目录 → main，子目录 → extra）；
    /// manual 模式只把 `episodes` 里的文件作为 main，其余归入 extra。
    /// 旧数据库升级后没有 kind 语义，启动时也会跑一次。
    pub fn reclassify_video_kinds(&self) -> Result<usize> {
        let conn = self.conn()?;
        let rows = {
            let mut stmt = conn.prepare(
                "SELECT v.id, v.path, s.path
                   FROM videos v
                   JOIN series s ON s.id = v.series_id
                  WHERE v.status <> 'missing'",
            )?;
            let rows = stmt.query_map([], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                ))
            })?;
            rows.collect::<rusqlite::Result<Vec<_>>>()?
        };

        let mut configs: HashMap<String, series_config::SeriesConfig> = HashMap::new();
        let mut changed = 0;

        for (id, path, series_path) in rows {
            let config = configs
                .entry(series_path.clone())
                .or_insert_with(|| series_config::load(Path::new(&series_path)));

            let file = Path::new(&path);
            let relative = series_config::relative_path(Path::new(&series_path), file);
            let is_root_file = file
                .parent()
                .map(|parent| {
                    normalize_path(&parent.to_string_lossy())
                        == normalize_path(&series_path)
                })
                .unwrap_or(false);
            let kind = series_config::classify_kind(
                &config.episode_mode,
                &config.episodes,
                &relative,
                is_root_file,
            );

            changed += conn.execute(
                "UPDATE videos SET kind = ?2 WHERE id = ?1 AND kind <> ?2",
                params![id, kind],
            )?;
        }

        Ok(changed)
    }

    /// 某个番剧下所有有效视频，供选集选择器展示。
    pub fn series_videos(&self, series_id: i64) -> Result<Vec<SeriesVideoRow>> {
        let conn = self.conn()?;
        let mut stmt = conn.prepare(
            "SELECT v.path, v.filename, v.kind, s.path
               FROM videos v
               JOIN series s ON s.id = v.series_id
              WHERE v.series_id = ?1 AND v.status <> 'missing'
              ORDER BY v.path",
        )?;
        let rows = stmt.query_map([series_id], |row| {
            Ok(SeriesVideoRow {
                path: row.get(0)?,
                filename: row.get(1)?,
                kind: row.get(2)?,
                series_path: row.get(3)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// 保存选集配置后，立即按新配置修正单个番剧的视频分类。
    pub fn reclassify_series_videos(&self, series_id: i64) -> Result<usize> {
        let conn = self.conn()?;
        let series_path: String = conn.query_row(
            "SELECT path FROM series WHERE id = ?1",
            [series_id],
            |row| row.get(0),
        )?;

        let config = series_config::load(Path::new(&series_path));
        let rows = {
            let mut stmt = conn.prepare(
                "SELECT id, path FROM videos
                  WHERE series_id = ?1 AND status <> 'missing'",
            )?;
            let rows = stmt.query_map([series_id], |row| {
                Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
            })?;
            rows.collect::<rusqlite::Result<Vec<_>>>()?
        };

        let mut changed = 0;
        for (id, path) in rows {
            let file = Path::new(&path);
            let relative = series_config::relative_path(Path::new(&series_path), file);
            let is_root_file = file
                .parent()
                .map(|parent| {
                    normalize_path(&parent.to_string_lossy())
                        == normalize_path(&series_path)
                })
                .unwrap_or(false);
            let kind = series_config::classify_kind(
                &config.episode_mode,
                &config.episodes,
                &relative,
                is_root_file,
            );

            changed += conn.execute(
                "UPDATE videos SET kind = ?2 WHERE id = ?1 AND kind <> ?2",
                params![id, kind],
            )?;
        }

        Ok(changed)
    }

    pub fn media_stats(&self, library_id: i64) -> Result<MediaStats> {
        Ok(self.conn()?.query_row(
            "SELECT
                COALESCE(SUM(CASE WHEN media_status = 'pending' THEN 1 ELSE 0 END), 0),
                COALESCE(SUM(CASE WHEN media_status = 'processing' THEN 1 ELSE 0 END), 0),
                COALESCE(SUM(CASE WHEN media_status = 'ready' THEN 1 ELSE 0 END), 0),
                COALESCE(SUM(CASE WHEN media_status = 'failed' THEN 1 ELSE 0 END), 0)
               FROM videos
              WHERE library_id = ?1 AND status <> 'missing'",
            [library_id],
            |row| {
                Ok(MediaStats {
                    pending: row.get(0)?,
                    processing: row.get(1)?,
                    ready: row.get(2)?,
                    failed: row.get(3)?,
                })
            },
        )?)
    }
}

fn normalize_path(path: &str) -> String {
    path.replace('/', "\\").trim_end_matches('\\').to_lowercase()
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        time::{SystemTime, UNIX_EPOCH},
    };

    use crate::{database::Database, time::now_ms};

    use super::ScannedFile;

    #[test]
    fn removing_library_cascades_video_rows() {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("kanfanos-db-{stamp}"));
        fs::create_dir_all(&dir).unwrap();

        let db = Database::open(&dir.join("library.sqlite")).unwrap();
        let library = db.add_library("X:\\fake", "fake", now_ms()).unwrap();
        let series_id = db
            .upsert_series(
                library.id,
                "未知时间",
                "fake",
                "X:\\fake\\series",
                "",
                None,
                None,
                now_ms(),
            )
            .unwrap();

        db.upsert_videos(
            library.id,
            &[ScannedFile {
                series_id,
                path: "X:\\fake\\a.mp4".to_string(),
                filename: "a.mp4".to_string(),
                extension: "mp4".to_string(),
                file_size: 1,
                modified_at: Some(1),
                kind: "main".to_string(),
            }],
            now_ms(),
        )
        .unwrap();
        assert_eq!(db.video_row_count(library.id).unwrap(), 1);

        assert!(db.remove_library(library.id).unwrap());
        assert_eq!(db.video_count().unwrap(), 0);

        drop(db);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn failed_media_jobs_are_requeued_and_cache_reset_retries_them() {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("kanfanos-db-media-{stamp}"));
        fs::create_dir_all(&dir).unwrap();

        let db = Database::open(&dir.join("library.sqlite")).unwrap();
        let library = db.add_library("X:\\fake", "fake", now_ms()).unwrap();
        let series_id = db
            .upsert_series(
                library.id,
                "未知时间",
                "fake",
                "X:\\fake\\series",
                "",
                None,
                None,
                now_ms(),
            )
            .unwrap();
        db.upsert_videos(
            library.id,
            &[ScannedFile {
                series_id,
                path: "X:\\fake\\a.mp4".to_string(),
                filename: "a.mp4".to_string(),
                extension: "mp4".to_string(),
                file_size: 1,
                modified_at: Some(1),
                kind: "main".to_string(),
            }],
            now_ms(),
        )
        .unwrap();

        let job = db.claim_media_jobs(library.id, 10).unwrap().remove(0);
        for _ in 0..3 {
            db.mark_media_failed(job.id, "系统找不到指定的文件。 (os error 2)")
                .unwrap();
        }
        assert_eq!(db.media_stats(library.id).unwrap().failed, 1);

        // 补上 FFprobe/FFmpeg 后重新入队。
        assert_eq!(db.requeue_failed_media(library.id).unwrap(), 1);
        let stats = db.media_stats(library.id).unwrap();
        assert_eq!(stats.pending, 1);
        assert_eq!(stats.failed, 0);
        assert_eq!(db.claim_media_jobs(library.id, 10).unwrap().len(), 1);

        // 文件本身损坏导致的重试耗尽不自动重排，否则每次启动都会重试。
        for _ in 0..3 {
            db.mark_media_failed(job.id, "ffprobe 失败: Invalid data found")
                .unwrap();
        }
        assert_eq!(db.media_stats(library.id).unwrap().failed, 1);
        assert_eq!(db.requeue_failed_media(library.id).unwrap(), 0);

        // 手动清理缩略图缓存则必须把它放回队列，
        // 即使这条记录从来没有 thumbnail_key。
        assert_eq!(db.reset_thumbnails().unwrap(), 1);
        let stats = db.media_stats(library.id).unwrap();
        assert_eq!(stats.pending, 1);
        assert_eq!(stats.failed, 0);

        drop(db);
        let _ = fs::remove_dir_all(&dir);
    }
}
