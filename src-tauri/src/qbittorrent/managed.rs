//! 随应用分发的 qBittorrent 实例。
//!
//! 应用首次连接时启动 `qbittorrent.exe`，使用独立 profile（`data_root/qbittorrent`）
//! 和随机空闲端口，只监听 127.0.0.1。退出应用时通过 WebUI 的 shutdown 接口
//! 关闭实例，超时再强制结束进程。
//!
//! qBittorrent 的 WebUI 即使允许 localhost 免登录，也必须配置非空的密码哈希
//! 才会启动，所以这里每次启动都会重写一份托管配置（端口和随机密码哈希）。

use std::{
    net::{SocketAddr, TcpListener, TcpStream},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    time::Duration,
};

use anyhow::{bail, Context, Result};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use pbkdf2::pbkdf2_hmac;
use rand::RngCore;
use sha2::Sha512;
use tokio::sync::Mutex;

/// 托管实例的 WebUI 用户名；密码每次生成后只写入哈希。
const WEBUI_USERNAME: &str = "admin";
const START_TIMEOUT: Duration = Duration::from_secs(20);
#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

#[derive(Default)]
pub struct ManagedQbittorrent {
    inner: Mutex<Option<Running>>,
}

struct Running {
    child: Child,
    port: u16,
}

impl Running {
    fn base(&self) -> String {
        format!("http://127.0.0.1:{}", self.port)
    }
}

impl ManagedQbittorrent {
    /// 确保内置实例在运行，返回 WebUI 根地址。
    ///
    /// `save_path` 只在新启动时应用；已经运行的实例由用户改目录时走 API。
    pub async fn ensure_started(
        &self,
        data_root: &Path,
        save_path: Option<&str>,
    ) -> Result<String> {
        let mut guard = self.inner.lock().await;

        if let Some(running) = guard.as_mut() {
            match running.child.try_wait() {
                Ok(None) => return Ok(running.base()),
                // 进程已经退出（崩溃或手动结束），重启一份。
                _ => *guard = None,
            }
        }

        let running = start(data_root, save_path).await?;
        let base = running.base();
        *guard = Some(running);
        Ok(base)
    }

    /// 内置实例是否正在运行。锁被占用时保守返回 true。
    pub fn is_running(&self) -> bool {
        let Ok(mut guard) = self.inner.try_lock() else {
            return true;
        };

        match guard.as_mut() {
            Some(running) => matches!(running.child.try_wait(), Ok(None)),
            None => false,
        }
    }

    /// 应用退出时调用：先请求 WebUI 关闭，超时再强制结束。
    pub fn stop(&self) {
        let mut guard = self.inner.blocking_lock();
        let Some(mut running) = guard.take() else {
            return;
        };

        request_shutdown(running.port);

        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        loop {
            match running.child.try_wait() {
                Ok(Some(_)) => return,
                Ok(None) if std::time::Instant::now() < deadline => {
                    std::thread::sleep(Duration::from_millis(100));
                }
                _ => break,
            }
        }

        let _ = running.child.kill();
        let _ = running.child.wait();
    }
}

async fn start(data_root: &Path, save_path: Option<&str>) -> Result<Running> {
    let executable = executable()?;
    let profile_root = data_root.join("qbittorrent");
    let base_dir = profile_root.join("qBittorrent");
    let config_dir = base_dir.join("config");
    let download_dir = base_dir.join("downloads");
    // qBittorrent 只在真正添加种子时创建下载目录；提前建好，界面上的提示才准确。
    for dir in [&config_dir, &download_dir] {
        std::fs::create_dir_all(dir)
            .with_context(|| format!("创建 qBittorrent 目录失败：{}", dir.display()))?;
    }

    let port = free_port()?;
    // 只在首次启动时写托管配置。之后保留 qBittorrent 自己的配置，
    // 让用户在 WebUI 里改的代理、限速等设置能持久化。
    let config_file = config_dir.join("qBittorrent.ini");
    if !config_file.exists() {
        write_config(&config_file, port)?;
    }

    crate::paths::append_log(
        data_root,
        &format!(
            "启动内置 qBittorrent：{}（127.0.0.1:{port}，profile={}）",
            executable.display(),
            profile_root.display()
        ),
    );

    let mut command = Command::new(&executable);
    command
        .arg(format!("--profile={}", profile_root.display()))
        .arg(format!("--webui-port={port}"))
        .arg("--no-splash")
        // 首次运行时跳过法律声明弹窗，避免原生窗口卡住启动。
        .arg("--confirm-legal-notice")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());

    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(CREATE_NO_WINDOW);
    }

    let mut child = command
        .spawn()
        .with_context(|| format!("无法启动 qBittorrent：{}", executable.display()))?;
    let pid = child.id();

    if let Err(error) = wait_ready(&mut child, port).await {
        let _ = child.kill();
        let _ = child.wait();
        return Err(error);
    }

    // 配置里选过下载目录时，启动后同步给 qBittorrent；失败只记日志，
    // 不影响下载（会退回 qBittorrent 的默认目录）。
    if let Some(path) = save_path.filter(|path| !path.trim().is_empty())
        && let Err(error) =
            super::apply_save_path(&format!("http://127.0.0.1:{port}"), path).await
    {
        crate::paths::append_log(
            data_root,
            &format!("设置 qBittorrent 下载目录失败：{error:#}"),
        );
    }

    // qBittorrent 是 GUI 程序，没有无界面模式：主窗口出现后隐藏它。
    // 窗口可能在 WebUI 就绪后才创建，所以过一会儿再补几次。
    hide_process_windows(pid);
    std::thread::spawn(move || {
        for _ in 0..8 {
            std::thread::sleep(Duration::from_millis(400));
            hide_process_windows(pid);
        }
    });

    Ok(Running { child, port })
}

fn executable() -> Result<PathBuf> {
    let resolved = crate::tools::resolve("qbittorrent");
    if resolved.is_file() {
        return Ok(resolved);
    }

    // 直接 `cargo run` / `cargo build` 时 Tauri CLI 不会复制 sidecar，
    // 开发构建回退到仓库里的 binaries 目录。
    #[cfg(debug_assertions)]
    {
        let dev = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("binaries")
            .join("qbittorrent-x86_64-pc-windows-msvc.exe");
        if dev.is_file() {
            return Ok(dev);
        }
    }

    bail!("找不到内置 qBittorrent，请先运行 pnpm sidecars")
}

fn free_port() -> Result<u16> {
    let listener =
        TcpListener::bind(("127.0.0.1", 0)).context("无法分配 qBittorrent WebUI 端口")?;
    Ok(listener.local_addr()?.port())
}

async fn wait_ready(child: &mut Child, port: u16) -> Result<()> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(1))
        .build()?;
    let url = format!("http://127.0.0.1:{port}/api/v2/app/version");
    let deadline = tokio::time::Instant::now() + START_TIMEOUT;

    loop {
        if let Ok(response) = client.get(&url).send().await
            && response.status().is_success()
        {
            return Ok(());
        }

        if let Some(status) = child.try_wait()? {
            bail!("qBittorrent 启动后立即退出：{status}");
        }

        if tokio::time::Instant::now() >= deadline {
            bail!("qBittorrent WebUI 启动超时（端口 {port}）");
        }

        tokio::time::sleep(Duration::from_millis(250)).await;
    }
}

/// 首次启动时写入托管配置，保证 WebUI 开启、只监听本机。
fn write_config(path: &Path, port: u16) -> Result<()> {
    let password_hash = password_hash(&random_password());
    let content = format!(
        "[LegalNotice]\n\
         Accepted=true\n\
         \n\
         [WebUI]\n\
         Enabled=true\n\
         Address=127.0.0.1\n\
         Port={port}\n\
         Username={WEBUI_USERNAME}\n\
         Password_PBKDF2={password_hash}\n\
         LocalHostAuth=false\n"
    );

    std::fs::write(path, content)
        .with_context(|| format!("写入 qBittorrent 配置失败：{}", path.display()))
}

fn random_password() -> String {
    let mut bytes = [0u8; 18];
    rand::rng().fill_bytes(&mut bytes);
    BASE64.encode(bytes)
}

/// qBittorrent 的密码格式：`base64(salt):base64(PBKDF2-HMAC-SHA512(...))`。
fn password_hash(password: &str) -> String {
    let mut salt = [0u8; 16];
    rand::rng().fill_bytes(&mut salt);
    hash_with_salt(password, &salt)
}

fn hash_with_salt(password: &str, salt: &[u8; 16]) -> String {
    let mut key = [0u8; 64];
    pbkdf2_hmac::<Sha512>(password.as_bytes(), salt, 100_000, &mut key);
    format!("{}:{}", BASE64.encode(salt), BASE64.encode(key))
}

fn request_shutdown(port: u16) {
    let address = SocketAddr::from(([127, 0, 0, 1], port));
    let Ok(mut stream) = TcpStream::connect_timeout(&address, Duration::from_secs(1)) else {
        return;
    };
    let _ = stream.set_write_timeout(Some(Duration::from_secs(1)));
    let request = format!(
        "POST /api/v2/app/shutdown HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
    );
    use std::io::Write;
    let _ = stream.write_all(request.as_bytes());
}

#[cfg(windows)]
fn hide_process_windows(pid: u32) {
    use windows::{
        core::BOOL,
        Win32::{
            Foundation::{HWND, LPARAM},
            UI::WindowsAndMessaging::{
                EnumWindows, GetWindowThreadProcessId, IsWindowVisible, ShowWindow, SW_HIDE,
            },
        },
    };

    unsafe extern "system" fn callback(hwnd: HWND, lparam: LPARAM) -> BOOL {
        let pid = lparam.0 as u32;
        let mut window_pid = 0u32;
        unsafe {
            GetWindowThreadProcessId(hwnd, Some(&mut window_pid));
            if window_pid == pid && IsWindowVisible(hwnd).as_bool() {
                let _ = ShowWindow(hwnd, SW_HIDE);
            }
        }
        BOOL(1)
    }

    unsafe {
        let _ = EnumWindows(Some(callback), LPARAM(pid as isize));
    }
}

#[cfg(not(windows))]
fn hide_process_windows(_pid: u32) {}

#[cfg(test)]
mod tests {
    use super::{hash_with_salt, ManagedQbittorrent};

    /// 固定 salt 的期望值由 Node.js crypto.pbkdf2Sync 生成，确保和 qBittorrent
    /// 的 PBKDF2-HMAC-SHA512（100000 轮、64 字节）完全一致。
    #[test]
    fn password_hash_matches_qbittorrent_format() {
        let salt: [u8; 16] = core::array::from_fn(|index| index as u8);
        assert_eq!(
            hash_with_salt("kanfanos", &salt),
            "AAECAwQFBgcICQoLDA0ODw==:/m1wQxz5+YVyj3WLJHB/HLIloNKpRPPXJzzwVudakaC/p3S/YSWKJfhDN+KEZ5yq0Fz87Dig7K8erUUl+EIRDw=="
        );
    }

    /// 真实启动一次 sidecar，验证配置生成、端口分配和退出清理。
    /// 需要 `src-tauri/binaries/qbittorrent-*.exe`，默认忽略：
    ///   cargo test --manifest-path src-tauri/Cargo.toml starts_and_stops -- --ignored
    #[tokio::test]
    #[ignore = "starts a real qBittorrent process"]
    async fn starts_and_stops_managed_instance() {
        let root = std::env::temp_dir().join(format!(
            "kanfanos-qbt-test-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();

        let save_dir = root.join("custom-downloads");
        std::fs::create_dir_all(&save_dir).unwrap();

        let manager = ManagedQbittorrent::default();
        let base = manager
            .ensure_started(&root, save_dir.to_str())
            .await
            .unwrap();
        assert!(manager.is_running());

        assert!(
            root.join("qbittorrent")
                .join("qBittorrent")
                .join("downloads")
                .is_dir()
        );

        let client = reqwest::Client::new();
        let response = client
            .get(format!("{base}/api/v2/app/version"))
            .send()
            .await
            .unwrap();
        assert!(response.status().is_success());

        // 启动时应该把配置里的下载目录写入 qBittorrent。
        let preferences: serde_json::Value = client
            .get(format!("{base}/api/v2/app/preferences"))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(
            preferences["save_path"].as_str(),
            save_dir.to_str()
        );

        tokio::task::spawn_blocking(move || manager.stop())
            .await
            .unwrap();

        let _ = std::fs::remove_dir_all(&root);
    }
}
