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
///
/// - `Season 2`、`Part 2` 里的数字是季/部编号，直接跳过；
/// - 标题里有 ` - ` 分隔符时，优先取分隔符之后的数字。季号通常写在
///   标题部分（`某番 2 - 06`、`某番 / Show 2 - 06`），集数在分隔符后面。
pub fn find_episode(title: &str) -> Option<(i64, usize)> {
    let hyphen = title.find(" - ");
    for (index, pattern) in episode_patterns().iter().enumerate() {
        // 只对「任意位置的数字」规则启用；SxxExx、第 N 话本身就是明确标记。
        let prefer_after = if index == 2 { hyphen } else { None };
        let mut fallback = None;

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
            // `Season 2`、`Part 2` 里的数字是季/部编号，不是集数。
            if is_season_marker(title, value.start()) {
                continue;
            }
            // 720/480 这类数字通常是分辨率，不是集数；`720P` 也按分辨率处理。
            if matches!(episode, 480 | 720 | 1080 | 2160)
                && title[value.end()..]
                    .trim_start()
                    .chars()
                    .next()
                    .is_some_and(|ch| ch.eq_ignore_ascii_case(&'p'))
            {
                continue;
            }

            let found = (episode, full.start());
            if let Some(cut) = prefer_after {
                if value.start() > cut {
                    return Some(found);
                }
                fallback.get_or_insert(found);
            } else {
                return Some(found);
            }
        }

        if let Some(found) = fallback {
            return Some(found);
        }
    }
    None
}

/// 数字紧跟在 `Season` / `Part` / `Cour` / `Volume` 后面时，它是季/部编号。
///
/// 例如 `[ANi] 戀上換裝娃娃 Season 2 - 24 [...]`：如果从前往后找数字，
/// 会先选中季号 2，导致第 13–24 集全部被当成第 2 集。
fn is_season_marker(title: &str, digits_start: usize) -> bool {
    let prefix = title[..digits_start].trim_end();
    let word = prefix
        .rsplit(|ch: char| !ch.is_ascii_alphabetic())
        .next()
        .unwrap_or_default();
    matches!(
        word.to_ascii_lowercase().as_str(),
        "season" | "part" | "cour" | "cours" | "volume" | "vol"
    )
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
/// - 重复：同一集的同一版本出现多个文件。同一集的不同版本（06 与 06v2）
///   不算重复：自动下载会保留旧版本，它们是同一集的新旧发布；
/// - 缺集：只检查最小集数和最大集数之间，首尾之外的缺失不算，
///   因为第二季可能直接从 14 集开始，总集数也无法从文件名推断。
pub fn analyze<'a>(files: impl IntoIterator<Item = &'a str>) -> EpisodeIssues {
    let mut by_episode: std::collections::BTreeMap<i64, Vec<&'a str>> =
        std::collections::BTreeMap::new();
    for file in files {
        if let Some(episode) = parse_episode_from_filename(file) {
            by_episode.entry(episode).or_default().push(file);
        }
    }

    let mut duplicates = Vec::new();
    for (&episode, names) in &by_episode {
        // 版本号是「同一集的不同发布」，只有版本也相同的多个文件才算重复。
        let mut by_version: std::collections::BTreeMap<i64, Vec<&str>> =
            std::collections::BTreeMap::new();
        for name in names {
            by_version
                .entry(release_version_from_filename(name))
                .or_default()
                .push(name);
        }
        let mut duplicated: Vec<String> = by_version
            .into_iter()
            .filter(|(_, files)| files.len() > 1)
            .flat_map(|(_, files)| files.into_iter().map(str::to_string))
            .collect();
        if duplicated.len() > 1 {
            duplicated.sort();
            duplicates.push(DuplicateEpisode {
                episode,
                files: duplicated,
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
    use super::{analyze, parse_episode, parse_episode_from_filename, release_version};

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
        // 大写分辨率标记也不能当成集数。
        assert_eq!(
            parse_episode_from_filename("[720P] 测试番 - 05.mkv"),
            Some(5)
        );
        assert_eq!(
            parse_episode_from_filename("[480P] 测试番 - 05.mkv"),
            Some(5)
        );
    }

    #[test]
    fn season_numbers_are_not_episodes() {
        assert_eq!(
            parse_episode_from_filename(
                "[ANi] 戀上換裝娃娃 Season 2 - 24 [1080P][Baha][WEB-DL][AAC AVC][CHT][MP4].mkv"
            ),
            Some(24)
        );

        // bangumi.moe 实际标题：Season 2/3 在标题里，集数在 ` - ` 后面。
        assert_eq!(
            parse_episode(
                "[黒ネズミたち] 黑化吧！聖女大人 第二季 / Kizu darake Seijo Season 2 - 07 (Baha 1920x1080 AVC AAC MP4)"
            ),
            Some(7)
        );
        assert_eq!(
            parse_episode(
                "[黒ネズミたち] 青之蘆葦 第二季 / Ao Ashi Season 2 - 06 (ABEMA 1920x1080 AVC AAC MKV)"
            ),
            Some(6)
        );
        assert_eq!(
            parse_episode(
                "[黒ネズミたち] 百妖谱·京师篇（日配版） / Fairies Album Season 3 (Japanese Dub) - 06 (ABEMA 1920x1080 AVC AAC MKV)"
            ),
            Some(6)
        );
        assert_eq!(
            parse_episode(
                "[黒ネズミたち] 黑色五葉草 Season 2 / Black Clover 2nd Season - 176 (ABEMA 1920x1080 AVC AAC MKV)"
            ),
            Some(176)
        );

        // 同一季的 13–24 集不应因为标题里的 `Season 2` 而全部变成第 2 集。
        let files: Vec<String> = (13..=24)
            .map(|episode| {
                format!(
                    "[ANi] 戀上換裝娃娃 Season 2 - {episode} [1080P][Baha][WEB-DL][AAC AVC][CHT][MP4].mkv"
                )
            })
            .collect();
        let names: Vec<&str> = files.iter().map(String::as_str).collect();
        let issues = analyze(names);
        assert!(issues.duplicates.is_empty());
        assert!(issues.missing.is_empty());

        // 只有季号、没有集数时不应解析出集数。
        assert_eq!(
            parse_episode_from_filename("[ANi] 测试番 Season 2 [1080P].mkv"),
            None
        );
    }

    #[test]
    fn hyphen_separator_skips_numbered_season_in_title() {
        // 季号直接跟在中文标题后面、没有 Season 关键字。
        for episode in 6..=8 {
            assert_eq!(
                parse_episode_from_filename(&format!(
                    "[ANi]  冰劍的魔術師將要統一世界 2 - {episode} [1080P][Baha][WEB-DL][AAC AVC][CHT][MP4].mkv"
                )),
                Some(episode)
            );
        }
        assert_eq!(
            parse_episode_from_filename(
                "[黒ネズミたち] 冰劍的魔術師將要統一世界 2 / Hyouken no Majutsushi ga Sekai wo Suberu II - 06 (CR 1920x1080 AVC AAC MKV).mkv"
            ),
            Some(6)
        );

        // 标题本身的数字（例如《86》）不再抢走集数。
        assert_eq!(
            parse_episode_from_filename("[Lilith-Raws] 86 - 01 [1080p].mkv"),
            Some(1)
        );

        // 分隔符后面没有数字时，仍取第一个集数（04_52 -> 04）。
        assert_eq!(
            parse_episode_from_filename("[BeanSub][Show][04_52][1080P].mkv"),
            Some(4)
        );
        assert_eq!(
            parse_episode_from_filename("番剧 06 - [字幕组].mkv"),
            Some(6)
        );
    }

    #[test]
    fn detects_duplicate_and_middle_missing_episodes() {
        let issues = analyze([
            "番剧 - 01.mkv",
            "番剧 - 02.mkv",
            "番剧 - 02v2.mkv",
            "番剧 - 04.mkv",
            "番剧 - 04 (1).mkv",
            "番剧 - 14.mkv",
        ]);
        assert_eq!(issues.duplicates.len(), 1);
        assert_eq!(issues.duplicates[0].episode, 4);
        assert_eq!(
            issues.duplicates[0].files,
            vec!["番剧 - 04 (1).mkv", "番剧 - 04.mkv"]
        );
        assert_eq!(issues.missing, vec![3, 5, 6, 7, 8, 9, 10, 11, 12, 13]);
    }

    #[test]
    fn different_versions_of_one_episode_are_not_duplicates() {
        // 自动下载会保留旧版本：06 与 06v2 是同一集的新旧发布，不是重复文件。
        let issues = analyze(["番剧 - 06.mkv", "番剧 - 06v2.mkv"]);
        assert!(issues.duplicates.is_empty());
        assert!(issues.missing.is_empty());

        let test_clips = analyze(["03_Sintel.mp4", "03v2_Sintel.mp4"]);
        assert!(test_clips.duplicates.is_empty());
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
