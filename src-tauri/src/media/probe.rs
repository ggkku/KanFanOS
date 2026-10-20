use std::time::Duration;

use anyhow::{bail, Context, Result};
use serde_json::Value;
use tokio::time::timeout;

use crate::database::videos::ProbeData;

use super::command;

const PROBE_TIMEOUT: Duration = Duration::from_secs(30);

/// 调用 FFprobe 提取业务需要的字段。
pub async fn probe(path: &str) -> Result<ProbeData> {
    let output = timeout(
        PROBE_TIMEOUT,
        command("ffprobe")
            .args([
                "-v",
                "error",
                "-print_format",
                "json",
                "-show_format",
                "-show_streams",
                path,
            ])
            .output(),
    )
    .await
    .map_err(|_| anyhow::anyhow!("ffprobe 超时"))??;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("ffprobe 失败: {}", stderr.trim());
    }

    let json: Value =
        serde_json::from_slice(&output.stdout).context("解析 ffprobe 输出失败")?;
    Ok(parse(&json))
}

fn parse(json: &Value) -> ProbeData {
    let format = json.get("format");
    let streams = json.get("streams").and_then(Value::as_array);

    let video = streams.and_then(|list| {
        list.iter().find(|stream| {
            stream.get("codec_type").and_then(Value::as_str) == Some("video")
        })
    });
    let audio = streams.and_then(|list| {
        list.iter().find(|stream| {
            stream.get("codec_type").and_then(Value::as_str) == Some("audio")
        })
    });

    let duration_ms = format
        .and_then(|value| value.get("duration"))
        .and_then(Value::as_str)
        .and_then(|value| value.parse::<f64>().ok())
        .map(seconds_to_ms)
        .or_else(|| {
            video
                .and_then(|value| value.get("duration"))
                .and_then(Value::as_str)
                .and_then(|value| value.parse::<f64>().ok())
                .map(seconds_to_ms)
        });

    ProbeData {
        duration_ms,
        width: video
            .and_then(|value| value.get("width"))
            .and_then(Value::as_i64),
        height: video
            .and_then(|value| value.get("height"))
            .and_then(Value::as_i64),
        video_codec: video
            .and_then(|value| value.get("codec_name"))
            .and_then(Value::as_str)
            .map(str::to_string),
        audio_codec: audio
            .and_then(|value| value.get("codec_name"))
            .and_then(Value::as_str)
            .map(str::to_string),
        frame_rate: video
            .and_then(|value| value.get("r_frame_rate"))
            .and_then(Value::as_str)
            .and_then(parse_rate),
    }
}

fn seconds_to_ms(seconds: f64) -> i64 {
    (seconds * 1000.0).round() as i64
}

fn parse_rate(value: &str) -> Option<f64> {
    if let Some((numerator, denominator)) = value.split_once('/') {
        let numerator = numerator.parse::<f64>().ok()?;
        let denominator = denominator.parse::<f64>().ok()?;
        if denominator == 0.0 {
            return None;
        }
        Some(numerator / denominator)
    } else {
        value.parse::<f64>().ok()
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{parse, parse_rate};

    #[test]
    fn parses_probe_json() {
        let json = json!({
            "format": { "duration": "1420.5", "format_name": "matroska,webm" },
            "streams": [
                {
                    "codec_type": "video",
                    "codec_name": "h264",
                    "width": 1920,
                    "height": 1080,
                    "r_frame_rate": "24000/1001"
                },
                {
                    "codec_type": "audio",
                    "codec_name": "aac"
                }
            ]
        });

        let probe = parse(&json);
        assert_eq!(probe.duration_ms, Some(1_420_500));
        assert_eq!(probe.width, Some(1920));
        assert_eq!(probe.height, Some(1080));
        assert_eq!(probe.video_codec.as_deref(), Some("h264"));
        assert_eq!(probe.audio_codec.as_deref(), Some("aac"));
        let rate = probe.frame_rate.unwrap();
        assert!((rate - 23.976).abs() < 0.01);
    }

    #[test]
    fn handles_missing_video_stream() {
        let json = json!({ "streams": [{ "codec_type": "audio", "codec_name": "mp3" }] });
        let probe = parse(&json);
        assert_eq!(probe.width, None);
        assert_eq!(probe.audio_codec.as_deref(), Some("mp3"));
    }

    #[test]
    fn parses_frame_rates() {
        assert_eq!(parse_rate("25/1"), Some(25.0));
        assert_eq!(parse_rate("0/0"), None);
        assert_eq!(parse_rate("30"), Some(30.0));
    }
}
