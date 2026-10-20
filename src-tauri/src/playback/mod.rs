//! 应用内嵌播放器：MPV 渲染到 Win32 子窗口，前端通过 IPC 控制。

pub mod mpv;
pub mod surface;

use std::time::{Duration, Instant};

use anyhow::{anyhow, bail, Context, Result};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    sync::mpsc::{self, UnboundedReceiver, UnboundedSender},
};

use crate::{database::history::PlayableEpisode, state::AppState, time::now_ms};

pub const STATE_EVENT: &str = "player-state";
pub const CLOSED_EVENT: &str = "player-closed";

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlayerRect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlayerState {
    pub episode_id: i64,
    pub position_ms: i64,
    pub duration_ms: i64,
    pub paused: bool,
    pub volume: f64,
    pub muted: bool,
    pub speed: f64,
    pub ended: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlayerStarted {
    pub episode_id: i64,
    pub duration_ms: i64,
    pub start_position_ms: i64,
}

pub struct PlayerSession {
    pub episode_id: i64,
    pub tx: UnboundedSender<String>,
    pub child: std::process::Child,
    pub surface: isize,
}

/// 启动播放。已有会话会先停止。
pub fn start(
    app: &AppHandle,
    episode: &PlayableEpisode,
    episode_id: i64,
    rect: PlayerRect,
) -> Result<PlayerStarted> {
    stop_session(app);

    let state = app.state::<AppState>();
    let surface = ensure_surface(app)?;

    let (x, y, width, height) = scaled_rect(app, rect)?;
    surface::show(surface, x, y, width, height)?;

    let (last_position, completed) = state.db.resume_position(episode_id)?;
    let start_ms = if completed { 0 } else { last_position };

    let input_conf = ensure_input_conf(&state.data_root)?;
    let pipe = mpv::pipe_name(episode_id);
    let child = mpv::spawn_embedded(
        &episode.path,
        start_ms,
        &pipe,
        surface,
        &input_conf,
    )?;

    let (tx, rx) = mpsc::unbounded_channel();
    for command in observe_commands() {
        let _ = tx.send(command);
    }

    {
        let mut guard = state
            .player
            .lock()
            .map_err(|_| anyhow!("播放器状态锁损坏"))?;
        *guard = Some(PlayerSession {
            episode_id,
            tx,
            child,
            surface,
        });
    }

    state.db.record_episode_opened(episode_id, now_ms())?;

    spawn_worker(app.clone(), episode_id, episode.duration_ms, pipe, rx);

    Ok(PlayerStarted {
        episode_id,
        duration_ms: episode.duration_ms.unwrap_or(0),
        start_position_ms: start_ms,
    })
}

pub fn stop(app: &AppHandle) {
    stop_session(app);
}

pub fn resize(app: &AppHandle, rect: PlayerRect) -> Result<()> {
    let state = app.state::<AppState>();

    let playing = state
        .player
        .lock()
        .map(|guard| guard.is_some())
        .unwrap_or(false);
    if !playing {
        return Ok(());
    }

    let surface = state
        .video_surface
        .lock()
        .ok()
        .and_then(|guard| *guard);
    let Some(surface) = surface else {
        return Ok(());
    };

    let (x, y, width, height) = scaled_rect(app, rect)?;
    surface::resize(surface, x, y, width, height)?;
    Ok(())
}

pub fn send(app: &AppHandle, command: serde_json::Value) -> Result<()> {
    let state = app.state::<AppState>();
    let guard = state
        .player
        .lock()
        .map_err(|_| anyhow!("播放器状态锁损坏"))?;
    let Some(session) = guard.as_ref() else {
        bail!("播放器未启动");
    };

    session
        .tx
        .send(command.to_string())
        .map_err(|_| anyhow!("播放器连接已关闭"))
}

fn stop_session(app: &AppHandle) {
    let state = app.state::<AppState>();
    let session = state
        .player
        .lock()
        .ok()
        .and_then(|mut guard| guard.take());

    let Some(session) = session else {
        return;
    };

    let _ = session
        .tx
        .send(r#"{"command":["quit"]}"#.to_string());
    surface::hide(session.surface);

    let PlayerSession { mut child, .. } = session;
    std::thread::spawn(move || {
        let started = Instant::now();
        loop {
            match child.try_wait() {
                Ok(Some(_)) => return,
                Ok(None) if started.elapsed() > Duration::from_secs(2) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return;
                }
                Ok(None) => std::thread::sleep(Duration::from_millis(50)),
                Err(_) => return,
            }
        }
    });
}

fn scaled_rect(app: &AppHandle, rect: PlayerRect) -> Result<(i32, i32, i32, i32)> {
    let window = app
        .get_webview_window("main")
        .context("找不到主窗口")?;
    let scale = window.scale_factor()?;

    Ok((
        (rect.x * scale).round().max(0.0) as i32,
        (rect.y * scale).round().max(0.0) as i32,
        (rect.width * scale).round().max(1.0) as i32,
        (rect.height * scale).round().max(1.0) as i32,
    ))
}

fn ensure_surface(app: &AppHandle) -> Result<isize> {
    let state = app.state::<AppState>();

    {
        let guard = state
            .video_surface
            .lock()
            .map_err(|_| anyhow!("视频窗口状态锁损坏"))?;
        if let Some(surface) = *guard {
            return Ok(surface);
        }
    }

    let (tx, rx) = std::sync::mpsc::channel();
    let main_app = app.clone();

    app.run_on_main_thread(move || {
        let result = (|| -> Result<isize> {
            let window = main_app
                .get_webview_window("main")
                .context("找不到主窗口")?;
            let parent = window.hwnd()?;
            surface::create(parent)
        })();

        let _ = tx.send(result);
    })?;

    let surface = rx
        .recv()
        .map_err(|_| anyhow!("创建视频窗口超时"))??;

    *state
        .video_surface
        .lock()
        .map_err(|_| anyhow!("视频窗口状态锁损坏"))? = Some(surface);

    Ok(surface)
}

/// 内嵌播放器使用自己的按键/鼠标绑定，不依赖 MPV 的 OSC。
fn ensure_input_conf(data_root: &std::path::Path) -> Result<String> {
    let directory = data_root.join("config");
    std::fs::create_dir_all(&directory)?;
    let path = directory.join("mpv-input.conf");

    if !path.exists() {
        std::fs::write(
            &path,
            "MBTN_LEFT cycle pause\nWHEEL_UP seek 5\nWHEEL_DOWN seek -5\n",
        )?;
    }

    Ok(path.to_string_lossy().into_owned())
}

fn observe_commands() -> Vec<String> {
    [
        (1, "time-pos"),
        (2, "duration"),
        (3, "pause"),
        (4, "volume"),
        (5, "mute"),
        (6, "speed"),
        (7, "eof-reached"),
    ]
    .iter()
    .map(|(id, property)| {
        serde_json::json!({
            "command": ["observe_property", id, property]
        })
        .to_string()
    })
    .collect()
}

fn spawn_worker(
    app: AppHandle,
    episode_id: i64,
    duration_ms: Option<i64>,
    pipe: String,
    rx: UnboundedReceiver<String>,
) {
    tauri::async_runtime::spawn(async move {
        if let Err(error) = worker(&app, episode_id, duration_ms, &pipe, rx).await {
            eprintln!("播放器 IPC 结束: {error:#}");
            cleanup(&app, episode_id);
        }
    });
}

async fn worker(
    app: &AppHandle,
    episode_id: i64,
    duration_ms: Option<i64>,
    pipe: &str,
    mut rx: UnboundedReceiver<String>,
) -> Result<()> {
    let client = mpv::connect_with_retry(pipe).await?;
    let (read_half, write_half) = tokio::io::split(client);
    let mut reader = BufReader::new(read_half);

    tokio::spawn(async move {
        let mut write_half = write_half;
        while let Some(line) = rx.recv().await {
            if write_half.write_all(line.as_bytes()).await.is_err() {
                break;
            }
            if write_half.write_all(b"\n").await.is_err() {
                break;
            }
        }
    });

    let mut state = PlayerState {
        episode_id,
        position_ms: 0,
        duration_ms: duration_ms.unwrap_or(0),
        paused: false,
        volume: 100.0,
        muted: false,
        speed: 1.0,
        ended: false,
    };

    let mut last_emit = Instant::now();
    let mut last_save = Instant::now();
    let mut line = String::new();

    loop {
        line.clear();
        let bytes = reader.read_line(&mut line).await?;
        if bytes == 0 {
            break;
        }

        let Ok(value) = serde_json::from_str::<serde_json::Value>(&line) else {
            continue;
        };
        if value.get("event").and_then(|item| item.as_str()) != Some("property-change") {
            continue;
        }

        let id = value.get("id").and_then(|item| item.as_i64());
        let data = value.get("data");
        let mut immediate = false;

        match id {
            Some(1) => {
                if let Some(position) = data.and_then(|item| item.as_f64()) {
                    state.position_ms =
                        (position.max(0.0) * 1000.0).round() as i64;
                }
            }
            Some(2) => {
                if let Some(duration) = data.and_then(|item| item.as_f64()) {
                    state.duration_ms =
                        (duration.max(0.0) * 1000.0).round() as i64;
                    immediate = true;
                }
            }
            Some(3) => {
                if let Some(paused) = data.and_then(|item| item.as_bool()) {
                    state.paused = paused;
                    immediate = true;
                }
            }
            Some(4) => {
                if let Some(volume) = data.and_then(|item| item.as_f64()) {
                    state.volume = volume;
                    immediate = true;
                }
            }
            Some(5) => {
                if let Some(muted) = data.and_then(|item| item.as_bool()) {
                    state.muted = muted;
                    immediate = true;
                }
            }
            Some(6) => {
                if let Some(speed) = data.and_then(|item| item.as_f64()) {
                    state.speed = speed;
                    immediate = true;
                }
            }
            Some(7) => {
                if let Some(ended) = data.and_then(|item| item.as_bool()) {
                    state.ended = ended;
                    immediate = true;
                }
            }
            _ => {}
        }

        if immediate || last_emit.elapsed() >= Duration::from_millis(250) {
            let _ = app.emit(STATE_EVENT, &state);
            last_emit = Instant::now();
        }

        if last_save.elapsed() >= Duration::from_secs(5) && state.position_ms > 0 {
            save_progress(app, episode_id, &state);
            last_save = Instant::now();
        }
    }

    if state.position_ms > 0 {
        save_progress(app, episode_id, &state);
    }

    cleanup(app, episode_id);
    Ok(())
}

fn save_progress(app: &AppHandle, episode_id: i64, state: &PlayerState) {
    let completed = state.duration_ms > 0
        && state.position_ms >= state.duration_ms * 9 / 10;

    let db = &app.state::<AppState>().db;
    if let Err(error) = db.save_watch_position(
        episode_id,
        state.position_ms,
        completed,
        now_ms(),
    ) {
        eprintln!("保存播放进度失败: {error:#}");
    }
}

fn cleanup(app: &AppHandle, episode_id: i64) {
    let state = app.state::<AppState>();
    let mut was_current = false;

    if let Ok(mut guard) = state.player.lock()
        && guard.as_ref().map(|session| session.episode_id) == Some(episode_id)
    {
        guard.take();
        was_current = true;
    }

    if was_current
        && let Some(surface) = state.video_surface.lock().ok().and_then(|guard| *guard)
    {
        surface::hide(surface);
    }

    let _ = app.emit(CLOSED_EVENT, episode_id);
}
