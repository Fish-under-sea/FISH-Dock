//! 命名空间内可恢复的分类资料替换；恢复副本与暂存均留在该数据根。
use std::{collections::{BTreeMap, HashSet}, fs, io::Write, path::{Path, PathBuf}, sync::atomic::{AtomicU64, Ordering}};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use crate::storage_context::{Namespace, Identity, EDIT, MAX_FILE};
pub type Changes = BTreeMap<String, Option<Vec<u8>>>;
const JOURNAL: &str = "tmp/storage-transaction.json";
const MAX_TOTAL: usize = 64 * 1024 * 1024;
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Row { path: String, before: Option<String>, after: Option<String> }
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Journal { version: u32, id: String, owner: Identity, rows: Vec<Row> }
fn digest(bytes: &[u8]) -> String { format!("{:x}", Sha256::digest(bytes)) }
fn id() -> String {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    format!("txn-{}-{}-{}", std::process::id(), crate::now_ms(), NEXT.fetch_add(1, Ordering::Relaxed))
}
fn target(namespace: &Namespace, name: &str) -> Result<PathBuf, String> {
    let (category, relative) = name.split_once('/').ok_or("分类事务路径缺少类别")?;
    if !["credentials", "roster", "keys", "config"].contains(&category) { return Err("事务只能替换凭据、花名册、密钥及配置".into()); }
    namespace.file(category, relative)
}
fn validate(namespace: &Namespace, journal: &Journal) -> Result<(), String> {
    if journal.version != 1 || &journal.owner != namespace.identity() || !journal.id.starts_with("txn-") || journal.id.len() > 100 || !journal.id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-') || journal.rows.is_empty() || journal.rows.len() > 128 {
        return Err("分类事务归属或记录无效".into());
    }
    let mut seen = HashSet::new();
    for row in &journal.rows {
        target(namespace, &row.path)?;
        if !seen.insert(row.path.to_ascii_lowercase()) || row.before.iter().chain(row.after.iter()).any(|d| d.len() != 64 || !d.bytes().all(|b| b.is_ascii_hexdigit())) { return Err("分类事务包含路径冲突或无效摘要".into()); }
    }
    Ok(())
}
fn write_new(path: &Path, bytes: &[u8]) -> Result<(), String> {
    crate::storage::safe(path)?;
    fs::create_dir_all(path.parent().ok_or("分类文件缺少父目录")?).map_err(|_| "分类暂存目录创建失败")?;
    let mut file = fs::OpenOptions::new().create_new(true).write(true).open(path).map_err(|_| "分类暂存文件创建失败")?;
    #[cfg(unix)] { use std::os::unix::fs::PermissionsExt; file.set_permissions(fs::Permissions::from_mode(0o600)).map_err(|_| "分类文件权限设置失败")?; }
    file.write_all(bytes).and_then(|_| file.sync_all()).map_err(|_| "分类暂存文件写入失败")?;
    drop(file);
    if crate::identity_store::read_optional(path)?.as_deref() != Some(bytes) { return Err("分类暂存文件回读校验失败".into()); }
    Ok(())
}
fn prepare(namespace: &Namespace, changes: Changes) -> Result<Journal, String> {
    // 完整预检在生成任何副本、改写任何活跃文件之前完成。
    if changes.is_empty() || changes.len() > 128 || changes.values().flatten().any(|bytes| bytes.len() > MAX_FILE) || changes.values().flatten().map(Vec::len).sum::<usize>() > MAX_TOTAL { return Err("分类事务文件数量或大小超过限制".into()); }
    let mut seen = HashSet::new();
    let mut previous = BTreeMap::new();
    for name in changes.keys() {
        let path = target(namespace, name)?;
        if !seen.insert(name.to_ascii_lowercase()) { return Err("分类事务路径大小写冲突".into()); }
        previous.insert(name.clone(), crate::identity_store::read_optional(&path)?);
    }
    recover_unlocked(namespace)?;
    // 恢复可能改变活跃文件，因此恢复后重新读取，不使用恢复前快照。
    for (name, bytes) in previous.iter_mut() { *bytes = crate::identity_store::read_optional(&target(namespace, name)?)?; }
    let id = id();
    let backup = namespace.path().join("backups").join(&id);
    let stage = namespace.path().join("tmp").join(&id);
    let mut rows = Vec::new();
    for (name, after) in changes {
        let before = previous.remove(&name).flatten();
        if let Some(bytes) = &before { write_new(&backup.join(&name), bytes)?; }
        if let Some(bytes) = &after { write_new(&stage.join(&name), bytes)?; }
        rows.push(Row { path: name, before: before.as_ref().map(|b| digest(b)), after: after.as_ref().map(|b| digest(b)) });
    }
    let journal = Journal { version: 1, id, owner: namespace.identity().clone(), rows };
    validate(namespace, &journal)?;
    let encoded = serde_json::to_vec(&journal).map_err(|_| "分类事务编码失败")?;
    write_new(&backup.join("transaction.json"), &encoded)?;
    // 最后发布恢复标记；中断后才会继续提交这一份事务。
    namespace.atomic(&namespace.path().join(JOURNAL), &encoded)?;
    Ok(journal)
}
fn commit_rows(namespace: &Namespace, journal: &Journal, mut after: impl FnMut(usize) -> Result<(), String>) -> Result<(), String> {
    validate(namespace, journal)?;
    let backup = namespace.path().join("backups").join(&journal.id);
    let stage = namespace.path().join("tmp").join(&journal.id);
    let encoded = serde_json::to_vec(journal).map_err(|_| "分类事务编码失败")?;
    if crate::identity_store::read_optional(&backup.join("transaction.json"))?.as_deref() != Some(encoded.as_slice()) { return Err("分类事务恢复记录不匹配".into()); }
    let mut total = 0usize;
    // 任一暂存或副本损坏，整组都不开始替换。
    for row in &journal.rows {
        let before = crate::identity_store::read_optional(&backup.join(&row.path))?;
        let bytes = crate::identity_store::read_optional(&stage.join(&row.path))?;
        total += before.as_ref().map_or(0, Vec::len) + bytes.as_ref().map_or(0, Vec::len);
        if total > MAX_TOTAL * 2 || before.as_ref().map(|b| digest(b)) != row.before || bytes.as_ref().map(|b| digest(b)) != row.after { return Err("分类事务暂存或恢复副本校验失败".into()); }
        target(namespace, &row.path)?;
    }
    for (index, row) in journal.rows.iter().enumerate() {
        let path = target(namespace, &row.path)?;
        if let Some(expected) = &row.after {
            let bytes = crate::identity_store::read_optional(&stage.join(&row.path))?.ok_or("分类事务暂存缺失")?;
            if &digest(&bytes) != expected { return Err("分类事务暂存校验失败".into()); }
            namespace.atomic(&path, &bytes)?;
        } else if crate::identity_store::read_optional(&path)?.is_some() {
            fs::remove_file(&path).map_err(|_| "分类文件删除未提交")?;
        }
        after(index)?;
    }
    crate::storage::safe(&namespace.path().join(JOURNAL))?;
    fs::remove_file(namespace.path().join(JOURNAL)).map_err(|_| "分类事务已替换，结束标记尚未清理；请重试恢复".into())
}
pub fn recover_unlocked(namespace: &Namespace) -> Result<(), String> {
    let Some(bytes) = crate::identity_store::read_optional(&namespace.path().join(JOURNAL))? else { return Ok(()) };
    if bytes.len() > 256 * 1024 { return Err("分类事务记录超过限制".into()); }
    let journal: Journal = serde_json::from_slice(&bytes).map_err(|_| "分类事务记录损坏")?;
    commit_rows(namespace, &journal, |_| Ok(()))
}
pub fn apply(namespace: &Namespace, changes: Changes) -> Result<PathBuf, String> {
    let _lock = EDIT.lock().map_err(|_| "分类事务锁不可用")?;
    let journal = prepare(namespace, changes)?;
    commit_rows(namespace, &journal, |_| Ok(()))?;
    Ok(namespace.path().join("backups").join(journal.id))
}
pub fn restore(namespace: &Namespace, backup: &Path) -> Result<PathBuf, String> {
    let _lock = EDIT.lock().map_err(|_| "分类事务锁不可用")?;
    crate::storage::safe(backup)?;
    if backup.parent() != Some(namespace.path().join("backups").as_path()) { return Err("恢复副本不属于当前插件目录".into()); }
    let bytes = crate::identity_store::read_optional(&backup.join("transaction.json"))?.ok_or("分类恢复记录缺失")?;
    if bytes.len() > 256 * 1024 { return Err("分类恢复记录超过限制".into()); }
    let journal: Journal = serde_json::from_slice(&bytes).map_err(|_| "分类恢复记录损坏")?;
    validate(namespace, &journal)?;
    if backup.file_name().and_then(|v| v.to_str()) != Some(journal.id.as_str()) { return Err("分类恢复记录归属不匹配".into()); }
    let mut changes = Changes::new();
    for row in journal.rows {
        let bytes = crate::identity_store::read_optional(&backup.join(&row.path))?;
        if bytes.as_ref().map(|b| digest(b)) != row.before { return Err("分类恢复副本摘要不匹配".into()); }
        changes.insert(row.path, bytes);
    }
    let recovery = prepare(namespace, changes)?;
    commit_rows(namespace, &recovery, |_| Ok(()))?;
    Ok(namespace.path().join("backups").join(recovery.id))
}
// prune_staging 清掉「确认无主」的事务暂存目录（<root>/tmp/txn-*）。
//
// prepare() 先把要写的字节放进 tmp/<id>、把旧字节放进 backups/<id>，最后才发布
// tmp/storage-transaction.json 这份 journal；提交成功时只删 journal，tmp/<id> 与
// backups/<id> 都留在盘上。backups/<id> 是 restore() 要用的恢复副本，必须留；
// tmp/<id> 一旦没有 journal 就是纯垃圾——本机 host/tmp 那 6 个（10-26 到今天）
// 就是这么攒下来的。
//
// 归属判定用 journal，不看 mtime：journal 里的 id 是唯一「进行中」的那个事务。
// journal 存在却解析不了（损坏）时一个都不删——读不了就等于判断不了归属。
// 另加 EDIT 锁：prepare()/apply()/restore() 全程持锁，所以握到锁之后看到的
// 「没有 journal 的 txn 目录」不可能是别人正在写的那一份（不会误删在途事务）。
pub fn prune_staging(root: &Path) -> Result<Vec<String>, String> {
    let _lock = EDIT.lock().map_err(|_| "分类事务锁不可用")?;
    let temporary = root.join("tmp");
    let entries = match fs::read_dir(&temporary) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(_) => return Err("事务暂存目录读取失败".into()),
    };
    let live = match fs::read(temporary.join("storage-transaction.json")).ok() {
        None => None,
        Some(bytes) => match serde_json::from_slice::<Journal>(&bytes) {
            Ok(journal) => Some(journal.id),
            // 损坏的 journal 由 recover 报错等人处理，这里不越权删任何东西。
            Err(_) => return Ok(Vec::new()),
        },
    };
    let mut removed = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|_| "事务暂存条目读取失败")?;
        // file_type() 不跟随符号链接：链接不会被当成目录删掉。
        if !entry.file_type().map_err(|_| "事务暂存条目类型读取失败")?.is_dir() { continue; }
        let name = entry.file_name().to_string_lossy().to_string();
        if !name.starts_with("txn-") || name.len() > 100 || !name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-') { continue; }
        if live.as_deref() == Some(name.as_str()) { continue; }
        fs::remove_dir_all(entry.path()).map_err(|_| format!("事务暂存目录清理失败：{name}"))?;
        removed.push(name);
    }
    removed.sort();
    Ok(removed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use crate::storage_context::Identity;
    // 分类事务不修改目录权限；这里创建合成文件系统夹具。
    // Context::open 的真实 Windows 权限验证由独立集成测试覆盖。
    fn namespace() -> Namespace {
        static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let root = std::env::temp_dir().join(format!("fish-transaction-{}-{}-{}", std::process::id(), crate::now_ms(), NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)));
        crate::storage::initialize(&root, "fish.ofm", "2.5.3", 1).unwrap();
        Namespace { root, identity: Identity { plugin_id: "fish.ofm".into(), directory: ".fishplugin-ourfreemodel".into(), plugin_version: "2.5.3".into(), data_schema: 1 } }
    }
    #[test]
    fn replaces_selected_files_together_with_verified_before_copy() {
        let plugin = namespace();
        plugin.write("keys", "gateway.json", b"old-key").unwrap();
        plugin.write("config", "keep.json", b"keep").unwrap();
        let backup = apply(&plugin, BTreeMap::from([("keys/gateway.json".into(), Some(b"new-key".to_vec())), ("roster/accounts.json".into(), Some(b"new-roster".to_vec()))])).unwrap();
        assert_eq!(plugin.read("keys", "gateway.json").unwrap().unwrap(), b"new-key");
        assert_eq!(plugin.read("roster", "accounts.json").unwrap().unwrap(), b"new-roster");
        assert_eq!(plugin.read("config", "keep.json").unwrap().unwrap(), b"keep");
        assert!(backup.starts_with(plugin.path().join("backups")));
        assert_eq!(fs::read(backup.join("keys/gateway.json")).unwrap(), b"old-key");
        assert!(!plugin.path().join("tmp/storage-transaction.json").exists());
        fs::remove_dir_all(plugin.path()).unwrap();
    }
    #[test]
    fn interruption_is_finished_before_the_next_namespace_read() {
        let plugin = namespace();
        plugin.write("keys", "api.json", b"old").unwrap();
        let journal = prepare(&plugin, BTreeMap::from([("keys/api.json".into(), Some(b"new".to_vec())), ("roster/accounts.json".into(), Some(b"accounts".to_vec()))])).unwrap();
        assert!(commit_rows(&plugin, &journal, |_| Err("synthetic interruption".into())).is_err());
        assert!(plugin.path().join(JOURNAL).exists());
        assert_eq!(plugin.read("roster", "accounts.json").unwrap().unwrap(), b"accounts");
        assert_eq!(plugin.read("keys", "api.json").unwrap().unwrap(), b"new");
        assert!(!plugin.path().join(JOURNAL).exists());
        fs::remove_dir_all(plugin.path()).unwrap();
    }
    #[test]
    fn damaged_stage_and_wrong_owner_never_begin_a_commit() {
        let plugin = namespace();
        plugin.write("keys", "api.json", b"old").unwrap();
        let mut journal = prepare(&plugin, BTreeMap::from([("keys/api.json".into(), Some(b"new".to_vec()))])).unwrap();
        fs::write(plugin.path().join("tmp").join(&journal.id).join("keys/api.json"), b"damaged").unwrap();
        assert!(recover_unlocked(&plugin).is_err());
        assert_eq!(fs::read(plugin.path().join("keys/api.json")).unwrap(), b"old");
        journal.owner.plugin_id = "other.plugin".into();
        assert!(commit_rows(&plugin, &journal, |_| Ok(())).is_err());
        assert_eq!(fs::read(plugin.path().join("keys/api.json")).unwrap(), b"old");
        fs::remove_dir_all(plugin.path()).unwrap();
    }
    #[test]
    fn restore_keeps_another_before_copy_and_rejects_tampered_metadata() {
        let plugin = namespace();
        plugin.write("keys", "api.json", b"old").unwrap();
        let backup = apply(&plugin, BTreeMap::from([("keys/api.json".into(), Some(b"new".to_vec())), ("config/fresh.json".into(), Some(b"added".to_vec()))])).unwrap();
        let undo = restore(&plugin, &backup).unwrap();
        assert_eq!(plugin.read("keys", "api.json").unwrap().unwrap(), b"old");
        assert!(plugin.read("config", "fresh.json").unwrap().is_none());
        assert_eq!(fs::read(undo.join("keys/api.json")).unwrap(), b"new");
        fs::write(backup.join("keys/api.json"), b"tampered").unwrap();
        assert!(restore(&plugin, &backup).is_err());
        assert_eq!(plugin.read("keys", "api.json").unwrap().unwrap(), b"old");
        fs::remove_dir_all(plugin.path()).unwrap();
    }
    #[test]
    fn invalid_transaction_never_replaces_any_existing_file() {
        let plugin = namespace();
        plugin.write("keys", "api.json", b"kept").unwrap();
        for bad in ["keys/../api.json", "../outside", "keys/README.md", "tmp/storage-transaction.json", "backups/other"] {
            assert!(apply(&plugin, BTreeMap::from([("keys/api.json".into(), Some(b"bad".to_vec())), (bad.into(), None)])).is_err());
            assert_eq!(plugin.read("keys", "api.json").unwrap().unwrap(), b"kept");
        }
        fs::remove_dir_all(plugin.path()).unwrap();
    }
    // #t11 回归①：事务提交完只删 journal，tmp/<id> 就留在盘上了（本机 host/tmp 攒了 6 个）。
    // 没有 journal 指认它 → 无主 → 清掉；backups/<id> 是 restore() 要用的恢复副本，必须留。
    #[test]
    fn txn_staging_without_a_journal_is_removed_and_the_recovery_copy_kept() {
        let plugin = namespace();
        plugin.write("keys", "api.json", b"old").unwrap();
        let journal = prepare(&plugin, BTreeMap::from([("keys/api.json".into(), Some(b"new".to_vec()))])).unwrap();
        let stage = plugin.path().join("tmp").join(&journal.id);
        let recovery = plugin.path().join("backups").join(&journal.id);
        assert!(stage.is_dir() && recovery.join("transaction.json").is_file());
        // 提交完成：journal 消失，暂存与恢复副本原样留着。
        fs::remove_file(plugin.path().join(JOURNAL)).unwrap();
        let removed = prune_staging(plugin.path()).unwrap();
        assert_eq!(removed, vec![journal.id.clone()], "无主的暂存目录要被清掉");
        assert!(!stage.exists(), "无主暂存目录必须不存在");
        assert!(recovery.join("transaction.json").is_file(), "恢复副本是 restore() 的来源，不许删");
        // 正常事务与用户文件都不受影响。
        assert_eq!(plugin.read("keys", "api.json").unwrap().unwrap(), b"old");
        assert!(plugin.path().join("tmp/README.md").is_file(), "tmp 下的目录说明不是事务残留");
        fs::remove_dir_all(plugin.path()).unwrap();
    }
    // #t11 回归②：journal 还在 = 这个事务进行中，暂存目录一个都不许动；也不能把恢复流程搞坏。
    #[test]
    fn txn_staging_under_a_live_journal_is_kept() {
        let plugin = namespace();
        plugin.write("keys", "api.json", b"old").unwrap();
        let journal = prepare(&plugin, BTreeMap::from([("keys/api.json".into(), Some(b"new".to_vec()))])).unwrap();
        let stage = plugin.path().join("tmp").join(&journal.id);
        assert!(prune_staging(plugin.path()).unwrap().is_empty(), "进行中的事务不许被清");
        assert!(stage.is_dir());
        assert!(plugin.path().join(JOURNAL).exists());
        // 清过之后恢复还能照常把这份事务做完。
        recover_unlocked(&plugin).unwrap();
        assert_eq!(plugin.read("keys", "api.json").unwrap().unwrap(), b"new");
        assert!(!stage.exists() || stage.is_dir(), "恢复不依赖我们删没删");
        // 事务结束后再清：这次它就是无主的了。
        assert!(prune_staging(plugin.path()).unwrap().iter().all(|name| name == &journal.id));
        assert!(!stage.exists());
        fs::remove_dir_all(plugin.path()).unwrap();
    }
    // #t11 回归③：journal 损坏时判断不了归属，一个都不删、更不许 panic。
    #[test]
    fn txn_prune_keeps_everything_when_the_journal_is_unreadable() {
        let plugin = namespace();
        plugin.write("keys", "api.json", b"old").unwrap();
        let journal = prepare(&plugin, BTreeMap::from([("keys/api.json".into(), Some(b"new".to_vec()))])).unwrap();
        let stage = plugin.path().join("tmp").join(&journal.id);
        fs::write(plugin.path().join(JOURNAL), b"not a journal").unwrap();
        assert!(prune_staging(plugin.path()).unwrap().is_empty(), "读不出 journal 就不许删任何暂存目录");
        assert!(stage.is_dir());
        // 顺带：坏 journal 仍然由 recover 如实报错，而不是被悄悄当成「没事」。
        assert!(recover_unlocked(&plugin).is_err());
        fs::remove_dir_all(plugin.path()).unwrap();
    }
    // #t11 回归④：清理失败只报错，不 panic——调用点（启动时的 cleanup）据此只记日志。
    #[test]
    fn txn_prune_reports_failure_instead_of_panicking() {
        let plugin = namespace();
        fs::remove_dir_all(plugin.path().join("tmp")).unwrap();
        fs::write(plugin.path().join("tmp"), b"not a directory").unwrap();
        assert!(prune_staging(plugin.path()).is_err(), "读不了暂存目录要如实返回错误");
        fs::remove_dir_all(plugin.path()).unwrap();
    }
    // #t11 回归⑤：真实副本的现场——host/tmp 里只有一堆无主 txn-* 与 README.md，
    // 清完之后只剩 README.md，别的什么都不碰。
    #[test]
    fn txn_prune_clears_ownerless_staging_and_leaves_everything_else() {
        let plugin = namespace();
        let temporary = plugin.path().join("tmp");
        for name in ["txn-3920-1793015486376-0", "txn-49948-1793088689563-0"] { fs::create_dir_all(temporary.join(name)).unwrap(); }
        fs::write(temporary.join("txn-3920-1793015486376-0/staged.bin"), b"staged").unwrap();
        fs::write(temporary.join("keep-me.txt"), b"user file").unwrap();
        let removed = prune_staging(plugin.path()).unwrap();
        assert_eq!(removed, vec!["txn-3920-1793015486376-0".to_string(), "txn-49948-1793088689563-0".to_string()]);
        let mut left: Vec<String> = fs::read_dir(&temporary).unwrap().map(|entry| entry.unwrap().file_name().to_string_lossy().to_string()).collect();
        left.sort();
        assert_eq!(left, vec!["README.md".to_string(), "keep-me.txt".to_string()]);
        fs::remove_dir_all(plugin.path()).unwrap();
    }
}
