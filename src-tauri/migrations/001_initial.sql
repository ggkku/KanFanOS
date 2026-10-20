-- KanFanOS 初始数据库结构
-- 数据库只是媒体库的索引，视频原件始终保留在用户原有目录中。

CREATE TABLE videos (
    id INTEGER PRIMARY KEY,
    path TEXT NOT NULL UNIQUE,
    filename TEXT NOT NULL,
    extension TEXT,
    file_size INTEGER NOT NULL DEFAULT 0,
    modified_at INTEGER,
    duration_ms INTEGER,
    width INTEGER,
    height INTEGER,
    video_codec TEXT,
    audio_codec TEXT,
    frame_rate REAL,
    thumbnail_key TEXT,
    last_scanned_at INTEGER,
    status TEXT NOT NULL DEFAULT 'active',
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);

CREATE TABLE libraries (
    id INTEGER PRIMARY KEY,
    root_path TEXT NOT NULL UNIQUE,
    display_name TEXT,
    enabled INTEGER NOT NULL DEFAULT 1,
    last_scan_at INTEGER,
    created_at INTEGER NOT NULL
);

CREATE TABLE tags (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL UNIQUE,
    color TEXT,
    created_at INTEGER NOT NULL
);

CREATE TABLE video_tags (
    video_id INTEGER NOT NULL,
    tag_id INTEGER NOT NULL,
    PRIMARY KEY (video_id, tag_id),
    FOREIGN KEY (video_id) REFERENCES videos(id) ON DELETE CASCADE,
    FOREIGN KEY (tag_id) REFERENCES tags(id) ON DELETE CASCADE
);

CREATE TABLE favorites (
    video_id INTEGER PRIMARY KEY,
    created_at INTEGER NOT NULL,
    FOREIGN KEY (video_id) REFERENCES videos(id) ON DELETE CASCADE
);

CREATE TABLE watch_history (
    video_id INTEGER PRIMARY KEY,
    last_opened_at INTEGER NOT NULL,
    last_position_ms INTEGER NOT NULL DEFAULT 0,
    completed INTEGER NOT NULL DEFAULT 0,
    FOREIGN KEY (video_id) REFERENCES videos(id) ON DELETE CASCADE
);

CREATE INDEX idx_videos_filename ON videos(filename);
CREATE INDEX idx_videos_modified_at ON videos(modified_at);
CREATE INDEX idx_videos_duration ON videos(duration_ms);
CREATE INDEX idx_videos_resolution ON videos(width, height);
CREATE INDEX idx_videos_status ON videos(status);
CREATE INDEX idx_video_tags_tag ON video_tags(tag_id, video_id);
