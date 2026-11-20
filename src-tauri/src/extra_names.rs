//! 「其他视频」的展示分类。
//!
//! 只根据文件名和相对路径识别特典类型，供 `kind = "extra"` 的视频显示徽章
//! 和按类型排序。不会修改 `videos.kind`，也不参与扫描或 RSS 的任何判断。

/// 其他视频的展示分类。相近类型合并成一个标签。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExtraKind {
    Ova,
    Sp,
    Nc,
    OpEd,
    PvCm,
    Extra,
}

impl ExtraKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Ova => "OVA",
            Self::Sp => "SP",
            Self::Nc => "NCOP/NCED",
            Self::OpEd => "OP/ED",
            Self::PvCm => "PV/CM",
            Self::Extra => "EXTRA",
        }
    }
}

/// 识别一个 `kind = "extra"` 的视频。
///
/// `relative_path` 是相对番剧目录的路径，目录名也参与判断；
/// `filename` 在相对路径退化成绝对路径时作为兜底。
/// `known_titles` 是番剧显示名、别名和文件夹名，用来避免
/// `SPY×FAMILY` / `EDENS ZERO` 这类标题被误判成 SP/ED。
pub fn classify(relative_path: &str, filename: &str, known_titles: &[&str]) -> Option<ExtraKind> {
    // 相对路径解析失败时会退化成绝对路径，此时只信文件名，
    // 避免把盘符或上级目录名当成类型。
    let source = if relative_path.is_empty()
        || relative_path.contains(':')
        || relative_path.starts_with('/')
        || relative_path.starts_with('\\')
    {
        filename
    } else {
        relative_path
    };

    let normalized = normalize(source);
    let text = strip_known_titles(&normalized, known_titles);
    let tokens: Vec<&str> = text.split_whitespace().collect();
    if tokens.is_empty() {
        return None;
    }

    if is_nc(&tokens, &text) {
        Some(ExtraKind::Nc)
    } else if is_ova(&tokens) {
        Some(ExtraKind::Ova)
    } else if is_sp(&tokens, &text) {
        Some(ExtraKind::Sp)
    } else if is_op_ed(&tokens, &text) {
        Some(ExtraKind::OpEd)
    } else if is_pv_cm(&tokens, &text) {
        Some(ExtraKind::PvCm)
    } else if is_extra(&tokens, &text) {
        Some(ExtraKind::Extra)
    } else {
        None
    }
}

/// 统一大小写、全角字符和分隔符。
fn normalize(value: &str) -> String {
    let mut output = String::with_capacity(value.len());
    let mut pending_space = false;

    for ch in value.chars() {
        let ch = fullwidth_to_ascii(ch);
        // 平假名长音符等在部分 Unicode 版本里不是 alphanumeric，
        // 单独保留，避免破坏「オープニング」这类日文关键词。
        if ch.is_alphanumeric() || ch == '\u{30FC}' {
            if pending_space && !output.is_empty() {
                output.push(' ');
            }
            pending_space = false;
            for lower in ch.to_lowercase() {
                output.push(lower);
            }
        } else {
            pending_space = true;
        }
    }

    output
}

fn fullwidth_to_ascii(ch: char) -> char {
    match ch {
        '\u{3000}' => ' ',
        '\u{FF01}'..='\u{FF5E}' => char::from_u32(ch as u32 - 0xFEE0).unwrap_or(ch),
        _ => ch,
    }
}

/// 从归一化文本里删掉已知番剧名/别名。
///
/// 先删更长的标题：`Show OVA` 必须比 `Show` 先删，否则
/// `Show OVA - 01` 会被删成 `OVA - 01`，误判成 OVA。
fn strip_known_titles(text: &str, known_titles: &[&str]) -> String {
    let mut titles: Vec<Vec<String>> = known_titles
        .iter()
        .filter_map(|title| {
            let normalized = normalize(title);
            if normalized.chars().count() < 2 {
                return None;
            }
            let tokens: Vec<String> = normalized.split_whitespace().map(str::to_string).collect();
            (!tokens.is_empty()).then_some(tokens)
        })
        .collect();

    titles.sort_by_key(|tokens| {
        std::cmp::Reverse(
            tokens
                .iter()
                .map(|token| token.chars().count())
                .sum::<usize>(),
        )
    });

    let mut tokens: Vec<&str> = text.split_whitespace().collect();
    for title_tokens in &titles {
        let mut index = 0;
        while index + title_tokens.len() <= tokens.len() {
            let matched = tokens[index..index + title_tokens.len()]
                .iter()
                .zip(title_tokens.iter())
                .all(|(left, right)| *left == right.as_str());
            if matched {
                tokens.drain(index..index + title_tokens.len());
            } else {
                index += 1;
            }
        }
    }

    tokens.join(" ")
}

/// 完整词元匹配，允许复数 `s` 和数字后缀。
///
/// `sp1` / `specials` / `ncop2` 命中；`spy` / `edens` / `opening`
///（由单独的 `opening` 关键词处理）不会被 `sp` / `ed` 误命中。
fn matches_word(token: &str, word: &str) -> bool {
    if token == word {
        return true;
    }
    let Some(rest) = token.strip_prefix(word) else {
        return false;
    };
    rest == "s" || has_numeric_suffix(rest)
}

/// `01`、`v01`、`01v2` 这类数字/版本后缀。
///
/// 发布组经常写 `NCOPv01`、`SP01v2`、`NCED04_EP12`，不能只接受纯数字。
fn has_numeric_suffix(rest: &str) -> bool {
    if rest.is_empty() {
        return false;
    }

    let mut parts = rest.split('v');
    let first = parts.next().unwrap_or_default();
    let second = parts.next();
    if parts.next().is_some() {
        return false;
    }

    let digits = |value: &str| !value.is_empty() && value.chars().all(|ch| ch.is_ascii_digit());

    match (first.is_empty(), second) {
        (true, Some(version)) => digits(version),
        (false, None) => digits(first),
        (false, Some(version)) => digits(first) && digits(version),
        (true, None) => false,
    }
}

fn is_nc(tokens: &[&str], text: &str) -> bool {
    tokens.iter().any(|token| {
        matches_word(token, "ncop")
            || matches_word(token, "nced")
            || matches_word(token, "ncoped")
            || *token == "nc"
            || token.starts_with("noncredit")
            || token.starts_with("creditless")
            || token.starts_with("textless")
    }) || tokens
        .windows(2)
        .any(|window| window[0] == "nc" && matches!(window[1], "op" | "ed"))
        || tokens
            .windows(2)
            .any(|window| window[0] == "non" && window[1] == "credit")
        || text.contains("ノンクレジット")
        || text.contains("无字幕")
        || text.contains("無字幕")
}

fn is_ova(tokens: &[&str]) -> bool {
    tokens.iter().any(|token| {
        matches_word(token, "ova") || matches_word(token, "oad") || matches_word(token, "ona")
    })
}

fn is_sp(tokens: &[&str], text: &str) -> bool {
    tokens.iter().any(|token| {
        matches_word(token, "sp")
            || matches_word(token, "special")
            || matches_word(token, "tvsp")
    }) || text.contains("特别篇")
        || text.contains("特別篇")
        || text.contains("特别节目")
        || text.contains("特別節目")
        || text.contains("番外")
        || text.contains("小剧场")
        || text.contains("小劇場")
        || text.contains("迷你")
        || text.contains("短篇")
        || text.contains("总集篇")
        || text.contains("總集篇")
        || text.contains("特番")
        || text.contains("ミニアニメ")
}

fn is_op_ed(tokens: &[&str], text: &str) -> bool {
    tokens.iter().any(|token| {
        matches_word(token, "op")
            || matches_word(token, "ed")
            || matches_word(token, "opening")
            || matches_word(token, "ending")
    }) || text.contains("片头")
        || text.contains("片頭")
        || text.contains("片尾")
        || text.contains("主题曲")
        || text.contains("主題曲")
        || text.contains("オープニング")
        || text.contains("エンディング")
}

fn is_pv_cm(tokens: &[&str], text: &str) -> bool {
    tokens.iter().any(|token| {
        matches_word(token, "pv")
            || matches_word(token, "cm")
            || matches_word(token, "preview")
            || matches_word(token, "trailer")
            || matches_word(token, "teaser")
            || matches_word(token, "commercial")
    }) || text.contains("预告")
        || text.contains("預告")
        || text.contains("予告")
        || text.contains("广告")
        || text.contains("廣告")
        || text.contains("宣传")
        || text.contains("宣傳")
}

fn is_extra(tokens: &[&str], text: &str) -> bool {
    tokens.iter().any(|token| {
        matches_word(token, "extra")
            || matches_word(token, "bonus")
            || matches_word(token, "menu")
            || matches_word(token, "making")
            || matches_word(token, "interview")
            || matches_word(token, "commentary")
            || matches_word(token, "tokuten")
    }) || tokens
        .windows(2)
        .any(|window| window[0] == "picture" && window[1] == "drama")
        || tokens
            .windows(3)
            .any(|window| window[0] == "behind" && window[1] == "the" && window[2] == "scenes")
        || text.contains("特典")
        || text.contains("菜单")
        || text.contains("菜單")
        || text.contains("花絮")
        || text.contains("访谈")
        || text.contains("訪談")
        || text.contains("声优")
        || text.contains("聲優")
        || text.contains("声優")
        || text.contains("トークショー")
        || text.contains("幕后")
        || text.contains("幕後")
        || text.contains("评论")
        || text.contains("評論")
        || text.contains("ピクチャードラマ")
        || text.contains("メニュー")
        || text.contains("インタビュー")
        || text.contains("メイキング")
        || text.contains("コメント")
}

#[cfg(test)]
mod tests {
    use super::{ExtraKind, classify};

    fn label(relative: &str, titles: &[&str]) -> Option<&'static str> {
        classify(relative, relative, titles).map(ExtraKind::label)
    }

    #[test]
    fn classifies_nc_variants() {
        for name in [
            "Show - NCOP [1080p].mkv",
            "Show - NCOP1.mkv",
            "Show - NCED2 [1080p].mkv",
            "Show - NC-OP.mkv",
            "Show - NC ED.mkv",
            "Show - Non-Credit OP.mkv",
            "Show - Non-Credit ED.mkv",
            "Show - Creditless OP.mkv",
            "Show - Textless ED.mkv",
            "Show - 无字幕OP.mkv",
            "Show - 無字幕片尾.mkv",
            "Show - ノンクレジットOP.mkv",
        ] {
            assert_eq!(label(name, &["Show"]), Some("NCOP/NCED"), "{name}");
        }
    }

    #[test]
    fn classifies_ova_sp_and_theme_variants() {
        for name in [
            "Show - OVA 01 [1080p].mkv",
            "Show - OVAs.mkv",
            "Show - OAD1.mkv",
            "Show - ONA.mkv",
        ] {
            assert_eq!(label(name, &["Show"]), Some("OVA"), "{name}");
        }

        for name in [
            "Show - SP1 [1080p].mkv",
            "Show - Special.mkv",
            "Show - Specials.mkv",
            "Show - 特别篇.mkv",
            "Show - 特別篇.mkv",
            "Show - 番外篇.mkv",
            "Show - 小剧场01.mkv",
            "Show - 总集篇.mkv",
        ] {
            assert_eq!(label(name, &["Show"]), Some("SP"), "{name}");
        }

        for name in [
            "Show - OP1.mkv",
            "Show - ED.mkv",
            "Show - Opening.mkv",
            "Show - Ending.mkv",
            "Show - 片头.mkv",
            "Show - 主题曲.mkv",
            "Show - オープニング.mkv",
        ] {
            assert_eq!(label(name, &["Show"]), Some("OP/ED"), "{name}");
        }
    }

    #[test]
    fn classifies_promotional_and_bonus_variants() {
        for name in [
            "Show - PV01.mkv",
            "Show - CM 03.mkv",
            "Show - Preview.mkv",
            "Show - Trailer.mkv",
            "Show - Teaser.mkv",
            "Show - Commercial.mkv",
            "Show - 预告.mkv",
            "Show - 予告編.mkv",
            "Show - 广告.mkv",
        ] {
            assert_eq!(label(name, &["Show"]), Some("PV/CM"), "{name}");
        }

        for name in [
            "Show - 特典映像 01.mkv",
            "Show - 映像特典.mkv",
            "Show - Menu01.mkv",
            "Show - Bonus.mkv",
            "Show - Extra.mkv",
            "Show - Picture Drama.mkv",
            "Show - Making.mkv",
            "Show - Interview.mkv",
            "Show - 花絮.mkv",
            "Show - 访谈.mkv",
            "Show - ピクチャードラマ.mkv",
        ] {
            assert_eq!(label(name, &["Show"]), Some("EXTRA"), "{name}");
        }
    }

    /// 真实发布组里常见的文件名：VCB-Studio、Judas、Doki、DB、DBD-Raws 等。
    #[test]
    fn classifies_real_release_filenames() {
        let cases: &[(&str, Option<&str>)] = &[
            ("[Judas] Servamp - NCOP 01.mkv", Some("NCOP/NCED")),
            ("[Judas] Endro! - S1 NCED.mkv", Some("NCOP/NCED")),
            ("[Judas] Upotte!! - OVA.mkv", Some("OVA")),
            (
                "[Judas] Hikaru no Go - New Year Special.mkv",
                Some("SP"),
            ),
            (
                "[Doki] Sankarea - NCOP (1920x1080 Hi10P BD FLAC) [5687B33E].mkv",
                Some("NCOP/NCED"),
            ),
            (
                "[DB] Kage no Jitsuryokusha ni Naritakute!_-_NCOPv01_(10bit_BD1080p_x265).mkv",
                Some("NCOP/NCED"),
            ),
            (
                "[Eivind]_Dragon_Ball_Kai_-_NCOP-1_(BD_1440x1080_x264_FLAC).mkv",
                Some("NCOP/NCED"),
            ),
            ("NCOP01.mkv", Some("NCOP/NCED")),
            ("NCED01.mkv", Some("NCOP/NCED")),
            ("anime/ncop01.mkv", Some("NCOP/NCED")),
            (
                "[VCB-Studio] Death Billiards [OVA][Ma10p_1080p][x265_flac].mkv",
                Some("OVA"),
            ),
            (
                "[VCB-Studio] High School of the Dead [OVA][Ma10p_1080p][x265_flac].mkv",
                Some("OVA"),
            ),
            (
                "[VCB-Studio] Ore wo Suki nano wa Omae dake ka yo [13(OVA)][Ma10p_1080p][x265_flac].mkv",
                Some("OVA"),
            ),
            (
                "[VCB-Studio] Sword Art Online Extra Edition [TVSP][Ma10p_1080p][x265_flac].mkv",
                Some("SP"),
            ),
            (
                "[DMG][Yuragi-sou no Yuuna-san][OVA][1080P_Ma10P][HEVC_AAC].mkv",
                Some("OVA"),
            ),
            (
                "[北宇治字幕组] LoveLive！虹咲学园学园偶像同好会 Next Sky [OVA][BDRip][1080P Ma10p_Flac][简繁外挂].mkv",
                Some("OVA"),
            ),
            (
                "[VCB-Studio] Tamako Market [CM01][Ma10p_1080p][x265_flac].mkv",
                Some("PV/CM"),
            ),
            (
                "[VCB-Studio] Tamako Market [PV][Ma10p_1080p][x265_flac].mkv",
                Some("PV/CM"),
            ),
            (
                "[VCB-Studio] Tamako Market [PV01][Ma10p_1080p][x265_flac].mkv",
                Some("PV/CM"),
            ),
            (
                "[VCB-Studio] One-Punch Man S2 [Menu01][Ma10p_1080p][x265_flac].mkv",
                Some("EXTRA"),
            ),
            (
                "[VCB-Studio] One-Punch Man S2 [NCOP][Ma10p_1080p][x265_flac].mkv",
                Some("NCOP/NCED"),
            ),
            (
                "[VCB-Studio] Some Show [NCED02][Ma10p_1080p][x265_flac].mkv",
                Some("NCOP/NCED"),
            ),
            (
                "[VCB-Studio] Some Show [NCOPv01][Ma10p_1080p][x265_flac].mkv",
                Some("NCOP/NCED"),
            ),
            (
                "[VCB-Studio] Some Show [NCOP01_1][Ma10p_1080p][x265_flac].mkv",
                Some("NCOP/NCED"),
            ),
            (
                "[VCB-Studio] Some Show [NCED04_EP12][Ma10p_1080p][x265_flac].mkv",
                Some("NCOP/NCED"),
            ),
            (
                "[VCB-Studio] Some Show [NCoped][Ma10p_1080p][x265_flac].mkv",
                Some("NCOP/NCED"),
            ),
            (
                "[Kamigami&Mabors&VCB-Studio] GIRLS und PANZER der FILM [SP01v2][Ma10p_1080p][x265_flac].mkv",
                Some("SP"),
            ),
            (
                "[Hakugetsu&VCB-Studio] No Game No Life [SP03][Hi10p_1080p][x264_flac].mkv",
                Some("SP"),
            ),
            (
                "[DMG&VCB-Studio] Aquatope of White Sand/SPs/[DMG&VCB-Studio] Aquatope of White Sand [Job Experience 02][Ma10p_1080p][x265_flac].mkv",
                Some("SP"),
            ),
            (
                "[VCB-Studio] SHIROBAKO [Making06][Ma10p_1080p][x265_aac].mkv",
                Some("EXTRA"),
            ),
            (
                "特典映像/[DBD-Raws][Revue Starlight][Tokuten][01][1080P][BDRip][HEVC-10bit][FLAC].mkv",
                Some("EXTRA"),
            ),
            (
                "[DBD-Raws][Revue Starlight][Tokuten][02][1080P][BDRip][HEVC-10bit][FLAC].mkv",
                Some("EXTRA"),
            ),
            (
                "特典映像/[DBD-Raws][Title][声優トークショー][1080P][BDRip][HEVC-10bit][FLAC].mkv",
                Some("EXTRA"),
            ),
            (
                "[VCB-Studio] Some Show [NCOP&NCED][Ma10p_1080p][x265_flac].mkv",
                Some("NCOP/NCED"),
            ),
            (
                "[VCB-Studio] Some Show [SP01][Ma10p_1080p][x265_flac].mkv",
                Some("SP"),
            ),
            (
                "[VCB-Studio] Some Show [OP1][Ma10p_1080p][x265_flac].mkv",
                Some("OP/ED"),
            ),
            (
                "[VCB-Studio] Some Show [ED1][Ma10p_1080p][x265_flac].mkv",
                Some("OP/ED"),
            ),
            (
                "[VCB-Studio] Some Show [08][Ma10p_1080p][x265_flac].mkv",
                None,
            ),
            (
                "[ANi] SPY×FAMILY - 01 [1080P][Baha][WEB-DL][AAC AVC][CHT][MP4]",
                None,
            ),
            ("[Group] EDENS ZERO - 01 [1080p].mkv", None),
        ];

        for (name, expected) in cases {
            assert_eq!(label(name, &[]), *expected, "{name}");
        }
    }

    #[test]
    fn uses_folder_names_as_fallback() {
        assert_eq!(label("SP/01.mkv", &["Show"]), Some("SP"));
        assert_eq!(label("OVA/01.mkv", &["Show"]), Some("OVA"));
        assert_eq!(label("NCOP/01.mkv", &["Show"]), Some("NCOP/NCED"));
        assert_eq!(label("PV/特典.mp4", &["Show"]), Some("PV/CM"));
        assert_eq!(label("111/pv.mp4", &["Show"]), Some("PV/CM"));
        assert_eq!(label("111/01.mp4", &["Show"]), None);
    }

    #[test]
    fn does_not_match_series_title_prefixes() {
        // 不能把 SPY / EDENS 当成 SP / ED。
        assert_eq!(label("SPY×FAMILY - 01 [1080p].mkv", &[]), None);
        assert_eq!(label("EDENS ZERO - 01 [1080p].mkv", &[]), None);
        assert_eq!(label("SPY×FAMILY - 01 [1080p].mkv", &["SPY×FAMILY"]), None);
        assert_eq!(label("EDENS ZERO - 01 [1080p].mkv", &["EDENS ZERO"]), None);
        assert_eq!(label("Show - 01 [1080p][CHS].mkv", &["Show"]), None);
    }

    #[test]
    fn strips_longer_known_titles_first() {
        // 追番名本身就是 OVA 系列时，不能把每一集都标成 OVA。
        assert_eq!(label("Show OVA - 01.mkv", &["Show", "Show OVA"]), None);
        // 只配置了 Show 时，OVA 是真正的集内标记。
        assert_eq!(label("Show OVA - 01.mkv", &["Show"]), Some("OVA"));
    }

    #[test]
    fn ignores_absolute_path_prefixes() {
        // 相对路径解析失败时退回文件名，不能拿上级目录判断。
        assert_eq!(
            classify("C:\\Anime\\OVA\\01.mkv", "01.mkv", &["Show"]).map(ExtraKind::label),
            None
        );
        assert_eq!(
            classify("/anime/SP/01.mkv", "01.mkv", &["Show"]).map(ExtraKind::label),
            None
        );
    }

    #[test]
    fn prioritizes_more_specific_types() {
        assert_eq!(label("Show - SP NCOP.mkv", &["Show"]), Some("NCOP/NCED"));
        assert_eq!(label("SP/OVA 01.mkv", &["Show"]), Some("OVA"));
        assert_eq!(label("PV/特典.mp4", &["Show"]), Some("PV/CM"));
    }
}
