//! 从发布标题/视频文件名里解析集数与版本，并做集数完整性分析。
//!
//! 这个模块只处理「文件名本身」，不依赖 RSS、下载或播放流程；
//! 剧集排序、重复集数/中间缺集提示都以这里的结果为准。

use std::sync::OnceLock;

use regex::Regex;
use serde::Serialize;

/// 明确带标记的集数：`S02E06`、`EP06`、`#06`、`第 6 话` 等。
///
/// 这类数字不需要参与置信度竞争，直接按标记取。
fn explicit_patterns() -> &'static [Regex] {
    static PATTERNS: OnceLock<Vec<Regex>> = OnceLock::new();
    PATTERNS.get_or_init(|| {
        [
            r"(?i)(?:\bs\s*\d{1,2}\s*[._-]?\s*e\s*p?|\bepisode\s*|\bep?\s*|#)\s*0*(\d{1,4})(?:v\d+)?",
            r"第\s*0*(\d{1,4})\s*[话話集回页頁]",
        ]
        .iter()
        .filter_map(|pattern| Regex::new(pattern).ok())
        .collect()
    })
}

/// 标题/文件名里的一个数字段，以及它「像集数」「像季数」的置信度。
///
/// 集数和季数在很多命名里都只是裸数字（`Show 2 - 06`、`Show 2 [06]`），
/// 单靠位置无法覆盖全部情况。这里给每个数字累加特征分，再比较谁更像集数：
/// 上下文（Season/Part/第 N 季/分辨率/编码/x264）决定大方向，位置和分隔符
/// 用来区分同一标题里相邻的两个数字。
#[derive(Debug, Clone)]
struct NumberCandidate {
    value: i64,
    start: usize,
    end: usize,
    /// 返回给标题截取的起点：数字被括号包住时用左括号的位置，
    /// 避免把半个 `[06]` 留在标题里。
    match_start: usize,
    episode_score: i32,
    season_score: i32,
    /// `(1)` 这类小括号单数字通常是重复下载的副本后缀，不是集数。
    copy_suffix: bool,
}

impl NumberCandidate {
    fn adjusted(&self) -> i32 {
        self.episode_score - self.season_score
    }

    /// 有明确的季/部标记，不能参与集数候选。
    fn is_season(&self) -> bool {
        self.season_score >= 60
    }
}

fn number_pattern() -> &'static Regex {
    static NUMBERS: OnceLock<Regex> = OnceLock::new();
    NUMBERS.get_or_init(|| Regex::new(r"[0-9]+").unwrap())
}

/// 截取数字前面的最后一个英文单词（跳过空格和常见点号）。
///
/// 不跨越 ` - ` / `/` / `|` 这类分段符：`Black Clover 2nd Season - 176`
/// 里的 176 不能因为前面隔着一个 ` - ` 就继承 `Season` 的季号置信度。
fn word_before(value: &str) -> &str {
    let segment_start = value
        .rfind(" - ")
        .map(|index| index + 3)
        .into_iter()
        .chain(value.rfind('/').map(|index| index + 1))
        .chain(value.rfind('|').map(|index| index + 1))
        .chain(value.rfind('｜').map(|index| index + '｜'.len_utf8()))
        .max()
        .unwrap_or(0);
    let segment = &value[segment_start..];
    let trimmed = segment
        .trim_end_matches(|ch: char| ch.is_whitespace() || matches!(ch, '.' | ':' | '_' | '-'));
    let mut start = 0;
    for (index, ch) in trimmed.char_indices() {
        if !ch.is_ascii_alphabetic() {
            start = index + ch.len_utf8();
        }
    }
    &trimmed[start..]
}

/// 数字外面套着的最近一层括号（只处理单层，足够覆盖发布命名）。
fn enclosing_bracket(text: &str, start: usize, end: usize) -> Option<(usize, char, usize)> {
    const PAIRS: [(char, char); 4] = [('[', ']'), ('(', ')'), ('【', '】'), ('（', '）')];
    for (index, ch) in text[..start].char_indices().rev() {
        let Some(&(_, close)) = PAIRS.iter().find(|(open, _)| *open == ch) else {
            continue;
        };
        // 这个左括号在数字前面已经闭合了，说明数字不在它里面。
        if text[index + ch.len_utf8()..start].contains(close) {
            continue;
        }
        let Some(offset) = text[end..].find(close) else {
            continue;
        };
        return Some((index, ch, end + offset));
    }
    None
}

/// 技术参数里的数字：分辨率、编码、码率、声道、位深等。
fn looks_technical(before: &str, after: &str, value: i64) -> bool {
    let after_trim = after.trim_start();
    let after_lower = after_trim.to_ascii_lowercase();
    let after_word = after_lower.trim_start_matches(['-', '_']);

    // 5.1 / 2.0 / 23.976 这类小数。
    if after.starts_with('.') && after[1..].starts_with(|ch: char| ch.is_ascii_digit()) {
        return true;
    }
    if before.ends_with('.') {
        let prefix = &before[..before.len() - 1];
        if prefix.ends_with(|ch: char| ch.is_ascii_digit()) {
            return true;
        }
    }

    // 只匹配完整的单位词：`5.1ch` 是声道，`06 CHS` 是字幕语言，不能混为一谈。
    let unit_end = after_word
        .find(|ch: char| !ch.is_ascii_alphabetic())
        .unwrap_or(after_word.len());
    let unit = &after_word[..unit_end];
    if matches!(
        unit,
        "bit" | "bits" | "fps" | "kbps" | "mbps" | "kb" | "hz" | "khz" | "ch" | "mb" | "gb"
    ) {
        return true;
    }

    // 720P / 480p / 1080i 以及 1280x720。
    if matches!(
        value,
        360 | 480 | 540 | 576 | 720 | 1080 | 1440 | 2160 | 4320
    ) && (after_trim
        .chars()
        .next()
        .is_some_and(|ch| matches!(ch, 'p' | 'P' | 'i' | 'I'))
        || (before.ends_with('x') || before.ends_with('X'))
            && before[..before.len() - 1]
                .chars()
                .next_back()
                .is_some_and(|ch| ch.is_ascii_digit()))
    {
        return true;
    }
    // 1920x1080 这类分辨率宽度：数字后面紧跟 xNNN，且数值不是季/集的小编号。
    if value >= 360
        && (after.starts_with('x') || after.starts_with('X'))
        && after[1..].starts_with(|ch: char| ch.is_ascii_digit())
    {
        return true;
    }

    let word = word_before(before).to_ascii_lowercase();
    matches!(
        word.as_str(),
        "x" | "h"
            | "av"
            | "avc"
            | "hevc"
            | "av1"
            | "vp9"
            | "vp"
            | "hdr"
            | "dv"
            | "uhd"
            | "ma"
            | "hi"
            | "yuv"
            | "bt"
            | "ddp"
            | "dd"
            | "aac"
            | "flac"
            | "opus"
            | "mp"
            | "mpeg"
            | "dts"
            | "eac"
            | "ac"
            | "xvid"
            | "divx"
            | "web"
            | "dl"
            | "bd"
            | "remux"
            | "imax"
            | "hc"
            | "fhd"
            | "webdl"
            | "big"
    )
}

fn has_leading_zero(digits: &str) -> bool {
    digits.len() > 1 && digits.starts_with('0')
}

/// `06v2` / `[V2]`：数字紧跟在版本号 `v` 后面。
fn is_version_number(before: &str) -> bool {
    let Some(prev) = before.chars().next_back() else {
        return false;
    };
    if prev != 'v' && prev != 'V' {
        return false;
    }
    before[..before.len() - 1]
        .chars()
        .next_back()
        .map_or(true, |ch| !ch.is_ascii_alphabetic())
}

/// 一段文字里是否还有「可能当集数」的数字（排除版本、分辨率、编码等）。
fn has_plausible_number(value: &str) -> bool {
    number_pattern()
        .find_iter(value)
        .filter_map(|matched| {
            let number = matched.as_str().parse::<i64>().ok()?;
            (1..=9999).contains(&number).then_some((matched, number))
        })
        .any(|(matched, number)| {
            let before = &value[..matched.start()];
            let after = &value[matched.end()..];
            !is_version_number(before) && !looks_technical(before, after, number)
        })
}

/// 把标题/文件名切成数字段并逐个打分。
fn numeric_candidates(title: &str) -> Vec<NumberCandidate> {
    // 用最后一个 ` - `：`Show - 12 - 06` 里集数更可能在最后一段。
    let hyphen = title.rfind(" - ");
    let mut candidates = Vec::new();

    for matched in number_pattern().find_iter(title) {
        let digits = matched.as_str();
        let Ok(value) = digits.parse::<i64>() else {
            continue;
        };
        // 长篇番剧会有 4 位数集数（蜡笔小新、柯南等），但要排除年份。
        if !(1..=9999).contains(&value) || (1900..=2099).contains(&value) {
            continue;
        }

        let start = matched.start();
        let end = matched.end();
        let before = &title[..start];
        let after = &title[end..];

        // `06v2` / `[V2]` 里的 2 是发布版本，不是集数。
        if is_version_number(before) {
            continue;
        }

        // `2x06` 里的 06 紧跟在 x 后面，但这是集数而不是 x264 这类编码。
        // `1920x1080` 的 x 前面是四位宽度，不能套用这个规则。
        let x_episode = if matches!(before.chars().next_back(), Some('x' | 'X')) {
            let prefix = &before[..before.len() - 1];
            let width = prefix
                .chars()
                .rev()
                .take_while(|ch| ch.is_ascii_digit())
                .count();
            (1..=2).contains(&width)
        } else {
            false
        };

        if !x_episode && looks_technical(before, after, value) {
            continue;
        }

        let before_trim = before.trim_end();
        let after_trim = after.trim_start();
        // `第30号杀人事件` 这类是标题序号，不是集数。
        if before_trim.ends_with('第') && after_trim.starts_with('号') {
            continue;
        }
        let after_lower = after_trim.to_ascii_lowercase();
        let prev = before.chars().next_back();
        let next = after.chars().next();

        let mut episode = 0;
        let mut season = 0;
        let mut copy_suffix = false;
        let mut match_start = start;

        // 基础权重：两位数是典型集数，三位数多见于长篇，一位数最不确定。
        episode += match value {
            1..=9 => 3,
            10..=99 => 12,
            _ => 4,
        };

        // 括号内容：`[06]`、`[04_52]` 很像集数；开头的 `[字幕组7]` 不是，
        // `[WebRip ... ASSx1]` 这类参数括号里的数字也要压下去。
        let mut metadata_bracket = false;
        if let Some((open_index, open, close_index)) = enclosing_bracket(title, start, end) {
            let content = &title[open_index + open.len_utf8()..close_index];
            let content_trim = content.trim();
            let has_alpha = content_trim.chars().any(|ch| ch.is_ascii_alphabetic());
            let leading_group = title[..open_index].trim().is_empty();

            if leading_group && has_alpha {
                // 开头的 `[Group7]` / `[Show 2]` 是组名或标题，不是集数。
                continue;
            }

            match_start = open_index;
            let numeric_content = content_trim.chars().all(|ch| {
                ch.is_ascii_digit()
                    || ch.is_whitespace()
                    || matches!(ch, '_' | '-' | '~' | ',' | '/')
            });
            let first_word = content_trim.split_whitespace().next().unwrap_or("");
            let exact = first_word == digits
                || (first_word.starts_with(digits)
                    && first_word[digits.len()..]
                        .trim_start_matches(['v', 'V'])
                        .chars()
                        .all(|ch| ch.is_ascii_digit())
                    && first_word.len() > digits.len());

            if !has_alpha || numeric_content || exact {
                episode += 15;
                if numeric_content {
                    episode += 10;
                }
                if exact {
                    episode += 20;
                }
            } else {
                // 组名/编码/参数括号：里面的数字基本不可能是集数。
                metadata_bracket = true;
                episode -= 30;
            }

            // `04 (1)` / `04 (12)`：小括号里的裸数字通常是重复下载的副本后缀。
            // `(06)` 这类带前导零的仍可能是正式集数，不在这里丢弃。
            if matches!(open, '(' | '（')
                && !has_leading_zero(content_trim)
                && content_trim.chars().all(|ch| ch.is_ascii_digit())
                && content_trim
                    .parse::<i64>()
                    .is_ok_and(|number| (1..=99).contains(&number))
            {
                copy_suffix = true;
            }
        }

        // 有 ` - ` 时，后面的数字更像集数；前面的通常是标题里的季/部号。
        if let Some(position) = hyphen {
            if start > position {
                if metadata_bracket {
                    episode -= 20;
                } else {
                    episode += 30;
                }
            } else {
                episode -= 5;
                // `Show (2) - 06`、`Show [2] - 06`：分隔符前、后面还有
                // 真正像集数的数字，这个小数字多半是季/部号。
                if has_plausible_number(&title[position + 3..]) {
                    season += 30;
                }
            }
        }

        // 数字两侧是分隔符时，更像单独的一段编号。
        if prev.map_or(true, |ch| {
            ch.is_whitespace()
                || matches!(
                    ch,
                    '[' | '(' | '【' | '（' | '_' | '-' | '/' | '|' | '｜' | '#'
                )
        }) {
            episode += 5;
        }
        if next.map_or(true, |ch| {
            ch.is_whitespace()
                || matches!(
                    ch,
                    ']' | ')' | '】' | '）' | '_' | '-' | '.' | ',' | '|' | '｜' | '❘'
                )
        }) {
            episode += 5;
        }
        if matches!(next, Some('v' | 'V')) && after[1..].starts_with(|ch: char| ch.is_ascii_digit())
        {
            episode += 5;
        }

        // 中文集数标记：`06话`、`第06`。`期` 按季号处理（`第2期`）。
        if after_trim
            .chars()
            .next()
            .is_some_and(|ch| matches!(ch, '话' | '話' | '集' | '回'))
        {
            episode += 40;
        }
        if before_trim.ends_with('第') {
            episode += 25;
        }
        if prev == Some('#') {
            episode += 40;
        }

        // `2x06`：x 后面的数字是集数，x 前面是季数。
        if x_episode {
            episode += 50;
        }
        if matches!(next, Some('x' | 'X')) && after[1..].starts_with(|ch: char| ch.is_ascii_digit())
        {
            season += 60;
        }

        // `Show_2_06`：下划线连接的两个数字里，带前导零的更像集数，
        // 没有前导零的单个数字更像季数。`[04_52]` 则取前一个。
        if after.starts_with('_') && after[1..].starts_with(|ch: char| ch.is_ascii_digit()) {
            if has_leading_zero(digits) {
                episode += 10;
            } else if value <= 9 {
                season += 40;
            }
        }
        if before.ends_with('_')
            && before[..before.len() - 1]
                .chars()
                .next_back()
                .is_some_and(|ch| ch.is_ascii_digit())
        {
            if has_leading_zero(digits) {
                episode += 20;
            } else {
                episode -= 10;
            }
        }

        // 连续的 `01-12` 是一个范围，通常第一个数字是集数。
        if after.starts_with('-') && after[1..].starts_with(|ch: char| ch.is_ascii_digit()) {
            episode += 10;
        }
        if before.ends_with('-')
            && before[..before.len() - 1]
                .chars()
                .next_back()
                .is_some_and(|ch| ch.is_ascii_digit())
        {
            episode -= 10;
        }

        // 季/部标记：Season 2、Part 2、2nd Season、S02、2季、第二季。
        let word = word_before(before).to_ascii_lowercase();
        if matches!(
            word.as_str(),
            "season" | "part" | "cour" | "cours" | "volume" | "vol"
        ) {
            season += 100;
        }
        if word == "s" && !matches!(next, Some('e' | 'E')) {
            season += 80;
        }
        if after_lower.starts_with("nd season")
            || after_lower.starts_with("rd season")
            || after_lower.starts_with("th season")
            || after_lower.starts_with("st season")
        {
            season += 100;
        }
        if before_trim.ends_with('第')
            && after_trim
                .chars()
                .next()
                .is_some_and(|ch| matches!(ch, '季' | '部' | '章' | '期'))
        {
            season += 100;
        }
        if after_trim
            .chars()
            .next()
            .is_some_and(|ch| matches!(ch, '季' | '部' | '章' | '期'))
        {
            season += 80;
        }

        candidates.push(NumberCandidate {
            value,
            start,
            end,
            match_start,
            episode_score: episode,
            season_score: season,
            copy_suffix,
        });
    }

    // 相邻的裸数字：`Show 2 06` 里的 2 更像季号，06 更像集数。
    candidates.sort_by_key(|candidate| candidate.start);
    for index in 0..candidates.len().saturating_sub(1) {
        let between = &title[candidates[index].end..candidates[index + 1].start];
        let only_separators = between
            .chars()
            .all(|ch| ch.is_whitespace() || matches!(ch, '|' | '｜' | '❘' | '/' | '／' | '~'));
        if only_separators && candidates[index].value <= 9 && candidates[index].season_score < 60 {
            candidates[index].season_score += 25;
        }
    }

    // 多段 ` - ` 时，最后一个含非季号数字的段更像集数所在段：
    // `Show - 12 - 06` / `Show 2 - 06 - 1080p` 都应落到 06。
    if title.contains(" - ") {
        let max_segment = candidates
            .iter()
            .filter(|candidate| !candidate.is_season())
            .map(|candidate| title[..candidate.start].matches(" - ").count())
            .max();
        if let Some(max_segment) = max_segment {
            for candidate in &mut candidates {
                if !candidate.is_season()
                    && title[..candidate.start].matches(" - ").count() == max_segment
                {
                    candidate.episode_score += 15;
                }
            }
        }
    }

    // `(1)` 这种副本后缀只有在前面已经有一个像样的集数时才丢弃；
    // `Show 2 (6)` 里的 (6) 仍可能是集数，不能删。
    let strong_before: Vec<usize> = candidates
        .iter()
        .filter(|candidate| !candidate.is_season() && candidate.episode_score >= 30)
        .map(|candidate| candidate.start)
        .collect();
    candidates.retain(|candidate| {
        !(candidate.copy_suffix && strong_before.iter().any(|start| *start < candidate.start))
    });

    candidates
}

/// 找到标题里的集数，返回 (集数, 匹配起点)。
///
/// 先取带明确标记的 `SxxExx` / `EPxx` / `第 N 话`；没有标记时，把标题里的
/// 每个数字段当作候选，分别累计「像集数」和「像季数」的特征分，取更像集数
/// 的那个。` - ` 后面的数字、`[06]`、`第` / `话` 会提高集数置信度，
/// `Season` / `Part` / `S02` / `2季` / `2nd Season` 会提高季数置信度。
pub fn find_episode(title: &str) -> Option<(i64, usize)> {
    for pattern in explicit_patterns() {
        for captures in pattern.captures_iter(title) {
            let (Some(value), Some(full)) = (captures.get(1), captures.get(0)) else {
                continue;
            };
            let Ok(episode) = value.as_str().parse::<i64>() else {
                continue;
            };
            if (1..=9999).contains(&episode) {
                return Some((episode, full.start()));
            }
        }
    }

    let candidates = numeric_candidates(title);
    if candidates.is_empty() {
        return None;
    }
    let has_season = candidates.iter().any(NumberCandidate::is_season);

    let mut best: Option<&NumberCandidate> = None;
    for candidate in candidates.iter().filter(|item| !item.is_season()) {
        if candidate.adjusted() <= 0 {
            continue;
        }
        let better = best.map_or(true, |current| {
            candidate.adjusted() > current.adjusted()
                || (candidate.adjusted() == current.adjusted()
                    && (candidate.episode_score > current.episode_score
                        || (candidate.episode_score == current.episode_score
                            && candidate.start < current.start)))
        });
        if better {
            best = Some(candidate);
        }
    }

    if let Some(best) = best {
        return Some((best.value, best.match_start));
    }

    // 只出现一个裸数字时沿用旧行为（`Show 2.mkv` -> 2）；但标题里已经
    // 明确写了季号（`Show Season 2`）就不再猜。
    let viable: Vec<&NumberCandidate> = candidates
        .iter()
        .filter(|candidate| !candidate.is_season())
        .collect();
    if !has_season && viable.len() == 1 {
        return Some((viable[0].value, viable[0].match_start));
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
    if let (Some(&first), Some(&last)) = (by_episode.keys().next(), by_episode.keys().next_back()) {
        for episode in (first + 1)..last {
            if !by_episode.contains_key(&episode) {
                missing.push(episode);
            }
        }
    }

    EpisodeIssues {
        duplicates,
        missing,
    }
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

    #[test]
    fn scores_each_number_by_context() {
        let candidates = super::numeric_candidates("Show Season 2 - 24 [1080P]");
        let season = candidates.iter().find(|item| item.value == 2).unwrap();
        let episode = candidates.iter().find(|item| item.value == 24).unwrap();
        assert!(season.is_season());
        assert!(episode.adjusted() > season.adjusted());

        let candidates = super::numeric_candidates("Show 2 [06]");
        let seasonish = candidates.iter().find(|item| item.value == 2).unwrap();
        let episode = candidates.iter().find(|item| item.value == 6).unwrap();
        assert!(episode.adjusted() > seasonish.adjusted());
        assert_eq!(parse_episode("Show 2 [06]"), Some(6));
    }

    #[test]
    fn parses_season_numbers_without_season_keyword() {
        for name in [
            "Show 2 [06][1080P].mkv",
            "Show 2 [06v2][1080P].mkv",
            "Show 2 06 [1080P].mkv",
            "Show 2nd Season - 06.mkv",
            "Show Part 2 - 06.mkv",
            "Show 2季 - 06.mkv",
            "Show 第2季 - 06.mkv",
            "Show 第2期 - 06.mkv",
            "Show (2) - 06.mkv",
            "Show [2] - 06.mkv",
            "Show S2 - 06.mkv",
            "Show 2x06.mkv",
            "Show 2 - 06 - 1080p.mkv",
        ] {
            assert_eq!(parse_episode_from_filename(name), Some(6), "{name}");
        }
        assert_eq!(release_version("Show 2 [06v2][1080P].mkv"), 2);
    }

    #[test]
    fn ignores_group_years_resolutions_and_codecs() {
        assert_eq!(
            parse_episode_from_filename("[Group7] Show - 06.mkv"),
            Some(6)
        );
        assert_eq!(
            parse_episode_from_filename("[Group] Show - 06 [1080p][x264][AAC 5.1].mkv"),
            Some(6)
        );
        assert_eq!(
            parse_episode_from_filename("[Group] Show - 06 [10bit][23.976fps].mkv"),
            Some(6)
        );
        assert_eq!(
            parse_episode_from_filename("[Group] Show - 06 1920x1080.mkv"),
            Some(6)
        );
        assert_eq!(
            parse_episode_from_filename("[Group] Show 2024 - 06.mkv"),
            Some(6)
        );
        assert_eq!(
            parse_episode_from_filename("[Group] Show - 06 CHS.mkv"),
            Some(6)
        );
        assert_eq!(
            parse_episode_from_filename("[Group] Show - 06 x264 CHS.mkv"),
            Some(6)
        );
        assert_eq!(
            parse_episode_from_filename("[Group] Show - 06 [x265 10bit].mkv"),
            Some(6)
        );
    }

    #[test]
    fn handles_episode_before_hyphen_and_numeric_ranges() {
        assert_eq!(parse_episode_from_filename("Show 06 - 1080p.mkv"), Some(6));
        assert_eq!(parse_episode_from_filename("Show - 12 - 06.mkv"), Some(6));
        assert_eq!(parse_episode_from_filename("Show 12 - 06.mkv"), Some(6));
        assert_eq!(parse_episode_from_filename("[01-12] Show.mkv"), Some(1));
        assert_eq!(parse_episode_from_filename("Show 01-12.mkv"), Some(1));
        assert_eq!(
            parse_episode_from_filename("[BeanSub][Show][04_52].mkv"),
            Some(4)
        );
        assert_eq!(parse_episode_from_filename("Show_2_06.mkv"), Some(6));
    }

    #[test]
    fn ignores_parenthesized_copy_suffixes() {
        assert_eq!(parse_episode_from_filename("番剧 - 04 (1).mkv"), Some(4));
        assert_eq!(parse_episode_from_filename("番剧 - 04 (12).mkv"), Some(4));
        assert_eq!(
            parse_episode_from_filename("番剧 - 04 (1) [1080p].mkv"),
            Some(4)
        );
        assert_eq!(parse_episode_from_filename("番剧 (06).mkv"), Some(6));
        // 前面没有像样的集数时，括号里的数字仍可能是本集集数。
        assert_eq!(parse_episode_from_filename("Show 2 (6).mkv"), Some(6));
    }

    #[test]
    fn falls_back_for_single_number_but_not_for_season_only() {
        assert_eq!(parse_episode_from_filename("Show 2.mkv"), Some(2));
        assert_eq!(parse_episode_from_filename("Show 2 - 1080p.mkv"), Some(2));
        assert_eq!(parse_episode_from_filename("Show - 720.mkv"), Some(720));
        for name in [
            "Show Season 2.mkv",
            "Show Part 2 [1080P].mkv",
            "Show S2 [1080P].mkv",
            "Show 2nd Season.mkv",
            "Show 2季.mkv",
            "Show 第2季.mkv",
        ] {
            assert_eq!(parse_episode_from_filename(name), None, "{name}");
        }
    }

    #[test]
    fn parses_explicit_markers_with_season_in_title() {
        assert_eq!(parse_episode("Show Season 2 Episode 6"), Some(6));
        assert_eq!(parse_episode("Show.S02EP03.1080p"), Some(3));
        assert_eq!(parse_episode("Show 第06話"), Some(6));
        assert_eq!(parse_episode("Show Season 2 #6"), Some(6));
    }

    #[test]
    fn parses_long_running_four_digit_episodes() {
        assert_eq!(
            parse_episode_from_filename(
                "[咪路fans制作组]蜡笔小新 Crayonshinchan [1361][2026.11.14][1080P][GB_JP][MP4]"
            ),
            Some(1361)
        );
        assert_eq!(
            parse_episode_from_filename(
                "[丸子家族][櫻桃小丸子第二期(Chibi Maruko-chan II)][1546]小丸子...[2026.09.06][BIG5][1080P][MP4]"
            ),
            Some(1546)
        );
        assert_eq!(parse_episode("历史番 第1361话 [1080P]"), Some(1361));
        assert_eq!(
            parse_episode_from_filename("[Group] Show - 1361 [1080p]"),
            Some(1361)
        );
        assert_eq!(parse_episode_from_filename("[2026] Show - 06.mkv"), Some(6));
        assert_eq!(
            parse_episode_from_filename("Show 1920x1080 [1361].mkv"),
            Some(1361)
        );
        assert_eq!(parse_episode_from_filename("Show - 1080p [BIG5].mkv"), None);
        // `第30号杀人事件` 是特别篇标题，不是第 30 集。
        assert_eq!(parse_episode("名侦探柯南 第30号杀人事件 [2160p]"), None);
    }

    #[test]
    fn title_numbers_do_not_steal_episode() {
        assert_eq!(
            parse_episode_from_filename("[Group] 86 [01][1080p].mkv"),
            Some(1)
        );
        assert_eq!(
            parse_episode_from_filename("[Group] 86 - 01 [1080p].mkv"),
            Some(1)
        );
        assert_eq!(
            parse_episode_from_filename("[Group] Made in Abyss 2 - 06.mkv"),
            Some(6)
        );
        assert_eq!(
            parse_episode_from_filename("[Group] 無職轉生 第2季 - 06.mkv"),
            Some(6)
        );
        assert_eq!(
            parse_episode_from_filename("[Group] 無職轉生 第2季 第06話.mkv"),
            Some(6)
        );
    }

    #[test]
    fn parses_bracket_and_version_variants() {
        assert_eq!(
            parse_episode_from_filename("[Group] Show 2 [06v2][1080P].mkv"),
            Some(6)
        );
        assert_eq!(
            parse_episode_from_filename("[Group] [Show 2][06][1080P].mkv"),
            Some(6)
        );
        assert_eq!(
            parse_episode_from_filename("[VCB-Studio] Show - 06 [Ma10p_1080p][x265_flac].mkv"),
            Some(6)
        );
    }
}
