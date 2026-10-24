//! 应用配置。保存在 `%LOCALAPPDATA%\KanFanOS\config\settings.json`。

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

/// 缩略图宽度限制，避免异常配置拖垮生成速度。
pub const MIN_THUMBNAIL_WIDTH: u32 = 160;
pub const MAX_THUMBNAIL_WIDTH: u32 = 1280;

/// 页面切换动画：淡入 + 小幅滑动。
pub const PAGE_TRANSITION_FADE: &str = "fade";
/// 页面切换动画：整页滑动，不淡入淡出。
pub const PAGE_TRANSITION_SLIDE: &str = "slide";

/// 打开番剧时的过渡动画。
pub const SERIES_TRANSITION_RISE: &str = "rise";
pub const SERIES_TRANSITION_ZOOM: &str = "zoom";
pub const SERIES_TRANSITION_SLIDE: &str = "slide";
pub const SERIES_TRANSITION_FLIP: &str = "flip";

/// 主窗口模式。
pub const WINDOW_MODE_WINDOWED: &str = "windowed";
pub const WINDOW_MODE_BORDERLESS: &str = "borderless";

/// qBittorrent WebUI 默认地址（qBittorrent 默认监听 8080 端口）。
pub const DEFAULT_QBITTORRENT_URL: &str = "http://127.0.0.1:8080";
pub const DEFAULT_QBITTORRENT_USERNAME: &str = "admin";

/// 使用随应用分发的 qBittorrent 实例（应用负责启停）。
pub const QBITTORRENT_MODE_SIDECAR: &str = "sidecar";
/// 连接用户自己运行的 qBittorrent WebUI。
pub const QBITTORRENT_MODE_EXTERNAL: &str = "external";

/// qBittorrent WebUI 连接配置。
///
/// 密码以明文保存，和 qBittorrent 自己的配置文件做法一致；
/// 如果不需要远程管理，建议只监听本机地址。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QbittorrentSettings {
    /// `sidecar`（默认，内置实例）或 `external`（外部 WebUI）。
    #[serde(default = "default_qbittorrent_mode")]
    pub mode: String,
    #[serde(default = "default_qbittorrent_url")]
    pub url: String,
    #[serde(default = "default_qbittorrent_username")]
    pub username: String,
    #[serde(default)]
    pub password: String,
    /// 默认下载目录；为空时使用 qBittorrent 自己的默认位置。
    #[serde(default)]
    pub save_path: Option<String>,
}

impl Default for QbittorrentSettings {
    fn default() -> Self {
        Self {
            mode: default_qbittorrent_mode(),
            url: default_qbittorrent_url(),
            username: default_qbittorrent_username(),
            password: String::new(),
            save_path: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    /// 番剧根目录，例如 `C:\...\番`。
    pub library_root: Option<String>,
    /// 是否监听目录变化并自动同步。
    #[serde(default = "default_auto_watch")]
    pub auto_watch: bool,
    /// 是否在 Windows 登录后自动启动。
    #[serde(default)]
    pub auto_start: bool,
    /// 生成缩略图时的目标宽度。
    #[serde(default = "default_thumbnail_width")]
    pub thumbnail_width: u32,
    /// 页面切换动画：`fade` 或 `slide`。
    #[serde(default = "default_page_transition")]
    pub page_transition: String,
    /// 打开番剧进入选集页的动画：`rise` / `zoom` / `slide` / `flip`。
    #[serde(default = "default_series_transition")]
    pub series_transition: String,
    /// 主窗口模式：`windowed` / `borderless`。
    #[serde(default = "default_window_mode")]
    pub window_mode: String,
    /// 内置资源页使用的 qBittorrent WebUI 连接配置。
    #[serde(default)]
    pub qbittorrent: QbittorrentSettings,
}

fn default_auto_watch() -> bool {
    true
}

fn default_thumbnail_width() -> u32 {
    480
}

fn default_page_transition() -> String {
    PAGE_TRANSITION_FADE.to_string()
}

fn default_series_transition() -> String {
    SERIES_TRANSITION_RISE.to_string()
}

fn default_window_mode() -> String {
    WINDOW_MODE_WINDOWED.to_string()
}

fn default_qbittorrent_mode() -> String {
    QBITTORRENT_MODE_SIDECAR.to_string()
}

fn default_qbittorrent_url() -> String {
    DEFAULT_QBITTORRENT_URL.to_string()
}

fn normalize_qbittorrent_mode(value: &str) -> String {
    if value == QBITTORRENT_MODE_EXTERNAL {
        QBITTORRENT_MODE_EXTERNAL.to_string()
    } else {
        QBITTORRENT_MODE_SIDECAR.to_string()
    }
}

fn default_qbittorrent_username() -> String {
    DEFAULT_QBITTORRENT_USERNAME.to_string()
}

/// 只接受已知值，避免配置文件被改坏后前端拿到无效样式。
fn normalize_page_transition(value: &str) -> String {
    if value == PAGE_TRANSITION_SLIDE {
        PAGE_TRANSITION_SLIDE.to_string()
    } else {
        PAGE_TRANSITION_FADE.to_string()
    }
}

fn normalize_series_transition(value: &str) -> String {
    match value {
        SERIES_TRANSITION_ZOOM
        | SERIES_TRANSITION_SLIDE
        | SERIES_TRANSITION_FLIP => value.to_string(),
        _ => SERIES_TRANSITION_RISE.to_string(),
    }
}

fn normalize_window_mode(value: &str) -> String {
    if value == WINDOW_MODE_BORDERLESS {
        WINDOW_MODE_BORDERLESS.to_string()
    } else {
        WINDOW_MODE_WINDOWED.to_string()
    }
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            library_root: None,
            auto_watch: default_auto_watch(),
            auto_start: false,
            thumbnail_width: default_thumbnail_width(),
            page_transition: default_page_transition(),
            series_transition: default_series_transition(),
            window_mode: default_window_mode(),
            qbittorrent: QbittorrentSettings::default(),
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
    let mut settings: Settings = serde_json::from_str(&content).unwrap_or_default();
    // 兼容旧配置或手动改坏的值。
    settings.page_transition = normalize_page_transition(&settings.page_transition);
    settings.series_transition = normalize_series_transition(&settings.series_transition);
    settings.window_mode = normalize_window_mode(&settings.window_mode);
    settings.qbittorrent.mode = normalize_qbittorrent_mode(&settings.qbittorrent.mode);
    Ok(settings)
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
    normalized.page_transition = normalize_page_transition(&normalized.page_transition);
    normalized.series_transition = normalize_series_transition(&normalized.series_transition);
    normalized.window_mode = normalize_window_mode(&normalized.window_mode);
    normalized.qbittorrent.mode = normalize_qbittorrent_mode(&normalized.qbittorrent.mode);

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
        load, save, settings_file, QbittorrentSettings, Settings, MAX_THUMBNAIL_WIDTH,
        MIN_THUMBNAIL_WIDTH, PAGE_TRANSITION_FADE, PAGE_TRANSITION_SLIDE,
        SERIES_TRANSITION_FLIP, SERIES_TRANSITION_RISE, WINDOW_MODE_BORDERLESS,
        WINDOW_MODE_WINDOWED,
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
        assert!(!settings.auto_start);
        assert_eq!(settings.thumbnail_width, 480);
        assert_eq!(settings.page_transition, PAGE_TRANSITION_FADE);
        assert_eq!(settings.series_transition, SERIES_TRANSITION_RISE);
        assert_eq!(settings.window_mode, WINDOW_MODE_WINDOWED);

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn window_mode_is_normalized() {
        let dir = temp_dir("settings-window-mode");

        save(
            &dir,
            &Settings {
                library_root: None,
                auto_start: false,
                auto_watch: true,
                thumbnail_width: 480,
                page_transition: PAGE_TRANSITION_FADE.to_string(),
                series_transition: SERIES_TRANSITION_RISE.to_string(),
                window_mode: WINDOW_MODE_BORDERLESS.to_string(),
                qbittorrent: QbittorrentSettings::default(),
            },
        )
        .unwrap();
        assert_eq!(load(&dir).unwrap().window_mode, WINDOW_MODE_BORDERLESS);

        save(
            &dir,
            &Settings {
                library_root: None,
                auto_start: false,
                auto_watch: true,
                thumbnail_width: 480,
                page_transition: PAGE_TRANSITION_FADE.to_string(),
                series_transition: SERIES_TRANSITION_RISE.to_string(),
                window_mode: "bogus".to_string(),
                qbittorrent: QbittorrentSettings::default(),
            },
        )
        .unwrap();
        assert_eq!(load(&dir).unwrap().window_mode, WINDOW_MODE_WINDOWED);

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn series_transition_is_normalized() {
        let dir = temp_dir("settings-series-transition");

        save(
            &dir,
            &Settings {
                library_root: None,
                auto_start: false,
                auto_watch: true,
                thumbnail_width: 480,
                page_transition: PAGE_TRANSITION_FADE.to_string(),
                series_transition: SERIES_TRANSITION_FLIP.to_string(),
                window_mode: WINDOW_MODE_WINDOWED.to_string(),
                qbittorrent: QbittorrentSettings::default(),
            },
        )
        .unwrap();
        assert_eq!(load(&dir).unwrap().series_transition, SERIES_TRANSITION_FLIP);

        save(
            &dir,
            &Settings {
                library_root: None,
                auto_start: false,
                auto_watch: true,
                thumbnail_width: 480,
                page_transition: PAGE_TRANSITION_FADE.to_string(),
                series_transition: "bogus".to_string(),
                window_mode: WINDOW_MODE_WINDOWED.to_string(),
                qbittorrent: QbittorrentSettings::default(),
            },
        )
        .unwrap();
        assert_eq!(load(&dir).unwrap().series_transition, SERIES_TRANSITION_RISE);

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn page_transition_is_normalized() {
        let dir = temp_dir("settings-transition");

        save(
            &dir,
            &Settings {
                library_root: None,
                auto_start: false,
                auto_watch: true,
                thumbnail_width: 480,
                page_transition: PAGE_TRANSITION_SLIDE.to_string(),
                series_transition: SERIES_TRANSITION_RISE.to_string(),
                window_mode: WINDOW_MODE_WINDOWED.to_string(),
                qbittorrent: QbittorrentSettings::default(),
            },
        )
        .unwrap();
        assert_eq!(load(&dir).unwrap().page_transition, PAGE_TRANSITION_SLIDE);

        save(
            &dir,
            &Settings {
                library_root: None,
                auto_start: false,
                auto_watch: true,
                thumbnail_width: 480,
                page_transition: "unknown".to_string(),
                series_transition: SERIES_TRANSITION_RISE.to_string(),
                window_mode: WINDOW_MODE_WINDOWED.to_string(),
                qbittorrent: QbittorrentSettings::default(),
            },
        )
        .unwrap();
        assert_eq!(load(&dir).unwrap().page_transition, PAGE_TRANSITION_FADE);

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn thumbnail_width_is_clamped() {
        let dir = temp_dir("settings-clamp");

        save(
            &dir,
            &Settings {
                library_root: None,
                auto_start: false,
                auto_watch: false,
                thumbnail_width: 5000,
                page_transition: PAGE_TRANSITION_FADE.to_string(),
                series_transition: SERIES_TRANSITION_RISE.to_string(),
                window_mode: WINDOW_MODE_WINDOWED.to_string(),
                qbittorrent: QbittorrentSettings::default(),
            },
        )
        .unwrap();
        assert_eq!(load(&dir).unwrap().thumbnail_width, MAX_THUMBNAIL_WIDTH);

        save(
            &dir,
            &Settings {
                library_root: None,
                auto_start: false,
                auto_watch: true,
                thumbnail_width: 1,
                page_transition: PAGE_TRANSITION_FADE.to_string(),
                series_transition: SERIES_TRANSITION_RISE.to_string(),
                window_mode: WINDOW_MODE_WINDOWED.to_string(),
                qbittorrent: QbittorrentSettings::default(),
            },
        )
        .unwrap();
        let settings = load(&dir).unwrap();
        assert_eq!(settings.thumbnail_width, MIN_THUMBNAIL_WIDTH);
        assert!(settings.auto_watch);

        let _ = fs::remove_dir_all(dir);
    }
}
