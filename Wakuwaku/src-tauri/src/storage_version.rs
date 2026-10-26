//! 版本目录说明更新事务；重启后按持久化日志完成同一版本，不触碰用户数据。
use std::{collections::BTreeMap, fs, path::Path};
use serde::{Serialize, Deserialize};
use crate::storage_context::{Identity, Namespace};
const JOURNAL: &str = "tmp/version-transaction.json";
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Journal { schema: u32, before: Identity, after: Identity, old: BTreeMap<String, Vec<u8>> }
pub fn change(namespace: &Namespace, version: &str, schema: u32) -> Result<Namespace, String> {
    let _lock = crate::storage_context::EDIT.lock().map_err(|_| "版本事务锁不可用")?;
    recover_unlocked(namespace.path())?;
    crate::plugin_manifest::version(version)?;
    if schema != namespace.identity().data_schema { return Err("数据 schema 变化需要专用迁移适配器".into()); }
    let bytes = crate::identity_store::read_optional(&namespace.path().join("storage.json"))?.ok_or("缺少存储归属记录")?;
    let before: Identity = serde_json::from_slice(&bytes).map_err(|_| "存储归属记录无效")?;
    if &before != namespace.identity() { return Err("存储版本已变化，请刷新后重试".into()); }
    if before.plugin_version == version { return Ok(namespace.clone()); }
    crate::storage_transaction::recover_unlocked(namespace)?;
    let mut after = before.clone(); after.plugin_version = version.into();
    let mut old = BTreeMap::from([("storage.json".into(),bytes)]);
    for category in std::iter::once("").chain(crate::storage::CATEGORIES.iter().copied()) {
        let name = if category.is_empty() { "README.md".to_owned() } else { format!("{category}/README.md") };
        old.insert(name.clone(),crate::identity_store::read_optional(&namespace.path().join(name))?.ok_or("缺少旧版本目录说明")?);
    }
    let journal = Journal { schema:1,before,after,old };
    namespace.atomic(&namespace.path().join(JOURNAL),&serde_json::to_vec(&journal).map_err(|_| "版本事务编码失败")?)?;
    finish(namespace.path(),journal)
}
pub fn recover(root: &Path) -> Result<(), String> {
    let _lock = crate::storage_context::EDIT.lock().map_err(|_| "版本恢复锁不可用")?;
    recover_unlocked(root)
}
fn recover_unlocked(root: &Path) -> Result<(), String> {
    if let Some(bytes) = crate::identity_store::read_optional(&root.join(JOURNAL))? {
        let journal: Journal = serde_json::from_slice(&bytes).map_err(|_| "版本事务日志损坏")?;
        finish(root,journal)?;
    }
    Ok(())
}
fn finish(root: &Path, journal: Journal) -> Result<Namespace, String> {
    use sha2::{Digest,Sha256};
    crate::storage::safe(root)?;
    let before = &journal.before; let after = &journal.after;
    let source = if before.plugin_id.starts_with("fish.") { crate::storage::Source::OfficialCatalog } else { crate::storage::Source::ThirdParty };
    let correct = if before.plugin_id == "host" { before.directory == "host" && root.file_name().and_then(|n| n.to_str()) == Some("host") } else { crate::storage::namespace(root.parent().ok_or("缺少数据根")?,&before.plugin_id,&before.directory,source)? == root };
    crate::plugin_manifest::version(&before.plugin_version)?; crate::plugin_manifest::version(&after.plugin_version)?;
    let names: std::collections::BTreeSet<String> = std::iter::once("README.md".to_owned()).chain(std::iter::once("storage.json".into())).chain(crate::storage::CATEGORIES.iter().map(|c|format!("{c}/README.md"))).collect();
    if !correct || journal.schema != 1 || before.plugin_id != after.plugin_id || before.directory != after.directory || before.data_schema == 0 || before.data_schema != after.data_schema || journal.old.keys().cloned().collect::<std::collections::BTreeSet<_>>() != names || journal.old.values().any(|b| b.len() > 1024*1024) { return Err("版本事务归属或说明文件列表无效".into()); }
    let marker = root.join("storage.json");
    let current: Identity = serde_json::from_slice(&crate::identity_store::read_optional(&marker)?.ok_or("存储归属记录缺失")?).map_err(|_| "存储归属记录损坏")?;
    if &current != before && &current != after { return Err("版本事务与当前存储归属冲突".into()); }
    let ns = Namespace { root:root.into(),identity:before.clone() };
    let backup = serde_json::to_vec(&journal).map_err(|_| "版本备份编码失败")?;
    let backup_name = format!("backups/version-{:x}.json",Sha256::digest(&backup));
    let backup_path = root.join(backup_name);
    if crate::identity_store::read_optional(&backup_path)?.as_deref() != Some(backup.as_slice()) { ns.atomic(&backup_path,&backup)?; }
    crate::storage::initialize(root,&after.plugin_id,&after.plugin_version,after.data_schema)?;
    ns.atomic(&marker,&serde_json::to_vec_pretty(after).map_err(|_| "新版归属记录编码失败")?)?;
    // 说明和归属先持久化，最后撤下日志；中断时可重复完成同一事务。
    fs::remove_file(root.join(JOURNAL)).map_err(|_| "版本事务已提交，清理日志失败，请重试")?;
    Ok(Namespace { root:root.into(),identity:journal.after })
}
#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> Namespace {
        let root = std::env::temp_dir().join(format!("fish-version-{}-{}",std::process::id(),crate::now_ms())).join(".plugin.acme.weather");
        crate::storage::initialize(&root,"acme.weather","1.0.0",1).unwrap();
        let identity = Identity { plugin_id:"acme.weather".into(),directory:".plugin.acme.weather".into(),plugin_version:"1.0.0".into(),data_schema:1 };
        fs::write(root.join("storage.json"),serde_json::to_vec(&identity).unwrap()).unwrap();
        Namespace::existing(&root).unwrap().unwrap()
    }
    #[test] fn upgrade_refreshes_readmes_without_rewriting_secrets_or_notes() {
        let ns = fixture(); ns.write("credentials","fixture.json",b"synthetic-kept").unwrap();
        fs::write(ns.path().join("USER-NOTES.md"),b"kept").unwrap();
        let new = change(&ns,"1.1.0",1).unwrap();
        assert_eq!(new.identity().plugin_version,"1.1.0");
        assert_eq!(new.read("credentials","fixture.json").unwrap().unwrap(),b"synthetic-kept");
        for part in std::iter::once("").chain(crate::storage::CATEGORIES.iter().copied()) { assert!(fs::read_to_string(new.path().join(part).join("README.md")).unwrap().contains("1.1.0")); }
        assert_eq!(fs::read(new.path().join("USER-NOTES.md")).unwrap(),b"kept");
        assert!(change(&new,"2.0.0",2).is_err());
        fs::remove_dir_all(new.path().parent().unwrap()).unwrap();
    }
}
