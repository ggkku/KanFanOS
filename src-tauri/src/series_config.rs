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

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SeriesConfig {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub aliases: Vec<String>,
    #[serde(default)]
    pub cover: String,
}

#[derive(Debug, Clone)]
pub struct ResolvedSeriesConfig {
    pub name: String,
    pub aliases: Vec<String>,
    pub cover_path: Option<String>,
    pub cover_version: Option<i64>,
}

/// 读取（必要时生成）配置，并解析出最终用于数据库的值。
pub fn resolve(series_dir: &Path, folder_name: &str) -> ResolvedSeriesConfig {
    let config = load_or_create(series_dir, folder_name);

    let name = {
        let trimmed = config.name.trim();
        if trimmed.is_empty() {
            folder_name.to_string()
        } else {
            trimmed.to_string()
        }
    };

    let aliases: Vec<String> = config
        .aliases
        .into_iter()
        .map(|alias| alias.trim().to_string())
        .filter(|alias| !alias.is_empty())
        .collect();

    let cover_path = resolve_cover(series_dir, config.cover.trim());
    let cover_version = cover_path
        .as_deref()
        .and_then(|path| cover_version(Path::new(path)));

    ResolvedSeriesConfig {
        name,
        aliases,
        cover_path,
        cover_version,
    }
}

fn load_or_create(series_dir: &Path, folder_name: &str) -> SeriesConfig {
    let path = series_dir.join(CONFIG_FILE);

    if !path.exists() {
        let template = format!(
            "# KanFanOS 番剧配置（可手动编辑，扫描不会覆盖本文件）\n\
             # 显示名称；留空则使用文件夹名\n\
             name = \"{}\"\n\
             \n\
             # 搜索别名，可以写多个，例如 [\"英文名\", \"简称\"]\n\
             aliases = []\n\
             \n\
             # 封面图片，相对本文件夹的路径；留空时自动查找 cover/poster 图片，\n\
             # 找不到就使用文件夹内排序最靠前的一张图片\n\
             cover = \"\"\n",
            escape_toml(folder_name)
        );

        if let Err(error) = fs::write(&path, template) {
            eprintln!("写入番剧配置失败 {}: {error}", path.display());
        }

        return SeriesConfig {
            name: folder_name.to_string(),
            ..Default::default()
        };
    }

    match fs::read_to_string(&path) {
        Ok(content) => toml::from_str(&content).unwrap_or_else(|error| {
            eprintln!("解析番剧配置失败 {}: {error}", path.display());
            SeriesConfig::default()
        }),
        Err(error) => {
            eprintln!("读取番剧配置失败 {}: {error}", path.display());
            SeriesConfig::default()
        }
    }
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

    use super::{resolve, CONFIG_FILE};

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
        assert!(resolved.cover_path.is_none());
        assert!(dir.join(CONFIG_FILE).is_file());

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
