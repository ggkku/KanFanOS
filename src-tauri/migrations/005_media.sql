-- 元数据与缩略图的后台处理状态。
-- pending: 等待处理；processing: 已领取；ready: 完成；failed: 重试耗尽。
ALTER TABLE videos ADD COLUMN media_status TEXT NOT NULL DEFAULT 'pending';
ALTER TABLE videos ADD COLUMN media_attempts INTEGER NOT NULL DEFAULT 0;
ALTER TABLE videos ADD COLUMN media_error TEXT;

CREATE INDEX idx_videos_media_status ON videos(library_id, media_status);
