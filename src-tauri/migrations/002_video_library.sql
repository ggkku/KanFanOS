-- 建立视频与媒体目录的关联。
-- 用 library_id 把扫描范围限定在某个根目录，避免按路径前缀匹配。
-- path 上的 UNIQUE 约束负责跨目录去重：同一文件只保留一条记录。
ALTER TABLE videos ADD COLUMN library_id INTEGER REFERENCES libraries(id) ON DELETE CASCADE;

CREATE INDEX idx_videos_library_id ON videos(library_id);
