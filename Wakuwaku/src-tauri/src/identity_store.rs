//! 身份文件成组替换：提交前保存旧副本；中断后先恢复，再允许身份读写。
use std::{collections::BTreeMap, fs, io::Write, path::{Path, PathBuf}, sync::{Mutex, atomic::{AtomicU64, Ordering}}};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub static EDIT: Mutex<()> = Mutex::new(());
pub type Changes = BTreeMap<String, Option<Vec<u8>>>;
pub const OFM_FILES: &[&str] = &[
    "settings.json", "stats.json", "availability.json", "catalog.json", "channel-credentials.json", "eac-user.json",
    "channel-pack/state.json", "channel-pack/token-ledger.json", "channel-pack/permanent-locks.json",
    "channel-pack/dead-models.json", "channel-pack/ui-preferences.json", "channel-pack/auto-checkin.json", "channel-pack/gemini-sigs.json",
];
pub const MAIL_FILES: &[&str] = &["mail-accounts.json", "mail-secrets.dpapi", "identity-state.json"];
const JOURNAL: &str = "identity-transaction.json";
const LIMIT: usize = 32 * 1024 * 1024;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Row { path: String, after: Option<String>, before: Option<String> }
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Journal { version: u32, id: String, rows: Vec<Row> }
fn allowed(name: &str) -> bool { OFM_FILES.contains(&name) || MAIL_FILES.contains(&name) }
fn digest(bytes: &[u8]) -> String { format!("{:x}", Sha256::digest(bytes)) }
fn id() -> String {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    format!("txn-{}-{}-{}", std::process::id(), crate::now_ms(), NEXT.fetch_add(1, Ordering::Relaxed))
}

pub fn safe(path: &Path) -> Result<(), String> {
    for part in path.ancestors() {
        match fs::symlink_metadata(part) {
            Ok(meta) => {
                let mut linked = meta.file_type().is_symlink();
                #[cfg(windows)] { use std::os::windows::fs::MetadataExt; linked |= meta.file_attributes() & 0x400 != 0; }
                if linked { return Err("身份路径包含链接或目录联接".into()); }
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => return Err("无法检查身份文件路径".into()),
        }
    }
    Ok(())
}
pub fn read_optional(path: &Path) -> Result<Option<Vec<u8>>, String> {
    safe(path)?;
    match fs::symlink_metadata(path) {
        Ok(meta) if meta.is_file() && meta.len() <= LIMIT as u64 => fs::read(path).map(Some).map_err(|_| "无法读取身份文件".into()),
        Ok(_) => Err("身份文件不是普通文件或超过大小限制".into()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(_) => Err("无法检查身份文件".into()),
    }
}
fn write_new(path: &Path, bytes: &[u8]) -> Result<(), String> {
    safe(path)?;
    if let Some(parent) = path.parent() { fs::create_dir_all(parent).map_err(|_| "无法创建身份暂存目录")?; }
    let mut file = fs::OpenOptions::new().write(true).create_new(true).open(path).map_err(|_| "无法创建身份暂存文件")?;
    #[cfg(unix)] { use std::os::unix::fs::PermissionsExt; file.set_permissions(fs::Permissions::from_mode(0o600)).map_err(|_| "无法限制身份文件权限")?; }
    file.write_all(bytes).and_then(|_| file.sync_all()).map_err(|_| "无法完整保存身份暂存文件".into())
}
#[cfg(windows)]
fn replace(from: &Path, to: &Path) -> Result<(), String> {
    use std::os::windows::ffi::OsStrExt;
    #[link(name = "kernel32")]
    unsafe extern "system" { fn MoveFileExW(from: *const u16, to: *const u16, flags: u32) -> i32; }
    let from: Vec<u16> = from.as_os_str().encode_wide().chain(Some(0)).collect();
    let to: Vec<u16> = to.as_os_str().encode_wide().chain(Some(0)).collect();
    if unsafe { MoveFileExW(from.as_ptr(), to.as_ptr(), 0x1 | 0x8) } != 0 { Ok(()) } else { Err("无法原子替换身份文件".into()) }
}
#[cfg(not(windows))]
fn replace(from: &Path, to: &Path) -> Result<(), String> { fs::rename(from, to).map_err(|_| "无法原子替换身份文件".into()) }
fn validate_journal(journal: &Journal) -> Result<(), String> {
    if journal.version != 1 || !journal.id.starts_with("txn-") || journal.id.len() > 100 || !journal.id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-') || journal.rows.is_empty() || journal.rows.len() > 32 {
        return Err("身份事务记录格式无效".into());
    }
    let mut seen = std::collections::HashSet::new();
    for row in &journal.rows {
        if !allowed(&row.path) || !seen.insert(&row.path) || row.after.iter().chain(row.before.iter()).any(|d| d.len() != 64 || !d.bytes().all(|b| b.is_ascii_hexdigit())) {
            return Err("身份事务包含非法路径或摘要".into());
        }
    }
    Ok(())
}
fn apply_rows(root: &Path, journal: &Journal, mut after: impl FnMut(usize) -> Result<(), String>) -> Result<(), String> {
    validate_journal(journal)?;
    let backup = root.join(".backups").join(&journal.id);
    let metadata = read_optional(&backup.join("transaction.json"))?.ok_or("身份恢复记录缺失，未替换当前身份")?;
    let expected = serde_json::to_vec(journal).map_err(|_| "身份事务编码失败")?;
    if metadata != expected { return Err("身份恢复记录不匹配，未替换当前身份".into()); }
    for row in &journal.rows {
        let before = read_optional(&backup.join(&row.path))?;
        if before.as_ref().map(|b| digest(b)) != row.before { return Err("身份恢复副本校验失败，未替换当前身份".into()); }
    }
    // 先验证全部暂存文件，再开始替换；损坏的事务不允许加载半份身份。
    let stage = root.join(".staging").join(&journal.id);
    for row in &journal.rows {
        if let Some(expected) = &row.after {
            let bytes = read_optional(&stage.join(&row.path))?.ok_or("身份事务暂存文件缺失")?;
            if &digest(&bytes) != expected { return Err("身份事务暂存文件校验失败".into()); }
        }
    }
    for (index, row) in journal.rows.iter().enumerate() {
        let target = root.join(&row.path);
        safe(&target)?;
        if row.after.is_some() {
            let bytes = read_optional(&stage.join(&row.path))?.ok_or("身份事务暂存文件缺失")?;
            let tmp = target.with_file_name(format!(".{}.tmp", id()));
            write_new(&tmp, &bytes)?;
            replace(&tmp, &target)?;
        } else if read_optional(&target)?.is_some() {
            fs::remove_file(&target).map_err(|_| "无法提交身份文件删除")?;
        }
        after(index)?;
    }
    safe(&root.join(JOURNAL))?;
    fs::remove_file(root.join(JOURNAL)).map_err(|_| "身份已写入，但无法完成事务标记；请重试恢复".into())
}
pub fn recover_unlocked(root: &Path) -> Result<(), String> {
    let Some(bytes) = read_optional(&root.join(JOURNAL))? else { return Ok(()) };
    if bytes.len() > 64 * 1024 { return Err("身份事务记录大小无效".into()); }
    let journal: Journal = serde_json::from_slice(&bytes).map_err(|_| "身份事务记录损坏，未启动身份功能")?;
    validate_journal(&journal)?;
    apply_rows(root, &journal, |_| Ok(()))
}
pub fn recover(root: &Path) -> Result<(), String> {
    let _lock = EDIT.lock().map_err(|_| "身份操作锁不可用")?;
    recover_unlocked(root)
}
fn prepare(root: &Path, changes: Changes) -> Result<Journal, String> {
    recover_unlocked(root)?;
    if changes.is_empty() || changes.len() > 32 || changes.keys().any(|key| !allowed(key)) || changes.values().flatten().map(Vec::len).sum::<usize>() > LIMIT {
        return Err("身份替换范围或数据大小无效".into());
    }
    safe(root)?;
    let id = id();
    let (stage, backup) = (root.join(".staging").join(&id), root.join(".backups").join(&id));
    let mut rows = Vec::new();
    for (name, bytes) in changes {
        let before = read_optional(&root.join(&name))?;
        if let Some(before) = &before { write_new(&backup.join(&name), before)?; }
        if let Some(bytes) = &bytes { write_new(&stage.join(&name), bytes)?; }
        rows.push(Row { path: name, after: bytes.as_ref().map(|b| digest(b)), before: before.as_ref().map(|b| digest(b)) });
    }
    let journal = Journal { version: 1, id, rows };
    let encoded = serde_json::to_vec(&journal).map_err(|_| "无法编码身份事务")?;
    // 恢复副本及其记录先完整落盘，再发布可恢复提交标记。
    write_new(&backup.join("transaction.json"), &encoded)?;
    write_new(&root.join(JOURNAL), &encoded)?;
    Ok(journal)
}
pub fn apply_unlocked(root: &Path, changes: Changes) -> Result<PathBuf, String> {
    let journal = prepare(root, changes)?;
    apply_rows(root, &journal, |_| Ok(()))?;
    Ok(root.join(".backups").join(journal.id))
}
pub fn apply(root: &Path, changes: Changes) -> Result<PathBuf, String> {
    let _lock = EDIT.lock().map_err(|_| "身份操作锁不可用")?;
    apply_unlocked(root, changes)
}
pub fn restore(root: &Path, backup: &Path) -> Result<(), String> {
    safe(backup)?;
    if backup.parent() != Some(root.join(".backups").as_path()) { return Err("身份恢复副本不属于当前目录".into()); }
    let bytes = read_optional(&backup.join("transaction.json"))?.ok_or("缺少身份恢复记录")?;
    if bytes.len() > 64 * 1024 { return Err("身份恢复记录大小无效".into()); }
    let journal: Journal = serde_json::from_slice(&bytes).map_err(|_| "身份恢复记录无效")?;
    validate_journal(&journal)?;
    if backup.file_name().and_then(|n| n.to_str()) != Some(journal.id.as_str()) { return Err("身份恢复记录归属不匹配".into()); }
    let mut changes = Changes::new();
    for row in journal.rows {
        let bytes = read_optional(&backup.join(&row.path))?;
        if bytes.as_ref().map(|b| digest(b)) != row.before { return Err("身份恢复副本校验失败".into()); }
        changes.insert(row.path, bytes);
    }
    apply(root, changes).map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn root() -> PathBuf {
        let root = std::env::temp_dir().join(id());
        fs::create_dir_all(&root).unwrap(); root
    }
    #[test]
    fn replaces_accounts_and_secrets_together_and_keeps_before_copy() {
        let root = root();
        fs::write(root.join("mail-accounts.json"), b"old accounts").unwrap();
        fs::write(root.join("mail-secrets.dpapi"), b"old encrypted").unwrap();
        let changes = BTreeMap::from([("mail-accounts.json".into(), Some(b"new accounts".to_vec())), ("mail-secrets.dpapi".into(), Some(b"new encrypted".to_vec()))]);
        let backup = apply(&root, changes).unwrap();
        assert_eq!(fs::read(root.join("mail-accounts.json")).unwrap(), b"new accounts");
        assert_eq!(fs::read(root.join("mail-secrets.dpapi")).unwrap(), b"new encrypted");
        assert_eq!(fs::read(backup.join("mail-accounts.json")).unwrap(), b"old accounts");
        recover(&root).unwrap();
        restore(&root, &backup).unwrap();
        assert_eq!(fs::read(root.join("mail-accounts.json")).unwrap(), b"old accounts");
        assert_eq!(fs::read(root.join("mail-secrets.dpapi")).unwrap(), b"old encrypted");
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn rejects_paths_outside_the_identity_contract_without_changes() {
        let root = root();
        fs::write(root.join("mail-accounts.json"), b"old").unwrap();
        for name in ["../outside", "C:/secret", "logs/debug.log", "Mail-Accounts.json"] { assert!(apply(&root, BTreeMap::from([(name.into(), Some(b"bad".to_vec()))])).is_err()); }
        assert_eq!(fs::read(root.join("mail-accounts.json")).unwrap(), b"old");
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn missing_backup_metadata_prevents_recovery_writes() {
        let root=root();fs::write(root.join("settings.json"),b"old").unwrap();
        let journal=prepare(&root,BTreeMap::from([("settings.json".into(),Some(b"new".to_vec()))])).unwrap();
        fs::remove_file(root.join(".backups").join(journal.id).join("transaction.json")).unwrap();
        assert!(recover(&root).is_err());
        assert_eq!(fs::read(root.join("settings.json")).unwrap(),b"old");
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn restore_validates_paths_before_reading_any_backup_data() {
        let root=root();let backup=root.join(".backups/txn-fixture");fs::create_dir_all(&backup).unwrap();
        let journal=Journal{version:1,id:"txn-fixture".into(),rows:vec![Row{path:"../../outside.json".into(),before:None,after:None}]};
        fs::write(backup.join("transaction.json"),serde_json::to_vec(&journal).unwrap()).unwrap();
        assert!(restore(&root,&backup).unwrap_err().contains("非法路径"));
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn restart_finishes_an_interrupted_pair_before_loading() {
        let root = root();
        fs::write(root.join("mail-accounts.json"), b"old").unwrap();
        let journal = prepare(&root, BTreeMap::from([("mail-accounts.json".into(), Some(b"new".to_vec())), ("mail-secrets.dpapi".into(), Some(b"encrypted".to_vec()))])).unwrap();
        assert!(apply_rows(&root, &journal, |_| Err("synthetic interruption".into())).is_err());
        assert!(root.join(JOURNAL).is_file());
        recover(&root).unwrap();
        assert_eq!(fs::read(root.join("mail-secrets.dpapi")).unwrap(), b"encrypted");
        assert!(!root.join(JOURNAL).exists());
        fs::remove_dir_all(root).unwrap();
    }
}
