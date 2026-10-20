mod catalog;
mod commands;
mod database;
mod media;
mod paths;
mod playback;
mod scanner;
mod series_config;
mod settings;
mod state;
mod time;
mod watcher;

use anyhow::{Context, Result};
use state::{AppState, MediaState, ScanState};
use tauri::Manager;

const DATA_SUBDIRS: [&str; 4] = ["database", "thumbnails", "logs", "config"];

fn initialize(app: &mut tauri::App) -> Result<()> {
    let root = paths::app_data_root(app.handle())?;

    for dir in DATA_SUBDIRS {
        let path = root.join(dir);
        std::fs::create_dir_all(&path)
            .with_context(|| format!("创建目录失败: {}", path.display()))?;
    }

    // 首次运行时生成 settings.json。
    let app_settings = settings::load(&root)?;

    let db_path = root.join("database").join("library.sqlite");
    let db = database::Database::open(&db_path)?;
    // 上次运行中断的媒体任务回到队列。
    db.reset_media_processing()?;

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
        data_root: root,
    });

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
            commands::settings::set_thumbnail_width,
            commands::cache::get_cache_stats,
            commands::cache::clear_thumbnail_cache,
            commands::catalog::list_categories,
            commands::catalog::list_series,
            commands::catalog::search_series,
            commands::catalog::list_episodes,
            commands::media::start_media_processing,
            commands::media::cancel_media_processing,
            commands::media::get_media_stats,
            commands::playback::open_episode,
            commands::playback::list_recent,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
