//! 版本化迁移脚本。
//!
//! 数组下标 + 1 即数据库版本号，记录在 `PRAGMA user_version` 中。
//! 新增迁移时只能在末尾追加，不能修改已发布的脚本。

pub const MIGRATIONS: &[&str] = &[
    include_str!("../../migrations/001_initial.sql"),
    include_str!("../../migrations/002_video_library.sql"),
    include_str!("../../migrations/003_series.sql"),
    include_str!("../../migrations/004_categories.sql"),
    include_str!("../../migrations/005_media.sql"),
    include_str!("../../migrations/006_series_meta.sql"),
];
