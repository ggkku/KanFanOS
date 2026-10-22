//! 嵌入 MPV：进程启动与 IPC 连接。
//!
//! MPV 通过 `--wid` 渲染到主窗口下的子窗口里，不再弹出独立的播放器窗口。

use std::{
    path::PathBuf,
    process::{Child, Command},
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, Instant},
};

use anyhow::{bail, Context, Result};

static PIPE_SEQ: AtomicU64 = AtomicU64::new(0);

/// 每个播放会话使用独立的命名管道。
pub fn pipe_name(video_id: i64) -> String {
    let stamp = crate::time::now_ms();
    let seq = PIPE_SEQ.fetch_add(1, Ordering::Relaxed);

    #[cfg(windows)]
    {
        format!(r"\\.\pipe\kanfanos-player-{video_id}-{stamp}-{seq}")
    }

    #[cfg(not(windows))]
    {
        format!("/tmp/kanfanos-player-{video_id}-{stamp}-{seq}.sock")
    }
}

/// 启动 MPV 并把画面嵌入 `wid` 指定的窗口。
pub fn spawn_embedded(
    path: &str,
    start_ms: i64,
    pipe: &str,
    wid: isize,
    input_conf: &str,
    log_file: &str,
    subtitle_files: &[PathBuf],
    suppress_subtitle_auto: bool,
) -> Result<Child> {
    let program = crate::tools::resolve("mpv");
    let mut command = Command::new(&program);
    command
        .arg("--terminal=no")
        .arg("--osc=no")
        .arg("--osd-level=0")
        .arg("--keep-open=yes")
        .arg("--force-window=yes")
        // 软件解码。部分老显卡驱动上 D3D11 硬件解码会出现音画不同步和卡顿，
        // 而 1080p H.264 的软件解码开销很低。
        .arg("--hwdec=no")
        // 精确 seek，避免在关键帧间隔较大的文件上跳转后长时间等待。
        .arg("--hr-seek=yes")
        .arg("--msg-level=all=warn")
        .arg(format!("--log-file={log_file}"))
        .arg(format!("--wid={wid}"))
        .arg(format!("--input-conf={input_conf}"))
        .arg(format!("--input-ipc-server={pipe}"))
        .arg(if suppress_subtitle_auto {
            "--sub-auto=no"
        } else {
            "--sub-auto=fuzzy"
        });

    // 外挂字幕已经由 Rust 侧统一转成 UTF-8 缓存，显式加载，避免
    // mpv/libavformat 用错误编码重新读取原始 GBK 文件。
    for subtitle in subtitle_files {
        command.arg(format!("--sub-file={}", subtitle.display()));
    }

    if start_ms >= 5_000 {
        command.arg(format!("--start={:.3}", start_ms as f64 / 1000.0));
    }

    command.arg(path);

    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }

    command.spawn().with_context(|| {
        format!(
            "无法启动 mpv（{}），安装包可能不完整，或 mpv 不在 PATH 中",
            program.display()
        )
    })
}

#[cfg(windows)]
pub(crate) async fn connect_with_retry(
    pipe: &str,
) -> Result<tokio::net::windows::named_pipe::NamedPipeClient> {
    let started = Instant::now();

    loop {
        match tokio::net::windows::named_pipe::ClientOptions::new().open(pipe) {
            Ok(client) => return Ok(client),
            Err(error) => {
                // 音频设备较多时 mpv 启动可能偏慢，给足时间再判定失败。
                if started.elapsed() > Duration::from_secs(15) {
                    bail!("连接 mpv IPC 失败: {error}");
                }
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        }
    }
}

#[cfg(not(windows))]
pub(crate) async fn connect_with_retry(
    pipe: &str,
) -> Result<tokio::net::UnixStream> {
    let started = Instant::now();

    loop {
        match tokio::net::UnixStream::connect(pipe).await {
            Ok(stream) => return Ok(stream),
            Err(error) => {
                // 音频设备较多时 mpv 启动可能偏慢，给足时间再判定失败。
                if started.elapsed() > Duration::from_secs(15) {
                    bail!("连接 mpv IPC 失败: {error}");
                }
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        }
    }
}

#[cfg(all(test, windows))]
mod tests {
    use super::pipe_name;

    #[test]
    fn pipe_names_are_unique() {
        let first = pipe_name(1);
        let second = pipe_name(1);

        assert!(first.starts_with(r"\\.\pipe\kanfanos-player-1-"));
        assert_ne!(first, second);
    }
}
