//! 应用内嵌播放器：MPV 渲染到 Win32 子窗口，前端通过 IPC 控制。

pub mod mpv;
pub mod surface;
mod subtitles;

use std::{
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant},
};

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
pub const ERROR_EVENT: &str = "player-error";
/// 鼠标移动通知。原生视频窗口会吞掉网页的 mousemove，只能从系统层面轮询光标。
pub const MOUSE_EVENT: &str = "player-mouse-activity";
/// 原生视频窗口上的左键单击。网页收不到这个点击，需要后端轮询系统鼠标。
pub const CLICK_EVENT: &str = "player-click";

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlayerError {
    pub episode_id: i64,
    pub message: String,
}

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
pub struct SubtitleTrack {
    pub id: i64,
    pub title: String,
    pub lang: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioTrack {
    pub id: i64,
    pub title: String,
    pub lang: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioDevice {
    pub name: String,
    pub description: String,
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
    /// 字幕是否有字幕轨可选。
    pub subtitles: Vec<SubtitleTrack>,
    /// 当前字幕轨；`None` 表示字幕关闭。
    pub subtitle_id: Option<i64>,
    /// 字幕总开关（`sub-visibility`）。
    pub subtitles_visible: bool,
    /// 音轨列表；MKV 等多音轨文件可以切换。
    pub audio_tracks: Vec<AudioTrack>,
    /// 当前音轨；`None` 表示 mpv 当前没有音频轨。
    pub audio_id: Option<i64>,
    /// 系统可用的音频输出设备。
    pub audio_devices: Vec<AudioDevice>,
    /// 当前音频输出设备；`auto` 表示跟随系统默认。
    pub audio_device: Option<String>,
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
    let log_file = state
        .data_root
        .join("logs")
        .join(format!("mpv-{episode_id}.log"));
    let pipe = mpv::pipe_name(episode_id);
    let subtitle_plan = subtitles::prepare(&episode.path, &state.data_root);
    let child = mpv::spawn_embedded(
        &episode.path,
        start_ms,
        &pipe,
        surface,
        &input_conf,
        &log_file.to_string_lossy(),
        &subtitle_plan.files,
        subtitle_plan.suppress_auto,
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
    spawn_mouse_watcher(app.clone());

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

static CURSOR_HIDDEN: AtomicBool = AtomicBool::new(false);

/// 隐藏/显示鼠标指针。全屏收起控制栏时一并隐藏，和常见播放器一致。
pub fn set_cursor_visible(app: &AppHandle, visible: bool) -> Result<()> {
    if CURSOR_HIDDEN.load(Ordering::SeqCst) == !visible {
        return Ok(());
    }
    CURSOR_HIDDEN.store(!visible, Ordering::SeqCst);

    // ShowCursor 维护的是线程内的显示计数，放在主线程调用。
    app.run_on_main_thread(move || unsafe {
        let _ = windows::Win32::UI::WindowsAndMessaging::ShowCursor(visible);
    })?;

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

    surface::hide(session.surface);

    // 直接结束进程，而不是先发送 quit。
    //
    // mpv 处理 quit 后还要走完整的退出流程（实测 1~3 秒），这期间音频仍在输出，
    // 表现为关闭播放器后还能听到声音。关闭时没有需要 mpv 落盘的状态，直接结束
    // 进程可以让音频立即停止；观看进度由后台 worker 在管道关闭后写入。
    let PlayerSession { mut child, .. } = session;
    let _ = child.kill();
    let _ = child.wait();
}

fn scaled_rect(app: &AppHandle, rect: PlayerRect) -> Result<(i32, i32, i32, i32)> {
    let window = crate::window::main_window(app).context("找不到主窗口")?;
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
            let window = crate::window::main_window(&main_app)
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

    // 每次启动都重写为应用管理的版本：单击由网页/系统鼠标轮询处理，
    // 不能再让 mpv 的 MBTN_LEFT 绑定把点击变成播放/暂停。
    std::fs::write(
        &path,
        "MBTN_LEFT ignore\nWHEEL_UP seek 5\nWHEEL_DOWN seek -5\n",
    )?;

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
        (8, "track-list"),
        (9, "sub-visibility"),
        (10, "sid"),
        (11, "aid"),
        (12, "audio-device-list"),
        (13, "audio-device"),
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

/// 播放期间轮询全局光标位置，移动时通知前端。
///
/// 网页只能收到控制栏等 HTML 区域的鼠标事件，光标移到原生视频窗口上时
/// 事件会被 mpv 吃掉，所以全屏唤出控制栏必须从系统层面检测。
fn spawn_mouse_watcher(app: AppHandle) {
    std::thread::spawn(move || {
        use std::ffi::c_void;

        use windows::Win32::{
            Foundation::{HWND, POINT, RECT},
            UI::{
                Input::KeyboardAndMouse::{GetAsyncKeyState, VK_LBUTTON},
                WindowsAndMessaging::{GetCursorPos, GetWindowRect},
            },
        };

        let mut last = None;
        let mut left_was_down = false;

        loop {
            let playing = app
                .state::<AppState>()
                .player
                .lock()
                .map(|guard| guard.is_some())
                .unwrap_or(false);
            if !playing {
                break;
            }

            let mut point = POINT::default();
            if unsafe { GetCursorPos(&mut point) }.is_ok() {
                let position = (point.x, point.y);
                if last != Some(position) {
                    last = Some(position);
                    let _ = app.emit(MOUSE_EVENT, ());
                }

                let left_down = unsafe { GetAsyncKeyState(VK_LBUTTON.0 as i32) } as u16
                    & 0x8000
                    != 0;
                if left_down && !left_was_down {
                    let surface = app
                        .state::<AppState>()
                        .video_surface
                        .lock()
                        .ok()
                        .and_then(|guard| *guard);
                    if let Some(surface) = surface {
                        let mut rect = RECT::default();
                        if unsafe {
                            GetWindowRect(
                                HWND(surface as *mut c_void),
                                &mut rect,
                            )
                        }
                        .is_ok()
                            && point.x >= rect.left
                            && point.x <= rect.right
                            && point.y >= rect.top
                            && point.y <= rect.bottom
                        {
                            let _ = app.emit(CLICK_EVENT, ());
                        }
                    }
                }
                left_was_down = left_down;
            }

            std::thread::sleep(Duration::from_millis(40));
        }
    });
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

            let state = app.state::<AppState>();
            let exited = state
                .player
                .lock()
                .ok()
                .and_then(|mut guard| {
                    guard
                        .as_mut()
                        .and_then(|session| session.child.try_wait().ok().flatten())
                })
                .is_some();

            if exited {
                // mpv 已经退出，说明启动失败，收起视频区域。
                cleanup(&app, episode_id);
            }

            let message = if exited {
                format!("播放器启动失败: {error:#}")
            } else {
                format!("播放控制连接失败，视频可能仍在播放: {error:#}")
            };

            let _ = app.emit(
                ERROR_EVENT,
                PlayerError {
                    episode_id,
                    message,
                },
            );
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
        subtitles: Vec::new(),
        subtitle_id: None,
        subtitles_visible: true,
        audio_tracks: Vec::new(),
        audio_id: None,
        audio_devices: Vec::new(),
        audio_device: None,
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
            Some(8) => {
                if let Some(tracks) = data {
                    state.subtitles = parse_subtitle_tracks(tracks);
                    state.audio_tracks = parse_audio_tracks(tracks);
                    immediate = true;
                }
            }
            Some(9) => {
                if let Some(visible) = data.and_then(|item| item.as_bool()) {
                    state.subtitles_visible = visible;
                    immediate = true;
                }
            }
            Some(10) => {
                // 字幕关闭时 mpv 返回 false，而不是数字。
                state.subtitle_id = data.and_then(|item| item.as_i64());
                immediate = true;
            }
            Some(11) => {
                // 没有音频轨时 mpv 返回 false。
                state.audio_id = data.and_then(|item| item.as_i64());
                immediate = true;
            }
            Some(12) => {
                if let Some(devices) = data {
                    state.audio_devices = parse_audio_devices(devices);
                    immediate = true;
                }
            }
            Some(13) => {
                state.audio_device = data
                    .and_then(|item| item.as_str())
                    .map(str::to_string);
                immediate = true;
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

/// 从 mpv 的 `track-list` 响应中筛出字幕轨。
fn parse_subtitle_tracks(data: &serde_json::Value) -> Vec<SubtitleTrack> {
    let Some(tracks) = data.as_array() else {
        return Vec::new();
    };

    tracks
        .iter()
        .filter(|track| {
            // mpv 的 track-list 里字幕类型是 "sub"（部分版本写作 "subtitle"）。
            matches!(
                track.get("type").and_then(|item| item.as_str()),
                Some("sub" | "subtitle")
            )
        })
        .filter_map(|track| {
            Some(SubtitleTrack {
                id: track.get("id")?.as_i64()?,
                title: track
                    .get("title")
                    .and_then(|item| item.as_str())
                    .unwrap_or_default()
                    .to_string(),
                lang: track
                    .get("lang")
                    .and_then(|item| item.as_str())
                    .unwrap_or_default()
                    .to_string(),
            })
        })
        .collect()
}

/// 从 mpv 的 `audio-device-list` 响应中解析输出设备。
fn parse_audio_devices(data: &serde_json::Value) -> Vec<AudioDevice> {
    let Some(devices) = data.as_array() else {
        return Vec::new();
    };

    devices
        .iter()
        .filter_map(|device| {
            let name = device.get("name")?.as_str()?.to_string();
            let description = device
                .get("description")
                .and_then(|item| item.as_str())
                .filter(|value| !value.is_empty())
                .unwrap_or(&name)
                .to_string();
            Some(AudioDevice { name, description })
        })
        .collect()
}

/// 从 mpv 的 `track-list` 响应中筛出音轨。
fn parse_audio_tracks(data: &serde_json::Value) -> Vec<AudioTrack> {
    let Some(tracks) = data.as_array() else {
        return Vec::new();
    };

    tracks
        .iter()
        .filter(|track| {
            track.get("type").and_then(|item| item.as_str()) == Some("audio")
        })
        .filter_map(|track| {
            Some(AudioTrack {
                id: track.get("id")?.as_i64()?,
                title: track
                    .get("title")
                    .and_then(|item| item.as_str())
                    .unwrap_or_default()
                    .to_string(),
                lang: track
                    .get("lang")
                    .and_then(|item| item.as_str())
                    .unwrap_or_default()
                    .to_string(),
            })
        })
        .collect()
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

#[cfg(test)]
mod tests {
    use super::{parse_audio_devices, parse_audio_tracks, parse_subtitle_tracks};

    #[test]
    fn parses_subtitle_tracks_only() {
        let data = serde_json::json!([
            {"id": 1, "type": "video", "codec": "hevc"},
            {"id": 2, "type": "audio", "lang": "ja"},
            {"id": 1, "type": "sub", "title": "CHS_JP", "lang": "zh-Hans"},
            {"id": 2, "type": "sub", "title": "CHT_JP", "lang": "zh-Hant"},
        ]);

        let tracks = parse_subtitle_tracks(&data);

        assert_eq!(tracks.len(), 2);
        assert_eq!(tracks[0].id, 1);
        assert_eq!(tracks[0].title, "CHS_JP");
        assert_eq!(tracks[0].lang, "zh-Hans");
        assert_eq!(tracks[1].id, 2);
    }

    #[test]
    fn parses_audio_tracks_only() {
        let data = serde_json::json!([
            {"id": 1, "type": "video", "codec": "hevc"},
            {"id": 1, "type": "audio", "title": "Japanese", "lang": "ja"},
            {"id": 2, "type": "audio", "title": "English", "lang": "en"},
            {"id": 1, "type": "sub", "title": "CHS_JP", "lang": "zh-Hans"},
        ]);

        let tracks = parse_audio_tracks(&data);

        assert_eq!(tracks.len(), 2);
        assert_eq!(tracks[0].id, 1);
        assert_eq!(tracks[0].title, "Japanese");
        assert_eq!(tracks[0].lang, "ja");
        assert_eq!(tracks[1].id, 2);
        assert_eq!(tracks[1].title, "English");
    }

    #[test]
    fn parses_audio_devices() {
        let data = serde_json::json!([
            {"name": "auto", "description": "Autoselect device"},
            {"name": "wasapi/{speaker}", "description": "扬声器 (Realtek)"},
            {"name": "wasapi/{headphone}"},
        ]);

        let devices = parse_audio_devices(&data);

        assert_eq!(devices.len(), 3);
        assert_eq!(devices[0].name, "auto");
        assert_eq!(devices[1].description, "扬声器 (Realtek)");
        assert_eq!(devices[2].description, "wasapi/{headphone}");
    }

    #[test]
    fn tolerates_empty_and_missing_fields() {
        let empty = parse_subtitle_tracks(&serde_json::json!([]));
        assert!(empty.is_empty());

        let unnamed = parse_subtitle_tracks(&serde_json::json!([
            {"id": 7, "type": "subtitle"},
        ]));
        assert_eq!(unnamed.len(), 1);
        assert_eq!(unnamed[0].title, "");
        assert_eq!(unnamed[0].lang, "");
    }
}
