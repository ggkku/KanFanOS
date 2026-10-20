//! 通过外部 MPV 播放，并使用命名管道跟踪播放位置。
//!
//! 位置跟踪是可选的：如果 IPC 连接失败，播放本身不受影响，
//! 只是无法记录精确进度。

use std::{
    process::Command,
    sync::atomic::{AtomicU64, Ordering},
    time::Duration,
};

use anyhow::{bail, Context, Result};
use tauri::Manager;

use crate::state::AppState;

static PIPE_SEQ: AtomicU64 = AtomicU64::new(0);

#[cfg(windows)]
use std::time::Instant;

#[cfg(windows)]
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

/// 传送给 mpv 的 IPC 管道名。
pub fn pipe_name(video_id: i64) -> String {
    let stamp = crate::time::now_ms();
    let seq = PIPE_SEQ.fetch_add(1, Ordering::Relaxed);

    #[cfg(windows)]
    {
        format!(r"\\.\pipe\kanfanos-mpv-{video_id}-{stamp}-{seq}")
    }

    #[cfg(not(windows))]
    {
        format!("/tmp/kanfanos-mpv-{video_id}-{stamp}-{seq}.sock")
    }
}

/// 启动 mpv。`start_ms` 大于 5 秒时从上一次位置继续播放。
pub fn spawn(path: &str, start_ms: i64, pipe: &str) -> Result<()> {
    let mut command = Command::new("mpv");
    command
        .arg("--force-window=yes")
        .arg("--terminal=no")
        .arg("--sub-auto=fuzzy")
        .arg(format!("--input-ipc-server={pipe}"));

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

    command
        .spawn()
        .with_context(|| "无法启动 mpv，请确认 mpv 已安装并加入 PATH")?;

    Ok(())
}

/// 后台跟踪播放位置；mpv 退出后写入最终进度。
pub fn track_progress(
    app: tauri::AppHandle,
    video_id: i64,
    duration_ms: Option<i64>,
    pipe: String,
) {
    #[cfg(windows)]
    tauri::async_runtime::spawn(async move {
        let result = track(&pipe, |position_ms| {
            save(&app, video_id, position_ms, duration_ms);
        })
        .await;

        if let Err(error) = result {
            // 播放本身已经成功，跟踪失败只记录日志。
            eprintln!("播放进度跟踪结束: {error:#}");
        }
    });

    #[cfg(not(windows))]
    let _ = (app, video_id, duration_ms, pipe);
}

#[cfg(windows)]
async fn track<F>(pipe: &str, mut on_position: F) -> Result<()>
where
    F: FnMut(i64),
{
    use serde_json::Value;

    let client = connect_with_retry(pipe).await?;
    let (read_half, mut write_half) = tokio::io::split(client);
    let mut reader = BufReader::new(read_half);

    write_half
        .write_all(b"{\"command\":[\"observe_property\",1,\"time-pos\"]}\n")
        .await
        .context("向 mpv 发送 IPC 命令失败")?;

    let mut last_saved = Instant::now();
    let mut last_position_ms = 0i64;
    let mut received = false;
    let mut line = String::new();

    loop {
        line.clear();
        let bytes = reader.read_line(&mut line).await?;
        if bytes == 0 {
            break;
        }

        let Ok(value) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        if value.get("event").and_then(Value::as_str) != Some("property-change") {
            continue;
        }
        if value.get("id").and_then(Value::as_i64) != Some(1) {
            continue;
        }
        let Some(position) = value.get("data").and_then(Value::as_f64) else {
            continue;
        };

        received = true;
        last_position_ms = (position.max(0.0) * 1000.0).round() as i64;

        if last_saved.elapsed() >= Duration::from_secs(5) {
            on_position(last_position_ms);
            last_saved = Instant::now();
        }
    }

    // 没有收到任何位置事件时不要覆盖已有进度。
    if received {
        on_position(last_position_ms);
    }
    Ok(())
}

#[cfg(windows)]
fn save(app: &tauri::AppHandle, video_id: i64, position_ms: i64, duration_ms: Option<i64>) {
    let completed = duration_ms
        .map(|duration| duration > 0 && position_ms >= duration * 9 / 10)
        .unwrap_or(false);

    let state = app.state::<AppState>();
    if let Err(error) =
        state
            .db
            .save_watch_position(video_id, position_ms, completed, crate::time::now_ms())
    {
        eprintln!("保存播放进度失败: {error:#}");
    }
}

#[cfg(windows)]
async fn connect_with_retry(
    pipe: &str,
) -> Result<tokio::net::windows::named_pipe::NamedPipeClient> {
    let started = Instant::now();

    loop {
        match tokio::net::windows::named_pipe::ClientOptions::new().open(pipe) {
            Ok(client) => return Ok(client),
            Err(error) => {
                if started.elapsed() > Duration::from_secs(5) {
                    bail!("连接 mpv IPC 失败: {error}");
                }
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        }
    }
}

#[cfg(all(test, windows))]
mod tests {
    use std::sync::{Arc, Mutex};

    use tokio::io::AsyncWriteExt;
    use tokio::net::windows::named_pipe::ServerOptions;

    use super::{pipe_name, track};

    #[test]
    fn pipe_names_are_unique() {
        let first = pipe_name(1);
        let second = pipe_name(1);

        assert!(first.starts_with(r"\\.\pipe\kanfanos-mpv-1-"));
        assert_ne!(first, second);
    }

    /// 手动运行：
    /// `KANFANOS_TEST_VIDEO=<视频路径> cargo test tracks_real_mpv_process -- --ignored --nocapture`
    #[tokio::test]
    #[ignore = "需要 mpv 在 PATH 中，并设置 KANFANOS_TEST_VIDEO"]
    async fn tracks_real_mpv_process() {
        let Ok(video) = std::env::var("KANFANOS_TEST_VIDEO") else {
            return;
        };
        let pipe = pipe_name(7);

        let mut command = std::process::Command::new("mpv");
        command
            .arg("--no-video")
            .arg("--ao=null")
            .arg("--length=2")
            .arg("--terminal=no")
            .arg(format!("--input-ipc-server={pipe}"))
            .arg(&video);

        let mut child = command.spawn().expect("需要 mpv 在 PATH 中");

        let positions = Arc::new(Mutex::new(Vec::new()));
        let sink = positions.clone();

        let result = tokio::time::timeout(
            std::time::Duration::from_secs(15),
            track(&pipe, move |position| {
                sink.lock().unwrap().push(position);
            }),
        )
        .await
        .expect("跟踪超时");

        assert!(result.is_ok());
        let _ = child.wait();

        let values = positions.lock().unwrap();
        assert!(!values.is_empty(), "没有收到播放位置事件");
        assert!(values.iter().any(|position| *position > 0));
    }

    #[tokio::test]
    async fn tracks_positions_from_mpv_ipc() {
        let pipe = pipe_name(42);
        let mut server = ServerOptions::new().create(&pipe).unwrap();

        let positions = Arc::new(Mutex::new(Vec::new()));
        let sink = positions.clone();
        let pipe_for_task = pipe.clone();

        let task = tokio::spawn(async move {
            track(&pipe_for_task, move |position| {
                sink.lock().unwrap().push(position);
            })
            .await
        });

        server.connect().await.unwrap();
        server
            .write_all(b"{\"event\":\"property-change\",\"id\":1,\"data\":12.5}\n")
            .await
            .unwrap();

        // 关闭服务端，客户端才会读到 EOF 并写入最终位置。
        drop(server);

        let result = task.await.unwrap();
        assert!(result.is_ok());
        assert_eq!(*positions.lock().unwrap(), vec![12_500]);
    }
}
