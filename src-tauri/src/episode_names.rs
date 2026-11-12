//! 从发布标题/视频文件名里解析集数与版本，并做集数完整性分析。
//!
//! 这个模块只处理「文件名本身」，不依赖 RSS、下载或播放流程；
//! 剧集排序、重复集数/中间缺集提示都以这里的结果为准。

use std::sync::OnceLock;

use regex::Regex;
use serde::Serialize;

fn episode_patterns() -> &'static [Regex] {
    static PATTERNS: OnceLock<Vec<Regex>> = OnceLock::new();
    PATTERNS.get_or_init(|| {
        [
            r"(?i)(?:s\d{1,2}e|\bep?)\s*0*(\d{1,3})(?:v\d+)?",
            r"第\s*0*(\d{1,3})\s*[话話集]",
            r"(?:^|[\s\[\]\(\)【】_-])0*(\d{1,3})(?:v\d+)?(?:v\d+)?(?:[\s\[\]\(\)【】_]|$)",
            r"[\[\(]\s*0*(\d{1,3})(?:v\d+)?\s*[\]\)]",
        ]
        .iter()
        .filter_map(|pattern| Regex::new(pattern).ok())
        .collect()
    })
}

/// 找到标题里的第一个集数，返回 (集数, 匹配起点)。
pub fn find_episode(title: &str) -> Option<(i64, usize)> {
    for pattern in episode_patterns() {
        for captures in pattern.captures_iter(title) {
            let (Some(value), Some(full)) = (captures.get(1), captures.get(0)) else {
                continue;
            };
            let Ok(episode) = value.as_str().parse::<i64>() else {
                continue;
            };
            if !(1..=999).contains(&episode) {
                continue;
            }
            // 720/480 这类数字通常是分辨率，不是集数。
            if matches!(episode, 480 | 720 | 1080 | 2160)
                && title[value.end()..].trim_start().starts_with('p')
            {
                continue;
            }
            return Some((episode, full.start()));
        }
    }
    None
}

pub fn parse_episode(title: &str) -> Option<i64> {
    find_episode(title).map(|(episode, _)| episode)
}

/// 解析发布版本号（v1、v2…）；没有版本标记时视为 1。
///
/// 与常见的 `\bv(\d+)\b` 不同，这里允许紧跟在集数后面（`06v2`），
/// 只是要求 `v` 前面不是英文字母、后面不接数字。
pub fn version_number(title: &str) -> Option<i64> {
    static VERSION: OnceLock<Regex> = OnceLock::new();
    let regex = VERSION.get_or_init(|| Regex::new(r"(?i)(?:^|[^a-z])v(\d+)").unwrap());
    regex
        .captures_iter(title)
        .filter_map(|captures| captures.get(1))
        .filter_map(|capture| capture.as_str().parse::<i64>().ok())
        .max()
}

pub fn release_version(title: &str) -> i64 {
    version_number(title).unwrap_or(1)
}

/// 去掉扩展名，避免 `- 02.mkv` 里的集数因为后面跟着 `.` 而解析失败。
fn file_stem(value: &str) -> &str {
    let value = value.trim();
    match value.rsplit_once('.') {
        Some((stem, extension)) if !stem.is_empty() && !extension.is_empty() => stem,
        _ => value,
    }
}

pub fn parse_episode_from_filename(filename: &str) -> Option<i64> {
    parse_episode(file_stem(filename))
}

pub fn release_version_from_filename(filename: &str) -> i64 {
    release_version(file_stem(filename))
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DuplicateEpisode {
    pub episode: i64,
    pub files: Vec<String>,
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct EpisodeIssues {
    pub duplicates: Vec<DuplicateEpisode>,
    pub missing: Vec<i64>,
}

/// 按文件名分析集数完整性。
///
/// 只传入番剧配置里被选为「选集」的视频（自动/手动模式不限）；
/// 「其它视频」的编号不参与统计：
/// - 重复：同一集出现多个文件（包括 06 和 06v2）；
/// - 缺集：只检查最小集数和最大集数之间，首尾之外的缺失不算，
///   因为第二季可能直接从 14 集开始，总集数也无法从文件名推断。
pub fn analyze<'a>(files: impl IntoIterator<Item = &'a str>) -> EpisodeIssues {
    let mut by_episode: std::collections::BTreeMap<i64, Vec<String>> =
        std::collections::BTreeMap::new();
    for file in files {
        if let Some(episode) = parse_episode_from_filename(file) {
            by_episode.entry(episode).or_default().push(file.to_string());
        }
    }

    let mut duplicates = Vec::new();
    for (episode, names) in &mut by_episode {
        if names.len() > 1 {
            names.sort();
            duplicates.push(DuplicateEpisode {
                episode: *episode,
                files: names.clone(),
            });
        }
    }

    let mut missing = Vec::new();
    if let (Some(&first), Some(&last)) = (by_episode.keys().next(), by_episode.keys().next_back())
    {
        for episode in (first + 1)..last {
            if !by_episode.contains_key(&episode) {
                missing.push(episode);
            }
        }
    }

    EpisodeIssues { duplicates, missing }
}

#[cfg(test)]
mod tests {
    use super::{analyze, parse_episode, release_version};

    #[test]
    fn parses_episode_and_version_from_release_names() {
        assert_eq!(parse_episode("[ANi] 测试番 - 06v2 [1080p].mkv"), Some(6));
        assert_eq!(release_version("[ANi] 测试番 - 06v2 [1080p].mkv"), 2);
        assert_eq!(release_version("[ANi] 测试番 - 06 [1080p].mkv"), 1);
        assert_eq!(release_version("[1080p][V3][x264].mkv"), 3);
        assert_eq!(parse_episode("测试番 第14话 [1080p].mkv"), Some(14));
        assert_eq!(parse_episode("Show.S02E03.1080p.mkv"), Some(3));
        assert_eq!(parse_episode("03_Sintel.mp4"), Some(3));
        assert_eq!(parse_episode("05_BigBuckBunny_短.mp4"), Some(5));
        assert_eq!(parse_episode("NCOP.mkv"), None);
        assert_eq!(parse_episode("Anime 1080p.mkv"), None);
    }

    #[test]
    fn detects_duplicate_and_middle_missing_episodes() {
        let issues = analyze([
            "番剧 - 01.mkv",
            "番剧 - 02.mkv",
            "番剧 - 02v2.mkv",
            "番剧 - 04.mkv",
            "番剧 - 14.mkv",
        ]);
        assert_eq!(issues.duplicates.len(), 1);
        assert_eq!(issues.duplicates[0].episode, 2);
        assert_eq!(issues.duplicates[0].files.len(), 2);
        assert_eq!(issues.missing, vec![3, 5, 6, 7, 8, 9, 10, 11, 12, 13]);
    }

    #[test]
    fn ignores_missing_before_first_and_after_last() {
        let issues = analyze(["番剧 - 14.mkv", "番剧 - 15.mkv", "番剧 - 17.mkv"]);
        assert!(issues.duplicates.is_empty());
        assert_eq!(issues.missing, vec![16]);

        let single = analyze(["番剧 - 08.mkv"]);
        assert!(single.missing.is_empty());
    }
}
