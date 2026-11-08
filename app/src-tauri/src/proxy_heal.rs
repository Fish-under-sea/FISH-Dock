//! 系统代理被留在「指向一个已经不在的内核」的状态：从插件外面修回来。
//!
//! 三种触发，都在这里：
//!   · `--proxy-heal`（[`FLAG`]）：宿主 exe 自己的一种启动模式，跑一次就退出，
//!     由任务计划程序在「登录时 + 每 5 分钟」拉起（[`ensure_task`]）。它的路径
//!     稳定（安装目录），不依赖插件装没装、升没升，卸载时随 exe 一起消失。
//!   · 宿主在 `child.kill()` 之前兜底（plugin_process.rs）：插件 `deactivate`
//!     超时被强杀时，注册表里那份代理还指着刚死的端口。
//!   · 手工：`FISH-Dock.exe --proxy-heal`。
//!
//! 判定顺序是**结论的一部分**，换顺序就会在「内核还在跑」时把代理关掉：
//!   1. 没有租约 → 什么都不做（这是「没启动内核就不要开」的全部保证）；
//!   2. 租约坏了（schema 不是 1、缺 `owned`/`previous`）→ 删租约，不动注册表；
//!   3. 注册表三键与 `owned` 不是逐字相同 → 别人改过，删租约，不动注册表；
//!   4. `owned.ProxyServer` 的端口还有人监听 → 内核还在，什么都不做；
//!   5. 端口没人听 → 写回 `previous` 三键，删租约；
//!   6. 写失败 → 保留租约（下次再试），把被拒的键名与退出码写进
//!      `host/proxy-heal.log`（64 KB 封顶）。
//!
//! 租约的格式与插件侧（`plugins/clash/runtime.mjs` 的 `createProxyLease`）同一份，
//! 所以这里只读它、绝不发明字段。
use serde_json::Value;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

/// 宿主以「修一次就退出」跑起来的开关。
pub const FLAG: &str = "--proxy-heal";

/// 注册表里系统代理的三个键（与插件同一份口径）。
const KEY_PATH: &str = r"Software\Microsoft\Windows\CurrentVersion\Internet Settings";

/// 一次修的结论。`Busy` 只表示「另一个修的人正在动手」，不是结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    NoLease,
    Busy,
    BadLease,
    NotOwned,
    LivePort,
    Restored,
    WriteFailed,
}

impl Outcome {
    fn text(self) -> &'static str {
        match self {
            Outcome::NoLease => "no lease",
            Outcome::Busy => "busy",
            Outcome::BadLease => "bad lease",
            Outcome::NotOwned => "not ours",
            Outcome::LivePort => "kernel still listening",
            Outcome::Restored => "restored",
            Outcome::WriteFailed => "write failed",
        }
    }
}

/// 注册表三键的一份快照。`ProxyServer`/`ProxyOverride` 缺省（或租约里是 null）都记
/// 成 `None`，这样「不见了」与「空串」不会被当成一回事。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Record {
    pub enable: u32,
    pub server: Option<String>,
    pub bypass: Option<String>,
}

/// 读注册表、写注册表、探端口：三个动作都可替换，判定逻辑因此能单测。
pub trait Env {
    fn read_proxy(&self) -> Result<Record, String>;
    fn write_proxy(&self, record: &Record) -> Result<(), String>;
    fn listening(&self, port: u16) -> bool;
}

// 租约里的一个对象（`owned` / `previous`）→ 三键快照。字段类型不对就是坏记录，
// 让调用方按 `BadLease` 处理，不猜。
fn record_of(value: &Value) -> Option<Record> {
    let object = value.as_object()?;
    let enable = match object.get("ProxyEnable") {
        None | Some(Value::Null) => 0,
        Some(value) => u32::try_from(value.as_u64()?).ok()?,
    };
    let text = |name: &str| match object.get(name) {
        None | Some(Value::Null) => Some(None),
        Some(value) => Some(Some(value.as_str()?.to_string())),
    };
    Some(Record { enable, server: text("ProxyServer")?, bypass: text("ProxyOverride")? })
}

// `127.0.0.1:7895`（插件写的那一种）与带 scheme / 结尾斜杠的写法都认；不是本机
// 回环就返回 None——那不是我们的端口，不能拿它判断内核死活。
fn port_of(record: &Record) -> Option<u16> {
    let server = record.server.as_deref()?;
    let host_port = server.rsplit("://").next().unwrap_or(server).trim_end_matches('/');
    let (host, port) = host_port.rsplit_once(':')?;
    let host = host.trim_start_matches('[').trim_end_matches(']');
    if !matches!(host, "127.0.0.1" | "localhost" | "::1") {
        return None;
    }
    port.parse::<u16>().ok().filter(|port| *port >= 1024)
}

fn discard(lease: &Path) {
    let _ = std::fs::remove_file(lease);
}

// 刚写下的租约在 `window` 内算「正在写」：插件先落租约、再写注册表，中间隔着一次
// pwsh。窗口内不删租约，也不动注册表。
const FRESH_LEASE: Duration = Duration::from_secs(60);

// 一次修，不碰全局状态：判定全在这里，`heal_one` 只是给了它真实的 Env 与日志。
// `window` 是「刚写下的租约」的宽限；0 表示不给宽限（单测用）。
pub fn heal_one_with_window(env: &dyn Env, lease: &Path, window: Duration) -> Outcome {
    let mut detail = String::new();
    heal_one_detailed(env, lease, window, &mut detail)
}

// 带「为什么」的那一层：`detail` 只用来写日志——没有它，一次写失败在日志里就只是
// 一句 write failed，用户与我们都不知道是被拒的键名还是权限（#44 的教训）。
fn heal_one_detailed(env: &dyn Env, lease: &Path, window: Duration, detail: &mut String) -> Outcome {
    let raw = match std::fs::read_to_string(lease) {
        Ok(text) => text,
        // 没有租约（或读不到）就是没有接管：这是「没启动内核就什么都不做」。
        Err(_) => return Outcome::NoLease,
    };
    if raw.trim().is_empty() {
        return Outcome::NoLease;
    }
    let value: Value = match serde_json::from_str(&raw) {
        Ok(value) => value,
        Err(_) => {
            discard(lease);
            return Outcome::BadLease;
        }
    };
    if value.get("schema").and_then(Value::as_u64) != Some(1) {
        discard(lease);
        return Outcome::BadLease;
    }
    let (Some(owned), Some(previous)) = (
        value.get("owned").and_then(record_of),
        value.get("previous").and_then(record_of),
    ) else {
        discard(lease);
        return Outcome::BadLease;
    };
    let current = match env.read_proxy() {
        Ok(record) => record,
        // 读不到注册表就无从判断：租约留着，下次再试。
        Err(why) => {
            *detail = why;
            return Outcome::WriteFailed;
        }
    };
    // 系统代理已经被关掉、地址也空了：什么都不用做，把租约当过期记录清掉。
    // 但刚写下的租约可能是「正在接管」的中间态（插件先落租约、再写注册表），把它
    // 删掉就等于删掉正在进行的接管，之后真崩了就再也没有记录可读——宽限期内不动。
    if current.enable == 0 && current.server.as_deref().map(str::is_empty).unwrap_or(true) {
        if fresh(lease, window) {
            return Outcome::Busy;
        }
        discard(lease);
        return Outcome::NotOwned;
    }
    // 注册表里不是我们写的那一份：别的程序（TUN、别的代理、用户自己）改过，别动。
    // 但有一瞬间是**我们自己还没写完**：插件先落租约、再写注册表，中间隔着一次
    // pwsh（0.3–0.5 s）。这时删租约就等于「删掉正在进行的接管」，之后它再崩就再也
    // 没有记录可读。所以刚写下的租约一律不动，留到下一次（或下一个进程）判断。
    if current != owned {
        if fresh(lease, window) {
            return Outcome::Busy;
        }
        discard(lease);
        return Outcome::NotOwned;
    }
    let Some(port) = port_of(&owned) else {
        discard(lease);
        return Outcome::NotOwned;
    };
    // 内核还在跑：代理指着它是**对的**，这里什么都不做（每 5 分钟一次的常态）。
    if env.listening(port) {
        return Outcome::LivePort;
    }
    match env.write_proxy(&previous) {
        Ok(()) => {
            discard(lease);
            Outcome::Restored
        }
        // 写失败就保留租约：删了就再也认不出注册表里那份是谁写的，下次也没得重试。
        Err(why) => {
            *detail = why;
            Outcome::WriteFailed
        }
    }
}

// 同一个租约上的两个修的人（宿主 kill 前兜底、计划任务）不打架：先到先得，
// 另一个直接走开——两边写的都是同一份 `previous`，重复写没有意义。
struct Guard(PathBuf);

impl Guard {
    fn take(lease: &Path) -> Option<Guard> {
        let lock = lease.with_extension("lock");
        for attempt in 0..2 {
            match std::fs::OpenOptions::new().write(true).create_new(true).open(&lock) {
                Ok(_) => return Some(Guard(lock)),
                Err(_) if attempt == 0 => {
                    if stale(&lock) {
                        let _ = std::fs::remove_file(&lock);
                        continue;
                    }
                    return None;
                }
                Err(_) => return None,
            }
        }
        None
    }
}

impl Drop for Guard {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

// 超过一分钟的锁当作崩在半路留下的：正常一次修是毫秒级。
fn stale(lock: &Path) -> bool {
    age(lock).is_some_and(|age| age > Duration::from_secs(60))
}

// 文件有多久没动过（读不到就是 None）。
fn age(path: &Path) -> Option<Duration> {
    let modified = std::fs::metadata(path).and_then(|meta| meta.modified()).ok()?;
    SystemTime::now().duration_since(modified).ok()
}

fn fresh(lease: &Path, window: Duration) -> bool {
    !window.is_zero() && age(lease).is_some_and(|age| age < window)
}

// 数据根：租约在 `<根>/<命名空间>/config/system-proxy.json`，日志跟宿主同层
// （`<根>/host/proxy-heal.log`）。
fn data_root_of(lease: &Path) -> Option<PathBuf> {
    lease.parent()?.parent()?.parent().map(Path::to_path_buf)
}

// 日志只记结论，不记代理地址与端口（那不是秘密，但也没有用处）。64 KB 封顶：
// 每 5 分钟一次，一年也写不到，真写满了说明一直在失败，保留最近的更值钱。
fn log_line(lease: &Path, outcome: Outcome, detail: &str) {
    let Some(root) = data_root_of(lease) else { return };
    let folder = root.join("host");
    let file = folder.join("proxy-heal.log");
    if std::fs::metadata(&file).map(|meta| meta.len() > 64 * 1024).unwrap_or(false) {
        let _ = std::fs::write(&file, "");
    }
    let stamp = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).map(|elapsed| elapsed.as_millis()).unwrap_or(0);
    if std::fs::create_dir_all(&folder).is_err() {
        return;
    }
    if let Ok(mut handle) = std::fs::OpenOptions::new().create(true).append(true).open(&file) {
        let _ = writeln!(handle, "{stamp} {} {}{}{}", outcome.text(), lease.display(), if detail.is_empty() { "" } else { " · " }, detail);
    }
}

/// 修一个租约（真实注册表 + 真实端口探测），并记一行日志。
pub fn heal_one(lease: &Path) -> Outcome {
    let Some(_guard) = Guard::take(lease) else { return Outcome::Busy };
    let mut detail = String::new();
    let outcome = heal_one_detailed(&system(), lease, FRESH_LEASE, &mut detail);
    log_line(lease, outcome, &detail);
    outcome
}

/// `--proxy-heal` 的入口：扫本机所有插件命名空间，逐个修，跑完就返回退出码
/// （0 = 没有动作或成功；1 = 至少一个写失败，任务计划的「上次结果」能看到）。
/// 指定数据根跑一次（`--proxy-heal <根>`）：诊断时不必去改用户目录，也让
/// 「计划任务里跑一遍」这种真实验证不必动真格的插件数据。根必须是绝对路径。
pub fn run_at(root: Option<&Path>) -> i32 {
    let mut code = 0;
    for lease in leases(root) {
        if heal_one(&lease) == Outcome::WriteFailed {
            code = 1;
        }
    }
    code
}

// `%USERPROFILE%\.fish-dock\.<命名空间>\config\system-proxy.json`：只扫「点开头」
// 的目录（插件命名空间的写法），宿主自己的 `host/` 与无关目录不看。
fn leases(explicit: Option<&Path>) -> Vec<PathBuf> {
    let root = match explicit {
        Some(root) if root.is_absolute() => root.to_path_buf(),
        Some(_) => return Vec::new(),
        None => {
            let Ok(home) = crate::storage::profile_directory() else { return Vec::new() };
            let Ok(root) = crate::storage::resolve_root(&home, None, crate::config::unified_root_env().as_deref()) else { return Vec::new() };
            root
        }
    };
    let Ok(entries) = std::fs::read_dir(&root) else { return Vec::new() };
    let mut found: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .filter(|entry| entry.file_name().to_string_lossy().starts_with('.'))
        .map(|entry| entry.path().join("config").join("system-proxy.json"))
        .filter(|lease| lease.is_file())
        .collect();
    found.sort();
    found
}

// ---- 任务计划：登录时 + 每 5 分钟跑一次 `--proxy-heal` ----

/// 这个用户的任务名（任务计划的根文件夹是全机的，所以名字带 SID）。
pub fn task_name() -> Option<String> {
    crate::logon::user_sid().map(|sid| format!("FISH Dock proxy heal-{sid}"))
}

fn escape(text: &str) -> String {
    text.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

/// 自愈任务的 XML：登录时一次（崩溃后重登能修），外加每 5 分钟一次（不重登也修）。
pub fn task_xml(user: &str, exe: &str, args: &str) -> String {
    let (user, exe, args) = (escape(user), escape(exe), escape(args));
    format!(
        r#"<?xml version="1.0" encoding="UTF-16"?>
<Task version="1.2" xmlns="http://schemas.microsoft.com/windows/2004/02/mit/task">
  <RegistrationInfo>
    <Author>FISH Dock</Author>
    <Description>Puts the system proxy back when the kernel it pointed at is gone: once as you log on, and every five minutes. 内核不在时把系统代理恢复回去（登录时一次 + 每 5 分钟一次）。</Description>
  </RegistrationInfo>
  <Triggers>
    <LogonTrigger>
      <Enabled>true</Enabled>
      <UserId>{user}</UserId>
    </LogonTrigger>
    <TimeTrigger>
      <Enabled>true</Enabled>
      <StartBoundary>2026-01-01T00:00:00</StartBoundary>
      <Repetition>
        <Interval>PT5M</Interval>
        <StopAtDurationEnd>false</StopAtDurationEnd>
      </Repetition>
    </TimeTrigger>
  </Triggers>
  <Principals>
    <Principal id="Author">
      <UserId>{user}</UserId>
      <LogonType>InteractiveToken</LogonType>
      <RunLevel>LeastPrivilege</RunLevel>
    </Principal>
  </Principals>
  <Settings>
    <MultipleInstancesPolicy>IgnoreNew</MultipleInstancesPolicy>
    <DisallowStartIfOnBatteries>false</DisallowStartIfOnBatteries>
    <StopIfGoingOnBatteries>false</StopIfGoingOnBatteries>
    <ExecutionTimeLimit>PT2M</ExecutionTimeLimit>
    <Priority>7</Priority>
    <Enabled>true</Enabled>
  </Settings>
  <Actions Context="Author">
    <Exec>
      <Command>{exe}</Command>
      <Arguments>{args}</Arguments>
    </Exec>
  </Actions>
</Task>
"#
    )
}

// 装了「会接管系统代理」的插件吗？看得是插件注册表里的能力声明（`system-proxy`），
// 不是「有没有租约」——没开过代理的 clash 用户也要保留这个任务，否则他第一次
// 开代理就正好落在「没有任务」的空档里。
//
// 三态：Some(true) 有、Some(false) 明确没有、None 读不出来。读不出来时按「有」处理：
// 删错了要等下次启动才补回来，保留只是多跑一个 0.1 秒的空转。
fn proxy_plugin_state(host: &Path) -> Option<bool> {
    let file = host.join("config").join("installed-plugins.json");
    let bytes = std::fs::read(file).ok()?;
    let value: Value = serde_json::from_slice(&bytes).ok()?;
    let entries = value.get("entries")?.as_object()?;
    Some(entries.values().any(|row| {
        row.pointer("/manifest/capabilities")
            .and_then(Value::as_array)
            .is_some_and(|caps| caps.iter().any(|cap| cap.as_str() == Some("system-proxy")))
    }))
}

/// 让自愈任务与「有没有代理插件」对上：有就注册（已存在且指向本程序就不动），
/// 明确没有就删掉。宿主启动时调一次，卸载时调 [`remove_task`]。
pub fn ensure_task() -> Result<(), String> {
    // 源码树里跑（cargo run / 测试）不碰这台机器的任务表：那是开发路径，不是安装路径。
    if crate::data::from_source() {
        return Ok(());
    }
    let Some(name) = task_name() else { return Ok(()) };
    if proxy_plugin_state(&crate::data::folder()) == Some(false) {
        return crate::logon::remove(&name).map(|_| ());
    }
    let exe = crate::data::exe().map_err(|error| format!("本程序路径不可用：{error}"))?;
    let exe = exe.to_string_lossy().to_string();
    if let Some(seen) = crate::logon::xml(&name)?.map(|xml| crate::logon::seen(&xml)) {
        let same_program = seen.program.eq_ignore_ascii_case(&exe);
        let has_flag = seen.args.split_whitespace().any(|arg| arg == FLAG);
        if seen.enabled && same_program && has_flag {
            return Ok(());
        }
    }
    let sid = crate::logon::user_sid().ok_or("无法确认当前 Windows 用户 SID")?;
    crate::logon::register(&name, &task_xml(&sid, &exe, FLAG))
}

/// 卸载时把这个用户的自愈任务拿走（不属于本程序的同名任务不动）。
pub fn remove_task() -> Result<bool, String> {
    let Some(name) = task_name() else { return Ok(false) };
    // 认不出自己的路径就不动手：删掉别人的同名任务比留着它坏得多。
    let exe = crate::data::exe().map(|exe| exe.to_string_lossy().to_string()).map_err(|error| format!("本程序路径不可用：{error}"))?;
    if exe.is_empty() {
        return Err("本程序路径为空，保留同名任务".into());
    }
    match crate::logon::xml(&name)? {
        None => Ok(false),
        Some(xml) => {
            let seen = crate::logon::seen(&xml);
            if !seen.program.eq_ignore_ascii_case(&exe) {
                return Err(format!("同名任务不是本程序的，保留：{name}"));
            }
            crate::logon::remove(&name)
        }
    }
}

// ---- 真实环境 ----

#[cfg(windows)]
fn system() -> impl Env {
    System
}

#[cfg(not(windows))]
fn system() -> impl Env {
    Unsupported
}

#[cfg(windows)]
struct System;

#[cfg(windows)]
impl Env for System {
    fn read_proxy(&self) -> Result<Record, String> {
        use winreg::enums::{HKEY_CURRENT_USER, KEY_QUERY_VALUE};
        // 只问值：KEY_READ 还带着 READ_CONTROL，受限令牌下会被拒（实测 os error 5）。
        let key = winreg::RegKey::predef(HKEY_CURRENT_USER)
            .open_subkey_with_flags(KEY_PATH, KEY_QUERY_VALUE)
            .map_err(|error| format!("读取系统代理设置失败：{error}"))?;
        let enable = key.get_value::<u32, _>("ProxyEnable").unwrap_or(0);
        let server = key.get_value::<String, _>("ProxyServer").ok();
        let bypass = key.get_value::<String, _>("ProxyOverride").ok();
        Ok(Record { enable, server, bypass })
    }

    fn write_proxy(&self, record: &Record) -> Result<(), String> {
        match write_proxy_native(record) {
            Ok(()) => Ok(()),
            // 原生路子被拒（受限令牌、被安全软件挂上钩子、键的 ACL 只放行某些写入者）
            // 时退回插件走过的那条路：Windows PowerShell 写同一个键。它要是也不行，
            // 那插件本来也接管不了；两条都失败才把原因交回去。
            Err(native) => write_proxy_shell(record).map_err(|shell| format!("{native}；改用 PowerShell 也不行：{shell}")),
        }
    }
    fn listening(&self, port: u16) -> bool {
        use std::net::{SocketAddr, TcpStream};
        let address = SocketAddr::from(([127, 0, 0, 1], port));
        TcpStream::connect_timeout(&address, crate::config::PROXY_HEAL_CONNECT_TIMEOUT).is_ok()
    }
}

// 原生写：直接开键改三个值。最快，也是首选。
#[cfg(windows)]
fn write_proxy_native(record: &Record) -> Result<(), String> {
    use winreg::enums::{HKEY_CURRENT_USER, KEY_QUERY_VALUE, KEY_SET_VALUE};
    // 最小权限：只要这几个位。KEY_READ 还带着 READ_CONTROL，受限令牌下会被拒。
    let key = winreg::RegKey::predef(HKEY_CURRENT_USER)
        .open_subkey_with_flags(KEY_PATH, KEY_QUERY_VALUE | KEY_SET_VALUE)
        .map_err(|error| format!("打开系统代理设置失败：{error}"))?;
    if let Err(error) = key.set_value("ProxyEnable", &record.enable) {
        return Err(format!("写入 ProxyEnable 失败（Win32 {}）", error.raw_os_error().unwrap_or(0)));
    }
    for (name, value) in [("ProxyServer", &record.server), ("ProxyOverride", &record.bypass)] {
        let written = match value {
            Some(value) => key.set_value(name, value),
            // 原来没有这一项：删掉，而不是写空串（空串对 ProxyServer 是「没有代理」，
            // 但 ProxyOverride 的空串与缺省并不等价）。
            None => match key.delete_value(name) {
                Ok(()) => Ok(()),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
                Err(error) => Err(error),
            },
        };
        if let Err(error) = written {
            return Err(format!("写入 {name} 失败（Win32 {}）", error.raw_os_error().unwrap_or(0)));
        }
    }
    refresh();
    Ok(())
}

// 兜底写：Windows PowerShell 5.1 写同一个键（插件也是这么写的）。值走环境变量里的
// base64 JSON，脚本本身是固定的——配置值不会被拼进代码。
#[cfg(windows)]
fn write_proxy_shell(record: &Record) -> Result<(), String> {
    use base64::Engine as _;
    use std::os::windows::process::CommandExt as _;
    const SCRIPT: &str = r#"$ErrorActionPreference='Stop'
$r = ConvertFrom-Json ([Text.Encoding]::UTF8.GetString([Convert]::FromBase64String($env:FISH_DOCK_HEAL_VALUES)))
$p = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Internet Settings'
Set-ItemProperty -LiteralPath $p -Name ProxyEnable -Value ([int]$r.enable) -Type DWord
foreach ($k in @('ProxyServer','ProxyOverride')) {
  $v = $r.$k
  if ($null -eq $v) { Remove-ItemProperty -LiteralPath $p -Name $k -ErrorAction SilentlyContinue }
  else { Set-ItemProperty -LiteralPath $p -Name $k -Value ([string]$v) -Type String }
}
Add-Type -TypeDefinition 'using System;using System.Runtime.InteropServices;public class FishDockHeal{[DllImport("wininet.dll")]public static extern bool InternetSetOption(IntPtr h,int o,IntPtr b,int l);}'
[void][FishDockHeal]::InternetSetOption([IntPtr]::Zero,39,[IntPtr]::Zero,0)
[void][FishDockHeal]::InternetSetOption([IntPtr]::Zero,37,[IntPtr]::Zero,0)
Write-Output 'ok'"#;
    let values = serde_json::json!({
        "enable": record.enable,
        "server": record.server,
        "bypass": record.bypass,
    });
    let encoded = base64::engine::general_purpose::STANDARD.encode(values.to_string());
    let script: Vec<u8> = SCRIPT.encode_utf16().flat_map(u16::to_le_bytes).collect();
    let script = base64::engine::general_purpose::STANDARD.encode(script);
    let system = std::env::var("SystemRoot").unwrap_or_else(|_| "C:\\Windows".into());
    let shell = std::path::Path::new(&system).join("System32").join("WindowsPowerShell").join("v1.0").join("powershell.exe");
    let output = std::process::Command::new(shell)
        // -OutputFormat Text：不给它机会把错误包成 CLIXML，日志里要能一眼看懂。
        .args(["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-OutputFormat", "Text", "-EncodedCommand", &script])
        .env("FISH_DOCK_HEAL_VALUES", encoded)
        .creation_flags(0x0800_0000) // CREATE_NO_WINDOW：这是后台修，不弹黑框
        .output()
        .map_err(|error| format!("起不来 PowerShell：{error}"))?;
    if output.status.success() {
        Ok(())
    } else {
        let said: String = String::from_utf8_lossy(&output.stderr)
            .lines()
            .map(str::trim)
            .find(|line| !line.is_empty())
            .unwrap_or("")
            .chars()
            .take(200)
            .collect();
        Err(format!("PowerShell 退出码 {:?}：{said}", output.status.code()))
    }
}

// WinINet 自己的缓存：改完注册表不通知它，已经跑着的程序还按老设置走。
#[cfg(windows)]
fn refresh() {
    #[link(name = "wininet")]
    unsafe extern "system" {
        fn InternetSetOptionW(internet: *mut std::ffi::c_void, option: u32, buffer: *mut std::ffi::c_void, length: u32) -> i32;
    }
    const OPTION_SETTINGS_CHANGED: u32 = 39;
    const OPTION_REFRESH: u32 = 37;
    // SAFETY: 两个调用都只带空指针与 0 长度，返回值只是成功与否。
    unsafe {
        let _ = InternetSetOptionW(std::ptr::null_mut(), OPTION_SETTINGS_CHANGED, std::ptr::null_mut(), 0);
        let _ = InternetSetOptionW(std::ptr::null_mut(), OPTION_REFRESH, std::ptr::null_mut(), 0);
    }
}

#[cfg(not(windows))]
struct Unsupported;

#[cfg(not(windows))]
impl Env for Unsupported {
    fn read_proxy(&self) -> Result<Record, String> {
        Err("系统代理接管仅支持 Windows".into())
    }

    fn write_proxy(&self, _record: &Record) -> Result<(), String> {
        Err("系统代理接管仅支持 Windows".into())
    }

    fn listening(&self, _port: u16) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT: AtomicU64 = AtomicU64::new(0);

    struct Fake {
        current: RefCell<Record>,
        listening: bool,
        refuse: bool,
        writes: RefCell<Vec<Record>>,
    }

    impl Fake {
        fn new(record: Record, listening: bool) -> Fake {
            Fake { current: RefCell::new(record), listening, refuse: false, writes: RefCell::new(Vec::new()) }
        }
    }

    impl Env for Fake {
        fn read_proxy(&self) -> Result<Record, String> {
            Ok(self.current.borrow().clone())
        }

        fn write_proxy(&self, record: &Record) -> Result<(), String> {
            if self.refuse {
                return Err("拒绝写入".into());
            }
            self.writes.borrow_mut().push(record.clone());
            *self.current.borrow_mut() = record.clone();
            Ok(())
        }

        fn listening(&self, _port: u16) -> bool {
            self.listening
        }
    }

    fn owned() -> Record {
        Record { enable: 1, server: Some("127.0.0.1:7895".into()), bypass: Some("localhost;<local>".into()) }
    }

    fn previous() -> Record {
        Record { enable: 0, server: None, bypass: Some("localhost;<local>".into()) }
    }

    fn lease() -> PathBuf {
        let serial = NEXT.fetch_add(1, Ordering::Relaxed);
        let folder = std::env::temp_dir().join(format!("fish-proxy-heal-{}-{serial}", std::process::id()));
        std::fs::create_dir_all(&folder).unwrap();
        let path = folder.join("system-proxy.json");
        std::fs::write(
            &path,
            serde_json::json!({
                "schema": 1,
                "previous": {"ProxyEnable": 0, "ProxyServer": null, "ProxyOverride": "localhost;<local>"},
                "owned": {"ProxyEnable": 1, "ProxyServer": "127.0.0.1:7895", "ProxyOverride": "localhost;<local>"},
                "owner": {"pid": 1, "at": 1}
            })
            .to_string(),
        )
        .unwrap();
        path
    }

    #[test]
    fn no_lease_does_nothing_at_all() {
        let folder = std::env::temp_dir().join(format!("fish-proxy-heal-none-{}", std::process::id()));
        let path = folder.join("system-proxy.json");
        let env = Fake::new(owned(), false);
        assert_eq!(heal_one_with_window(&env, &path, Duration::ZERO), Outcome::NoLease);
        assert!(env.writes.borrow().is_empty());
    }

    #[test]
    fn an_empty_lease_is_no_lease() {
        let path = lease();
        std::fs::write(&path, "").unwrap();
        let env = Fake::new(owned(), false);
        assert_eq!(heal_one_with_window(&env, &path, Duration::ZERO), Outcome::NoLease);
        assert!(env.writes.borrow().is_empty());
    }

    #[test]
    fn a_broken_lease_is_dropped_without_touching_the_registry() {
        let path = lease();
        std::fs::write(&path, "{not json").unwrap();
        let env = Fake::new(owned(), false);
        assert_eq!(heal_one_with_window(&env, &path, Duration::ZERO), Outcome::BadLease);
        assert!(env.writes.borrow().is_empty());
        assert!(!path.exists());
    }

    #[test]
    fn a_lease_of_another_schema_is_dropped() {
        let path = lease();
        std::fs::write(&path, serde_json::json!({"schema": 2, "previous": {}, "owned": {}}).to_string()).unwrap();
        let env = Fake::new(owned(), false);
        assert_eq!(heal_one_with_window(&env, &path, Duration::ZERO), Outcome::BadLease);
        assert!(env.writes.borrow().is_empty());
        assert!(!path.exists());
    }

    #[test]
    fn a_registry_someone_else_changed_is_left_alone() {
        let path = lease();
        let mut other = owned();
        other.server = Some("127.0.0.1:7890".into());
        let env = Fake::new(other, false);
        assert_eq!(heal_one_with_window(&env, &path, Duration::ZERO), Outcome::NotOwned);
        assert!(env.writes.borrow().is_empty());
        assert!(!path.exists());
    }

    #[test]
    fn a_lease_written_a_moment_ago_is_left_for_the_next_round() {
        // 插件刚写完租约、还没把注册表改成自己那份：这时候删租约，等于删掉正在进行的
        // 接管，之后它真崩了就没有记录可读（#断网 的另一半）。
        let path = lease();
        let mut other = owned();
        other.server = Some("127.0.0.1:7890".into());
        let env = Fake::new(other.clone(), false);
        assert_eq!(heal_one_with_window(&env, &path, FRESH_LEASE), Outcome::Busy);
        assert!(path.exists(), "宽限期内不删");
        assert!(env.writes.borrow().is_empty());
        // 注册表还关着的那一瞬间同理。
        let path = lease();
        let env = Fake::new(Record { enable: 0, server: None, bypass: None }, false);
        assert_eq!(heal_one_with_window(&env, &path, FRESH_LEASE), Outcome::Busy);
        assert!(path.exists());
    }

    #[test]
    fn a_disabled_proxy_with_no_server_is_never_touched() {
        let path = lease();
        let env = Fake::new(Record { enable: 0, server: None, bypass: Some("localhost;<local>".into()) }, false);
        assert_eq!(heal_one_with_window(&env, &path, Duration::ZERO), Outcome::NotOwned);
        assert!(env.writes.borrow().is_empty());
        assert!(!path.exists());
    }

    #[test]
    fn a_kernel_still_listening_keeps_the_lease() {
        let path = lease();
        let env = Fake::new(owned(), true);
        assert_eq!(heal_one_with_window(&env, &path, Duration::ZERO), Outcome::LivePort);
        assert!(env.writes.borrow().is_empty());
        assert!(path.exists(), "内核还在跑时不删租约：它是对的");
    }

    #[test]
    fn a_dead_port_restores_what_was_there_before() {
        let path = lease();
        let env = Fake::new(owned(), false);
        assert_eq!(heal_one_with_window(&env, &path, Duration::ZERO), Outcome::Restored);
        assert_eq!(*env.writes.borrow(), vec![previous()]);
        assert!(!path.exists());
    }

    #[test]
    fn a_refused_write_keeps_the_lease_for_the_next_try() {
        let path = lease();
        let mut env = Fake::new(owned(), false);
        env.refuse = true;
        assert_eq!(heal_one_with_window(&env, &path, Duration::ZERO), Outcome::WriteFailed);
        assert!(path.exists());
    }

    #[test]
    fn a_busy_lease_is_skipped_not_restored_twice() {
        let path = lease();
        let first = Guard::take(&path).expect("第一个修的人拿到锁");
        assert!(Guard::take(&path).is_none(), "第二个修的人拿不到");
        drop(first);
        assert!(Guard::take(&path).is_some(), "锁会释放");
    }

    #[test]
    fn the_port_comes_from_the_lease_not_from_a_guess() {
        assert_eq!(port_of(&owned()), Some(7895));
        assert_eq!(port_of(&Record { enable: 1, server: Some("http://127.0.0.1:7890/".into()), bypass: None }), Some(7890));
        assert_eq!(port_of(&Record { enable: 1, server: Some("10.0.0.1:7895".into()), bypass: None }), None);
        assert_eq!(port_of(&Record { enable: 1, server: None, bypass: None }), None);
    }


    #[test]
    fn the_task_runs_the_heal_once_at_logon_and_every_five_minutes() {
        let xml = task_xml("S-1-5-21-1", r"C:\Program Files\FISH Dock\FISH-Dock.exe", FLAG);
        assert!(xml.contains("<LogonTrigger>"));
        assert!(xml.contains("<Interval>PT5M</Interval>"));
        assert!(xml.contains("<MultipleInstancesPolicy>IgnoreNew</MultipleInstancesPolicy>"));
        assert!(xml.contains("<ExecutionTimeLimit>PT2M</ExecutionTimeLimit>"));
        assert!(xml.contains(r"<Arguments>--proxy-heal</Arguments>"));
        assert!(xml.contains("S-1-5-21-1"));
    }

    #[test]
    fn a_plugin_that_takes_the_system_proxy_is_what_keeps_the_task() {
        let folder = std::env::temp_dir().join(format!("fish-proxy-heal-plugins-{}", std::process::id()));
        std::fs::create_dir_all(folder.join("config")).unwrap();
        let file = folder.join("config").join("installed-plugins.json");
        let with = serde_json::json!({"schema":1,"entries":{
            "fish.clash":{"manifest":{"id":"fish.clash","capabilities":["storage","system-proxy"]}},
            "fish.magpie":{"manifest":{"id":"fish.magpie","capabilities":["storage"]}}
        }});
        std::fs::write(&file, with.to_string()).unwrap();
        assert_eq!(proxy_plugin_state(&folder), Some(true));
        std::fs::write(&file, serde_json::json!({"schema":1,"entries":{"fish.magpie":{"manifest":{"capabilities":["storage"]}}}}).to_string()).unwrap();
        assert_eq!(proxy_plugin_state(&folder), Some(false));
        std::fs::remove_file(&file).unwrap();
        assert_eq!(proxy_plugin_state(&folder), None, "读不出来不能当成「没有」");
    }
}
