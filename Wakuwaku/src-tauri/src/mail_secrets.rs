//! 邮件密码使用当前用户作用域 DPAPI，与 DSH 和桌面配置隔离。
use std::{collections::BTreeMap, fs, io::Write, path::{Path, PathBuf}, sync::Mutex};

use base64::{engine::general_purpose::STANDARD, Engine as _};
use serde::{Deserialize, Serialize};

const FILE: &str = "mail-secrets.dpapi";
const MAX_FILE: u64 = 1024 * 1024;
const MAX_SECRET: usize = 16 * 1024;
static EDIT: Mutex<()> = Mutex::new(());

#[derive(Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Vault {
    version: u32,
    entries: BTreeMap<String, String>,
}

pub fn valid_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 200 && id.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'.'))
}

fn path(root: &Path) -> PathBuf { root.join(FILE) }

fn load(root: &Path) -> Result<Vault, String> {
    let path = path(root);
    crate::identity_store::safe(&path)?;
    let metadata = match fs::symlink_metadata(&path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vault { version: 1, entries: BTreeMap::new() }),
        Err(_) => return Err("无法检查邮件凭据文件".into()),
    };
    if metadata.file_type().is_symlink() || !metadata.is_file() || metadata.len() > MAX_FILE { return Err("邮件凭据文件不安全或超出大小限制".into()); }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if metadata.file_attributes() & 0x400 != 0 { return Err("邮件凭据文件是目录联接".into()); }
    }
    let bytes = fs::read(path).map_err(|_| "无法读取邮件凭据文件")?;
    let vault: Vault = serde_json::from_slice(&bytes).map_err(|_| "邮件凭据文件格式无效")?;
    if vault.version != 1 || vault.entries.len() > 1000 || vault.entries.keys().any(|id| !valid_id(id)) {
        return Err("邮件凭据文件版本或账号 ID 无效".into());
    }
    for encoded in vault.entries.values() {
        let blob = STANDARD.decode(encoded).map_err(|_| "邮件凭据密文格式无效")?;
        if blob.is_empty() || blob.len() > MAX_SECRET + 64 * 1024 { return Err("邮件凭据密文大小无效".into()); }
    }
    Ok(vault)
}

pub fn snapshot(root: &Path) -> Result<BTreeMap<String, String>, String> {
    let _lock = EDIT.lock().map_err(|_| "邮件凭据锁不可用")?;
    let vault = load(root)?;
    let mut result = BTreeMap::new();
    for (id, encoded) in vault.entries {
        let blob = STANDARD.decode(encoded).map_err(|_| "邮件凭据密文格式无效")?;
        let secret = String::from_utf8(unprotect(&blob)?).map_err(|_| "邮件凭据不是有效 UTF-8")?;
        result.insert(id, secret);
    }
    Ok(result)
}

pub fn encode(secrets: &BTreeMap<String, String>) -> Result<Vec<u8>, String> {
    if secrets.len() > 1000 { return Err("邮件账号数量超过限制".into()); }
    let mut vault = Vault { version: 1, entries: BTreeMap::new() };
    for (id, secret) in secrets {
        if !valid_id(id) || secret.is_empty() || secret.len() > MAX_SECRET { return Err("邮件凭据字段无效或超出大小限制".into()); }
        let blob = protect(secret.as_bytes())?;
        if unprotect(&blob)? != secret.as_bytes() { return Err("邮件凭据 DPAPI 回读校验失败".into()); }
        vault.entries.insert(id.clone(), STANDARD.encode(blob));
    }
    let bytes = serde_json::to_vec(&vault).map_err(|_| "无法编码邮件凭据")?;
    if bytes.len() as u64 > MAX_FILE { return Err("邮件凭据文件超出大小限制".into()); }
    Ok(bytes)
}

pub fn read(root: &Path, id: &str) -> Result<Option<String>, String> {
    if !valid_id(id) { return Err("邮件账号 ID 无效".into()); }
    let _lock = EDIT.lock().map_err(|_| "邮件凭据锁不可用")?;
    let vault = load(root)?;
    vault.entries.get(id).map(|encoded| -> Result<String, String> {
        let blob = STANDARD.decode(encoded).map_err(|_| "邮件凭据密文格式无效")?;
        let plain = unprotect(&blob)?;
        String::from_utf8(plain).map_err(|_| "邮件凭据不是有效 UTF-8".to_string())
    }).transpose()
}

pub fn write(root: &Path, id: &str, secret: &str) -> Result<(), String> {
    if !valid_id(id) || secret.is_empty() || secret.len() > MAX_SECRET { return Err("邮件凭据字段无效或超出大小限制".into()); }
    let _lock = EDIT.lock().map_err(|_| "邮件凭据锁不可用")?;
    let mut vault = load(root)?;
    let protected = protect(secret.as_bytes())?;
    if unprotect(&protected)? != secret.as_bytes() { return Err("邮件凭据 DPAPI 回读校验失败".into()); }
    vault.version = 1;
    vault.entries.insert(id.to_string(), STANDARD.encode(protected));
    write_vault(root, &vault)
}

pub fn import(root: &Path, secrets: &[(String, String)]) -> Result<(), String> {
    let _lock = EDIT.lock().map_err(|_| "邮件凭据锁不可用")?;
    let mut vault = load(root)?;
    let mut added = Vec::with_capacity(secrets.len());
    for (id, secret) in secrets {
        if !valid_id(id) || secret.is_empty() || secret.len() > MAX_SECRET { return Err("旧邮件账号凭据字段无效或超出大小限制".into()); }
        let protected = protect(secret.as_bytes())?;
        if unprotect(&protected)? != secret.as_bytes() { return Err("旧邮件凭据 DPAPI 回读校验失败；未提交迁移".into()); }
        added.push((id.clone(), STANDARD.encode(protected)));
    }
    if vault.entries.len() + added.len() > 1000 { return Err("邮件账号数量超过迁移限制".into()); }
    vault.version = 1;
    for (id, encoded) in added { vault.entries.entry(id).or_insert(encoded); }
    write_vault(root, &vault)
}

pub fn delete(root: &Path, id: &str) -> Result<(), String> {
    if !valid_id(id) { return Err("邮件账号 ID 无效".into()); }
    let _lock = EDIT.lock().map_err(|_| "邮件凭据锁不可用")?;
    let mut vault = load(root)?;
    vault.entries.remove(id);
    write_vault(root, &vault)
}

fn write_vault(root: &Path, vault: &Vault) -> Result<(), String> {
    let bytes = serde_json::to_vec(vault).map_err(|_| "无法序列化邮件凭据")?;
    if bytes.len() as u64 > MAX_FILE { return Err("邮件凭据文件超出大小限制".into()); }
    let path = path(root);
    let tmp = root.join(format!(".mail-secrets-{}-{}.tmp", std::process::id(), crate::now_ms()));
    let mut file = fs::OpenOptions::new().write(true).create_new(true).open(&tmp).map_err(|_| "无法创建邮件凭据暂存文件")?;
    if file.write_all(&bytes).and_then(|_| file.sync_all()).is_err() { return Err("无法写入邮件凭据暂存文件".into()); }
    drop(file);
    replace_file(&tmp, &path)?;
    Ok(())
}

#[cfg(windows)]
fn replace_file(from: &Path, to: &Path) -> Result<(), String> {
    use std::os::windows::ffi::OsStrExt;
    #[link(name = "kernel32")]
    unsafe extern "system" { fn MoveFileExW(from: *const u16, to: *const u16, flags: u32) -> i32; }
    let from: Vec<u16> = from.as_os_str().encode_wide().chain(Some(0)).collect();
    let to: Vec<u16> = to.as_os_str().encode_wide().chain(Some(0)).collect();
    if unsafe { MoveFileExW(from.as_ptr(), to.as_ptr(), 0x1 | 0x8) } == 0 { return Err("无法原子更新邮件凭据文件".into()); }
    Ok(())
}

#[cfg(not(windows))]
fn replace_file(from: &Path, to: &Path) -> Result<(), String> {
    fs::rename(from, to).map_err(|_| "无法原子更新邮件凭据文件".into())
}

#[cfg(windows)]
#[repr(C)]
struct DataBlob { size: u32, data: *mut u8 }

#[cfg(windows)]
fn protect(bytes: &[u8]) -> Result<Vec<u8>, String> {
    use std::ffi::c_void;
    #[link(name = "crypt32")]
    unsafe extern "system" { fn CryptProtectData(input: *const DataBlob, description: *const u16, entropy: *const DataBlob, reserved: *mut c_void, prompt: *mut c_void, flags: u32, output: *mut DataBlob) -> i32; }
    #[link(name = "kernel32")]
    unsafe extern "system" { fn LocalFree(memory: *mut c_void) -> *mut c_void; }
    let input = DataBlob { size: bytes.len() as u32, data: bytes.as_ptr() as *mut u8 };
    let mut output = DataBlob { size: 0, data: std::ptr::null_mut() };
    if unsafe { CryptProtectData(&input, std::ptr::null(), std::ptr::null(), std::ptr::null_mut(), std::ptr::null_mut(), 1, &mut output) } == 0 {
        return Err("Windows 当前用户 DPAPI 加密失败".into());
    }
    let encrypted = unsafe { std::slice::from_raw_parts(output.data, output.size as usize).to_vec() };
    unsafe { LocalFree(output.data.cast()); }
    Ok(encrypted)
}

#[cfg(windows)]
fn unprotect(bytes: &[u8]) -> Result<Vec<u8>, String> {
    use std::ffi::c_void;
    #[link(name = "crypt32")]
    unsafe extern "system" { fn CryptUnprotectData(input: *const DataBlob, description: *mut *mut u16, entropy: *const DataBlob, reserved: *mut c_void, prompt: *mut c_void, flags: u32, output: *mut DataBlob) -> i32; }
    #[link(name = "kernel32")]
    unsafe extern "system" { fn LocalFree(memory: *mut c_void) -> *mut c_void; }
    let input = DataBlob { size: bytes.len() as u32, data: bytes.as_ptr() as *mut u8 };
    let mut output = DataBlob { size: 0, data: std::ptr::null_mut() };
    if unsafe { CryptUnprotectData(&input, std::ptr::null_mut(), std::ptr::null(), std::ptr::null_mut(), std::ptr::null_mut(), 1, &mut output) } == 0 {
        return Err("Windows 当前用户 DPAPI 解密失败".into());
    }
    let plain = unsafe { std::slice::from_raw_parts(output.data, output.size as usize).to_vec() };
    unsafe { LocalFree(output.data.cast()); }
    Ok(plain)
}

#[cfg(not(windows))]
fn protect(_bytes: &[u8]) -> Result<Vec<u8>, String> { Err("邮件凭据的 DPAPI 迁移仅支持 Windows".into()) }
#[cfg(not(windows))]
fn unprotect(_bytes: &[u8]) -> Result<Vec<u8>, String> { Err("邮件凭据的 DPAPI 解密仅支持 Windows".into()) }

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn current_user_dpapi_roundtrip_writes_no_plaintext() {
        let root = std::env::temp_dir().join(format!("fish-dock-mail-vault-{}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        let synthetic = "synthetic-only-test-secret";
        write(&root, "test-account", synthetic).unwrap();
        let stored = fs::read(path(&root)).unwrap();
        assert!(!String::from_utf8_lossy(&stored).contains(synthetic));
        assert_eq!(read(&root, "test-account").unwrap().as_deref(), Some(synthetic));
        delete(&root, "test-account").unwrap();
        assert_eq!(read(&root, "test-account").unwrap(), None);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn rejects_invalid_account_ids() {
        assert!(!valid_id("../outside"));
        assert!(!valid_id(""));
        assert!(valid_id("account-1.a"));
    }
}
