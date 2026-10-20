//! 应用配置。保存在 `%LOCALAPPDATA%\KanFanOS\config\settings.json`。

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

/// 缩略图宽度限制，避免异常配置拖垮生成速度。
pub const MIN_THUMBNAIL_WIDTH: u32 = 160;
pub const MAX_THUMBNAIL_WIDTH: u32 = 1280;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    /// 番剧根目录，例如 `C:\...\番`。
    pub library_root: Option<String>,
    /// 是否监听目录变化并自动同步。
    #[serde(default = "default_auto_watch")]
    pub auto_watch: bool,
    /// 生成缩略图时的目标宽度。
    #[serde(default = "default_thumbnail_width")]
    pub thumbnail_width: u32,
}

fn default_auto_watch() -> bool {
    true
}

fn default_thumbnail_width() -> u32 {
    480
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            library_root: None,
            auto_watch: default_auto_watch(),
            thumbnail_width: default_thumbnail_width(),
        }
    }
}

pub fn settings_file(data_root: &Path) -> PathBuf {
    data_root.join("config").join("settings.json")
}

/// 读取配置；文件不存在时生成一份默认配置。
pub fn load(data_root: &Path) -> Result<Settings> {
    let path = settings_file(data_root);
    if !path.exists() {
        let settings = Settings::default();
        save(data_root, &settings)?;
        return Ok(settings);
    }

    let content = std::fs::read_to_string(&path)
        .with_context(|| format!("读取配置文件失败: {}", path.display()))?;
    Ok(serde_json::from_str(&content).unwrap_or_default())
}

pub fn save(data_root: &Path, settings: &Settings) -> Result<()> {
    let path = settings_file(data_root);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("创建配置目录失败: {}", parent.display()))?;
    }

    let mut normalized = settings.clone();
    normalized.thumbnail_width = normalized.thumbnail_width.clamp(
        MIN_THUMBNAIL_WIDTH,
        MAX_THUMBNAIL_WIDTH,
    );

    let json = serde_json::to_string_pretty(&normalized)?;
    std::fs::write(&path, json)
        .with_context(|| format!("写入配置文件失败: {}", path.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        time::{SystemTime, UNIX_EPOCH},
    };

    use super::{
        load, save, settings_file, Settings, MAX_THUMBNAIL_WIDTH,
        MIN_THUMBNAIL_WIDTH,
    };

    fn temp_dir(name: &str) -> std::path::PathBuf {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("kanfanos-{name}-{stamp}"));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn legacy_settings_get_new_defaults() {
        let dir = temp_dir("settings-legacy");
        fs::create_dir_all(dir.join("config")).unwrap();
        fs::write(
            settings_file(&dir),
            r#"{"libraryRoot":"X:\\番"}"#,
        )
        .unwrap();

        let settings = load(&dir).unwrap();
        assert_eq!(settings.library_root.as_deref(), Some("X:\\番"));
        assert!(settings.auto_watch);
        assert_eq!(settings.thumbnail_width, 480);

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn thumbnail_width_is_clamped() {
        let dir = temp_dir("settings-clamp");

        save(
            &dir,
            &Settings {
                library_root: None,
                auto_watch: false,
                thumbnail_width: 5000,
            },
        )
        .unwrap();
        assert_eq!(load(&dir).unwrap().thumbnail_width, MAX_THUMBNAIL_WIDTH);

        save(
            &dir,
            &Settings {
                library_root: None,
                auto_watch: true,
                thumbnail_width: 1,
            },
        )
        .unwrap();
        let settings = load(&dir).unwrap();
        assert_eq!(settings.thumbnail_width, MIN_THUMBNAIL_WIDTH);
        assert!(settings.auto_watch);

        let _ = fs::remove_dir_all(dir);
    }
}
