//! 统一身份目录解析与仅复制迁移；不删除旧目录，也不合并已有目标。
use std::{
    ffi::OsString,
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};
use sha2::{Digest, Sha256};

#[derive(Clone, Debug, PartialEq)]
pub struct Location {
    pub root: PathBuf,
    pub legacy: Option<PathBuf>,
    pub warning: Option<&'static str>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Prepared {
    Existing,
    Created,
    Migrated { files: usize, bytes: u64 },
}

pub fn resolve(home: &Path, new: Option<OsString>, old: Option<OsString>) -> Result<Location, String> {
    if !home.is_absolute() { return Err("用户目录不是绝对路径".into()); }
    let new = nonempty(new);
    let old = nonempty(old);
    if let Some(path) = new {
        let path = absolute(path)?;
        return Ok(Location {
            root: path,
            legacy: None,
            warning: old.map(|_| "FISH_DOCK_DATA 优先于旧 DSH_FISH_DATA"),
        });
    }
    if let Some(path) = old {
        return Ok(Location {
            root: absolute(path)?,
            legacy: None,
            warning: Some("DSH_FISH_DATA 已弃用；请迁移至 .fish-dock"),
        });
    }
    Ok(Location {
        root: home.join(".fish-dock"),
        legacy: Some(home.join(".dsh-fish")),
        warning: None,
    })
}

fn nonempty(path: Option<OsString>) -> Option<OsString> {
    path.filter(|value| !value.is_empty())
}

fn absolute(path: OsString) -> Result<PathBuf, String> {
    let path = PathBuf::from(path);
    if path.is_absolute() { Ok(path) } else { Err("身份目录覆盖值必须是绝对路径".into()) }
}

fn check_ancestors(path: &Path) -> Result<(), String> {
    let mut current = Some(path);
    while let Some(item) = current {
        if let Some(meta) = symlink_metadata_if_exists(item)? {
            if linked(&meta) { return Err("身份目录路径包含符号链接或目录联接".into()); }
        }
        current = item.parent();
    }
    Ok(())
}

#[cfg(windows)]
fn canonical_for_guard(path: &Path) -> Result<String, String> {
    let value = fs::canonicalize(path).map_err(|_| "无法规范化 OFM 数据目录，未迁移")?.to_string_lossy().into_owned();
    let value = value.strip_prefix(r"\\?\UNC\").map(|tail| format!(r"\\{tail}"))
        .or_else(|| value.strip_prefix(r"\\?\").map(str::to_owned)).unwrap_or(value);
    Ok(value.to_lowercase())
}

#[cfg(not(windows))]
fn canonical_for_guard(path: &Path) -> Result<PathBuf, String> {
    fs::canonicalize(path).map_err(|_| "无法规范化 OFM 数据目录，未迁移".into())
}

pub fn from_environment() -> Result<Location, String> {
    let home = std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
        .ok_or("找不到用户主目录")?;
    resolve(&home, std::env::var_os("FISH_DOCK_DATA"), std::env::var_os("DSH_FISH_DATA"))
}

/// Create the canonical root or copy the legacy default into a sibling staging
/// directory, verifying every file before one final rename. The source remains intact.
pub fn prepare(location: &Location) -> Result<Prepared, String> {
    prepare_with(location, &private_directory, &service_guard)
}

pub(crate) fn operation_guard(root:&Path)->Result<Box<dyn std::any::Any>,String>{
    check_ancestors(root)?;
    let guard=service_guard(root)?;
    if service_lock_live_or_unknown(&root.join("service.lock")){return Err("身份目录仍被 OFM 服务占用，请退出独立 OFM 后重试".into())}
    Ok(guard)
}

pub(crate) fn protect_file(path:&Path)->Result<(),String>{
    check_ancestors(path)?;
    #[cfg(windows)]{
        let sid=crate::logon::user_sid().ok_or("无法确认当前 Windows 用户")?;
        set_private_dacl(path,&format!("D:PAI(A;;FA;;;SY)(A;;FA;;;BA)(A;;FA;;;{sid})"))
    }
    #[cfg(not(windows))]{
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path,fs::Permissions::from_mode(0o600)).map_err(|_|"无法限制导出文件权限".into())
    }
}

fn prepare_with(
    location: &Location,
    make_private: &dyn Fn(&Path) -> Result<(), String>,
    guard: &dyn Fn(&Path) -> Result<Box<dyn std::any::Any>, String>,
) -> Result<Prepared, String> {
    if !location.root.is_absolute() { return Err("身份目录必须是绝对路径".into()); }
    check_ancestors(&location.root)?;
    if let Some(meta) = symlink_metadata_if_exists(&location.root)? {
        if linked(&meta) || !meta.is_dir() { return Err("身份目录不是安全的普通目录".into()); }
        make_private(&location.root)?;
        return Ok(Prepared::Existing);
    }
    let Some(source) = location.legacy.as_deref().filter(|source| source.exists()) else {
        create_parent(&location.root)?;
        make_private(&location.root)?;
        return Ok(Prepared::Created);
    };
    if source == location.root { return Err("旧目录与新目录不能相同".into()); }
    if let Some(meta) = symlink_metadata_if_exists(source)? {
        if linked(&meta) || !meta.is_dir() { return Err("旧身份目录不是安全的普通目录".into()); }
    }
    check_ancestors(source)?;
    let _source_guard = guard(source)?;
    if service_lock_live_or_unknown(&source.join("service.lock")) {
        return Err("旧 OFM 服务锁仍被占用或归属不明；请正常退出服务后重试".into());
    }
    create_parent(&location.root)?;
    let stage = unique_stage(&location.root);
    make_private(&stage)?;
    let result = copy_tree_verified(source, &stage);
    let (files, bytes) = match result {
        Ok(result) => result,
        Err(error) => return Err(format!("旧数据迁移未提交，旧目录保持不变；暂存副本保留供恢复：{error}")),
    };
    if service_lock_live_or_unknown(&source.join("service.lock")) {
        return Err("迁移期间旧 OFM 服务状态发生变化；暂存副本已保留，请确认退出后重试".into());
    }
    fs::rename(&stage, &location.root)
        .map_err(|_| "无法原子提交身份迁移；旧目录保持不变，暂存副本已保留".to_string())?;
    Ok(Prepared::Migrated { files, bytes })
}

fn linked(meta: &fs::Metadata) -> bool {
    if meta.file_type().is_symlink() { return true; }
    #[cfg(windows)]
    { use std::os::windows::fs::MetadataExt; return meta.file_attributes() & 0x400 != 0; }
    #[cfg(not(windows))]
    { false }
}

fn create_parent(root: &Path) -> Result<(), String> {
    let parent = root.parent().ok_or("身份目录缺少父目录")?;
    fs::create_dir_all(parent).map_err(|_| "无法准备身份目录，请检查访问权限".to_string())
}

fn private_directory(path: &Path) -> Result<(), String> {
    #[cfg(windows)]
    {
        let sid = crate::logon::user_sid().ok_or("无法确认当前 Windows 用户 SID，未创建身份目录")?;
        let sddl = format!("D:PAI(A;OICI;FA;;;SY)(A;OICI;FA;;;BA)(A;OICI;FA;;;{sid})");
        if !path.exists() {
            match create_private_directory(path, &sddl) {
                Ok(()) => return Ok(()),
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(error) => return Err(format!("无法创建当前用户专属身份目录（Windows 错误码 {:?}）", error.raw_os_error())),
            }
        }
        set_private_dacl(path, &sddl)?;
        for relative in [
            "settings.json", "channel-credentials.json", "eac-user.json", "mail-accounts.json", "mail-secrets.dpapi",
            "channel-pack/state.json", "channel-pack/token-ledger.json", "channel-pack/permanent-locks.json",
            "channel-pack/dead-models.json", "channel-pack/ui-preferences.json", "channel-pack/auto-checkin.json", "channel-pack/gemini-sigs.json",
            "stats.json", "availability.json", "catalog.json",
        ] {
            let file = path.join(relative);
            match fs::symlink_metadata(&file) {
                Ok(meta) => {
                    if linked(&meta) || !meta.is_file() { return Err("身份目录中的关键配置不是普通文件".into()); }
                    set_private_dacl(&file, &sddl)?;
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(_) => return Err("无法检查身份目录中的关键配置权限".into()),
            }
        }
        Ok(())
    }
    #[cfg(not(windows))]
    {
        fs::create_dir_all(path).map_err(|_| "无法创建身份目录".to_string())?;
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700)).map_err(|_| "无法限制身份目录权限".to_string())
    }
}

#[cfg(windows)]
fn create_private_directory(path: &Path, sddl: &str) -> std::io::Result<()> {
    use std::{ffi::c_void, os::windows::ffi::OsStrExt};
    #[repr(C)]
    struct SecurityAttributes { size: u32, descriptor: *mut c_void, inherit: i32 }
    #[link(name = "advapi32")]
    unsafe extern "system" {
        fn ConvertStringSecurityDescriptorToSecurityDescriptorW(text: *const u16, revision: u32, descriptor: *mut *mut c_void, size: *mut u32) -> i32;
    }
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn CreateDirectoryW(path: *const u16, attributes: *const SecurityAttributes) -> i32;
        fn LocalFree(memory: *mut c_void) -> *mut c_void;
    }
    let path: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    let sddl: Vec<u16> = sddl.encode_utf16().chain(Some(0)).collect();
    // Windows 分配安全描述符；仅在建目录期间使用，之后释放。
    unsafe {
        let mut descriptor = std::ptr::null_mut();
        if ConvertStringSecurityDescriptorToSecurityDescriptorW(sddl.as_ptr(), 1, &mut descriptor, std::ptr::null_mut()) == 0 {
            return Err(std::io::Error::last_os_error());
        }
        let attributes = SecurityAttributes { size: std::mem::size_of::<SecurityAttributes>() as u32, descriptor, inherit: 0 };
        let made = CreateDirectoryW(path.as_ptr(), &attributes) != 0;
        let error = std::io::Error::last_os_error();
        LocalFree(descriptor);
        if made { Ok(()) } else { Err(error) }
    }
}

#[cfg(windows)]
fn set_private_dacl(path: &Path, sddl: &str) -> Result<(), String> {
    use std::{ffi::c_void, os::windows::ffi::OsStrExt};
    #[link(name = "advapi32")]
    unsafe extern "system" {
        fn ConvertStringSecurityDescriptorToSecurityDescriptorW(text: *const u16, revision: u32, descriptor: *mut *mut c_void, size: *mut u32) -> i32;
        fn GetSecurityDescriptorDacl(descriptor: *mut c_void, present: *mut i32, dacl: *mut *mut c_void, defaulted: *mut i32) -> i32;
        fn SetNamedSecurityInfoW(name: *const u16, kind: u32, info: u32, owner: *mut c_void, group: *mut c_void, dacl: *mut c_void, sacl: *mut c_void) -> u32;
    }
    #[link(name = "kernel32")]
    unsafe extern "system" { fn LocalFree(memory: *mut c_void) -> *mut c_void; }
    let wide_path: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    let wide_sddl: Vec<u16> = sddl.encode_utf16().chain(Some(0)).collect();
    unsafe {
        let mut descriptor = std::ptr::null_mut();
        if ConvertStringSecurityDescriptorToSecurityDescriptorW(wide_sddl.as_ptr(), 1, &mut descriptor, std::ptr::null_mut()) == 0 {
            return Err("无法构造身份目录 ACL".into());
        }
        let (mut present, mut defaulted, mut dacl) = (0, 0, std::ptr::null_mut());
        let got = GetSecurityDescriptorDacl(descriptor, &mut present, &mut dacl, &mut defaulted) != 0 && present != 0;
        let result = if got { SetNamedSecurityInfoW(wide_path.as_ptr(), 1, 0x4 | 0x8000_0000, std::ptr::null_mut(), std::ptr::null_mut(), dacl, std::ptr::null_mut()) } else { 87 };
        LocalFree(descriptor);
        if result == 0 { Ok(()) } else { Err("无法设置当前用户专属身份目录 ACL".into()) }
    }
}

#[cfg(windows)]
struct ServiceGuard(isize);
#[cfg(windows)]
impl Drop for ServiceGuard { fn drop(&mut self) { unsafe { CloseHandle(self.0); } } }

#[cfg(windows)]
fn service_guard(path: &Path) -> Result<Box<dyn std::any::Any>, String> {
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn CreateNamedPipeW(name: *const u16, open_mode: u32, pipe_mode: u32, instances: u32, out_size: u32, in_size: u32, timeout: u32, attrs: *mut std::ffi::c_void) -> isize;
        fn CloseHandle(handle: isize) -> i32;
    }
    let canonical = canonical_for_guard(path)?;
    let digest = Sha256::digest(canonical.as_bytes());
    let suffix: String = digest.iter().map(|byte| format!("{byte:02x}")).collect();
    let name: Vec<u16> = format!(r"\\.\pipe\ofm-service-lock-{suffix}").encode_utf16().chain(Some(0)).collect();
    let handle = unsafe { CreateNamedPipeW(name.as_ptr(), 0x3 | 0x0008_0000, 0, 1, 4096, 4096, 0, std::ptr::null_mut()) };
    if handle == -1 { return Err("旧 OFM 数据目录正在启动或被服务占用；请正常退出后重试".into()); }
    Ok(Box::new(ServiceGuard(handle)))
}

#[cfg(unix)]
struct ServiceGuard(std::os::fd::RawFd);
#[cfg(unix)]
impl Drop for ServiceGuard { fn drop(&mut self) { unsafe { close(self.0); } } }

#[cfg(target_os = "linux")]
fn service_guard(path: &Path) -> Result<Box<dyn std::any::Any>, String> {
    use std::{os::unix::ffi::OsStrExt, os::fd::RawFd};
    #[repr(C)] struct SockAddrUn { family: u16, path: [i8; 108] }
    unsafe extern "C" { fn socket(domain: i32, kind: i32, protocol: i32) -> RawFd; fn bind(fd: i32, addr: *const SockAddrUn, len: u32) -> i32; fn close(fd: i32) -> i32; }
    let canonical = canonical_for_guard(path)?;
    let digest = Sha256::digest(canonical.as_os_str().as_bytes());
    let suffix: String = digest.iter().map(|byte| format!("{byte:02x}")).collect();
    let name = format!("ofm-service-lock-{suffix}");
    if name.len() + 1 > 108 { return Err("无法取得 OFM 服务互斥".into()); }
    let fd = unsafe { socket(1, 1 | 0x80000, 0) };
    if fd < 0 { return Err("无法取得 OFM 服务互斥".into()); }
    let mut addr = SockAddrUn { family: 1, path: [0; 108] };
    for (to, from) in addr.path[1..].iter_mut().zip(name.bytes()) { *to = from as i8; }
    if unsafe { bind(fd, &addr, (2 + 1 + name.len()) as u32) } != 0 {
        unsafe { close(fd); }
        return Err("旧 OFM 数据目录正在启动或被服务占用；请正常退出后重试".into());
    }
    Ok(Box::new(ServiceGuard(fd)))
}

#[cfg(all(unix, not(target_os = "linux")))]
fn service_guard(_path: &Path) -> Result<Box<dyn std::any::Any>, String> {
    Err("此系统无法确认 OFM 服务互斥；旧目录保持不变".into())
}

#[cfg(not(any(windows, unix)))]
fn service_guard(_path: &Path) -> Result<Box<dyn std::any::Any>, String> {
    Err("此系统无法确认 OFM 服务互斥；旧目录保持不变".into())
}

#[cfg(windows)]
#[link(name = "kernel32")]
unsafe extern "system" { fn CloseHandle(handle: isize) -> i32; }
#[cfg(unix)]
unsafe extern "C" { fn close(fd: i32) -> i32; }

fn service_lock_live_or_unknown(path: &Path) -> bool {
    let meta = match fs::symlink_metadata(path) {
        Ok(meta) => meta,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return false,
        Err(_) => return true,
    };
    if meta.file_type().is_symlink() || !meta.is_file() || meta.len() > 16 * 1024 { return true; }
    let text = match fs::read(path) { Ok(text) => text, Err(_) => return true };
    let Ok(owner) = serde_json::from_slice::<serde_json::Value>(&text) else { return true };
    if owner.get("product").and_then(serde_json::Value::as_str) != Some("our-free-model-standalone") { return true; }
    let Some(pid) = owner.get("pid").and_then(serde_json::Value::as_u64).filter(|pid| *pid > 0 && *pid <= u32::MAX as u64) else { return true };
    process_alive(pid as u32) != Some(false)
}

#[cfg(windows)]
fn process_alive(pid: u32) -> Option<bool> {
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn OpenProcess(access: u32, inherit: i32, pid: u32) -> isize;
        fn GetExitCodeProcess(process: isize, code: *mut u32) -> i32;
        fn CloseHandle(handle: isize) -> i32;
    }
    let handle = unsafe { OpenProcess(0x1000, 0, pid) };
    if handle == 0 {
        return match std::io::Error::last_os_error().raw_os_error() {
            Some(87) => Some(false),
            _ => None,
        };
    }
    let mut code = 0;
    let ok = unsafe { GetExitCodeProcess(handle, &mut code) } != 0;
    unsafe { CloseHandle(handle); }
    if !ok { return None; }
    Some(code == 259)
}

#[cfg(unix)]
fn process_alive(pid: u32) -> Option<bool> {
    unsafe extern "C" { fn kill(pid: i32, signal: i32) -> i32; }
    if pid > i32::MAX as u32 { return Some(false); }
    let result = unsafe { kill(pid as i32, 0) };
    if result == 0 { return Some(true); }
    match std::io::Error::last_os_error().raw_os_error() {
        Some(3) => Some(false),
        Some(1) => Some(true),
        _ => None,
    }
}

#[cfg(not(any(windows, unix)))]
fn process_alive(_pid: u32) -> Option<bool> { None }

fn symlink_metadata_if_exists(path: &Path) -> Result<Option<fs::Metadata>, String> {
    match fs::symlink_metadata(path) {
        Ok(meta) => Ok(Some(meta)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(_) => Err("无法检查身份目录".into()),
    }
}

fn unique_stage(root: &Path) -> PathBuf {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let stamp = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_nanos();
    let serial = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    root.with_file_name(format!(".fish-dock-migration-{}-{stamp}-{serial}", std::process::id()))
}

fn copy_tree_verified(source: &Path, stage: &Path) -> Result<(usize, u64), String> {
    const FILES: &[&str] = &[
        "settings.json", "stats.json", "availability.json", "catalog.json", "channel-credentials.json", "eac-user.json",
        "channel-pack/state.json", "channel-pack/token-ledger.json", "channel-pack/permanent-locks.json",
        "channel-pack/dead-models.json", "channel-pack/ui-preferences.json", "channel-pack/auto-checkin.json", "channel-pack/gemini-sigs.json",
    ];
    let mut count = 0usize;
    let mut total = 0u64;
    for relative in FILES {
        let input = source.join(relative);
        check_ancestors(&input)?;
        let metadata = match fs::symlink_metadata(&input) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(_) => return Err("无法检查旧身份文件".into()),
        };
        if linked(&metadata) || !metadata.is_file() { return Err("旧身份文件不是安全的普通文件".into()); }
        if count >= 50_000 || metadata.len() > 512 * 1024 * 1024 || total > 2 * 1024 * 1024 * 1024 - metadata.len() {
            return Err("旧身份数据超过安全迁移上限".into());
        }
        let output = stage.join(relative);
        if let Some(parent) = output.parent() {
            fs::create_dir_all(parent).map_err(|_| "无法准备迁移子目录")?;
        }
        copy_file_verified(&input, &output, metadata.len())?;
        total += metadata.len();
        count += 1;
    }
    Ok((count, total))
}

fn copy_file_verified(input: &Path, output: &Path, expected_len: u64) -> Result<(), String> {
    let mut input_file = fs::File::open(input).map_err(|_| "无法读取旧身份文件")?;
    let mut output_file = fs::OpenOptions::new().write(true).create_new(true).open(output)
        .map_err(|_| "无法写入迁移暂存文件")?;
    let mut source_hash = Sha256::new();
    let mut copied = 0u64;
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let read = input_file.read(&mut buffer).map_err(|_| "读取旧身份文件失败")?;
        if read == 0 { break; }
        copied = copied.checked_add(read as u64).ok_or("旧身份文件大小溢出")?;
        source_hash.update(&buffer[..read]);
        output_file.write_all(&buffer[..read]).map_err(|_| "写入迁移暂存文件失败")?;
    }
    output_file.sync_all().map_err(|_| "无法校验迁移暂存文件")?;
    drop(output_file);
    if copied != expected_len { return Err("旧身份文件在迁移时发生变化".into()); }
    let source_digest = source_hash.finalize().to_vec();
    let (staged_digest, staged_bytes) = hash_file(output)?;
    if staged_bytes != copied || staged_digest != source_digest { return Err("迁移文件校验失败".into()); }
    let (after_digest, after_bytes) = hash_file(input)?;
    if after_bytes != copied || after_digest != source_digest { return Err("旧身份文件在迁移时发生变化".into()); }
    Ok(())
}

fn hash_file(path: &Path) -> Result<(Vec<u8>, u64), String> {
    let mut file = fs::File::open(path).map_err(|_| "无法复核旧身份文件")?;
    let mut hash = Sha256::new();
    let mut bytes = 0u64;
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer).map_err(|_| "无法复核旧身份文件")?;
        if read == 0 { break; }
        bytes = bytes.checked_add(read as u64).ok_or("旧身份文件大小溢出")?;
        hash.update(&buffer[..read]);
    }
    Ok((hash.finalize().to_vec(), bytes))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn prepare_fixture(location: &Location) -> Result<Prepared, String> {
        let make_private = |path: &Path| {
            if path.is_dir() { Ok(()) }
            else { fs::create_dir(path).map_err(|_| "fixture mkdir failed".to_string()) }
        };
        let guard = |_path: &Path| Ok(Box::new(()) as Box<dyn std::any::Any>);
        prepare_with(location, &make_private, &guard)
    }

    fn fixture() -> PathBuf {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let id = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        std::env::temp_dir().join(format!("fish-dock-identity-test-{}-{id}", std::process::id()))
    }

    #[test]
    fn default_is_fish_dock_not_other_products() {
        let h = fixture();
        let location = resolve(&h, None, None).unwrap();
        assert_eq!(location.root, h.join(".fish-dock"));
        assert_eq!(location.legacy, Some(h.join(".dsh-fish")));
    }

    #[test]
    fn explicit_new_wins_without_merging_legacy() {
        let h = fixture();
        let new = h.join("chosen");
        let location = resolve(&h, Some(new.clone().into()), Some(h.join("old").into())).unwrap();
        assert_eq!(location.root, new);
        assert!(location.legacy.is_none());
        assert!(location.warning.is_some());
    }

    #[test]
    fn old_override_is_supported_but_warned() {
        let h = fixture();
        let old = h.join("old-override");
        let location = resolve(&h, Some(OsString::new()), Some(old.clone().into())).unwrap();
        assert_eq!(location.root, old);
        assert!(location.legacy.is_none());
        assert!(location.warning.is_some());
    }

    #[test]
    fn relative_override_is_an_error_not_fallback() {
        assert!(resolve(&fixture(), Some("relative".into()), None).is_err());
        assert!(resolve(&fixture(), None, Some("relative".into())).is_err());
    }

    #[test]
    fn migration_copies_and_verifies_legacy_without_removing_it() {
        let h = fixture();
        fs::create_dir_all(h.join(".dsh-fish/channel-pack")).unwrap();
        fs::write(h.join(".dsh-fish/channel-pack/token-ledger.json"), b"ledger").unwrap();
        fs::write(h.join(".dsh-fish/stats.json"), b"stats").unwrap();
        fs::write(h.join(".dsh-fish/service.lock"), br#"{"product":"our-free-model-standalone","pid":4294967295}"#).unwrap();
        let location = resolve(&h, None, None).unwrap();
        assert_eq!(prepare_fixture(&location), Ok(Prepared::Migrated { files: 2, bytes: 11 }));
        assert_eq!(fs::read(h.join(".fish-dock/channel-pack/token-ledger.json")).unwrap(), b"ledger");
        assert_eq!(fs::read(h.join(".fish-dock/stats.json")).unwrap(), b"stats");
        assert!(h.join(".dsh-fish/stats.json").exists());
        assert!(!h.join(".fish-dock/service.lock").exists());
        fs::remove_dir_all(h).unwrap();
    }

    #[test]
    fn existing_destination_is_never_merged_or_overwritten() {
        let h = fixture();
        fs::create_dir_all(h.join(".dsh-fish")).unwrap();
        fs::create_dir_all(h.join(".fish-dock")).unwrap();
        fs::write(h.join(".dsh-fish/old.json"), b"old").unwrap();
        fs::write(h.join(".fish-dock/current.json"), b"current").unwrap();
        let location = resolve(&h, None, None).unwrap();
        assert_eq!(prepare_fixture(&location), Ok(Prepared::Existing));
        assert!(!h.join(".fish-dock/old.json").exists());
        assert_eq!(fs::read(h.join(".fish-dock/current.json")).unwrap(), b"current");
        fs::remove_dir_all(h).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn symlink_source_is_rejected_without_copying_external_file() {
        let h = fixture();
        fs::create_dir_all(h.join(".dsh-fish")).unwrap();
        fs::write(h.join("outside"), b"secret").unwrap();
        std::os::unix::fs::symlink(h.join("outside"), h.join(".dsh-fish/linked")).unwrap();
        let location = resolve(&h, None, None).unwrap();
        assert!(prepare_fixture(&location).unwrap_err().contains("链接"));
        assert!(!location.root.exists());
        fs::remove_dir_all(h).unwrap();
    }

    #[test]
    fn live_or_ambiguous_service_lock_blocks_migration() {
        let h = fixture();
        fs::create_dir_all(h.join(".dsh-fish")).unwrap();
        fs::write(h.join(".dsh-fish/service.lock"), b"locked").unwrap();
        let location = resolve(&h, None, None).unwrap();
        assert!(prepare_fixture(&location).unwrap_err().contains("服务锁"));
        assert!(!location.root.exists());
        fs::remove_dir_all(h).unwrap();
    }
}
