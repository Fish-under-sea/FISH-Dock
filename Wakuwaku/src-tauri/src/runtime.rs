//! Installed runtime resources.
//!
//! The installer expands Node, the fixed-version WebView2 runtime and the local
//! model service once, beside the program, and the program uses those files in
//! place. Nothing is unpacked at start-up: the first start verifies the whole
//! tree once (and grants the WebView2 sandbox read access once), then records the
//! manifest digest; every later start only looks at the few files that must
//! exist, so a daily start neither hashes ~776 MiB nor touches ACLs again.
#![cfg_attr(not(feature = "installed"), allow(dead_code))]
use std::path::{Path, PathBuf};
use serde::{Deserialize, Serialize};

/// Files the program cannot start without; checked on every start.
pub const REQUIRED: [&str; 14] = [
    "node/node.exe",
    "node/LICENSE",
    "webview2/msedgewebview2.exe",
    "licenses/Wakuwaku.txt",
    "windhawk/windhawk.exe",
    "windhawk/windhawk-x64-helper.exe",
    "windhawk/Engine/32/windhawk.dll",
    "windhawk/Engine/32/windhawk.lib",
    "windhawk/Engine/64/windhawk.dll",
    "windhawk/Engine/64/windhawk.lib",
    "windhawk/Compiler/bin/clang++.exe",
    "windhawk/Compiler/include/windhawk_api.h",
    "windhawk/LICENSE-Windhawk.txt",
    "windhawk/Compiler/LICENSE.TXT",
];
const SCHEMA: u32 = 2;
const PRODUCT: &str = "DSH-FISH";

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct FileRecord { pub path: String, pub bytes: u64, pub sha256: String }

/// The manifest the installer writes next to the resources it expanded.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Manifest {
    pub schema: u32, pub product: String, pub version: String, pub architecture: String,
    pub node_version: String, pub webview2_version: String,
    pub files: Vec<FileRecord>,
}

fn safe_relative(value: &str) -> Result<PathBuf, String> {
    if value.is_empty() || value.contains(['\\', ':', '\0']) {
        return Err(format!("不安全的资源路径：{value:?}"));
    }
    for part in value.split('/') {
        let base = part.split('.').next().unwrap_or("").to_ascii_uppercase();
        let reserved = matches!(base.as_str(), "CON" | "PRN" | "AUX" | "NUL" | "CLOCK$")
            || (base.len() == 4 && (base.starts_with("COM") || base.starts_with("LPT"))
                && matches!(base.as_bytes()[3], b'1'..=b'9'));
        if part.is_empty() || part == "." || part == ".." || part.ends_with(['.', ' '])
            || part.chars().any(|c| c.is_control() || "<>\"|?*".contains(c)) || reserved {
            return Err(format!("不安全的资源路径：{value:?}"));
        }
    }
    Ok(value.split('/').collect())
}

fn linked(meta: &std::fs::Metadata) -> bool {
    #[cfg(windows)] {
        use std::os::windows::fs::MetadataExt;
        meta.file_attributes() & 0x400 != 0
    }
    #[cfg(not(windows))] { meta.file_type().is_symlink() }
}

fn plain_directory(path: &Path) -> Result<(), String> {
    let meta = std::fs::symlink_metadata(path).map_err(|e| format!("无法检查 {}：{e}", path.display()))?;
    if !meta.is_dir() || linked(&meta) { return Err(format!("目录不是普通目录：{}", path.display())); }
    Ok(())
}

fn create_directories(path: &Path) -> Result<(), String> {
    if path.as_os_str().is_empty() { return Ok(()); }
    match std::fs::symlink_metadata(path) {
        Ok(_) => plain_directory(path),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            if let Some(parent) = path.parent() { create_directories(parent)?; }
            match std::fs::create_dir(path) {
                Ok(()) => Ok(()),
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => plain_directory(path),
                Err(e) => Err(format!("无法创建 {}，请检查空间与写入权限：{e}", path.display())),
            }
        }
        Err(e) => Err(format!("无法访问 {}：{e}", path.display())),
    }
}

fn file_digest(path: &Path) -> Result<(u64, String), String> {
    use sha2::{Digest, Sha256};
    use std::io::Read;
    let meta = std::fs::symlink_metadata(path).map_err(|e| e.to_string())?;
    if !meta.is_file() || linked(&meta) { return Err(format!("资源不是普通文件：{}", path.display())); }
    let mut file = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let mut hash = Sha256::new(); let mut bytes = 0; let mut buffer = [0u8; 65536];
    loop {
        let n = file.read(&mut buffer).map_err(|e| e.to_string())?;
        if n == 0 { break; }
        hash.update(&buffer[..n]); bytes += n as u64;
    }
    Ok((bytes, format!("{:x}", hash.finalize())))
}

fn valid_hash(hash: &str) -> bool { hash.len() == 64 && hash.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)) }

fn reinstall(message: String) -> String {
    format!("{message}。安装资源不完整或已损坏，请重新安装 FISH Dock。")
}

/// Reads and validates the manifest the installer left beside the resources.
fn read_manifest(root: &Path) -> Result<(Manifest, String), String> {
    let path = root.join("manifest.json");
    let meta = std::fs::symlink_metadata(&path)
        .map_err(|_| reinstall(format!("找不到安装资源清单 {}", path.display())))?;
    if !meta.is_file() || linked(&meta) { return Err(reinstall(format!("安装资源清单不是普通文件：{}", path.display()))); }
    let bytes = std::fs::read(&path).map_err(|e| reinstall(format!("无法读取安装资源清单 {}：{e}", path.display())))?;
    let manifest: Manifest = serde_json::from_slice(&bytes).map_err(|e| reinstall(format!("安装资源清单无法解析：{e}")))?;
    Ok((manifest, file_digest(&path)?.1))
}

fn records(m: &Manifest) -> Result<std::collections::BTreeMap<String, &FileRecord>, String> {
    if m.schema != SCHEMA || m.product != PRODUCT || m.architecture != "x86_64" || m.version.is_empty() {
        return Err(reinstall("安装资源清单的版本或架构无效".into()));
    }
    let mut names = std::collections::BTreeSet::new();
    let mut files = std::collections::BTreeMap::new();
    for f in &m.files {
        safe_relative(&f.path)?;
        if !valid_hash(&f.sha256) || !names.insert(f.path.to_lowercase()) {
            return Err(format!("资源清单有无效摘要或重名路径：{}", f.path));
        }
        files.insert(f.path.clone(), f);
    }
    for needed in REQUIRED {
        if !files.contains_key(needed) { return Err(reinstall(format!("资源清单缺少 {needed}"))); }
    }
    Ok(files)
}

/// The one-off check: every file the manifest lists, size and SHA-256.
fn verify_all(root: &Path, files: &std::collections::BTreeMap<String, &FileRecord>) -> Result<(), String> {
    for f in files.values() {
        let path = root.join(safe_relative(&f.path)?);
        let (size, hash) = file_digest(&path).map_err(|e| reinstall(format!("资源文件无法读取：{}（{e}）", f.path)))?;
        if size != f.bytes || hash != f.sha256 { return Err(reinstall(format!("资源文件校验失败：{}", f.path))); }
    }
    Ok(())
}

/// The daily check: only the files the program must have, by size.
fn check_required(root: &Path, files: &std::collections::BTreeMap<String, &FileRecord>) -> Result<(), String> {
    for name in REQUIRED {
        let record = files.get(name).ok_or_else(|| reinstall(format!("资源清单缺少 {name}")))?;
        let path = root.join(safe_relative(name)?);
        match std::fs::symlink_metadata(&path) {
            Ok(meta) if meta.is_file() && !linked(&meta) && meta.len() == record.bytes => {}
            _ => return Err(reinstall(format!("缺少安装资源 {name}"))),
        }
    }
    Ok(())
}

fn lock_at(root: &Path, name: &str) -> Result<std::fs::File, String> {
    create_directories(root)?;
    let path = root.join(name);
    if let Ok(meta) = std::fs::symlink_metadata(&path) {
        if !meta.is_file() || linked(&meta) { return Err(format!("锁文件不是普通文件：{}", path.display())); }
    }
    let file = std::fs::OpenOptions::new().read(true).write(true).create(true).truncate(false).open(&path)
        .map_err(|e| format!("无法取得目录锁 {}：{e}", path.display()))?;
    file.lock().map_err(|e| format!("无法锁定 {}：{e}", path.display()))?;
    Ok(file)
}

struct Staging(PathBuf);
impl Staging {
    fn new(parent: &Path, prefix: &str) -> Result<Self, String> {
        use std::sync::atomic::{AtomicU64, Ordering};
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_err(|e| e.to_string())?.as_nanos();
        let path = parent.join(format!(".{prefix}-{}-{stamp}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
        std::fs::create_dir(&path).map_err(|e| format!("无法准备临时目录 {}：{e}", path.display()))?;
        Ok(Self(path))
    }
}
impl Drop for Staging { fn drop(&mut self) { let _ = std::fs::remove_dir_all(&self.0); } }

fn migrate_data(old: &Path, new: &Path) -> Result<(), String> {
    if new.exists() { return plain_directory(new); }
    let parent = new.parent().ok_or("数据目录没有父目录")?;
    let _lock = lock_at(parent, ".migration.lock")?;
    if new.exists() { return plain_directory(new); }
    let stage = Staging::new(parent, "data-migration")?;
    fn copy_tree(from: &Path, to: &Path) -> Result<(), String> {
        plain_directory(from)?;
        for entry in std::fs::read_dir(from).map_err(|e| e.to_string())? {
            let entry = entry.map_err(|e| e.to_string())?;
            let src = entry.path(); let dst = to.join(entry.file_name());
            let meta = std::fs::symlink_metadata(&src).map_err(|e| e.to_string())?;
            if linked(&meta) { return Err(format!("旧数据中有链接，未迁移：{}", src.display())); }
            if meta.is_dir() { std::fs::create_dir(&dst).map_err(|e| e.to_string())?; copy_tree(&src, &dst)?; }
            else if meta.is_file() {
                std::fs::copy(&src, &dst).map_err(|e| format!("复制旧数据失败，原数据未删除：{e}"))?;
                if file_digest(&src)? != file_digest(&dst)? { return Err(format!("旧数据副本校验失败：{}", src.display())); }
            } else { return Err("旧数据包含特殊文件，未迁移".into()); }
        }
        Ok(())
    }
    if old.exists() { copy_tree(old, &stage.0)?; }
    std::fs::rename(&stage.0, new).map_err(|e| format!("启用新数据目录失败，原数据未删除：{e}"))
}

fn accept_terms(home: &Path, digest: &str, prompt: impl FnOnce() -> Result<bool, String>) -> Result<(), String> {
    use std::io::Write;
    let _lock = lock_at(home, ".terms.lock")?;
    let marker = home.join("accepted-runtime-terms");
    if let Ok(meta) = std::fs::symlink_metadata(&marker) {
        if !meta.is_file() || linked(&meta) { return Err("许可记录不是普通文件".into()); }
        if meta.len() < 1024 && std::fs::read_to_string(&marker).map_err(|e| e.to_string())? == digest { return Ok(()); }
    }
    if !prompt()? { return Err("未同意内置 WebView2 许可，已停止启动；未修改账号和原数据".into()); }
    let mut file = std::fs::OpenOptions::new().create(true).write(true).truncate(true).open(&marker).map_err(|e| e.to_string())?;
    file.write_all(digest.as_bytes()).and_then(|_| file.sync_all()).map_err(|e| e.to_string())
}

/// Where the last completed verification is remembered (`home/state/verified`).
fn verified_marker(home: &Path) -> PathBuf { home.join("state").join("verified") }

/// The form two paths to the same folder share, so that remembering one and
/// comparing it with another does not depend on how each was written: the
/// real path where the folder is there, without the trailing separator, and
/// as Windows compares them (case aside, `/` and `\` alike).
fn normalized(path: &Path) -> String {
    let real = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let text = real.to_string_lossy().replace('/', "\\");
    let text = text.trim_end_matches('\\');
    if text.is_empty() { return text.to_string(); }
    text.to_lowercase()
}

/// What was remembered: the manifest digest verified, and the resources
/// folder it was verified for. Two lines, `digest` and `path`; nothing when
/// the marker is not there or is not one this program wrote.
fn remembered(home: &Path) -> Option<(String, String)> {
    let text = std::fs::read_to_string(verified_marker(home)).ok()?;
    let mut lines = text.lines();
    let digest = lines.next()?.trim();
    let path = lines.next()?.trim();
    (digest.len() == 64 && !path.is_empty()).then(|| (digest.to_string(), path.to_string()))
}

fn remember(home: &Path, digest: &str, resources: &Path) -> Result<(), String> {
    use std::io::Write;
    let marker = verified_marker(home);
    create_directories(marker.parent().ok_or("状态目录没有父目录")?)?;
    let mut file = std::fs::OpenOptions::new().create(true).write(true).truncate(true).open(&marker)
        .map_err(|e| format!("无法记录安装资源校验结果 {}：{e}", marker.display()))?;
    let text = format!("{digest}\n{}\n", normalized(resources));
    file.write_all(text.as_bytes()).and_then(|_| file.sync_all()).map_err(|e| e.to_string())
}

#[cfg(all(feature = "installed", windows))]
fn prompt_terms(terms: &Path) -> Result<bool, String> {
    #[link(name = "user32")] extern "system" { fn MessageBoxW(hwnd: isize, text: *const u16, caption: *const u16, kind: u32) -> i32; }
    #[link(name = "shell32")] extern "system" { fn ShellExecuteW(hwnd: *mut core::ffi::c_void, operation: *const u16, file: *const u16, params: *const u16, directory: *const u16, show: i32) -> isize; }
    let wide = |s: &str| s.encode_utf16().chain(Some(0)).collect::<Vec<u16>>();
    let text = wide(&format!("FISH Dock 内置 Microsoft Edge WebView2 Runtime（固定版）。\n\n继续使用前，请阅读并同意其完整许可条款：\n{}\n\nMicrosoft Defender SmartScreen 默认启用，可能收集并向 Microsoft 发送信息。\n隐私声明：https://aka.ms/privacy\nSmartScreen：https://learn.microsoft.com/en-us/microsoft-edge/privacy-whitepaper#smartscreen\n\n是：我已阅读并同意上述条款，继续启动。\n否：打开完整许可原文，读完后返回确认。\n取消：退出，不启用运行时。", terms.display()));
    let title = wide("FISH Dock · 内置运行时许可");
    loop {
        match unsafe { MessageBoxW(0, text.as_ptr(), title.as_ptr(), 0x0000_0003 | 0x0000_0040 | 0x0000_0200) } {
            6 => return Ok(true),
            7 => {
                let open = wide("open"); let file = wide(&terms.to_string_lossy());
                let result = unsafe { ShellExecuteW(std::ptr::null_mut(), open.as_ptr(), file.as_ptr(), std::ptr::null(), std::ptr::null(), 1) };
                if result <= 32 { return Err(format!("无法打开许可原文，请直接查看 {}", terms.display())); }
            }
            2 => return Ok(false),
            _ => return Err("无法显示内置运行时许可确认窗口".into()),
        }
    }
}

fn webview_acl_arguments(path: &Path) -> Vec<std::ffi::OsString> {
    [path.as_os_str().to_owned(), "/grant".into(), "*S-1-15-2-1:(OI)(CI)(RX)".into(), "*S-1-15-2-2:(OI)(CI)(RX)".into(), "/T".into()].into()
}

#[cfg(all(feature = "installed", windows))]
fn grant_webview_read(path: &Path) -> Result<(), String> {
    use std::os::windows::process::CommandExt;
    // 固定版 WebView2 v120+ 在 Windows 10 上要求这两个沙箱主体能读取运行时。
    let windows = std::env::var_os("SystemRoot").ok_or("找不到 Windows 系统目录")?;
    let status = std::process::Command::new(PathBuf::from(windows).join("System32/icacls.exe"))
        .args(webview_acl_arguments(path)).creation_flags(0x0800_0000)
        .stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).status()
        .map_err(|e| format!("无法设置内置浏览器的沙箱读取权限：{e}"))?;
    if status.success() { Ok(()) } else { Err(format!("无法设置内置浏览器的沙箱读取权限：{status}")) }
}

#[cfg(feature = "installed")]
static INSTALLED: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();

#[cfg(feature = "installed")]
pub fn app_home() -> Result<PathBuf, String> {
    if let Some(path) = std::env::var_os("DSH_FISH_RUNTIME_HOME").filter(|p| !p.is_empty()) { return Ok(path.into()); }
    let local = std::env::var_os("LOCALAPPDATA").ok_or("找不到 LOCALAPPDATA 用户目录")?;
    Ok(PathBuf::from(local).join("DSH-FISH"))
}

/// Where the installer expanded Node, WebView2 and the local model service: the
/// `resources` folder beside the program, unless `DSH_FISH_RESOURCES` points
/// somewhere else (a development checkout).
#[cfg(feature = "installed")]
pub fn resources_directory() -> Result<PathBuf, String> {
    if let Some(path) = std::env::var_os("DSH_FISH_RESOURCES").filter(|p| !p.is_empty()) { return Ok(path.into()); }
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    Ok(exe.parent().ok_or("找不到程序目录")?.join("resources"))
}

#[cfg(feature = "installed")]
pub fn component(name: &str) -> Result<PathBuf, String> {
    INSTALLED.get().map(|p| p.join(name)).ok_or_else(|| "内置运行环境尚未就绪".into())
}

/// Nothing is unpacked here: the installer expands every file beside the program.
/// The first start verifies the whole tree once and grants the WebView2 sandbox
/// read access once (with the rights the person running it already has); later
/// starts only check the files that must exist, so a daily start neither hashes
/// ~776 MiB nor recursively sets permissions again. Errors always say what is
/// missing and that the program must be installed again: there is no quiet
/// fall-back to a system Node or a system WebView2.
fn prepare_at(
    resources: &Path,
    home: &Path,
    prompt: &mut dyn FnMut(&Path) -> Result<bool, String>,
    grant: &mut dyn FnMut(&Path) -> Result<(), String>,
) -> Result<(), String> {
    let (m, digest) = read_manifest(resources)?;
    let files = records(&m)?;
    create_directories(home)?;
    let first = remembered(home).is_none_or(|(was, where_)| was != digest || where_ != normalized(resources));
    if first {
        verify_all(resources, &files)?;
    } else {
        check_required(resources, &files)?;
    }
    let license = files.iter().find(|(name, _)| *name == "licenses/WebView2-Fixed-Version.html")
        .map(|(_, record)| *record)
        .ok_or_else(|| reinstall("安装资源缺少 WebView2 分发许可".into()))?;
    accept_terms(home, &license.sha256, || prompt(&resources.join("licenses/WebView2-Fixed-Version.html")))?;
    if first {
        // 固定版 WebView2 的沙箱主体读不到运行时，界面根本建不起来（WebView2
        // 只会报“灾难性故障”），所以这里必须停下来说清楚：说清是哪一步失败、
        // 该怎么修，而不是让程序起来再崩。日常启动不走这一段，所以不递归授权。
        grant(&resources.join("webview2")).map_err(|reason| {
            format!("{reason}。内置浏览器需要读取安装目录里的运行时；请把 FISH Dock 安装到你有权限写入的目录后重试（通常是重新运行安装程序）。")
        })?;
        // 只有校验、授权和许可都成功后才记住，失败的下次启动会重新校验。
        remember(home, &digest, resources)?;
    }
    create_directories(&resources.join("webview2"))?;
    create_directories(&webview_directory(home))?;
    Ok(())
}

/// Where WebView2 keeps this program's own profile: her home, so that an
/// install, a repair and an uninstall never touch it.
fn webview_directory(home: &Path) -> PathBuf { home.join("webview") }

#[cfg(feature = "installed")]
pub fn prepare() -> Result<(), String> {
    let home = app_home()?;
    let resources = resources_directory()?;
    prepare_at(&resources, &home, &mut |terms| prompt_terms(terms), &mut |webview| grant_webview_read(webview))?;
    if let Some(override_path) = std::env::var_os("WAKUWAKU_USER_DATA").filter(|p| !p.is_empty()) {
        create_directories(Path::new(&override_path))?;
    } else {
        let exe = std::env::current_exe().map_err(|e| e.to_string())?;
        let old = exe.parent().ok_or("找不到程序目录")?.join("wakuwaku-data");
        migrate_data(&old, &home.join("data"))?;
    }
    std::env::set_var("WEBVIEW2_BROWSER_EXECUTABLE_FOLDER", resources.join("webview2"));
    std::env::set_var("WEBVIEW2_USER_DATA_FOLDER", webview_directory(&home));
    INSTALLED.set(resources).map_err(|_| "运行环境重复初始化".to_string())?;
    Ok(())
}

#[cfg(feature = "installed")]
pub fn show_failure(message: &str) {
    let text = format!("FISH Dock 启动失败\n\n{message}\n\n账号与原数据不会被删除；若提示安装资源缺失或损坏，请重新运行安装程序修复。");
    eprintln!("{text}");
    #[cfg(windows)] unsafe {
        #[link(name = "user32")] extern "system" { fn MessageBoxW(hwnd: isize, text: *const u16, caption: *const u16, kind: u32) -> i32; }
        let text: Vec<u16> = text.encode_utf16().chain(Some(0)).collect();
        let title: Vec<u16> = "FISH Dock".encode_utf16().chain(Some(0)).collect();
        MessageBoxW(0, text.as_ptr(), title.as_ptr(), 0x0000_0010 | 0x0001_0000);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{fs, sync::atomic::{AtomicU64, Ordering}};
    use sha2::{Digest, Sha256};
    static NEXT: AtomicU64 = AtomicU64::new(0);
    struct Temp(PathBuf);
    impl Temp { fn new() -> Self {
        let p = std::env::temp_dir().join(format!("fish-runtime-test-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
        fs::create_dir_all(&p).unwrap(); Self(p)
    }}
    impl Drop for Temp { fn drop(&mut self) { let _ = fs::remove_dir_all(&self.0); } }

    /// Writes the files an install leaves behind, plus a manifest describing them.
    fn installed(root: &Path, extra: bool, version: &str) -> Manifest {
        let mut names: Vec<String> = REQUIRED.iter().map(|n| n.to_string()).collect();
        names.push("licenses/WebView2-Fixed-Version.html".into());
        if extra { names.push("licenses/NOTICE.txt".into()); }
        let mut files = Vec::new();
        for name in names {
            let data = format!("installed: {name}").into_bytes();
            let target = root.join(&name);
            fs::create_dir_all(target.parent().unwrap()).unwrap();
            fs::write(&target, &data).unwrap();
            files.push(FileRecord { path: name, bytes: data.len() as u64, sha256: format!("{:x}", Sha256::digest(&data)) });
        }
        let m = Manifest { schema: 2, product: "DSH-FISH".into(), version: version.into(), architecture: "x86_64".into(), node_version: "24.14.0".into(), webview2_version: "155.0.4283.62".into(), files };
        fs::write(root.join("manifest.json"), serde_json::to_vec(&m).unwrap()).unwrap();
        m
    }

    #[test] fn first_start_verifies_everything_then_daily_starts_only_look_at_required_files() {
        let t = Temp::new(); let root = t.0.join("resources"); fs::create_dir_all(&root).unwrap();
        let home = t.0.join("home");
        installed(&root, true, "0.3.3");
        let prompts = std::cell::Cell::new(0); let grants = std::cell::Cell::new(0);
        let mut prompt = |_: &Path| { prompts.set(prompts.get() + 1); Ok(true) };
        let mut grant = |_: &Path| { grants.set(grants.get() + 1); Ok(()) };
        prepare_at(&root, &home, &mut prompt, &mut grant).unwrap();
        assert_eq!((prompts.get(), grants.get()), (1, 1), "首次启动校验并授权一次");
        // 日常启动不遍历整棵树、不再递归授权：清单里非必需的文件消失后照样启动。
        fs::remove_file(root.join("licenses/NOTICE.txt")).unwrap();
        prepare_at(&root, &home, &mut prompt, &mut grant).unwrap();
        assert_eq!((prompts.get(), grants.get()), (1, 1), "日常启动不得再全量校验或重复授权");
        // 必需资源缺失时必须明确提示重装。
        fs::remove_file(root.join("node/node.exe")).unwrap();
        let error = prepare_at(&root, &home, &mut prompt, &mut grant).unwrap_err();
        assert!(error.contains("重新安装"), "{error}");
        assert!(error.contains("node/node.exe"), "{error}");
    }

    #[test] fn a_first_start_verifies_every_file_and_names_the_damaged_one() {
        let t = Temp::new(); let root = t.0.join("resources"); fs::create_dir_all(&root).unwrap();
        let home = t.0.join("home");
        installed(&root, true, "0.3.3");
        fs::write(root.join("licenses/NOTICE.txt"), "tampered").unwrap();
        let mut prompt = |_: &Path| Ok(true); let mut grant = |_: &Path| Ok(());
        let error = prepare_at(&root, &home, &mut prompt, &mut grant).unwrap_err();
        assert!(error.contains("licenses/NOTICE.txt"), "{error}");
        assert!(error.contains("重新安装"), "{error}");
        assert!(!verified_marker(&home).exists(), "校验失败不能记成已验证");
    }

    #[test] fn new_install_resources_are_verified_again_and_the_persons_data_survives() {
        let t = Temp::new(); let root = t.0.join("resources"); fs::create_dir_all(&root).unwrap();
        let home = t.0.join("home");
        installed(&root, true, "0.3.3");
        let mut prompt = |_: &Path| Ok(true); let mut grant = |_: &Path| Ok(());
        prepare_at(&root, &home, &mut prompt, &mut grant).unwrap();
        fs::create_dir_all(home.join("data")).unwrap();
        fs::write(home.join("data/settings.json"), "mine").unwrap();
        fs::write(home.join("account.json"), "keep").unwrap();
        // 换了安装资源（清单摘要不同）：整棵树重验一次，用户数据原地不动。
        installed(&root, true, "0.3.4");
        fs::remove_file(root.join("licenses/NOTICE.txt")).unwrap();
        let error = prepare_at(&root, &home, &mut prompt, &mut grant).unwrap_err();
        assert!(error.contains("NOTICE.txt"), "{error}");
        assert_eq!(fs::read_to_string(home.join("data/settings.json")).unwrap(), "mine");
        assert_eq!(fs::read_to_string(home.join("account.json")).unwrap(), "keep");
    }

    #[test] fn missing_install_resources_ask_for_a_reinstall_instead_of_a_fall_back() {
        let t = Temp::new(); let root = t.0.join("missing"); let home = t.0.join("home");
        let mut prompt = |_: &Path| Ok(true); let mut grant = |_: &Path| Ok(());
        let error = prepare_at(&root, &home, &mut prompt, &mut grant).unwrap_err();
        assert!(error.contains("重新安装"), "{error}");
        assert!(error.contains("manifest.json"), "{error}");
    }

    #[test] fn an_unsafe_or_foreign_manifest_is_refused() {
        let t = Temp::new(); let root = t.0.join("resources"); fs::create_dir_all(&root).unwrap();
        let home = t.0.join("home");
        let mut m = installed(&root, true, "0.3.3");
        m.files.push(FileRecord { path: "../escape".into(), bytes: 1, sha256: "0".repeat(64) });
        fs::write(root.join("manifest.json"), serde_json::to_vec(&m).unwrap()).unwrap();
        let mut prompt = |_: &Path| Ok(true); let mut grant = |_: &Path| Ok(());
        assert!(prepare_at(&root, &home, &mut prompt, &mut grant).is_err());
        let mut m = installed(&root, true, "0.3.3");
        m.schema = 1;
        fs::write(root.join("manifest.json"), serde_json::to_vec(&m).unwrap()).unwrap();
        assert!(prepare_at(&root, &home, &mut prompt, &mut grant).unwrap_err().contains("重新安装"));
    }

    #[test] fn a_refused_license_stops_the_start_without_remembering_anything() {
        let t = Temp::new(); let root = t.0.join("resources"); fs::create_dir_all(&root).unwrap();
        let home = t.0.join("home");
        installed(&root, true, "0.3.3");
        let mut prompt = |_: &Path| Ok(false); let mut grant = |_: &Path| panic!("授权只能发生在同意许可之后");
        assert!(prepare_at(&root, &home, &mut prompt, &mut grant).is_err());
        assert!(!verified_marker(&home).exists());
    }

    #[test] fn traversal_and_windows_aliases_are_rejected() {
        for s in ["/outside", "../escape", "a/../escape", "C:/escape", "C:escape", "\\\\server\\file", "a\\escape", "a//b", "./a", "NUL", "a/COM1.txt", "a/name.", "a/name ", "a:stream", ""] {
            assert!(safe_relative(s).is_err(), "accepted unsafe path: {s:?}");
        }
        assert!(safe_relative("webview2/locales/zh-CN.pak").is_ok());
    }

    #[test] fn blocked_home_directory_is_reported_without_touching_it() {
        let t = Temp::new(); let root = t.0.join("resources"); fs::create_dir_all(&root).unwrap();
        installed(&root, true, "0.3.3");
        let blocked = t.0.join("blocked"); fs::write(&blocked, "keep").unwrap();
        let mut prompt = |_: &Path| Ok(true); let mut grant = |_: &Path| Ok(());
        assert!(prepare_at(&root, &blocked, &mut prompt, &mut grant).is_err());
        assert_eq!(fs::read_to_string(blocked).unwrap(), "keep");
    }

    #[test] fn webview_permissions_are_read_only_and_runtime_scoped() {
        let path = Path::new("C:/runtime/webview2");
        let args = webview_acl_arguments(path);
        assert_eq!(args[0], path.as_os_str());
        assert_eq!(args[1], "/grant");
        assert_eq!(args[2], "*S-1-15-2-1:(OI)(CI)(RX)");
        assert_eq!(args[3], "*S-1-15-2-2:(OI)(CI)(RX)");
        assert_eq!(args[4], "/T");
    }

    #[test] fn terms_require_acceptance_and_only_remember_the_same_license() {
        let t = Temp::new();
        assert!(accept_terms(&t.0, "license-a", || Ok(false)).is_err());
        assert!(!t.0.join("accepted-runtime-terms").exists());
        accept_terms(&t.0, "license-a", || Ok(true)).unwrap();
        accept_terms(&t.0, "license-a", || panic!("accepted terms must not prompt again")).unwrap();
        assert!(accept_terms(&t.0, "license-b", || Ok(false)).is_err());
        assert_eq!(fs::read_to_string(t.0.join("accepted-runtime-terms")).unwrap(), "license-a");
    }

    #[test] fn old_data_is_copied_and_existing_new_data_wins() {
        let t = Temp::new(); let old = t.0.join("old"); let new = t.0.join("new");
        fs::create_dir_all(old.join("pets")).unwrap(); fs::write(old.join("pets/a.json"), "pet").unwrap();
        migrate_data(&old, &new).unwrap();
        assert_eq!(fs::read_to_string(new.join("pets/a.json")).unwrap(), "pet");
        fs::write(new.join("pets/a.json"), "changed").unwrap(); migrate_data(&old, &new).unwrap();
        assert_eq!(fs::read_to_string(new.join("pets/a.json")).unwrap(), "changed");
        assert_eq!(fs::read_to_string(old.join("pets/a.json")).unwrap(), "pet");
    }

    // 同一个 home 换了一套安装资源（另一个目录，或搬过家的安装）：那份“已验证”
    // 只对这一套目录成立，所以必须重新全量校验并按新目录重新授权一次；同目录
    // 的日常启动仍只 stat 必需文件、不再授权。
    #[test]
    fn a_different_resources_directory_is_verified_and_granted_again() {
        let t = Temp::new();
        let (a, b) = (t.0.join("a/resources"), t.0.join("b/resources"));
        for root in [&a, &b] { fs::create_dir_all(root).unwrap(); installed(root, true, "0.3.3"); }
        let home = t.0.join("home");
        let prompts = std::cell::Cell::new(0); let grants = std::cell::Cell::new(0);
        let mut prompt = |_: &Path| { prompts.set(prompts.get() + 1); Ok(true) };
        let mut grant = |_: &Path| { grants.set(grants.get() + 1); Ok(()) };
        prepare_at(&a, &home, &mut prompt, &mut grant).unwrap();
        assert_eq!((prompts.get(), grants.get()), (1, 1));
        // 同一套目录再来一次：不快照、不哈希、不再授权。
        prepare_at(&a, &home, &mut prompt, &mut grant).unwrap();
        assert_eq!((prompts.get(), grants.get()), (1, 1), "同目录的日常启动不得重新校验或授权");
        // 另一套目录：重新全量校验（非必需文件被改动也点名）并且重新授权。
        fs::write(b.join("licenses/NOTICE.txt"), "tampered").unwrap();
        let error = prepare_at(&b, &home, &mut prompt, &mut grant).unwrap_err();
        assert!(error.contains("licenses/NOTICE.txt"), "{error}");
        assert!(error.contains("重新安装"), "{error}");
        assert_eq!(grants.get(), 1, "校验没通过就不该授权");
        // 修好之后：全量校验通过，按新目录重新授权一次，标记改记新目录。
        installed(&b, true, "0.3.3");
        prepare_at(&b, &home, &mut prompt, &mut grant).unwrap();
        assert_eq!((prompts.get(), grants.get()), (1, 2), "换了安装目录要重新授权一次");
        assert_eq!(remembered(&home).unwrap().1, normalized(&b));
        prepare_at(&b, &home, &mut prompt, &mut grant).unwrap();
        assert_eq!(grants.get(), 2, "新目录的日常启动也不得重复授权");
    }

    // 换目录那次授权被拒：不能把这一套记成“已验证”，下次启动得再试一次
    // （否则界面会因为沙箱读不到运行时建不起来，而程序还以为一切就绪）。
    #[test]
    fn a_refused_grant_for_a_new_directory_is_not_remembered() {
        let t = Temp::new();
        let (a, b) = (t.0.join("a/resources"), t.0.join("b/resources"));
        for root in [&a, &b] { fs::create_dir_all(root).unwrap(); installed(root, true, "0.3.3"); }
        let home = t.0.join("home");
        let mut prompt = |_: &Path| Ok(true);
        let mut ok = |_: &Path| Ok(());
        prepare_at(&a, &home, &mut prompt, &mut ok).unwrap();
        let was = remembered(&home).unwrap();
        let mut refused = |_: &Path| Err("无法设置内置浏览器的沙箱读取权限：exit code: 5".to_string());
        let error = prepare_at(&b, &home, &mut prompt, &mut refused).unwrap_err();
        assert!(error.contains("重新运行安装程序"), "{error}");
        assert_eq!(remembered(&home).unwrap(), was, "被拒的那套目录不能记成已验证");
    }

    #[test]
    fn the_installed_resources_are_used_in_place() {
        let t = Temp::new(); let root = t.0.join("resources"); fs::create_dir_all(&root).unwrap();
        let home = t.0.join("home");
        installed(&root, true, "0.3.3");
        let mut prompt = |_: &Path| Ok(true); let mut grant = |_: &Path| Ok(());
        prepare_at(&root, &home, &mut prompt, &mut grant).unwrap();
        assert!(root.join(REQUIRED[0]).is_file());
        assert!(root.join("webview2").is_dir());
        assert_eq!(webview_directory(&home), home.join("webview"));
        assert!(home.join("webview").is_dir());
    }

    // 安装目录的 ACL 由安装目录继承的权限决定：icacls 有时会被拒。被拒时
    // WebView2 建不出界面，所以必须在启动时停下来说清楚怎样修，而且不能
    // 记成“以后不必再授权”——日常启动本来就不做这一段。
    #[test]
    fn a_refused_permission_grant_stops_the_start_and_says_how_to_fix_it() {
        let t = Temp::new(); let root = t.0.join("resources"); fs::create_dir_all(&root).unwrap();
        let home = t.0.join("home");
        installed(&root, true, "0.3.3");
        let mut prompt = |_: &Path| Ok(true);
        let mut grant = |_: &Path| Err("无法设置内置浏览器的沙箱读取权限：exit code: 5".to_string());
        let error = prepare_at(&root, &home, &mut prompt, &mut grant).unwrap_err();
        assert!(error.contains("沙箱读取权限"), "{error}");
        assert!(error.contains("重新运行安装程序"), "{error}");
        assert!(!verified_marker(&home).exists(), "授权失败不能记成已验证，修好后要能重试");
    }
}
