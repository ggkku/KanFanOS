//! 应用数据目录解析。
//!
//! 默认使用 `%LOCALAPPDATA%\KanFanOS`。安装到自定义目录（例如
//! `D:\KanFanOS`）且该目录可写时，运行数据放在 `<安装目录>\data`，
//! 避免数据库和缩略图占用系统盘。
//!
//! 开发模式（`pnpm tauri dev`）的 exe 在 `target\debug`，如果也套用这条
//! 规则会把数据写进构建目录，所以只在 release 构建里启用可移动数据目录。

use std::{
    io::Write,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};
use tauri::Manager;

/// 选择好的数据根目录，以及首次运行时的迁移结果。
#[derive(Debug, Clone)]
pub struct DataRoot {
    pub path: PathBuf,
    /// 是否使用了安装目录下的 `data`。
    pub portable: bool,
    /// 是否从旧的 `%LOCALAPPDATA%\KanFanOS` 复制了数据库和配置。
    pub migrated_from_legacy: bool,
}

impl DataRoot {
    fn legacy(path: PathBuf) -> Self {
        Self {
            path,
            portable: false,
            migrated_from_legacy: false,
        }
    }
}

/// 目录名作为显示名称，根目录（如 `C:\`）则退回完整路径。
pub fn display_name(path: &str) -> String {
    std::path::Path::new(path)
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.to_string())
}

/// 追加一行启动日志。数据目录的选择和迁移结果都写在这里，
/// 用户可以直接确认数据到底落在哪个盘。
pub fn append_log(root: &Path, message: &str) {
    let dir = root.join("logs");
    if std::fs::create_dir_all(&dir).is_err() {
        return;
    }

    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir.join("app.log"))
    {
        let _ = writeln!(file, "[{}] {message}", crate::time::now_ms());
    }
}

pub fn app_data_root(app: &tauri::AppHandle) -> Result<DataRoot> {
    let legacy = legacy_data_root(app)?;

    let Some(exe_dir) = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(Path::to_path_buf))
    else {
        return Ok(DataRoot::legacy(legacy));
    };

    // 默认安装目录本身就是 `%LOCALAPPDATA%\KanFanOS`，保持原来的布局，
    // 不要在里面再套一层 data。
    let Some(portable) = choose_portable_root(
        &exe_dir,
        &legacy,
        cfg!(debug_assertions),
    ) else {
        return Ok(DataRoot::legacy(legacy));
    };

    let portable_db = portable.join("database").join("library.sqlite");
    let legacy_db = legacy.join("database").join("library.sqlite");

    let mut migrated = false;
    if !portable_db.is_file() && legacy_db.is_file() && is_dir_empty_or_missing(&portable) {
        match migrate_legacy(&legacy, &portable) {
            Ok(()) => migrated = true,
            Err(error) => {
                let message = format!(
                    "把旧数据迁移到 {} 失败，本次继续使用 {}：{error:#}",
                    portable.display(),
                    legacy.display()
                );
                append_log(&legacy, &message);
                return Ok(DataRoot::legacy(legacy));
            }
        }
    }

    Ok(DataRoot {
        path: portable,
        portable: true,
        migrated_from_legacy: migrated,
    })
}

fn legacy_data_root(app: &tauri::AppHandle) -> Result<PathBuf> {
    #[cfg(windows)]
    if let Ok(local_app_data) = std::env::var("LOCALAPPDATA")
        && !local_app_data.is_empty()
    {
        return Ok(PathBuf::from(local_app_data).join("KanFanOS"));
    }

    Ok(app.path().app_local_data_dir()?)
}

/// 自定义安装目录且可写时返回 `<安装目录>\data`。
fn choose_portable_root(
    exe_dir: &Path,
    legacy: &Path,
    is_dev_build: bool,
) -> Option<PathBuf> {
    if is_dev_build || same_dir(exe_dir, legacy) {
        return None;
    }

    let portable = exe_dir.join("data");
    is_writable_dir(&portable).then_some(portable)
}

fn same_dir(a: &Path, b: &Path) -> bool {
    match (std::fs::canonicalize(a), std::fs::canonicalize(b)) {
        (Ok(a), Ok(b)) => a == b,
        _ => a == b,
    }
}

fn is_writable_dir(path: &Path) -> bool {
    if std::fs::create_dir_all(path).is_err() {
        return false;
    }

    // 清掉上次异常退出留下的探测文件，避免它挡住首次数据迁移。
    if let Ok(entries) = std::fs::read_dir(path) {
        for entry in entries.flatten() {
            if entry
                .file_name()
                .to_string_lossy()
                .starts_with(".write-test-")
            {
                let _ = std::fs::remove_file(entry.path());
            }
        }
    }

    let probe = path.join(format!(
        ".write-test-{}-{}",
        std::process::id(),
        crate::time::now_ms()
    ));

    match std::fs::write(&probe, b"") {
        Ok(()) => {
            let _ = std::fs::remove_file(&probe);
            true
        }
        Err(_) => false,
    }
}

fn is_dir_empty_or_missing(path: &Path) -> bool {
    match std::fs::read_dir(path) {
        Ok(mut entries) => entries.next().is_none(),
        Err(_) => !path.exists(),
    }
}

/// 只复制数据库和配置，缩略图交给后台队列重新生成，
/// 避免首次启动时搬运大量图片。
fn migrate_legacy(legacy: &Path, portable: &Path) -> Result<()> {
    let staging = portable.with_file_name("data.migrating");

    if staging.exists() {
        std::fs::remove_dir_all(&staging)
            .with_context(|| format!("清理残留迁移目录失败: {}", staging.display()))?;
    }
    std::fs::create_dir_all(&staging)
        .with_context(|| format!("创建迁移目录失败: {}", staging.display()))?;

    let copy_result = (|| -> Result<()> {
        let database = legacy.join("database");
        let target_database = staging.join("database");
        std::fs::create_dir_all(&target_database).with_context(|| {
            format!("创建数据库目录失败: {}", target_database.display())
        })?;

        // 优先用 `VACUUM INTO` 生成一份一致的快照，旧版可能还在运行；
        // 失败时退回直接复制数据库文件（连同 WAL/SHM）。
        let target_db = target_database.join("library.sqlite");
        if snapshot_database(&database.join("library.sqlite"), &target_db).is_err() {
            let _ = std::fs::remove_file(&target_db);
            let _ = std::fs::remove_dir_all(&target_database);
            copy_dir(&database, &target_database)?;
        }

        let config = legacy.join("config");
        if config.is_dir() {
            copy_dir(&config, &staging.join("config"))?;
        }

        Ok(())
    })();

    if let Err(error) = copy_result {
        let _ = std::fs::remove_dir_all(&staging);
        return Err(error);
    }

    // `is_writable_dir` 会先创建空的 portable 目录，迁移时把它换掉。
    if portable.exists() {
        std::fs::remove_dir(portable)
            .with_context(|| format!("数据目录已存在且非空: {}", portable.display()))?;
    }

    std::fs::rename(&staging, portable).with_context(|| {
        format!("把迁移目录移动到 {} 失败", portable.display())
    })?;

    Ok(())
}

fn snapshot_database(source: &Path, target: &Path) -> Result<()> {
    let conn = rusqlite::Connection::open_with_flags(
        source,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .with_context(|| format!("打开旧数据库失败: {}", source.display()))?;

    let target = target.to_string_lossy().into_owned();
    conn.execute("VACUUM INTO ?1", [target])
        .context("复制数据库快照失败")?;

    Ok(())
}

fn copy_dir(from: &Path, to: &Path) -> Result<()> {
    std::fs::create_dir_all(to)
        .with_context(|| format!("创建目录失败: {}", to.display()))?;

    for entry in std::fs::read_dir(from)
        .with_context(|| format!("读取目录失败: {}", from.display()))?
    {
        let entry = entry?;
        let file_type = entry.file_type()?;
        let target = to.join(entry.file_name());

        if file_type.is_dir() {
            copy_dir(&entry.path(), &target)?;
        } else if file_type.is_file() {
            std::fs::copy(entry.path(), &target)
                .with_context(|| format!("复制文件失败: {}", target.display()))?;
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        path::PathBuf,
        time::{SystemTime, UNIX_EPOCH},
    };

    use super::{
        choose_portable_root, migrate_legacy,
    };

    fn temp_dir(name: &str) -> PathBuf {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("kanfanos-paths-{name}-{stamp}"));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn custom_writable_install_dir_uses_data_subdirectory() {
        let note = temp_dir("portable");
        let exe_dir = note.join("D-KanFanOS");
        let legacy = note.join("legacy");
        fs::create_dir_all(&exe_dir).unwrap();
        fs::create_dir_all(&legacy).unwrap();

        let portable = choose_portable_root(&exe_dir, &legacy, false)
            .expect("应选择安装目录下的 data");
        assert_eq!(portable, exe_dir.join("data"));
        assert!(portable.is_dir());

        let _ = fs::remove_dir_all(note);
    }

    #[test]
    fn default_install_dir_and_dev_builds_keep_legacy_root() {
        let note = temp_dir("legacy");
        let exe_dir = note.join("KanFanOS");
        fs::create_dir_all(&exe_dir).unwrap();

        assert!(choose_portable_root(&exe_dir, &exe_dir, false).is_none());
        assert!(choose_portable_root(&exe_dir, &note.join("other"), true).is_none());

        let _ = fs::remove_dir_all(note);
    }

    #[test]
    fn migration_copies_database_and_config_only() {
        let note = temp_dir("migrate");
        let legacy = note.join("legacy");
        let portable = note.join("data");

        fs::create_dir_all(legacy.join("database")).unwrap();
        fs::create_dir_all(legacy.join("config")).unwrap();
        fs::create_dir_all(legacy.join("thumbnails")).unwrap();
        fs::write(legacy.join("database").join("library.sqlite"), b"db").unwrap();
        fs::write(legacy.join("config").join("settings.json"), b"{}").unwrap();
        fs::write(legacy.join("thumbnails").join("a.jpg"), b"img").unwrap();

        // 模拟 choose_portable_root 已经创建的空目录。
        fs::create_dir_all(&portable).unwrap();
        migrate_legacy(&legacy, &portable).unwrap();

        assert!(portable.join("database").join("library.sqlite").is_file());
        assert!(portable.join("config").join("settings.json").is_file());
        assert!(!portable.join("thumbnails").exists());
        // 旧目录仍然保留，确认迁移成功后再由用户清理。
        assert!(legacy.join("database").join("library.sqlite").is_file());
        assert!(!note.join("data.migrating").exists());

        let _ = fs::remove_dir_all(note);
    }

    #[test]
    fn migration_uses_a_consistent_database_snapshot() {
        let note = temp_dir("snapshot");
        let legacy = note.join("legacy");
        let portable = note.join("data");

        fs::create_dir_all(legacy.join("database")).unwrap();
        let source = legacy.join("database").join("library.sqlite");
        {
            let conn = rusqlite::Connection::open(&source).unwrap();
            conn.execute_batch(
                "CREATE TABLE marker (value INTEGER NOT NULL);
                 INSERT INTO marker VALUES (42);",
            )
            .unwrap();
        }

        fs::create_dir_all(&portable).unwrap();
        migrate_legacy(&legacy, &portable).unwrap();

        let copied = portable.join("database").join("library.sqlite");
        let conn = rusqlite::Connection::open(&copied).unwrap();
        let value: i64 = conn
            .query_row("SELECT value FROM marker", [], |row| row.get(0))
            .unwrap();
        assert_eq!(value, 42);

        let _ = fs::remove_dir_all(note);
    }
}
