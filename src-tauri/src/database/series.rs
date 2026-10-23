use anyhow::{Context, Result};
use rusqlite::{params, OptionalExtension};
use serde::Serialize;

use crate::catalog::schedule_sort_key;

use super::Database;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CategorySummary {
    /// 用于筛选的分类键，目前与 label 相同。
    pub key: String,
    pub label: String,
    pub count: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SeriesSummary {
    pub id: i64,
    pub library_id: i64,
    pub category: String,
    pub name: String,
    pub path: String,
    pub episode_count: i64,
    /// 第一集有缩略图时作为番剧封面。
    pub cover_key: Option<String>,
    /// kanfanos.toml 中指定的封面图片绝对路径。
    pub cover_path: Option<String>,
    /// 封面文件的修改时间，用于避免浏览器缓存。
    pub cover_version: Option<i64>,
    /// 用户手动维护的「看过遍数」。
    pub watch_count: i64,
    /// 用户评分 0-10；None 表示未评分。
    pub rating: Option<i64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SeriesPage {
    pub items: Vec<SeriesSummary>,
    pub total: i64,
}

/// 配置弹窗里「上一季 / 下一季」选择目标时使用的最小信息。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SeriesOption {
    pub id: i64,
    pub name: String,
    pub category: String,
    pub path: String,
}

/// 修复配置文件时需要的最小番剧信息。
#[derive(Debug, Clone)]
pub struct SeriesConfigTarget {
    pub id: i64,
    pub category: String,
    pub path: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchItem {
    pub id: i64,
    pub library_id: i64,
    pub category: String,
    pub name: String,
    pub path: String,
    pub episode_count: i64,
    pub cover_key: Option<String>,
    pub cover_path: Option<String>,
    pub cover_version: Option<i64>,
    pub matched_episodes: i64,
    /// 第一条命中的剧集文件名，便于确认搜索为什么命中。
    pub match_preview: Option<String>,
    /// 用户手动维护的「看过遍数」。
    pub watch_count: i64,
    /// 用户评分 0-10；None 表示未评分。
    pub rating: Option<i64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchPage {
    pub items: Vec<SearchItem>,
    pub total: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EpisodeSummary {
    pub id: i64,
    pub filename: String,
    pub path: String,
    pub file_size: i64,
    pub duration_ms: Option<i64>,
    pub width: Option<i64>,
    pub height: Option<i64>,
    pub thumbnail_key: Option<String>,
    pub last_position_ms: Option<i64>,
    pub completed: bool,
}

const SERIES_SELECT: &str = "
    SELECT s.id,
           s.library_id,
           s.category,
           s.name,
           s.path,
           (SELECT COUNT(*) FROM videos v
             WHERE v.series_id = s.id
               AND v.status <> 'missing'
               AND v.kind = 'main') AS episode_count,
           (SELECT v.thumbnail_key FROM videos v
             WHERE v.series_id = s.id
               AND v.status <> 'missing'
               AND v.kind = 'main'
               AND v.thumbnail_key IS NOT NULL
             ORDER BY v.filename
             LIMIT 1) AS cover_key,
           s.cover_path,
           s.cover_version,
           s.watch_count,
           s.rating
      FROM series s
";

const SERIES_HAS_EPISODES: &str = "
    EXISTS (SELECT 1 FROM videos v
             WHERE v.series_id = s.id AND v.status <> 'missing')
";

impl Database {
    /// 按路径写入番剧；已存在时更新名称与分类。
    #[allow(clippy::too_many_arguments)]
    pub fn upsert_series(
        &self,
        library_id: i64,
        category: &str,
        name: &str,
        path: &str,
        aliases: &str,
        cover_path: Option<&str>,
        cover_version: Option<i64>,
        now: i64,
    ) -> Result<i64> {
        let conn = self.conn()?;
        conn.execute(
            "INSERT INTO series
                 (library_id, category, name, path, aliases, cover_path, cover_version, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?8)
             ON CONFLICT(path) DO UPDATE SET
                 library_id    = excluded.library_id,
                 category      = excluded.category,
                 name          = excluded.name,
                 aliases       = excluded.aliases,
                 cover_path    = excluded.cover_path,
                 cover_version = excluded.cover_version,
                 updated_at    = excluded.updated_at",
            params![
                library_id,
                category,
                name,
                path,
                aliases,
                cover_path,
                cover_version,
                now
            ],
        )?;

        Ok(conn.query_row(
            "SELECT id FROM series WHERE path = ?1",
            [path],
            |row| row.get(0),
        )?)
    }

    /// 设置手动「看过遍数」，不自动增减。
    pub fn set_series_watch_count(
        &self,
        series_id: i64,
        count: i64,
    ) -> Result<i64> {
        let count = count.max(0);
        self.conn()?.execute(
            "UPDATE series SET watch_count = ?2 WHERE id = ?1",
            params![series_id, count],
        )?;
        Ok(count)
    }

    /// 设置用户评分（0-10）；None 表示清除评分。
    pub fn set_series_rating(
        &self,
        series_id: i64,
        rating: Option<i64>,
    ) -> Result<Option<i64>> {
        if let Some(value) = rating
            && !(0..=10).contains(&value)
        {
            anyhow::bail!("评分必须在 0 到 10 之间");
        }

        let changed = self.conn()?.execute(
            "UPDATE series SET rating = ?2 WHERE id = ?1",
            params![series_id, rating],
        )?;
        if changed == 0 {
            anyhow::bail!("番剧不存在: id={series_id}");
        }

        Ok(rating)
    }

    /// 记录扫描中发现的档期目录，空档期同样保留。
    pub fn upsert_category(&self, library_id: i64, label: &str, now: i64) -> Result<()> {
        self.conn()?.execute(
            "INSERT INTO categories (library_id, label, sort_key, created_at)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(library_id, label) DO UPDATE SET
                 sort_key = excluded.sort_key",
            params![library_id, label, schedule_sort_key(label), now],
        )?;
        Ok(())
    }

    /// 完整扫描结束后删除已经不存在的档期目录。
    pub fn prune_categories(&self, library_id: i64, keep: &[String]) -> Result<()> {
        let conn = self.conn()?;

        if keep.is_empty() {
            conn.execute(
                "DELETE FROM categories WHERE library_id = ?1",
                [library_id],
            )?;
            return Ok(());
        }

        let placeholders = vec!["?"; keep.len()].join(",");
        let sql = format!(
            "DELETE FROM categories
              WHERE library_id = ?1 AND label NOT IN ({placeholders})"
        );

        let mut values: Vec<&dyn rusqlite::ToSql> = Vec::with_capacity(keep.len() + 1);
        values.push(&library_id);
        for label in keep {
            values.push(label);
        }

        conn.execute(&sql, values.as_slice())?;
        Ok(())
    }

    /// 档期来自目录名；count 是该档期下仍有视频的番剧数量。
    pub fn list_categories(&self, library_id: i64) -> Result<Vec<CategorySummary>> {
        let conn = self.conn()?;
        let mut stmt = conn.prepare(
            "SELECT c.label,
                    (SELECT COUNT(*)
                       FROM series s
                      WHERE s.library_id = c.library_id
                        AND s.category = c.label
                        AND EXISTS (
                            SELECT 1 FROM videos v
                             WHERE v.series_id = s.id AND v.status <> 'missing'
                        ))
               FROM categories c
              WHERE c.library_id = ?1
              ORDER BY c.sort_key DESC, c.label",
        )?;

        let rows = stmt.query_map([library_id], |row| {
            Ok(CategorySummary {
                key: row.get::<_, String>(0)?,
                label: row.get(0)?,
                count: row.get(1)?,
            })
        })?;

        let mut categories = Vec::new();
        for row in rows {
            categories.push(row?);
        }
        Ok(categories)
    }

    pub fn get_series(&self, id: i64) -> Result<SeriesSummary> {
        let conn = self.conn()?;
        conn.query_row(
            &format!("{SERIES_SELECT} WHERE s.id = ?1"),
            [id],
            map_series,
        )
        .optional()?
        .with_context(|| format!("番剧不存在: id={id}"))
    }

    /// 遍历一个库里的所有番剧目录，用于批量检查/修复 kanfanos.toml。
    pub fn list_series_config_targets(
        &self,
        library_id: i64,
    ) -> Result<Vec<SeriesConfigTarget>> {
        let conn = self.conn()?;
        let mut stmt = conn.prepare(
            "SELECT id, category, path
               FROM series
              WHERE library_id = ?1
              ORDER BY path COLLATE NOCASE",
        )?;

        let rows = stmt.query_map([library_id], |row| {
            Ok(SeriesConfigTarget {
                id: row.get(0)?,
                category: row.get(1)?,
                path: row.get(2)?,
            })
        })?;

        let mut items = Vec::new();
        for row in rows {
            items.push(row?);
        }
        Ok(items)
    }

    /// 配置弹窗的季链接候选：按关键词过滤整个库里有视频的番剧。
    /// 番剧可能上千部，因此始终带 LIMIT，由前端输入缩小范围。
    pub fn search_series_options(
        &self,
        library_id: i64,
        query: &str,
        limit: i64,
    ) -> Result<Vec<SeriesOption>> {
        let conn = self.conn()?;
        let limit = limit.clamp(1, 500);
        let pattern = like_pattern(query);
        let mut stmt = conn.prepare(&format!(
            "SELECT s.id, s.name, s.category, s.path
               FROM series s
               LEFT JOIN categories c
                 ON c.library_id = s.library_id AND c.label = s.category
              WHERE s.library_id = ?1 AND {SERIES_HAS_EPISODES}
                AND (
                    ?2 = ''
                    OR s.name LIKE ?3 ESCAPE '\\'
                    OR s.aliases LIKE ?3 ESCAPE '\\'
                    OR s.category LIKE ?3 ESCAPE '\\'
                    OR s.path LIKE ?3 ESCAPE '\\'
                )
              ORDER BY c.sort_key DESC, s.name COLLATE NOCASE
              LIMIT ?4"
        ))?;

        let rows = stmt.query_map(
            params![library_id, query, pattern, limit],
            map_series_option,
        )?;
        let mut items = Vec::new();
        for row in rows {
            items.push(row?);
        }
        Ok(items)
    }

    /// 按文件夹路径找季链接目标；番剧不存在或没有视频时返回 None。
    pub fn series_option_by_path(&self, path: &str) -> Result<Option<SeriesOption>> {
        Ok(self
            .conn()?
            .query_row(
                &format!(
                    "SELECT s.id, s.name, s.category, s.path
                       FROM series s
                      WHERE s.path = ?1 AND {SERIES_HAS_EPISODES}"
                ),
                [path],
                map_series_option,
            )
            .optional()?)
    }

    /// 配置弹窗保存后立即更新索引中的名称与别名，不必等待下一次扫描。
    pub fn update_series_meta(
        &self,
        series_id: i64,
        name: &str,
        aliases: &str,
        now: i64,
    ) -> Result<()> {
        let changed = self.conn()?.execute(
            "UPDATE series
                SET name = ?2, aliases = ?3, updated_at = ?4
              WHERE id = ?1",
            params![series_id, name, aliases, now],
        )?;

        if changed == 0 {
            anyhow::bail!("番剧不存在: id={series_id}");
        }
        Ok(())
    }

    /// 分页返回一个档期下的番剧。
    /// sort: `name` | `episodes` | `updated`。
    pub fn list_series_page(
        &self,
        library_id: i64,
        category: &str,
        sort: &str,
        limit: i64,
        offset: i64,
    ) -> Result<SeriesPage> {
        let conn = self.conn()?;
        let limit = limit.clamp(1, 200);
        let offset = offset.max(0);

        let total: i64 = conn.query_row(
            &format!(
                "SELECT COUNT(*) FROM series s
                  WHERE s.library_id = ?1 AND s.category = ?2 AND {SERIES_HAS_EPISODES}"
            ),
            params![library_id, category],
            |row| row.get(0),
        )?;

        // 排序字段在白名单内拼接，避免注入。
        let order = match sort {
            "episodes" => "episode_count DESC, s.name COLLATE NOCASE",
            "updated" => "s.updated_at DESC, s.name COLLATE NOCASE",
            _ => "s.name COLLATE NOCASE",
        };

        let sql = format!(
            "{SERIES_SELECT}
              WHERE s.library_id = ?1 AND s.category = ?2 AND {SERIES_HAS_EPISODES}
              ORDER BY {order}
              LIMIT ?3 OFFSET ?4"
        );

        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(
            params![library_id, category, limit, offset],
            map_series,
        )?;

        let mut items = Vec::new();
        for row in rows {
            items.push(row?);
        }
        Ok(SeriesPage { items, total })
    }

    /// 全库搜索：番剧名或剧集文件名命中即可。
    pub fn search_series(
        &self,
        library_id: i64,
        query: &str,
        limit: i64,
        offset: i64,
    ) -> Result<SearchPage> {
        let conn = self.conn()?;
        let limit = limit.clamp(1, 200);
        let offset = offset.max(0);
        let pattern = like_pattern(query);

        let total: i64 = conn.query_row(
            &format!(
                "SELECT COUNT(*) FROM series s
                  WHERE s.library_id = ?1 AND {SERIES_HAS_EPISODES}
                    AND (
                        s.name LIKE ?2 ESCAPE '\\'
                        OR s.aliases LIKE ?2 ESCAPE '\\'
                        OR EXISTS (
                            SELECT 1 FROM videos v
                             WHERE v.series_id = s.id
                               AND v.status <> 'missing'
                               AND v.filename LIKE ?2 ESCAPE '\\'
                        )
                    )"
            ),
            params![library_id, pattern],
            |row| row.get(0),
        )?;

        let sql = format!(
            "SELECT s.id,
                    s.library_id,
                    s.category,
                    s.name,
                    s.path,
                    (SELECT COUNT(*) FROM videos v
                      WHERE v.series_id = s.id
                        AND v.status <> 'missing'
                        AND v.kind = 'main') AS episode_count,
                    (SELECT v.thumbnail_key FROM videos v
                      WHERE v.series_id = s.id
                        AND v.status <> 'missing'
                        AND v.kind = 'main'
                        AND v.thumbnail_key IS NOT NULL
                      ORDER BY v.filename
                      LIMIT 1) AS cover_key,
                    s.cover_path,
                    s.cover_version,
                    (SELECT COUNT(*) FROM videos v
                      WHERE v.series_id = s.id
                        AND v.status <> 'missing'
                        AND v.filename LIKE ?2 ESCAPE '\\') AS matched_episodes,
                    (SELECT v.filename FROM videos v
                      WHERE v.series_id = s.id
                        AND v.status <> 'missing'
                        AND v.filename LIKE ?2 ESCAPE '\\'
                      ORDER BY v.filename
                      LIMIT 1) AS match_preview,
                    s.watch_count,
                    s.rating
               FROM series s
              WHERE s.library_id = ?1 AND {SERIES_HAS_EPISODES}
                AND (
                    s.name LIKE ?2 ESCAPE '\\'
                    OR s.aliases LIKE ?2 ESCAPE '\\'
                    OR EXISTS (
                        SELECT 1 FROM videos v
                         WHERE v.series_id = s.id
                           AND v.status <> 'missing'
                           AND v.filename LIKE ?2 ESCAPE '\\'
                    )
                )
              ORDER BY CASE WHEN s.name LIKE ?2 ESCAPE '\\'
                              OR s.aliases LIKE ?2 ESCAPE '\\'
                            THEN 0 ELSE 1 END,
                       matched_episodes DESC,
                       s.name COLLATE NOCASE
              LIMIT ?3 OFFSET ?4"
        );

        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(
            params![library_id, pattern, limit, offset],
            |row| {
                Ok(SearchItem {
                    id: row.get(0)?,
                    library_id: row.get(1)?,
                    category: row.get(2)?,
                    name: row.get(3)?,
                    path: row.get(4)?,
                    episode_count: row.get(5)?,
                    cover_key: row.get(6)?,
                    cover_path: row.get(7)?,
                    cover_version: row.get(8)?,
                    matched_episodes: row.get(9)?,
                    match_preview: row.get(10)?,
                    watch_count: row.get(11)?,
                    rating: row.get(12)?,
                })
            },
        )?;

        let mut items = Vec::new();
        for row in rows {
            items.push(row?);
        }
        Ok(SearchPage { items, total })
    }

    pub fn list_episodes(&self, series_id: i64) -> Result<Vec<EpisodeSummary>> {
        self.episodes_by_kind(series_id, "main")
    }

    /// 番剧子文件夹里的 CM/PV/特典，进入二级选集页。
    pub fn list_extra_episodes(&self, series_id: i64) -> Result<Vec<EpisodeSummary>> {
        self.episodes_by_kind(series_id, "extra")
    }

    fn episodes_by_kind(
        &self,
        series_id: i64,
        kind: &str,
    ) -> Result<Vec<EpisodeSummary>> {
        let conn = self.conn()?;
        let mut stmt = conn.prepare(
            "SELECT v.id,
                    v.filename,
                    v.path,
                    v.file_size,
                    v.duration_ms,
                    v.width,
                    v.height,
                    v.thumbnail_key,
                    w.last_position_ms,
                    COALESCE(w.completed, 0)
               FROM videos v
               LEFT JOIN watch_history w ON w.video_id = v.id
              WHERE v.series_id = ?1
                AND v.status <> 'missing'
                AND v.kind = ?2
              ORDER BY v.filename COLLATE NOCASE, v.path",
        )?;

        let rows = stmt.query_map(params![series_id, kind], |row| {
            Ok(EpisodeSummary {
                id: row.get(0)?,
                filename: row.get(1)?,
                path: row.get(2)?,
                file_size: row.get(3)?,
                duration_ms: row.get(4)?,
                width: row.get(5)?,
                height: row.get(6)?,
                thumbnail_key: row.get(7)?,
                last_position_ms: row.get(8)?,
                completed: row.get::<_, i64>(9)? != 0,
            })
        })?;

        let mut episodes = Vec::new();
        for row in rows {
            episodes.push(row?);
        }
        Ok(episodes)
    }
}

fn map_series(row: &rusqlite::Row<'_>) -> rusqlite::Result<SeriesSummary> {
    Ok(SeriesSummary {
        id: row.get(0)?,
        library_id: row.get(1)?,
        category: row.get(2)?,
        name: row.get(3)?,
        path: row.get(4)?,
        episode_count: row.get(5)?,
        cover_key: row.get(6)?,
        cover_path: row.get(7)?,
        cover_version: row.get(8)?,
        watch_count: row.get(9)?,
        rating: row.get(10)?,
    })
}

fn map_series_option(row: &rusqlite::Row<'_>) -> rusqlite::Result<SeriesOption> {
    Ok(SeriesOption {
        id: row.get(0)?,
        name: row.get(1)?,
        category: row.get(2)?,
        path: row.get(3)?,
    })
}

/// 转义 LIKE 通配符，避免用户输入 `%` 或 `_` 时匹配范围失控。
fn like_pattern(query: &str) -> String {
    let mut pattern = String::with_capacity(query.len() + 2);
    pattern.push('%');

    for ch in query.chars() {
        if matches!(ch, '\\' | '%' | '_') {
            pattern.push('\\');
        }
        pattern.push(ch);
    }

    pattern.push('%');
    pattern
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

    use super::like_pattern;

    fn temp_db(name: &str) -> (Database, std::path::PathBuf) {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("kanfanos-{name}-{stamp}"));
        fs::create_dir_all(&dir).unwrap();
        (Database::open(&dir.join("library.sqlite")).unwrap(), dir)
    }

    fn add_series_with_episodes(
        db: &Database,
        library_id: i64,
        category: &str,
        name: &str,
        episode_names: &[&str],
    ) -> i64 {
        let series_id = db
            .upsert_series(
                library_id,
                category,
                name,
                &format!("X:\\{name}"),
                "",
                None,
                None,
                now_ms(),
            )
            .unwrap();

        let files: Vec<ScannedFile> = episode_names
            .iter()
            .map(|filename| ScannedFile {
                series_id,
                path: format!("X:\\{name}\\{filename}"),
                filename: (*filename).to_string(),
                extension: "mp4".to_string(),
                file_size: 1,
                modified_at: Some(1),
                kind: "main".to_string(),
            })
            .collect();

        db.upsert_videos(library_id, &files, now_ms()).unwrap();
        series_id
    }

    #[test]
    fn paginates_and_sorts_series() {
        let (db, dir) = temp_db("series-page");
        let library = db.add_library("X:\\library", "library", now_ms()).unwrap();

        add_series_with_episodes(&db, library.id, "2026年7月", "A番", &["第1集.mp4", "第2集.mp4"]);
        add_series_with_episodes(&db, library.id, "2026年7月", "B番", &["第1集.mp4"]);
        add_series_with_episodes(&db, library.id, "2026年7月", "C番", &["第1集.mp4"]);
        add_series_with_episodes(&db, library.id, "2026年7月", "空番", &[]);

        let page = db
            .list_series_page(library.id, "2026年7月", "episodes", 2, 0)
            .unwrap();
        assert_eq!(page.total, 3);
        assert_eq!(page.items.len(), 2);
        assert_eq!(page.items[0].name, "A番");
        assert_eq!(page.items[0].episode_count, 2);

        let second = db
            .list_series_page(library.id, "2026年7月", "episodes", 2, 2)
            .unwrap();
        assert_eq!(second.items.len(), 1);

        let by_name = db
            .list_series_page(library.id, "2026年7月", "name", 10, 0)
            .unwrap();
        assert_eq!(by_name.items[0].name, "A番");

        drop(db);
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn sets_and_clears_rating() {
        let (db, dir) = temp_db("series-rating");
        let library = db.add_library("X:\\library", "library", now_ms()).unwrap();
        let series = add_series_with_episodes(
            &db,
            library.id,
            "2026年7月",
            "评番",
            &["第1集.mp4"],
        );

        assert_eq!(db.get_series(series).unwrap().rating, None);
        assert_eq!(db.set_series_rating(series, Some(0)).unwrap(), Some(0));
        assert_eq!(db.get_series(series).unwrap().rating, Some(0));
        assert_eq!(db.set_series_rating(series, Some(10)).unwrap(), Some(10));
        assert_eq!(db.get_series(series).unwrap().rating, Some(10));

        // 超出范围要拒绝，不能写进数据库。
        assert!(db.set_series_rating(series, Some(11)).is_err());
        assert!(db.set_series_rating(series, Some(-1)).is_err());
        assert_eq!(db.get_series(series).unwrap().rating, Some(10));

        assert_eq!(db.set_series_rating(series, None).unwrap(), None);
        assert_eq!(db.get_series(series).unwrap().rating, None);

        // 列表和搜索也要带上评分。
        db.set_series_rating(series, Some(7)).unwrap();
        let page = db
            .list_series_page(library.id, "2026年7月", "name", 10, 0)
            .unwrap();
        assert_eq!(page.items[0].rating, Some(7));
        let search = db.search_series(library.id, "评番", 10, 0).unwrap();
        assert_eq!(search.items[0].rating, Some(7));

        drop(db);
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn updates_meta_and_resolves_series_options() {
        let (db, dir) = temp_db("series-options");
        let library = db.add_library("X:\\library", "library", now_ms()).unwrap();

        // 扫描时档期目录会先写入 categories；排序依赖它的 sort_key。
        db.upsert_category(library.id, "2025年10月", now_ms())
            .unwrap();
        db.upsert_category(library.id, "2026年1月", now_ms())
            .unwrap();

        let first = add_series_with_episodes(
            &db,
            library.id,
            "2025年10月",
            "A季",
            &["第1集.mp4"],
        );
        let second = add_series_with_episodes(
            &db,
            library.id,
            "2026年1月",
            "B季",
            &["第1集.mp4"],
        );

        let options = db.search_series_options(library.id, "", 200).unwrap();
        assert_eq!(options.len(), 2);
        assert_eq!(options[0].name, "B季");
        assert_eq!(options[1].name, "A季");

        // 关键词匹配番剧名，也支持别名和档期。
        let filtered = db.search_series_options(library.id, "A季", 200).unwrap();
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].name, "A季");
        assert_eq!(
            db.search_series_options(library.id, "2025年", 200)
                .unwrap()
                .len(),
            1
        );

        let by_path = db.series_option_by_path("X:\\B季").unwrap().unwrap();
        assert_eq!(by_path.id, second);
        assert!(db.series_option_by_path("X:\\不存在").unwrap().is_none());

        db.update_series_meta(first, "A季（新）", "别名甲\n别名乙", now_ms())
            .unwrap();
        assert_eq!(db.get_series(first).unwrap().name, "A季（新）");
        assert_eq!(
            db.search_series(library.id, "别名乙", 10, 0)
                .unwrap()
                .total,
            1
        );

        drop(db);
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn searches_by_series_and_episode_name() {
        let (db, dir) = temp_db("series-search");
        let library = db.add_library("X:\\library", "library", now_ms()).unwrap();

        add_series_with_episodes(
            &db,
            library.id,
            "2026年7月",
            "测试番",
            &["第1集 初次见面.mp4", "第2集.mp4"],
        );
        add_series_with_episodes(&db, library.id, "2025年10月", "另一个番", &["第1集.mp4"]);

        let by_episode = db.search_series(library.id, "初次见面", 10, 0).unwrap();
        assert_eq!(by_episode.total, 1);
        assert_eq!(by_episode.items[0].name, "测试番");
        assert_eq!(by_episode.items[0].matched_episodes, 1);
        assert_eq!(
            by_episode.items[0].match_preview.as_deref(),
            Some("第1集 初次见面.mp4")
        );

        let by_name = db.search_series(library.id, "另一个", 10, 0).unwrap();
        assert_eq!(by_name.total, 1);
        assert_eq!(by_name.items[0].matched_episodes, 0);

        // LIKE 通配符必须被转义。
        let wildcard = db.search_series(library.id, "%", 10, 0).unwrap();
        assert_eq!(wildcard.total, 0);

        drop(db);
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn escapes_like_patterns() {
        assert_eq!(like_pattern("ab"), "%ab%");
        assert_eq!(like_pattern("50%"), "%50\\%%");
        assert_eq!(like_pattern("a_b"), "%a\\_b%");
    }
}
