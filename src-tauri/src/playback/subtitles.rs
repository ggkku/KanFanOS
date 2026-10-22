//! 外挂文本字幕编码归一化。
//!
//! mpv 的 `--sub-codepage` 最终传给 libavformat 的 `sub_charenc`，不支持
//! `utf8:gb18030` 这种回退链（会直接报 `Error opening iconv`）。这里在启动
//! mpv 前把视频同目录下匹配的 SRT/VTT/ASS/SSA 统一转成 UTF-8 缓存文件，
//! 再用 `--sub-file` 显式加载，避免 GBK 字幕被当成 UTF-8。

use std::{
    collections::hash_map::DefaultHasher,
    fs,
    hash::{Hash, Hasher},
    path::{Path, PathBuf},
};

use anyhow::{bail, Context, Result};

use crate::paths::append_log;

const TEXT_EXTENSIONS: &[&str] = &["srt", "vtt", "ass", "ssa"];
const VIDEO_EXTENSIONS: &[&str] = &[
    "mkv", "mp4", "m4v", "avi", "mov", "wmv", "flv", "webm", "ts", "m2ts", "mpg",
    "mpeg", "rmvb", "rm", "3gp", "ogv",
];

#[derive(Debug, Default)]
pub struct SubtitlePlan {
    /// 已转成 UTF-8 的外挂字幕缓存文件。
    pub files: Vec<PathBuf>,
    /// 目录里存在文本字幕，关闭 mpv 的自动加载，避免原始文件被再次加载。
    pub suppress_auto: bool,
}

/// 为某个视频准备外挂字幕。失败时宁可本次不加载外挂字幕，也不加载乱码。
pub fn prepare(video: &str, data_root: &Path) -> SubtitlePlan {
    let video_path = Path::new(video);
    let Some(dir) = video_path.parent() else {
        return SubtitlePlan::default();
    };
    let Some(video_stem) = video_path.file_stem().and_then(|stem| stem.to_str()) else {
        return SubtitlePlan::default();
    };
    let video_stem = video_stem.to_lowercase();

    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(error) => {
            append_log(
                data_root,
                &format!("读取字幕目录失败 {}: {error}", dir.display()),
            );
            return SubtitlePlan::default();
        }
    };

    let mut text_files: Vec<(PathBuf, Vec<u8>, String)> = Vec::new();
    let mut video_count = 0usize;
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }

        let Some(extension) = path
            .extension()
            .and_then(|value| value.to_str())
            .map(|value| value.to_ascii_lowercase())
        else {
            continue;
        };
        if VIDEO_EXTENSIONS.contains(&extension.as_str()) {
            video_count += 1;
            continue;
        }
        if !TEXT_EXTENSIONS.contains(&extension.as_str()) {
            continue;
        }

        let bytes = match fs::read(&path) {
            Ok(bytes) => bytes,
            Err(error) => {
                append_log(
                    data_root,
                    &format!("读取字幕失败 {}: {error}", path.display()),
                );
                continue;
            }
        };
        if bytes.contains(&0) {
            // 二进制字幕（例如 VobSub）交给 mpv 自己处理，不做文本转换。
            continue;
        }

        let stem = path
            .file_stem()
            .and_then(|value| value.to_str())
            .unwrap_or_default()
            .to_lowercase();
        text_files.push((path, bytes, stem));
    }

    if text_files.is_empty() {
        return SubtitlePlan::default();
    }

    let mut selected: Vec<(PathBuf, Vec<u8>, String)> = text_files
        .iter()
        .filter(|(_, _, stem)| matches_stem(&video_stem, stem))
        .cloned()
        .collect();

    // 单视频目录里，JPSC.ass / JPTC.ass 这种语言名文件通常就是本片的字幕，
    // 不要求文件名和视频名一致；只有一个字幕时也直接加载。
    if selected.is_empty() && (video_count == 1 || text_files.len() == 1) {
        selected = text_files;
    }

    // 目录里有文本字幕就关闭自动加载，避免原文件被 mpv 以编码错误的方式再次加载。
    if selected.is_empty() {
        return SubtitlePlan {
            files: Vec::new(),
            suppress_auto: true,
        };
    }

    let cache_dir = data_root.join("cache").join("subtitles");
    if let Err(error) = fs::create_dir_all(&cache_dir) {
        append_log(
            data_root,
            &format!("创建字幕缓存目录失败 {}: {error}", cache_dir.display()),
        );
        return SubtitlePlan {
            files: Vec::new(),
            suppress_auto: true,
        };
    }

    let mut files = Vec::new();
    for (path, bytes, _) in selected {
        let extension = path
            .extension()
            .and_then(|value| value.to_str())
            .unwrap_or("srt")
            .to_ascii_lowercase();

        match normalize(&path, &bytes, &extension, &cache_dir) {
            Ok(target) => files.push(target),
            Err(error) => append_log(
                data_root,
                &format!("字幕编码转换失败 {}: {error:#}", path.display()),
            ),
        }
    }

    if files.is_empty() {
        append_log(data_root, "字幕：没有转换成功的外挂文本字幕");
    } else {
        append_log(
            data_root,
            &format!("字幕：已把 {} 个外挂字幕转成 UTF-8", files.len()),
        );
    }

    SubtitlePlan {
        files,
        suppress_auto: true,
    }
}

fn matches_stem(video_stem: &str, subtitle_stem: &str) -> bool {
    if video_stem == subtitle_stem {
        return true;
    }

    let Some(rest) = subtitle_stem.strip_prefix(video_stem) else {
        return false;
    };

    [".", " ", "_", "-", "["]
        .iter()
        .any(|separator| rest.starts_with(separator))
}

fn normalize(
    source: &Path,
    bytes: &[u8],
    extension: &str,
    cache_dir: &Path,
) -> Result<PathBuf> {
    let text = decode_text(bytes)
        .with_context(|| format!("无法识别字幕编码: {}", source.display()))?;

    let mut output = Vec::with_capacity(text.len() + 3);
    // SRT/VTT 加 UTF-8 BOM，mpv 一定按 UTF-8 读取；ASS/SSA 的 BOM 可能
    // 干扰 `[Script Info]` 解析，所以不加。
    if matches!(extension, "srt" | "vtt") {
        output.extend_from_slice(&[0xEF, 0xBB, 0xBF]);
    }
    output.extend_from_slice(text.as_bytes());

    let mut hasher = DefaultHasher::new();
    source.to_string_lossy().hash(&mut hasher);
    let folder = cache_dir.join(format!("{:016x}", hasher.finish()));
    fs::create_dir_all(&folder)
        .with_context(|| format!("创建字幕缓存目录失败: {}", folder.display()))?;

    // 保留原始文件名，mpv 的轨道标题就会显示 JPSC.ass / 字幕.srt，
    // 而不是缓存哈希名。
    let file_name = source
        .file_name()
        .context("字幕文件没有文件名")?;
    let target = folder.join(file_name);
    fs::write(&target, output)
        .with_context(|| format!("写入字幕缓存失败: {}", target.display()))?;
    Ok(target)
}

fn decode_text(bytes: &[u8]) -> Result<String> {
    if let Some(rest) = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]) {
        return String::from_utf8(rest.to_vec()).context("UTF-8 字幕内容无效");
    }

    if bytes.starts_with(&[0xFF, 0xFE]) {
        return decode_utf16(&bytes[2..], true).context("UTF-16LE 字幕解码失败");
    }

    if bytes.starts_with(&[0xFE, 0xFF]) {
        return decode_utf16(&bytes[2..], false).context("UTF-16BE 字幕解码失败");
    }

    if let Ok(text) = std::str::from_utf8(bytes) {
        return Ok(text.to_string());
    }

    match decode_gb18030(bytes) {
        Some(text) => Ok(text),
        None => bail!("既不是有效 UTF-8，也不是有效 GB18030"),
    }
}

fn decode_utf16(bytes: &[u8], little_endian: bool) -> Result<String> {
    if bytes.len() % 2 != 0 {
        bail!("UTF-16 字节数为奇数");
    }

    let units: Vec<u16> = bytes
        .chunks_exact(2)
        .map(|chunk| {
            if little_endian {
                u16::from_le_bytes([chunk[0], chunk[1]])
            } else {
                u16::from_be_bytes([chunk[0], chunk[1]])
            }
        })
        .collect();

    String::from_utf16(&units).context("UTF-16 内容包含非法代理项")
}

#[cfg(windows)]
fn decode_gb18030(bytes: &[u8]) -> Option<String> {
    use windows::Win32::Globalization::{
        MB_ERR_INVALID_CHARS, MultiByteToWideChar,
    };

    // Windows 的 GB18030 代码页。
    const CP_GB18030: u32 = 54936;

    if bytes.is_empty() {
        return Some(String::new());
    }

    let mut flags = MB_ERR_INVALID_CHARS;
    let mut length =
        unsafe { MultiByteToWideChar(CP_GB18030, flags, bytes, None) };
    if length <= 0 {
        // 部分系统不接受 MB_ERR_INVALID_CHARS，再按宽松模式试一次。
        flags = Default::default();
        length = unsafe { MultiByteToWideChar(CP_GB18030, flags, bytes, None) };
    }
    if length <= 0 {
        return None;
    }

    let mut wide = vec![0u16; length as usize];
    let written = unsafe {
        MultiByteToWideChar(CP_GB18030, flags, bytes, Some(&mut wide))
    };
    if written <= 0 {
        return None;
    }

    String::from_utf16(&wide[..written as usize]).ok()
}

#[cfg(not(windows))]
fn decode_gb18030(_bytes: &[u8]) -> Option<String> {
    None
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        path::PathBuf,
        time::{SystemTime, UNIX_EPOCH},
    };

    use super::prepare;

    fn temp_dir(name: &str) -> PathBuf {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("kanfanos-sub-{name}-{stamp}"));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn converts_gbk_and_utf8_subtitles() {
        let dir = temp_dir("convert");
        let video = dir.join("测试番 第1集.mkv");
        fs::write(&video, b"").unwrap();

        let mut gbk_bytes =
            b"1\n00:00:01,000 --> 00:00:02,000\n".to_vec();
        // GBK: 别急 一个个来
        gbk_bytes.extend_from_slice(&[
            0xB1, 0xF0, 0xBC, 0xB1, 0x20, 0xD2, 0xBB, 0xB8, 0xF6, 0xB8, 0xF6, 0xC0,
            0xB4, 0x0A,
        ]);
        fs::write(dir.join("测试番 第1集.gbk.srt"), gbk_bytes).unwrap();
        fs::write(
            dir.join("测试番 第1集.utf8.srt"),
            "1\n00:00:01,000 --> 00:00:02,000\n好漂亮\n".as_bytes(),
        )
        .unwrap();

        let data_root = dir.join("appdata");
        let plan = prepare(video.to_str().unwrap(), &data_root);

        assert!(plan.suppress_auto);
        assert_eq!(plan.files.len(), 2);
        let names: Vec<String> = plan
            .files
            .iter()
            .map(|file| {
                file.file_name()
                    .unwrap()
                    .to_string_lossy()
                    .into_owned()
            })
            .collect();
        assert!(names.contains(&"测试番 第1集.gbk.srt".to_string()));
        assert!(names.contains(&"测试番 第1集.utf8.srt".to_string()));
        for file in &plan.files {
            let text = fs::read_to_string(file).unwrap();
            assert!(std::str::from_utf8(text.as_bytes()).is_ok());
            assert!(text.contains("别急") || text.contains("好漂亮"));
        }

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn loads_language_named_ass_for_single_video() {
        let dir = temp_dir("language-ass");
        let video = dir.join("movie.mkv");
        fs::write(&video, b"").unwrap();
        fs::write(dir.join("JPSC.ass"), b"[Script Info]\nTitle: JPSC\n").unwrap();
        fs::write(dir.join("JPTC.ass"), b"[Script Info]\nTitle: JPTC\n").unwrap();

        let plan = prepare(video.to_str().unwrap(), &dir.join("appdata"));

        assert!(plan.suppress_auto);
        let names: Vec<String> = plan
            .files
            .iter()
            .map(|file| {
                file.file_name()
                    .unwrap()
                    .to_string_lossy()
                    .into_owned()
            })
            .collect();
        assert_eq!(names.len(), 2);
        assert!(names.contains(&"JPSC.ass".to_string()));
        assert!(names.contains(&"JPTC.ass".to_string()));

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn ignores_directories_without_text_subtitles() {
        let dir = temp_dir("empty");
        let video = dir.join("a.mkv");
        fs::write(&video, b"").unwrap();
        fs::write(dir.join("a.sup"), [0u8, 1, 2]).unwrap();

        let plan = prepare(video.to_str().unwrap(), &dir.join("appdata"));
        assert!(!plan.suppress_auto);
        assert!(plan.files.is_empty());

        let _ = fs::remove_dir_all(dir);
    }
}
