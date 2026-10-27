//! qBittorrent WebUI API 客户端。
//!
//! 支持两种连接方式：
//! - `sidecar`：连接应用自己启动的内置实例。实例只监听 127.0.0.1 且关闭了
//!   本机鉴权，所以不需要登录。
//! - `external`：连接用户指定地址的 WebUI，需要用账号密码登录。
//!
//! 目前只实现登录、版本查询和种子列表；后续要加添加种子、暂停/恢复、
//! 删除等操作时在这里扩展，命令层保持薄。

pub mod managed;

pub use managed::ManagedQbittorrent;

use std::{
    sync::Mutex,
    time::Duration,
};

use anyhow::{bail, Context, Result};
use reqwest::{header, Client};
use serde::{Deserialize, Serialize};

/// 一次连接的目标。`credentials` 为 `None` 时跳过登录（内置实例）。
#[derive(Debug, Clone)]
pub struct Endpoint {
    pub base: String,
    pub credentials: Option<(String, String)>,
}

impl Endpoint {
    pub fn local(base: impl Into<String>) -> Self {
        Self {
            base: base.into(),
            credentials: None,
        }
    }

    pub fn external(
        base: impl Into<String>,
        username: impl Into<String>,
        password: impl Into<String>,
    ) -> Self {
        Self {
            base: base.into(),
            credentials: Some((username.into(), password.into())),
        }
    }
}

/// 复用已登录的会话。配置变化时由命令层调用 [`QbState::invalidate`]。
#[derive(Default)]
pub struct QbState {
    session: Mutex<Option<Session>>,
}

struct Session {
    base: String,
    credentials: Option<(String, String)>,
    client: Client,
}

impl QbState {
    fn cached(&self, base: &str, credentials: &Option<(String, String)>) -> Option<Client> {
        let guard = self.session.lock().ok()?;
        guard
            .as_ref()
            .filter(|session| {
                session.base == base && session.credentials == *credentials
            })
            .map(|session| session.client.clone())
    }

    fn store(
        &self,
        base: &str,
        credentials: &Option<(String, String)>,
        client: &Client,
    ) {
        if let Ok(mut guard) = self.session.lock() {
            *guard = Some(Session {
                base: base.to_string(),
                credentials: credentials.clone(),
                client: client.clone(),
            });
        }
    }

    /// 丢弃当前会话，下一次请求重新连接。
    pub fn invalidate(&self) {
        if let Ok(mut guard) = self.session.lock() {
            *guard = None;
        }
    }
}

/// 返回给前端的种子摘要（字段名转为 camelCase）。
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all(serialize = "camelCase", deserialize = "snake_case"))]
pub struct TorrentInfo {
    pub hash: String,
    pub name: String,
    #[serde(default)]
    pub size: i64,
    #[serde(default)]
    pub progress: f64,
    #[serde(default)]
    pub dlspeed: i64,
    #[serde(default)]
    pub upspeed: i64,
    #[serde(default)]
    pub state: String,
    #[serde(default)]
    pub eta: i64,
    #[serde(default)]
    pub num_seeds: i64,
    #[serde(default)]
    pub num_leechs: i64,
    #[serde(default)]
    pub category: String,
    #[serde(default)]
    pub added_on: i64,
    /// 任务的基础保存目录，用来筛选当前「下载到」目录。
    #[serde(default)]
    pub save_path: String,
}

/// 去掉末尾斜杠并校验协议。
fn normalize_base(url: &str) -> Result<String> {
    let trimmed = url.trim().trim_end_matches('/');
    if trimmed.is_empty() {
        bail!("qBittorrent 地址不能为空");
    }

    let parsed = tauri::Url::parse(trimmed)
        .with_context(|| format!("无法解析 qBittorrent 地址：{trimmed}"))?;
    match parsed.scheme() {
        "http" | "https" => Ok(trimmed.to_string()),
        scheme => bail!("qBittorrent 地址不支持 {scheme} 协议"),
    }
}

fn build_client(base: &str) -> Result<Client> {
    // qBittorrent 的 CSRF 校验会比对 Origin / Referer，登录和写操作都依赖它。
    let mut headers = header::HeaderMap::new();
    headers.insert(header::REFERER, base.parse()?);
    headers.insert(header::ORIGIN, base.parse()?);

    Client::builder()
        .cookie_store(true)
        .timeout(Duration::from_secs(10))
        .default_headers(headers)
        .build()
        .context("创建 HTTP 客户端失败")
}

/// 登录并返回带 SID cookie 的客户端。
async fn login(base: &str, username: &str, password: &str) -> Result<Client> {
    let client = build_client(base)?;

    let response = client
        .post(format!("{base}/api/v2/auth/login"))
        .form(&[("username", username), ("password", password)])
        .send()
        .await
        .with_context(|| format!("无法连接 qBittorrent WebUI：{base}"))?;

    let status = response.status();
    if status == reqwest::StatusCode::FORBIDDEN {
        bail!("qBittorrent 用户名或密码错误（或 IP 已被封禁）");
    }
    if !status.is_success() {
        bail!("qBittorrent 登录失败（HTTP {status}）");
    }

    let body = response.text().await.unwrap_or_default();
    match body.trim() {
        // 本机免登录时登录接口返回 204 空响应，同样视为成功。
        "" | "Ok." => Ok(client),
        "Fails." => bail!("qBittorrent 用户名或密码错误"),
        other => bail!("qBittorrent 登录失败：{other}"),
    }
}

/// 获取可用的客户端；缓存未命中时重新创建（必要时登录）。
async fn client_for(state: &QbState, endpoint: &Endpoint) -> Result<(String, Client)> {
    let base = normalize_base(&endpoint.base)?;

    if let Some(client) = state.cached(&base, &endpoint.credentials) {
        return Ok((base, client));
    }

    let client = match &endpoint.credentials {
        Some((username, password)) => login(&base, username, password).await?,
        None => build_client(&base)?,
    };
    state.store(&base, &endpoint.credentials, &client);
    Ok((base, client))
}

async fn fetch_version(base: &str, client: &Client) -> Result<reqwest::Response> {
    client
        .get(format!("{base}/api/v2/app/version"))
        .send()
        .await
        .context("查询 qBittorrent 版本失败")
}

/// 查询 qBittorrent 版本，用于连接测试。
pub async fn version(state: &QbState, endpoint: &Endpoint) -> Result<String> {
    let (base, client) = client_for(state, endpoint).await?;
    let mut response = fetch_version(&base, &client).await?;

    if response.status() == reqwest::StatusCode::FORBIDDEN {
        // 会话过期：丢弃缓存后重试一次。
        state.invalidate();
        let (base, client) = client_for(state, endpoint).await?;
        response = fetch_version(&base, &client).await?;
    }

    if !response.status().is_success() {
        bail!("qBittorrent 返回 HTTP {}", response.status());
    }

    let raw = response.text().await.context("读取 qBittorrent 版本失败")?;
    let version = raw.trim();
    // qBittorrent 返回 "v5.3.0"，前端会自己加 v，统一去掉前缀。
    Ok(version.strip_prefix('v').unwrap_or(version).to_string())
}

/// Windows 路径比较：忽略大小写和斜杠差异，去掉末尾分隔符。
fn normalize_path(path: &str) -> String {
    path.replace('/', "\\")
        .trim_end_matches('\\')
        .to_ascii_lowercase()
}

/// 列出指定下载目录及其子目录下的全部任务（不区分状态），按添加时间倒序。
///
/// 只按 `save_path` 过滤；已停止、暂停、出错的任务也会显示。
/// `save_path` 为空时不做目录过滤（例如偏好设置还没读到时）。
pub async fn dir_torrents(
    state: &QbState,
    endpoint: &Endpoint,
    save_path: &str,
) -> Result<Vec<TorrentInfo>> {
    let (base, client) = client_for(state, endpoint).await?;
    let mut response = fetch_torrents(&base, &client).await?;

    if response.status() == reqwest::StatusCode::FORBIDDEN {
        // 会话过期：丢弃缓存后重试一次。
        state.invalidate();
        let (base, client) = client_for(state, endpoint).await?;
        response = fetch_torrents(&base, &client).await?;
    }

    let list = parse_torrents(response).await?;
    let wanted = normalize_path(save_path);

    Ok(list
        .into_iter()
        .filter(|torrent| {
            if wanted.is_empty() {
                return true;
            }
            let path = normalize_path(&torrent.save_path);
            // 当前目录本身以及它下面的番剧子目录都算当前下载范围。
            path == wanted || path.starts_with(&(wanted.clone() + "\\"))
        })
        .collect())
}

async fn fetch_torrents(base: &str, client: &Client) -> Result<reqwest::Response> {
    client
        .get(format!("{base}/api/v2/torrents/info"))
        .query(&[("sort", "added_on"), ("reverse", "true")])
        .send()
        .await
        .context("请求 qBittorrent 任务列表失败")
}

async fn parse_torrents(response: reqwest::Response) -> Result<Vec<TorrentInfo>> {
    if !response.status().is_success() {
        bail!("qBittorrent 返回 HTTP {}", response.status());
    }

    response
        .json::<Vec<TorrentInfo>>()
        .await
        .context("解析 qBittorrent 种子列表失败")
}

/// qBittorrent 偏好设置里前端关心的字段。
#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct Preferences {
    #[serde(default)]
    pub save_path: String,
}

async fn fetch_preferences(base: &str, client: &Client) -> Result<reqwest::Response> {
    client
        .get(format!("{base}/api/v2/app/preferences"))
        .send()
        .await
        .context("读取 qBittorrent 偏好设置失败")
}

/// 读取偏好设置（目前只用到默认下载目录）。
pub async fn preferences(state: &QbState, endpoint: &Endpoint) -> Result<Preferences> {
    let (base, client) = client_for(state, endpoint).await?;
    let mut response = fetch_preferences(&base, &client).await?;

    if response.status() == reqwest::StatusCode::FORBIDDEN {
        state.invalidate();
        let (base, client) = client_for(state, endpoint).await?;
        response = fetch_preferences(&base, &client).await?;
    }

    if !response.status().is_success() {
        bail!("qBittorrent 返回 HTTP {}", response.status());
    }

    response
        .json::<Preferences>()
        .await
        .context("解析 qBittorrent 偏好设置失败")
}

/// 设置默认下载目录。
pub async fn set_save_path(
    state: &QbState,
    endpoint: &Endpoint,
    path: &str,
) -> Result<()> {
    let (base, client) = client_for(state, endpoint).await?;
    let mut response = post_save_path(&base, &client, path).await?;

    if response.status() == reqwest::StatusCode::FORBIDDEN {
        state.invalidate();
        let (base, client) = client_for(state, endpoint).await?;
        response = post_save_path(&base, &client, path).await?;
    }

    ensure_ok(response, "设置下载目录").await
}

async fn post_save_path(
    base: &str,
    client: &Client,
    path: &str,
) -> Result<reqwest::Response> {
    let payload = serde_json::json!({ "save_path": path }).to_string();
    client
        .post(format!("{base}/api/v2/app/setPreferences"))
        .form(&[("json", payload.as_str())])
        .send()
        .await
        .context("设置 qBittorrent 下载目录失败")
}

/// 内置实例启动后应用配置里的下载目录。
pub(crate) async fn apply_save_path(base: &str, path: &str) -> Result<()> {
    let client = build_client(base)?;
    let response = post_save_path(base, &client, path).await?;
    ensure_ok(response, "设置下载目录").await
}

/// 启动或停止指定任务。
pub async fn set_running(
    state: &QbState,
    endpoint: &Endpoint,
    hash: &str,
    running: bool,
) -> Result<()> {
    let (base, client) = client_for(state, endpoint).await?;
    let action = if running { "start" } else { "stop" };
    let mut response = post_torrent_action(&base, &client, action, hash).await?;

    if response.status() == reqwest::StatusCode::FORBIDDEN {
        state.invalidate();
        let (base, client) = client_for(state, endpoint).await?;
        response = post_torrent_action(&base, &client, action, hash).await?;
    }

    ensure_ok(response, if running { "启动任务" } else { "停止任务" }).await
}

async fn post_torrent_action(
    base: &str,
    client: &Client,
    action: &str,
    hash: &str,
) -> Result<reqwest::Response> {
    client
        .post(format!("{base}/api/v2/torrents/{action}"))
        .form(&[("hashes", hash)])
        .send()
        .await
        .with_context(|| format!("请求 qBittorrent {action} 任务失败"))
}

/// 添加磁力链接或种子 URL。
pub async fn add_url(
    state: &QbState,
    endpoint: &Endpoint,
    url: &str,
    save_path: Option<&str>,
) -> Result<()> {
    let (base, client) = client_for(state, endpoint).await?;
    let mut response = post_add_url(&base, &client, url, save_path).await?;

    if response.status() == reqwest::StatusCode::FORBIDDEN {
        state.invalidate();
        let (base, client) = client_for(state, endpoint).await?;
        response = post_add_url(&base, &client, url, save_path).await?;
    }

    ensure_ok(response, "添加种子").await
}

async fn post_add_url(
    base: &str,
    client: &Client,
    url: &str,
    save_path: Option<&str>,
) -> Result<reqwest::Response> {
    let mut form: Vec<(&str, String)> = vec![("urls", url.to_string())];
    if let Some(path) = save_path.filter(|path| !path.trim().is_empty()) {
        form.push(("savepath", path.to_string()));
    }

    client
        .post(format!("{base}/api/v2/torrents/add"))
        .form(&form)
        .send()
        .await
        .context("请求 qBittorrent 添加种子失败")
}

/// 以文件内容添加种子；RSS 先用系统代理抓取 .torrent，再交给 qBittorrent。
pub async fn add_file(
    state: &QbState,
    endpoint: &Endpoint,
    bytes: &[u8],
    filename: &str,
    save_path: Option<&str>,
) -> Result<()> {
    let (base, client) = client_for(state, endpoint).await?;
    let mut response = post_add_file(&base, &client, bytes, filename, save_path).await?;

    if response.status() == reqwest::StatusCode::FORBIDDEN {
        state.invalidate();
        let (base, client) = client_for(state, endpoint).await?;
        response = post_add_file(&base, &client, bytes, filename, save_path).await?;
    }

    ensure_ok(response, "添加种子文件").await
}

async fn post_add_file(
    base: &str,
    client: &Client,
    bytes: &[u8],
    filename: &str,
    save_path: Option<&str>,
) -> Result<reqwest::Response> {
    let part = reqwest::multipart::Part::bytes(bytes.to_vec())
        .file_name(filename.to_string())
        .mime_str("application/x-bittorrent")?;
    let mut form = reqwest::multipart::Form::new().part("torrents", part);
    if let Some(path) = save_path.filter(|path| !path.trim().is_empty()) {
        form = form.text("savepath", path.to_string());
    }

    client
        .post(format!("{base}/api/v2/torrents/add"))
        .multipart(form)
        .send()
        .await
        .context("请求 qBittorrent 添加种子文件失败")
}

/// qBittorrent 的写操作返回 200/空 body 或 `Ok.`；`Fails.` 和 4xx 视为失败。
async fn ensure_ok(response: reqwest::Response, action: &str) -> Result<()> {
    let status = response.status();
    let body = response.text().await.unwrap_or_default();

    if !status.is_success() {
        bail!("{action}失败（HTTP {status}）：{body}");
    }
    if body.trim() == "Fails." {
        bail!("{action}失败：qBittorrent 拒绝了请求");
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use std::{
        io::{Read, Write},
        net::{TcpListener, TcpStream},
        thread,
    };

    use super::{
        add_file, add_url, dir_torrents, preferences, set_save_path, Endpoint, QbState,
    };

    /// 读取一个完整的 HTTP 请求（含表单 body），避免提前关闭连接导致 RST。
    fn read_request(stream: &mut TcpStream) -> String {
        let mut request = Vec::new();
        let mut buffer = [0u8; 1024];

        loop {
            let read = match stream.read(&mut buffer) {
                Ok(0) | Err(_) => break,
                Ok(read) => read,
            };
            request.extend_from_slice(&buffer[..read]);

            let Some(split) = request.windows(4).position(|w| w == b"\r\n\r\n") else {
                continue;
            };

            let headers = String::from_utf8_lossy(&request[..split]);
            let content_length = headers
                .lines()
                .find_map(|line| {
                    let (name, value) = line.split_once(':')?;
                    name.eq_ignore_ascii_case("content-length")
                        .then(|| value.trim().parse::<usize>().ok())
                        .flatten()
                })
                .unwrap_or(0);

            if request.len() >= split + 4 + content_length {
                break;
            }
        }

        String::from_utf8_lossy(&request).into_owned()
    }

    /// 最小 WebUI mock：登录返回 Ok.，种子接口返回一条记录。
    fn spawn_mock(requests: usize) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();

        thread::spawn(move || {
            for stream in listener.incoming().take(requests) {
                let Ok(mut stream) = stream else {
                    continue;
                };

                let request = read_request(&mut stream);
                let path = request.split_whitespace().nth(1).unwrap_or("");

                let (content_type, body) = if path.starts_with("/api/v2/auth/login") {
                    ("text/plain", "Ok.".to_string())
                } else if path.starts_with("/api/v2/app/preferences") {
                    (
                        "application/json",
                        r#"{"save_path":"D:\\Downloads"}"#.to_string(),
                    )
                } else if path.starts_with("/api/v2/app/setPreferences")
                    || path.starts_with("/api/v2/torrents/add")
                {
                    ("text/plain", "Ok.".to_string())
                } else {
                    (
                        "application/json",
                        r#"[{"hash":"abc","name":"测试种子","size":1024,"progress":0.5,"dlspeed":2048,"upspeed":0,"state":"downloading","eta":60,"num_seeds":3,"num_leechs":1,"category":"","added_on":1700000000,"save_path":"D:\\Downloads"},{"hash":"def","name":"已停止任务","state":"stoppedDL","save_path":"D:\\Downloads"}]"#
                            .to_string(),
                    )
                };

                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nSet-Cookie: SID=test; Path=/\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = stream.write_all(response.as_bytes());
            }
        });

        format!("http://{address}")
    }

    fn assert_torrent(list: &[super::TorrentInfo]) {
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].name, "测试种子");
        assert_eq!(list[0].num_seeds, 3);
        assert_eq!(list[0].added_on, 1_700_000_000);
        assert_eq!(list[0].save_path, "D:\\Downloads");
        assert!((list[0].progress - 0.5).abs() < f64::EPSILON);
        // 已停止的任务也要保留，只是按目录过滤。
        assert_eq!(list[1].name, "已停止任务");
        assert_eq!(list[1].state, "stoppedDL");
        assert_eq!(list[1].save_path, "D:\\Downloads");
    }

    #[tokio::test]
    async fn lists_torrents_after_login() {
        let base = spawn_mock(2);
        let state = QbState::default();
        let endpoint = Endpoint::external(base, "admin", "secret");

        let list = dir_torrents(&state, &endpoint, "d:/downloads/")
            .await
            .unwrap();

        assert_torrent(&list);
    }

    #[tokio::test]
    async fn lists_torrents_without_login_for_local_instance() {
        let base = spawn_mock(1);
        let state = QbState::default();
        let endpoint = Endpoint::local(base);

        let list = dir_torrents(&state, &endpoint, "D:\\Downloads")
            .await
            .unwrap();

        assert_torrent(&list);
    }

    #[tokio::test]
    async fn filters_torrents_by_save_path() {
        let base = spawn_mock(1);
        let state = QbState::default();
        let endpoint = Endpoint::local(base);

        let list = dir_torrents(&state, &endpoint, "E:\\Other")
            .await
            .unwrap();

        assert!(list.is_empty());
    }

    #[tokio::test]
    async fn adds_magnet_url() {
        let base = spawn_mock(1);
        let state = QbState::default();

        add_url(
            &state,
            &Endpoint::local(base),
            "magnet:?xt=urn:btih:0123456789abcdef0123456789abcdef01234567",
            Some("D:\\Downloads"),
        )
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn adds_torrent_file() {
        let base = spawn_mock(1);
        let state = QbState::default();

        add_file(
            &state,
            &Endpoint::local(base),
            b"d8:announce14:http://tracker4:infod4:name4:testee",
            "rss-test.torrent",
            Some("D:\\Downloads"),
        )
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn reads_and_sets_save_path() {
        let base = spawn_mock(2);
        let state = QbState::default();
        let endpoint = Endpoint::local(base);

        let prefs = preferences(&state, &endpoint).await.unwrap();
        assert_eq!(prefs.save_path, "D:\\Downloads");

        set_save_path(&state, &endpoint, "E:\\Anime").await.unwrap();
    }
}
