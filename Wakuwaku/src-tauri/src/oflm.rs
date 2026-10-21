//! The Local models page's service: Our Free Model's standalone console, run on
//! the machine's own Node and shown in the settings page as a frame (models.js).
//!
//! The console is not rewritten here, and not bundled as a second app: it is the
//! same tree Our Free Model's desktop app ships, staged by scripts/stage-ofm.mjs
//! into .runtime/ofm while developing, expanded by the installer into
//! resources/ofm, and it runs on the Node that sits beside it (a development
//! checkout uses the machine's own Node). Missing or too-old resources are said
//! plainly, so the page can offer guidance instead of a blank frame.
//!
//! One contract is read off the service: its handshake, one JSON line on stdout,
//! `{"protocol":"ofm-desktop-ready/v1", ...}` carrying the port it listens on.
//! Everything else here is process hygiene: one service however often the page is
//! asked for, a deadline, and a quit that leaves no orphan node.exe behind.

use std::collections::VecDeque;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use serde::Serialize;
use serde_json::Value;

/// How long the service may take to say where it listens (the desktop app's own
/// patience with it).
const READY_TIMEOUT: Duration = Duration::from_secs(30);
/// How long a quit waits for the service to go by itself before it is killed.
const STOP_TIMEOUT: Duration = Duration::from_secs(8);
const PROTOCOL: &str = "ofm-desktop-ready/v1";
const PRODUCT: &str = "our-free-model-standalone";
/// The console's own port (Our Free Model's desktop app takes 18900, so the two
/// can run side by side); taken, the service is asked for any port it likes.
const DEFAULT_PORT: u16 = 18901;
const HANDSHAKE_ERROR_PROTOCOL: &str = "ofm-desktop-error/v1";

/// The service, while this app runs it.
struct Oflm {
    child: Option<Child>,
    /// The address the frame shows: the console's page, with its one-time login
    /// token in the fragment.
    url: Option<String>,
    /// The service's stdout, read to the end of its life: unread, the pipe fills
    /// and the service stops writing (it logs there).
    drain: Option<Receiver<String>>,
}

static OFLM: Mutex<Oflm> = Mutex::new(Oflm { child: None, url: None, drain: None });

/// A poisoned lock still holds the truth: a panic elsewhere is no reason to
/// leave a node.exe behind.
fn held() -> MutexGuard<'static, Oflm> {
    OFLM.lock().unwrap_or_else(|error| error.into_inner())
}

/// What went wrong, in the page's terms: a code it can word itself, and the
/// plain reason too (the service's last words, Node's version, the file missing).
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OflmFailure {
    code: String,
    message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    detail: Option<String>,
}

impl OflmFailure {
    fn new(code: &str, message: impl Into<String>) -> Self {
        Self { code: code.into(), message: message.into(), detail: None }
    }

    fn detail(mut self, detail: impl Into<String>) -> Self {
        let detail = detail.into();
        if !detail.trim().is_empty() {
            self.detail = Some(detail);
        }
        self
    }
}

/// The frame's address, once the service is up.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OflmReady {
    pub url: String,
}

/// Whether the service runs, and why not if it does not.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OflmState {
    pub running: bool,
    pub url: Option<String>,
    pub error: Option<OflmFailure>,
}

fn no_console(command: &mut Command) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // CREATE_NO_WINDOW: Node's console must not flash over her.
        command.creation_flags(0x0800_0000);
    }
    #[cfg(not(windows))]
    let _ = command;
}

/// The origin of the page that will embed the console: only Wakuwaku itself may.
fn embed_ancestors(origin: &str) -> Result<String, OflmFailure> {
    let origin = origin.trim().trim_end_matches('/');
    let ours = origin == "http://tauri.localhost"
        || origin.starts_with("http://localhost:")
        || origin.starts_with("http://127.0.0.1:");
    if !ours {
        return Err(OflmFailure::new("ORIGIN_REJECTED", format!("不认识的页面来源：{origin}")));
    }
    Ok(origin.to_string())
}

#[cfg(not(feature = "installed"))]
fn find_on_path(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path).map(|directory| directory.join(name)).find(|candidate| candidate.is_file())
}

/// The machine's Node: `DSH_FISH_NODE` when set (a Node somewhere else than on
/// the PATH), else `node` on the PATH. The installed build only ever uses the
/// Node the installer expanded beside the program: no PATH look-up, no quiet
/// fall-back to a system Node that may be missing or too old.
fn node_executable() -> Result<PathBuf, OflmFailure> {
    #[cfg(feature = "installed")]
    {
        let node = crate::runtime::component("node/node.exe")
            .map_err(|e| OflmFailure::new("NODE_MISSING", e).detail("安装资源尚未就绪，请重新安装 FISH Dock"))?;
        if !node.is_file() {
            return Err(OflmFailure::new("NODE_MISSING", "内置的 Node.js 不见了")
                .detail(format!("缺少 {}；安装资源不完整，请重新安装 FISH Dock", node.display())));
        }
        return Ok(node);
    }
    #[cfg(not(feature = "installed"))]
    {
    if let Some(configured) = std::env::var_os("DSH_FISH_NODE") {
        let configured = PathBuf::from(configured);
        if configured.is_file() {
            return Ok(configured);
        }
    }
    let names: &[&str] = if cfg!(windows) { &["node.exe", "node"] } else { &["node"] };
    for name in names {
        if let Some(found) = find_on_path(name) {
            return Ok(found);
        }
    }
    Err(OflmFailure::new(
        "NODE_MISSING",
        "没找到 Node.js：本地模型服务要靠它运行，装好 Node.js 22.19 以上（或 24 以上）再来",
    ))
    }
}

/// The version the local model service runs on: `^22.19.0 || >=24` (its own
/// `engines`), asked of Node itself.
fn check_node(node: &Path) -> Result<String, OflmFailure> {
    let mut command = Command::new(node);
    command.args(["-p", "process.versions.node"]);
    no_console(&mut command);
    let output = command.output().map_err(|error| {
        OflmFailure::new("NODE_MISSING", "没能运行 node").detail(format!("{}：{error}", node.display()))
    })?;
    if !output.status.success() {
        return Err(OflmFailure::new("NODE_MISSING", "没能运行 node")
            .detail(String::from_utf8_lossy(&output.stderr).trim().to_string()));
    }
    let version = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if !node_supported(&version) {
        return Err(OflmFailure::new(
            "NODE_TOO_OLD",
            format!("Node.js {version} 太旧：本地模型服务要 22.19 以上（或 24 以上）"),
        )
        .detail(format!("{}", node.display())));
    }
    Ok(version)
}

fn node_supported(version: &str) -> bool {
    let mut parts = version.trim_start_matches('v').split('.');
    let major: u32 = parts.next().and_then(|part| part.parse().ok()).unwrap_or(0);
    let minor: u32 = parts.next().and_then(|part| part.parse().ok()).unwrap_or(0);
    (major == 22 && minor >= 19) || major >= 24
}

/// Where the staged service lives: the installer's resources when installed,
/// else `DSH_FISH_OFM_DIR` (development), else the bundle's resources, else the
/// checkout's own `.runtime/ofm` (a dev build).
fn service_directory() -> Result<PathBuf, OflmFailure> {
    #[cfg(feature = "installed")]
    {
        let directory = crate::runtime::component("ofm")
            .map_err(|e| OflmFailure::new("SERVICE_MISSING", e)
                .detail("安装资源尚未就绪，请重新安装 FISH Dock"))?;
        if !directory.join("packages/desktop/service-host.mjs").is_file() {
            return Err(OflmFailure::new("SERVICE_MISSING", "内置的本地模型服务不见了")
                .detail(format!("缺少 {}；安装资源不完整，请重新安装 FISH Dock", directory.join("packages/desktop/service-host.mjs").display())));
        }
        return Ok(directory);
    }
    #[cfg(not(feature = "installed"))]
    {
    let mut candidates: Vec<PathBuf> = Vec::new();
    if let Some(configured) = std::env::var_os("DSH_FISH_OFM_DIR") {
        candidates.push(PathBuf::from(configured));
    }
    if let Some(beside) = std::env::current_exe().ok().and_then(|exe| exe.parent().map(Path::to_path_buf)) {
        candidates.push(beside.join("resources/ofm"));
    }
    if let Some(checkout) = Path::new(env!("CARGO_MANIFEST_DIR")).parent() {
        candidates.push(checkout.join(".runtime/ofm"));
        candidates.push(checkout.join("../.runtime/ofm"));
    }
    for candidate in &candidates {
        if candidate.join("packages/desktop/service-host.mjs").is_file() {
            return Ok(candidate.clone());
        }
    }
    Err(OflmFailure::new("SERVICE_MISSING", "没找到本地模型服务的文件")
        .detail(format!("找过：{}", candidates.iter().map(|p| p.display().to_string()).collect::<Vec<_>>().join("、"))))
    }
}

/// Her own data directory, apart from Our Free Model's desktop app: `%USERPROFILE%\.dsh-fish`
/// (`~/.dsh-fish`), so both can run at once with their own accounts and locks.
pub(crate) fn data_directory() -> Result<PathBuf, OflmFailure> {
    crate::identity::from_environment()
        .map(|location| location.root)
        .map_err(|error| OflmFailure::new("DATA_DIRECTORY_INVALID", "本机身份目录配置无效").detail(error))
}

/// The address in a handshake, checked as the desktop app checks it: plain http,
/// loopback, the port it names, and a one-time token in the fragment.
fn ready_url(value: &Value, expected_port: u16) -> Result<String, String> {
    if value.get("protocol").and_then(Value::as_str) != Some(PROTOCOL) {
        return Err("握手协议不对".into());
    }
    if value.get("product").and_then(Value::as_str) != Some(PRODUCT) {
        return Err("握手里的产品不对".into());
    }
    let port = value
        .get("port")
        .and_then(Value::as_u64)
        .filter(|port| *port > 0 && *port <= 65_535)
        .ok_or_else(|| "握手里的端口不对".to_string())?;
    let url = value
        .get("managementUrl")
        .and_then(Value::as_str)
        .ok_or_else(|| "握手里没有管理地址".to_string())?;
    let rest = url.strip_prefix("http://127.0.0.1:").ok_or_else(|| "管理地址不是本机 http".to_string())?;
    let (authority, tail) = match rest.find('/') {
        Some(at) => rest.split_at(at),
        None => (rest, ""),
    };
    if authority != port.to_string() {
        return Err("管理地址的端口与握手不符".into());
    }
    if expected_port != 0 && port != u64::from(expected_port) {
        return Err(format!("服务没有落在端口 {expected_port}"));
    }
    let fragment = tail.strip_prefix("/#").ok_or_else(|| "管理地址不是控制台首页".to_string())?;
    let token = fragment.strip_prefix("login=").ok_or_else(|| "管理地址里没有登录口令".to_string())?;
    let token_ok = token.len() == 43
        && token.bytes().all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_');
    if !token_ok {
        return Err("登录口令的样子不对".into());
    }
    Ok(url.to_string())
}

/// Start the service on `port`, and read its handshake.
fn spawn(
    host: &Path,
    node: &Path,
    data_dir: &Path,
    ancestors: &str,
    port: u16,
) -> Result<(Child, String, Receiver<String>), OflmFailure> {
    let cwd = host.parent().unwrap_or(Path::new("."));
    let name = host.file_name().map(|name| name.to_owned()).unwrap_or_default();
    let mut command = Command::new(node);
    command
        .current_dir(cwd)
        .arg(name)
        .arg("--data-dir")
        .arg(data_dir)
        .arg("--port")
        .arg(port.to_string())
        // The console may be embedded by this page only: the very switch added to
        // the standalone service for a shell that shows it in a frame.
        .env("OFM_EMBED_ANCESTORS", ancestors)
        .env_remove("NODE_OPTIONS")
        .env_remove("NODE_PATH")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    no_console(&mut command);
    let mut child = command.spawn().map_err(|error| {
        OflmFailure::new("START_FAILED", "没能启动本地模型服务").detail(format!("{}：{error}", node.display()))
    })?;

    // Its stderr, kept for the reason a start failed: the last lines say why.
    let tail: Arc<Mutex<VecDeque<String>>> = Arc::new(Mutex::new(VecDeque::new()));
    if let Some(stderr) = child.stderr.take() {
        let tail = Arc::clone(&tail);
        std::thread::spawn(move || {
            for line in BufReader::new(stderr).lines().map_while(Result::ok) {
                let mut kept = tail.lock().unwrap_or_else(|error| error.into_inner());
                if kept.len() >= 20 {
                    kept.pop_front();
                }
                kept.push_back(line);
            }
        });
    }
    let last_words = move || {
        let kept = tail.lock().unwrap_or_else(|error| error.into_inner());
        kept.iter().cloned().collect::<Vec<_>>().join("\n")
    };

    let stdout = match child.stdout.take() {
        Some(stdout) => stdout,
        None => {
            let _ = child.kill();
            return Err(OflmFailure::new("START_FAILED", "服务没有输出"));
        }
    };
    let (tell, hears) = std::sync::mpsc::channel::<String>();
    std::thread::spawn(move || {
        // Read to the end of its life: the pipe stays empty, the service writing
        // its log there never blocks (the receiver is kept in Oflm).
        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            if tell.send(line).is_err() {
                break;
            }
        }
    });

    let deadline = Instant::now() + READY_TIMEOUT;
    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            let _ = child.kill();
            return Err(OflmFailure::new("START_FAILED", "本地模型服务 30 秒内没有就绪").detail(last_words()));
        }
        match hears.recv_timeout(left) {
            Ok(line) => {
                let line = line.trim();
                if !line.starts_with('{') {
                    continue;
                }
                let Ok(value) = serde_json::from_str::<Value>(line) else { continue };
                if value.get("protocol").and_then(Value::as_str) == Some(HANDSHAKE_ERROR_PROTOCOL) {
                    let code = value.get("code").and_then(Value::as_str).unwrap_or("START_FAILED");
                    let _ = child.kill();
                    if code == "DATA_DIRECTORY_IN_USE" {
                        return Err(OflmFailure::new("DATA_DIRECTORY_IN_USE", "本地模型服务已经在跑（数据目录被占用）")
                            .detail(last_words()));
                    }
                    return Err(OflmFailure::new("START_FAILED", "本地模型服务没能启动").detail(last_words()));
                }
                match ready_url(&value, port) {
                    Ok(url) => return Ok((child, url, hears)),
                    Err(reason) => {
                        let _ = child.kill();
                        return Err(OflmFailure::new("START_FAILED", "本地模型服务的握手不对")
                            .detail(format!("{reason}\n{}", last_words())));
                    }
                }
            }
            Err(RecvTimeoutError::Timeout) => {
                let _ = child.kill();
                return Err(OflmFailure::new("START_FAILED", "本地模型服务 30 秒内没有就绪").detail(last_words()));
            }
            Err(RecvTimeoutError::Disconnected) => {
                let _ = child.kill();
                return Err(OflmFailure::new("SERVICE_EXITED", "本地模型服务启动后立即退出").detail(last_words()));
            }
        }
    }
}

/// Start it: the console's own port first, and any port when that one is taken.
fn launch(
    ancestors: &str,
    data_dir: &Path,
    host: &Path,
    node: &Path,
) -> Result<(Child, String, Receiver<String>), OflmFailure> {
    let mut last: Option<OflmFailure> = None;
    for port in [DEFAULT_PORT, 0] {
        match spawn(host, node, data_dir, ancestors, port) {
            Ok(started) => return Ok(started),
            Err(failure) => {
                if failure.code != "START_FAILED" {
                    return Err(failure);
                }
                last = Some(failure);
            }
        }
    }
    Err(last.unwrap_or_else(|| OflmFailure::new("START_FAILED", "本地模型服务没能启动")))
}

fn stop(held: &mut Oflm) {
    let Some(mut child) = held.child.take() else {
        held.url = None;
        held.drain.take();
        return;
    };
    // The service's own way out: a word on its stdin, then its stdin closed.
    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(b"shutdown\n");
        let _ = stdin.flush();
    }
    let deadline = Instant::now() + STOP_TIMEOUT;
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if Instant::now() >= deadline => {
                let _ = child.kill();
                let _ = child.wait();
                break;
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(100)),
            Err(_) => {
                let _ = child.kill();
                let _ = child.wait();
                break;
            }
        }
    }
    held.drain.take();
    held.url = None;
}

/// Whatever it was started with, a quit leaves no service behind (main.rs, the
/// exit hook).
pub fn shutdown() {
    stop(&mut held());
}

/// The console's address: the service started the first time, and reused after.
#[tauri::command]
pub fn oflm_ensure(app: tauri::AppHandle, origin: String) -> Result<OflmReady, OflmFailure> {
    if crate::shared(&app).identity_busy.load(std::sync::atomic::Ordering::SeqCst) {
        return Err(OflmFailure::new("IDENTITY_BUSY", "正在处理身份文件，请稍后重试"));
    }
    resume_identity(app, origin)
}

pub(crate) fn resume_identity(app: tauri::AppHandle, origin: String) -> Result<OflmReady, OflmFailure> {
    let ancestors = embed_ancestors(&origin)?;
    let sh = crate::shared(&app);
    if let Some(error) = sh.identity_error.lock().unwrap().clone() {
        return Err(OflmFailure::new("IDENTITY_NOT_READY", "身份迁移尚未完成，请打开身份设置").detail(error));
    }
    let mut held = held();
    // Already up (and the address still good): the frame is left as it is.
    if let Some(url) = held.url.clone() {
        let alive = matches!(held.child.as_mut().map(Child::try_wait), Some(Ok(None)));
        if alive {
            return Ok(OflmReady { url });
        }
        held.child = None;
        held.url = None;
    }
    crate::shared(&app).log("local models: starting the service");
    let node = node_executable()?;
    let version = check_node(&node)?;
    let directory = service_directory()?;
    let host = directory.join("packages/desktop/service-host.mjs");
    let data_dir = sh.identity_dir.clone();
    if !data_dir.is_dir() {
        return Err(OflmFailure::new("IDENTITY_NOT_READY", "身份目录尚未准备完成"));
    }
    let started = Instant::now();
    let (child, url, drain) = launch(&ancestors, &data_dir, &host, &node)?;
    crate::shared(&app).log(&format!(
        "local models: node {version}, service up in {} ms",
        started.elapsed().as_millis()
    ));
    held.child = Some(child);
    held.url = Some(url.clone());
    held.drain = Some(drain);
    Ok(OflmReady { url })
}

/// Whether it runs, without starting it: the page's first frame, and its reason
/// when the last start failed.
#[tauri::command]
pub fn oflm_state() -> OflmState {
    let mut held = held();
    let alive = matches!(held.child.as_mut().map(Child::try_wait), Some(Ok(None)));
    if !alive {
        held.child = None;
        held.url = None;
    }
    OflmState { running: alive, url: held.url.clone(), error: None }
}

/// Stop it now (the page's own switch, and a quit).
#[tauri::command]
pub fn oflm_stop(app: tauri::AppHandle) {
    crate::shared(&app).log("local models: stopping the service");
    stop(&mut held());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn node_versions_the_service_accepts() {
        assert!(node_supported("24.14.0"));
        assert!(node_supported("22.19.0"));
        assert!(node_supported("26.0.0"));
        assert!(!node_supported("22.18.9"));
        assert!(!node_supported("23.1.0"));
        assert!(!node_supported("18.20.0"));
        assert!(!node_supported(""));
    }

    #[test]
    fn only_our_own_pages_may_embed_the_console() {
        assert_eq!(embed_ancestors("http://tauri.localhost").unwrap(), "http://tauri.localhost");
        assert_eq!(embed_ancestors("http://localhost:1420/").unwrap(), "http://localhost:1420");
        assert_eq!(embed_ancestors("http://127.0.0.1:5173").unwrap(), "http://127.0.0.1:5173");
        assert!(embed_ancestors("https://evil.example").is_err());
        assert!(embed_ancestors("http://tauri.localhost.evil.example").is_err());
        assert!(embed_ancestors("file:///tmp/x.html").is_err());
    }

    #[test]
    fn the_handshake_is_read_strictly() {
        let token = "A".repeat(43);
        let good = serde_json::json!({
            "protocol": PROTOCOL,
            "product": PRODUCT,
            "pid": 1,
            "port": 18901,
            "managementUrl": format!("http://127.0.0.1:18901/#login={token}"),
        });
        assert!(ready_url(&good, 18901).is_ok());
        assert!(ready_url(&good, 0).is_ok());
        assert!(ready_url(&good, 18902).is_err());

        let mut other = good.clone();
        other["product"] = serde_json::json!("something-else");
        assert!(ready_url(&other, 0).is_err());

        let mut short = good.clone();
        short["managementUrl"] = serde_json::json!("http://127.0.0.1:18901/#login=tooshort");
        assert!(ready_url(&short, 0).is_err());

        let mut wrong_host = good.clone();
        wrong_host["managementUrl"] = serde_json::json!(
            format!("http://evil.example:18901/#login={token}")
        );
        assert!(ready_url(&wrong_host, 0).is_err());

        let mut no_token = good;
        no_token["managementUrl"] = serde_json::json!("http://127.0.0.1:18901/");
        assert!(ready_url(&no_token, 0).is_err());
    }
}
