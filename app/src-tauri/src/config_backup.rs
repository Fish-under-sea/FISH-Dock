//! 整个 FISH Dock 配置仓的可迁移备份与成组恢复。
use std::{collections::BTreeMap, path::{Path, PathBuf}};
use serde::{Deserialize, Serialize};
use crate::storage_context::Identity;
pub const LIMIT: usize = 128 * 1024 * 1024;
pub const FILE_LIMIT: usize = 16 * 1024 * 1024;
pub fn read_zip(path: &Path) -> Result<Vec<u8>, String> {
    crate::storage::safe(path)?;
    let meta = std::fs::symlink_metadata(path).map_err(|_| "备份 ZIP 已不存在或不可读")?;
    if !meta.is_file() || meta.len() > LIMIT as u64 { return Err("备份 ZIP 文件类型或大小无效".into()); }
    use std::io::Read;
    let mut bytes=Vec::new();
    std::fs::File::open(path).map_err(|_|"备份 ZIP 读取失败")?.take(LIMIT as u64+1).read_to_end(&mut bytes).map_err(|_|"备份 ZIP 读取失败")?;
    if bytes.len()>LIMIT {return Err("备份 ZIP 超过限制".into());} Ok(bytes)
}
pub fn is_config(bytes: &[u8]) -> Result<bool,String> {
    let files=crate::identity_archive::unzip_limits(bytes, |_|true, LIMIT, FILE_LIMIT, ENTRIES)?;
    let value:serde_json::Value=serde_json::from_slice(files.get("manifest.json").ok_or("备份缺少清单")?).map_err(|_|"备份清单格式无效")?;
    Ok(value["format"]=="fish-dock/config-backup")
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Preview { pub owners: Vec<Identity>, pub files: usize, pub bytes: usize, pub missing: usize }
const ENTRIES: usize = 8192;
const JOURNAL: &str = "host/tmp/config-restore.json";
const README: &str = "# FISH Dock 配置仓备份\n\n包含宿主设置、布局、插件注册信息及各插件的 config、keys、roster、credentials，以及宿主自定义插件、宠物、草稿和偏好。\n\n邮件正文（host/mail-store）不在备份内；邮件账号花名册与登录凭据仍会带上，导入后重新收信即可。\n\n排除 tmp、exports、backups、运行态数据（trash、routing、使用记录、模型与市场缓存）、WebView 缓存、服务锁、程序资源与可重新下载的插件包。目标机器缺少插件包时须在商店重新安装，配置继续保留。\n\n此 ZIP 未加密。邮件、GitHub 与宿主加密存储的凭据导出为便携明文，导入时由目标用户重新加密；当前用户解不开的凭据原样带走，导入后需在该机器重新登录一次。旧备份按原范围恢复。\n";
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Entry { path: String, bytes: usize, sha256: String, encoding: String }
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest { format: String, version: u32, owners: Vec<Identity>, files: Vec<Entry> }
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Row { path: String, before: Option<String>, after: Option<String> }
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Journal { version: u32, id: String, committed: bool, owners: Vec<Identity>, rows: Vec<Row> }
fn sha(bytes: &[u8]) -> String { use sha2::{Digest, Sha256}; format!("{:x}", Sha256::digest(bytes)) }
fn owner_path(root: &Path, owner: &Identity) -> Result<PathBuf, String> {
    if owner.data_schema == 0 || owner.plugin_version.is_empty() || owner.plugin_version.len() > 100 || !owner.plugin_version.bytes().all(|b| b.is_ascii_alphanumeric() || b".-+".contains(&b)) { return Err("备份命名空间版本无效".into()); }
    if owner.plugin_id == "host" {
        if owner.directory != "host" { return Err("宿主备份归属无效".into()); }
        Ok(root.join("host"))
    } else {
        crate::storage::namespace(root, &owner.plugin_id, &owner.directory, if owner.plugin_id.starts_with("fish.") { crate::storage::Source::OfficialCatalog } else { crate::storage::Source::ThirdParty })
    }
}
// 运行态与可重新下载的东西：读得到也不是配置，带进备份只会让它变大、
// 让恢复变慢。按段名与文件名判断，不写死某个插件的路径。
fn runtime_data(name: &str) -> bool {
    let segment = |value: &str| name.split('/').any(|part| part == value);
    if ["README.md", "service.lock", "cache", "node_modules", "codearts-cache", "trash", "routing", "tmp", "dist"].iter().any(|value| segment(value)) { return true; }
    let file = name.rsplit('/').next().unwrap_or("");
    file.ends_with(".bak") || file.ends_with(".jsonl") || file.ends_with(".tgz") || file.ends_with(".exe") || file.ends_with(".gz")
        || matches!(file, "models.json" | "plugin-github.json" | "plugin-npm.json" | "providers-state.json" | "sync-server.magpie-backup" | "usage.jsonl")
}
fn included(owner: &Identity, name: &str) -> bool {
    if runtime_data(name) { return false; }
    if name.starts_with("credentials/magpie-runtime/") && name != "credentials/magpie-runtime/data" && !name.starts_with("credentials/magpie-runtime/data/") { return false; }
    if name.starts_with("config/windhawk/") && name != "config/windhawk/data" && !name.starts_with("config/windhawk/data/") { return false; }
    let category = name.split('/').next().unwrap_or("");
    ["credentials", "roster", "keys", "config"].contains(&category) || (owner.plugin_id == "host" && (name == "plugins" || name == "pets" || name.starts_with("plugins/") || name.starts_with("pets/") || ["mail-draft.json", "today.json", "icons.json", "USER-NOTES.md"].contains(&name)))
}
fn target(root: &Path, owners: &[Identity], name: &str, marker: bool) -> Result<PathBuf, String> {
    let (directory, relative) = name.split_once('/').ok_or("配置备份文件缺少命名空间")?;
    let owner = owners.iter().find(|o| o.directory == directory).ok_or("配置备份文件没有有效归属")?;
    let ns = owner_path(root, owner)?;
    if marker && (relative == "storage.json" || relative == "README.md" || crate::storage::CATEGORIES.iter().any(|category| relative == format!("{category}/README.md"))) { return Ok(ns.join(relative)); }
    if !included(owner, relative) { return Err("配置备份包含非持久资料或保留路径".into()); }
    crate::storage::category_file(&ns, "config", relative)?;
    let path = ns.join(relative); crate::storage::safe(&path)?; Ok(path)
}
fn owners(root: &Path) -> Result<Vec<Identity>, String> {
    let mut result = Vec::new();
    for entry in std::fs::read_dir(root).map_err(|_| "配置仓读取失败")? {
        let path = entry.map_err(|_| "配置仓目录读取失败")?.path();
        let name = path.file_name().and_then(|s| s.to_str()).unwrap_or("");
        if name != "host" && !name.starts_with(".fishplugin-") && !name.starts_with(".plugin.") { continue; }
        if let Some(ns) = crate::storage_context::Namespace::existing(&path)? { result.push(ns.identity().clone()); }
    }
    result.sort_by(|a,b| a.directory.cmp(&b.directory));
    if !result.iter().any(|o| o.plugin_id == "host") { return Err("配置仓缺少宿主归属".into()); }
    Ok(result)
}
fn collect(root: &Path, owner: &Identity, folder: &Path, files: &mut BTreeMap<String, Vec<u8>>, depth: usize) -> Result<(), String> {
    if depth > 24 { return Err("配置目录嵌套超过限制".into()); }
    crate::storage::safe(folder)?;
    let namespace = owner_path(root, owner)?;
    for entry in std::fs::read_dir(folder).map_err(|_| "配置目录读取失败")? {
        let path = entry.map_err(|_| "配置条目读取失败")?.path();
        let relative = path.strip_prefix(&namespace).map_err(|_| "配置目录越界")?.components().map(|c| c.as_os_str().to_str().ok_or("配置文件名不是 UTF-8")).collect::<Result<Vec<_>,_>>()?.join("/");
        if !included(owner, &relative) { continue; }
        crate::storage::safe(&path)?;
        let meta = std::fs::symlink_metadata(&path).map_err(|_| "配置条目检查失败")?;
        if meta.is_dir() { collect(root, owner, &path, files, depth+1)?; }
        else {
            let name = format!("{}/{}", owner.directory, relative); target(root, std::slice::from_ref(owner), &name, false)?;
            if !meta.is_file() || meta.len() > FILE_LIMIT as u64 || files.len() >= ENTRIES-2 { return Err("配置文件类型、大小或数量超过限制".into()); }
            let bytes = crate::identity_store::read_optional(&path)?.ok_or("配置文件读取期间已消失")?;
            files.insert(name, bytes);
            if files.values().map(Vec::len).sum::<usize>() > LIMIT-65536 { return Err("配置仓持久资料超过 128 MiB".into()); }
        }
    }
    Ok(())
}
fn snapshot(root: &Path, owners: &[Identity]) -> Result<BTreeMap<String, Vec<u8>>, String> {
    let mut files = BTreeMap::new();
    for owner in owners { collect(root, owner, &owner_path(root, owner)?, &mut files, 0)?; }
    Ok(files)
}
fn dpapi(bytes: &[u8]) -> bool { bytes.starts_with(&[1,0,0,0,0xd0,0x8c,0x9d,0xdf,1,0x15,0xd1,0x11,0x8c,0x7a,0,0xc0,0x4f,0xc2,0x97,0xeb]) }
pub fn export(root: &Path) -> Result<Vec<u8>, String> { export_with_report(root).map(|(bytes, _)| bytes) }
// 导出并报告有多少份凭据只能原样带走：那份在目标机器上需要重新登录一次。
pub fn export_with_report(root: &Path) -> Result<(Vec<u8>, usize), String> {
    recover(root)?;
    let owners = owners(root)?;
    let _lock = crate::storage_context::EDIT.lock().map_err(|_| "配置仓备份锁不可用")?;
    for owner in &owners { if let Some(ns)=crate::storage_context::Namespace::claimed(&owner_path(root,owner)?)? { crate::storage_transaction::recover_unlocked(&ns)?; } }
    let mut files = snapshot(root, &owners)?;
    let mut entries = Vec::new();
    let mut opaque = 0usize;
    for (name, bytes) in files.iter_mut() {
        let encoding = if name == "host/credentials/mail-secrets.dpapi" {
            *bytes = serde_json::to_vec(&crate::mail_secrets::snapshot(&root.join("host"))?).map_err(|_| "邮件凭据编码失败")?; "mail-secrets"
        } else if dpapi(bytes) {
            // 解不开的 DPAPI（别的用户、别的机器、带 entropy）：原样带走，
            // 目标机器上重新登录一次即可。不能让它把整包导出拖垮。
            match crate::mail_secrets::unprotect(bytes) {
                Ok(plain) => { *bytes = plain; "dpapi" }
                Err(_) => { opaque += 1; "opaque" }
            }
        } else { "plain" };
        entries.push(Entry { path:name.clone(), bytes:bytes.len(), sha256:sha(bytes), encoding:encoding.into() });
    }
    files.insert("README.md".into(), README.as_bytes().to_vec());
    entries.push(Entry {path:"README.md".into(),bytes:README.len(),sha256:sha(README.as_bytes()),encoding:"plain".into()});
    let manifest = Manifest { format:"fish-dock/config-backup".into(), version:1, owners, files:entries };
    files.insert("manifest.json".into(), serde_json::to_vec_pretty(&manifest).map_err(|_| "配置备份清单编码失败")?);
    Ok((crate::identity_archive::zip_limits(&files, LIMIT, FILE_LIMIT, ENTRIES)?, opaque))
}
fn parse(root: &Path, bytes: &[u8]) -> Result<(Manifest,BTreeMap<String,Vec<u8>>),String> {
    let files = crate::identity_archive::unzip_limits(bytes, |name| name == "README.md" || name == "manifest.json" || name.split_once('/').is_some_and(|(_,p)| crate::storage::category_file(root,"config",p).is_ok()), LIMIT, FILE_LIMIT, ENTRIES)?;
    let manifest: Manifest = serde_json::from_slice(files.get("manifest.json").ok_or("配置备份缺少清单")?).map_err(|_| "配置备份清单格式无效")?;
    if manifest.format != "fish-dock/config-backup" || manifest.version != 1 || manifest.owners.is_empty() || manifest.owners.len()>101 || manifest.files.len()+1!=files.len() || !files.contains_key("README.md") { return Err("配置备份格式或数量无效".into()); }
    let mut seen=std::collections::HashSet::new();
    for owner in &manifest.owners {
        owner_path(root,owner)?;
        if !seen.insert(owner.directory.to_lowercase()) { return Err("配置备份命名空间重复".into()); }
        if let Some(current)=crate::storage_context::Namespace::claimed(&root.join(&owner.directory))? {
            let existing=current.identity();
            // schema 是数据的形状，必须一致；版本号只是插件的标签，升级过也能恢复。
            if existing.plugin_id!=owner.plugin_id || existing.data_schema!=owner.data_schema { return Err("配置备份与目标插件的归属或 schema 不匹配，请先迁移".into()); }
        } else {
            let directory=root.join(&owner.directory);
            if directory.exists() && std::fs::read_dir(&directory).map_err(|_|"目标命名空间读取失败")?.next().is_some() { return Err("目标插件目录已有未归属资料，请先迁移或选择干净数据根".into()); }
        }
    }
    if !manifest.owners.iter().any(|o|o.plugin_id=="host") { return Err("配置备份缺少宿主".into()); }
    seen.clear();
    for entry in &manifest.files {
        if entry.path == "manifest.json" || !seen.insert(entry.path.to_lowercase()) || !["plain","dpapi","opaque","mail-secrets"].contains(&entry.encoding.as_str()) { return Err("配置清单包含重复文件或未知编码".into()); }
        if entry.path != "README.md" { target(root,&manifest.owners,&entry.path,false)?; }
        if entry.encoding=="mail-secrets" && entry.path!="host/credentials/mail-secrets.dpapi" { return Err("邮件凭据归属无效".into()); }
        if matches!(entry.encoding.as_str(),"dpapi"|"opaque") && !entry.path.split('/').nth(1).is_some_and(|c|c=="keys"||c=="credentials") { return Err("加密凭据类别无效".into()); }
        let file=files.get(&entry.path).ok_or("配置备份缺少声明文件")?;
        if file.len()!=entry.bytes || sha(file)!=entry.sha256 { return Err("配置备份文件摘要不匹配".into()); }
    }
    Ok((manifest,files))
}
pub fn inspect(root:&Path, bytes:&[u8])->Result<Preview,String>{
    let(manifest,files)=parse(root,bytes)?;
    let missing=manifest.files.iter().filter(|e|e.encoding=="opaque").count();
    Ok(Preview{owners:manifest.owners,files:manifest.files.len()-1,bytes:files.values().map(Vec::len).sum(),missing})
}
fn atomic(path:&Path,bytes:&[u8])->Result<(),String>{
    use std::io::Write;
    crate::storage::safe(path)?;
    std::fs::create_dir_all(path.parent().ok_or("配置文件缺少父目录")?).map_err(|_|"配置目录创建失败")?;
    let tmp=path.with_extension(format!("config-{}.tmp",crate::now_ms()));
    let mut file=std::fs::OpenOptions::new().create_new(true).write(true).open(&tmp).map_err(|_|"配置暂存创建失败")?;
    file.write_all(bytes).and_then(|_|file.sync_all()).map_err(|_|"配置暂存写入失败")?; drop(file);
    crate::storage::replace(&tmp,path).map_err(|_|"配置原子替换失败".into())
}
fn journal_folder(root:&Path,journal:&Journal)->Result<PathBuf,String>{
    if journal.version!=1 || !journal.id.starts_with("config-") || journal.id.len()>100 || !journal.id.bytes().all(|b|b.is_ascii_alphanumeric()||b==b'-') || journal.rows.len()>ENTRIES || journal.owners.len()>101 {return Err("配置恢复记录无效".into());}
    let mut seen=std::collections::HashSet::new();
    for row in &journal.rows {
        target(root,&journal.owners,&row.path,true)?;
        if !seen.insert(row.path.to_lowercase()) || row.before.iter().chain(row.after.iter()).any(|s|s.len()!=64||!s.bytes().all(|b|b.is_ascii_hexdigit())) {return Err("配置恢复记录路径或摘要无效".into());}
    }
    Ok(root.join("host/backups").join(&journal.id))
}
fn rollback(root:&Path,journal:&Journal)->Result<(),String>{
    let folder=journal_folder(root,journal)?;
    let mut before=BTreeMap::new();
    for row in &journal.rows {
        let bytes=crate::identity_store::read_optional(&folder.join("before").join(&row.path))?;
        if bytes.as_ref().map(|b|sha(b))!=row.before {return Err("配置恢复副本摘要不匹配，请保留备份".into());}
        before.insert(row.path.clone(),bytes);
    }
    for (name,bytes) in before.into_iter().rev() {
        let path=target(root,&journal.owners,&name,true)?;
        if let Some(bytes)=bytes {atomic(&path,&bytes)?;} else if path.is_file(){std::fs::remove_file(path).map_err(|_|"配置回滚删除新增文件失败")?;}
    }
    for owner in &journal.owners {
        if journal.rows.iter().any(|r| r.path == format!("{}/storage.json", owner.directory) && r.before.is_none()) {
            let namespace=owner_path(root,owner)?;
            for row in &journal.rows {
                let path=target(root,&journal.owners,&row.path,true)?;
                for directory in path.ancestors().skip(1) {
                    if !directory.starts_with(&namespace) { break; }
                    crate::storage::safe(directory)?;
                    let _=std::fs::remove_dir(directory);
                }
            }
            let _=std::fs::remove_dir(&namespace);
        }
    }
    Ok(())
}
pub fn recover(root:&Path)->Result<(),String>{
    let _lock=crate::storage_context::EDIT.lock().map_err(|_|"配置恢复锁不可用")?;
    recover_unlocked(root)
}
fn recover_unlocked(root:&Path)->Result<(),String>{
    let Some(bytes)=crate::identity_store::read_optional(&root.join(JOURNAL))? else{return Ok(())};
    let journal:Journal=serde_json::from_slice(&bytes).map_err(|_|"配置恢复记录损坏，请保留数据根")?;
    journal_folder(root,&journal)?;
    if !journal.committed {rollback(root,&journal)?;}
    std::fs::remove_file(root.join(JOURNAL)).map_err(|_|"配置恢复标记清理失败".into())
}
pub fn restore(root:&Path,bytes:&[u8],mut activate:impl FnMut()->Result<(),String>)->Result<PathBuf,String>{
    recover(root)?;
    let(manifest,mut files)=parse(root,bytes)?;
    let _lock=crate::storage_context::EDIT.lock().map_err(|_|"配置恢复锁不可用")?;
    for entry in &manifest.files {
        let data=files.get_mut(&entry.path).ok_or("配置内容缺失")?;
        if entry.encoding=="dpapi" {*data=crate::mail_secrets::protect(data)?;}
        if entry.encoding=="mail-secrets" {let secrets:BTreeMap<String,String>=serde_json::from_slice(data).map_err(|_|"便携邮件凭据格式无效")?;*data=crate::mail_secrets::encode(&secrets)?;}
    }
    files.remove("manifest.json");files.remove("README.md");
    // 宿主的归属记录由这台机器自己写（它按当前宿主版本迁移存储）；
    // 把备份里那份旧版本写回去只会让版本倒退，再触发一次多余迁移。
    files.remove("host/storage.json");
    let previous=snapshot(root,&manifest.owners.iter().filter(|o|root.join(&o.directory).is_dir()).cloned().collect::<Vec<_>>())?;
    let mut names=std::collections::BTreeSet::new();names.extend(previous.keys().cloned());names.extend(files.keys().cloned());
    // 新命名空间的归属同样纳入事务；现有归属保留目标机器版本。
    for owner in &manifest.owners {
        let name=format!("{}/storage.json",owner.directory);
        if !root.join(&name).exists(){files.insert(name.clone(),serde_json::to_vec(owner).map_err(|_|"归属编码失败")?);names.insert(name);}
    }
    static NEXT:std::sync::atomic::AtomicUsize=std::sync::atomic::AtomicUsize::new(0);
    let id=format!("config-{}-{}-{}",std::process::id(),crate::now_ms(),NEXT.fetch_add(1,std::sync::atomic::Ordering::Relaxed));
    let mut journal=Journal{version:1,id,committed:false,owners:manifest.owners,rows:Vec::new()};
    let folder=root.join("host/backups").join(&journal.id);
    for owner in &journal.owners {
        if !root.join(&owner.directory).join("storage.json").exists() {
            let stage=folder.join("readmes").join(&owner.directory);
            crate::storage::initialize(&stage,&owner.plugin_id,&owner.plugin_version,owner.data_schema)?;
            for category in std::iter::once("").chain(crate::storage::CATEGORIES.iter().copied()) {
                let relative=if category.is_empty(){"README.md".to_string()}else{format!("{category}/README.md")};
                let name=format!("{}/{}",owner.directory,relative);
                files.insert(name.clone(),crate::identity_store::read_optional(&stage.join(relative))?.ok_or("新命名空间说明缺失")?);names.insert(name);
            }
        }
    }
    for name in names {
        let path=target(root,&journal.owners,&name,true)?;
        let before=crate::identity_store::read_optional(&path)?;
        let after=files.get(&name);
        if before.as_ref()==after {continue;}
        if let Some(data)=&before{atomic(&folder.join("before").join(&name),data)?;}
        if let Some(data)=after{atomic(&folder.join("after").join(&name),data)?;}
        journal.rows.push(Row{path:name,before:before.as_ref().map(|b|sha(b)),after:after.map(|b|sha(b))});
    }
    journal_folder(root,&journal)?;
    let encoded=serde_json::to_vec(&journal).map_err(|_|"恢复事务编码失败")?;
    atomic(&folder.join("transaction.json"),&encoded)?;atomic(&root.join(JOURNAL),&encoded)?;
    let result=(||{
        for row in &journal.rows {
            let path=target(root,&journal.owners,&row.path,true)?;
            if let Some(expected)=&row.after {
                let data=crate::identity_store::read_optional(&folder.join("after").join(&row.path))?.ok_or("配置暂存缺失")?;
                if &sha(&data)!=expected {return Err("配置暂存摘要不匹配".into());}
                atomic(&path,&data)?;
            }else if path.is_file(){std::fs::remove_file(path).map_err(|_|"旧配置替换失败")?;}
        }
        Ok::<(),String>(())
    })();
    if let Err(error)=result {rollback(root,&journal)?;std::fs::remove_file(root.join(JOURNAL)).map_err(|_|"配置回滚标记清理失败")?;return Err(format!("{error}；已恢复原配置仓"));}
    drop(_lock);
    if let Err(error)=activate(){
        {let _lock=crate::storage_context::EDIT.lock().map_err(|_|"配置回滚锁不可用")?;rollback(root,&journal)?;std::fs::remove_file(root.join(JOURNAL)).map_err(|_|"配置回滚标记清理失败")?;}
        activate().map_err(|_|"原配置仓已恢复，请重启 Dock 恢复服务")?;
        return Err(format!("{error}；已恢复导入前整个配置仓"));
    }
    journal.committed=true;
    atomic(&root.join(JOURNAL),&serde_json::to_vec(&journal).map_err(|_|"配置完成标记编码失败")?)?;
    std::fs::remove_file(root.join(JOURNAL)).map_err(|_|"配置已恢复，完成标记清理待重启")?;
    Ok(folder)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use crate::{storage_context::{Context, Namespace}, storage::Source};
    fn context() -> Context {
        static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let root = std::env::temp_dir().join(format!("fish-config-backup-{}-{}-{}", std::process::id(), crate::now_ms(), NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed))).join("中文 空格/.fish-dock");
        Context::open(root, env!("CARGO_PKG_VERSION")).unwrap()
    }
    fn fixture(context: &Context) -> Namespace {
        context.host.write("config", "config.json", br#"{"display":"island","islandPos":"left","settingsPin":false}"#).unwrap();
        let plugin = context.plugin("fish.feishu", ".fishplugin-feishu", Source::OfficialCatalog, "1.0.0", 1).unwrap();
        plugin.write("config", "notifications.json", b"synthetic-config").unwrap();
        plugin.write("credentials", "robot.json", &crate::mail_secrets::protect(b"synthetic-plugin-secret").unwrap()).unwrap();
        plugin.write("tmp", "cache.json", b"excluded-cache").unwrap();
        context.host.write("exports", "old.zip", b"excluded-export").unwrap();
        fs::write(context.host.path().join("mail-draft.json"), b"synthetic-draft").unwrap();
        plugin
    }
    #[test]
    fn whole_config_roundtrip_includes_host_and_plugins_without_runtime_or_recursive_backups() {
        let source = context(); let plugin = fixture(&source);
        let bytes = export(&source.root).unwrap();
        let preview = inspect(&source.root, &bytes).unwrap();
        assert_eq!(preview.owners.len(), 2);
        assert!(preview.files >= 4);
        assert!(!String::from_utf8_lossy(&bytes).contains("excluded-cache"));
        assert!(!String::from_utf8_lossy(&bytes).contains("excluded-export"));
        assert!(!String::from_utf8_lossy(&bytes).contains(&source.root.to_string_lossy().to_string()));
        let target = context();
        let backup = restore(&target.root, &bytes, || Ok(())).unwrap();
        assert!(backup.starts_with(target.host.path().join("backups")));
        assert_eq!(target.host.read("config", "config.json").unwrap(), source.host.read("config", "config.json").unwrap());
        let restored = Namespace::existing(&target.root.join(".fishplugin-feishu")).unwrap().unwrap();
        assert_eq!(restored.read("config", "notifications.json").unwrap(), plugin.read("config", "notifications.json").unwrap());
        assert_eq!(crate::mail_secrets::unprotect(&restored.read("credentials", "robot.json").unwrap().unwrap()).unwrap(), b"synthetic-plugin-secret");
        assert_eq!(fs::read(target.host.path().join("mail-draft.json")).unwrap(), b"synthetic-draft");
    }
    #[test]
    fn failed_activation_rolls_back_all_namespaces_and_keeps_unrelated_files() {
        let source = context(); fixture(&source);
        let bytes = export(&source.root).unwrap();
        let target = context();
        let plugin = fixture(&target);
        target.host.write("config", "config.json", br#"{"display":"taskbar"}"#).unwrap();
        plugin.write("config", "notifications.json", b"old-plugin").unwrap();
        fs::write(target.root.join("mine.txt"), b"unrelated").unwrap();
        let mut calls = 0;
        let result = restore(&target.root, &bytes, || { calls += 1; if calls == 1 { Err("synthetic activation failure".into()) } else { Ok(()) } });
        assert!(result.is_err()); assert_eq!(calls, 2);
        assert_eq!(target.host.read("config", "config.json").unwrap().unwrap(), br#"{"display":"taskbar"}"#);
        assert_eq!(plugin.read("config", "notifications.json").unwrap().unwrap(), b"old-plugin");
        assert_eq!(fs::read(target.root.join("mine.txt")).unwrap(), b"unrelated");
    }
    #[test]
    fn rollback_of_new_namespace_allows_install_after_failed_import() {
        let source=context(); fixture(&source); let bytes=export(&source.root).unwrap(); let target=context();
        let mut count=0;
        assert!(restore(&target.root,&bytes,||{count+=1;if count==1{Err("synthetic failed activation".into())}else{Ok(())}}).is_err());
        assert!(target.plugin("fish.feishu",".fishplugin-feishu",Source::OfficialCatalog,"1.0.0",1).is_ok());
    }
    #[test]
    fn interrupted_restore_is_rolled_back_before_context_opens() {
        let context=context(); fixture(&context);
        let id="config-interrupted".to_string(); let folder=context.root.join("host/backups").join(&id);
        let before=br#"{"display":"island"}"#.to_vec(); let after=br#"{"display":"taskbar"}"#.to_vec();
        let name="host/config/config.json".to_string(); atomic(&folder.join("before").join(&name),&before).unwrap();
        let journal=Journal{version:1,id,committed:false,owners:owners(&context.root).unwrap(),rows:vec![Row{path:name.clone(),before:Some(sha(&before)),after:Some(sha(&after))}]};
        atomic(&context.root.join(&name),&after).unwrap();atomic(&context.root.join(JOURNAL),&serde_json::to_vec(&journal).unwrap()).unwrap();
        Context::open(context.root.clone(),env!("CARGO_PKG_VERSION")).unwrap();
        assert_eq!(context.host.read("config","config.json").unwrap().unwrap(),before);
        assert!(!context.root.join(JOURNAL).exists());
    }
    #[test]
    fn windhawk_user_sources_are_backed_up_but_engine_is_excluded() {
        let context=context();fixture(&context);
        context.host.write("config","windhawk/data/ModsSource/example.wh.cpp",b"user-source").unwrap();
        context.host.write("config","windhawk/Engine/engine.dll",b"runtime-excluded").unwrap();
        let bytes=export(&context.root).unwrap();let files=crate::identity_archive::unzip_limits(&bytes,|_|true,LIMIT,FILE_LIMIT,ENTRIES).unwrap();
        assert!(files.contains_key("host/config/windhawk/data/ModsSource/example.wh.cpp"));
        assert!(!files.contains_key("host/config/windhawk/Engine/engine.dll"));
    }
    #[test]
    fn plugin_portable_data_and_host_custom_assets_are_included_without_binary_copies() {
        let context=context();fixture(&context);
        let magpie=context.plugin("fish.magpie",".fishplugin-magpie",Source::OfficialCatalog,"1.0.1",1).unwrap();
        magpie.write("credentials","magpie-runtime/data/config.json",b"synthetic-magpie-profile").unwrap();
        magpie.write("credentials","magpie-runtime/magpie.exe",b"excluded-runtime").unwrap();
        for name in ["pets/custom/sprite.png","plugins/weather.ps1"] {let file=context.host.path().join(name);fs::create_dir_all(file.parent().unwrap()).unwrap();fs::write(file,b"user-asset").unwrap();}
        let bytes=export(&context.root).unwrap();let files=crate::identity_archive::unzip_limits(&bytes,|_|true,LIMIT,FILE_LIMIT,ENTRIES).unwrap();
        assert!(files.contains_key(".fishplugin-magpie/credentials/magpie-runtime/data/config.json"));
        assert!(files.contains_key("host/pets/custom/sprite.png"));assert!(files.contains_key("host/plugins/weather.ps1"));
        assert!(!files.contains_key(".fishplugin-magpie/credentials/magpie-runtime/magpie.exe"));
    }
    #[test]
    fn tampering_and_a_different_schema_are_rejected_before_overwrite() {
        let source = context(); fixture(&source);
        let bytes = export(&source.root).unwrap();
        let mut files = crate::identity_archive::unzip_limits(&bytes, |_| true, LIMIT, FILE_LIMIT, 8192).unwrap();
        files.insert("host/config/config.json".into(), b"tampered".to_vec());
        assert!(inspect(&source.root, &crate::identity_archive::zip(&files).unwrap()).is_err());
        let target = context();
        target.plugin("fish.feishu", ".fishplugin-feishu", Source::OfficialCatalog, "2.0.0", 2).unwrap();
        assert!(restore(&target.root, &bytes, || Ok(())).is_err());
        assert!(target.host.read("config", "config.json").unwrap().is_none());
    }
    // 插件升级过也能恢复：版本号只是标签，数据的形状（schema）才必须一致。
    #[test]
    fn a_backup_from_an_older_plugin_version_still_restores() {
        let source = context(); fixture(&source);
        let bytes = export(&source.root).unwrap();
        let target = context();
        let plugin = target.plugin("fish.feishu", ".fishplugin-feishu", Source::OfficialCatalog, "2.0.0", 1).unwrap();
        plugin.write("config", "notifications.json", b"old-plugin").unwrap();
        restore(&target.root, &bytes, || Ok(())).unwrap();
        let restored = Namespace::existing(&target.root.join(".fishplugin-feishu")).unwrap().unwrap();
        assert_eq!(restored.identity().plugin_version, "2.0.0", "导入不改目标机器已装的版本");
        assert_eq!(restored.read("config", "notifications.json").unwrap().unwrap(), b"synthetic-config");
        assert_eq!(target.host.read("config", "config.json").unwrap(), source.host.read("config", "config.json").unwrap());
    }
    // 运行态与可重新下载的东西不进备份：凭据还在，垃圾不留。
    #[test]
    fn runtime_records_and_rewritable_assets_stay_out_of_the_backup() {
        let context = context(); fixture(&context);
        let magpie = context.plugin("fish.magpie", ".fishplugin-magpie", Source::OfficialCatalog, "1.0.1", 1).unwrap();
        for (name, body) in [
            ("magpie-runtime/data/trash/sessions/a/session.json", b"excluded-trash".as_slice()),
            ("magpie-runtime/data/routing/2026-10-28.jsonl", b"excluded-routing".as_slice()),
            ("magpie-runtime/data/models.json", b"excluded-models".as_slice()),
            ("magpie-runtime/data/settings.json.bak", b"excluded-backup".as_slice()),
            ("magpie/plugins/dist/plugin.tgz", b"excluded-tgz".as_slice()),
            ("magpie/logins.json", b"synthetic-login".as_slice()),
        ] {
            magpie.write("credentials", name, body).unwrap();
        }
        let bytes = export(&context.root).unwrap();
        let files = crate::identity_archive::unzip_limits(&bytes, |_| true, LIMIT, FILE_LIMIT, ENTRIES).unwrap();
        assert!(files.contains_key(".fishplugin-magpie/credentials/magpie/logins.json"), "凭据照旧带走");
        for name in ["trash", "routing", "models.json", "settings.json.bak", ".tgz"] {
            assert!(!files.keys().any(|key| key.contains(name)), "运行态 {name} 不进备份");
        }
    }
    // 别的用户、别的机器留下的 DPAPI：原样带走，不让整包导出失败。
    #[test]
    fn an_unreadable_credential_is_carried_as_is_instead_of_failing_the_export() {
        let source = context(); fixture(&source);
        let mut foreign = vec![1u8, 0, 0, 0, 0xd0, 0x8c, 0x9d, 0xdf, 1, 0x15, 0xd1, 0x11, 0x8c, 0x7a, 0, 0xc0, 0x4f, 0xc2, 0x97, 0xeb];
        foreign.extend_from_slice(b"not-this-users-dpapi");
        source.host.write("keys", "foreign.dpapi", &foreign).unwrap();
        let (bytes, opaque) = export_with_report(&source.root).unwrap();
        assert_eq!(opaque, 1, "解不开的凭据计入待重新登录数");
        let files = crate::identity_archive::unzip_limits(&bytes, |_| true, LIMIT, FILE_LIMIT, ENTRIES).unwrap();
        assert_eq!(files.get("host/keys/foreign.dpapi").unwrap(), &foreign, "原样带走");
        assert_eq!(inspect(&source.root, &bytes).unwrap().missing, 1);
        let target = context();
        restore(&target.root, &bytes, || Ok(())).unwrap();
        assert_eq!(target.host.read("keys", "foreign.dpapi").unwrap().unwrap(), foreign);
    }
    // 宿主的归属记录只由这台机器写：备份里的那份不覆盖它。
    #[test]
    fn the_host_ownership_record_is_never_rewritten_from_a_backup() {
        let source = context(); fixture(&source);
        let bytes = export(&source.root).unwrap();
        let target = context();
        let before = fs::read(target.host.path().join("storage.json")).unwrap();
        restore(&target.root, &bytes, || Ok(())).unwrap();
        assert_eq!(fs::read(target.host.path().join("storage.json")).unwrap(), before);
    }
}
