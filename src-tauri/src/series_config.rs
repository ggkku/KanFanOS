//! 番剧文件夹内的 `kanfanos.toml`。
//!
//! 首次扫描该文件夹时生成默认配置；之后扫描只读取，不覆盖用户的修改。

use std::{
    fs,
    path::Path,
    time::UNIX_EPOCH,
};

use serde::{Deserialize, Serialize};

pub const CONFIG_FILE: &str = "kanfanos.toml";

const IMAGE_EXTENSIONS: &[&str] = &[
    "jpg", "jpeg", "png", "webp", "gif", "avif", "bmp",
];

/// 没有显式配置封面时，自动查找这些文件名。
const COVER_STEMS: &[&str] = &["cover", "poster", "folder"];

/// 配置文件里表示「未评分」的值。
pub const UNRATED: i64 = -1;

/// 选集来源：根目录视频自动成为选集（旧行为）。
pub const EPISODE_MODE_AUTO: &str = "auto";
/// 选集来源：只使用配置里的 `episodes` 列表。
pub const EPISODE_MODE_MANUAL: &str = "manual";

/// 标准配置必须包含的字段；旧文件缺少任意一项就重写补齐。
const CONFIG_KEYS: &[&str] = &[
    "name",
    "aliases",
    "cover",
    "rating",
    "awards",
    "previous",
    "next",
    "episode_mode",
    "episodes",
];

/// 单个配置文件修复结果。
#[derive(Debug, Default, Clone, Copy)]
pub struct RepairOutcome {
    pub created: bool,
    pub updated: bool,
    pub names_cleared: bool,
    pub invalid: bool,
}

fn unrated() -> i64 {
    UNRATED
}

fn default_episode_mode() -> String {
    EPISODE_MODE_AUTO.to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SeriesConfig {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub aliases: Vec<String>,
    #[serde(default)]
    pub cover: String,
    /// 个人评分 0-10；-1 表示未评分。
    #[serde(default = "unrated")]
    pub rating: i64,
    /// 用户自由输入的奖项名称。
    #[serde(default)]
    pub awards: Vec<String>,
    /// 上一季番剧文件夹的绝对路径；空字符串表示没有上一季。
    #[serde(default)]
    pub previous: String,
    /// 下一季番剧文件夹的绝对路径；空字符串表示没有下一季。
    #[serde(default)]
    pub next: String,
    /// 选集来源：`auto`（根目录视频自动成为选集）或 `manual`（只用 episodes 列表）。
    #[serde(default = "default_episode_mode")]
    pub episode_mode: String,
    /// manual 模式下作为选集的视频，相对番剧目录，使用 `/` 分隔。
    #[serde(default)]
    pub episodes: Vec<String>,
}

impl Default for SeriesConfig {
    fn default() -> Self {
        Self {
            name: String::new(),
            aliases: Vec::new(),
            cover: String::new(),
            rating: UNRATED,
            awards: Vec::new(),
            previous: String::new(),
            next: String::new(),
            episode_mode: default_episode_mode(),
            episodes: Vec::new(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct ResolvedSeriesConfig {
    pub name: String,
    pub aliases: Vec<String>,
    pub rating: Option<i64>,
    pub cover_path: Option<String>,
    pub cover_version: Option<i64>,
    pub episode_mode: String,
    pub episodes: Vec<String>,
}

/// 读取（必要时生成）配置，并解析出最终用于数据库的值。
pub fn resolve(series_dir: &Path, folder_name: &str) -> ResolvedSeriesConfig {
    let config = load_or_create(series_dir);
    build(series_dir, folder_name, &config)
}

/// 只读取配置，不创建文件；供配置弹窗读取当前值。
pub fn load(series_dir: &Path) -> SeriesConfig {
    let path = series_dir.join(CONFIG_FILE);

    match fs::read_to_string(&path) {
        Ok(content) => toml::from_str(&content).unwrap_or_else(|error| {
            eprintln!("解析番剧配置失败 {}: {error}", path.display());
            SeriesConfig::default()
        }),
        Err(error) => {
            if error.kind() != std::io::ErrorKind::NotFound {
                eprintln!("读取番剧配置失败 {}: {error}", path.display());
            }
            SeriesConfig::default()
        }
    }
}

/// 写回配置。配置弹窗保存名称、别名和季链接时使用。
pub fn save(series_dir: &Path, config: &SeriesConfig) -> std::io::Result<()> {
    fs::write(series_dir.join(CONFIG_FILE), render(config))
}

/// 检查并修复单个配置文件：
/// - 不存在 → 生成默认配置（name 留空）；
/// - 名称与文件夹名相同 → 清空名称；
/// - 缺少标准字段 → 用当前值重写补齐；
/// - 解析失败 → 先备份为 kanfanos.toml.bak，再重建默认配置。
///
/// 需要修改文件时，原始文件会保留为 kanfanos.toml.bak（已存在则不覆盖）。
pub fn repair_file(series_dir: &Path, folder_name: &str) -> std::io::Result<RepairOutcome> {
    let path = series_dir.join(CONFIG_FILE);
    let mut outcome = RepairOutcome::default();

    if !path.exists() {
        save(series_dir, &SeriesConfig::default())?;
        outcome.created = true;
        return Ok(outcome);
    }

    let content = fs::read_to_string(&path)?;
    let mut config;
    let mut needs_write = false;

    match toml::from_str::<SeriesConfig>(&content) {
        Ok(parsed) => {
            config = parsed;

            if !config.name.trim().is_empty() && config.name.trim() == folder_name {
                config.name.clear();
                outcome.names_cleared = true;
                needs_write = true;
            }

            let missing = content
                .parse::<toml::Table>()
                .map(|table| {
                    CONFIG_KEYS
                        .iter()
                        .any(|key| !table.contains_key(*key))
                })
                .unwrap_or(true);
            if missing {
                needs_write = true;
            }
        }
        Err(error) => {
            eprintln!(
                "解析番剧配置失败，将重建 {}: {error}",
                path.display()
            );
            backup_file(&path);
            config = SeriesConfig::default();
            outcome.invalid = true;
            needs_write = true;
        }
    }

    if needs_write {
        if !outcome.invalid {
            backup_file(&path);
        }
        save(series_dir, &config)?;
        if !outcome.invalid {
            outcome.updated = true;
        }
    }

    Ok(outcome)
}

fn backup_file(path: &Path) {
    let backup = path.with_file_name(format!("{CONFIG_FILE}.bak"));
    if backup.exists() {
        return;
    }

    if let Err(error) = fs::copy(path, &backup) {
        eprintln!("备份番剧配置失败 {}: {error}", backup.display());
    }
}

/// 名称留空时回退到文件夹名。
pub fn resolved_name(config: &SeriesConfig, folder_name: &str) -> String {
    let trimmed = config.name.trim();
    if trimmed.is_empty() {
        folder_name.to_string()
    } else {
        trimmed.to_string()
    }
}

/// 去掉首尾空白和空别名。
pub fn clean_aliases(aliases: &[String]) -> Vec<String> {
    clean_list(aliases)
}

/// 去掉首尾空白、空项和重复项，保持原有顺序。
pub fn clean_list(values: &[String]) -> Vec<String> {
    let mut cleaned: Vec<String> = Vec::new();

    for value in values {
        let value = value.trim();
        if value.is_empty() || cleaned.iter().any(|item| item == value) {
            continue;
        }
        cleaned.push(value.to_string());
    }

    cleaned
}

/// 0-10 视为有效评分，其余（包括 -1）视为未评分。
pub fn resolved_rating(config: &SeriesConfig) -> Option<i64> {
    (0..=10).contains(&config.rating).then_some(config.rating)
}

/// 只接受已知的选集模式。
pub fn normalize_episode_mode(value: &str) -> String {
    if value.trim() == EPISODE_MODE_MANUAL {
        EPISODE_MODE_MANUAL.to_string()
    } else {
        EPISODE_MODE_AUTO.to_string()
    }
}

/// 规范化相对路径：统一 `/`、去掉 `./` 前缀和首尾空白。
pub fn normalize_relative(path: &str) -> String {
    let mut value = path.trim().replace('\\', "/");
    while let Some(rest) = value.strip_prefix("./") {
        value = rest.to_string();
    }
    value.trim_start_matches('/').to_string()
}

/// 去掉空项和重复项（Windows 下路径大小写不敏感）。
pub fn clean_episodes(paths: &[String]) -> Vec<String> {
    let mut cleaned: Vec<String> = Vec::new();

    for path in paths {
        let normalized = normalize_relative(path);
        if normalized.is_empty()
            || cleaned
                .iter()
                .any(|item| item.eq_ignore_ascii_case(&normalized))
        {
            continue;
        }
        cleaned.push(normalized);
    }

    cleaned
}

/// 视频文件相对番剧目录的路径，统一成 `/` 分隔。
pub fn relative_path(series_dir: &Path, file: &Path) -> String {
    file.strip_prefix(series_dir)
        .map(|path| normalize_relative(&path.to_string_lossy()))
        .unwrap_or_else(|_| normalize_relative(&file.to_string_lossy()))
}

pub fn is_selected_episode(episodes: &[String], relative: &str) -> bool {
    let relative = normalize_relative(relative);
    episodes
        .iter()
        .any(|path| path.eq_ignore_ascii_case(&relative))
}

/// 根据配置决定视频归入选集（`main`）还是其它视频（`extra`）。
pub fn classify_kind(
    episode_mode: &str,
    episodes: &[String],
    relative: &str,
    is_root_file: bool,
) -> &'static str {
    if normalize_episode_mode(episode_mode) == EPISODE_MODE_MANUAL {
        if is_selected_episode(episodes, relative) {
            "main"
        } else {
            "extra"
        }
    } else if is_root_file {
        "main"
    } else {
        "extra"
    }
}

fn build(
    series_dir: &Path,
    folder_name: &str,
    config: &SeriesConfig,
) -> ResolvedSeriesConfig {
    let name = resolved_name(config, folder_name);
    let aliases = clean_aliases(&config.aliases);
    let rating = resolved_rating(config);

    let cover_path = resolve_cover(series_dir, config.cover.trim());
    let cover_version = cover_path
        .as_deref()
        .and_then(|path| cover_version(Path::new(path)));

    ResolvedSeriesConfig {
        name,
        aliases,
        rating,
        cover_path,
        cover_version,
        episode_mode: normalize_episode_mode(&config.episode_mode),
        episodes: clean_episodes(&config.episodes),
    }
}

fn load_or_create(series_dir: &Path) -> SeriesConfig {
    let path = series_dir.join(CONFIG_FILE);

    if !path.exists() {
        // 名称默认留空：显示时直接使用文件夹名，文件夹改名后显示名也跟着变。
        let config = SeriesConfig::default();

        if let Err(error) = fs::write(&path, render(&config)) {
            eprintln!("写入番剧配置失败 {}: {error}", path.display());
        }

        return config;
    }

    load(series_dir)
}

/// 统一生成配置文件，保留说明注释。
fn render(config: &SeriesConfig) -> String {
    let aliases = render_list(&config.aliases);
    let awards = render_list(&config.awards);

    format!(
        "# KanFanOS 番剧配置（可手动编辑，扫描不会覆盖本文件）\n\
         # 显示名称；留空则使用文件夹名\n\
         name = \"{}\"\n\
         \n\
         # 搜索别名，可以写多个，例如 [\"英文名\", \"简称\"]\n\
         aliases = [{}]\n\
         \n\
         # 封面图片，相对本文件夹的路径；留空时自动查找 cover/poster 图片，\n\
         # 找不到就使用文件夹内排序最靠前的一张图片\n\
         cover = \"{}\"\n\
         \n\
         # 个人评分 0-10；-1 表示未评分\n\
         rating = {}\n\
         \n\
         # 获得的奖项，自由输入，可以写多个\n\
         awards = [{}]\n\
         \n\
         # 上一季 / 下一季番剧文件夹的绝对路径；留空表示没有\n\
         previous = \"{}\"\n\
         next = \"{}\"\n\
         \n\
         # 选集来源：auto = 根目录视频自动成为选集；manual = 只使用下面的 episodes 列表\n\
         episode_mode = \"{}\"\n\
         \n\
         # manual 模式下作为选集的视频，相对本文件夹，用 / 分隔；\n\
         # 未列出的视频会进入「其它视频」，文件本身不会被移动或删除\n\
         episodes = [{}]\n",
        escape_toml(&config.name),
        aliases,
        escape_toml(&config.cover),
        config.rating,
        awards,
        escape_toml(&config.previous),
        escape_toml(&config.next),
        normalize_episode_mode(&config.episode_mode),
        render_list(&clean_episodes(&config.episodes)),
    )
}

fn render_list(values: &[String]) -> String {
    values
        .iter()
        .map(|value| format!("\"{}\"", escape_toml(value)))
        .collect::<Vec<_>>()
        .join(", ")
}

fn resolve_cover(series_dir: &Path, configured: &str) -> Option<String> {
    if !configured.is_empty() {
        let candidate = series_dir.join(configured);
        if candidate.is_file() {
            return Some(candidate.to_string_lossy().into_owned());
        }
        eprintln!("封面文件不存在: {}", candidate.display());
    }

    // Windows 文件系统大小写不敏感，直接按常见文件名查找即可。
    for stem in COVER_STEMS {
        for extension in IMAGE_EXTENSIONS {
            let candidate = series_dir.join(format!("{stem}.{extension}"));
            if candidate.is_file() {
                return Some(candidate.to_string_lossy().into_owned());
            }
        }
    }

    // 没有约定名称时，使用文件夹内排序最靠前的一张图片。
    first_image(series_dir)
}

fn first_image(dir: &Path) -> Option<String> {
    let mut images: Vec<_> = fs::read_dir(dir)
        .ok()?
        .flatten()
        .filter(|entry| entry.file_type().map(|kind| kind.is_file()).unwrap_or(false))
        .map(|entry| entry.path())
        .filter(|path| {
            path.extension()
                .and_then(|value| value.to_str())
                .map(|extension| {
                    IMAGE_EXTENSIONS
                        .iter()
                        .any(|candidate| candidate.eq_ignore_ascii_case(extension))
                })
                .unwrap_or(false)
        })
        .collect();

    images.sort_by_key(|path| {
        path.file_name()
            .map(|name| name.to_string_lossy().to_lowercase())
            .unwrap_or_default()
    });

    images
        .into_iter()
        .next()
        .map(|path| path.to_string_lossy().into_owned())
}

fn cover_version(path: &Path) -> Option<i64> {
    fs::metadata(path)
        .ok()?
        .modified()
        .ok()?
        .duration_since(UNIX_EPOCH)
        .ok()
        .map(|duration| duration.as_millis() as i64)
}

fn escape_toml(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        time::{SystemTime, UNIX_EPOCH},
    };

    use super::{load, repair_file, resolve, save, CONFIG_FILE};

    fn temp_series(name: &str) -> std::path::PathBuf {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("kanfanos-series-{name}-{stamp}"));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn creates_default_config_on_first_scan() {
        let dir = temp_series("create");
        let resolved = resolve(&dir, "进击的巨人");

        assert_eq!(resolved.name, "进击的巨人");
        assert!(resolved.aliases.is_empty());
        assert_eq!(resolved.rating, None);
        assert!(resolved.cover_path.is_none());
        assert!(dir.join(CONFIG_FILE).is_file());

        // 新配置的名称留空，页面直接回退到文件夹名。
        let content = fs::read_to_string(dir.join(CONFIG_FILE)).unwrap();
        assert!(content.contains("name = \"\""));

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn reads_manual_edits_and_finds_cover() {
        let dir = temp_series("read");
        fs::write(
            dir.join(CONFIG_FILE),
            "name = \"显示名称\"\naliases = [\"别名A\", \"Alias B\"]\ncover = \"cover.png\"\n",
        )
        .unwrap();
        fs::write(dir.join("cover.png"), b"fake").unwrap();

        let resolved = resolve(&dir, "文件夹名");
        assert_eq!(resolved.name, "显示名称");
        assert_eq!(resolved.aliases, vec!["别名A", "Alias B"]);
        assert!(resolved
            .cover_path
            .as_deref()
            .unwrap()
            .ends_with("cover.png"));
        assert!(resolved.cover_version.is_some());

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn auto_detects_poster_when_cover_is_empty() {
        let dir = temp_series("auto");
        fs::write(dir.join(CONFIG_FILE), "name = \"\"\n").unwrap();
        fs::write(dir.join("poster.jpg"), b"fake").unwrap();

        let resolved = resolve(&dir, "文件夹名");
        assert_eq!(resolved.name, "文件夹名");
        assert!(resolved.cover_path.as_deref().unwrap().ends_with("poster.jpg"));

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn falls_back_to_first_image() {
        let dir = temp_series("first-image");
        fs::write(dir.join(CONFIG_FILE), "name = \"\"\n").unwrap();
        fs::write(dir.join("b.jpg"), b"fake").unwrap();
        fs::write(dir.join("a.png"), b"fake").unwrap();
        fs::write(dir.join("notes.txt"), b"not an image").unwrap();

        let resolved = resolve(&dir, "文件夹名");
        assert!(resolved.cover_path.as_deref().unwrap().ends_with("a.png"));

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn repair_creates_empty_name_for_new_config() {
        let dir = temp_series("repair-create");
        let outcome = repair_file(&dir, "文件夹名").unwrap();

        assert!(outcome.created);
        assert!(!outcome.updated);

        let content = fs::read_to_string(dir.join(CONFIG_FILE)).unwrap();
        assert!(content.contains("name = \"\""));
        assert_eq!(load(&dir).name, "");
        assert!(!dir.join(format!("{CONFIG_FILE}.bak")).exists());

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn repair_clears_folder_name_and_fills_missing_fields() {
        let dir = temp_series("repair-fields");
        let original = "name = \"文件夹名\"\naliases = [\"旧别名\"]\ncover = \"\"\n";
        fs::write(dir.join(CONFIG_FILE), original).unwrap();

        let outcome = repair_file(&dir, "文件夹名").unwrap();
        assert!(outcome.updated);
        assert!(outcome.names_cleared);
        assert!(!outcome.invalid);

        let config = load(&dir);
        assert_eq!(config.name, "");
        assert_eq!(config.aliases, vec!["旧别名"]);

        let content = fs::read_to_string(dir.join(CONFIG_FILE)).unwrap();
        assert!(content.contains("rating = -1"));
        assert!(content.contains("awards = []"));
        assert!(content.contains("previous = \"\""));
        assert!(content.contains("next = \"\""));

        let backup = fs::read_to_string(dir.join(format!("{CONFIG_FILE}.bak"))).unwrap();
        assert_eq!(backup, original);

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn repair_rebuilds_invalid_config_after_backup() {
        let dir = temp_series("repair-invalid");
        let broken = "name = [broken";
        fs::write(dir.join(CONFIG_FILE), broken).unwrap();

        let outcome = repair_file(&dir, "文件夹名").unwrap();
        assert!(outcome.invalid);
        assert!(!outcome.updated);

        let backup = fs::read_to_string(dir.join(format!("{CONFIG_FILE}.bak"))).unwrap();
        assert_eq!(backup, broken);
        assert_eq!(load(&dir).name, "");
        assert!(
            fs::read_to_string(dir.join(CONFIG_FILE))
                .unwrap()
                .contains("name = \"\"")
        );

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn repair_keeps_custom_name_and_does_not_rewrite() {
        let dir = temp_series("repair-keep");
        let config = super::SeriesConfig {
            name: "自定义名称".to_string(),
            ..Default::default()
        };
        save(&dir, &config).unwrap();
        let before = fs::read_to_string(dir.join(CONFIG_FILE)).unwrap();

        let outcome = repair_file(&dir, "文件夹名").unwrap();
        assert!(!outcome.created);
        assert!(!outcome.updated);
        assert!(!outcome.names_cleared);
        assert!(!outcome.invalid);

        assert_eq!(
            fs::read_to_string(dir.join(CONFIG_FILE)).unwrap(),
            before
        );
        assert!(!dir.join(format!("{CONFIG_FILE}.bak")).exists());

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn saves_editable_fields_and_reads_them_back() {
        let dir = temp_series("save");
        let config = super::SeriesConfig {
            name: "新名称".to_string(),
            aliases: vec!["别名甲".to_string(), "Alias B".to_string()],
            cover: "cover.jpg".to_string(),
            rating: 8,
            awards: vec!["最佳动画".to_string(), "年度推荐".to_string()],
            previous: "X:\\上一季".to_string(),
            next: "X:\\下一季".to_string(),
            episode_mode: super::EPISODE_MODE_MANUAL.to_string(),
            episodes: vec!["第01集.mp4".to_string()],
        };

        save(&dir, &config).unwrap();
        let loaded = load(&dir);

        assert_eq!(loaded.name, "新名称");
        assert_eq!(loaded.aliases, vec!["别名甲", "Alias B"]);
        assert_eq!(loaded.cover, "cover.jpg");
        assert_eq!(loaded.rating, 8);
        assert_eq!(loaded.awards, vec!["最佳动画", "年度推荐"]);
        assert_eq!(loaded.previous, "X:\\上一季");
        assert_eq!(loaded.next, "X:\\下一季");
        assert_eq!(loaded.episode_mode, super::EPISODE_MODE_MANUAL);
        assert_eq!(loaded.episodes, vec!["第01集.mp4"]);

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn episode_mode_defaults_to_auto() {
        let dir = temp_series("episode-mode");
        fs::write(dir.join(CONFIG_FILE), "name = \"\"\n").unwrap();
        let config = load(&dir);
        assert_eq!(config.episode_mode, super::EPISODE_MODE_AUTO);
        assert!(config.episodes.is_empty());
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn manual_mode_classifies_selected_paths() {
        use super::{classify_kind, clean_episodes, EPISODE_MODE_AUTO, EPISODE_MODE_MANUAL};

        let episodes = clean_episodes(&[
            "./第01集.mp4".to_string(),
            "SP\\特典.mkv".to_string(),
            "sp/特典.mkv".to_string(),
        ]);
        assert_eq!(episodes, vec!["第01集.mp4", "SP/特典.mkv"]);

        assert_eq!(
            classify_kind(EPISODE_MODE_MANUAL, &episodes, "第01集.mp4", true),
            "main"
        );
        assert_eq!(
            classify_kind(EPISODE_MODE_MANUAL, &episodes, "SP/特典.mkv", false),
            "main"
        );
        assert_eq!(
            classify_kind(EPISODE_MODE_MANUAL, &episodes, "下载的种子.mp4", true),
            "extra"
        );

        // auto 模式保持旧行为。
        assert_eq!(
            classify_kind(EPISODE_MODE_AUTO, &episodes, "下载的种子.mp4", true),
            "main"
        );
        assert_eq!(
            classify_kind(EPISODE_MODE_AUTO, &episodes, "SP/特典.mkv", false),
            "extra"
        );
    }

    #[test]
    fn existing_config_is_not_overwritten() {
        let dir = temp_series("keep");
        let custom = "# 我的配置\nname = \"自定义\"\n";
        fs::write(dir.join(CONFIG_FILE), custom).unwrap();

        let _ = resolve(&dir, "文件夹名");

        assert_eq!(
            fs::read_to_string(dir.join(CONFIG_FILE)).unwrap(),
            custom
        );

        let _ = fs::remove_dir_all(dir);
    }
}
