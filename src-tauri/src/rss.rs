//! RSS 抓取、解析和标题/集数匹配。

use std::sync::OnceLock;

use anyhow::{bail, Context, Result};
use chrono::{DateTime, NaiveDateTime};
use quick_xml::events::{BytesStart, Event};
use quick_xml::{Reader, XmlVersion};
use regex::Regex;
use serde::Serialize;

const USER_AGENT: &str = "KanFanOS/1.3 RSS";

#[derive(Debug, Clone)]
pub struct ParsedFeed {
    pub title: String,
    pub site: String,
    pub entries: Vec<ParsedEntry>,
}

#[derive(Debug, Clone)]
pub struct ParsedEntry {
    pub guid: String,
    pub title: String,
    pub link: String,
    pub download_url: String,
    pub published_at: i64,
}

#[derive(Debug, Clone, Copy)]
pub struct MatchScore {
    pub episode: Option<i64>,
    pub confidence: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ParsedReleaseTitle {
    pub raw: String,
    pub group: Option<String>,
    pub title: Option<String>,
    pub title_candidates: Vec<String>,
    pub episode: Option<i64>,
    pub resolution: Option<String>,
    pub source: Option<String>,
    pub subtitle: Option<String>,
    pub subtitle_packaging: Option<String>,
    pub version: Option<i64>,
    pub codec: Option<String>,
    pub container: Option<String>,
    pub confidence: i64,
}

pub async fn fetch(url: &str) -> Result<ParsedFeed> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .user_agent(USER_AGENT)
        .build()?;
    let response = client
        .get(url)
        .header(reqwest::header::ACCEPT, "application/rss+xml, application/atom+xml, application/xml, text/xml, */*")
        .send()
        .await
        .with_context(|| format!("请求 RSS 失败：{url}"))?;
    if !response.status().is_success() {
        bail!("RSS 返回 HTTP {}", response.status());
    }
    let body = response.text().await.context("读取 RSS 响应失败")?;
    let mut feed = parse(&body)?;
    if let Ok(base) = reqwest::Url::parse(url) {
        for entry in &mut feed.entries {
            entry.link = resolve_url(&base, &entry.link);
            entry.download_url = resolve_url(&base, &entry.download_url);
        }
    }
    Ok(feed)
}

/// 通过系统代理抓取 .torrent；qBittorrent 之后只接收文件，不需要自己联网抓链接。
pub async fn fetch_torrent(url: &str) -> Result<Vec<u8>> {
    const MAX_TORRENT_BYTES: usize = 32 * 1024 * 1024;
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .user_agent(USER_AGENT)
        .build()?;
    let response = client
        .get(url)
        .header(
            reqwest::header::ACCEPT,
            "application/x-bittorrent, application/octet-stream, */*",
        )
        .send()
        .await
        .with_context(|| format!("下载种子文件失败：{url}"))?;
    if !response.status().is_success() {
        bail!("下载种子文件返回 HTTP {}", response.status());
    }
    let bytes = response
        .bytes()
        .await
        .with_context(|| format!("读取种子文件失败：{url}"))?;
    if bytes.len() > MAX_TORRENT_BYTES {
        bail!("种子文件过大：{} 字节", bytes.len());
    }
    if !looks_like_torrent(&bytes) {
        bail!("返回内容不是有效的 .torrent 文件（可能是登录页或错误页）");
    }
    Ok(bytes.to_vec())
}

fn looks_like_torrent(bytes: &[u8]) -> bool {
    bytes.len() > 20
        && bytes.first() == Some(&b'd')
        && bytes.windows(6).any(|window| window == b"4:info")
}

fn resolve_url(base: &reqwest::Url, value: &str) -> String {
    let value = value.trim();
    if value.is_empty() || value.starts_with("magnet:") {
        return value.to_string();
    }
    base.join(value)
        .map(|url| url.to_string())
        .unwrap_or_else(|_| value.to_string())
}

pub fn parse(xml: &str) -> Result<ParsedFeed> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);

    let mut feed_title = String::new();
    let mut feed_site = String::new();
    let mut entries = Vec::new();
    let mut current: Option<ItemBuilder> = None;
    let mut text_tag: Option<String> = None;
    let mut text_buf = String::new();
    let mut in_item = false;

    loop {
        let event = reader.read_event().context("解析 RSS XML 失败")?;
        match event {
            Event::Start(event) => {
                let name = local_name_str(event.name().as_ref());
                if name == "item" || name == "entry" {
                    in_item = true;
                    current = Some(ItemBuilder::default());
                    text_tag = None;
                    text_buf.clear();
                    continue;
                }

                if in_item {
                    if let Some(builder) = current.as_mut() {
                        if name == "enclosure" || name == "link" {
                            if let Some(href) = attribute(&event, "href") {
                                if name == "enclosure" || is_download_url(&href) {
                                    builder.add_download(&href);
                                }
                                if name == "link" && !href.is_empty() && builder.link.is_empty() {
                                    builder.link = href;
                                }
                            }
                            if let Some(url) = attribute(&event, "url") {
                                builder.add_download(&url);
                            }
                        }
                        if matches!(
                            name.as_str(),
                            "title" | "link" | "guid" | "id" | "pubdate" | "published" | "updated"
                        ) {
                            text_tag = Some(name);
                            text_buf.clear();
                        }
                    }
                } else if name == "link" {
                    if let Some(href) = attribute(&event, "href") {
                        if feed_site.is_empty() {
                            feed_site = href;
                        }
                    }
                } else if name == "title" {
                    text_tag = Some("title".to_string());
                    text_buf.clear();
                }
            }
            Event::Empty(event) => {
                let name = local_name_str(event.name().as_ref());
                if in_item {
                    if let Some(builder) = current.as_mut()
                        && (name == "enclosure" || name == "link")
                    {
                        if let Some(href) = attribute(&event, "href") {
                            if name == "enclosure" || is_download_url(&href) {
                                builder.add_download(&href);
                            }
                            if name == "link" && !href.is_empty() && builder.link.is_empty() {
                                builder.link = href;
                            }
                        }
                        if let Some(url) = attribute(&event, "url") {
                            builder.add_download(&url);
                        }
                    }
                } else if name == "link"
                    && let Some(href) = attribute(&event, "href")
                    && feed_site.is_empty()
                {
                    feed_site = href;
                }
            }
            Event::Text(event) => {
                if text_tag.is_some() {
                    append_text(
                        &mut text_buf,
                        &event.xml_content(XmlVersion::Implicit1_0),
                    );
                }
            }
            Event::CData(event) => {
                if text_tag.is_some() {
                    append_text(
                        &mut text_buf,
                        &event.xml_content(XmlVersion::Implicit1_0),
                    );
                }
            }
            Event::End(event) => {
                let name = local_name_str(event.name().as_ref());
                if name == "item" || name == "entry" {
                    if let Some(builder) = current.take() {
                        if let Some(entry) = builder.finish() {
                            entries.push(entry);
                        }
                    }
                    in_item = false;
                    text_tag = None;
                    text_buf.clear();
                    continue;
                }

                if let Some(tag) = text_tag.take() {
                    let text = text_buf.trim().to_string();
                    text_buf.clear();
                    if in_item {
                        if let Some(builder) = current.as_mut() {
                            match tag.as_str() {
                                "title" => {
                                    if builder.title.is_empty() {
                                        builder.title = text.clone();
                                    }
                                    builder.add_download(&text);
                                }
                                "link" => {
                                    if !text.is_empty() {
                                        if builder.link.is_empty() {
                                            builder.link = text.clone();
                                        }
                                        if is_download_url(&text) {
                                            builder.add_download(&text);
                                        }
                                    }
                                }
                                "guid" | "id" => {
                                    if builder.guid.is_empty() {
                                        builder.guid = text.clone();
                                    }
                                    builder.add_download(&text);
                                }
                                "pubdate" | "published" | "updated" => {
                                    if builder.published_at == 0 {
                                        builder.published_at = parse_date(&text).unwrap_or(0);
                                    }
                                }
                                _ => {}
                            }
                        }
                    } else {
                        match tag.as_str() {
                            "title" => {
                                if feed_title.is_empty() {
                                    feed_title = text;
                                }
                            }
                            "link" => {
                                if feed_site.is_empty() {
                                    feed_site = text;
                                }
                            }
                            _ => {}
                        }
                    }
                }
            }
            Event::Eof => break,
            _ => {}
        }
    }

    if entries.is_empty() {
        bail!("RSS 中没有找到条目");
    }

    Ok(ParsedFeed {
        title: feed_title.trim().to_string(),
        site: feed_site.trim().to_string(),
        entries,
    })
}

#[derive(Default)]
struct ItemBuilder {
    guid: String,
    title: String,
    link: String,
    download_url: String,
    published_at: i64,
}

impl ItemBuilder {
    fn add_download(&mut self, value: &str) {
        let value = value.trim();
        if self.download_url.is_empty() && is_download_url(value) {
            self.download_url = value.to_string();
        }
    }

    fn finish(self) -> Option<ParsedEntry> {
        let title = self.title.trim().to_string();
        if title.is_empty() {
            return None;
        }
        let download_url = if self.download_url.is_empty() {
            self.link.trim().to_string()
        } else {
            self.download_url
        };
        let link = if self.link.is_empty() {
            download_url.clone()
        } else {
            self.link
        };
        let guid = if self.guid.is_empty() {
            format!("{link}|{title}")
        } else {
            self.guid
        };
        Some(ParsedEntry {
            guid,
            title,
            link,
            download_url,
            published_at: self.published_at,
        })
    }
}

fn local_name_str(name: &str) -> String {
    name.rsplit(':').next().unwrap_or(name).to_ascii_lowercase()
}

fn attribute(event: &BytesStart<'_>, key: &str) -> Option<String> {
    event
        .attributes()
        .flatten()
        .find(|attribute| attribute.key.as_ref().eq_ignore_ascii_case(key))
        .and_then(|attribute| {
            attribute
                .normalized_value(XmlVersion::Implicit1_0)
                .ok()
                .map(|value| value.into_owned())
        })
}

fn append_text(buffer: &mut String, value: &str) {
    if !buffer.is_empty() {
        buffer.push(' ');
    }
    buffer.push_str(value.trim());
}

fn is_download_url(value: &str) -> bool {
    let value = value.trim();
    value.starts_with("magnet:?")
        || value.starts_with("http://")
            && (value.contains(".torrent") || value.contains("download"))
        || value.starts_with("https://")
            && (value.contains(".torrent") || value.contains("download"))
}

fn parse_date(value: &str) -> Option<i64> {
    let value = value.trim();
    if value.is_empty() {
        return None;
    }
    if let Ok(date) = DateTime::parse_from_rfc2822(value) {
        return Some(date.timestamp_millis());
    }
    if let Ok(date) = DateTime::parse_from_rfc3339(value) {
        return Some(date.timestamp_millis());
    }
    for format in ["%Y-%m-%d %H:%M:%S", "%Y/%m/%d %H:%M:%S", "%Y-%m-%d"] {
        if let Ok(date) = NaiveDateTime::parse_from_str(value, format) {
            return Some(date.and_utc().timestamp_millis());
        }
    }
    None
}

/// 判断一条标题是否匹配追番，并解析集数/分辨率。
pub fn score_release(
    follow_title: &str,
    aliases: &[String],
    preferred_resolution: &str,
    release_title: &str,
) -> Option<MatchScore> {
    if is_batch(release_title) {
        return None;
    }

    let release = normalize(release_title);
    if release.is_empty() {
        return None;
    }

    let mut matched = false;
    for title in std::iter::once(follow_title).chain(aliases.iter().map(String::as_str)) {
        let normalized = normalize(title);
        if normalized.len() >= 2 && release.contains(&normalized) {
            matched = true;
            break;
        }
    }
    if !matched {
        return None;
    }

    let episode = parse_episode(release_title);
    let resolution = parse_resolution(release_title);
    let preferred = preferred_resolution.to_ascii_lowercase();
    let resolution_bonus = if resolution.as_deref() == Some(preferred.as_str()) {
        20
    } else if resolution.is_some() {
        8
    } else {
        0
    };
    let confidence = 55 + if episode.is_some() { 25 } else { 0 } + resolution_bonus;
    Some(MatchScore {
        episode,
        confidence,
    })
}

/// 发布标题开头的 `[组名]` / `【组名】` 通常是字幕组。
pub fn release_group(title: &str) -> Option<String> {
    let title = title.trim_start();
    let mut chars = title.chars();
    let open = chars.next()?;
    let close = match open {
        '[' => ']',
        '【' => '】',
        '（' => '）',
        '(' => ')',
        _ => return None,
    };
    let end = title[open.len_utf8()..].find(close)?;
    let group = title[open.len_utf8()..open.len_utf8() + end].trim();
    if group.is_empty() {
        None
    } else {
        Some(group.to_string())
    }
}

/// 返回字幕组在用户优先级列表中的位置；未命中或没有组信息时返回 `usize::MAX`。
pub fn preferred_group_rank(preferred: &[String], title: &str) -> usize {
    let Some(group) = release_group(title) else {
        return usize::MAX;
    };
    let group = normalize(&group);
    if group.is_empty() {
        return usize::MAX;
    }
    preferred
        .iter()
        .position(|candidate| !candidate.trim().is_empty() && normalize(candidate) == group)
        .unwrap_or(usize::MAX)
}

pub fn subtitle_packaging_matches(expected: &str, title: &str) -> bool {
    let expected = normalize(expected);
    if expected.is_empty() {
        return true;
    }
    release_subtitle_packaging(title)
        .map(|packaging| normalize(&packaging) == expected)
        .unwrap_or(false)
}

pub fn release_subtitle_packaging(title: &str) -> Option<String> {
    bracket_tokens(title)
        .into_iter()
        .find(|token| is_packaging_token(token))
}

fn is_packaging_token(token: &str) -> bool {
    token.contains("内封")
        || token.contains("內封")
        || token.contains("外挂")
        || token.contains("外掛")
        || token.contains("内嵌")
        || token.contains("內嵌")
        || token.contains("封装")
        || token.contains("封裝")
}

/// 把用户选中的一串发布标题拆成可编辑字段。
pub fn parse_release_title(text: &str) -> ParsedReleaseTitle {
    let raw = select_release_line(text);
    let group = release_group(&raw);
    let rest = strip_leading_group(&raw);
    let found_episode = find_episode(&rest);
    let episode = found_episode.map(|(episode, _)| episode);
    let title_region = match found_episode {
        Some((_, start)) => trim_title_tail(&rest[..start]),
        None => trim_title_region(&rest),
    };
    let mut title_candidates = split_title_candidates(title_region);
    let mut title = pick_title(&title_candidates);
    if title.is_none() && !title_region.trim().is_empty() {
        let fallback = title_region.trim().to_string();
        title_candidates.push(fallback.clone());
        title = Some(fallback);
    }

    let tokens = bracket_tokens(&raw);
    let resolution = parse_resolution(&raw);
    let source = classify_tokens(&tokens, group.as_deref(), |token| {
        token.contains("baha")
            || token.contains("web-dl")
            || token.contains("webrip")
            || token == "web"
            || token.contains("abema")
            || token == "cr"
            || token.contains("b-global")
            || token == "tv"
            || token == "bd"
            || token.contains("blu-ray")
            || token == "dvd"
    });
    let subtitle_packaging = classify_tokens(&tokens, group.as_deref(), is_packaging_token);
    let subtitle = classify_tokens(&tokens, group.as_deref(), |token| {
        !is_packaging_token(token)
            && (token.contains("cht")
                || token.contains("chs")
                || token == "gb"
                || token == "big5"
                || token.contains('简')
                || token.contains('繁')
                || token.contains("字幕"))
    });
    let codec = classify_tokens(&tokens, group.as_deref(), |token| {
        token.contains("avc")
            || token.contains("hevc")
            || token.contains("x264")
            || token.contains("x265")
            || token.contains("aac")
            || token.contains("flac")
            || token.contains("opus")
    });
    let container = classify_tokens(&tokens, group.as_deref(), |token| {
        matches!(token, "mp4" | "mkv" | "avi" | "ts" | "flv")
    });
    let version = version_number(&raw);

    let mut confidence = 0;
    if group.is_some() {
        confidence += 15;
    }
    if title.is_some() {
        confidence += 30;
    }
    if episode.is_some() {
        confidence += 25;
    }
    if resolution.is_some() {
        confidence += 15;
    }
    if source.is_some() {
        confidence += 5;
    }
    if subtitle.is_some() {
        confidence += 5;
    }
    if subtitle_packaging.is_some() {
        confidence += 5;
    }
    if version.is_some() {
        confidence += 5;
    }

    ParsedReleaseTitle {
        raw,
        group,
        title,
        title_candidates,
        episode,
        resolution,
        source,
        subtitle,
        subtitle_packaging,
        version,
        codec,
        container,
        confidence: confidence.min(100),
    }
}

fn select_release_line(text: &str) -> String {
    let lines: Vec<&str> = text
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect();
    let selected = lines
        .into_iter()
        .max_by_key(|line| {
            let brackets = line.matches('[').count() + line.matches('【').count();
            (find_episode(line).is_some(), brackets, line.len())
        })
        .unwrap_or_else(|| text.trim());
    selected.to_string()
}

fn strip_leading_group(value: &str) -> &str {
    let value = value.trim_start();
    let Some(open) = value.chars().next() else {
        return value;
    };
    let close = match open {
        '[' => ']',
        '【' => '】',
        '（' => '）',
        '(' => ')',
        _ => return value,
    };
    let Some(end) = value[open.len_utf8()..].find(close) else {
        return value;
    };
    &value[open.len_utf8() + end + close.len_utf8()..].trim_start()
}

fn trim_title_region(value: &str) -> &str {
    let cut = value
        .char_indices()
        .find(|(_, ch)| matches!(ch, '[' | '【' | '(' | '（'))
        .map(|(index, _)| index)
        .unwrap_or(value.len());
    trim_title_tail(&value[..cut])
}

fn trim_title_tail(value: &str) -> &str {
    value.trim().trim_end_matches(|ch: char| {
        matches!(ch, '-' | '–' | '—' | '_' | '~' | '/' | '|' | '｜')
    })
}

fn split_title_candidates(value: &str) -> Vec<String> {
    static SPLIT: OnceLock<Regex> = OnceLock::new();
    let regex = SPLIT.get_or_init(|| Regex::new(r"\s+[/|｜]\s+").unwrap());
    let mut candidates = Vec::new();
    for part in regex.split(value) {
        let candidate = trim_title_tail(part.trim());
        let candidate = candidate.strip_prefix('[').unwrap_or(candidate);
        let mut candidate = candidate.strip_suffix(']').unwrap_or(candidate).trim();
        if let Some(group) = release_group(candidate) {
            if candidate.starts_with(&format!("[{group}]")) || candidate.starts_with(&format!("【{group}】"))
            {
                candidate = strip_leading_group(candidate);
            }
        }
        let candidate = candidate.trim().to_string();
        if !candidate.is_empty() && !candidates.contains(&candidate) {
            candidates.push(candidate);
        }
    }
    candidates
}

fn pick_title(candidates: &[String]) -> Option<String> {
    candidates
        .iter()
        .find(|candidate| contains_cjk(candidate))
        .cloned()
        .or_else(|| candidates.first().cloned())
}

fn contains_cjk(value: &str) -> bool {
    value
        .chars()
        .any(|ch| ('\u{3400}'..='\u{9fff}').contains(&ch))
}

fn bracket_tokens(value: &str) -> Vec<String> {
    static TOKEN: OnceLock<Regex> = OnceLock::new();
    let regex = TOKEN.get_or_init(|| Regex::new(r"[\[\(【]([^\]\)】]+)[\]\)】]").unwrap());
    regex
        .captures_iter(value)
        .filter_map(|captures| captures.get(1).map(|group| group.as_str().trim().to_string()))
        .filter(|token| !token.is_empty())
        .collect()
}

fn classify_tokens<F>(tokens: &[String], group: Option<&str>, predicate: F) -> Option<String>
where
    F: Fn(&str) -> bool,
{
    let group_key = group.map(normalize);
    let mut values = Vec::new();
    for token in tokens {
        let token_key = normalize(token);
        if group_key.as_deref() == Some(token_key.as_str()) {
            continue;
        }
        let parts: Vec<&str> = if token.chars().any(char::is_whitespace) {
            token.split_whitespace().collect()
        } else {
            vec![token.as_str()]
        };
        for part in parts {
            let lower = part.to_ascii_lowercase();
            if predicate(&lower) && !values.iter().any(|value: &String| value == part) {
                values.push(part.to_string());
            }
        }
    }
    if values.is_empty() {
        None
    } else {
        Some(values.join(" / "))
    }
}

fn version_number(value: &str) -> Option<i64> {
    static VERSION: OnceLock<Regex> = OnceLock::new();
    let regex = VERSION.get_or_init(|| Regex::new(r"(?i)\bv(\d+)\b").unwrap());
    regex
        .captures_iter(value)
        .filter_map(|captures| captures.get(1))
        .filter_map(|capture| capture.as_str().parse::<i64>().ok())
        .max()
}

fn normalize(value: &str) -> String {
    value
        .chars()
        .filter(|ch| ch.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

fn episode_patterns() -> &'static [Regex] {
    static PATTERNS: OnceLock<Vec<Regex>> = OnceLock::new();
    PATTERNS.get_or_init(|| {
        [
            r"(?i)(?:s\d{1,2}e|ep?)\s*0*(\d{1,3})(?:v\d+)?",
            r"第\s*0*(\d{1,3})\s*[话話集]",
            r"(?:^|[\s\[\]\(\)【】_-])0*(\d{1,3})(?:v\d+)?(?:v\d+)?(?:[\s\[\]\(\)【】]|$)",
            r"[\[\(]\s*0*(\d{1,3})(?:v\d+)?\s*[\]\)]",
        ]
        .iter()
        .filter_map(|pattern| Regex::new(pattern).ok())
        .collect()
    })
}

fn find_episode(title: &str) -> Option<(i64, usize)> {
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

fn parse_episode(title: &str) -> Option<i64> {
    find_episode(title).map(|(episode, _)| episode)
}

fn parse_resolution(title: &str) -> Option<String> {
    static RESOLUTION_P: OnceLock<Regex> = OnceLock::new();
    static RESOLUTION_DIM: OnceLock<Regex> = OnceLock::new();
    let explicit = RESOLUTION_P.get_or_init(|| Regex::new(r"(?i)\b(2160|1080|720|480)p\b").unwrap());
    if let Some(value) = explicit
        .captures(title)
        .and_then(|captures| captures.get(1))
    {
        return Some(format!("{}p", value.as_str()));
    }

    let dimensions =
        RESOLUTION_DIM.get_or_init(|| Regex::new(r"(?i)\b\d{3,4}\s*[x×]\s*(\d{3,4})\b").unwrap());
    let height = dimensions
        .captures(title)
        .and_then(|captures| captures.get(1))
        .and_then(|value| value.as_str().parse::<u32>().ok())?;
    if matches!(height, 2160 | 1080 | 720 | 480) {
        Some(format!("{height}p"))
    } else {
        None
    }
}

fn is_batch(title: &str) -> bool {
    static BATCH: OnceLock<Regex> = OnceLock::new();
    let regex = BATCH.get_or_init(|| {
        Regex::new(
            r"(?i)(合集|batch|全集|全\s*\d{1,3}\s*[话話集]|\[\s*\d{1,3}\s*[-~]\s*\d{1,3}\s*\]|(?:^|\s)\d{1,3}\s*[-~]\s*\d{1,3}(?:\s|$))",
        )
        .unwrap()
    });
    regex.is_match(title)
}

/// 把用户输入的别名（逗号/换行分隔）整理成字符串列表。
pub fn split_aliases(value: &str) -> Vec<String> {
    value
        .split(|ch| ch == ',' || ch == '，' || ch == '\n')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{
        looks_like_torrent, parse, parse_release_title, preferred_group_rank, release_group,
        score_release,
    };

    #[test]
    fn recognizes_torrent_payloads() {
        assert!(looks_like_torrent(
            b"d8:announce14:http://tracker4:infod4:name4:testee"
        ));
        assert!(!looks_like_torrent(b"<!doctype html><html>login</html>"));
    }

    #[test]
    fn extracts_and_ranks_release_groups() {
        assert_eq!(
            release_group("[ANi] 测试番 - 04 [1080P]").as_deref(),
            Some("ANi")
        );
        assert_eq!(
            release_group("【喵萌奶茶屋】测试番 - 04").as_deref(),
            Some("喵萌奶茶屋")
        );
        assert_eq!(release_group("测试番 - 04"), None);

        let preferred = vec!["黒ネズミたち".to_string(), "ANi".to_string()];
        assert_eq!(
            preferred_group_rank(&preferred, "[ANi] 测试番 - 04 [1080P]"),
            1
        );
        assert_eq!(
            preferred_group_rank(&preferred, "[黒ネズミたち] 测试番 - 04"),
            0
        );
        assert_eq!(
            preferred_group_rank(&preferred, "[其它组] 测试番 - 04"),
            usize::MAX
        );
    }

    #[test]
    fn parses_bracketed_bilingual_title_and_packaging() {
        let parsed = parse_release_title(
            "【悠哈璃羽字幕社】[FX战士久留美 / FX Senshi Kurumi-chan][04][1080p HEVC][CHS_JP&CHT_JP][简繁日内封]",
        );
        assert_eq!(parsed.group.as_deref(), Some("悠哈璃羽字幕社"));
        assert_eq!(parsed.title.as_deref(), Some("FX战士久留美"));
        assert!(parsed
            .title_candidates
            .contains(&"FX Senshi Kurumi-chan".to_string()));
        assert_eq!(parsed.episode, Some(4));
        assert_eq!(parsed.resolution.as_deref(), Some("1080p"));
        assert_eq!(parsed.subtitle_packaging.as_deref(), Some("简繁日内封"));
        assert!(parsed
            .subtitle
            .as_deref()
            .unwrap_or_default()
            .contains("CHS_JP&CHT_JP"));
        assert!(super::subtitle_packaging_matches("简繁日内封", &parsed.raw));
        assert!(!super::subtitle_packaging_matches("内封", &parsed.raw));
    }

    #[test]
    fn parses_release_title_fields() {
        let parsed = parse_release_title(
            "[ANi] Seitokai ni mo Ana wa Aru / 學生會也有洞！ - 04 [1080P][Baha][WEB-DL][AAC AVC][CHT][V2][MP4]",
        );
        assert_eq!(parsed.group.as_deref(), Some("ANi"));
        assert_eq!(parsed.title.as_deref(), Some("學生會也有洞！"));
        assert!(parsed
            .title_candidates
            .contains(&"Seitokai ni mo Ana wa Aru".to_string()));
        assert_eq!(parsed.episode, Some(4));
        assert_eq!(parsed.resolution.as_deref(), Some("1080p"));
        assert_eq!(parsed.source.as_deref(), Some("Baha / WEB-DL"));
        assert_eq!(parsed.subtitle.as_deref(), Some("CHT"));
        assert_eq!(parsed.version, Some(2));
        assert_eq!(parsed.container.as_deref(), Some("MP4"));

        let black = parse_release_title(
            "[黒ネズミたち] 學生會也有洞！ / Seitokai ni mo Ana wa Aru! - 04 (ABEMA 1280x720 AVC AAC MKV)",
        );
        assert_eq!(black.group.as_deref(), Some("黒ネズミたち"));
        assert_eq!(black.resolution.as_deref(), Some("720p"));
        assert_eq!(black.container.as_deref(), Some("MKV"));
    }

    #[test]
    fn parses_rss_items_and_enclosures() {
        let xml = r#"<?xml version="1.0"?>
        <rss version="2.0"><channel>
          <title>测试源</title>
          <link>https://example.com/</link>
          <item>
            <title>[字幕组] 测试番 - 03 [1080p]</title>
            <link>https://example.com/3</link>
            <guid>ep-3</guid>
            <enclosure url="https://example.com/3.torrent" type="application/x-bittorrent"/>
            <pubDate>Fri, 24 Feb 2023 14:20:16 +0000</pubDate>
          </item>
        </channel></rss>"#;
        let feed = parse(xml).unwrap();
        assert_eq!(feed.title, "测试源");
        assert_eq!(feed.entries.len(), 1);
        assert_eq!(feed.entries[0].title, "[字幕组] 测试番 - 03 [1080p]");
        assert_eq!(
            feed.entries[0].download_url,
            "https://example.com/3.torrent"
        );
        assert!(feed.entries[0].published_at > 0);
    }

    #[test]
    fn parses_atom_links_and_magnet() {
        let xml = r#"<feed xmlns="http://www.w3.org/2005/Atom">
          <title>Atom 源</title>
          <entry>
            <title>测试番 第4话</title>
            <link href="https://example.com/4"/>
            <id>ep-4</id>
            <updated>2026-01-01T00:00:00Z</updated>
          </entry>
        </feed>"#;
        let feed = parse(xml).unwrap();
        assert_eq!(feed.entries[0].link, "https://example.com/4");
    }

    #[test]
    fn scores_title_and_episode() {
        let score = score_release(
            "测试番",
            &["测试番第二季".to_string()],
            "1080p",
            "[字幕组] 测试番 - 03 [1080p]",
        )
        .unwrap();
        assert_eq!(score.episode, Some(3));
        assert!(score.confidence >= 90);

        assert!(score_release("测试番", &[], "1080p", "其他番 - 03 [1080p]").is_none());
        assert!(score_release("测试番", &[], "1080p", "测试番 合集 01-12").is_none());
    }
}
