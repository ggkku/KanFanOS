use std::{fs, path::Path, time::Duration};

use anyhow::{bail, Context, Result};
use tokio::time::timeout;

use crate::settings::{MAX_THUMBNAIL_WIDTH, MIN_THUMBNAIL_WIDTH};

use super::command;

const THUMBNAIL_TIMEOUT: Duration = Duration::from_secs(60);

/// 缩略图键：两级分片 + 视频 id 与更新时间，文件变化后自然换新文件名。
pub fn key(id: i64, updated_at: i64) -> String {
    format!("{:02x}/{id}-{updated_at}.jpg", (id as u64 % 256) as u8)
}

pub async fn generate(
    input: &str,
    duration_ms: i64,
    output: &Path,
    width: u32,
) -> Result<()> {
    if let Some(parent) = output.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("创建缩略图目录失败: {}", parent.display()))?;
    }

    let offset = offset_seconds(duration_ms);
    let offset_arg = format!("{offset:.3}");
    let output_arg = output.to_string_lossy().into_owned();
    let width = width.clamp(MIN_THUMBNAIL_WIDTH, MAX_THUMBNAIL_WIDTH);
    let scale_arg = format!("scale=min({width}\\,iw):-2");

    let result = timeout(
        THUMBNAIL_TIMEOUT,
        command("ffmpeg")
            .args([
                "-v",
                "error",
                "-ss",
                &offset_arg,
                "-i",
                input,
                "-frames:v",
                "1",
                // 只在宽度大于 480 时缩小，避免把低分辨率视频放大。
                "-vf",
                &scale_arg,
                "-q:v",
                "4",
                "-y",
                &output_arg,
            ])
            .output(),
    )
    .await;

    let output_result = match result {
        Ok(result) => result.context("启动 ffmpeg 失败")?,
        Err(_) => bail!("ffmpeg 超时"),
    };

    if !output_result.status.success() {
        let stderr = String::from_utf8_lossy(&output_result.stderr);
        bail!("ffmpeg 失败: {}", stderr.trim());
    }
    if !output.exists() {
        bail!("ffmpeg 没有生成缩略图");
    }
    Ok(())
}

/// 取时长的 20%，并给极短视频留出安全边界。
fn offset_seconds(duration_ms: i64) -> f64 {
    if duration_ms <= 2000 {
        return 0.0;
    }
    let offset_ms = (duration_ms / 5).clamp(1000, duration_ms - 1000);
    offset_ms as f64 / 1000.0
}

#[cfg(test)]
mod tests {
    use super::{key, offset_seconds};

    #[test]
    fn picks_safe_offsets() {
        assert_eq!(offset_seconds(0), 0.0);
        assert_eq!(offset_seconds(1500), 0.0);
        assert_eq!(offset_seconds(10_000), 2.0);
        assert_eq!(offset_seconds(3000), 1.0);
    }

    #[test]
    fn keys_are_sharded() {
        assert_eq!(key(1, 100), "01/1-100.jpg");
        assert_eq!(key(257, 200), "01/257-200.jpg");
    }
}
