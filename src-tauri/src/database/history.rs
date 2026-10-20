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

    pub fn recent_items(&self, library_id: i64, limit: i64) -> Result<Vec<RecentItem>> {
        let conn = self.conn()?;
        let mut stmt = conn.prepare(
            "SELECT v.id,
                    v.series_id,
                    s.name,
                    s.category,
                    v.filename,
                    v.path,
                    v.duration_ms,
                    v.thumbnail_key,
                    w.last_opened_at,
                    w.last_position_ms,
                    w.completed
               FROM watch_history w
               JOIN videos v ON v.id = w.video_id
               LEFT JOIN series s ON s.id = v.series_id
              WHERE v.library_id = ?1 AND v.status <> 'missing'
              ORDER BY w.last_opened_at DESC
              LIMIT ?2",
        )?;

        let rows = stmt.query_map(params![library_id, limit], |row| {
            Ok(RecentItem {
                video_id: row.get(0)?,
                series_id: row.get(1)?,
                series_name: row.get(2)?,
                category: row.get(3)?,
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
            },
            ScannedFile {
                series_id: series,
                path: "X:\\library\\测试番\\第2集.mp4".to_string(),
                filename: "第2集.mp4".to_string(),
                extension: "mp4".to_string(),
                file_size: 1,
                modified_at: Some(1),
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
        assert_eq!(recent.len(), 2);
        assert_eq!(recent[0].video_id, second);
        assert!(recent[0].completed);
        assert_eq!(recent[0].series_name.as_deref(), Some("测试番"));
        assert_eq!(recent[1].last_position_ms, 50_000);

        // 再次打开第一集只更新时间，不丢进度。
        db.record_episode_opened(first, 3_000).unwrap();
        let recent = db.recent_items(library.id, 10).unwrap();
        assert_eq!(recent[0].video_id, first);
        assert_eq!(recent[0].last_position_ms, 50_000);
        assert!(!recent[0].completed);

        drop(db);
        let _ = fs::remove_dir_all(dir);
    }
}
