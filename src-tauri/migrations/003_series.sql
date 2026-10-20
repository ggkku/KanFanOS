-- 番剧（series）：一个番剧对应一个文件夹，视频是它的剧集。
-- category 保存从目录名解析出的档期（如 "2026年7月"），无法解析的归入 "未知时间"。
CREATE TABLE series (
    id INTEGER PRIMARY KEY,
    library_id INTEGER NOT NULL REFERENCES libraries(id) ON DELETE CASCADE,
    category TEXT NOT NULL,
    name TEXT NOT NULL,
    path TEXT NOT NULL UNIQUE,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);

CREATE INDEX idx_series_library ON series(library_id);
CREATE INDEX idx_series_category ON series(library_id, category);

ALTER TABLE videos ADD COLUMN series_id INTEGER REFERENCES series(id) ON DELETE CASCADE;
CREATE INDEX idx_videos_series ON videos(series_id);
