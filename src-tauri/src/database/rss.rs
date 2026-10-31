//! RSS 追番：订阅源、追番规则、条目和匹配记录。

use std::path::Path;

use anyhow::Result;
use rusqlite::{params, OptionalExtension};
use serde::Serialize;

use super::Database;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RssFeed {
    pub id: i64,
    pub url: String,
    pub title: String,
    pub site: String,
    pub last_fetch_at: Option<i64>,
    pub last_error: Option<String>,
    pub created_at: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RssFollow {
    pub id: i64,
    pub feed_id: i64,
    pub feed_title: String,
    pub series_id: Option<i64>,
    pub title: String,
    pub aliases: Vec<String>,
    pub preferred_groups: Vec<String>,
    pub subtitle_packaging: String,
    pub resolution: String,
    pub save_path: Option<String>,
    pub save_path_exists: bool,
    pub auto_download: bool,
    pub last_episode: Option<i64>,
    pub status: String,
    pub created_at: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RssEntry {
    pub id: i64,
    pub feed_id: i64,
    pub title: String,
    pub link: String,
    pub download_url: String,
    pub published_at: i64,
    pub first_seen_at: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RssMatch {
    pub id: i64,
    pub entry_id: i64,
    pub follow_id: i64,
    pub follow_title: String,
    pub entry_title: String,
    pub release_group: Option<String>,
    pub subtitle_packaging: Option<String>,
    pub link: String,
    pub download_url: String,
    pub published_at: i64,
    pub episode: Option<i64>,
    pub confidence: i64,
    pub status: String,
    pub auto_downloaded: bool,
    pub error: Option<String>,
    pub created_at: i64,
}

impl Database {
    pub fn list_rss_feeds(&self) -> Result<Vec<RssFeed>> {
        let conn = self.conn()?;
        let mut stmt = conn.prepare(
            "SELECT id, url, title, site, last_fetch_at, last_error, created_at
               FROM rss_feeds
              ORDER BY created_at ASC, id ASC",
        )?;
        let rows = stmt.query_map([], map_feed)?;
        let mut feeds = Vec::new();
        for row in rows {
            feeds.push(row?);
        }
        Ok(feeds)
    }

    pub fn get_rss_feed(&self, id: i64) -> Result<Option<RssFeed>> {
        let conn = self.conn()?;
        Ok(conn
            .query_row(
                "SELECT id, url, title, site, last_fetch_at, last_error, created_at
                   FROM rss_feeds WHERE id = ?1",
                [id],
                map_feed,
            )
            .optional()?)
    }

    pub fn add_rss_feed(&self, url: &str, title: &str, site: &str, now: i64) -> Result<RssFeed> {
        let conn = self.conn()?;
        conn.execute(
            "INSERT INTO rss_feeds (url, title, site, created_at)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(url) DO UPDATE SET
                 title = CASE WHEN excluded.title <> '' THEN excluded.title ELSE rss_feeds.title END,
                 site = CASE WHEN excluded.site <> '' THEN excluded.site ELSE rss_feeds.site END",
            params![url, title, site, now],
        )?;
        Ok(conn.query_row(
            "SELECT id, url, title, site, last_fetch_at, last_error, created_at
               FROM rss_feeds WHERE url = ?1",
            [url],
            map_feed,
        )?)
    }

    pub fn update_rss_feed_fetch(
        &self,
        id: i64,
        title: &str,
        site: &str,
        fetched_at: i64,
        error: Option<&str>,
    ) -> Result<()> {
        self.conn()?.execute(
            "UPDATE rss_feeds
                SET title = CASE WHEN ?2 <> '' THEN ?2 ELSE title END,
                    site = CASE WHEN ?3 <> '' THEN ?3 ELSE site END,
                    last_fetch_at = ?4,
                    last_error = ?5
              WHERE id = ?1",
            params![id, title, site, fetched_at, error],
        )?;
        Ok(())
    }

    pub fn delete_rss_feed(&self, id: i64) -> Result<usize> {
        Ok(self
            .conn()?
            .execute("DELETE FROM rss_feeds WHERE id = ?1", [id])?)
    }

    pub fn list_rss_follows(&self) -> Result<Vec<RssFollow>> {
        let conn = self.conn()?;
        let mut stmt = conn.prepare(
            "SELECT f.id, f.feed_id, rf.title, f.series_id, f.title, f.aliases,
                    f.preferred_groups, f.subtitle_packaging, f.resolution,
                    f.auto_download, f.save_path, f.last_episode, f.status, f.created_at
               FROM rss_follows f
               JOIN rss_feeds rf ON rf.id = f.feed_id
              ORDER BY f.created_at ASC, f.id ASC",
        )?;
        let rows = stmt.query_map([], map_follow)?;
        let mut follows = Vec::new();
        for row in rows {
            follows.push(row?);
        }
        Ok(follows)
    }

    pub fn get_rss_follow(&self, id: i64) -> Result<Option<RssFollow>> {
        let conn = self.conn()?;
        Ok(conn
            .query_row(
                "SELECT f.id, f.feed_id, rf.title, f.series_id, f.title, f.aliases,
                        f.preferred_groups, f.subtitle_packaging, f.resolution,
                        f.auto_download, f.save_path, f.last_episode, f.status, f.created_at
                   FROM rss_follows f JOIN rss_feeds rf ON rf.id = f.feed_id
                  WHERE f.id = ?1",
                [id],
                map_follow,
            )
            .optional()?)
    }

    pub fn add_rss_follow(
        &self,
        feed_id: i64,
        series_id: Option<i64>,
        title: &str,
        aliases: &[String],
        preferred_groups: &[String],
        subtitle_packaging: &str,
        resolution: &str,
        save_path: Option<&str>,
        auto_download: bool,
        now: i64,
    ) -> Result<RssFollow> {
        let conn = self.conn()?;
        let aliases = serde_json::to_string(aliases)?;
        let preferred_groups = serde_json::to_string(preferred_groups)?;
        conn.execute(
            "INSERT INTO rss_follows
                 (feed_id, series_id, title, aliases, preferred_groups,
                  subtitle_packaging, resolution, save_path, auto_download,
                  created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?10)",
            params![
                feed_id,
                series_id,
                title,
                aliases,
                preferred_groups,
                subtitle_packaging,
                resolution,
                save_path,
                if auto_download { 1 } else { 0 },
                now
            ],
        )?;
        let id = conn.last_insert_rowid();
        Ok(conn.query_row(
            "SELECT f.id, f.feed_id, rf.title, f.series_id, f.title, f.aliases,
                    f.preferred_groups, f.subtitle_packaging, f.resolution,
                    f.auto_download, f.save_path, f.last_episode, f.status, f.created_at
               FROM rss_follows f JOIN rss_feeds rf ON rf.id = f.feed_id
              WHERE f.id = ?1",
            [id],
            map_follow,
        )?)
    }

    pub fn update_rss_follow(
        &self,
        id: i64,
        title: &str,
        aliases: &[String],
        preferred_groups: &[String],
        subtitle_packaging: &str,
        resolution: &str,
        save_path: Option<&str>,
        auto_download: bool,
        status: &str,
        now: i64,
    ) -> Result<()> {
        let aliases = serde_json::to_string(aliases)?;
        let preferred_groups = serde_json::to_string(preferred_groups)?;
        self.conn()?.execute(
            "UPDATE rss_follows
                SET title = ?2, aliases = ?3, preferred_groups = ?4,
                    subtitle_packaging = ?5, resolution = ?6, save_path = ?7,
                    auto_download = ?8, status = ?9, updated_at = ?10
              WHERE id = ?1",
            params![
                id,
                title,
                aliases,
                preferred_groups,
                subtitle_packaging,
                resolution,
                save_path,
                if auto_download { 1 } else { 0 },
                status,
                now
            ],
        )?;
        Ok(())
    }

    pub fn delete_rss_follow(&self, id: i64) -> Result<usize> {
        Ok(self
            .conn()?
            .execute("DELETE FROM rss_follows WHERE id = ?1", [id])?)
    }

    pub fn insert_rss_entry(
        &self,
        feed_id: i64,
        guid: &str,
        title: &str,
        link: &str,
        download_url: &str,
        published_at: i64,
        now: i64,
    ) -> Result<Option<i64>> {
        let conn = self.conn()?;
        let inserted = conn.execute(
            "INSERT OR IGNORE INTO rss_entries
                 (feed_id, guid, title, link, download_url, published_at, first_seen_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![feed_id, guid, title, link, download_url, published_at, now],
        )?;
        if inserted == 0 {
            return Ok(None);
        }
        Ok(Some(conn.query_row(
            "SELECT id FROM rss_entries WHERE feed_id = ?1 AND guid = ?2",
            params![feed_id, guid],
            |row| row.get(0),
        )?))
    }

    pub fn recent_rss_entries(&self, feed_id: i64, limit: i64) -> Result<Vec<RssEntry>> {
        let conn = self.conn()?;
        let mut stmt = conn.prepare(
            "SELECT id, feed_id, title, link, download_url, published_at, first_seen_at
               FROM rss_entries WHERE feed_id = ?1
              ORDER BY published_at DESC, id DESC LIMIT ?2",
        )?;
        let rows = stmt.query_map(params![feed_id, limit], |row| {
            Ok(RssEntry {
                id: row.get(0)?,
                feed_id: row.get(1)?,
                title: row.get(2)?,
                link: row.get(3)?,
                download_url: row.get(4)?,
                published_at: row.get(5)?,
                first_seen_at: row.get(6)?,
            })
        })?;
        let mut entries = Vec::new();
        for row in rows {
            entries.push(row?);
        }
        Ok(entries)
    }

    pub fn insert_rss_match(
        &self,
        entry_id: i64,
        follow_id: i64,
        episode: Option<i64>,
        confidence: i64,
        now: i64,
    ) -> Result<bool> {
        Ok(self.conn()?.execute(
            "INSERT OR IGNORE INTO rss_matches
                 (entry_id, follow_id, episode, confidence, status, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, 'pending', ?5, ?5)",
            params![entry_id, follow_id, episode, confidence, now],
        )? > 0)
    }

    pub fn pending_rss_matches(&self, follow_id: i64) -> Result<Vec<RssMatch>> {
        let conn = self.conn()?;
        let mut stmt = conn.prepare(
            "SELECT m.id, m.entry_id, m.follow_id, f.title, e.title, e.link,
                    e.download_url, e.published_at, m.episode, m.confidence,
                    m.status, m.auto_downloaded, m.error, m.created_at
               FROM rss_matches m
               JOIN rss_entries e ON e.id = m.entry_id
               JOIN rss_follows f ON f.id = m.follow_id
              WHERE m.follow_id = ?1 AND m.status = 'pending'
              ORDER BY m.episode DESC NULLS LAST, m.confidence DESC, e.published_at DESC",
        )?;
        let rows = stmt.query_map([follow_id], map_match)?;
        let mut matches = Vec::new();
        for row in rows {
            matches.push(row?);
        }
        Ok(matches)
    }

    pub fn recent_rss_matches(&self, limit: i64) -> Result<Vec<RssMatch>> {
        let conn = self.conn()?;
        let mut stmt = conn.prepare(
            "SELECT m.id, m.entry_id, m.follow_id, f.title, e.title, e.link,
                    e.download_url, e.published_at, m.episode, m.confidence,
                    m.status, m.auto_downloaded, m.error, m.created_at
               FROM rss_matches m
               JOIN rss_entries e ON e.id = m.entry_id
               JOIN rss_follows f ON f.id = m.follow_id
              ORDER BY m.created_at DESC, m.id DESC LIMIT ?1",
        )?;
        let rows = stmt.query_map([limit], map_match)?;
        let mut matches = Vec::new();
        for row in rows {
            matches.push(row?);
        }
        Ok(matches)
    }

    pub fn recent_rss_matches_for_follows(
        &self,
        limit: i64,
        follow_ids: &[i64],
    ) -> Result<Vec<RssMatch>> {
        if follow_ids.is_empty() {
            return Ok(Vec::new());
        }
        let conn = self.conn()?;
        let placeholders = std::iter::repeat_n("?", follow_ids.len())
            .collect::<Vec<_>>()
            .join(",");
        let sql = format!(
            "SELECT m.id, m.entry_id, m.follow_id, f.title, e.title, e.link,
                    e.download_url, e.published_at, m.episode, m.confidence,
                    m.status, m.auto_downloaded, m.error, m.created_at
               FROM rss_matches m
               JOIN rss_entries e ON e.id = m.entry_id
               JOIN rss_follows f ON f.id = m.follow_id
              WHERE m.follow_id IN ({placeholders})
              ORDER BY m.created_at DESC, m.id DESC LIMIT ?"
        );
        let mut stmt = conn.prepare(&sql)?;
        let params = follow_ids
            .iter()
            .copied()
            .chain(std::iter::once(limit));
        let rows = stmt.query_map(rusqlite::params_from_iter(params), map_match)?;
        let mut matches = Vec::new();
        for row in rows {
            matches.push(row?);
        }
        Ok(matches)
    }

    pub fn get_rss_match(&self, id: i64) -> Result<Option<RssMatch>> {
        let conn = self.conn()?;
        Ok(conn
            .query_row(
                "SELECT m.id, m.entry_id, m.follow_id, f.title, e.title, e.link,
                        e.download_url, e.published_at, m.episode, m.confidence,
                        m.status, m.auto_downloaded, m.error, m.created_at
                   FROM rss_matches m
                   JOIN rss_entries e ON e.id = m.entry_id
                   JOIN rss_follows f ON f.id = m.follow_id
                  WHERE m.id = ?1",
                [id],
                map_match,
            )
            .optional()?)
    }

    pub fn set_rss_match_status(
        &self,
        id: i64,
        status: &str,
        error: Option<&str>,
        now: i64,
    ) -> Result<()> {
        self.conn()?.execute(
            "UPDATE rss_matches SET status = ?2, error = ?3, updated_at = ?4 WHERE id = ?1",
            params![id, status, error, now],
        )?;
        Ok(())
    }

    pub fn mark_rss_match_queued(&self, id: i64, auto_downloaded: bool, now: i64) -> Result<()> {
        self.conn()?.execute(
            "UPDATE rss_matches
                SET status = 'queued', auto_downloaded = ?2, error = NULL, updated_at = ?3
              WHERE id = ?1",
            params![id, if auto_downloaded { 1 } else { 0 }, now],
        )?;
        Ok(())
    }

    pub fn set_rss_follow_last_episode(&self, id: i64, episode: i64) -> Result<()> {
        self.conn()?.execute(
            "UPDATE rss_follows SET last_episode = ?2 WHERE id = ?1",
            params![id, episode],
        )?;
        Ok(())
    }
}

fn map_feed(row: &rusqlite::Row<'_>) -> rusqlite::Result<RssFeed> {
    Ok(RssFeed {
        id: row.get(0)?,
        url: row.get(1)?,
        title: row.get(2)?,
        site: row.get(3)?,
        last_fetch_at: row.get(4)?,
        last_error: row.get(5)?,
        created_at: row.get(6)?,
    })
}

fn map_follow(row: &rusqlite::Row<'_>) -> rusqlite::Result<RssFollow> {
    let aliases: String = row.get(5)?;
    let preferred_groups: String = row.get(6)?;
    let save_path: Option<String> = row.get(10)?;
    Ok(RssFollow {
        id: row.get(0)?,
        feed_id: row.get(1)?,
        feed_title: row.get(2)?,
        series_id: row.get(3)?,
        title: row.get(4)?,
        aliases: serde_json::from_str(&aliases).unwrap_or_default(),
        preferred_groups: serde_json::from_str(&preferred_groups).unwrap_or_default(),
        subtitle_packaging: row.get(7)?,
        resolution: row.get(8)?,
        auto_download: row.get::<_, i64>(9)? != 0,
        save_path_exists: save_path
            .as_deref()
            .map(|path| Path::new(path).is_dir())
            .unwrap_or(true),
        save_path,
        last_episode: row.get(11)?,
        status: row.get(12)?,
        created_at: row.get(13)?,
    })
}

fn map_match(row: &rusqlite::Row<'_>) -> rusqlite::Result<RssMatch> {
    let entry_title: String = row.get(4)?;
    Ok(RssMatch {
        id: row.get(0)?,
        entry_id: row.get(1)?,
        follow_id: row.get(2)?,
        follow_title: row.get(3)?,
        release_group: crate::rss::release_group(&entry_title),
        subtitle_packaging: crate::rss::release_subtitle_packaging(&entry_title),
        entry_title,
        link: row.get(5)?,
        download_url: row.get(6)?,
        published_at: row.get(7)?,
        episode: row.get(8)?,
        confidence: row.get(9)?,
        status: row.get(10)?,
        auto_downloaded: row.get::<_, i64>(11)? != 0,
        error: row.get(12)?,
        created_at: row.get(13)?,
    })
}

#[cfg(test)]
mod tests {
    use std::{fs, time::{SystemTime, UNIX_EPOCH}};

    use super::Database;

    fn temp_db() -> (Database, std::path::PathBuf) {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("kanfanos-rss-{stamp}"));
        fs::create_dir_all(&dir).unwrap();
        (Database::open(&dir.join("library.sqlite")).unwrap(), dir)
    }

    #[test]
    fn stores_feeds_follows_entries_and_matches() {
        let (db, dir) = temp_db();
        let feed = db.add_rss_feed("https://example.com/rss", "测试源", "", 1).unwrap();
        let follow = db
            .add_rss_follow(
                feed.id,
                None,
                "测试番",
                &["别名".to_string()],
                &[],
                "",
                "1080p",
                Some("D:\\Anime\\测试番"),
                true,
                2,
            )
            .unwrap();
        assert_eq!(follow.save_path.as_deref(), Some("D:\\Anime\\测试番"));
        let entry = db
            .insert_rss_entry(
                feed.id,
                "ep-1",
                "[组] 测试番 - 01 [1080p]",
                "https://example.com/1",
                "magnet:?xt=urn:btih:test",
                10,
                11,
            )
            .unwrap()
            .unwrap();
        assert!(db
            .insert_rss_match(entry, follow.id, Some(1), 100, 12)
            .unwrap());
        assert!(!db
            .insert_rss_match(entry, follow.id, Some(1), 100, 13)
            .unwrap());

        let pending = db.pending_rss_matches(follow.id).unwrap();
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].episode, Some(1));
        assert_eq!(pending[0].follow_title, "测试番");

        let follows = db.list_rss_follows().unwrap();
        assert_eq!(follows[0].aliases, vec!["别名"]);
        assert!(follows[0].auto_download);

        let entries = db.recent_rss_entries(feed.id, 10).unwrap();
        assert_eq!(entries[0].download_url, "magnet:?xt=urn:btih:test");

        assert_eq!(
            db.recent_rss_matches_for_follows(10, &[follow.id])
                .unwrap()
                .len(),
            1
        );
        assert!(db
            .recent_rss_matches_for_follows(10, &[999])
            .unwrap()
            .is_empty());
        assert!(db.recent_rss_matches_for_follows(10, &[]).unwrap().is_empty());

        db.update_rss_follow(
            follow.id,
            "测试番",
            &["别名".to_string()],
            &["ANi".to_string()],
            "简繁日内封",
            "1080p",
            Some("E:\\Anime\\测试番"),
            false,
            "paused",
            3,
        )
        .unwrap();
        let updated = db.get_rss_follow(follow.id).unwrap().unwrap();
        assert_eq!(updated.save_path.as_deref(), Some("E:\\Anime\\测试番"));
        assert_eq!(updated.preferred_groups, vec!["ANi"]);
        assert_eq!(updated.subtitle_packaging, "简繁日内封");
        assert!(!updated.auto_download);
        assert_eq!(updated.status, "paused");

        db.set_rss_follow_last_episode(follow.id, 1).unwrap();
        db.set_rss_match_status(pending[0].id, "queued", None, 14).unwrap();
        db.mark_rss_match_queued(pending[0].id, true, 15).unwrap();
        assert!(db.get_rss_match(pending[0].id).unwrap().unwrap().auto_downloaded);
        db.mark_rss_match_queued(pending[0].id, false, 16).unwrap();
        assert!(!db.get_rss_match(pending[0].id).unwrap().unwrap().auto_downloaded);
        assert!(db
            .get_rss_follow(follow.id)
            .unwrap()
            .unwrap()
            .last_episode
            == Some(1));
        assert_eq!(
            db.get_rss_match(pending[0].id)
                .unwrap()
                .unwrap()
                .status,
            "queued"
        );

        let _ = fs::remove_dir_all(dir);
    }
}
