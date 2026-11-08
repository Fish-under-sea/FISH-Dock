//! Installed runtime resources.
//!
//! The installer expands Node and the local model service once, beside the
//! program, and the program uses those files in place. WebView2 is not bundled
//! since 2.1.9: the system Evergreen runtime is used instead, and a missing one
//! is reported by the Setup installer's pre-check — the program neither checks
//! nor prompts for it at start-up. Nothing is unpacked at start-up, and the
//! start-up path never walks the whole tree (2.0.3 T07): it stat()s the handful
//! of files the program cannot start without — milliseconds — and a background
//! thread hashes only the files whose recorded size+mtime no longer matches.
//! `state/verified` keeps one fingerprint per file, not the manifest's digest,
//! so a new manifest (a version bump, a new file list) no longer means a full
//! re-verification (T06).
#![cfg_attr(not(feature = "installed"), allow(dead_code))]
use std::path::{Path, PathBuf};
use serde::{Deserialize, Serialize};

/// Files the program cannot start without; checked on every start.
pub const REQUIRED: [&str; 3] = [
    "node/node.exe",
    "node/LICENSE",
    "licenses/Wakuwaku.txt",
];
// #73：`windhawk/` 整棵（引擎 + LLVM-MinGW 编译器，原始 557.8 MB / 8413 个文件）不再随包分发，
// 因此也不再是启动必需项。外观页改为只连接用户自己安装的 Windhawk（`windhawk::configured`
// 的「配置路径 → 注册表 → Program Files」三档）。
const SCHEMA: u32 = 2;
/// `state/verified` 自己的格式版本。旧的两行格式（清单摘要 + 目录）解析不出来，
/// 按「没有记录」处理：代价是升级后多校验一次，不写迁移脚本（T06）。
const MARKER_SCHEMA: u32 = 1;
// 安装资源清单里的 product：2.0.0 起写 "FISH Dock"（旧名清理 #27 P1），
// 但机器上还装着旧包（写 "DSH-FISH"）时也得读得动，所以两个都认。
const PRODUCTS: [&str; 2] = ["FISH Dock", "DSH-FISH"];

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct FileRecord { pub path: String, pub bytes: u64, pub sha256: String }

/// 一个文件的指纹：大小 + 修改时刻（纳秒；Windows 的时间戳精度是 100 ns，
/// 足以分开两次写入）。判据就是它——不是清单摘要，也不是文件内容。
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct Stamp { pub bytes: u64, pub mtime: u64 }

/// 上一次校验通过时记下的东西（`home/state/verified`）：校验过的那套资源目录，
/// 以及每个文件的指纹。更新换了清单只说明「清单变了」，文件没变就不重校。
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Verified {
    pub schema: u32,
    pub resources: String,
    pub files: std::collections::BTreeMap<String, Stamp>,
}

/// The manifest the installer writes next to the resources it expanded.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Manifest {
    pub schema: u32, pub product: String, pub version: String, pub architecture: String,
    pub node_version: String,
    /// 2.1.9 起不再随包内置 WebView2，清单不再写它；旧清单（schema 2，2.1.8 及以前）里的
    /// 值仍可解析，只是不再使用。写入端也兼容两头：有值照写（老包），没有就整键省略。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub webview2_version: Option<String>,
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
/// 它的摘要不再参与「要不要重校」的判断（T06），所以这里也不读它一遍：
/// 清单自己坏了（架构/PRODUCT/路径/摘要无效）由 `records()` 当场拒绝。
fn read_manifest(root: &Path) -> Result<Manifest, String> {
    let path = root.join("manifest.json");
    let meta = std::fs::symlink_metadata(&path)
        .map_err(|_| reinstall(format!("找不到安装资源清单 {}", path.display())))?;
    if !meta.is_file() || linked(&meta) { return Err(reinstall(format!("安装资源清单不是普通文件：{}", path.display()))); }
    let bytes = std::fs::read(&path).map_err(|e| reinstall(format!("无法读取安装资源清单 {}：{e}", path.display())))?;
    serde_json::from_slice(&bytes).map_err(|e| reinstall(format!("安装资源清单无法解析：{e}")))
}

fn records(m: &Manifest) -> Result<std::collections::BTreeMap<String, &FileRecord>, String> {
    if m.schema != SCHEMA || !PRODUCTS.contains(&m.product.as_str()) || m.architecture != "x86_64" || m.version.is_empty() {
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

/// 一个文件的指纹；不存在的、目录、链接、特殊文件都没有指纹（None）。
fn stamp(path: &Path) -> Option<Stamp> {
    let meta = std::fs::symlink_metadata(path).ok()?;
    if !meta.is_file() || linked(&meta) { return None; }
    let mtime = meta.modified().ok()?.duration_since(std::time::UNIX_EPOCH).ok()?.as_nanos();
    Some(Stamp { bytes: meta.len(), mtime: mtime as u64 })
}

/// 哪些文件要重算哈希：磁盘上的 size+mtime 与记下的不符（新文件、被替换、
/// 被改过、被删掉），或者整套资源换了目录。清单里已经不再列出的文件不查，
/// 记下的多余条目也只是白记——下一次 remember 就不再写它们。
fn pending(resources: &Path, files: &std::collections::BTreeMap<String, &FileRecord>, was: Option<&Verified>)
    -> (Vec<String>, std::collections::BTreeMap<String, Stamp>) {
    let same_place = was.is_some_and(|was| was.resources == normalized(resources));
    let mut stale = Vec::new();
    // R3：把这一轮**实际看到**的指纹一起带出去。它才是判据用的那一份，记标记时只写它；
    // 以前是在哈希完之后再 stat 一遍，那次 stat 会把「没校验过的内容」记成已验证。
    let mut seen = std::collections::BTreeMap::new();
    for name in files.keys() {
        let now = safe_relative(name).ok().and_then(|relative| stamp(&resources.join(relative)));
        let known = was.filter(|_| same_place).and_then(|was| was.files.get(name));
        if known != now.as_ref() { stale.push(name.clone()); }
        if let Some(now) = now { seen.insert(name.clone(), now); }
    }
    (stale, seen)
}

/// 启动关键路径之外的那一半：只对指纹变了的文件重算 SHA-256。更新通常只
/// 换一两个文件（install.log：「替换 1 个文件，复用 10259 个」），所以常态是
/// 零个文件、零次读盘；指纹都没变、目录也没换，就连标记都不必重写。
fn verify_pending_at(resources: &Path, home: &Path) -> Result<(), String> {
    let manifest = read_manifest(resources)?;
    let files = records(&manifest)?;
    let was = remembered(home);
    let (stale, mut seen) = pending(resources, &files, was.as_ref());
    for name in &stale {
        let record = files.get(name).ok_or_else(|| reinstall(format!("资源清单缺少 {name}")))?;
        let path = resources.join(safe_relative(&record.path)?);
        // R3：哈希与「记指纹」之间文件若又被改写，绝不能把没校验过的内容记成已验证。
        // 哈希前后各取一次指纹：两次一样，说明读到的就是这份内容，标记里也只写它；
        // 不一样就按 stale 重来一次，仍不稳定就报错——宁可重校，也不假装验过。
        let mut stable = None;
        for _ in 0..2 {
            let before = stamp(&path);
            let (size, hash) = file_digest(&path).map_err(|e| reinstall(format!("资源文件无法读取：{}（{e}）", record.path)))?;
            if size != record.bytes || hash != record.sha256 { return Err(reinstall(format!("资源文件校验失败：{}", record.path))); }
            if stamp(&path) == before {
                if let Some(before) = before { seen.insert(name.clone(), before); }
                stable = Some(());
                break;
            }
            eprintln!("FISH Dock: 校验期间资源文件被改动，重算一遍：{}", record.path);
        }
        if stable.is_none() { return Err(reinstall(format!("校验期间资源文件反复被改动：{}", record.path))); }
    }
    let kept = stale.is_empty() && was.is_some_and(|was| was.resources == normalized(resources));
    if kept { return Ok(()); }
    remember(home, resources, &seen)
}

/// The synchronous half of a start: only the files the program must have, by
/// size — three stat()s, milliseconds. Everything else waits for the
/// background thread (`verify_pending_at`), so the first frame never waits for
/// 10,513 files / 1.30 GB (T07).
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

/// What was remembered: one fingerprint per file the manifest lists, and the
/// resources folder they were verified for. Nothing when the marker is not
/// there, is not one this program wrote, or is the old two-line format (a
/// digest and a path) — that last one is treated as no record at all, so the
/// upgrade after which it appears verifies once more and rewrites it.
fn remembered(home: &Path) -> Option<Verified> {
    let text = std::fs::read(verified_marker(home)).ok()?;
    let was: Verified = serde_json::from_slice(&text).ok()?;
    (was.schema == MARKER_SCHEMA && !was.resources.is_empty()).then_some(was)
}

fn remember(home: &Path, resources: &Path, recorded: &std::collections::BTreeMap<String, Stamp>) -> Result<(), String> {
    use std::io::Write;
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let marker = verified_marker(home);
    let directory = marker.parent().ok_or("状态目录没有父目录")?;
    create_directories(directory)?;
    // 指纹用这一轮实际看到的那一份（R3），不在这里重新 stat 一遍。
    let was = Verified { schema: MARKER_SCHEMA, resources: normalized(resources), files: recorded.clone() };
    let text = serde_json::to_vec(&was).map_err(|e| format!("无法记录安装资源校验结果：{e}"))?;
    // 原子替换（R1）：先写同目录的临时文件、sync 到盘，再 rename 盖上去。同一卷上的
    // rename 是原子的，所以断电最坏也只是留下**完整**的旧记录或**完整**的新记录；
    // 以前 truncate+write 撞上断电会留半截 JSON，下次启动解析失败 → 整棵树（10,513 个
    // 文件 / 1.30 GB）重校一遍，正好把 T06/T07 省下的那次又赔回去。
    // 锁照同文件 lock_at 的那套：多开或将来并发写时不会
    // 互相截断。rename/写入任何一步失败都只删掉临时文件，旧记录原封不动，并留一行日志。
    let _lock = lock_at(directory, ".verified.lock")?;
    let temporary = directory.join(format!(".verified-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
    let written = (|| -> std::io::Result<()> {
        let mut file = std::fs::OpenOptions::new().create_new(true).write(true).open(&temporary)?;
        file.write_all(&text)?;
        file.sync_all()
    })();
    let placed = written.and_then(|()| crate::storage::replace(&temporary, &marker));
    if let Err(error) = placed {
        let _ = std::fs::remove_file(&temporary);
        eprintln!("FISH Dock: 无法记录安装资源校验结果，旧记录保留（下次启动会再校验一次）：{error}");
        return Err(format!("无法记录安装资源校验结果 {}：{error}", marker.display()));
    }
    Ok(())
}

#[cfg(feature = "installed")]
static INSTALLED: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();

// 数据根的名字只有一份：config::HOME_NAME（2.0.0 改名 %LOCALAPPDATA%\FISH Dock；
// 与安装器的一致性说明写在那个常量上，这里只取用，不再各写一个同名字面量）。

// app_home_from 是数据根的唯一来源：FISH_DOCK_APP_HOME（非空、绝对路径）优先，
// 否则 %LOCALAPPDATA%\FISH Dock。空串与相对路径按「没设」处理，回落默认——排障时
// 手滑写出这两种值，不该把数据根指到半截路径上。
//
// 别再借用 FISH_DOCK_HOME：那是「统一数据根」（storage_context.rs，凭据、roster、
// keys 那一套，默认 %USERPROFILE%\.fish-dock）的覆盖名，与这里的数据根是两个根，
// 共用一个变量会把两边指到同一目录。旧名的 DSH_FISH_RUNTIME_HOME 也不再认：2.0.0
// 只认新根，迁移与删旧由安装器负责，宿主不读旧目录。
#[cfg(any(feature = "installed", test))]
fn app_home_from(override_home: Option<std::ffi::OsString>, local_app_data: Option<std::ffi::OsString>) -> Result<PathBuf, String> {
    let chosen = override_home.filter(|raw| !raw.is_empty()).map(PathBuf::from).filter(|path| path.is_absolute());
    if let Some(path) = chosen { return Ok(path); }
    let local = local_app_data.ok_or("找不到 LOCALAPPDATA 用户目录")?;
    Ok(PathBuf::from(local).join(crate::config::HOME_NAME))
}

#[cfg(any(feature = "installed", test))]
pub fn app_home() -> Result<PathBuf, String> {
    app_home_from(std::env::var_os("FISH_DOCK_APP_HOME"), std::env::var_os("LOCALAPPDATA"))
}

/// Where the installer expanded Node and the local model service: the
/// `resources` folder beside the program, unless `FISH_DOCK_RESOURCES` (or its
/// read-only legacy alias `DSH_FISH_RESOURCES`) points somewhere else (a
/// development checkout). 2.0.4 T-Q: the resolution lives in
/// [`crate::config::resources_override`] only — no second copy of the env names.
#[cfg(feature = "installed")]
pub fn resources_directory() -> Result<PathBuf, String> {
    if let Some(path) = crate::config::resources_override() { return Ok(path); }
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    Ok(exe.parent().ok_or("找不到程序目录")?.join("resources"))
}

#[cfg(feature = "installed")]
pub fn component(name: &str) -> Result<PathBuf, String> {
    INSTALLED.get().map(|p| p.join(name)).ok_or_else(|| "内置运行环境尚未就绪".into())
}

/// Nothing is unpacked here: the installer expands every file beside the program.
///
/// This is the whole synchronous half of a start (T07). It reads the manifest,
/// rejects one that is foreign or malformed, and stat()s the three files the
/// program cannot start without. It does **not** hash anything: the per-file
/// fingerprints (`verify_pending_at`) are the background thread's work, so the
/// first frame no longer waits for the 10,513 files / 1.30 GB walk.
/// Since 2.1.9 the fixed-version WebView2 is gone (the system Evergreen runtime
/// is used instead, checked by Setup before it runs): there is no licence to
/// take and no sandbox DACL to grant here.
/// Errors always say what is missing and that the program must be installed
/// again: there is no quiet fall-back to a system Node.
fn prepare_at(resources: &Path, home: &Path) -> Result<(), String> {
    let manifest = read_manifest(resources)?;
    let files = records(&manifest)?;
    create_directories(home)?;
    check_required(resources, &files)?;
    create_directories(&webview_directory(home))?;
    // 2.1.9：WebView2 不再随包内置，运行时由系统 Evergreen 提供。缺失时的提示与安装
    // 由 Setup 安装程序的前置检测负责（Update 包不检测），宿主启动既不检测也不弹窗；
    // 这里也不再处理固定版的分发许可或沙箱 ACL。webview 用户数据目录照旧建好（见下）。
    Ok(())
}

/// Where WebView2 keeps this program's own profile: her home, so that an
/// install, a repair and an uninstall never touch it.
fn webview_directory(home: &Path) -> PathBuf { home.join("webview") }

/// `prepare()` 解析出来、原样交给后台那半的两个目录（R2）。后台线程**不再自己推导**
/// `app_home()` / `resources_directory()`：那两个调用一旦失败，旧实现只剩一句无声的
/// `return`，全篇唯一一处「静默吞掉」——用户与日志都不知道这次启动根本没做全量校验。
#[derive(Clone, Debug)]
pub struct Prepared { pub home: PathBuf, pub resources: PathBuf }

/// 目录齐不齐：空路径当作没有。`Prepared` 由 `prepare()` 填，构造上不该为空；这条判据
/// 留给手写 `Prepared` 的调用方，而它必须出声（`verify_in_background` 会照着打一行日志），
/// 不许无日志跳过。
fn prepared_directories(prepared: &Prepared) -> Result<(), String> {
    if prepared.home.as_os_str().is_empty() || prepared.resources.as_os_str().is_empty() {
        return Err("后台资源校验没有可用的数据目录或资源目录，本次跳过全量校验".into());
    }
    Ok(())
}

#[cfg(feature = "installed")]
pub fn prepare() -> Result<Prepared, String> {
    let home = app_home()?;
    let resources = resources_directory()?;
    prepare_at(&resources, &home)?;
    if let Some(override_path) = crate::config::user_data_override() {
        create_directories(&override_path)?;
    } else {
        let exe = std::env::current_exe().map_err(|e| e.to_string())?;
        let old = exe.parent().ok_or("找不到程序目录")?.join("wakuwaku-data");
        migrate_data(&old, &home.join("data"))?;
    }
    // 只保留用户数据目录：窗口创建前必须就位。2.1.9 起不再设置
    // WEBVIEW2_BROWSER_EXECUTABLE_FOLDER，WebView2 使用系统 Evergreen 运行时。
    std::env::set_var("WEBVIEW2_USER_DATA_FOLDER", webview_directory(&home));
    INSTALLED.set(resources.clone()).map_err(|_| "运行环境重复初始化".to_string())?;
    // 两个目录原样交出去（R2）：后台那半拿着用，不再重新推导一遍。
    Ok(Prepared { home, resources })
}

/// 启动关键路径之外的那一半（T07）：只对指纹变了的文件重算 SHA-256，然后把
/// 新指纹记回 `state/verified`。更新通常只换一两个文件（install.log：「替换 1
/// 个文件，复用 10259 个」），所以常态是零个文件、零次读盘；首帧与插件启动
/// 都不等它。它失败说明安装资源真的坏了：弹错并退出，与 2.0.2「校验通过才
/// 往下走」的保证一致——只是这个判断晚了几秒，不再挡着第一帧。
///
/// 目录由 `prepare()` 交过来（R2），这里不推导、也不会无声跳过：万一拿到的目录是空的，
/// 先打一行日志再说。
#[cfg(feature = "installed")]
pub fn verify_in_background(prepared: Prepared) {
    std::thread::spawn(move || {
        if let Err(reason) = prepared_directories(&prepared) {
            eprintln!("FISH Dock: {reason}（首帧不受影响，下次启动再试）");
            return;
        }
        if let Err(error) = verify_pending_at(&prepared.resources, &prepared.home) {
            show_failure(&error);
            std::process::exit(1);
        }
    });
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
    // 改环境变量的用例互斥；与 config.rs 的测试共用同一把锁（两边都会动 FISH_DOCK_APP_HOME）。
    struct Temp(PathBuf);
    impl Temp { fn new() -> Self {
        let p = std::env::temp_dir().join(format!("fish-runtime-test-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
        fs::create_dir_all(&p).unwrap(); Self(p)
    }}
    impl Drop for Temp { fn drop(&mut self) { let _ = fs::remove_dir_all(&self.0); } }

    /// Writes the files an install leaves behind, plus the manifest the 2.1.9
    /// installer writes (schema 2, no `webview2_version`; that key still parses
    /// when an old install left it behind).
    fn installed(root: &Path, extra: bool, version: &str) -> Manifest {
        let mut names: Vec<String> = REQUIRED.iter().map(|n| n.to_string()).collect();
        if extra { names.push("licenses/NOTICE.txt".into()); }
        let mut files = Vec::new();
        for name in names {
            let data = format!("installed: {name}").into_bytes();
            let target = root.join(&name);
            fs::create_dir_all(target.parent().unwrap()).unwrap();
            fs::write(&target, &data).unwrap();
            files.push(FileRecord { path: name, bytes: data.len() as u64, sha256: format!("{:x}", Sha256::digest(&data)) });
        }
        let m = Manifest { schema: 2, product: "DSH-FISH".into(), version: version.into(), architecture: "x86_64".into(), node_version: "24.14.0".into(), webview2_version: None, files };
        fs::write(root.join("manifest.json"), serde_json::to_vec(&m).unwrap()).unwrap();
        m
    }

    /// 一次完整的启动，两半都当场跑完（真实启动里第二半在后台线程里跑）。
    fn start(resources: &Path, home: &Path) -> Result<(), String> {
        prepare_at(resources, home)?;
        verify_pending_at(resources, home)
    }

    fn stale_paths(resources: &Path, home: &Path) -> Vec<String> {
        let manifest = read_manifest(resources).unwrap();
        let files = records(&manifest).unwrap();
        let was = remembered(home);
        pending(resources, &files, was.as_ref()).0
    }

    /// 只改内容、不动 size 与 mtime（按回原样的时间戳）：指纹没变，就不该重算哈希。
    /// 顺手断言前提成立——时间戳确实按得回去，否则这个用例什么也证明不了。
    fn tamper_keeping_stamp(path: &Path, filler: u8) {
        let was = stamp(path).expect("被改的文件要有指纹");
        fs::write(path, vec![filler; was.bytes as usize]).unwrap();
        fs::OpenOptions::new().write(true).open(path).unwrap()
            .set_modified(std::time::UNIX_EPOCH + std::time::Duration::from_nanos(was.mtime)).unwrap();
        assert_eq!(stamp(path), Some(was), "size+mtime 必须原样按回去");
    }

    // 同步那一半只 stat 三个必需文件；它的存在本身就是 T07 的证据——全量
    // 指纹校验在 verify_pending_at 里，真实启动里那是另一个线程。
    // 2.1.2 的事故：手写 resources/manifest.json 时漏了 node_version，启动在反序列化
    // 这一步就崩了（「missing field `node_version`」），用户装完完全起不来。node_version
    // 仍是必填的——这条钉住它，将来给 Manifest 加字段也要同步 installer.mjs 的 writeManifest。
    // 2.1.9 起 webview2_version 反过来是**可选**的：旧清单（2.1.8 及以前）写着它，新清单
    // 不再写，两种都必须读得动；写入端没有值就整键省略，不写 `null`。
    #[test] fn a_manifest_missing_node_version_is_rejected_but_webview2_version_is_optional() {
        let partial = r#"{"schema":2,"product":"FISH Dock","version":"2.1.3","architecture":"x86_64","webview2_version":"155.0.4283.62","files":[]}"#;
        let error = serde_json::from_str::<Manifest>(partial).unwrap_err().to_string();
        assert!(error.contains("node_version"), "缺 node_version 必须报出字段名：{error}");
        // 旧清单：带 webview2_version（2.1.8 及以前写的），仍要能解析，只是不再使用。
        let old = r#"{"schema":2,"product":"FISH Dock","version":"2.1.8","architecture":"x86_64","node_version":"24.14.0","webview2_version":"155.0.4283.62","files":[]}"#;
        let old: Manifest = serde_json::from_str(old).expect("旧清单必须能解析");
        assert_eq!(old.webview2_version.as_deref(), Some("155.0.4283.62"));
        // 新清单：2.1.9 起不写这个字段，也必须能解析。
        let new = r#"{"schema":2,"product":"FISH Dock","version":"2.1.9","architecture":"x86_64","node_version":"24.14.0","files":[]}"#;
        let new: Manifest = serde_json::from_str(new).expect("新清单（不含 webview2_version）必须能解析");
        assert_eq!(new.webview2_version, None);
        // 写入端两头都兼容：没有值就整键省略，有值就照写（老包读得动）。
        let written = serde_json::to_value(&new).unwrap();
        assert!(written.get("webview2_version").is_none(), "新清单不许写 webview2_version：{written}");
        let written = serde_json::to_value(&old).unwrap();
        assert_eq!(written["webview2_version"], "155.0.4283.62", "有值就照写，老包读得动");
    }

    // 旧安装留下的清单（带 webview2_version，还列着随包 webview2/ 的文件）和新安装的
    // 清单（不带字段、不列 webview2/）都要能走完一次启动：读清单那侧不依赖这个字段，
    // 也不再要求任何 webview2/ 资源。
    #[test] fn both_the_old_and_the_new_manifest_shape_start() {
        let t = Temp::new(); let root = t.0.join("resources"); fs::create_dir_all(&root).unwrap();
        let home = t.0.join("home");
        let manifest = installed(&root, true, "0.3.3");
        let runtime = b"old fixed runtime";
        fs::create_dir_all(root.join("webview2")).unwrap();
        fs::write(root.join("webview2/msedgewebview2.exe"), runtime).unwrap();
        let mut old = serde_json::to_value(&manifest).unwrap();
        old["webview2_version"] = serde_json::json!("155.0.4283.62");
        old["files"].as_array_mut().unwrap().push(serde_json::json!({
            "path": "webview2/msedgewebview2.exe",
            "bytes": runtime.len(),
            "sha256": format!("{:x}", Sha256::digest(runtime)),
        }));
        fs::write(root.join("manifest.json"), serde_json::to_vec(&old).unwrap()).unwrap();
        start(&root, &home).expect("旧清单（含 webview2_version 与 webview2/ 文件）必须能启动");
        assert_eq!(remembered(&home).unwrap().files.len(), 5, "旧清单里的 webview2 文件照常校验");
        // 新清单：键不存在，也不再列 webview2/ 文件。
        let new = serde_json::to_value(&manifest).unwrap();
        assert!(new.get("webview2_version").is_none());
        fs::write(root.join("manifest.json"), serde_json::to_vec(&new).unwrap()).unwrap();
        start(&root, &home).expect("新清单（不含 webview2_version）必须能启动");
    }

    #[test] fn the_start_checks_the_required_files_and_remembers_a_fingerprint_per_file() {
        let t = Temp::new(); let root = t.0.join("resources"); fs::create_dir_all(&root).unwrap();
        let home = t.0.join("home");
        installed(&root, true, "0.3.3");
        prepare_at(&root, &home).unwrap();
        assert!(!verified_marker(&home).exists(), "同步那一半不写标记");
        verify_pending_at(&root, &home).unwrap();
        let was = remembered(&home).expect("校验通过要记下指纹");
        assert_eq!(was.schema, MARKER_SCHEMA);
        assert_eq!(was.resources, normalized(&root));
        assert_eq!(was.files.len(), 4, "3 个必需文件（node.exe、node/LICENSE、Wakuwaku.txt）+ 一个非必需文件（NOTICE.txt）");
        // 日常启动：指纹一个没变，所以连标记都不重写。
        let before = fs::read(verified_marker(&home)).unwrap();
        prepare_at(&root, &home).unwrap();
        verify_pending_at(&root, &home).unwrap();
        assert_eq!(fs::read(verified_marker(&home)).unwrap(), before, "没变就不该重写标记");
        assert!(stale_paths(&root, &home).is_empty());
        // 必需资源缺失时必须明确提示重装。
        fs::remove_file(root.join("node/node.exe")).unwrap();
        let error = prepare_at(&root, &home).unwrap_err();
        assert!(error.contains("重新安装"), "{error}");
        assert!(error.contains("node/node.exe"), "{error}");
    }

    // T07 的核心：同步那一半不读任何文件内容。清单里的非必需文件被改得跟清单
    // 对不上，prepare_at 照样成功（只有指纹校验那半才看得见），所以第一帧
    // 不会等 10,513 个文件 / 1.30 GB 的遍历。
    #[test] fn the_start_up_path_hashes_nothing_and_a_damaged_file_is_named_in_the_background() {
        let t = Temp::new(); let root = t.0.join("resources"); fs::create_dir_all(&root).unwrap();
        let home = t.0.join("home");
        installed(&root, true, "0.3.3");
        fs::write(root.join("licenses/NOTICE.txt"), "tampered").unwrap();
        prepare_at(&root, &home).expect("同步那一半只看必需文件");
        let error = verify_pending_at(&root, &home).unwrap_err();
        assert!(error.contains("licenses/NOTICE.txt"), "{error}");
        assert!(error.contains("重新安装"), "{error}");
        assert!(!verified_marker(&home).exists(), "校验失败不能记成已验证");
    }

    // T06 的核心两条：改一个已记录文件（size 或 mtime 变了）就必须重校，而且
    // 只重校它；没变的一个都不读。
    #[test] fn one_changed_file_is_verified_again_and_the_unchanged_ones_are_not() {
        let t = Temp::new(); let root = t.0.join("resources"); fs::create_dir_all(&root).unwrap();
        let home = t.0.join("home");
        installed(&root, true, "0.3.3");
        start(&root, &home).unwrap();
        let notice = root.join("licenses/NOTICE.txt");
        // 内容变了、指纹没变：不重校。真重算哈希这里必然报「校验失败」。
        tamper_keeping_stamp(&notice, b'X');
        assert!(stale_paths(&root, &home).is_empty(), "size+mtime 没变就不该排队重校");
        prepare_at(&root, &home).unwrap();
        verify_pending_at(&root, &home).expect("指纹没变的文件不得重算哈希");
        // 只动 mtime（内容与长度都不变）：必须重校，而且排队的只有这一个。
        let was = stamp(&notice).unwrap();
        fs::OpenOptions::new().write(true).open(&notice).unwrap()
            .set_modified(std::time::UNIX_EPOCH + std::time::Duration::from_nanos(was.mtime + 2_000_000_000)).unwrap();
        assert_eq!(stamp(&notice).unwrap().bytes, was.bytes, "长度不变，变的只是 mtime");
        assert_eq!(stale_paths(&root, &home), vec!["licenses/NOTICE.txt".to_string()]);
        // 长度也变了：同样只重校这一个，并且实测点出它。
        fs::write(&notice, "tampered-and-longer").unwrap();
        assert_eq!(stale_paths(&root, &home), vec!["licenses/NOTICE.txt".to_string()]);
        prepare_at(&root, &home).expect("非必需文件不影响同步那一半");
        let error = verify_pending_at(&root, &home).unwrap_err();
        assert!(error.contains("licenses/NOTICE.txt"), "{error}");
        // 修好（重新写一套资源）：再次通过，标记改成新指纹。
        installed(&root, true, "0.3.3");
        verify_pending_at(&root, &home).unwrap();
        assert!(stale_paths(&root, &home).is_empty());
    }

    // T06：清单本身变了（版本号、清单顺序）不再等价于全量重校——判据是文件指纹。
    #[test] fn a_new_manifest_alone_does_not_ask_for_the_tree_again() {
        let t = Temp::new(); let root = t.0.join("resources"); fs::create_dir_all(&root).unwrap();
        let home = t.0.join("home");
        let mut manifest = installed(&root, true, "0.3.3");
        start(&root, &home).unwrap();
        let before = fs::read(verified_marker(&home)).unwrap();
        // 只改清单（版本号与文件顺序），文件一个都不动。
        manifest.version = "0.3.4".into();
        manifest.files.reverse();
        fs::write(root.join("manifest.json"), serde_json::to_vec(&manifest).unwrap()).unwrap();
        assert!(stale_paths(&root, &home).is_empty(), "清单变了不等于文件变了");
        prepare_at(&root, &home).unwrap();
        verify_pending_at(&root, &home).unwrap();
        assert_eq!(fs::read(verified_marker(&home)).unwrap(), before, "指纹没变，标记不必重写");
        // 内容变了而指纹没变，清单的改动也不会让这份文件被重读一遍。
        tamper_keeping_stamp(&root.join("licenses/NOTICE.txt"), b'X');
        verify_pending_at(&root, &home).expect("清单变了不代表文件要重新哈希");
    }

    // T06：旧的两行格式（清单摘要 + 目录）按「没有记录」处理——升级后多校验
    // 一次就自愈，不写迁移脚本。
    #[test] fn the_old_two_line_marker_counts_as_missing() {
        let t = Temp::new(); let root = t.0.join("resources"); fs::create_dir_all(&root).unwrap();
        let home = t.0.join("home");
        installed(&root, true, "0.3.3");
        start(&root, &home).unwrap();
        let marker = verified_marker(&home);
        fs::write(&marker, format!("{}\n{}\n", "a".repeat(64), normalized(&root))).unwrap();
        assert!(remembered(&home).is_none(), "旧格式读不出指纹，按缺失处理");
        // 旧格式 = 没有记录：连「指纹没变」的文件也要重读，所以这里能抓出篡改。
        tamper_keeping_stamp(&root.join("licenses/NOTICE.txt"), b'X');
        let error = verify_pending_at(&root, &home).unwrap_err();
        assert!(error.contains("licenses/NOTICE.txt"), "{error}");
        assert!(remembered(&home).is_none(), "没通过就还是没记录");
        // 修好之后自愈：重建标记，格式是新的。
        installed(&root, true, "0.3.3");
        verify_pending_at(&root, &home).unwrap();
        assert_eq!(remembered(&home).unwrap().schema, MARKER_SCHEMA);
    }

    // R1：state/verified 是「临时文件 + rename」的原子替换（同卷 rename 原子）。写不进去
    // 时（这里是目标文件只读 → 替换被拒）旧记录必须原样留着、仍能解析，临时文件也不许
    // 留下；修好之后同一轮变化照常记下来。以前 truncate+write 撞上中断会留半截 JSON，
    // 下次启动解析失败就把整棵树（10,513 个文件 / 1.30 GB）重校一遍。
    #[test] fn a_failed_write_of_the_marker_keeps_the_old_record() {
        let t = Temp::new(); let root = t.0.join("resources"); fs::create_dir_all(&root).unwrap();
        let home = t.0.join("home");
        installed(&root, true, "0.3.3");
        start(&root, &home).unwrap();
        let marker = verified_marker(&home);
        let before = fs::read(&marker).unwrap();
        // 只动 mtime、内容不动：这一轮会走到「重写标记」，而不是先报校验失败。
        let notice = root.join("licenses/NOTICE.txt");
        let was = stamp(&notice).unwrap();
        fs::OpenOptions::new().write(true).open(&notice).unwrap()
            .set_modified(std::time::UNIX_EPOCH + std::time::Duration::from_nanos(was.mtime + 1_000_000_000)).unwrap();
        // 目标文件只读：Windows 上 rename/MoveFileEx(REPLACE_EXISTING) 会被拒。
        let mut permissions = fs::metadata(&marker).unwrap().permissions();
        permissions.set_readonly(true); fs::set_permissions(&marker, permissions).unwrap();
        let error = verify_pending_at(&root, &home).expect_err("写不进标记就要报错，不能假装记下了");
        assert!(error.contains("无法记录安装资源校验结果"), "{error}");
        assert_eq!(fs::read(&marker).unwrap(), before, "旧记录必须原样留着");
        assert_eq!(remembered(&home).expect("旧记录仍要能解析").schema, MARKER_SCHEMA,
            "旧记录完好，下次启动不会因为半截 JSON 触发整树重校");
        let leftovers: Vec<String> = fs::read_dir(marker.parent().unwrap()).unwrap().flatten()
            .map(|entry| entry.file_name().to_string_lossy().to_string())
            .filter(|name| name.starts_with(".verified-") && name != ".verified.lock").collect();
        assert!(leftovers.is_empty(), "写失败不许留下临时文件：{leftovers:?}");
        // 恢复正常之后，同一轮指纹变化照常记下来。
        let mut permissions = fs::metadata(&marker).unwrap().permissions();
        permissions.set_readonly(false); fs::set_permissions(&marker, permissions).unwrap();
        verify_pending_at(&root, &home).unwrap();
        assert_eq!(remembered(&home).unwrap().files.get("licenses/NOTICE.txt"), Some(&stamp(&notice).unwrap()),
            "修好之后要把新指纹记下来");
    }

    // R3：标记里写的是「判据实际看到的那份指纹」，不是哈希完之后重新 stat 出来的那一份。
    // 以前 remember 会再 stat 一遍：判据之后被改写的内容会被记成已验证，下次启动直接跳过。
    #[test] fn the_marker_records_the_stamps_the_judgement_saw_not_a_later_stat() {
        let t = Temp::new(); let root = t.0.join("resources"); fs::create_dir_all(&root).unwrap();
        let home = t.0.join("home");
        installed(&root, true, "0.3.3");
        start(&root, &home).unwrap();
        let manifest = read_manifest(&root).unwrap();
        let files = records(&manifest).unwrap();
        let (stale, seen) = pending(&root, &files, remembered(&home).as_ref());
        assert!(stale.is_empty());
        let observed = seen.get("licenses/NOTICE.txt").cloned().expect("判据要看到这个文件的指纹");
        // 判据之后文件又被改写（模拟「哈希与记指纹之间」那个窗口），然后照旧记标记。
        let notice = root.join("licenses/NOTICE.txt");
        fs::write(&notice, "changed after the judgement, longer").unwrap();
        remember(&home, &root, &seen).unwrap();
        assert_eq!(remembered(&home).unwrap().files.get("licenses/NOTICE.txt"), Some(&observed),
            "记下的必须是判据那一份指纹，不能是判据之后 stat 出来的");
        assert_eq!(stale_paths(&root, &home), vec!["licenses/NOTICE.txt".to_string()],
            "没校验过的新内容必须下一轮重校");
    }

    // R2：后台那半不再自己推导目录（目录由 prepare() 交过来）；万一拿到的是空目录，
    // 必须能说出一句人话（verify_in_background 照这句打日志），不许无声跳过。
    #[test] fn the_background_half_is_told_where_and_says_so_when_it_is_not() {
        let ok = Prepared { home: PathBuf::from(r"C:\Users\me\AppData\Local\FISH Dock"), resources: PathBuf::from(r"C:\Program Files\FISH Dock\resources") };
        assert!(prepared_directories(&ok).is_ok());
        let error = prepared_directories(&Prepared { home: PathBuf::new(), resources: ok.resources.clone() }).unwrap_err();
        assert!(error.contains("跳过全量校验"), "{error}");
        let error = prepared_directories(&Prepared { home: ok.home.clone(), resources: PathBuf::new() }).unwrap_err();
        assert!(error.contains("跳过全量校验"), "{error}");
    }

    #[test] fn new_install_resources_are_verified_again_and_the_persons_data_survives() {
        let t = Temp::new(); let root = t.0.join("resources"); fs::create_dir_all(&root).unwrap();
        let home = t.0.join("home");
        installed(&root, true, "0.3.3");
        start(&root, &home).unwrap();
        fs::create_dir_all(home.join("data")).unwrap();
        fs::write(home.join("data/settings.json"), "mine").unwrap();
        fs::write(home.join("account.json"), "keep").unwrap();
        // 换了安装资源（版本与时间戳都不同）：指纹变了就重验一次，用户数据原地不动。
        installed(&root, true, "0.3.4");
        fs::remove_file(root.join("licenses/NOTICE.txt")).unwrap();
        let error = start(&root, &home).unwrap_err();
        assert!(error.contains("NOTICE.txt"), "{error}");
        assert_eq!(fs::read_to_string(home.join("data/settings.json")).unwrap(), "mine");
        assert_eq!(fs::read_to_string(home.join("account.json")).unwrap(), "keep");
    }

    #[test] fn missing_install_resources_ask_for_a_reinstall_instead_of_a_fall_back() {
        let t = Temp::new(); let root = t.0.join("missing"); let home = t.0.join("home");
        let error = prepare_at(&root, &home).unwrap_err();
        assert!(error.contains("重新安装"), "{error}");
        assert!(error.contains("manifest.json"), "{error}");
    }

    #[test] fn an_unsafe_or_foreign_manifest_is_refused() {
        let t = Temp::new(); let root = t.0.join("resources"); fs::create_dir_all(&root).unwrap();
        let home = t.0.join("home");
        let mut m = installed(&root, true, "0.3.3");
        m.files.push(FileRecord { path: "../escape".into(), bytes: 1, sha256: "0".repeat(64) });
        fs::write(root.join("manifest.json"), serde_json::to_vec(&m).unwrap()).unwrap();
        assert!(prepare_at(&root, &home).is_err());
        let mut m = installed(&root, true, "0.3.3");
        m.schema = 1;
        fs::write(root.join("manifest.json"), serde_json::to_vec(&m).unwrap()).unwrap();
        assert!(prepare_at(&root, &home).unwrap_err().contains("重新安装"));
    }

    #[test] fn traversal_and_windows_aliases_are_rejected() {
        for s in ["/outside", "../escape", "a/../escape", "C:/escape", "C:escape", "\\\\server\\file", "a\\escape", "a//b", "./a", "NUL", "a/COM1.txt", "a/name.", "a/name ", "a:stream", ""] {
            assert!(safe_relative(s).is_err(), "accepted unsafe path: {s:?}");
        }
        assert!(safe_relative("node/licenses/LICENSE.txt").is_ok());
    }

    #[test] fn blocked_home_directory_is_reported_without_touching_it() {
        let t = Temp::new(); let root = t.0.join("resources"); fs::create_dir_all(&root).unwrap();
        installed(&root, true, "0.3.3");
        let blocked = t.0.join("blocked"); fs::write(&blocked, "keep").unwrap();
        assert!(prepare_at(&root, &blocked).is_err());
        assert_eq!(fs::read_to_string(blocked).unwrap(), "keep");
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
    // 只对这一套目录成立，所以按新目录重验一次；同目录的日常启动仍只 stat
    // 必需文件，不重写标记。
    #[test]
    fn a_different_resources_directory_is_verified_again() {
        let t = Temp::new();
        let (a, b) = (t.0.join("a/resources"), t.0.join("b/resources"));
        for root in [&a, &b] { fs::create_dir_all(root).unwrap(); installed(root, true, "0.3.3"); }
        let home = t.0.join("home");
        prepare_at(&a, &home).unwrap();
        verify_pending_at(&a, &home).unwrap();
        // 同一套目录再来一次：不重验、不重写标记。
        prepare_at(&a, &home).unwrap();
        verify_pending_at(&a, &home).unwrap();
        // 另一套目录：整套按新目录重验（非必需文件被改动也点名）。
        fs::write(b.join("licenses/NOTICE.txt"), "tampered").unwrap();
        prepare_at(&b, &home).unwrap();
        let error = verify_pending_at(&b, &home).unwrap_err();
        assert!(error.contains("licenses/NOTICE.txt"), "{error}");
        assert!(error.contains("重新安装"), "{error}");
        assert_eq!(remembered(&home).unwrap().resources, normalized(&a), "没通过就不能改记新目录");
        // 修好之后：通过，标记改记新目录。
        installed(&b, true, "0.3.3");
        prepare_at(&b, &home).unwrap();
        verify_pending_at(&b, &home).unwrap();
        assert_eq!(remembered(&home).unwrap().resources, normalized(&b));
    }

    #[test]
    fn the_installed_resources_are_used_in_place() {
        let t = Temp::new(); let root = t.0.join("resources"); fs::create_dir_all(&root).unwrap();
        let home = t.0.join("home");
        installed(&root, true, "0.3.3");
        prepare_at(&root, &home).unwrap();
        assert!(root.join(REQUIRED[0]).is_file());
        // 2.1.9 起固定版 WebView2 不再是必需资源：三个必需文件里没有它。
        assert_eq!(REQUIRED, ["node/node.exe", "node/LICENSE", "licenses/Wakuwaku.txt"]);
        assert!(!REQUIRED.iter().any(|name| name.starts_with("webview2/")));
        // 安装目录里不再有随包的 WebView2，用户数据目录仍在。
        assert!(!root.join("webview2").exists(), "不再随包内置 WebView2");
        assert_eq!(webview_directory(&home), home.join("webview"));
        assert!(home.join("webview").is_dir());
    }

    // 2.0.0 的数据根改名：安装器会删旧目录 %LOCALAPPDATA%\DSH-FISH，宿主必须
    // 已经在用 %LOCALAPPDATA%\FISH Dock 这套活数据，否则被删的就是正在用的。
    #[test]
    fn app_home_default_is_fish_dock_under_local_app_data() {
        let local = std::ffi::OsString::from(r"C:\Users\me\AppData\Local");
        let home = app_home_from(None, Some(local)).unwrap();
        assert_eq!(home, PathBuf::from(r"C:\Users\me\AppData\Local").join("FISH Dock"));
        assert_eq!(home.file_name().and_then(|name| name.to_str()), Some("FISH Dock"));
        let text = home.to_string_lossy();
        assert!(text.ends_with("FISH Dock"), "{text}");
        assert!(!text.contains("DSH-FISH"), "数据根不能再落到旧名上：{text}");
        // 没有 LOCALAPPDATA 就该报错，而不是猜一个目录。
        assert!(app_home_from(None, None).is_err());
    }

    // 排障与测试用：FISH_DOCK_APP_HOME 覆盖数据根（绝对路径）。FISH_DOCK_HOME
    // 是统一数据根的覆盖名，宿主读 app_home() 时只用本变量，不借用它。
    #[test]
    fn app_home_takes_the_fish_dock_app_home_override_from_the_environment() {
        let _serial = crate::config::ENV_LOCK.lock().unwrap_or_else(|poison| poison.into_inner());
        let previous = std::env::var_os("FISH_DOCK_APP_HOME");
        let target = std::env::temp_dir().join("fish-dock-app-home-override");
        std::env::set_var("FISH_DOCK_APP_HOME", &target);
        let home = app_home();
        match &previous {
            Some(value) => std::env::set_var("FISH_DOCK_APP_HOME", value),
            None => std::env::remove_var("FISH_DOCK_APP_HOME"),
        }
        assert_eq!(home.unwrap(), target);
        // 覆盖变量只影响 app_home()：默认根的名字仍是 %LOCALAPPDATA%\FISH Dock。
        assert_eq!(app_home_from(None, Some(std::ffi::OsString::from(r"C:\Users\me\AppData\Local"))).unwrap(),
                   PathBuf::from(r"C:\Users\me\AppData\Local").join("FISH Dock"));
    }

    // 空串与相对路径按「没设」处理：排障时手滑写出这两种值，宁可按默认走，
    // 也不要把数据根指到一个相对于当前目录的半截路径上。
    #[test]
    fn app_home_ignores_an_empty_or_relative_override() {
        let local = || std::ffi::OsString::from(r"C:\Users\me\AppData\Local");
        let default = PathBuf::from(r"C:\Users\me\AppData\Local").join("FISH Dock");
        assert_eq!(app_home_from(Some(std::ffi::OsString::from("")), Some(local())).unwrap(), default);
        assert_eq!(app_home_from(Some(std::ffi::OsString::from(r"relative\fish-dock")), Some(local())).unwrap(), default);
        assert_eq!(app_home_from(Some(std::ffi::OsString::from(".")), Some(local())).unwrap(), default);
    }
}
