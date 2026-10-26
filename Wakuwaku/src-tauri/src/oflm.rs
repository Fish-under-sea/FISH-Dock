//! The Local models page's service: magpie's own web app, run on this machine
//! and shown in the settings page as a frame (models.js).
//!
//! It used to be Our Free Model's standalone console on Node (18901). That
//! console is gone from this page: what she shows now is magpie — the same
//! gateway this machine already routes its models through, with its accounts,
//! quotas, usage and library — whose web app is served on loopback by
//! `magpie web`. Nothing here starts a second gateway: magpie's web app stands
//! beside a running gateway ("the gateway on 127.0.0.1:3425 is served by
//! another magpie; this page shows the same settings and accounts"), and picks
//! the gateway up by itself if that one ever stops.
//!
//! One contract is read off the service: the line it prints when its page is
//! up, `● magpie web on http://127.0.0.1:<port>/?k=<key>`. Everything else
//! here is process hygiene: one service however often the page is asked for, a
//! deadline, and a quit that leaves no magpie behind.
//!
//! The key is ours and fixed (`MAGPIE_WEB_KEY`), so the session cookie the page
//! sets survives a restart of this app: reopening the settings page does not
//! ask to sign in again.

use std::collections::VecDeque;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use serde::Serialize;

/// How long the service may take to say where its page listens.
const READY_TIMEOUT: Duration = Duration::from_secs(30);
/// How long a quit waits for the service to go by itself before it is killed.
const STOP_TIMEOUT: Duration = Duration::from_secs(8);
/// The port her page asks for first. Not 3430: that one belongs to the magpie
/// the desktop shortcut starts (`magpie serve --web=127.0.0.1:3430`), and two
/// pages must not fight over it. Taken, any port is asked for instead.
const DEFAULT_PORT: u16 = 3432;
/// The sign-in key her frame carries (`MAGPIE_WEB_KEY`): 16+ letters, digits,
/// `-`, `.`, `_` or `~`. Fixed rather than random per run, so the cookie magpie
/// sets for the page lives on across restarts of this app.
const WEB_KEY: &str = "fishdock-models-frame-9f2c7a41d8e6b053";
/// The mark of the line magpie prints once its page is up: `● magpie web on
/// http://127.0.0.1:3432/?k=…`.
const READY_MARK: &str = "magpie web on ";

/// The service, while this app runs it.
struct Oflm {
    child: Option<Child>,
    /// The address her frame shows: this program's own door, not magpie's.
    url: Option<String>,
    /// The service's stdout, read to the end of its life: unread, the pipe fills
    /// and the service stops writing (it logs there).
    drain: Option<Receiver<String>>,
    /// The door onto magpie's page, while it stands.
    frame: Option<Frame>,
}

static OFLM: Mutex<Oflm> = Mutex::new(Oflm { child: None, url: None, drain: None, frame: None });

/// A poisoned lock still holds the truth: a panic elsewhere is no reason to
/// leave a magpie behind.
fn held() -> MutexGuard<'static, Oflm> {
    OFLM.lock().unwrap_or_else(|error| error.into_inner())
}

/// What went wrong, in the page's terms: a code it can word itself, and the
/// plain reason too (the service's last words, the file missing).
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
        // CREATE_NO_WINDOW: magpie's console must not flash over her.
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

fn find_on_path(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path).map(|directory| directory.join(name)).find(|candidate| candidate.is_file())
}

/// magpie itself: `MAGPIE_EXE` when set, else a copy the installer expanded
/// beside the program (`resources/magpie[.exe]`, for a build that ships one),
/// else where magpie installs itself for the current user, else on the PATH.
fn magpie_executable() -> Result<PathBuf, OflmFailure> {
    let mut tried: Vec<String> = Vec::new();

    if let Some(configured) = std::env::var_os("MAGPIE_EXE") {
        let configured = PathBuf::from(configured);
        if configured.is_file() {
            return Ok(configured);
        }
        tried.push(configured.display().to_string());
    }

    // An installed build may carry magpie in its own resources; today's does
    // not, so this is only the first of the places looked at.
    #[cfg(feature = "installed")]
    if let Ok(resources) = crate::runtime::installed_root() {
        for candidate in [resources.join("magpie/magpie.exe"), resources.join("magpie.exe")] {
            if candidate.is_file() {
                return Ok(candidate);
            }
            tried.push(candidate.display().to_string());
        }
    }

    if let Some(local) = std::env::var_os("LOCALAPPDATA") {
        let candidate = PathBuf::from(local).join("Programs/magpie/magpie.exe");
        if candidate.is_file() {
            return Ok(candidate);
        }
        tried.push(candidate.display().to_string());
    }

    let names: &[&str] = if cfg!(windows) { &["magpie.exe", "magpie"] } else { &["magpie"] };
    for name in names {
        if let Some(found) = find_on_path(name) {
            return Ok(found);
        }
    }

    Err(OflmFailure::new(
        "MAGPIE_MISSING",
        "没找到 magpie：本地模型页要靠它运行，装好 magpie 再来",
    )
    .detail(if tried.is_empty() {
        "找过：PATH".to_string()
    } else {
        format!("找过：{}、PATH", tried.join("、"))
    }))
}

/// The address in magpie's own line: `http://127.0.0.1:<port>/?k=<key>`, plain
/// http on loopback, the key it was given.
fn ready_url(line: &str, expected_port: u16) -> Result<String, String> {
    let at = line.find(READY_MARK).ok_or_else(|| "没有 magpie web 的就绪行".to_string())?;
    let url = line[at + READY_MARK.len()..].trim();
    let rest = url.strip_prefix("http://127.0.0.1:").ok_or_else(|| format!("页面地址不是本机 http：{url}"))?;
    let (authority, tail) = match rest.find('/') {
        Some(at) => rest.split_at(at),
        None => (rest, ""),
    };
    let port: u16 = authority.parse().map_err(|_| format!("页面地址的端口不对：{url}"))?;
    if expected_port != 0 && port != expected_port {
        return Err(format!("服务没有落在端口 {expected_port}"));
    }
    let key = tail.strip_prefix("/?k=").ok_or_else(|| format!("页面地址里没有登录口令：{url}"))?;
    let key_ok = key.len() >= 16
        && key.bytes().all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~'));
    if !key_ok {
        return Err(format!("登录口令的样子不对：{url}"));
    }
    if key != WEB_KEY {
        return Err("页面用的不是我们发下去的口令".to_string());
    }
    Ok(url.to_string())
}

/// Start her page on `port` (0: any), and read the line that says where it is.
fn spawn(magpie: &Path, port: u16) -> Result<(Child, String, Receiver<String>), OflmFailure> {
    let addr = if port == 0 { "127.0.0.1:0".to_string() } else { format!("127.0.0.1:{port}") };
    let mut command = Command::new(magpie);
    command
        .arg("web")
        .arg("--addr")
        .arg(&addr)
        .arg("--no-open")
        // The key her frame carries: fixed, so the page's cookie outlives a
        // restart of this app, and only Wakuwaku's own page may know it.
        .env("MAGPIE_WEB_KEY", WEB_KEY)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    no_console(&mut command);
    let mut child = command.spawn().map_err(|error| {
        OflmFailure::new("START_FAILED", "没能启动 magpie").detail(format!("{}：{error}", magpie.display()))
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
            return Err(OflmFailure::new("START_FAILED", "magpie 30 秒内没有就绪").detail(last_words()));
        }
        match hears.recv_timeout(left) {
            Ok(line) => {
                if !line.contains(READY_MARK) {
                    continue;
                }
                match ready_url(&line, port) {
                    Ok(url) => return Ok((child, url, hears)),
                    Err(reason) => {
                        let _ = child.kill();
                        return Err(OflmFailure::new("START_FAILED", "magpie 的就绪行不对")
                            .detail(format!("{reason}\n{}", last_words())));
                    }
                }
            }
            Err(RecvTimeoutError::Timeout) => {
                let _ = child.kill();
                return Err(OflmFailure::new("START_FAILED", "magpie 30 秒内没有就绪").detail(last_words()));
            }
            Err(RecvTimeoutError::Disconnected) => {
                let _ = child.kill();
                return Err(OflmFailure::new("SERVICE_EXITED", "magpie 启动后立即退出").detail(last_words()));
            }
        }
    }
}

/// Start it: her page's own port first, and any port when that one is taken.
///
/// A taken port is not a failure of the service: magpie says so on its stderr
/// and exits at once, which reaches us as a start that ended. So every way the
/// first try can end is another try on a port the system picks — only the last
/// word is ever reported.
fn launch(magpie: &Path) -> Result<(Child, String, Receiver<String>), OflmFailure> {
    let mut last: Option<OflmFailure> = None;
    for port in [DEFAULT_PORT, 0] {
        match spawn(magpie, port) {
            Ok(started) => return Ok(started),
            Err(failure) => {
                // Nothing to retry when magpie itself is not there.
                if failure.code == "MAGPIE_MISSING" {
                    return Err(failure);
                }
                last = Some(failure);
            }
        }
    }
    Err(last.unwrap_or_else(|| OflmFailure::new("START_FAILED", "magpie 没能启动")))
}

/// The door her frame is shown through.
///
/// magpie signs its page in with a cookie of its own (`magpie_web_<port>`, SameSite=Lax).
/// Her frame's origin is http://tauri.localhost, a site of its own, so the
/// browser never sends that cookie with the frame's requests: the frame would
/// show magpie's sign-in page asking for the key, every time. This door puts the
/// cookie on every request it hands over, and takes the answer's cookie back out
/// so her browser is never given one to keep.
///
/// One connection, one request: each request asks magpie to close when it is
/// done, and its answer says so too. On loopback nothing is gained by keeping a
/// connection open, and this way the door never has to know how long an answer
/// is.
struct Frame {
    /// Where her frame points.
    port: u16,
    /// Set once the service goes: the door stops taking requests.
    closing: Arc<AtomicBool>,
}

/// The cookie magpie's page is read with: its name carries the port the page
/// listens on, its value is the key it was started with.
fn frame_cookie(upstream: u16) -> String {
    format!("magpie_web_{upstream}={WEB_KEY}")
}

/// Whether an answer keeps a header: magpie's own cookie goes (it is ours to
/// send, and her browser must not keep one), and so does anything about who may
/// frame the page or where it came from — her own page is the only one that
/// shows it, and the door is the one that talks to magpie.
fn frame_keeps(header: &str) -> bool {
    let lower = header.to_ascii_lowercase();
    !(lower.starts_with("set-cookie:")
        || lower.starts_with("content-security-policy")
        || lower.starts_with("cross-origin-")
        || lower.starts_with("x-frame-options:")
        || lower.starts_with("permissions-policy:")
        || lower.starts_with("report-to:")
        || lower.starts_with("nel:"))
}

/// A request's head as the door hands it over: the same request line and fields,
/// with the host it is really for, the cookie it is read with, and a close when
/// it is done. The frame's own cookies are not hers to send.
fn frame_request(head: &str, upstream: u16) -> String {
    let mut out = String::with_capacity(head.len() + 128);
    let mut lines = head.split("\r\n");
    out.push_str(lines.next().unwrap_or("GET / HTTP/1.1"));
    out.push_str("\r\n");
    out.push_str(&format!("host: 127.0.0.1:{upstream}\r\n"));
    out.push_str(&format!("cookie: {}\r\n", frame_cookie(upstream)));
    out.push_str("connection: close\r\n");
    for line in lines {
        if line.is_empty() {
            continue;
        }
        let lower = line.to_ascii_lowercase();
        if lower.starts_with("host:")
            || lower.starts_with("cookie:")
            || lower.starts_with("connection:")
            || lower.starts_with("keep-alive:")
            || lower.starts_with("proxy-")
        {
            continue;
        }
        out.push_str(line);
        out.push_str("\r\n");
    }
    out.push_str("\r\n");
    out
}

/// Reads a head (a request's or an answer's) up to the blank line that ends it.
fn read_head(stream: &mut TcpStream) -> std::io::Result<Vec<u8>> {
    let mut head = Vec::with_capacity(2048);
    let mut byte = [0u8; 1];
    loop {
        if stream.read(&mut byte)? == 0 {
            break;
        }
        head.push(byte[0]);
        if head.len() >= 4 && head[head.len() - 4..] == *b"\r\n\r\n" {
            break;
        }
        // No page asks for anything near this much; a head that does is not one.
        if head.len() > 64 * 1024 {
            return Err(std::io::Error::other("请求头过长"));
        }
    }
    Ok(head)
}

/// How much body a request's head promises: its content-length, or a chunked
/// body carried whole (the few writes her page makes are small).
fn body_length(head: &str) -> usize {
    let mut chunked = false;
    for line in head.split("\r\n") {
        let lower = line.to_ascii_lowercase();
        if let Some(rest) = lower.strip_prefix("content-length:") {
            if let Ok(length) = rest.trim().parse::<usize>() {
                return length;
            }
        }
        if lower.starts_with("transfer-encoding:") && lower.contains("chunked") {
            chunked = true;
        }
    }
    if chunked {
        // Read to the end of the chunks below; the request asks to close, so the
        // whole body is what is left of the connection.
        usize::MAX
    } else {
        0
    }
}

/// Carries one request to magpie's page and its answer back.
fn frame_carry(client: &mut TcpStream, upstream: u16) {
    let _ = client.set_read_timeout(Some(Duration::from_secs(30)));
    let _ = client.set_write_timeout(Some(Duration::from_secs(60)));
    let Ok(head) = read_head(client) else { return };
    if head.is_empty() {
        return;
    }
    let head = String::from_utf8_lossy(&head).to_string();
    let Ok(mut page) = TcpStream::connect(("127.0.0.1", upstream)) else {
        let _ = client.write_all(b"HTTP/1.1 502 Bad Gateway\r\ncontent-length: 0\r\nconnection: close\r\n\r\n");
        return;
    };
    let _ = page.set_read_timeout(Some(Duration::from_secs(60)));
    let _ = page.set_write_timeout(Some(Duration::from_secs(60)));
    if page.write_all(frame_request(&head, upstream).as_bytes()).is_err() {
        return;
    }
    // Whatever the request carried after its head goes as it is.
    let length = body_length(&head);
    if length > 0 {
        let mut left = length;
        let mut buf = [0u8; 16 * 1024];
        while left > 0 {
            let want = buf.len().min(left);
            match client.read(&mut buf[..want]) {
                Ok(0) => break,
                Ok(read) => {
                    if page.write_all(&buf[..read]).is_err() {
                        return;
                    }
                    if left != usize::MAX {
                        left -= read;
                    }
                }
                Err(_) => return,
            }
        }
    }
    let Ok(answer) = read_head(&mut page) else { return };
    if answer.is_empty() {
        return;
    }
    let mut reply = String::with_capacity(answer.len());
    for (at, line) in String::from_utf8_lossy(&answer).split("\r\n").enumerate() {
        if line.is_empty() {
            continue;
        }
        if at > 0 && !frame_keeps(line) {
            continue;
        }
        reply.push_str(line);
        reply.push_str("\r\n");
    }
    reply.push_str("\r\n");
    if client.write_all(reply.as_bytes()).is_err() {
        return;
    }
    // The rest is the answer's body: its length was in the head, or (the request
    // asked it to close) it ends when magpie does.
    let mut buf = [0u8; 32 * 1024];
    loop {
        match page.read(&mut buf) {
            Ok(0) => break,
            Ok(read) => {
                if client.write_all(&buf[..read]).is_err() {
                    break;
                }
            }
            Err(_) => break,
        }
    }
}

/// Opens the door onto magpie's page at `upstream`, and says where it is.
fn frame_open(upstream: u16) -> Result<Frame, OflmFailure> {
    let failed = |what: &str, error: std::io::Error| {
        OflmFailure::new("FRAME_FAILED", format!("没能为 magpie 的页面开门（{what}）")).detail(error.to_string())
    };
    let listener = TcpListener::bind(("127.0.0.1", 0u16)).map_err(|error| failed("bind", error))?;
    let port = listener.local_addr().map_err(|error| failed("addr", error))?.port();
    let _ = listener.set_nonblocking(true);
    let closing = Arc::new(AtomicBool::new(false));
    let heard = Arc::clone(&closing);
    std::thread::spawn(move || {
        while !heard.load(Ordering::SeqCst) {
            match listener.accept() {
                Ok((mut client, _)) => {
                    std::thread::spawn(move || {
                        frame_carry(&mut client, upstream);
                        let _ = client.shutdown(Shutdown::Both);
                    });
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(20));
                }
                Err(_) => break,
            }
        }
    });
    Ok(Frame { port, closing })
}

/// The port magpie's own line names: `http://127.0.0.1:<port>/?k=…`.
fn port_of(url: &str) -> Option<u16> {
    let rest = url.strip_prefix("http://127.0.0.1:")?;
    rest.split('/').next()?.parse().ok()
}

fn stop(held: &mut Oflm) {
    // The door first: nothing more is taken once the service it leads to goes.
    if let Some(door) = held.frame.take() {
        door.closing.store(true, Ordering::SeqCst);
    }
    let Some(mut child) = held.child.take() else {
        held.url = None;
        held.drain.take();
        return;
    };
    // magpie has no word of its own to stop (`shutdown` on its stdin does
    // nothing): it is killed, and the child it runs goes with it.
    let _ = child.kill();
    let deadline = Instant::now() + STOP_TIMEOUT;
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if Instant::now() >= deadline => {
                let _ = child.kill();
                let _ = child.wait();
                break;
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(50)),
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

/// 在系统默认浏览器里打开一个网页（v4 §7）：登录页在置顶面板里点不开，
/// 由 dock 代开。只放行 http/https，别的（file: 之类）一律拒绝。
#[tauri::command]
pub fn oflm_open_url(url: String) -> Result<(), String> {
    let url = url.trim();
    if !(url.starts_with("http://") || url.starts_with("https://")) {
        return Err(format!("只放行 http/https，不是：{url}"));
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // ShellExecuteW 的 "open" 走文件关联，最短路径就是 start 一个 URL；
        // cmd 的 start 把 & 吃掉，所以这里用 explorer.exe 直开。
        let mut command = Command::new("explorer.exe");
        command.arg(url);
        command.creation_flags(0x0800_0000);
        command
            .spawn()
            .map(|_| ())
            .map_err(|error| format!("浏览器没能打开：{error}"))
    }
    #[cfg(not(windows))]
    {
        Command::new("xdg-open").arg(url).spawn().map(|_| ()).map_err(|error| format!("浏览器没能打开：{error}"))
    }
}

/// 登录前一步（v4 §7）：一次性解除锁定与置顶——自动取消，不走全屏询问。
#[tauri::command]
pub fn settings_step_aside(app: tauri::AppHandle) -> Value {
    let sh = crate::shared(&app);
    {
        let mut focus = sh.settings_focus.lock().unwrap();
        let now = std::time::Instant::now();
        focus.set(Some(false), Some(false), now);
    }
    crate::island::keep_on_top(&sh);
    serde_json::json!({ "aside": true })
}

/// Her page's address: the service started the first time, and reused after.
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
        held.frame = None;
    }
    crate::shared(&app).log("local models: starting magpie's page");
    let magpie = magpie_executable()?;
    let started = Instant::now();
    let (mut child, url, drain) = launch(&magpie)?;
    crate::shared(&app).log(&format!(
        "local models: magpie up in {} ms (frame origin {ancestors})",
        started.elapsed().as_millis()
    ));
    // Its own address is not the one her frame is shown: magpie signs its page
    // in with a cookie the frame's site never sends (SameSite=Lax across sites),
    // so the frame is shown a door of this program's that carries it.
    let upstream = match port_of(&url) {
        Some(upstream) => upstream,
        None => {
            let _ = child.kill();
            return Err(OflmFailure::new("START_FAILED", "magpie 的页面地址里没有端口")
                .detail(url));
        }
    };
    let door = match frame_open(upstream) {
        Ok(door) => door,
        Err(failure) => {
            let _ = child.kill();
            return Err(failure);
        }
    };
    let url = format!("http://127.0.0.1:{}/", door.port);
    crate::shared(&app).log(&format!(
        "local models: frame door on {} -> magpie {upstream}",
        door.port
    ));
    held.child = Some(child);
    held.url = Some(url.clone());
    held.drain = Some(drain);
    held.frame = Some(door);
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
        held.frame = None;
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
    fn only_our_own_pages_may_embed_the_console() {
        assert_eq!(embed_ancestors("http://tauri.localhost").unwrap(), "http://tauri.localhost");
        assert_eq!(embed_ancestors("http://localhost:1420/").unwrap(), "http://localhost:1420");
        assert_eq!(embed_ancestors("http://127.0.0.1:5173").unwrap(), "http://127.0.0.1:5173");
        assert!(embed_ancestors("https://evil.example").is_err());
        assert!(embed_ancestors("http://tauri.localhost.evil.example").is_err());
        assert!(embed_ancestors("file:///tmp/x.html").is_err());
    }

    #[test]
    fn the_ready_line_is_read_strictly() {
        let good = format!("● {READY_MARK}http://127.0.0.1:3432/?k={WEB_KEY}");
        assert_eq!(ready_url(&good, 3432).unwrap(), format!("http://127.0.0.1:3432/?k={WEB_KEY}"));
        assert!(ready_url(&good, 0).is_ok());
        assert!(ready_url(&good, 3433).is_err());

        let wrong_host = format!("● {READY_MARK}http://evil.example:3432/?k={WEB_KEY}");
        assert!(ready_url(&wrong_host, 0).is_err());

        let not_https = format!("● {READY_MARK}https://127.0.0.1:3432/?k={WEB_KEY}");
        assert!(ready_url(&not_https, 0).is_err());

        let no_key = format!("● {READY_MARK}http://127.0.0.1:3432/");
        assert!(ready_url(&no_key, 0).is_err());

        let short = format!("● {READY_MARK}http://127.0.0.1:3432/?k=short");
        assert!(ready_url(&short, 0).is_err());

        // A key that is not the one we handed down: another magpie's page.
        let other_key = format!("● {READY_MARK}http://127.0.0.1:3432/?k=someoneelseskey1234567");
        assert!(ready_url(&other_key, 0).is_err());

        assert!(ready_url("● magpie gateway on http://127.0.0.1:3425", 0).is_err());
        assert!(ready_url("", 0).is_err());
    }

    /// Her frame's site is not magpie's (http://tauri.localhost against
    /// 127.0.0.1), so magpie's SameSite=Lax cookie never comes with it: the
    /// door puts the cookie on, and hands the request on as one of its own.
    #[test]
    fn the_door_carries_her_page_s_cookie() {
        let head = "GET /app.js HTTP/1.1\r\nHost: 127.0.0.1:5555\r\nCookie: someone=else\r\nAccept: */*\r\nConnection: keep-alive\r\n";
        let sent = frame_request(head, 3432);
        let lines: Vec<&str> = sent.split("\r\n").collect();
        assert_eq!(lines[0], "GET /app.js HTTP/1.1");
        assert!(lines.contains(&"host: 127.0.0.1:3432"));
        assert!(lines.contains(&format!("cookie: magpie_web_3432={WEB_KEY}").as_str()));
        assert!(lines.contains(&"connection: close"));
        // The frame's own cookies are not hers to send, and the connection the
        // browser asked to keep open is not the door's to keep.
        assert!(!lines.iter().any(|line| line.to_ascii_lowercase().starts_with("cookie: someone")));
        assert!(!lines.iter().any(|line| line.to_ascii_lowercase().starts_with("connection: keep-alive")));
        assert!(lines.contains(&"Accept: */*"));
        assert!(sent.ends_with("\r\n\r\n"));
    }

    /// The answer keeps its page and loses the cookie: her browser must never
    /// be given magpie's sign-in to keep (it is the door that holds it), and
    /// nothing about who may frame the page is magpie's to say here.
    #[test]
    fn the_answer_keeps_no_cookie_and_no_frame_policy() {
        assert!(frame_keeps("content-type: text/html; charset=utf-8"));
        assert!(frame_keeps("content-length: 12"));
        assert!(!frame_keeps("Set-Cookie: magpie_web_3432=abc; Path=/; HttpOnly"));
        assert!(!frame_keeps("set-cookie: x=y"));
        assert!(!frame_keeps("Content-Security-Policy: frame-ancestors 'none'"));
        assert!(!frame_keeps("X-Frame-Options: DENY"));
        assert!(!frame_keeps("Cross-Origin-Opener-Policy: same-origin"));
        assert!(!frame_keeps("Permissions-Policy: geolocation=()"));
    }

    /// The port of magpie's own line, and nothing else.
    #[test]
    fn the_port_comes_from_magpie_s_own_line() {
        assert_eq!(port_of("http://127.0.0.1:3432/?k=fishdock"), Some(3432));
        assert_eq!(port_of("http://127.0.0.1:0/"), Some(0));
        assert_eq!(port_of("http://127.0.0.1:65535"), Some(65535));
        assert_eq!(port_of("https://127.0.0.1:3432/"), None);
        assert_eq!(port_of("http://evil.example:3432/"), None);
        assert_eq!(port_of("http://127.0.0.1:notaport/"), None);
    }

    /// A request and its answer go through the door as they are, the answer
    /// signed with the cookie and the request read with it: this is what makes
    /// magpie's page show as itself inside her frame.
    #[test]
    fn the_door_carries_a_request_and_its_answer() {
        use std::io::{BufRead, BufReader, Write};
        // What magpie's page does: answers every request, refusing one that
        // comes without the cookie it was started with.
        let page = TcpListener::bind(("127.0.0.1", 0u16)).unwrap();
        let upstream = page.local_addr().unwrap().port();
        let serving = std::thread::spawn(move || {
            let (mut page, _) = page.accept().unwrap();
            // One reader for the whole head: a second one would find what the
            // first buffered gone, and wait for what the page has already sent.
            let mut reader = BufReader::new(page.try_clone().unwrap());
            let mut lines = String::new();
            loop {
                let mut line = String::new();
                if reader.read_line(&mut line).unwrap() == 0 {
                    break;
                }
                if line.trim().is_empty() {
                    break;
                }
                lines.push_str(&line);
            }
            let signed = lines.contains(&format!("cookie: magpie_web_{upstream}={WEB_KEY}"));
            let body = if signed { "signed in" } else { "sign in" };
            let answer = format!(
                "HTTP/1.1 200 OK\r\ncontent-type: text/html\r\ncontent-length: {}\r\nset-cookie: magpie_web_{upstream}={WEB_KEY}; Path=/\r\n\r\n{body}",
                body.len()
            );
            page.write_all(answer.as_bytes()).unwrap();
            page.flush().unwrap();
        });

        let door = frame_open(upstream).expect("a door");
        let mut her = TcpStream::connect(("127.0.0.1", door.port)).unwrap();
        her.write_all(b"GET / HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n").unwrap();
        let mut answer = Vec::new();
        her.read_to_end(&mut answer).unwrap();
        let answer = String::from_utf8_lossy(&answer).to_string();
        // Her browser is shown the page itself, never magpie's cookie.
        assert!(answer.contains("signed in"), "{answer}");
        assert!(!answer.to_ascii_lowercase().contains("set-cookie"), "{answer}");
        serving.join().unwrap();
        door.closing.store(true, Ordering::SeqCst);
    }

    /// The key has to satisfy what magpie accepts (`16+ letters, digits, - . _
    /// ~`), or it refuses to start at all.
    #[test]
    fn the_key_is_one_magpie_accepts() {
        assert!(WEB_KEY.len() >= 16);
        assert!(WEB_KEY.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~')));
    }
}
