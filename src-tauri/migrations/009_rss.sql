-- RSS 追番：订阅源、追番规则、抓取条目和匹配结果。
CREATE TABLE rss_feeds (
    id INTEGER PRIMARY KEY,
    url TEXT NOT NULL UNIQUE,
    title TEXT NOT NULL DEFAULT '',
    site TEXT NOT NULL DEFAULT '',
    last_fetch_at INTEGER,
    last_error TEXT,
    created_at INTEGER NOT NULL
);

CREATE TABLE rss_follows (
    id INTEGER PRIMARY KEY,
    feed_id INTEGER NOT NULL REFERENCES rss_feeds(id) ON DELETE CASCADE,
    series_id INTEGER REFERENCES series(id) ON DELETE SET NULL,
    title TEXT NOT NULL,
    aliases TEXT NOT NULL DEFAULT '[]',
    resolution TEXT NOT NULL DEFAULT '1080p',
    auto_download INTEGER NOT NULL DEFAULT 0,
    last_episode INTEGER,
    status TEXT NOT NULL DEFAULT 'active',
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);

CREATE INDEX idx_rss_follows_feed ON rss_follows(feed_id);
CREATE INDEX idx_rss_follows_series ON rss_follows(series_id);

CREATE TABLE rss_entries (
    id INTEGER PRIMARY KEY,
    feed_id INTEGER NOT NULL REFERENCES rss_feeds(id) ON DELETE CASCADE,
    guid TEXT NOT NULL,
    title TEXT NOT NULL,
    link TEXT NOT NULL DEFAULT '',
    download_url TEXT NOT NULL DEFAULT '',
    published_at INTEGER NOT NULL DEFAULT 0,
    first_seen_at INTEGER NOT NULL,
    UNIQUE(feed_id, guid)
);

CREATE INDEX idx_rss_entries_feed ON rss_entries(feed_id, published_at DESC);

CREATE TABLE rss_matches (
    id INTEGER PRIMARY KEY,
    entry_id INTEGER NOT NULL REFERENCES rss_entries(id) ON DELETE CASCADE,
    follow_id INTEGER NOT NULL REFERENCES rss_follows(id) ON DELETE CASCADE,
    episode INTEGER,
    confidence INTEGER NOT NULL DEFAULT 0,
    status TEXT NOT NULL DEFAULT 'pending',
    error TEXT,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    UNIQUE(entry_id, follow_id)
);

CREATE INDEX idx_rss_matches_follow ON rss_matches(follow_id, status);
CREATE INDEX idx_rss_matches_entry ON rss_matches(entry_id);
