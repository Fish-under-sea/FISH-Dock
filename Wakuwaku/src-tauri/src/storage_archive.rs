//! 按插件与类别导入导出；归档内不携带源机器路径。
use std::{collections::{BTreeMap, HashSet}, fs, path::Path};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use crate::{storage_context::{Namespace, Identity, EDIT, MAX_FILE}, storage_transaction::Changes};
const CATEGORIES: &[&str] = &["credentials", "roster", "keys", "config"];
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct File { path: String, bytes: usize, sha256: String }
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest { format: String, version: u32, encrypted: bool, owner: Identity, categories: Vec<String>, files: Vec<File> }
fn sha(bytes: &[u8]) -> String { format!("{:x}", Sha256::digest(bytes)) }
fn categories(values: &[String]) -> Result<(), String> {
    let mut seen = HashSet::new();
    if values.is_empty() || values.len() > CATEGORIES.len() || values.iter().any(|c| !CATEGORIES.contains(&c.as_str()) || !seen.insert(c)) { return Err("归档类别为空、重复或不受支持".into()); }
    Ok(())
}
fn data_path(namespace: &Namespace, name: &str) -> Result<(), String> {
    let (category, relative) = name.split_once('/').ok_or("归档文件缺少类别")?;
    if !CATEGORIES.contains(&category) { return Err("归档包含未允许的类别".into()); }
    namespace.file(category, relative).map(|_| ())
}
fn collect(namespace: &Namespace, category: &str, folder: &Path, files: &mut BTreeMap<String, Vec<u8>>, depth: usize) -> Result<(), String> {
    crate::storage::safe(folder)?;
    if depth > 16 { return Err("分类目录嵌套超过限制".into()); }
    for entry in fs::read_dir(folder).map_err(|_| "分类目录读取失败")? {
        let path = entry.map_err(|_| "分类条目读取失败")?.path();
        crate::storage::safe(&path)?;
        let meta = fs::symlink_metadata(&path).map_err(|_| "分类条目检查失败")?;
        let name = path.strip_prefix(namespace.path()).map_err(|_| "分类条目越过命名空间")?.components().map(|c| c.as_os_str().to_str().ok_or("分类条目文件名不是 UTF-8")).collect::<Result<Vec<_>, _>>()?.join("/");
        if name == format!("{category}/README.md") { continue; }
        if meta.is_dir() { collect(namespace, category, &path, files, depth + 1)?; }
        else {
            data_path(namespace, &name)?;
            if !meta.is_file() || meta.len() > MAX_FILE as u64 || files.len() >= 110 { return Err("分类文件类型、大小或数量超过限制".into()); }
            let bytes = crate::identity_store::read_optional(&path)?.ok_or("分类文件读取时已不存在")?;
            files.insert(name, bytes);
            if files.values().map(Vec::len).sum::<usize>() > 30 * 1024 * 1024 { return Err("分类导出总大小超过限制".into()); }
        }
    }
    Ok(())
}
fn selected_files(namespace: &Namespace, values: &[String]) -> Result<BTreeMap<String, Vec<u8>>, String> {
    let mut files = BTreeMap::new();
    for category in values { collect(namespace, category, &namespace.path().join(category), &mut files, 0)?; }
    Ok(files)
}
pub fn export(namespace: &Namespace, selected: &[String]) -> Result<Vec<u8>, String> {
    categories(selected)?;
    let _lock = EDIT.lock().map_err(|_| "分类导出锁不可用")?;
    crate::storage_transaction::recover_unlocked(namespace)?;
    let mut files = selected_files(namespace, selected)?;
    if namespace.identity().plugin_id == "host" && selected.iter().any(|category| category == "credentials") {
        files.remove("credentials/mail-secrets.dpapi");
        let entries = crate::mail_secrets::snapshot(namespace.path())?;
        files.insert("credentials/mail-secrets.json".into(), serde_json::to_vec(&serde_json::json!({"format":"fish-dock/mail-secrets","version":1,"entries":entries})).map_err(|_| "邮件导出编码失败")?);
    }
    let owner = namespace.identity();
    let readme = format!("# FISH Dock 分类备份\n\n插件：{}  \n版本：{}  \n数据 schema：{}\n\n包含类别：{}。\n\n在目标机器由宿主重新解析数据根，校验插件身份、目录键、版本与摘要后导入；导入前副本保存在当前插件的 backups。只有所选类别会被替换。\n\n本 ZIP 未加密；含密钥时请妥善保管。不要将账号或密钥实值复制到此说明。\n", owner.plugin_id, owner.plugin_version, owner.data_schema, selected.join("、"));
    files.insert("README.md".into(), readme.into_bytes());
    for category in selected {
        let bytes = crate::identity_store::read_optional(&namespace.path().join(category).join("README.md"))?.ok_or("已安装版本的目录说明缺失")?;
        files.insert(format!("{category}/README.md"), bytes);
    }
    let manifest = Manifest { format: "fish-dock/plugin-backup".into(), version: 1, encrypted: false, owner: owner.clone(), categories: selected.to_vec(), files: files.iter().map(|(path, bytes)| File { path: path.clone(), bytes: bytes.len(), sha256: sha(bytes) }).collect() };
    files.insert("manifest.json".into(), serde_json::to_vec_pretty(&manifest).map_err(|_| "分类归档清单编码失败")?);
    crate::identity_archive::zip(&files)
}
pub fn inspect(namespace: &Namespace, bytes: &[u8]) -> Result<Changes, String> {
    let files = crate::identity_archive::unzip_with(bytes, |name| {
        name == "manifest.json" || name == "README.md" || CATEGORIES.iter().any(|category| name == format!("{category}/README.md")) || data_path(namespace, name).is_ok()
    })?;
    let manifest: Manifest = serde_json::from_slice(files.get("manifest.json").ok_or("分类备份缺少清单")?).map_err(|_| "分类备份清单格式无效")?;
    if manifest.format != "fish-dock/plugin-backup" || manifest.version != 1 || manifest.encrypted || &manifest.owner != namespace.identity() { return Err("归档插件身份、版本或数据 schema 不匹配；请使用对应版本适配器".into()); }
    categories(&manifest.categories)?;
    if manifest.files.len() + 1 != files.len() || !files.contains_key("README.md") || manifest.categories.iter().any(|category| !files.contains_key(&format!("{category}/README.md"))) { return Err("归档文件与清单数量不匹配或缺少版本说明".into()); }
    let mut seen = HashSet::new();
    for row in &manifest.files {
        if row.path == "manifest.json" || !seen.insert(row.path.to_ascii_lowercase()) { return Err("归档清单包含重复文件".into()); }
        let file = files.get(&row.path).ok_or("归档清单引用缺失文件")?;
        if row.bytes != file.len() || row.sha256 != sha(file) { return Err("归档文件大小或摘要不匹配".into()); }
        if row.path != "README.md" {
            let category = row.path.split('/').next().unwrap_or("");
            if !manifest.categories.iter().any(|c| c == category) { return Err("归档包含用户未选择的类别".into()); }
        }
    }
    let _lock = EDIT.lock().map_err(|_| "分类导入预览锁不可用")?;
    crate::storage_transaction::recover_unlocked(namespace)?;
    let mut changes: Changes = selected_files(namespace, &manifest.categories)?.into_keys().map(|name| (name, None)).collect();
    for (name, bytes) in files {
        if name == "manifest.json" || name.ends_with("README.md") { continue; }
        data_path(namespace, &name)?;
        changes.insert(name, Some(bytes));
    }
    if namespace.identity().plugin_id == "host" {
        if changes.get("credentials/mail-secrets.dpapi").is_some_and(Option::is_some) { return Err("宿主备份应包含便携凭据，不能导入源机器密文".into()); }
        let roster = match changes.get("roster/mail-accounts.json") {
            Some(Some(bytes)) => Some(bytes.clone()),
            Some(None) => return Err("宿主花名册类别缺少邮件账号快照".into()),
            None => crate::identity_store::read_optional(&namespace.file("roster", "mail-accounts.json")?)?,
        };
        let accounts = if let Some(bytes) = roster {
            let value: serde_json::Value = serde_json::from_slice(&bytes).map_err(|_| "邮件账号快照 JSON 无效")?;
            if value["format"] != "fish-dock/mail-accounts" || value["version"] != 1 { return Err("邮件账号快照格式无效".into()); }
            crate::mail_identity::validate(&value["accounts"])?
        } else { Vec::new() };
        if let Some(Some(bytes)) = changes.remove("credentials/mail-secrets.json") {
            let value: serde_json::Value = serde_json::from_slice(&bytes).map_err(|_| "邮件凭据快照 JSON 无效")?;
            if value["format"] != "fish-dock/mail-secrets" || value["version"] != 1 { return Err("邮件凭据快照格式无效".into()); }
            let entries = value["entries"].as_object().ok_or("邮件凭据映射无效")?;
            let secrets = entries.iter().map(|(id, value)| Ok((id.clone(), value.as_str().ok_or("邮件凭据应为字符串")?.to_owned()))).collect::<Result<BTreeMap<_, _>, String>>()?;
            if secrets.keys().any(|id| !accounts.iter().any(|a| a["id"].as_str() == Some(id))) { return Err("邮件凭据引用了未导入的账号".into()); }
            changes.insert("credentials/mail-secrets.dpapi".into(), Some(crate::mail_secrets::encode(&secrets)?));
        } else if manifest.categories.iter().any(|category| category == "credentials") { return Err("宿主凭据类别缺少便携邮件快照".into()); }
    }
    Ok(changes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage_context::Identity;
    use std::{fs, path::PathBuf};
    fn namespace(id: &str, directory: &str) -> Namespace {
        static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let root: PathBuf = std::env::temp_dir().join(format!("fish-export-{}-{}-{}", std::process::id(), crate::now_ms(), NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)));
        crate::storage::initialize(&root, id, "2.5.3", 1).unwrap();
        Namespace { root, identity: Identity { plugin_id: id.into(), directory: directory.into(), plugin_version: "2.5.3".into(), data_schema: 1 } }
    }
    #[test]
    fn selected_categories_restore_at_another_root_without_other_secrets() {
        let source = namespace("fish.ofm", ".fishplugin-ourfreemodel");
        source.write("keys", "gateway.json", b"synthetic-gateway").unwrap();
        source.write("credentials", "oauth.json", b"synthetic-private-token").unwrap();
        let bytes = export(&source, &["keys".into()]).unwrap();
        let text = String::from_utf8_lossy(&bytes);
        assert!(!text.contains("synthetic-private-token"));
        assert!(!text.contains(source.path().to_str().unwrap()));
        let target = namespace("fish.ofm", ".fishplugin-ourfreemodel");
        target.write("credentials", "oauth.json", b"keep-target-token").unwrap();
        let changes = inspect(&target, &bytes).unwrap();
        crate::storage_transaction::apply(&target, changes).unwrap();
        assert_eq!(target.read("keys", "gateway.json").unwrap().unwrap(), b"synthetic-gateway");
        assert_eq!(target.read("credentials", "oauth.json").unwrap().unwrap(), b"keep-target-token");
        let files = crate::identity_archive::unzip_with(&bytes, |_| true).unwrap();
        assert!(String::from_utf8_lossy(&files["keys/README.md"]).contains("2.5.3"));
        assert!(inspect(&namespace("fish.magpie", ".fishplugin-magpie"), &bytes).is_err());
        fs::remove_dir_all(source.path()).unwrap(); fs::remove_dir_all(target.path()).unwrap();
    }
    #[test]
    fn host_mail_export_is_portable_and_import_reencrypts_locally() {
        let base = namespace("fish.ofm", ".fishplugin-ourfreemodel");
        let root = base.path().join("host");
        crate::storage::initialize(&root, "host", "1.0.2", 1).unwrap();
        let owner = Identity { plugin_id: "host".into(), directory: "host".into(), plugin_version: "1.0.2".into(), data_schema: 1 };
        fs::write(root.join("storage.json"), serde_json::to_vec(&owner).unwrap()).unwrap();
        let host = Namespace { root, identity: owner };
        let accounts = serde_json::json!([{"id":"synthetic-mail","address":"fixture@example.test","host":"imap.example.test","port":993,"security":"ssl","username":"fixture@example.test","on":false}]);
        crate::mail_identity::initialize(host.path(), &accounts, |_| Ok(Some("synthetic-secret".into()))).unwrap();
        let bytes = export(&host, &["credentials".into(), "roster".into()]).unwrap();
        let files = crate::identity_archive::unzip_with(&bytes, |_| true).unwrap();
        assert!(!files.contains_key("credentials/mail-secrets.dpapi"));
        assert!(files.contains_key("credentials/mail-secrets.json"));
        let changes = inspect(&host, &bytes).unwrap();
        assert!(changes.contains_key("credentials/mail-secrets.dpapi"));
        assert!(!changes.contains_key("credentials/mail-secrets.json"));
        crate::storage_transaction::apply(&host, changes).unwrap();
        assert_eq!(crate::mail_secrets::read(host.path(), "synthetic-mail").unwrap().as_deref(), Some("synthetic-secret"));
        fs::remove_dir_all(base.path()).unwrap();
    }
    #[test]
    fn wrong_digest_and_unselected_files_are_rejected() {
        let plugin = namespace("fish.ofm", ".fishplugin-ourfreemodel");
        plugin.write("keys", "gateway.json", b"synthetic-gateway").unwrap();
        let bytes = export(&plugin, &["keys".into()]).unwrap();
        let mut files = crate::identity_archive::unzip_with(&bytes, |_| true).unwrap();
        files.insert("keys/gateway.json".into(), b"tampered".to_vec());
        assert!(inspect(&plugin, &crate::identity_archive::zip(&files).unwrap()).is_err());
        assert!(export(&plugin, &["tmp".into()]).is_err());
        fs::remove_dir_all(plugin.path()).unwrap();
    }
}
