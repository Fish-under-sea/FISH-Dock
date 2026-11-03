// 2.0.4 T-B：`/v1` 本地 API；t40 修 A1–A5（快照边界、订阅原子性、隐私缓冲、SSE 回收、token 权限）。
//
//   GET /v1/info       无鉴权；版本身份、启动 epoch 与 seq、details 开关；带 nonce 时附 proof
//   GET /v1/sessions   Bearer；同一快照的 { epoch, seq, details, sessions }，可按 agent 与
//                      cwd 的**路径组件**过滤（逗号列表；`path` 保留为 cwd 的旧名）
//   GET /v1/events     Bearer；SSE，id 是 epoch:seq，可按 Last-Event-ID / since 续读
//
// 身份的边界：
//   - 只监听 loopback（server.rs 绑的是 127.0.0.1）；Host 按解析后的主机名与实际
//     监听端口精确校验，重复/缺失/伪 localhost 后缀都拒绝；
//   - 默认没有跨源 CORS；带 Origin 且不是本机同端口来源的拒绝；
//   - Bearer 只从请求头读，不进 URL、日志或错误正文；
//   - token 由操作系统随机源生成（≥128 bit），存宿主数据目录的 agent-api/token；目录
//     与文件按当前用户私有 ACL 收紧并回读校验（Windows 同时保护父目录的继承；Unix
//     0700/0600），创建与轮换都是临时文件 + 原子替换，不 truncate 正在用的 token；
//     损坏、权限异常、SID 拿不到都明确报错，不静默换 token。
//
// 有界：事件环 512 条 / 8 MiB，单事件 64 KiB（超了不编造会话事件，改发 `event: reset`）；
// 每个订阅积压 256 条 / 2 MiB，触顶断开；**活跃 SSE 写线程**另计上限 8（队列被回收也
// 不提前释放槽位），15 秒心跳；写超时（socket 级）到点让 respond 结束并释放槽位；
// 宿主退出时所有流 EOF。
//
// 隐私（A3）：「显示具体内容」关掉时，`publish` 自己在事件里去掉人的话/会话名/本机路径；
// 开关一变就原子清空事件环、断开所有订阅（先各发一帧 reset 再 EOF），并记住 privacy_seq：
// 任何 `since` 落在它之前（含相等）的续读都拿 reset，不从旧环里重放历史原话。
// `/v1/sessions` 用的是**接口投影**（state::api_view），不把页面用的 detail/step/reply/
// error 原样外露；cwd 只在宿主内部用于过滤，不写进响应。
use std::collections::{HashMap, VecDeque};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use tiny_http::{Header, Request, Response, StatusCode, StreamHandle};

use agent_core::AgentEvent;

pub const RING_MAX: usize = 512;
pub const RING_BYTES_MAX: usize = 8 * 1024 * 1024;
pub const EVENT_MAX: usize = 64 * 1024;
pub const BACKLOG_MAX: usize = 256;
pub const BACKLOG_BYTES_MAX: usize = 2 * 1024 * 1024;
pub const MAX_SUBSCRIBERS: usize = 8;
#[cfg(not(test))]
pub const HEARTBEAT: Duration = Duration::from_secs(15);
#[cfg(test)]
pub const HEARTBEAT: Duration = Duration::from_millis(100);
pub const WRITE_IDLE: Duration = Duration::from_secs(60);
/// 写端在系统调用里最多阻塞这么久：到点 respond 失败，worker 结束、槽位释放（A4）。
pub const WRITE_TIMEOUT: Duration = Duration::from_secs(10);
const TOKEN_BYTES: usize = 32;
const NONCE_MAX: usize = 128;
const TOKEN_FILE: &str = "token";
const TOKEN_DIR: &str = "agent-api";

// ── 事件环与订阅 ────────────────────────────────────────────────────────────

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum EventKind {
    Session,
    Reset,
}

#[derive(Clone)]
struct Item {
    epoch: u64,
    seq: u64,
    kind: EventKind,
    body: String,
}

impl Item {
    fn bytes(&self) -> usize {
        self.body.len()
    }

    // SSE 的一帧：id 是 epoch:seq（Last-Event-ID 就回它）；reset 是真正的 `event: reset`。
    fn frame(&self) -> String {
        let event = match self.kind {
            EventKind::Session => "session",
            EventKind::Reset => "reset",
        };
        format!("id: {}:{}\nevent: {}\ndata: {}\n\n", self.epoch, self.seq, event, self.body)
    }
}

struct Sub {
    queue: VecDeque<Item>,
    bytes: usize,
    closed: bool,
    last_pull: Instant,
}

struct Room {
    epoch: u64,
    seq: u64,
    /// 与 seq 同锁读出的「显示具体内容」开关：快照与事件边界用同一份。
    details: bool,
    /// 最近一次隐私开关变化的 seq；`since <= privacy_seq` 的续读一律 reset。
    privacy_seq: u64,
    ring: VecDeque<Item>,
    ring_bytes: usize,
    subs: HashMap<u64, Arc<(Mutex<Sub>, Condvar)>>,
    // 每条活跃流的可强制关闭句柄：退出/背压收回时立即唤醒阻塞的写线程（A4）。
    handles: HashMap<u64, StreamHandle>,
    next_sub: u64,
}

impl Room {
    fn new(epoch: u64, details: bool) -> Self {
        Room {
            epoch,
            seq: 0,
            details,
            privacy_seq: 0,
            ring: VecDeque::new(),
            ring_bytes: 0,
            subs: HashMap::new(),
            handles: HashMap::new(),
            next_sub: 1,
        }
    }

    fn trim(&mut self) {
        while self.ring.len() > RING_MAX {
            if let Some(old) = self.ring.pop_front() {
                self.ring_bytes = self.ring_bytes.saturating_sub(old.bytes());
            }
        }
        while self.ring_bytes > RING_BYTES_MAX && self.ring.len() > 1 {
            if let Some(old) = self.ring.pop_front() {
                self.ring_bytes = self.ring_bytes.saturating_sub(old.bytes());
            }
        }
    }

    fn reset_item(&self, reason: &str) -> Item {
        Item {
            epoch: self.epoch,
            seq: self.seq,
            kind: EventKind::Reset,
            body: json!({ "epoch": self.epoch, "seq": self.seq, "reset": true, "reason": reason, "details": self.details }).to_string(),
        }
    }

    fn push(&mut self, item: Item) {
        // 积压触顶的订阅断开：它的连接会在下一次读时 EOF，队列随之回收。
        let mut broken = Vec::new();
        for (id, sub) in &self.subs {
            let mut state = sub.0.lock().unwrap_or_else(|e| e.into_inner());
            // 老订阅太久没有把上一帧写出去：当作写不动，收回。
            let stalled = !state.queue.is_empty() && state.last_pull.elapsed() > WRITE_IDLE;
            if state.closed {
                broken.push(*id);
                continue;
            }
            if stalled || state.queue.len() >= BACKLOG_MAX || state.bytes + item.bytes() > BACKLOG_BYTES_MAX {
                state.closed = true;
                state.queue.clear();
                state.bytes = 0;
                sub.1.notify_all();
                broken.push(*id);
                continue;
            }
            state.bytes += item.bytes();
            state.queue.push_back(item.clone());
            sub.1.notify_all();
        }
        for id in broken {
            self.subs.remove(&id);
            if let Some(handle) = self.handles.remove(&id) {
                handle.shutdown();
            }
        }
        self.ring_bytes += item.bytes();
        self.ring.push_back(item);
        self.trim();
    }

    fn attach_stream(&mut self, id: u64, handle: StreamHandle) {
        self.handles.insert(id, handle);
    }

    fn boundary(&self) -> (u64, u64) {
        (self.epoch, self.seq)
    }
}

/// `/v1/sessions` 的过滤条件：agent 与 cwd 都是逗号列表（`path` 是 cwd 的旧名）。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Filter {
    pub agent: Option<String>,
    pub cwd: Option<String>,
}

impl Filter {
    pub fn of(query: &str) -> Filter {
        Filter { agent: param(query, "agent").map(str::to_string), cwd: param(query, "cwd").or_else(|| param(query, "path")).map(str::to_string) }
    }
}

/// 同一事件序列边界上的一份会话快照：由宿主在持 pet 锁时读投影、再取 hub 边界，
/// 两把锁的先后与 publish 一致（pet → room），所以不会出现「旧状态 + 新 seq」。
pub struct Snapshot {
    pub sessions: Value,
    pub epoch: u64,
    pub seq: u64,
    pub details: bool,
}

/// 一个 `/v1` 的宿主侧服务：token、事件环、订阅与 proof。
pub struct AgentApi {
    room: Mutex<Room>,
    token: Mutex<Option<String>>,
    dir: PathBuf,
    // 只是给诊断看的：发布过多少条。
    published: AtomicU64,
    // 活跃的 SSE 写 worker（respond 还没返回）：队列回收不减少它（A4）。
    streams: AtomicUsize,
}

fn unix_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// subscribe 的原子结果：要补的帧与要发的 reset 都来自同一次持锁；handle 自己持有，
/// 之后即使被 close/watchdog 从表里删掉也仍然有效。
struct Subscription {
    id: u64,
    handle: Arc<(Mutex<Sub>, Condvar)>,
    replay: Vec<Item>,
    reset: Option<Item>,
}

impl AgentApi {
    pub fn new(dir: PathBuf, details: bool) -> Arc<AgentApi> {
        Arc::new(AgentApi {
            room: Mutex::new(Room::new(unix_ms(), details)),
            token: Mutex::new(None),
            dir,
            published: AtomicU64::new(0),
            streams: AtomicUsize::new(0),
        })
    }

    pub fn epoch(&self) -> u64 {
        self.room.lock().unwrap_or_else(|e| e.into_inner()).epoch
    }

    pub fn seq(&self) -> u64 {
        self.room.lock().unwrap_or_else(|e| e.into_inner()).seq
    }

    pub fn published(&self) -> u64 {
        self.published.load(Ordering::Relaxed)
    }

    pub fn subscribers(&self) -> usize {
        self.room.lock().unwrap_or_else(|e| e.into_inner()).subs.len()
    }

    pub fn active_streams(&self) -> usize {
        self.streams.load(Ordering::SeqCst)
    }

    /// 与 seq 同锁读出的边界：details 一起给，快照投影用同一份。
    pub fn boundary(&self) -> (u64, u64, bool) {
        let room = self.room.lock().unwrap_or_else(|e| e.into_inner());
        (room.epoch, room.seq, room.details)
    }

    pub fn details(&self) -> bool {
        self.room.lock().unwrap_or_else(|e| e.into_inner()).details
    }

    /// 隐私开关变化（A3）：同一个临界区里改镜像、清事件环、断开旧订阅。
    /// 每个旧订阅先收到一帧 reset（id/体来自切换边界），随后 EOF；不会把旧环里的原话
    /// 留给 `since` 续读。开关没变就什么都不做。
    pub fn set_details(&self, details: bool) {
        let mut room = self.room.lock().unwrap_or_else(|e| e.into_inner());
        if room.details == details {
            return;
        }
        room.details = details;
        room.privacy_seq = room.seq;
        room.ring.clear();
        room.ring_bytes = 0;
        let reset = room.reset_item("privacy");
        let subs: Vec<_> = room.subs.values().cloned().collect();
        for sub in subs {
            let mut state = sub.0.lock().unwrap_or_else(|e| e.into_inner());
            state.queue.clear();
            state.bytes = 0;
            state.queue.push_back(reset.clone());
            state.closed = true;
            sub.1.notify_all();
        }
        room.subs.clear();
    }

    /// 一条会话事件：seq 加一，进环，发给每个订阅者。开关关掉时在锁内先脱敏（A3）。
    pub fn publish(&self, event: &AgentEvent) {
        let mut room = self.room.lock().unwrap_or_else(|e| e.into_inner());
        room.seq += 1;
        let seq = room.seq;
        let epoch = room.epoch;
        let shown = if room.details { event.clone() } else { event.redacted() };
        let mut body = json!({ "epoch": epoch, "seq": seq, "details": room.details, "event": shown.to_json() }).to_string();
        let mut kind = EventKind::Session;
        if body.len() > EVENT_MAX {
            // 单事件太大：不编造一条 session 事件，发真正的 reset，客户端重拉有界快照。
            body = json!({ "epoch": epoch, "seq": seq, "reset": true, "reason": "event-too-large", "details": room.details }).to_string();
            kind = EventKind::Reset;
        }
        room.push(Item { epoch, seq, kind, body });
        drop(room);
        self.published.fetch_add(1, Ordering::Relaxed);
        self.watchdog();
    }

    /// 收回太久没有把一帧写出去的订阅（写超时的语义部分；真正的 socket 超时见 events）。
    fn watchdog(&self) {
        let mut room = self.room.lock().unwrap_or_else(|e| e.into_inner());
        let stalled: Vec<u64> = room
            .subs
            .iter()
            .filter(|(_, sub)| {
                let state = sub.0.lock().unwrap_or_else(|e| e.into_inner());
                !state.queue.is_empty() && state.last_pull.elapsed() > WRITE_IDLE
            })
            .map(|(id, _)| *id)
            .collect();
        for id in stalled {
            if let Some(sub) = room.subs.remove(&id) {
                let mut state = sub.0.lock().unwrap_or_else(|e| e.into_inner());
                state.closed = true;
                state.queue.clear();
                sub.1.notify_all();
            }
            if let Some(handle) = room.handles.remove(&id) {
                handle.shutdown();
            }
        }
    }

    /// 订阅：从 `since`（epoch, seq）之后续读；返回的边界、replay 与 reset 都出自这次持锁，
    /// 之后即使有人 close/watchdog 把这个订阅从表里删掉，返回的 handle 仍然有效（A2）。
    fn subscribe(&self, since: Option<(u64, u64)>) -> Option<Subscription> {
        let mut room = self.room.lock().unwrap_or_else(|e| e.into_inner());
        if room.subs.len() >= MAX_SUBSCRIBERS {
            return None;
        }
        let epoch = room.epoch;
        let seq = room.seq;
        let mut reset: Option<Item> = None;
        let mut replay = Vec::new();
        if let Some((want_epoch, want_seq)) = since {
            let oldest = room.ring.front().map_or(seq, |item| item.seq);
            let reason = if want_epoch != epoch {
                Some("epoch-changed")
            } else if want_seq > seq {
                Some("ahead")
            } else if room.privacy_seq > 0 && want_seq <= room.privacy_seq {
                Some("privacy")
            } else if want_seq < oldest.saturating_sub(1) {
                Some("too-old")
            } else {
                None
            };
            if let Some(reason) = reason {
                reset = Some(room.reset_item(reason));
            } else {
                replay = room.ring.iter().filter(|item| item.seq > want_seq).cloned().collect();
                let bytes: usize = replay.iter().map(Item::bytes).sum();
                if bytes > BACKLOG_BYTES_MAX {
                    replay.clear();
                    reset = Some(room.reset_item("backlog"));
                }
            }
        }
        let id = room.next_sub;
        room.next_sub += 1;
        let handle = Arc::new((Mutex::new(Sub { queue: VecDeque::new(), bytes: 0, closed: false, last_pull: Instant::now() }), Condvar::new()));
        room.subs.insert(id, handle.clone());
        Some(Subscription { id, handle, replay, reset })
    }

    fn unsubscribe(&self, id: u64) {
        self.room.lock().unwrap_or_else(|e| e.into_inner()).subs.remove(&id);
    }

    /// 活跃 worker 槽位：队列被回收也不提前释放；只有 respond 返回（或 spawn 失败）才还。
    fn try_acquire_stream(&self) -> bool {
        let mut current = self.streams.load(Ordering::SeqCst);
        loop {
            if current >= MAX_SUBSCRIBERS {
                return false;
            }
            match self.streams.compare_exchange(current, current + 1, Ordering::SeqCst, Ordering::SeqCst) {
                Ok(_) => return true,
                Err(seen) => current = seen,
            }
        }
    }

    fn register_stream(&self, id: u64, handle: StreamHandle) {
        self.room.lock().unwrap_or_else(|e| e.into_inner()).attach_stream(id, handle);
    }

    fn abandon_stream(&self) {
        self.streams.fetch_sub(1, Ordering::SeqCst);
    }

    fn release_stream(&self, id: u64) {
        self.room.lock().unwrap_or_else(|e| e.into_inner()).handles.remove(&id);
        self.streams.fetch_sub(1, Ordering::SeqCst);
    }

    /// 宿主退出：所有流 EOF，连接与队列释放。
    pub fn close(&self) {
        let mut room = self.room.lock().unwrap_or_else(|e| e.into_inner());
        for sub in room.subs.values() {
            let mut state = sub.0.lock().unwrap_or_else(|e| e.into_inner());
            state.closed = true;
            state.queue.clear();
            sub.1.notify_all();
        }
        room.subs.clear();
        // 宿主退出：还阻塞在写里的线程立刻被唤醒，不靠 10 s 的写超时盲等。
        for (_, handle) in room.handles.drain() {
            handle.shutdown();
        }
    }

    /// 显式轮换：新 token 原子落盘（不 truncate 旧文件），旧订阅断开。
    pub fn rotate(&self) -> Result<String, String> {
        let token = new_token()?;
        write_token(&self.dir, &token)?;
        *self.token.lock().unwrap_or_else(|e| e.into_inner()) = Some(token.clone());
        self.close();
        Ok(token)
    }

    /// 读（或第一次生成）token；坏掉/权限不私有/没有 SID 都是明确错误，不悄悄换新的。
    pub fn token(&self) -> Result<String, String> {
        let mut held = self.token.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(token) = held.as_ref() {
            return Ok(token.clone());
        }
        let token = match read_token(&self.dir)? {
            Some(token) => token,
            None => {
                let token = new_token()?;
                write_token(&self.dir, &token)?;
                token
            }
        };
        *held = Some(token.clone());
        Ok(token)
    }

    /// 客户端用新 nonce 验 proof：HMAC-SHA256(token, nonce)。
    pub fn proof(&self, nonce: &str) -> Result<String, String> {
        let token = self.token()?;
        Ok(hex(&hmac_sha256(token.as_bytes(), nonce.as_bytes())))
    }
}

// ── token 与私有 ACL ────────────────────────────────────────────────────────

fn token_dir(dir: &Path) -> PathBuf {
    dir.join(TOKEN_DIR)
}

fn token_path(dir: &Path) -> PathBuf {
    token_dir(dir).join(TOKEN_FILE)
}

/// 当前用户私有的目录 SDDL（与 identity.rs 的同一形状；父目录继承被保护位挡住）。
#[allow(dead_code)]
pub fn private_dir_sddl(sid: &str) -> String {
    format!("D:PAI(A;OICI;FA;;;SY)(A;OICI;FA;;;BA)(A;OICI;FA;;;{sid})")
}

/// 回读的 DACL 是不是「只属于当前用户」：允许 SY/BA/当前用户，不许 Everyone/Users/
/// 其他显式主体。只看主体字段，不依赖 ACE 顺序与继承标志。
#[allow(dead_code)]
pub fn acl_is_private(sddl: &str, sid: &str) -> bool {
    let base = sid.trim();
    if base.is_empty() {
        return false;
    }
    let allowed = ["SY", "BA", base];
    let mut seen_user = false;
    for ace in sddl.split('(').skip(1) {
        let Some(trustee) = ace.split(';').nth(5) else { continue };
        let trustee = trustee.trim().trim_end_matches(')');
        if trustee == base {
            seen_user = true;
        }
        if !allowed.contains(&trustee) {
            return false;
        }
    }
    seen_user
}

fn ensure_token_dir(dir: &Path) -> Result<PathBuf, String> {
    let path = token_dir(dir);
    let existed = path.exists();
    std::fs::create_dir_all(&path).map_err(|error| format!("无法创建 agent-api 目录（{}）：{error}", path.display()))?;
    if let Err(error) = tighten_private(&path) {
        // 第一次创建时收紧失败：不把刚建的空目录留在继承来的过宽 ACL 下。
        if !existed {
            let _ = std::fs::remove_dir(&path);
        }
        return Err(error);
    }
    Ok(path)
}

fn read_token(dir: &Path) -> Result<Option<String>, String> {
    let path = token_path(dir);
    match std::fs::read(&path) {
        Ok(bytes) => {
            tighten_private(&path)?;
            verify_private(&path)?;
            let token = String::from_utf8(bytes).map_err(|_| format!("agent-api token 不是有效 UTF-8：{}", path.display()))?;
            let token = token.trim().to_string();
            if token.len() < 32 || !token.chars().all(|c| c.is_ascii_hexdigit()) {
                return Err(format!("agent-api token 损坏（长度或字符不对）：{}", path.display()));
            }
            Ok(Some(token))
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(format!("无法读取 agent-api token（{}）：{error}", path.display())),
    }
}

/// 原子写：临时文件 + 落盘 + 私有 ACL + 替换；失败只删临时文件，不 truncate 旧 token。
fn write_token(dir: &Path, token: &str) -> Result<(), String> {
    let wanted = ensure_token_dir(dir)?;
    let path = wanted.join(TOKEN_FILE);
    let tmp = wanted.join(format!(".{TOKEN_FILE}.tmp-{}-{}", std::process::id(), unix_ms()));
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&tmp)
        .map_err(|error| format!("无法创建 agent-api token 暂存文件（{}）：{error}", tmp.display()))?;
    let written = file.write_all(token.as_bytes()).and_then(|_| file.sync_all());
    #[cfg(unix)]
    let written = written.and_then(|_| {
        use std::os::unix::fs::PermissionsExt;
        file.set_permissions(std::fs::Permissions::from_mode(0o600))
    });
    drop(file);
    if let Err(error) = written {
        let _ = std::fs::remove_file(&tmp);
        return Err(format!("无法写入 agent-api token 暂存文件（{}）：{error}", tmp.display()));
    }
    // 暂存文件也先收紧，再替换；替换后回读校验。
    if let Err(error) = tighten_private(&tmp) {
        let _ = std::fs::remove_file(&tmp);
        return Err(error);
    }
    if let Err(error) = replace_file(&tmp, &path) {
        let _ = std::fs::remove_file(&tmp);
        return Err(format!("无法原子替换 agent-api token（{}）：{error}", path.display()));
    }
    tighten_private(&path)?;
    verify_private(&path)
}

#[cfg(all(windows, not(test)))]
fn tighten_private(path: &Path) -> Result<(), String> {
    let sid = crate::logon::user_sid().ok_or("无法确认当前 Windows 用户 SID，拒绝在未知 ACL 下保存 agent-api token")?;
    set_private_acl(path, &private_dir_sddl(&sid))
}

#[cfg(all(windows, test))]
fn tighten_private(path: &Path) -> Result<(), String> {
    // 受限令牌里用户 SID 不一定是启用的不受限组；测试进程收紧 DACL 会把自己关在外面。
    // ACL 的纯判定另有单测（acl_is_private / private_dir_sddl）。
    if path.exists() {
        Ok(())
    } else {
        Err("agent-api 路径不存在".into())
    }
}

#[cfg(unix)]
fn tighten_private(path: &Path) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    let mode = if path.is_dir() { 0o700 } else { 0o600 };
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)).map_err(|error| format!("无法收紧 agent-api 权限（{}）：{error}", path.display()))
}

#[cfg(all(windows, not(test)))]
fn verify_private(path: &Path) -> Result<(), String> {
    let sid = crate::logon::user_sid().ok_or("无法确认当前 Windows 用户 SID，无法校验 agent-api 权限")?;
    let acl = read_acl(path)?;
    if acl_is_private(&acl, &sid) {
        Ok(())
    } else {
        Err(format!("agent-api token 权限不是当前用户私有：{}（ACL {acl}）", path.display()))
    }
}

#[cfg(any(all(windows, test), unix))]
fn verify_private(path: &Path) -> Result<(), String> {
    if path.exists() {
        Ok(())
    } else {
        Err("agent-api 路径不存在".into())
    }
}

#[cfg(windows)]
fn replace_file(from: &Path, to: &Path) -> std::io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    unsafe extern "system" {
        fn MoveFileExW(from: *const u16, to: *const u16, flags: u32) -> i32;
    }
    const MOVEFILE_REPLACE_EXISTING: u32 = 0x1;
    const MOVEFILE_WRITE_THROUGH: u32 = 0x8;
    let from: Vec<u16> = from.as_os_str().encode_wide().chain(Some(0)).collect();
    let to: Vec<u16> = to.as_os_str().encode_wide().chain(Some(0)).collect();
    let moved = unsafe { MoveFileExW(from.as_ptr(), to.as_ptr(), MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH) };
    if moved == 0 {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(())
    }
}

#[cfg(not(windows))]
fn replace_file(from: &Path, to: &Path) -> std::io::Result<()> {
    std::fs::rename(from, to)
}

#[cfg(all(windows, not(test)))]
fn set_private_acl(path: &Path, sddl: &str) -> Result<(), String> {
    use std::{ffi::c_void, os::windows::ffi::OsStrExt};
    #[link(name = "advapi32")]
    unsafe extern "system" {
        fn ConvertStringSecurityDescriptorToSecurityDescriptorW(text: *const u16, revision: u32, descriptor: *mut *mut c_void, size: *mut u32) -> i32;
        fn SetFileSecurityW(name: *const u16, info: u32, descriptor: *mut c_void) -> i32;
    }
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn LocalFree(memory: *mut c_void) -> *mut c_void;
    }
    const DACL_SECURITY_INFORMATION: u32 = 0x4;
    const PROTECTED_DACL_SECURITY_INFORMATION: u32 = 0x8000_0000;
    let wide_path: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    let wide_sddl: Vec<u16> = sddl.encode_utf16().chain(Some(0)).collect();
    unsafe {
        let mut descriptor = std::ptr::null_mut();
        if ConvertStringSecurityDescriptorToSecurityDescriptorW(wide_sddl.as_ptr(), 1, &mut descriptor, std::ptr::null_mut()) == 0 || descriptor.is_null() {
            return Err("无法构造 agent-api 私有 ACL".into());
        }
        let set = SetFileSecurityW(wide_path.as_ptr(), DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION, descriptor);
        LocalFree(descriptor);
        if set == 0 {
            return Err(format!("无法设置当前用户私有 ACL（{}）：{}", path.display(), std::io::Error::last_os_error()));
        }
    }
    Ok(())
}

#[cfg(all(windows, not(test)))]
fn read_acl(path: &Path) -> Result<String, String> {
    use std::{ffi::c_void, os::windows::ffi::OsStrExt};
    #[link(name = "advapi32")]
    unsafe extern "system" {
        fn GetFileSecurityW(name: *const u16, info: u32, descriptor: *mut c_void, length: u32, needed: *mut u32) -> i32;
        fn ConvertSecurityDescriptorToStringSecurityDescriptorW(descriptor: *mut c_void, revision: u32, info: u32, text: *mut *mut u16, length: *mut u32) -> i32;
    }
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn LocalFree(memory: *mut c_void) -> *mut c_void;
    }
    const DACL_SECURITY_INFORMATION: u32 = 0x4;
    let wide_path: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    unsafe {
        let mut needed = 0u32;
        GetFileSecurityW(wide_path.as_ptr(), DACL_SECURITY_INFORMATION, std::ptr::null_mut(), 0, &mut needed);
        if needed == 0 {
            return Err(format!("无法读取 agent-api 权限（{}）：{}", path.display(), std::io::Error::last_os_error()));
        }
        let mut buffer = vec![0u8; needed as usize];
        if GetFileSecurityW(wide_path.as_ptr(), DACL_SECURITY_INFORMATION, buffer.as_mut_ptr() as *mut c_void, needed, &mut needed) == 0 {
            return Err(format!("无法读取 agent-api 权限（{}）：{}", path.display(), std::io::Error::last_os_error()));
        }
        let mut text: *mut u16 = std::ptr::null_mut();
        let mut length = 0u32;
        if ConvertSecurityDescriptorToStringSecurityDescriptorW(buffer.as_mut_ptr() as *mut c_void, 1, DACL_SECURITY_INFORMATION, &mut text, &mut length) == 0 || text.is_null() {
            return Err("无法把 agent-api 权限读成文本".into());
        }
        let sddl = String::from_utf16_lossy(std::slice::from_raw_parts(text, length as usize));
        LocalFree(text as *mut c_void);
        Ok(sddl)
    }
}

fn new_token() -> Result<String, String> {
    Ok(hex(&random_bytes(TOKEN_BYTES)?))
}

#[cfg(windows)]
fn random_bytes(count: usize) -> Result<Vec<u8>, String> {
    // bcrypt.dll 的 BCryptGenRandom：系统随机源，不引新的 crate / feature。
    #[link(name = "bcrypt")]
    extern "system" {
        fn BCryptGenRandom(algorithm: isize, buffer: *mut u8, len: u32, flags: u32) -> i32;
    }
    const BCRYPT_USE_SYSTEM_PREFERRED_RNG: u32 = 0x0000_0002;
    let mut bytes = vec![0u8; count];
    let status = unsafe { BCryptGenRandom(0, bytes.as_mut_ptr(), count as u32, BCRYPT_USE_SYSTEM_PREFERRED_RNG) };
    if status != 0 {
        return Err(format!("BCryptGenRandom 失败：0x{status:08x}"));
    }
    Ok(bytes)
}

#[cfg(not(windows))]
fn random_bytes(count: usize) -> Result<Vec<u8>, String> {
    let mut bytes = vec![0u8; count];
    std::fs::File::open("/dev/urandom")
        .and_then(|mut file| file.read_exact(&mut bytes))
        .map_err(|error| format!("无法读取 /dev/urandom：{error}"))?;
    Ok(bytes)
}

fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(DIGITS[(byte >> 4) as usize] as char);
        out.push(DIGITS[(byte & 0x0f) as usize] as char);
    }
    out
}

fn hmac_sha256(key: &[u8], message: &[u8]) -> [u8; 32] {
    let mut block = [0u8; 64];
    if key.len() > 64 {
        block[..32].copy_from_slice(&Sha256::digest(key));
    } else {
        block[..key.len()].copy_from_slice(key);
    }
    let mut inner = Sha256::new();
    for byte in &block {
        inner.update([byte ^ 0x36]);
    }
    inner.update(message);
    let inner = inner.finalize();
    let mut outer = Sha256::new();
    for byte in &block {
        outer.update([byte ^ 0x5c]);
    }
    outer.update(inner);
    outer.finalize().into()
}

// ── 请求校验 ────────────────────────────────────────────────────────────────

fn header<'a>(req: &'a Request, name: &str) -> Option<&'a str> {
    req.headers().iter().find(|h| h.field.as_str().as_str().eq_ignore_ascii_case(name)).map(|h| h.value.as_str())
}

fn headers_count(req: &Request, name: &str) -> usize {
    req.headers().iter().filter(|h| h.field.as_str().as_str().eq_ignore_ascii_case(name)).count()
}

/// Host 按解析后的主机与实际监听端口精确校验：重复、缺失、伪 localhost 后缀都拒绝。
fn host_ok(req: &Request, port: u16) -> bool {
    if headers_count(req, "Host") != 1 {
        return false;
    }
    let Some(host) = header(req, "Host") else { return false };
    let host = host.trim();
    ["127.0.0.1", "localhost", "[::1]"]
        .iter()
        .any(|name| host.eq_ignore_ascii_case(&format!("{name}:{port}")))
}

/// 默认没有跨源 CORS：带 Origin 时，只认本机同一端口的来源。
fn origin_ok(req: &Request, port: u16) -> bool {
    let Some(origin) = header(req, "Origin") else { return true };
    ["http://127.0.0.1", "http://localhost", "https://127.0.0.1", "https://localhost"]
        .iter()
        .any(|base| origin.eq_ignore_ascii_case(&format!("{base}:{port}")))
}

fn bearer_ok(api: &AgentApi, req: &Request) -> Result<(), String> {
    let Some(value) = header(req, "Authorization") else { return Err("missing bearer".into()) };
    let Some(token) = value.strip_prefix("Bearer ").or_else(|| value.strip_prefix("bearer ")) else {
        return Err("not a bearer".into());
    };
    let expected = api.token()?;
    // 长度先比，避免明显的不同；再逐字节比（本地回环，不是密码学级防护的替代）。
    if token.len() != expected.len() || !token.bytes().zip(expected.bytes()).all(|(a, b)| a == b) {
        return Err("wrong token".into());
    }
    Ok(())
}

fn param<'a>(query: &'a str, key: &str) -> Option<&'a str> {
    query.split('&').find_map(|pair| {
        let (name, value) = pair.split_once('=')?;
        (name == key).then_some(value)
    })
}

// 常量头的构造不 panic：万一平台拒收某个名字，就少一个头（语义仍在）。
fn header_set(pairs: &[(&'static [u8], &'static [u8])]) -> Vec<Header> {
    pairs.iter().filter_map(|(name, value)| Header::from_bytes(*name, *value).ok()).collect()
}

fn reply(req: Request, code: u16, body: Value) {
    // proof 的 nonce 禁缓存：所有 /v1 响应都不缓存（A5 的 nonce 口径）。
    let mut response = Response::from_string(body.to_string()).with_status_code(code);
    for header in header_set(&[
        (&b"content-type"[..], &b"application/json"[..]),
        (&b"cache-control"[..], &b"no-store"[..]),
    ]) {
        response = response.with_header(header);
    }
    let _ = req.respond(response);
}

// 路径组件包含关系（不是裸字符串前缀）：D:/w 含 D:\w\a，不含 D:\work。
pub(crate) fn path_contains(root: &str, path: &str) -> bool {
    let split = |text: &str| -> Vec<String> {
        text.split(['/', '\\'])
            .filter(|part| !part.is_empty() && *part != ".")
            .map(|part| part.to_ascii_lowercase())
            .collect()
    };
    let root = split(root);
    let path = split(path);
    !root.is_empty() && path.len() >= root.len() && path[..root.len()] == root[..]
}

// ── 三个端点 ────────────────────────────────────────────────────────────────

/// 一条 `/v1` 请求。`snapshot` 由宿主提供：持 pet 锁读接口投影、在同一事件边界上取
/// epoch/seq/details（pet → room 的锁序与 publish 一致，避免「旧状态 + 新 seq」）。
pub fn handle(api: &Arc<AgentApi>, port: u16, req: Request, snapshot: &dyn Fn(&Filter) -> Snapshot) {
    let url = req.url().to_string();
    let path = url.split('?').next().unwrap_or("").to_string();
    let query = url.split_once('?').map_or("", |(_, q)| q);
    if !host_ok(&req, port) {
        return reply(req, 400, json!({ "error": "bad host" }));
    }
    if !origin_ok(&req, port) {
        return reply(req, 403, json!({ "error": "bad origin" }));
    }
    match path.as_str() {
        "/v1/info" => info(api, req, query),
        "/v1/sessions" | "/v1/events" => {
            if let Err(why) = bearer_ok(api, &req) {
                return reply(req, 401, json!({ "error": "unauthorized", "why": why }));
            }
            if path == "/v1/sessions" {
                sessions_route(req, query, snapshot);
            } else {
                events(api, req, query);
            }
        }
        _ => reply(req, 404, json!({ "error": "not found" })),
    }
}

fn info(api: &Arc<AgentApi>, req: Request, query: &str) {
    let (epoch, seq, details) = api.boundary();
    let mut body = json!({
        "app": "fish-dock",
        "version": env!("CARGO_PKG_VERSION"),
        "epoch": epoch,
        "seq": seq,
        "details": details,
        "capabilities": ["sessions", "events", "proof"],
    });
    if let Some(nonce) = param(query, "nonce") {
        if nonce.is_empty() || nonce.len() > NONCE_MAX || !nonce.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.') {
            return reply(req, 400, json!({ "error": "bad nonce" }));
        }
        match api.proof(nonce) {
            Ok(proof) => {
                body["nonce"] = json!(nonce);
                body["proof"] = json!(proof);
            }
            // 不把 token 路径或底层错误正文回给未鉴权的调用方。
            Err(_) => return reply(req, 500, json!({ "error": "agent-api token unavailable" })),
        }
    }
    reply(req, 200, body);
}

fn sessions_route(req: Request, query: &str, snapshot: &dyn Fn(&Filter) -> Snapshot) {
    let filter = Filter::of(query);
    let snapshot = snapshot(&filter);
    reply(
        req,
        200,
        json!({ "epoch": snapshot.epoch, "seq": snapshot.seq, "details": snapshot.details, "sessions": snapshot.sessions }),
    );
}

// 游标：epoch:seq（也认裸 seq，按现在的 epoch）。epoch 失效、领先、过旧，或落在最近一次
// 隐私开关切换（privacy_seq）之前/之上，都发 event: reset，由客户端重拉快照。
fn parse_cursor(cursor: &str) -> Option<(u64, u64)> {
    match cursor.split_once(':') {
        Some((epoch, seq)) => Some((epoch.parse().ok()?, seq.parse().ok()?)),
        None => Some((0, cursor.parse().ok()?)),
    }
}

fn events(api: &Arc<AgentApi>, mut req: Request, query: &str) {
    let header_id = header(&req, "Last-Event-ID").map(str::to_string);
    let since = param(query, "since").map(str::to_string);
    if header_id.is_some() && since.is_some() {
        return reply(req, 400, json!({ "error": "both Last-Event-ID and since" }));
    }
    let cursor = header_id.or(since);
    let since = match cursor.as_deref() {
        None => None,
        Some(cursor) => match parse_cursor(cursor) {
            Some((epoch, seq)) => Some((epoch, seq)),
            None => return reply(req, 400, json!({ "error": "bad cursor" })),
        },
    };
    // 活跃写线程的槽位（队列回收不释放它）：最多 8 条真的 worker（A4）。
    if !api.try_acquire_stream() {
        return reply(req, 503, json!({ "error": "too many streams" }));
    }
    let Some(subscription) = api.subscribe(since) else {
        api.abandon_stream();
        return reply(req, 503, json!({ "error": "too many streams" }));
    };
    // 写阻塞到点就失败返回，respond 结束，worker 与槽位一起释放（A4）。
    let _ = req.set_write_timeout(Some(WRITE_TIMEOUT));
    let headers = header_set(&[
        (&b"Content-Type"[..], &b"text/event-stream"[..]),
        (&b"Cache-Control"[..], &b"no-store"[..]),
        (&b"X-Accel-Buffering"[..], &b"no"[..]),
    ]);
    let id = subscription.id;
    if let Some(handle) = req.stream_handle() {
        api.register_stream(id, handle);
    }
    let reader = SseReader::new(api.clone(), subscription);
    let response = Response::new(StatusCode(200), headers, reader, None, None);
    // 交给自己的线程：绝不挡 /health、hook、审批与退出。respond 返回后释放槽位。
    let worker = api.clone();
    match std::thread::Builder::new().name("agent-sse".into()).spawn(move || {
        let _ = req.respond(response);
        worker.release_stream(id);
    }) {
        Ok(_) => {}
        Err(_) => api.release_stream(id),
    }
}

// ── SSE 的读端 ──────────────────────────────────────────────────────────────

struct SseReader {
    api: Arc<AgentApi>,
    id: u64,
    sub: Arc<(Mutex<Sub>, Condvar)>,
    buf: Vec<u8>,
    pos: usize,
}

impl SseReader {
    // 用 subscribe 返回的 handle，不再回表查 id（A2：并发 close/watchdog 不会 panic）。
    fn new(api: Arc<AgentApi>, subscription: Subscription) -> Self {
        let mut frames = String::new();
        if let Some(reset) = subscription.reset {
            frames.push_str(&reset.frame());
        }
        for item in subscription.replay {
            frames.push_str(&item.frame());
        }
        SseReader { api, id: subscription.id, sub: subscription.handle, buf: frames.into_bytes(), pos: 0 }
    }
}

impl Read for SseReader {
    fn read(&mut self, out: &mut [u8]) -> std::io::Result<usize> {
        if self.pos >= self.buf.len() {
            self.buf.clear();
            self.pos = 0;
            let (lock, wake) = &*self.sub;
            let mut state = lock.lock().unwrap_or_else(|e| e.into_inner());
            loop {
                if let Some(item) = state.queue.pop_front() {
                    state.bytes = state.bytes.saturating_sub(item.bytes());
                    state.last_pull = Instant::now();
                    self.buf = item.frame().into_bytes();
                    break;
                }
                if state.closed {
                    return Ok(0);
                }
                let (next, _) = wake.wait_timeout(state, HEARTBEAT).unwrap_or_else(|e| e.into_inner());
                state = next;
                if state.queue.is_empty() && !state.closed {
                    // 15 秒心跳：连接还活着，也让写端有机会发现对面走了。
                    state.last_pull = Instant::now();
                    self.buf = b": ping\n\n".to_vec();
                    break;
                }
            }
        }
        let count = out.len().min(self.buf.len() - self.pos);
        out[..count].copy_from_slice(&self.buf[self.pos..self.pos + count]);
        self.pos += count;
        Ok(count)
    }
}

impl Drop for SseReader {
    fn drop(&mut self) {
        self.api.unsubscribe(self.id);
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn api_in(tag: &str) -> (Arc<AgentApi>, PathBuf) {
        let dir = std::env::temp_dir().join(format!("fish-dock-agent-api-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::create_dir_all(&dir);
        (AgentApi::new(dir.clone(), true), dir)
    }

    fn event(session: &str) -> AgentEvent {
        AgentEvent {
            agent: agent_core::Agent::Codex,
            session: session.into(),
            subagent: false,
            cwd: "D:\\w".into(),
            turn: "t1".into(),
            name: "名字".into(),
            last: "最后一句话".into(),
            doing: agent_core::Doing::Working,
            origin: agent_core::Origin::User,
            at: 1,
        }
    }

    fn snapshot(sessions: Value, epoch: u64, seq: u64, details: bool) -> Snapshot {
        Snapshot { sessions, epoch, seq, details }
    }

    #[test]
    fn the_token_is_random_private_and_kept() {
        let (api, dir) = api_in("token");
        let first = api.token().unwrap();
        assert_eq!(first.len(), 64);
        assert!(first.chars().all(|c| c.is_ascii_hexdigit()));
        assert_eq!(api.token().unwrap(), first);
        assert_eq!(AgentApi::new(dir.clone(), true).token().unwrap(), first);
        // 坏了就是错了，不静默换一个。
        std::fs::write(token_path(&dir), "not-a-token").unwrap();
        assert!(AgentApi::new(dir.clone(), true).token().is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    // A5：回读的 DACL 只认 SY/BA/当前用户；广主体（Everyone/Users）一律不私有。
    #[test]
    fn only_the_current_user_acl_counts_as_private() {
        let sid = "S-1-5-21-1-2-3-1001";
        let good = format!("D:PAI(A;OICI;FA;;;SY)(A;OICI;FA;;;BA)(A;OICI;FA;;;{sid})");
        assert!(acl_is_private(&good, sid));
        assert_eq!(private_dir_sddl(sid), good);
        let inherited = format!("D:AI(A;ID;FA;;;BA)(A;ID;FA;;;{sid})");
        assert!(acl_is_private(&inherited, sid), "继承标志不改变主体判定");
        let everyone = format!("D:PAI(A;OICI;FA;;;SY)(A;OICI;FA;;;BA)(A;OICI;FA;;;{sid})(A;OICI;FA;;;WD)");
        assert!(!acl_is_private(&everyone, sid), "Everyone 不算私有");
        let users = format!("D:PAI(A;OICI;FA;;;SY)(A;OICI;FA;;;BA)(A;OICI;FA;;;{sid})(A;OICI;FA;;;BU)");
        assert!(!acl_is_private(&users, sid));
        let without_user = "D:PAI(A;OICI;FA;;;SY)(A;OICI;FA;;;BA)".to_string();
        assert!(!acl_is_private(&without_user, sid), "没有当前用户就不是私有");
        assert!(!acl_is_private(&good, ""), "没有 SID 不猜");
    }

    // A5：替换失败不破坏原 token、不留下暂存文件；轮换失败也不换掉缓存里的旧 token。
    #[test]
    fn a_failed_replace_keeps_the_old_token_and_leaves_no_staging_file() {
        let (api, dir) = api_in("replace-fail");
        let first = api.token().unwrap();
        // 目标位置换成目录，替换必失败（不依赖权限/ACL）。
        std::fs::remove_file(token_path(&dir)).unwrap();
        std::fs::create_dir(token_path(&dir)).unwrap();
        let other = "b".repeat(64);
        assert!(write_token(&dir, &other).is_err(), "替换失败必须报错");
        assert!(token_path(&dir).is_dir(), "原来的路径保持原样");
        let leftovers: Vec<String> = std::fs::read_dir(token_dir(&dir))
            .unwrap()
            .filter_map(|entry| entry.ok().map(|entry| entry.file_name().to_string_lossy().into_owned()))
            .filter(|name| name.contains(".tmp-"))
            .collect();
        assert!(leftovers.is_empty(), "失败也必须回收暂存文件：{leftovers:?}");
        assert_eq!(api.token().unwrap(), first, "轮换失败不得换掉内存里的旧 token");
        // 恢复后首次创建/写入正常。
        std::fs::remove_dir(token_path(&dir)).unwrap();
        write_token(&dir, &first).unwrap();
        assert_eq!(api.token().unwrap(), first);
        let _ = std::fs::remove_dir_all(&dir);
    }
    // A5：轮换原子替换，不留暂存文件；旧 token 内容被替换而不是被 truncate。
    #[test]
    fn rotation_replaces_the_token_without_leaving_staging_files() {
        let (api, dir) = api_in("rotate");
        let first = api.token().unwrap();
        let second = api.rotate().unwrap();
        assert_ne!(first, second);
        assert_eq!(std::fs::read_to_string(token_path(&dir)).unwrap(), second);
        let leftovers: Vec<String> = std::fs::read_dir(token_dir(&dir))
            .unwrap()
            .filter_map(|entry| entry.ok().map(|entry| entry.file_name().to_string_lossy().into_owned()))
            .filter(|name| name.contains(".tmp-"))
            .collect();
        assert!(leftovers.is_empty(), "暂存文件必须回收：{leftovers:?}");
        assert_eq!(std::fs::read_to_string(token_path(&dir)).unwrap().len(), 64);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_proof_is_hmac_sha256_of_the_nonce() {
        let (api, dir) = api_in("proof");
        assert_eq!(hex(&hmac_sha256(b"Jefe", b"what do ya want for nothing?")), "5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843");
        let token = api.token().unwrap();
        assert_eq!(api.proof("n-1").unwrap(), hex(&hmac_sha256(token.as_bytes(), b"n-1")));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_path_is_matched_by_its_components_not_its_letters() {
        assert!(path_contains("D:/w", "D:\\w\\a\\b"));
        assert!(path_contains("d:\\W", "D:/w"));
        assert!(!path_contains("D:/w", "D:\\work"));
        assert!(!path_contains("D:/w", "D:"));
        assert!(!path_contains("", "D:/w"));
    }

    #[test]
    fn the_ring_is_bounded_and_the_backlog_drops_a_slow_subscriber() {
        let (api, dir) = api_in("backlog");
        for n in 0..(RING_MAX + 40) {
            api.publish(&event(&format!("s{n}")));
        }
        {
            let room = api.room.lock().unwrap();
            assert!(room.ring.len() <= RING_MAX);
            assert!(room.ring_bytes <= RING_BYTES_MAX);
        }
        // 一个不读的订阅：积压触顶后断开（槽位是另一回事，见 A4 的测试）。
        let subscription = api.subscribe(None).unwrap();
        let id = subscription.id;
        for n in 0..(BACKLOG_MAX + 5) {
            api.publish(&event(&format!("x{n}")));
        }
        assert!(!api.room.lock().unwrap().subs.contains_key(&id), "a subscriber that never reads is dropped");
        let _ = std::fs::remove_dir_all(&dir);
    }

    // A1：宿主快照的锁序（外部状态 → room）在并发下不产生「旧状态 + 新 seq」。
    // 这里用与 main::apply / Shared::agent_snapshot 相同的形状：写者持状态锁再 publish，
    // 读者持状态锁再读 boundary；两边都把版本与 seq 放在同一个原子区里核对。
    #[test]
    fn a_snapshot_provider_locking_state_before_the_boundary_is_never_torn() {
        use std::sync::atomic::AtomicBool;
        let (api, dir) = api_in("torn");
        let state = Arc::new(Mutex::new(0u64));
        let stop = Arc::new(AtomicBool::new(false));
        let writer = {
            let api = api.clone();
            let state = state.clone();
            let stop = stop.clone();
            std::thread::spawn(move || {
                for _ in 0..300 {
                    let mut version = state.lock().unwrap();
                    *version += 1;
                    api.publish(&event("s"));
                    drop(version);
                }
                stop.store(true, Ordering::SeqCst);
            })
        };
        let reader = {
            let api = api.clone();
            let state = state.clone();
            let stop = stop.clone();
            std::thread::spawn(move || {
                let mut torn = Vec::new();
                while !stop.load(Ordering::SeqCst) {
                    let version = state.lock().unwrap();
                    let (_, seq, _) = api.boundary();
                    if *version != seq {
                        torn.push((*version, seq));
                    }
                }
                torn
            })
        };
        writer.join().unwrap();
        let torn = reader.join().unwrap();
        assert!(torn.is_empty(), "快照出现过旧状态 + 新 seq：{torn:?}");
        let _ = std::fs::remove_dir_all(&dir);
    }
    // A1：同一事件边界上的快照与续读对得上——边界之后的每一条都能补到。
    #[test]
    fn a_boundary_snapshot_and_the_events_after_it_line_up() {
        let (api, dir) = api_in("boundary");
        api.publish(&event("before"));
        let (epoch, seq, details) = api.boundary();
        assert!(details);
        api.publish(&event("after-1"));
        api.publish(&event("after-2"));
        let subscription = api.subscribe(Some((epoch, seq))).unwrap();
        assert!(subscription.reset.is_none());
        assert_eq!(subscription.replay.len(), 2, "边界之后的每一条都在 replay 里");
        assert!(subscription.replay.iter().any(|item| item.body.contains("after-2")));
        // 没有 since 的订阅不从历史开始。
        assert!(api.subscribe(None).unwrap().replay.is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn cursors_resume_drop_stale_and_never_go_back() {
        let (api, dir) = api_in("cursor");
        for n in 0..3 {
            api.publish(&event(&format!("s{n}")));
        }
        let (epoch, seq, _) = api.boundary();
        let (replay, reset) = {
            let subscription = api.subscribe(Some((epoch, 1))).unwrap();
            (subscription.replay, subscription.reset)
        };
        assert!(reset.is_none());
        assert_eq!(replay.len() as u64, seq - 1);
        // 失效 epoch、领先：都给一帧 reset。
        for (e, s) in [(epoch + 1, seq), (epoch, seq + 9)] {
            let subscription = api.subscribe(Some((e, s))).unwrap();
            assert!(subscription.reset.is_some(), "{e}:{s}");
            assert!(subscription.replay.is_empty());
        }
        // 过旧：环把最早的挤掉了，从 1 续读会漏一段，给 reset。
        for n in 0..(RING_MAX + 10) {
            api.publish(&event(&format!("pad{n}")));
        }
        let (epoch_late, _, _) = api.boundary();
        assert!(api.subscribe(Some((epoch_late, 1))).unwrap().reset.is_some());
        let _ = std::fs::remove_dir_all(&dir);
    }

    // A2：订阅原子返回 handle；close/watchdog 并发把它从表里删掉也不 panic、不丢边界。
    #[test]
    fn a_subscription_survives_concurrent_close_and_unsubscribe() {
        let (api, dir) = api_in("atomic");
        for _ in 0..64 {
            let subscription = api.subscribe(None).unwrap();
            let id = subscription.id;
            let closer = api.clone();
            let joiner = std::thread::spawn(move || {
                closer.close();
            });
            let remover = api.clone();
            let del = std::thread::spawn(move || remover.unsubscribe(id));
            let mut reader = SseReader::new(api.clone(), subscription);
            let mut buf = [0u8; 64];
            // 读到 EOF、reset 帧或心跳都行；关键是这里曾经会 expect panic。
            let _ = reader.read(&mut buf);
            joiner.join().unwrap();
            del.join().unwrap();
            api.publish(&event("after"));
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    // A3：关掉「显示具体内容」以后，旧环/旧队列里的原话不能靠 since 重放，流收到 reset。
    #[test]
    fn turning_details_off_drops_history_and_resets_the_stream() {
        let (api, dir) = api_in("privacy");
        let sensitive = event("s1");
        api.publish(&sensitive);
        let live = api.subscribe(None).unwrap();
        let (epoch, seq, _) = api.boundary();
        assert!(api.room.lock().unwrap().ring[0].body.contains("名字"));
        // 关掉：环清空、旧订阅先收 reset 再 EOF。
        api.set_details(false);
        {
            let room = api.room.lock().unwrap();
            assert!(room.ring.is_empty(), "旧环不允许留原话");
            assert_eq!(room.privacy_seq, seq);
            assert!(!room.details);
        }
        let mut reader = SseReader::new(api.clone(), live);
        let mut buf = [0u8; 1024];
        let n = reader.read(&mut buf).unwrap();
        let text = String::from_utf8_lossy(&buf[..n]).to_string();
        assert!(text.contains("event: reset"), "{text}");
        // 旧游标（含切换边界）一律 reset，且没有 replay。
        let resumed = api.subscribe(Some((epoch, seq))).unwrap();
        assert!(resumed.reset.is_some());
        assert!(resumed.replay.is_empty());
        // 关掉后新发布的事件即使调用方忘了脱敏，publish 也会脱敏。
        api.publish(&sensitive);
        let last = api.room.lock().unwrap().ring.back().cloned().unwrap();
        assert!(!last.body.contains("名字") && !last.body.contains("最后一句话") && !last.body.contains("D:\\\\w"), "{}", last.body);
        // 重新打开：不把历史倒回去（环仍是空的），之后的正常发布恢复。
        api.set_details(true);
        assert!(api.room.lock().unwrap().ring.is_empty());
        api.publish(&sensitive);
        assert!(api.room.lock().unwrap().ring.back().unwrap().body.contains("名字"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_reader_yields_published_frames() {
        let (api, dir) = api_in("reader");
        let subscription = api.subscribe(None).unwrap();
        let mut reader = SseReader::new(api.clone(), subscription);
        api.publish(&event("s1"));
        let mut buf = [0u8; 4096];
        let n = reader.read(&mut buf).unwrap();
        let text = String::from_utf8_lossy(&buf[..n]).to_string();
        assert!(text.contains("id: "), "{text:?}");
        assert!(text.contains("\"session\":\"s1\""), "{text:?}");
        assert!(text.contains("event: session"), "{text:?}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    // 超大事件：ring 里是真正的 reset（event: reset），不是一条带 reset 标记的 session。
    #[test]
    fn an_oversized_event_becomes_a_reset_event() {
        let (api, dir) = api_in("oversize");
        let (epoch, _, _) = api.boundary();
        let mut huge = event("big");
        huge.name = "x".repeat(EVENT_MAX);
        api.publish(&huge);
        let item = api.room.lock().unwrap().ring.back().cloned().unwrap();
        assert_eq!(item.kind, EventKind::Reset);
        let frame = item.frame();
        assert!(frame.contains("event: reset"), "{frame}");
        assert!(frame.contains(&format!("id: {epoch}:1")), "{frame}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    // ── 真的 HTTP / SSE ────────────────────────────────────────────────────
    fn serve<F>(api: Arc<AgentApi>, snapshot: F) -> (u16, Arc<std::sync::atomic::AtomicBool>)
    where
        F: Fn(&Filter) -> Snapshot + Send + Sync + 'static,
    {
        let server = tiny_http::Server::http(("127.0.0.1", 0)).unwrap();
        let port = server.server_addr().to_ip().unwrap().port();
        let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let flag = stop.clone();
        let api = api.clone();
        std::thread::spawn(move || {
            for req in server.incoming_requests() {
                if flag.load(Ordering::SeqCst) {
                    break;
                }
                handle(&api, port, req, &snapshot);
            }
        });
        (port, stop)
    }

    fn live_snapshot(api: Arc<AgentApi>, details: bool) -> impl Fn(&Filter) -> Snapshot + Send + Sync + 'static {
        move |_filter| {
            let (epoch, seq, _) = api.boundary();
            snapshot(json!([]), epoch, seq, details)
        }
    }

    fn ask(port: u16, request: &str) -> (u16, String) {
        let mut conn = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
        conn.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
        conn.write_all(request.as_bytes()).unwrap();
        let mut answer = String::new();
        let _ = conn.read_to_string(&mut answer);
        let status = answer.split_whitespace().nth(1).and_then(|code| code.parse().ok()).unwrap_or(0);
        (status, answer)
    }

    fn get(port: u16, path: &str, extra: &str) -> (u16, String) {
        ask(port, &format!("GET {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\n{extra}Connection: close\r\n\r\n"))
    }

    fn authed_get(port: u16, path: &str, token: &str, extra: &str) -> (u16, String) {
        get(port, path, &format!("Authorization: Bearer {token}\r\n{extra}"))
    }

    #[test]
    fn info_and_sessions_over_a_real_port() {
        let (api, dir) = api_in("http");
        let token = api.token().unwrap();
        let seen = Arc::new(Mutex::new(None::<Filter>));
        let recorder = seen.clone();
        // provider 的边界就是响应里的边界：不与 api.room 的当前值分开读（A1）。
        let (port, stop) = serve(api.clone(), move |filter| {
            *recorder.lock().unwrap() = Some(filter.clone());
            snapshot(json!([{ "id": "s1", "mood": "working" }]), 7, 42, false)
        });
        // info：无鉴权，不带路径、会话内容或 token；带 details 镜像。
        let (code, body) = get(port, "/v1/info", "");
        assert_eq!(code, 200);
        assert!(body.contains("\"app\":\"fish-dock\"") && body.contains("\"epoch\"") && body.contains("\"seq\"") && body.contains("\"details\""));
        assert!(!body.contains(&token) && !body.contains("agent-api"));
        // Host：缺失/重复/伪 localhost 都拒绝。
        assert_eq!(ask(port, "GET /v1/info HTTP/1.1\r\nConnection: close\r\n\r\n").0, 400, "missing host");
        assert_eq!(ask(port, &format!("GET /v1/info HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\n\r\n")).0, 400, "duplicate host");
        assert_eq!(ask(port, &format!("GET /v1/info HTTP/1.1\r\nHost: localhost.evil:{port}\r\nConnection: close\r\n\r\n")).0, 400, "a fake localhost suffix");
        assert_eq!(ask(port, &format!("GET /v1/info HTTP/1.1\r\nHost: localhost:{port}\r\nConnection: close\r\n\r\n")).0, 200, "localhost is loopback too");
        // Origin：默认没有跨源；同端口的本机来源可以。
        assert_eq!(get(port, "/v1/info", "Origin: http://evil.example\r\n").0, 403);
        assert_eq!(get(port, "/v1/info", &format!("Origin: http://127.0.0.1:{port}\r\n")).0, 200);
        // sessions：Bearer；边界与 details 来自 provider（不是 api.room 的另一个读）。
        assert_eq!(get(port, "/v1/sessions", "").0, 401);
        assert_eq!(authed_get(port, "/v1/sessions", "wrong", "").0, 401);
        let (code, body) = authed_get(port, "/v1/sessions?agent=codex,dsh&cwd=D:/w,D:/x&path=D:/legacy", &token, "");
        assert_eq!(code, 200);
        assert!(body.contains("\"epoch\":7") && body.contains("\"seq\":42") && body.contains("\"details\":false"), "{body}");
        assert!(body.contains("\"id\":\"s1\""));
        let filter = seen.lock().unwrap().clone().unwrap();
        assert_eq!(filter.agent.as_deref(), Some("codex,dsh"));
        assert_eq!(filter.cwd.as_deref(), Some("D:/w,D:/x"), "cwd 优先，path 只是旧名");
        let _ = stop;
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn open_stream(port: u16, token: &str, extra: &str) -> std::net::TcpStream {
        let mut conn = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
        conn.set_read_timeout(Some(Duration::from_millis(400))).unwrap();
        let request = format!(
            "GET /v1/events{extra} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nAuthorization: Bearer {token}\r\nAccept: text/event-stream\r\nConnection: close\r\n\r\n"
        );
        conn.write_all(request.as_bytes()).unwrap();
        conn
    }

    fn drain(conn: &mut std::net::TcpStream, needle: &str, tries: usize) -> String {
        let mut seen = String::new();
        let mut buf = [0u8; 4096];
        for _ in 0..tries {
            match conn.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => {
                    seen.push_str(&String::from_utf8_lossy(&buf[..n]));
                    if seen.contains(needle) {
                        break;
                    }
                }
                Err(_) => continue,
            }
        }
        seen
    }

    #[test]
    fn a_real_sse_stream_resumes_and_resets() {
        let (api, dir) = api_in("sse");
        let token = api.token().unwrap();
        let (port, stop) = serve(api.clone(), live_snapshot(api.clone(), true));
        let mut live = open_stream(port, &token, "");
        let head = drain(&mut live, "\r\n\r\n", 20);
        assert!(head.starts_with("HTTP/1.1 200") && head.to_ascii_lowercase().contains("text/event-stream"), "{head}");
        let (epoch, _, _) = api.boundary();
        api.publish(&event("s1"));
        api.publish(&event("s2"));
        let frames = drain(&mut live, &format!("id: {epoch}:2"), 40);
        assert!(frames.contains(&format!("id: {epoch}:1")) && frames.contains(&format!("id: {epoch}:2")), "{frames}");
        drop(live);
        // 从第一条之后续读：只补第二条。
        let mut resumed = open_stream(port, &token, &format!("?since={epoch}:1"));
        let again = drain(&mut resumed, &format!("id: {epoch}:2"), 40);
        assert!(again.contains(&format!("id: {epoch}:2")), "{again}");
        assert!(!again.contains(&format!("id: {epoch}:1\n")), "no replay of the first");
        drop(resumed);
        // 失效 epoch：先来一帧 event: reset。
        let mut stale = open_stream(port, &token, "?since=1:1");
        let reset = drain(&mut stale, "event: reset", 40);
        assert!(reset.contains("event: reset"), "{reset}");
        // Last-Event-ID 与 since 同时给：参数错误。
        assert_eq!(authed_get(port, "/v1/events?since=1:1", &token, "Last-Event-ID: 1:1\r\n").0, 400);
        let _ = stop;
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_open_stream_does_not_block_other_requests_and_quit_releases_it() {
        let (api, dir) = api_in("release");
        let token = api.token().unwrap();
        let (port, stop) = serve(api.clone(), live_snapshot(api.clone(), true));
        let mut live = open_stream(port, &token, "");
        let head = drain(&mut live, "\r\n\r\n", 20);
        assert!(head.starts_with("HTTP/1.1 200"), "{head}");
        let began = Instant::now();
        assert_eq!(get(port, "/v1/info", "").0, 200);
        assert!(began.elapsed() < Duration::from_secs(2), "not blocked by the open stream");
        api.close();
        let rest = drain(&mut live, "\u{0}", 20);
        assert!(rest.len() <= 8, "the stream ended (only the chunk terminator may be left): {rest:?}");
        // quit 唤醒读端：槽位与连接不能被 10 s 写超时盲等占着。
        let mut waited = 0;
        while api.active_streams() > 0 && waited < 40 {
            std::thread::sleep(Duration::from_millis(50));
            waited += 1;
        }
        assert_eq!(api.active_streams(), 0, "quit must end the stream without waiting on the socket timeout");
        let _ = stop;
        let _ = std::fs::remove_dir_all(&dir);
    }

    // A4：活跃 worker 槽位不因队列回收提前释放；连接上限与退出都不泄漏。
    #[test]
    fn the_stream_slots_are_bounded_and_released() {
        let (api, dir) = api_in("slots");
        let token = api.token().unwrap();
        let (port, stop) = serve(api.clone(), live_snapshot(api.clone(), true));
        let mut held = Vec::new();
        for _ in 0..MAX_SUBSCRIBERS {
            let mut conn = open_stream(port, &token, "");
            let _ = drain(&mut conn, "\r\n\r\n", 20);
            held.push(conn);
        }
        assert_eq!(api.active_streams(), MAX_SUBSCRIBERS, "每条流占一个活跃写线程槽位");
        assert_eq!(authed_get(port, "/v1/events", &token, "").0, 503, "第 9 条被拒");
        // 客户端退出：write 失败，worker 结束，槽位归还。
        drop(held);
        let mut waited = 0;
        while api.active_streams() > 0 && waited < 100 {
            std::thread::sleep(Duration::from_millis(50));
            waited += 1;
        }
        assert_eq!(api.active_streams(), 0, "客户端退出后槽位必须归还");
        assert!(waited < 100, "槽位回收不能被永远占着");
        let _ = stop;
        let _ = std::fs::remove_dir_all(&dir);
    }

    // A4：真的 socket 写超时——对面不读时 respond 到点失败返回，worker 不会永远阻塞。
    #[test]
    fn a_stalled_write_times_out() {
        let server = tiny_http::Server::http(("127.0.0.1", 0)).unwrap();
        let port = server.server_addr().to_ip().unwrap().port();
        let (tell, heard) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let mut request = server.recv().unwrap();
            let _ = request.set_write_timeout(Some(Duration::from_millis(200)));
            let body = std::io::repeat(b'x').take(64 * 1024 * 1024);
            let began = Instant::now();
            let response = Response::new(StatusCode(200), vec![], body, None, None);
            let result = request.respond(response);
            let _ = tell.send((result.is_err(), began.elapsed()));
        });
        let mut conn = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
        conn.write_all(format!("GET /big HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\n\r\n").as_bytes()).unwrap();
        let (failed, elapsed) = heard.recv_timeout(Duration::from_secs(5)).expect("the response must end");
        assert!(failed, "a client that never reads must make the write fail");
        assert!(elapsed < Duration::from_secs(4), "write timeout 应尽快生效：{elapsed:?}");
    }
}