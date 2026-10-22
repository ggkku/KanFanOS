//! 定位随安装包分发的多媒体工具。
//!
//! Tauri 的 `externalBin` 会把 sidecar 复制到主程序旁边（开发时同样如此），
//! 所以优先使用同目录下的副本；找不到时回退到系统 PATH，方便开发机直接
//! 使用已经安装好的 mpv / ffmpeg。

use std::{
    path::{Path, PathBuf},
    sync::OnceLock,
};

/// 解析要执行的外部程序。返回裸名字时由操作系统在 PATH 中查找。
pub fn resolve(program: &str) -> PathBuf {
    sidecar(program)
        .or_else(|| path_lookup(program))
        .unwrap_or_else(|| PathBuf::from(program))
}

/// 工具是否可用：安装目录里有 sidecar，或 PATH 中能找到一个可执行文件。
///
/// 用来区分「某个文件处理失败」和「整台机器缺 FFmpeg / FFprobe」，
/// 后者不应该反复重试失败任务，也不应该静默地让缩略图一直缺失。
pub fn available(program: &str) -> bool {
    sidecar(program).is_some() || path_lookup(program).is_some()
}

/// 主程序同目录下的 sidecar，例如 `C:\...\KanFanOS\ffprobe.exe`。
///
/// `current_exe` 在进程生命周期内不会变化，缓存一下，避免设置页和后台
/// 队列反复查询。
fn sidecar(program: &str) -> Option<PathBuf> {
    static EXE_DIR: OnceLock<Option<PathBuf>> = OnceLock::new();

    let dir = EXE_DIR.get_or_init(|| {
        std::env::current_exe()
            .ok()
            .and_then(|exe| exe.parent().map(Path::to_path_buf))
    });

    let candidate = sidecar_path(dir.as_deref()?, program);
    candidate.is_file().then_some(candidate)
}

fn path_lookup(program: &str) -> Option<PathBuf> {
    let name = executable_name(program);
    let paths = std::env::var_os("PATH")?;

    std::env::split_paths(&paths).find_map(|entry| {
        // PATH 里偶尔会有带引号的目录。
        let entry = entry.to_string_lossy().trim_matches('"').to_string();
        let candidate = PathBuf::from(entry).join(&name);
        candidate.is_file().then_some(candidate)
    })
}

fn executable_name(program: &str) -> String {
    #[cfg(windows)]
    {
        format!("{program}.exe")
    }

    #[cfg(not(windows))]
    {
        program.to_string()
    }
}

fn sidecar_path(dir: &Path, program: &str) -> PathBuf {
    dir.join(executable_name(program))
}

#[cfg(test)]
mod tests {
    use super::sidecar_path;
    use std::path::Path;

    #[test]
    fn appends_platform_extension() {
        let path = sidecar_path(Path::new("C:\\app"), "mpv");
        #[cfg(windows)]
        assert_eq!(path, Path::new("C:\\app").join("mpv.exe"));
        #[cfg(not(windows))]
        assert_eq!(path, Path::new("C:\\app").join("mpv"));
    }
}
