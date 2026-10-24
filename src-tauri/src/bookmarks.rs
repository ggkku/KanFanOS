//! 网页收藏夹。保存在数据目录的 `config/bookmarks.json`。
//!
//! 收藏项完全由用户手动维护：新增、改标题/地址、删除。地址按规范化后的
//! 形式去重，重复收藏同一地址只更新标题。

use std::{
    fs,
    path::{Path, PathBuf},
};

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};

use crate::time::now_ms;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Bookmark {
    pub id: u64,
    pub title: String,
    pub url: String,
    #[serde(default)]
    pub created_at: i64,
}

fn file(data_root: &Path) -> PathBuf {
    data_root.join("config").join("bookmarks.json")
}

pub fn list(data_root: &Path) -> Result<Vec<Bookmark>> {
    let path = file(data_root);
    if !path.exists() {
        return Ok(Vec::new());
    }

    let content = fs::read_to_string(&path)
        .with_context(|| format!("读取收藏夹失败：{}", path.display()))?;
    if content.trim().is_empty() {
        return Ok(Vec::new());
    }

    serde_json::from_str(&content)
        .with_context(|| format!("解析收藏夹失败：{}", path.display()))
}

fn save(data_root: &Path, items: &[Bookmark]) -> Result<()> {
    let path = file(data_root);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("创建收藏夹目录失败：{}", parent.display()))?;
    }

    let content = serde_json::to_string_pretty(items)?;
    fs::write(&path, content)
        .with_context(|| format!("写入收藏夹失败：{}", path.display()))
}

/// 补全协议、校验并返回规范化 URL。
fn normalize_url(input: &str) -> Result<String> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        bail!("网址不能为空");
    }

    let candidate = if trimmed.contains("://") {
        trimmed.to_string()
    } else {
        format!("https://{trimmed}")
    };

    let url = tauri::Url::parse(&candidate)
        .with_context(|| format!("无法解析网址：{trimmed}"))?;

    match url.scheme() {
        "http" | "https" => Ok(url.to_string()),
        scheme => bail!("收藏夹不支持 {scheme} 链接"),
    }
}

fn clean_title(title: &str, url: &str) -> String {
    let title = title.trim();
    if !title.is_empty() {
        return title.to_string();
    }

    tauri::Url::parse(url)
        .ok()
        .and_then(|url| url.host_str().map(str::to_string))
        .unwrap_or_else(|| url.to_string())
}

fn next_id(items: &[Bookmark]) -> u64 {
    let mut id = now_ms().max(1) as u64;
    while items.iter().any(|item| item.id == id) {
        id += 1;
    }
    id
}

pub fn add(data_root: &Path, title: &str, url: &str) -> Result<Vec<Bookmark>> {
    let url = normalize_url(url)?;
    let title = clean_title(title, &url);
    let mut items = list(data_root)?;

    if let Some(existing) = items.iter_mut().find(|item| item.url == url) {
        existing.title = title;
    } else {
        let id = next_id(&items);
        items.push(Bookmark {
            id,
            title,
            url,
            created_at: now_ms(),
        });
    }

    save(data_root, &items)?;
    Ok(items)
}

pub fn update(
    data_root: &Path,
    id: u64,
    title: &str,
    url: &str,
) -> Result<Vec<Bookmark>> {
    let url = normalize_url(url)?;
    let title = clean_title(title, &url);
    let mut items = list(data_root)?;

    let Some(item) = items.iter_mut().find(|item| item.id == id) else {
        bail!("收藏不存在");
    };
    item.title = title;
    item.url = url;

    save(data_root, &items)?;
    Ok(items)
}

pub fn remove(data_root: &Path, id: u64) -> Result<Vec<Bookmark>> {
    let mut items = list(data_root)?;
    items.retain(|item| item.id != id);
    save(data_root, &items)?;
    Ok(items)
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        time::{SystemTime, UNIX_EPOCH},
    };

    use super::{add, list, remove, update};

    fn temp_dir(name: &str) -> std::path::PathBuf {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("kanfanos-bookmarks-{name}-{stamp}"));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn add_dedupes_and_updates_title() {
        let dir = temp_dir("add");
        let items = add(&dir, "Example", "example.com").unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].url, "https://example.com/");
        assert_eq!(items[0].title, "Example");

        let items = add(&dir, "Example 2", "https://example.com/").unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].title, "Example 2");

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn title_falls_back_to_host() {
        let dir = temp_dir("fallback");
        let items = add(&dir, "  ", "https://example.com/path").unwrap();
        assert_eq!(items[0].title, "example.com");
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn update_and_remove() {
        let dir = temp_dir("update");
        let items = add(&dir, "A", "https://a.example").unwrap();
        let id = items[0].id;

        let items = update(&dir, id, "B", "b.example").unwrap();
        assert_eq!(items[0].title, "B");
        assert_eq!(items[0].url, "https://b.example/");

        let items = remove(&dir, id).unwrap();
        assert!(items.is_empty());
        assert!(list(&dir).unwrap().is_empty());

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn rejects_non_http_schemes() {
        let dir = temp_dir("scheme");
        assert!(add(&dir, "bad", "file:///C:/secret.txt").is_err());
        let _ = fs::remove_dir_all(dir);
    }
}
