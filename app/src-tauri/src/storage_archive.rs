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
struct Manifest {
    format: String, version: u32, encrypted: bool, owner: Identity, categories: Vec<String>, files: Vec<File>,
    // 选择性回滚点专用：这些路径在导出时存在、但被跳过（大文件/非文本），恢复时
    // 必须原样保留，不能被当成「新版本引入的待删文件」。普通导出为空（不写盘）。
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    skipped: Vec<String>,
}
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
// —— 升级回滚点（#37）：选择性、可跳过、总量受单一阈值约束 ——
// 只打包配置类小文件；`credentials` 默认排除（Magpie 的 credentials/ 里塞着 477 MB
// 的便携运行时，整类导出必然撞上 16 MiB 单文件 / 30 MiB 总量两道闸门）。逐文件超限
// 就跳过并记录，绝不因为一个大文件让整次更新失败。
pub const ROLLBACK_CATEGORIES: &[&str] = &["roster", "keys", "config"];
/// 回滚点文件数上限：`identity_archive::zip` 的条目上限是 128，这里留出 README 与
/// manifest 的余量，避免「文件太多」把回滚点也撑爆。
const ROLLBACK_ENTRIES: usize = 100;

#[derive(Clone, Debug, Serialize)]
pub struct SkippedFile { pub path: String, pub bytes: u64, pub reason: String }
#[derive(Clone, Debug)]
pub struct RollbackExport { pub bytes: Option<Vec<u8>>, pub skipped: Vec<SkippedFile>, pub total_bytes: usize, pub categories: Vec<String> }

/// 配置类小文件才进回滚点：含 NUL 或不是合法 UTF-8 的按二进制处理，直接跳过。
fn is_text(bytes: &[u8]) -> bool { !bytes.contains(&0) && std::str::from_utf8(bytes).is_ok() }

fn collect_limited(namespace: &Namespace, category: &str, folder: &Path, files: &mut BTreeMap<String, Vec<u8>>, skipped: &mut Vec<SkippedFile>, total: &mut usize, limit: usize, depth: usize) -> Result<(), String> {
    crate::storage::safe(folder)?;
    if depth > 16 { return Err("分类目录嵌套超过限制".into()); }
    let entries = match fs::read_dir(folder) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(_) => return Err("分类目录读取失败".into()),
    };
    for entry in entries {
        let path = entry.map_err(|_| "分类条目读取失败")?.path();
        crate::storage::safe(&path)?;
        let meta = fs::symlink_metadata(&path).map_err(|_| "分类条目检查失败")?;
        let name = path.strip_prefix(namespace.path()).map_err(|_| "分类条目越过命名空间")?.components().map(|c| c.as_os_str().to_str().ok_or("分类条目文件名不是 UTF-8")).collect::<Result<Vec<_>, _>>()?.join("/");
        if name == format!("{category}/README.md") { continue; }
        if meta.is_dir() { collect_limited(namespace, category, &path, files, skipped, total, limit, depth + 1)?; continue; }
        data_path(namespace, &name)?;
        if !meta.is_file() { skipped.push(SkippedFile { path: name, bytes: meta.len(), reason: "不是普通文件".into() }); continue; }
        if meta.len() > limit as u64 { skipped.push(SkippedFile { path: name, bytes: meta.len(), reason: format!("单文件超过回滚点上限 {limit} 字节") }); continue; }
        if files.len() >= ROLLBACK_ENTRIES { skipped.push(SkippedFile { path: name, bytes: meta.len(), reason: format!("回滚点文件数上限 {ROLLBACK_ENTRIES}") }); continue; }
        let bytes = match crate::identity_store::read_optional(&path) {
            Ok(Some(bytes)) => bytes,
            Ok(None) => continue,
            Err(_) => { skipped.push(SkippedFile { path: name, bytes: meta.len(), reason: "文件读取失败".into() }); continue; }
        };
        if !is_text(&bytes) { skipped.push(SkippedFile { path: name, bytes: bytes.len() as u64, reason: "非文本内容，默认不进回滚点".into() }); continue; }
        if total.saturating_add(bytes.len()) > limit { skipped.push(SkippedFile { path: name, bytes: bytes.len() as u64, reason: format!("累计超过回滚点上限 {limit} 字节") }); continue; }
        *total += bytes.len();
        files.insert(name, bytes);
    }
    Ok(())
}

/// 选择性导出升级回滚点。`limit` 是单文件与总量共用的唯一上限（来自 `config.rs`）。
/// 没有任何可打包的小文件时返回 `bytes: None`（调用方降级为「本次无回滚点」，不阻断更新）。
// list_all 只列出类别下的文件名（不读内容、不设大小上限）：回滚点恢复时用它算出
// 「这次失败的新版本引入了哪些原先没有的文件」，再减去导出时被跳过保留的路径。
fn list_all(namespace: &Namespace, category: &str, folder: &Path, out: &mut Vec<String>, depth: usize) -> Result<(), String> {
    crate::storage::safe(folder)?;
    if depth > 16 { return Ok(()); }
    let entries = match fs::read_dir(folder) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(_) => return Err("分类目录读取失败".into()),
    };
    for entry in entries {
        let path = entry.map_err(|_| "分类条目读取失败")?.path();
        let meta = fs::symlink_metadata(&path).map_err(|_| "分类条目检查失败")?;
        if meta.is_dir() { list_all(namespace, category, &path, out, depth + 1)?; continue; }
        let name = path.strip_prefix(namespace.path()).map_err(|_| "分类条目越过命名空间")?.components().map(|c| c.as_os_str().to_str().ok_or("分类条目文件名不是 UTF-8")).collect::<Result<Vec<_>, _>>()?.join("/");
        if name == format!("{category}/README.md") { continue; }
        out.push(name);
    }
    Ok(())
}

pub fn export_rollback(namespace: &Namespace, limit: usize) -> Result<RollbackExport, String> {
    let selected: Vec<String> = ROLLBACK_CATEGORIES.iter().map(|category| (*category).to_string()).collect();
    categories(&selected)?;
    let _lock = EDIT.lock().map_err(|_| "回滚点导出锁不可用")?;
    crate::storage_transaction::recover_unlocked(namespace)?;
    let mut files = BTreeMap::new();
    let mut skipped = Vec::new();
    let mut total = 0usize;
    let mut present: Vec<String> = Vec::new();
    for category in &selected {
        let folder = namespace.path().join(category);
        if !folder.is_dir() { continue; }
        collect_limited(namespace, category, &folder, &mut files, &mut skipped, &mut total, limit, 0)?;
        present.push(category.clone());
    }
    if files.is_empty() { return Ok(RollbackExport { bytes: None, skipped, total_bytes: total, categories: present }); }
    let owner = namespace.identity();
    let readme = format!("# FISH Dock 升级回滚点\n\n插件：{}  \n版本：{}  \n数据 schema：{}\n\n包含类别：{}。\n\n这是升级前自动生成的选择性回滚点，只含配置类小文件；credentials 与运行时等大文件已跳过。\n", owner.plugin_id, owner.plugin_version, owner.data_schema, present.join("、"));
    files.insert("README.md".into(), readme.into_bytes());
    for category in &present {
        let bytes = crate::identity_store::read_optional(&namespace.path().join(category).join("README.md"))?.ok_or("已安装版本的目录说明缺失")?;
        files.insert(format!("{category}/README.md"), bytes);
    }
    let manifest = Manifest { format: "fish-dock/plugin-backup".into(), version: 1, encrypted: false, owner: owner.clone(), categories: present.clone(), files: files.iter().map(|(path, bytes)| File { path: path.clone(), bytes: bytes.len(), sha256: sha(bytes) }).collect(), skipped: skipped.iter().map(|s| s.path.clone()).collect() };
    files.insert("manifest.json".into(), serde_json::to_vec_pretty(&manifest).map_err(|_| "回滚点清单编码失败")?);
    let bytes = crate::identity_archive::zip(&files)?;
    Ok(RollbackExport { bytes: Some(bytes), skipped, total_bytes: total, categories: present })
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
    let manifest = Manifest { format: "fish-dock/plugin-backup".into(), version: 1, encrypted: false, owner: owner.clone(), categories: selected.to_vec(), files: files.iter().map(|(path, bytes)| File { path: path.clone(), bytes: bytes.len(), sha256: sha(bytes) }).collect(), skipped: Vec::new() };
    files.insert("manifest.json".into(), serde_json::to_vec_pretty(&manifest).map_err(|_| "分类归档清单编码失败")?);
    crate::identity_archive::zip(&files)
}
pub fn inspect(namespace: &Namespace, bytes: &[u8]) -> Result<Changes, String> { inspect_inner(namespace, bytes, true) }
/// 回滚点恢复：只把归档里的文件覆盖回去，不把「没进回滚点的大文件」当成删除项
/// （那些文件在升级过程中根本没被改过，误删等于毁用户数据）。
pub fn inspect_rollback(namespace: &Namespace, bytes: &[u8]) -> Result<Changes, String> { inspect_inner(namespace, bytes, false) }
fn inspect_inner(namespace: &Namespace, bytes: &[u8], prune: bool) -> Result<Changes, String> {
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
    let mut changes: Changes = if prune {
        selected_files(namespace, &manifest.categories)?.into_keys().map(|name| (name, None)).collect()
    } else {
        // 回滚点恢复：删掉「失败的新版本引入、导出时还不存在」的文件；导出时被跳过
        // 保留的大文件（manifest.skipped）原样不动。既不漏删新残留，也不误删用户数据。
        let mut current = Vec::new();
        for category in &manifest.categories { list_all(namespace, category, &namespace.path().join(category), &mut current, 0)?; }
        let preserved: HashSet<String> = manifest.skipped.iter().cloned().collect();
        current.into_iter().filter(|name| !preserved.contains(name)).map(|name| (name, None)).collect()
    };
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
    // #37 回归：回滚点选择性导出。credentials 整类排除；单文件超限、二进制大文件
    // 逐项跳过并写进 skipped，绝不因为一个大文件让整次更新失败。
    #[test]
    fn rollback_export_is_selective_skips_oversized_and_excludes_credentials() {
        let source = namespace("fish.magpie", ".fishplugin-magpie");
        source.write("keys", "gateway.json", b"synthetic-gateway").unwrap();
        source.write("roster", "accounts.json", br#"{"items":[]}"#).unwrap();
        source.write("config", "settings.json", br#"{"on":true}"#).unwrap();
        source.write("credentials", "oauth.json", b"synthetic-private-token").unwrap();
        // 4 KiB 文本：超过这次给的 1 KiB 上限 → 跳过；含 NUL 的二进制 → 跳过。
        source.write("config", "huge.json", &vec![b'x'; 4096]).unwrap();
        source.write("config", "blob.bin", b"abc\0def").unwrap();
        let point = export_rollback(&source, 1024).unwrap();
        let bytes = point.bytes.expect("有可打包的小文件时就该生成回滚点");
        assert!(point.skipped.iter().any(|s| s.path == "config/huge.json"), "超限文件要进 skipped：{:?}", point.skipped);
        assert!(point.skipped.iter().any(|s| s.path == "config/blob.bin"), "二进制文件要进 skipped：{:?}", point.skipped);
        assert!(!String::from_utf8_lossy(&bytes).contains("synthetic-private-token"), "credentials 默认不进回滚点");
        let files = crate::identity_archive::unzip_with(&bytes, |_| true).unwrap();
        assert!(files.contains_key("keys/gateway.json"));
        assert!(!files.keys().any(|name| name.starts_with("credentials/")), "回滚点不含 credentials 类别");
        // 回滚点必须能被恢复路径 accept；而且「被跳过的文件」不得被当成删除项。
        // 模拟「失败的新版本又写了一个新文件」：恢复时要删掉它，但不能碰被跳过的用户大文件。
        source.write("config", "introduced.json", br#"{"new":true}"#).unwrap();
        let changes = inspect_rollback(&source, &bytes).unwrap();
        assert_eq!(changes.get("keys/gateway.json"), Some(&Some(b"synthetic-gateway".to_vec())));
        assert_eq!(changes.get("config/introduced.json"), Some(&None), "失败版本新引入的文件要在恢复时删掉");
        assert!(!changes.contains_key("config/huge.json"), "没进回滚点的大文件不能被当成待删除项");
        assert!(!changes.contains_key("config/blob.bin"), "没进回滚点的二进制文件同样不能被当成待删除项");
        // 常规导出仍可整类导出 credentials；两者互不影响。
        assert!(export(&source, &["credentials".into()]).is_ok());
        fs::remove_dir_all(source.path()).unwrap();
    }
    // 全部文件都被跳过时回滚点是 None（调用方降级为「本次无回滚点」），而不是报错。
    #[test]
    fn rollback_export_with_only_oversized_files_is_none_not_an_error() {
        let source = namespace("fish.magpie", ".fishplugin-magpie");
        source.write("config", "huge.json", &vec![b'x'; 8192]).unwrap();
        let point = export_rollback(&source, 1024).unwrap();
        assert!(point.bytes.is_none());
        assert_eq!(point.skipped.len(), 1);
        fs::remove_dir_all(source.path()).unwrap();
    }
}
