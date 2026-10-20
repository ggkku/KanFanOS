use anyhow::{Context, Result};
use rusqlite::{params, OptionalExtension};
use serde::Serialize;

use super::Database;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LibrarySummary {
    pub id: i64,
    pub root_path: String,
    pub display_name: String,
    pub enabled: bool,
    pub last_scan_at: Option<i64>,
    pub video_count: i64,
    pub created_at: i64,
}

/// 视频只统计未标记为 missing 的记录。
const SELECT_LIBRARIES: &str = "
    SELECT l.id,
           l.root_path,
           COALESCE(l.display_name, l.root_path),
           l.enabled,
           l.last_scan_at,
           l.created_at,
           (SELECT COUNT(*) FROM videos v
             WHERE v.library_id = l.id AND v.status <> 'missing')
      FROM libraries l
";

impl Database {
    /// 添加目录；重复添加同一个路径时复用原记录。
    pub fn add_library(
        &self,
        root_path: &str,
        display_name: &str,
        now: i64,
    ) -> Result<LibrarySummary> {
        let id = {
            let conn = self.conn()?;
            conn.execute(
                "INSERT INTO libraries (root_path, display_name, enabled, created_at)
                 VALUES (?1, ?2, 1, ?3)
                 ON CONFLICT(root_path) DO UPDATE SET
                     enabled = 1,
                     display_name = excluded.display_name",
                params![root_path, display_name, now],
            )?;
            conn.query_row(
                "SELECT id FROM libraries WHERE root_path = ?1",
                [root_path],
                |row| row.get::<_, i64>(0),
            )?
        };

        self.get_library(id)
    }

    pub fn get_library(&self, id: i64) -> Result<LibrarySummary> {
        let conn = self.conn()?;
        conn.query_row(
            &format!("{SELECT_LIBRARIES} WHERE l.id = ?1"),
            [id],
            map_library,
        )
        .optional()?
        .with_context(|| format!("媒体目录不存在: id={id}"))
    }

    /// 只删除索引记录，不触碰原始文件。保留给后续的目录管理功能。
    #[allow(dead_code)]
    pub fn remove_library(&self, id: i64) -> Result<bool> {
        let deleted = self
            .conn()?
            .execute("DELETE FROM libraries WHERE id = ?1", [id])?;
        Ok(deleted > 0)
    }

    pub fn library_root(&self, id: i64) -> Result<String> {
        let conn = self.conn()?;
        conn.query_row(
            "SELECT root_path FROM libraries WHERE id = ?1",
            [id],
            |row| row.get(0),
        )
        .optional()?
        .with_context(|| format!("媒体目录不存在: id={id}"))
    }
}

fn map_library(row: &rusqlite::Row<'_>) -> rusqlite::Result<LibrarySummary> {
    Ok(LibrarySummary {
        id: row.get(0)?,
        root_path: row.get(1)?,
        display_name: row.get(2)?,
        enabled: row.get::<_, i64>(3)? != 0,
        last_scan_at: row.get(4)?,
        created_at: row.get(5)?,
        video_count: row.get(6)?,
    })
}
