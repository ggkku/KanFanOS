use anyhow::{Context, Result};
use rusqlite::{params, OptionalExtension};
use serde::Serialize;

use super::Database;

#[derive(Debug, Clone)]
pub struct PlayableEpisode {
    pub path: String,
    pub duration_ms: Option<i64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecentItem {
    pub video_id: i64,
    pub series_id: Option<i64>,
    pub series_name: Option<String>,
    pub category: Option<String>,
    pub filename: String,
    pub path: String,
    pub duration_ms: Option<i64>,
    pub thumbnail_key: Option<String>,
    pub last_opened_at: i64,
    pub last_position_ms: i64,
    pub completed: bool,
}

impl Database {
    pub fn playable_episode(&self, video_id: i64) -> Result<PlayableEpisode> {
        let conn = self.conn()?;
        conn.query_row(
            "SELECT path, duration_ms FROM videos
              WHERE id = ?1 AND status <> 'missing'",
            [video_id],
            |row| {
                Ok(PlayableEpisode {
                    path: row.get(0)?,
                    duration_ms: row.get(1)?,
                })
            },
        )
        .optional()?
        .with_context(|| format!("剧集不存在: id={video_id}"))
    }

    /// 返回 (上次播放位置毫秒, 是否已看完)。
    pub fn resume_position(&self, video_id: i64) -> Result<(i64, bool)> {
        let conn = self.conn()?;
        let row = conn
            .query_row(
                "SELECT last_position_ms, completed FROM watch_history WHERE video_id = ?1",
                [video_id],
                |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)? != 0)),
            )
            .optional()?;

        Ok(row.unwrap_or((0, false)))
    }

    /// 打开时只更新时间，保留已有进度和完成状态。
    pub fn record_episode_opened(&self, video_id: i64, now: i64) -> Result<()> {
        self.conn()?.execute(
            "INSERT INTO watch_history (video_id, last_opened_at, last_position_ms, completed)
             VALUES (?1, ?2, 0, 0)
             ON CONFLICT(video_id) DO UPDATE SET
                 last_opened_at = excluded.last_opened_at",
            params![video_id, now],
        )?;
        Ok(())
    }

    pub fn save_watch_position(
        &self,
        video_id: i64,
        position_ms: i64,
        completed: bool,
        now: i64,
    ) -> Result<()> {
        self.conn()?.execute(
            "INSERT INTO watch_history (video_id, last_opened_at, last_position_ms, completed)
             VALUES (?1, ?4, ?2, ?3)
             ON CONFLICT(video_id) DO UPDATE SET
                 last_position_ms = excluded.last_position_ms,
                 completed = excluded.completed",
            params![video_id, position_ms.max(0), if completed { 1 } else { 0 }, now],
        )?;
        Ok(())
    }

    /// 每部番剧只返回最近打开的一条记录，而不是每个视频一条。
    pub fn recent_items(&self, library_id: i64, limit: i64) -> Result<Vec<RecentItem>> {
        let conn = self.conn()?;
        let mut stmt = conn.prepare(
            "WITH latest AS (
                 SELECT v.series_id,
                        w.video_id,
                        w.last_opened_at,
                        w.last_position_ms,
                        w.completed,
                        ROW_NUMBER() OVER (
                            PARTITION BY v.series_id
                            ORDER BY w.last_opened_at DESC, w.video_id DESC
                        ) AS rn
                   FROM watch_history w
                   JOIN videos v ON v.id = w.video_id
                  WHERE v.library_id = ?1
                    AND v.status <> 'missing'
                    AND v.series_id IS NOT NULL
             )
             SELECT s.id,
                    s.name,
                    s.category,
                    v.id,
                    v.filename,
                    v.path,
                    v.duration_ms,
                    v.thumbnail_key,
                    l.last_opened_at,
                    l.last_position_ms,
                    l.completed
               FROM latest l
               JOIN videos v ON v.id = l.video_id
               JOIN series s ON s.id = l.series_id
              WHERE l.rn = 1
              ORDER BY l.last_opened_at DESC
              LIMIT ?2",
        )?;

        let rows = stmt.query_map(params![library_id, limit], |row| {
            Ok(RecentItem {
                series_id: row.get(0)?,
                series_name: row.get(1)?,
                category: row.get(2)?,
                video_id: row.get(3)?,
                filename: row.get(4)?,
                path: row.get(5)?,
                duration_ms: row.get(6)?,
                thumbnail_key: row.get(7)?,
                last_opened_at: row.get(8)?,
                last_position_ms: row.get(9)?,
                completed: row.get::<_, i64>(10)? != 0,
            })
        })?;

        let mut items = Vec::new();
        for row in rows {
            items.push(row?);
        }
        Ok(items)
    }

    /// 一键把整部番剧标记为已看完（包含二级选集）。
    pub fn mark_series_completed(&self, series_id: i64, now: i64) -> Result<usize> {
        let updated = self.conn()?.execute(
            "INSERT INTO watch_history (video_id, last_opened_at, last_position_ms, completed)
             SELECT v.id, ?2, COALESCE(v.duration_ms, 0), 1
               FROM videos v
              WHERE v.series_id = ?1 AND v.status <> 'missing'
             ON CONFLICT(video_id) DO UPDATE SET
                 completed = 1,
                 last_position_ms = MAX(
                     watch_history.last_position_ms,
                     excluded.last_position_ms
                 ),
                 last_opened_at = excluded.last_opened_at",
            params![series_id, now],
        )?;
        Ok(updated)
    }

    /// 一键清除整部番剧的观看记录（包含二级选集）。
    pub fn clear_series_history(&self, series_id: i64) -> Result<usize> {
        let removed = self.conn()?.execute(
            "DELETE FROM watch_history
              WHERE video_id IN (
                  SELECT id FROM videos WHERE series_id = ?1
              )",
            [series_id],
        )?;
        Ok(removed)
    }
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        time::{SystemTime, UNIX_EPOCH},
    };

    use crate::{
        database::{videos::ScannedFile, Database},
        time::now_ms,
    };

    fn temp_db(name: &str) -> (Database, std::path::PathBuf) {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("kanfanos-{name}-{stamp}"));
        fs::create_dir_all(&dir).unwrap();
        (Database::open(&dir.join("library.sqlite")).unwrap(), dir)
    }

    #[test]
    fn records_recent_and_resume_position() {
        let (db, dir) = temp_db("history");
        let library = db.add_library("X:\\library", "library", now_ms()).unwrap();
        let series = db
            .upsert_series(
                library.id,
                "2026年7月",
                "测试番",
                "X:\\library\\测试番",
                "",
                None,
                None,
                now_ms(),
            )
            .unwrap();

        let files = vec![
            ScannedFile {
                series_id: series,
                path: "X:\\library\\测试番\\第1集.mp4".to_string(),
                filename: "第1集.mp4".to_string(),
                extension: "mp4".to_string(),
                file_size: 1,
                modified_at: Some(1),
                kind: "main".to_string(),
            },
            ScannedFile {
                series_id: series,
                path: "X:\\library\\测试番\\第2集.mp4".to_string(),
                filename: "第2集.mp4".to_string(),
                extension: "mp4".to_string(),
                file_size: 1,
                modified_at: Some(1),
                kind: "main".to_string(),
            },
        ];
        db.upsert_videos(library.id, &files, now_ms()).unwrap();

        let episodes = db.list_episodes(series).unwrap();
        let first = episodes[0].id;
        let second = episodes[1].id;

        db.record_episode_opened(first, 1_000).unwrap();
        db.record_episode_opened(second, 2_000).unwrap();
        db.save_watch_position(first, 50_000, false, 2_500).unwrap();
        db.save_watch_position(second, 1_420_000, true, 2_600)
            .unwrap();

        assert_eq!(db.resume_position(first).unwrap(), (50_000, false));
        assert_eq!(db.resume_position(second).unwrap(), (1_420_000, true));

        let recent = db.recent_items(library.id, 10).unwrap();
        assert_eq!(recent.len(), 1);
        assert_eq!(recent[0].video_id, second);
        assert!(recent[0].completed);
        assert_eq!(recent[0].series_name.as_deref(), Some("测试番"));
        assert_eq!(recent[0].last_position_ms, 1_420_000);

        // 再次打开第一集只更新时间，不丢进度；同一部番仍然只有一条记录。
        db.record_episode_opened(first, 3_000).unwrap();
        let recent = db.recent_items(library.id, 10).unwrap();
        assert_eq!(recent.len(), 1);
        assert_eq!(recent[0].video_id, first);
        assert_eq!(recent[0].last_position_ms, 50_000);
        assert!(!recent[0].completed);

        // 整部番剧的批量操作。
        assert_eq!(db.mark_series_completed(series, 5_000).unwrap(), 2);
        let episodes = db.list_episodes(series).unwrap();
        assert!(episodes.iter().all(|episode| episode.completed));
        assert_eq!(db.clear_series_history(series).unwrap(), 2);
        let episodes = db.list_episodes(series).unwrap();
        assert!(episodes
            .iter()
            .all(|episode| !episode.completed && episode.last_position_ms.is_none()));
        assert!(db.recent_items(library.id, 10).unwrap().is_empty());

        // 手动计数不自动变化，并且不会小于 0。
        assert_eq!(db.set_series_watch_count(series, 3).unwrap(), 3);
        assert_eq!(db.get_series(series).unwrap().watch_count, 3);
        assert_eq!(db.set_series_watch_count(series, -5).unwrap(), 0);

        drop(db);
        let _ = fs::remove_dir_all(dir);
    }
}
