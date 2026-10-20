-- 番剧目录内的 kanfanos.toml 会同步到这些字段。
-- aliases 用换行分隔，参与关键词搜索；cover_path 是封面图片的绝对路径。
ALTER TABLE series ADD COLUMN aliases TEXT NOT NULL DEFAULT '';
ALTER TABLE series ADD COLUMN cover_path TEXT;
ALTER TABLE series ADD COLUMN cover_version INTEGER;
