//! The Clash page's core: a mihomo (Clash.Meta) kernel of the dock's own, run
//! beside whatever proxy the user already has (v4 设计 3.1：借 Clash Verge 的
//! 控制器三条路都走不通，见 docs/specs/2026-10-23-clash-zen-tabs-design-v4.md).
//!
//! The kernel is found (3.2), given a generated config in the identity
//! directory (3.3), started hidden, and polled on its controller until it
//! answers (3.8). Its panel (zashboard) rides on the kernel's own
//! `external-ui`, so the frame in clash.js is same-origin and needs no CORS
//! (3.5). The ports are fixed-first with a random fallback, and the ones that
//! won are written to ports.json and /health for the machine's agents to find
//! (3.7、3.10). The system proxy switch is the dock's own: the kernel cannot
//! set it, and neither can zashboard (3.7.1).

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Mutex, MutexGuard};
use std::time::{Duration, Instant};

use serde::Serialize;
use serde_json::{json, Value};

/// 端口策略（3.7）：固定优先，避开 Clash Verge 的 7897/7898/9097 与 mihomo
/// 官方默认的 7890；被占用时回退到系统给的随机空闲端口。
const MIXED_PREFERRED: u16 = 7899;
const CTL_PREFERRED: u16 = 9099;
/// 就绪轮询（3.8）：200ms 一次，上限 20s。
const READY_TIMEOUT: Duration = Duration::from_secs(20);
const READY_POLL: Duration = Duration::from_millis(200);
/// 强杀前的宽限。
const STOP_TIMEOUT: Duration = Duration::from_secs(8);

/// 正在跑的内核，与它的控制器凭据（secret 只活在这里和 config.yaml 里）。
struct Running {
    child: Child,
    mixed: u16,
    ctl: u16,
    secret: String,
    version: String,
    /// 系统代理是这份内核接管中的：退出或停止时按快照还原。
    sysproxy: bool,
}

struct Clash {
    running: Option<Running>,
    error: Option<ClashFailure>,
}

static CLASH: Mutex<Clash> = Mutex::new(Clash { running: None, error: None });

/// 锁毒化了也照样拿：别处 panic 不是把 mihomo.exe 留在桌面上的理由。
fn held() -> MutexGuard<'static, Clash> {
    CLASH.lock().unwrap_or_else(|error| error.into_inner())
}

/// 页面上说得清的失败：一个码、一句人话、一段细节（内核的最后几行输出）。
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClashFailure {
    code: String,
    message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    detail: Option<String>,
}

impl ClashFailure {
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

/// 拉起成功后的答复：面板地址（同源 zashboard，带 secret 与禁升级参数）。
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClashReady {
    pub url: String,
    pub version: String,
    pub mixed: u16,
    pub controller: u16,
}

/// clash_state 的答复，含系统代理开关的现状。
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClashState {
    pub running: bool,
    pub version: Option<String>,
    pub mixed: Option<u16>,
    pub controller: Option<u16>,
    pub panel_url: Option<String>,
    pub sysproxy: bool,
    pub error: Option<ClashFailure>,
}

fn no_console(command: &mut Command) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // CREATE_NO_WINDOW：Go 内核的控制台不许在她脸上闪。
        command.creation_flags(0x0800_0000);
    }
    #[cfg(not(windows))]
    let _ = command;
}

/// 页面来源白名单：只有 dock 自己的页面能把面板嵌进 frame（照 oflm.rs:111）。
fn embed_origin(origin: &str) -> Result<(), ClashFailure> {
    let origin = origin.trim().trim_end_matches('/');
    let ours = origin == "http://tauri.localhost"
        || origin.starts_with("http://localhost:")
        || origin.starts_with("http://127.0.0.1:");
    if !ours {
        return Err(ClashFailure::new("ORIGIN_REJECTED", format!("不认识的页面来源：{origin}")));
    }
    Ok(())
}

/// 数据目录：身份目录下的 clash/（1.0.2 起身份目录是 `.fish-dock`）。
fn base_dir() -> Result<PathBuf, ClashFailure> {
    crate::identity::from_environment()
        .map(|location| location.root.join("clash"))
        .map_err(|error| ClashFailure::new("DATA_DIRECTORY_INVALID", "本机身份目录配置无效").detail(error))
}

// --- 端口策略（3.7） ----------------------------------------------------------------

/// 一个端口现在是否空闲：绑得上就是空闲（绑完即还）。
fn port_free(port: u16) -> bool {
    TcpListener::bind(("127.0.0.1", port)).is_ok()
}

/// 系统给的随机空闲端口。
fn random_free() -> Option<u16> {
    TcpListener::bind(("127.0.0.1", 0)).ok()?.local_addr().ok().map(|addr| addr.port())
}

/// 端口策略的本体：首选空闲就用首选；否则要随机端口，且不许与另一个端口相
/// 同。`is_free` 与 `random` 都是注入的，测试不用真占端口。
fn pick_port(preferred: u16, other: Option<u16>, is_free: impl Fn(u16) -> bool, random: &mut impl FnMut() -> u16) -> u16 {
    if Some(preferred) != other && is_free(preferred) {
        return preferred;
    }
    for _ in 0..64 {
        let candidate = random();
        if candidate != 0 && Some(candidate) != other && is_free(candidate) {
            return candidate;
        }
    }
    preferred
}

/// 一对端口：mixed 与 controller 各按策略取，互不相同。
fn pick_ports(is_free: impl Fn(u16) -> bool, random: &mut impl FnMut() -> u16) -> (u16, u16) {
    let mixed = pick_port(MIXED_PREFERRED, None, &is_free, random);
    let ctl = pick_port(CTL_PREFERRED, Some(mixed), &is_free, random);
    (mixed, ctl)
}

/// 生成的 config.yaml（3.3）：没有 tun 段、没有 socks-port（mixed 同端口已
/// 覆盖 HTTP 与 SOCKS）；secret 全文只此一处。订阅（有 URL 时）走
/// proxy-provider：节点由内核自己下载与定时更新，控制字段永远不会被
/// 「导入订阅」的配置替换顶掉（v4 §3.3 的补充：面板的导入是全量替换，
/// 实测会把控制器字段顶掉，所以订阅只从 dock 这条路进）。
fn config_text(mixed: u16, secret: &str, subscription: &str) -> String {
    let sub = subscription.trim();
    let providers = if sub.is_empty() {
        String::new()
    } else {
        format!(
            "proxy-providers:\n  \
             sub:\n    \
             type: http\n    \
             url: {sub}\n    \
             path: ./providers/sub.yaml\n    \
             interval: 86400\n    \
             health-check:\n      \
             enable: true\n      \
             url: https://www.gstatic.com/generate_204\n      \
             interval: 600\n"
        )
    };
    let groups = if sub.is_empty() {
        String::new()
    } else {
        "proxy-groups:
  - name: PROXY
    type: select
    use:
      - sub
rules:
  - MATCH,PROXY
".to_string()
    };
    format!(
        "mixed-port: {mixed}\n\
         mode: rule\n\
         log-level: warning\n\
         unified-delay: true\n\
         tcp-concurrent: true\n\
         profile:\n  \
         store-selected: true\n\
         dns:\n  \
         enable: true\n  \
         ipv6: false\n  \
         enhanced-mode: fake-ip\n  \
         fake-ip-range: 198.18.0.1/16\n  \
         nameserver: [223.5.5.5, 119.29.29.29]\n  \
         fallback: [1.1.1.1, 8.8.8.8]\n{providers}{groups}"
    )
}

/// 控制器口令：生成一次、存 `<data>/clash/secret`、之后一直用它。固定不变
/// 是刻意的——zashboard 会记住后端（连密钥一起），每次随机换口令面板就
/// 失忆一次（实测踩过：保存订阅重启内核后「密码不对」）。文件与 config.yaml
/// 同目录，同机同权限，不进设置存储也不出本机。
fn load_or_create_secret(dir: &Path) -> Result<String, ClashFailure> {
    let file = dir.join("secret");
    if let Ok(saved) = std::fs::read_to_string(&file) {
        let saved = saved.trim();
        if saved.len() == 32 && saved.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Ok(saved.to_string());
        }
    }
    let secret = random_secret();
    std::fs::write(&file, &secret)
        .map_err(|error| ClashFailure::new("START_FAILED", "密钥文件没能落盘").detail(error.to_string()))?;
    Ok(secret)
}

/// 首次生成时的随机源：时间、进程号与栈地址搅在一起（SplitMix64），
/// 不进设置存储，也不出本机。
fn random_secret() -> String {
    let anchor = 0u8;
    let mut state = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0)
        ^ ((std::process::id() as u64) << 32)
        ^ (&anchor as *const u8 as u64);
    let mut out = String::with_capacity(32);
    for _ in 0..4 {
        state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^= z >> 31;
        out.push_str(&format!("{z:016x}"));
    }
    out.truncate(32);
    out
}

// --- ports.json（3.7 的落盘契约） ----------------------------------------------------

/// 生效端口落盘：文件存在 = 内核在跑；不含 secret。字段固定五个；
/// `pid` 是**内核进程**的，不是 dock 的——消费方按它找/停内核。
fn write_ports(dir: &Path, mixed: u16, ctl: u16, kernel_pid: u32) -> std::io::Result<()> {
    let body = json!({ "mixed": mixed, "controller": ctl, "panel": "/ui/", "pid": kernel_pid, "startedAt": now_ms() });
    std::fs::write(dir.join("ports.json"), body.to_string())
}

/// 内核停止：契约解除，文件删除。
fn clear_ports(dir: &Path) {
    let _ = std::fs::remove_file(dir.join("ports.json"));
}

// --- 内核发现（3.2） ----------------------------------------------------------------

/// 候选里第一个存在的可执行文件；全缺失即 None。
fn discover(candidates: &[PathBuf]) -> Option<PathBuf> {
    candidates.iter().find(|candidate| candidate.is_file()).cloned()
}

/// 发现顺序（3.2）：托管副本 → 环境变量 → Clash Verge 安装目录 → PATH →
/// `%LOCALAPPDATA%\Programs\*`。注册表与 PATH 只在真跑时查，测试走 `discover`。
fn core_candidates(data: &Path) -> Vec<PathBuf> {
    let mut list = vec![data.join("core").join("mihomo.exe")];
    if let Some(set) = std::env::var_os("DSH_FISH_CLASH_CORE") {
        list.push(PathBuf::from(set));
    }
    list.extend(verge_install_locations());
    for name in ["mihomo.exe", "verge-mihomo.exe"] {
        if let Some(on_path) = find_on_path(name) {
            list.push(on_path);
        }
    }
    if let Some(local) = std::env::var_os("LOCALAPPDATA") {
        if let Ok(entries) = std::fs::read_dir(PathBuf::from(&local).join("Programs")) {
            for entry in entries.flatten() {
                for name in ["mihomo.exe", "verge-mihomo.exe"] {
                    let candidate = entry.path().join(name);
                    if candidate.is_file() {
                        list.push(candidate);
                    }
                }
            }
        }
    }
    list
}

fn verge_install_locations() -> Vec<PathBuf> {
    let mut out = Vec::new();
    #[cfg(windows)]
    {
        use winreg::enums::HKEY_LOCAL_MACHINE;
        use winreg::RegKey;
        for key in [
            r"SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\Clash Verge",
            r"SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall\Clash Verge",
        ] {
            if let Ok(opened) = RegKey::predef(HKEY_LOCAL_MACHINE).open_subkey(key) {
                if let Ok(location) = opened.get_value::<String, _>("InstallLocation") {
                    for name in ["verge-mihomo.exe", "mihomo.exe"] {
                        out.push(PathBuf::from(&location).join(name));
                    }
                }
            }
        }
    }
    // 安装器不写卸载项时（本机实测），按两处常见目录回退。
    for base in ["ProgramFiles", "ProgramFiles(x86)"] {
        if let Some(dir) = std::env::var_os(base) {
            let dir = PathBuf::from(dir);
            out.push(dir.join(r"Clash Verge\verge-mihomo.exe"));
            out.push(dir.join(r"OTHER APP\Clash Verge\verge-mihomo.exe"));
        }
    }
    out
}

fn find_on_path(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path).map(|directory| directory.join(name)).find(|candidate| candidate.is_file())
}

/// 内核版本：`-v` 的第一行；跑不动不阻塞，标注版本未知。
fn kernel_version(core: &Path) -> String {
    Command::new(core)
        .arg("-v")
        .output()
        .ok()
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .and_then(|text| text.lines().next().map(str::to_string))
        .unwrap_or_else(|| "版本未知".into())
}

// --- 控制器就绪与面板地址 ------------------------------------------------------------

/// 控制器就绪了吗：`GET /version` 带 Bearer，200 且报文里真有版本才算。
fn controller_ready(ctl: u16, secret: &str) -> bool {
    let Ok(mut stream) = TcpStream::connect(("127.0.0.1", ctl)) else { return false };
    let _ = stream.set_read_timeout(Some(Duration::from_millis(500)));
    let request = format!("GET /version HTTP/1.1\r\nHost: 127.0.0.1:{ctl}\r\nAuthorization: Bearer {secret}\r\nConnection: close\r\n\r\n");
    if stream.write_all(request.as_bytes()).is_err() {
        return false;
    }
    let mut response = String::new();
    let _ = stream.read_to_string(&mut response);
    response.starts_with("HTTP/1.1 200") && response.contains("version")
}

/// 面板地址：同源 zashboard，secret 走参数，禁用面板自己的内核升级。
/// hostname/port 也要给：zashboard 不自己推断后端，缺了就落回它默认的
/// 9090，连的就不是这个内核（实测踩过）。
fn panel_url(ctl: u16, secret: &str) -> String {
    format!("http://127.0.0.1:{ctl}/ui/?hostname=127.0.0.1&port={ctl}&secret={secret}&disableUpgradeCore=1")
}

fn now_ms() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

// --- 系统代理（3.7.1） ---------------------------------------------------------------

#[cfg(windows)]
const INET_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Internet Settings";

/// 系统代理三元组的快照：接管前的原样，还原就是把它写回去。
#[derive(Clone, Debug, PartialEq)]
struct SysSnapshot {
    enable: u32,
    server: String,
    bypass: String,
}

fn read_inet() -> SysSnapshot {
    #[cfg(windows)]
    {
        use winreg::enums::{HKEY_CURRENT_USER, KEY_READ};
        use winreg::RegKey;
        let Ok(key) = RegKey::predef(HKEY_CURRENT_USER).open_subkey_with_flags(INET_KEY, KEY_READ) else {
            return SysSnapshot { enable: 0, server: String::new(), bypass: String::new() };
        };
        SysSnapshot {
            enable: key.get_value("ProxyEnable").unwrap_or(0),
            server: key.get_value("ProxyServer").unwrap_or_default(),
            bypass: key.get_value("ProxyOverride").unwrap_or_default(),
        }
    }
    #[cfg(not(windows))]
    SysSnapshot { enable: 0, server: String::new(), bypass: String::new() }
}

fn write_inet(snapshot: &SysSnapshot) -> std::io::Result<()> {
    #[cfg(windows)]
    {
        use winreg::enums::{HKEY_CURRENT_USER, KEY_SET_VALUE};
        use winreg::RegKey;
        let key = RegKey::predef(HKEY_CURRENT_USER).open_subkey_with_flags(INET_KEY, KEY_SET_VALUE)?;
        key.set_value("ProxyEnable", &snapshot.enable)?;
        key.set_value("ProxyServer", &snapshot.server)?;
        key.set_value("ProxyOverride", &snapshot.bypass)?;
        notify_refresh();
    }
    #[cfg(not(windows))]
    let _ = (snapshot,);
    Ok(())
}

/// 写完注册表叫 WinINET 立刻重读，不然浏览器要等下一次自己重读。
#[cfg(windows)]
fn notify_refresh() {
    // INTERNET_OPTION_SETTINGS_CHANGED (39) 与 INTERNET_OPTION_REFRESH (37)。
    const SETTINGS_CHANGED: u32 = 39;
    const REFRESH: u32 = 37;
    unsafe {
        windows_sys::Win32::Networking::WinInet::InternetSetOptionW(std::ptr::null_mut(), SETTINGS_CHANGED, std::ptr::null(), 0);
        windows_sys::Win32::Networking::WinInet::InternetSetOptionW(std::ptr::null_mut(), REFRESH, std::ptr::null(), 0);
    }
}

/// 本机绕行条目：用户原有的全保留，缺的本机三样补上。
fn with_local_bypass(bypass: &str) -> String {
    let wanted = ["localhost", "127.*", "<local>"];
    let mut parts: Vec<String> = bypass.split(';').map(str::trim).filter(|p| !p.is_empty()).map(str::to_string).collect();
    for want in wanted {
        if !parts.iter().any(|p| p.eq_ignore_ascii_case(want)) {
            parts.push(want.to_string());
        }
    }
    parts.join(";")
}

fn snapshot_path(dir: &Path) -> PathBuf {
    dir.join("sysproxy.json")
}

fn write_snapshot(dir: &Path, snapshot: &SysSnapshot) -> std::io::Result<()> {
    std::fs::write(
        snapshot_path(dir),
        json!({ "enable": snapshot.enable, "server": snapshot.server, "bypass": snapshot.bypass }).to_string(),
    )
}

fn read_snapshot(dir: &Path) -> Option<SysSnapshot> {
    let text = std::fs::read_to_string(snapshot_path(dir)).ok()?;
    let value: Value = serde_json::from_str(&text).ok()?;
    Some(SysSnapshot {
        enable: value["enable"].as_u64().unwrap_or(0) as u32,
        server: value["server"].as_str().unwrap_or_default().to_string(),
        bypass: value["bypass"].as_str().unwrap_or_default().to_string(),
    })
}

/// 接管：首次快照原样，然后把系统代理指向本内核的 mixed 端口。
fn sysproxy_on(dir: &Path, mixed: u16) -> Result<(), ClashFailure> {
    if read_snapshot(dir).is_none() {
        let before = read_inet();
        write_snapshot(dir, &before)
            .map_err(|error| ClashFailure::new("SYSPROXY_FAILED", "系统代理快照没能落盘").detail(error.to_string()))?;
    }
    let mut target = read_inet();
    target.enable = 1;
    target.server = format!("127.0.0.1:{mixed}");
    target.bypass = with_local_bypass(&target.bypass);
    write_inet(&target).map_err(|error| ClashFailure::new("SYSPROXY_FAILED", "系统代理没能写入").detail(error.to_string()))
}

/// 还原：按快照写回；没有快照（从没接管过）就是无事可做。
fn sysproxy_off(dir: &Path) -> Result<(), ClashFailure> {
    let Some(snapshot) = read_snapshot(dir) else { return Ok(()) };
    write_inet(&snapshot).map_err(|error| ClashFailure::new("SYSPROXY_FAILED", "系统代理没能还原").detail(error.to_string()))?;
    let _ = std::fs::remove_file(snapshot_path(dir));
    Ok(())
}

/// 孤儿补还原（3.7.1）：上次异常退出留下了接管快照、内核却没在跑——先还原。
fn repair_orphan(dir: &Path) {
    if snapshot_path(dir).exists() && held().running.is_none() {
        let _ = sysproxy_off(dir);
    }
}

// --- 拉起与停止（3.8） ---------------------------------------------------------------

fn spawn_core(core: &Path, dir: &Path, ctl: u16, secret: &str) -> Result<(Child, impl Fn() -> String + use<>), ClashFailure> {
    let mut command = Command::new(core);
    command
        .current_dir(dir)
        .arg("-d")
        .arg(dir)
        .arg("-f")
        .arg(dir.join("config.yaml"))
        // 控制面三件用命令行钉死：面板里导入订阅会让内核整体替换运行配置，
        // 配置文件里的控制器/密钥/面板字段会被机场的顶掉；命令行 flag 优先级
        // 更高、重载也保持——面板与 dock 因此不失联。
        .arg("-ext-ctl")
        .arg(format!("127.0.0.1:{ctl}"))
        .arg("-secret")
        .arg(secret)
        .arg("-ext-ui")
        .arg(dir.join("ui"))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped());
    no_console(&mut command);
    let mut child = command
        .spawn()
        .map_err(|error| ClashFailure::new("SPAWN_FAILED", "没能启动 mihomo 内核").detail(format!("{}：{error}", core.display())))?;
    // 它的最后几行 stderr，是起不来的原因所在。
    let tail: std::sync::Arc<Mutex<std::collections::VecDeque<String>>> = std::sync::Arc::new(Mutex::new(std::collections::VecDeque::new()));
    if let Some(stderr) = child.stderr.take() {
        let tail = std::sync::Arc::clone(&tail);
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
    Ok((
        child,
        move || {
            let kept = tail.lock().unwrap_or_else(|error| error.into_inner());
            kept.iter().cloned().collect::<Vec<_>>().join("\n")
        },
    ))
}

/// 拉起一次：写配置、落 ports.json、起进程、轮询控制器就绪。失败把内核杀掉
/// 并带上它的最后几句话。
fn start(held: &mut Clash, data: &Path, subscription: &str) -> Result<ClashReady, ClashFailure> {
    let core = discover(&core_candidates(data))
        .ok_or_else(|| ClashFailure::new("KERNEL_MISSING", "未找到 mihomo 内核").detail("找过托管副本、DSH_FISH_CLASH_CORE、Clash Verge 安装目录、PATH 与 %LOCALAPPDATA%\\Programs，可在页面选择本地文件或下载"))?;
    let version = kernel_version(&core);
    let secret = load_or_create_secret(data)?;
    std::fs::create_dir_all(data).map_err(|error| ClashFailure::new("DATA_DIRECTORY_INVALID", "数据目录建不出来").detail(error.to_string()))?;
    let (mixed, ctl) = pick_ports(port_free, &mut || random_free().unwrap_or(0));
    std::fs::write(data.join("config.yaml"), config_text(mixed, &secret, subscription))
        .map_err(|error| ClashFailure::new("START_FAILED", "配置文件没能落盘").detail(error.to_string()))?;
    let (mut child, last_words) = spawn_core(&core, data, ctl, &secret)?;
    write_ports(data, mixed, ctl, child.id())
        .map_err(|error| ClashFailure::new("START_FAILED", "ports.json 没能落盘").detail(error.to_string()))?;
    let deadline = Instant::now() + READY_TIMEOUT;
    loop {
        if controller_ready(ctl, &secret) {
            held.running = Some(Running { child, mixed, ctl, secret, version: version.clone(), sysproxy: false });
            held.error = None;
            return Ok(ClashReady { url: panel_url(ctl, &held.running.as_ref().unwrap().secret), version, mixed, controller: ctl });
        }
        match child.try_wait() {
            Ok(Some(status)) => {
                held.running = None;
                clear_ports(data);
                return Err(ClashFailure::new("SERVICE_EXITED", format!("内核起来又退了（{status}）")).detail(last_words()));
            }
            Ok(None) => {}
            Err(error) => {
                let _ = child.kill();
                held.running = None;
                clear_ports(data);
                return Err(ClashFailure::new("SERVICE_EXITED", "内核进程不可问").detail(error.to_string()));
            }
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            held.running = None;
            clear_ports(data);
            return Err(ClashFailure::new("READY_TIMEOUT", "内核 20 秒内没有就绪").detail(last_words()));
        }
        std::thread::sleep(READY_POLL);
    }
}

fn stop_held(held: &mut Clash, data: Option<&Path>) {
    if let Some(mut run) = held.running.take() {
        // mihomo 没有优雅退出的接口（Clash Verge 也是直接结束进程）。
        let _ = run.child.kill();
        let _ = run.child.wait();
    }
    held.error = None;
    if let Some(data) = data {
        clear_ports(data);
        let _ = sysproxy_off(data);
    }
}

fn ready_of(run: &Running) -> ClashReady {
    ClashReady { url: panel_url(run.ctl, &run.secret), version: run.version.clone(), mixed: run.mixed, controller: run.ctl }
}

// --- 对外（命令、退出、健康） ---------------------------------------------------------

fn ensure(origin: &str, subscription: &str) -> Result<ClashReady, ClashFailure> {
    embed_origin(origin)?;
    let data = base_dir()?;
    {
        let mut held = held();
        if let Some(run) = held.running.as_mut() {
            if matches!(run.child.try_wait(), Ok(None)) {
                return Ok(ready_of(run));
            }
            held.running = None;
        }
        held.error = None;
    }
    // 上次异常退出留下的系统代理接管：先还原再起新内核。
    repair_orphan(&data);
    let mut held = held();
    match start(&mut held, &data, subscription) {
        Ok(ready) => Ok(ready),
        Err(failure) => {
            held.error = Some(failure.clone());
            Err(failure)
        }
    }
}

/// dock 退出与命令共用：内核杀掉、ports.json 删除、系统代理按快照还原。
pub fn shutdown() {
    let data = base_dir().ok();
    stop_held(&mut held(), data.as_deref());
}

/// 启动时的一次孤儿补还原（3.7.1：上次异常退出可能把系统代理留在死端口上）。
pub fn startup_repair() {
    if let Some(data) = base_dir().ok() {
        repair_orphan(&data);
    }
}

/// /health 的 clash 段（3.7）：本机 agent 的文件级契约之外的 HTTP 契约，权威。
pub fn health() -> Value {
    let held = held();
    match &held.running {
        Some(run) => json!({ "running": true, "mixed": run.mixed, "controller": run.ctl, "version": run.version }),
        None => json!({ "running": false }),
    }
}

// --- Tauri 命令 -----------------------------------------------------------------------

#[tauri::command]
pub fn clash_ensure(app: tauri::AppHandle, origin: String) -> Result<ClashReady, ClashFailure> {
    let subscription = crate::shared(&app).setting("clashSubscription").as_str().unwrap_or("").to_string();
    ensure(&origin, &subscription)
}

#[tauri::command]
pub fn clash_state() -> ClashState {
    let held = held();
    let sysproxy = read_snapshot(&base_dir().unwrap_or_default()).is_some();
    match &held.running {
        Some(run) => ClashState {
            running: true,
            version: Some(run.version.clone()),
            mixed: Some(run.mixed),
            controller: Some(run.ctl),
            panel_url: Some(panel_url(run.ctl, &run.secret)),
            sysproxy,
            error: None,
        },
        None => ClashState {
            running: false,
            version: None,
            mixed: None,
            controller: None,
            panel_url: None,
            sysproxy,
            error: held.error.clone(),
        },
    }
}

#[tauri::command]
pub fn clash_stop(_app: tauri::AppHandle) {
    let data = base_dir().ok();
    stop_held(&mut held(), data.as_deref());
}

#[tauri::command]
pub fn clash_sysproxy(_app: tauri::AppHandle, enable: bool) -> Result<Value, ClashFailure> {
    let data = base_dir()?;
    let mut held = held();
    let mixed = held.running.as_ref().map(|run| run.mixed).ok_or_else(|| {
        ClashFailure::new("NOT_RUNNING", "内核没有在跑：先启动内核，系统代理才有可指向的端口")
    })?;
    if enable {
        sysproxy_on(&data, mixed)?;
        if let Some(run) = held.running.as_mut() {
            run.sysproxy = true;
        }
    } else {
        sysproxy_off(&data)?;
        if let Some(run) = held.running.as_mut() {
            run.sysproxy = false;
        }
    }
    Ok(json!({ "on": enable, "mixed": mixed }))
}

// --- 测试：纯函数与文件契约，不碰真内核与真注册表 ---------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn port_policy_prefers_fixed_when_free() {
        let (mixed, ctl) = pick_ports(|port| port == 7899 || port == 9099, &mut || 41000);
        assert_eq!((mixed, ctl), (7899, 9099));
    }

    #[test]
    fn port_policy_falls_back_to_random_and_keeps_them_apart() {
        let mut seq = [41000u16, 41001, 41002].iter().cycle();
        let (mixed, ctl) = pick_ports(|port| port >= 41000, &mut || *seq.next().unwrap());
        assert_eq!((mixed, ctl), (41000, 41001));
    }

    #[test]
    fn port_policy_never_picks_the_other_port() {
        // 首选都被占，随机只会给出与对方相同的值：兜底回首选，也不许撞上。
        let (mixed, ctl) = pick_ports(|_| false, &mut || 41000);
        assert_ne!(mixed, ctl);
    }

    #[test]
    fn config_has_the_contract_and_the_secret_once() {
        let text = config_text(7899, "abc123xyz", "");
        assert_eq!(text.matches("abc123xyz").count(), 0, "secret 只走命令行，不进配置文件");
        assert!(text.contains("mixed-port: 7899"));
        assert!(!text.contains("external-controller"), "控制器/密钥/面板改由命令行钉死，配置文件不再写");
        assert!(!text.contains("tun:"), "默认不开 TUN");
        assert!(!text.contains("socks-port"), "v4：mixed 已覆盖，不单开 socks");
    }

    #[test]
    fn config_with_subscription_uses_a_provider_not_plain_proxies() {
        let text = config_text(7899, "s1", "https://example.com/sub?token=x");
        assert!(text.contains("proxy-providers:"), "订阅走 provider，内核自己下载与更新");
        assert!(text.contains("url: https://example.com/sub?token=x"));
        assert!(text.contains("MATCH,PROXY"));
        let none = config_text(7899, "s1", "  ");
        assert!(!none.contains("proxy-providers"), "没有订阅就没有 provider 段");
    }

    #[test]
    fn secret_is_created_once_and_reused() {
        let dir = std::env::temp_dir().join(format!("wakuwaku-clash-sec-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let first = load_or_create_secret(&dir).unwrap();
        assert_eq!(first.len(), 32);
        assert_eq!(load_or_create_secret(&dir).unwrap(), first, "第二次读取应同一个，面板的记忆才有效");
        // 坏文件按无物处理：重新生成。
        std::fs::write(dir.join("secret"), "bad").unwrap();
        let again = load_or_create_secret(&dir).unwrap();
        assert_eq!(again.len(), 32);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn secret_is_hex_32_and_unique() {
        let a = random_secret();
        let b = random_secret();
        assert_eq!(a.len(), 32);
        assert!(a.bytes().all(|byte| byte.is_ascii_hexdigit()));
        assert_ne!(a, b);
    }

    #[test]
    fn ports_json_written_and_cleared() {
        let dir = std::env::temp_dir().join(format!("wakuwaku-clash-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        write_ports(&dir, 7899, 9099, 4242).unwrap();
        let saved: Value = serde_json::from_str(&std::fs::read_to_string(dir.join("ports.json")).unwrap()).unwrap();
        assert_eq!(saved["mixed"], 7899);
        assert_eq!(saved["controller"], 9099);
        assert_eq!(saved["panel"], "/ui/");
        assert_eq!(saved["pid"], 4242, "pid 是内核进程的");
        assert!(saved.get("secret").is_none(), "契约里没有 secret");
        clear_ports(&dir);
        assert!(!dir.join("ports.json").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn discovery_takes_the_first_that_exists() {
        let dir = std::env::temp_dir().join(format!("wakuwaku-clash-find-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("b")).unwrap();
        let real = dir.join("b").join("mihomo.exe");
        std::fs::write(&real, b"kernel").unwrap();
        let candidates = vec![dir.join("a").join("mihomo.exe"), real.clone(), dir.join("b").join("verge-mihomo.exe")];
        assert_eq!(discover(&candidates), Some(real));
        assert_eq!(discover(&[dir.join("none").join("mihomo.exe")]), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn sysproxy_snapshot_round_trips() {
        let dir = std::env::temp_dir().join(format!("wakuwaku-clash-sys-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let snapshot = SysSnapshot { enable: 1, server: "127.0.0.1:7897".into(), bypass: "localhost;<local>".into() };
        assert!(read_snapshot(&dir).is_none(), "没接管过就没有快照");
        write_snapshot(&dir, &snapshot).unwrap();
        assert_eq!(read_snapshot(&dir).unwrap(), snapshot);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn local_bypass_keeps_the_owners_entries() {
        assert_eq!(with_local_bypass("*.lan; 192.168.*"), "*.lan;192.168.*;localhost;127.*;<local>");
        assert_eq!(with_local_bypass("localhost;127.*;<local>"), "localhost;127.*;<local>");
        assert_eq!(with_local_bypass(""), "localhost;127.*;<local>");
    }

    #[test]
    fn health_says_running_false_before_anything() {
        let before = health();
        if !before["running"].as_bool().unwrap_or(false) {
            assert_eq!(before, json!({ "running": false }));
        }
    }
}
