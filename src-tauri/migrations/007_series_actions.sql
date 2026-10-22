-- 番剧手动操作与二级选集。
-- watch_count: 用户手动维护的「看过遍数」，不会自动增长。
-- kind: main = 番剧根目录的视频；extra = 子目录里的 CM/PV/特典等。
ALTER TABLE series ADD COLUMN watch_count INTEGER NOT NULL DEFAULT 0;
ALTER TABLE videos ADD COLUMN kind TEXT NOT NULL DEFAULT 'main';

CREATE INDEX idx_videos_series_kind ON videos(series_id, kind);
