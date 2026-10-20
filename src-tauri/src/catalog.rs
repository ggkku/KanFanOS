//! 目录命名解析。
//!
//! 番剧目录有三种形态：
//! - 时间分类：`2026年7月新番/番剧名/视频`，子目录是番剧；
//! - 剧场版：`剧场版/影片名/视频`，作为独立分类，固定排在侧栏最上面；
//! - 直接分类：`番剧名/视频`，归入「未知时间」。

/// 无法解析档期时使用的分类名。
pub const UNKNOWN_CATEGORY: &str = "未知时间";

/// 剧场版独立分类。
pub const MOVIE_CATEGORY: &str = "剧场版";

/// 目录名是否是剧场版分类。
pub fn is_movie_category(name: &str) -> bool {
    name.trim() == MOVIE_CATEGORY
}

/// 从目录名中解析 `YYYY年M月`，返回 (年, 月)。
/// 允许后缀，例如 `2026年7月新番`。
pub fn parse_schedule(name: &str) -> Option<(i32, u32)> {
    let year_end = name.find('年')?;
    let year = name[..year_end].trim().parse::<i32>().ok()?;

    let rest = &name[year_end + '年'.len_utf8()..];
    let month_end = rest.find('月')?;
    let month = rest[..month_end].trim().parse::<u32>().ok()?;

    if !(1..=12).contains(&month) {
        return None;
    }
    Some((year, month))
}

/// 目录名 → 分类标签。不是时间目录时返回 `None`。
pub fn schedule_label(name: &str) -> Option<String> {
    parse_schedule(name).map(|(year, month)| format!("{year}年{month}月"))
}

/// 分类标签的排序键，越大越新；
/// 「剧场版」固定排最前，「未知时间」排最后。
pub fn schedule_sort_key(label: &str) -> i64 {
    if label == MOVIE_CATEGORY {
        return i64::MAX;
    }
    if label == UNKNOWN_CATEGORY {
        return i64::MIN;
    }
    parse_schedule(label)
        .map(|(year, month)| year as i64 * 100 + month as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::{
        is_movie_category, parse_schedule, schedule_label, schedule_sort_key,
        MOVIE_CATEGORY, UNKNOWN_CATEGORY,
    };

    #[test]
    fn parses_schedule_folder_names() {
        assert_eq!(parse_schedule("2026年7月新番"), Some((2026, 7)));
        assert_eq!(parse_schedule("2025年10月"), Some((2025, 10)));
        assert_eq!(parse_schedule("2024年01月新番"), Some((2024, 1)));
        assert_eq!(parse_schedule("进击的巨人第一季"), None);
        assert_eq!(parse_schedule("2026年13月"), None);
    }

    #[test]
    fn normalizes_labels_and_sorts() {
        assert_eq!(
            schedule_label("2026年7月新番").as_deref(),
            Some("2026年7月")
        );
        assert!(schedule_sort_key("2026年7月") > schedule_sort_key("2025年10月"));
        assert!(schedule_sort_key(UNKNOWN_CATEGORY) < schedule_sort_key("2000年1月"));
        assert!(schedule_sort_key(MOVIE_CATEGORY) > schedule_sort_key("2999年12月"));
    }

    #[test]
    fn recognizes_movie_category() {
        assert!(is_movie_category("剧场版"));
        assert!(is_movie_category(" 剧场版 "));
        assert!(!is_movie_category("剧场版 合集"));
        assert!(!is_movie_category("2024年10月新番"));
    }
}
