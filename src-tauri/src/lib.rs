mod catalog;
mod commands;
mod database;
mod media;
mod autostart;
mod paths;
mod playback;
mod scanner;
mod series_config;
mod settings;
mod state;
mod time;
mod tools;
mod watcher;

use anyhow::{Context, Result};
use state::{AppState, MediaState, ScanState};
use tauri::Manager;

const DATA_SUBDIRS: [&str; 4] = ["database", "thumbnails", "logs", "config"];

fn initialize(app: &mut tauri::App) -> Result<()> {
    let data = paths::app_data_root(app.handle())?;
    let root = data.path.clone();

    for dir in DATA_SUBDIRS {
        let path = root.join(dir);
        std::fs::create_dir_all(&path)
            .with_context(|| format!("创建目录失败: {}", path.display()))?;
    }

    paths::append_log(
        &root,
        &format!(
            "启动：data_root={} portable={} migrated={}",
            root.display(),
            data.portable,
            data.migrated_from_legacy
        ),
    );

    // 首次运行时生成 settings.json。
    let app_settings = settings::load(&root)?;

    // 发布版启动时把开机启动项修正到当前 exe；调试版不做注册表写入。
    if let Err(error) = autostart::sync(app_settings.auto_start) {
        paths::append_log(
            &root,
            &format!("同步开机启动失败: {error:#}"),
        );
    }

    let db_path = root.join("database").join("library.sqlite");
    let db = database::Database::open(&db_path)?;
    // 上次运行中断的媒体任务回到队列。
    db.reset_media_processing()?;

    if data.migrated_from_legacy {
        match db.reset_thumbnails() {
            Ok(count) => paths::append_log(
                &root,
                &format!("已迁移旧数据库/配置，重置 {count} 个缩略图任务"),
            ),
            Err(error) => paths::append_log(
                &root,
                &format!("迁移后重置缩略图任务失败: {error:#}"),
            ),
        }
    }

    let library_id = match app_settings.library_root.as_deref() {
        Some(path) => Some(
            db.add_library(path, &paths::display_name(path), time::now_ms())?
                .id,
        ),
        None => None,
    };

    app.manage(AppState {
        db,
        scan: ScanState::new(),
        media: MediaState::new(),
        watcher: std::sync::Mutex::new(None),
        player: std::sync::Mutex::new(None),
        video_surface: std::sync::Mutex::new(None),
        data_root: root.clone(),
    });

    // 缩略图可能位于安装目录下的自定义数据目录；配置里的 scope 只覆盖默认位置，
    // 这里按实际数据根目录再放行一次。
    if let Err(error) = app
        .asset_protocol_scope()
        .allow_directory(root.join("thumbnails"), true)
    {
        eprintln!("允许访问缩略图目录失败: {error}");
    }

    // 允许前端通过 asset 协议读取番剧文件夹里的封面图片。
    if let Some(library_root) = app_settings.library_root.as_deref()
        && let Err(error) = app
            .asset_protocol_scope()
            .allow_directory(library_root, true)
    {
        eprintln!("允许访问番剧目录失败: {error}");
    }

    if app_settings.auto_watch
        && let (Some(library_id), Some(library_root)) =
            (library_id, app_settings.library_root)
    {
        watcher::start(
            app.handle().clone(),
            library_id,
            std::path::PathBuf::from(library_root),
        );
    }

    // 主窗口在配置里是隐藏创建的，先应用窗口模式再显示，避免启动时闪烁。
    if let Err(error) =
        commands::settings::apply_window_mode(app.handle(), &app_settings.window_mode)
    {
        eprintln!("应用窗口模式失败: {error}");
    }

    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
    }

    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            if let Err(error) = initialize(app) {
                eprintln!("应用初始化失败: {error:#}");
                return Err(error.to_string().into());
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::library::get_library_stats,
            commands::library::scan_library,
            commands::library::cancel_scan,
            commands::settings::get_settings,
            commands::settings::set_library_root,
            commands::settings::set_auto_watch,
            commands::settings::set_auto_start,
            commands::settings::set_thumbnail_width,
            commands::settings::set_page_transition,
            commands::settings::set_series_transition,
            commands::settings::set_window_mode,
            commands::settings::quit_app,
            commands::cache::get_cache_stats,
            commands::cache::clear_thumbnail_cache,
            commands::catalog::list_categories,
            commands::catalog::list_series,
            commands::catalog::get_series,
            commands::catalog::search_series,
            commands::catalog::list_episodes,
            commands::catalog::list_extra_episodes,
            commands::history::mark_series_watched,
            commands::history::clear_series_history,
            commands::history::set_series_watch_count,
            commands::media::start_media_processing,
            commands::media::cancel_media_processing,
            commands::media::get_media_stats,
            commands::playback::start_player,
            commands::playback::stop_player,
            commands::playback::resize_player,
            commands::playback::player_set_paused,
            commands::playback::player_seek,
            commands::playback::player_set_volume,
            commands::playback::player_set_muted,
            commands::playback::player_set_speed,
            commands::playback::player_set_subtitle,
            commands::playback::player_set_sub_delay,
            commands::playback::player_set_audio,
            commands::playback::set_player_cursor_visible,
            commands::playback::list_recent,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
