//! 可恢复安装注册表：先核验资源，最后切换活动版本；卸载保留用户命名空间。
use std::{collections::BTreeMap, path::PathBuf};
use serde::{Deserialize, Serialize};
use crate::{plugin_manifest::Manifest, plugin_package::Package, storage::Source, storage_context::{Context, Namespace}};
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all="camelCase")]
pub struct Installed { pub manifest: Manifest, pub digest: String, pub official: bool, pub enabled: bool,
    // 随 Dock 启动就把它的本机服务拉起来（旧注册表没有这一项时按开处理）。
    #[serde(default = "desired_autostart")] pub autostart: bool }
fn desired_autostart() -> bool { true }
#[derive(Default,Serialize,Deserialize)]
#[serde(deny_unknown_fields)]
struct Registry { schema: u32, entries: BTreeMap<String, Installed> }
pub fn platform() -> String { format!("{}-{}",std::env::consts::OS,std::env::consts::ARCH) }
fn source(row: &Installed) -> Source { if row.official { Source::OfficialCatalog } else { Source::ThirdParty } }
pub struct Manager<'a> { pub context: &'a Context }
static EDIT_REGISTRY:std::sync::Mutex<()>=std::sync::Mutex::new(());
impl Manager<'_> {
    fn registry(&self) -> Result<Registry,String> {
        let Some(bytes) = self.context.host.read("config","installed-plugins.json")? else { return Ok(Registry { schema:1,entries:BTreeMap::new() }); };
        let registry: Registry = serde_json::from_slice(&bytes).map_err(|_| "插件安装注册表损坏")?;
        if registry.schema != 1 || registry.entries.len() > 100 { return Err("插件安装注册表版本或数量无效".into()); }
        for (id,row) in &registry.entries {
            row.manifest.validate(&self.context.root,source(row),&platform())?;
            if id != &row.manifest.id || row.digest.len() != 64 || !row.digest.bytes().all(|b| b.is_ascii_hexdigit()) { return Err("插件注册归属或摘要无效".into()); }
        }
        Ok(registry)
    }
    fn save(&self, registry: &Registry) -> Result<(),String> { self.context.host.write("config","installed-plugins.json",&serde_json::to_vec_pretty(registry).map_err(|_| "插件注册表编码失败")?) }
    pub fn list(&self) -> Result<Vec<Installed>,String> { Ok(self.registry()?.entries.into_values().collect()) }
    pub fn directory(&self, row:&Installed) -> Result<PathBuf,String> {
        row.manifest.validate(&self.context.root,source(row),&platform())?;
        if row.digest.len()!=64 || !row.digest.bytes().all(|b|b.is_ascii_hexdigit()) { return Err("插件包摘要无效".into()); }
        crate::plugin_manifest::package_path(self.context.host.path(),&format!("packages/{}/{}/{}",row.manifest.id,row.manifest.version,row.digest.to_ascii_lowercase()))
    }
    pub fn namespace(&self,row:&Installed) -> Result<Namespace,String> {
        let m=&row.manifest;
        let root=crate::storage::namespace(&self.context.root,&m.id,&m.storage.directory,source(row))?;
        match Namespace::existing(&root)? {
            Some(ns) => crate::storage_version::change(&ns,&m.version,m.storage.data_schema),
            None => self.context.plugin(&m.id,&m.storage.directory,source(row),&m.version,m.storage.data_schema),
        }
    }
    pub fn verify(&self,row:&Installed) -> Result<(),String> {
        use sha2::{Digest,Sha256};
        let root=self.directory(row)?;
        let raw=crate::identity_store::read_optional(&root.join("plugin.json"))?.ok_or("已安装插件清单缺失")?;
        let manifest=Manifest::parse(&raw,&self.context.root,source(row),&platform())?;
        if serde_json::to_value(&manifest).ok()!=serde_json::to_value(&row.manifest).ok() { return Err("安装清单与注册表不一致".into()); }
        for (name,digest) in &manifest.files {
            let path=crate::plugin_manifest::package_path(&root,name)?;
            let bytes=crate::plugin_package::read(&path)?.ok_or("已安装插件资源缺失")?;
            if !format!("{:x}",Sha256::digest(&bytes)).eq_ignore_ascii_case(digest) { return Err(format!("已安装插件资源被改动：{name}")); }
        }
        Ok(())
    }
    pub fn install(&self, package:Package,digest:&str,official:bool) -> Result<Installed,String> {
        let _lock=EDIT_REGISTRY.lock().map_err(|_|"插件安装锁不可用")?;
        self.recover_unlocked()?;
        let mut registry=self.registry()?;
        let previous=registry.entries.get(&package.manifest.id).cloned();
        let row=Installed { manifest:package.manifest,digest:digest.to_ascii_lowercase(),official,enabled:previous.as_ref().map_or(true,|r|r.enabled),autostart:previous.as_ref().map_or(true,|r|r.autostart) };
        row.manifest.validate(&self.context.root,source(&row),&platform())?;
        let directory=self.directory(&row)?;
        // 发行文件仅写入摘要专属目录，旧版本始终可用。
        for (name,bytes) in package.files {
            let path=crate::plugin_manifest::package_path(&directory,&name)?;
            crate::plugin_package::write(self.context.host.path(),&path,&bytes)?;
        }
        self.verify(&row)?;
        let journal=serde_json::json!({"schema":1,"previous":previous,"next":row});
        let journal_name=format!("plugin-install-{}.json",row.manifest.id);
        self.context.host.write("tmp",&journal_name,&serde_json::to_vec(&journal).map_err(|_|"安装事务编码失败")?)?;
        self.namespace(&row)?;
        registry.entries.insert(row.manifest.id.clone(),row.clone());
        if let Err(error)=self.save(&registry) {
            if let Some(old)=&previous { self.namespace(old).map_err(|rollback|format!("{error}；旧版目录恢复待重试：{rollback}"))?; }
            return Err(error);
        }
        let marker=self.context.host.file("tmp",&journal_name)?;
        std::fs::remove_file(marker).map_err(|_|"插件安装已提交，事务清理待重试")?;
        Ok(row)
    }
    pub fn recover(&self) -> Result<(),String> {
        let _lock=EDIT_REGISTRY.lock().map_err(|_|"插件安装锁不可用")?;
        self.recover_unlocked()
    }
    fn recover_unlocked(&self) -> Result<(),String> {
        let registry=self.registry()?;
        let temporary=self.context.host.path().join("tmp");
        for entry in std::fs::read_dir(&temporary).map_err(|_|"插件事务目录读取失败")? {
            let path=entry.map_err(|_|"插件事务条目读取失败")?.path();
            let name=path.file_name().and_then(|n|n.to_str()).unwrap_or("");
            if !name.starts_with("plugin-install-") || !name.ends_with(".json") { continue; }
            let bytes=crate::plugin_package::read(&path)?.ok_or("插件事务缺失")?;
            let value:serde_json::Value=serde_json::from_slice(&bytes).map_err(|_|"插件事务损坏")?;
            if value["schema"]!=1 { return Err("插件事务版本无效".into()); }
            let next:Installed=serde_json::from_value(value["next"].clone()).map_err(|_|"插件事务下一版本无效")?;
            if name!=format!("plugin-install-{}.json",next.manifest.id) { return Err("插件事务身份不一致".into()); }
            if let Some(active)=registry.entries.get(&next.manifest.id) { self.verify(active)?; self.namespace(active)?; }
            crate::storage::safe(&path)?;
            std::fs::remove_file(path).map_err(|_|"插件事务清理失败")?;
        }
        Ok(())
    }
    pub(super) fn restore_registration(&self, row:&Installed) -> Result<(),String> {
        let _lock=EDIT_REGISTRY.lock().map_err(|_|"插件安装锁不可用")?;
        row.manifest.validate(&self.context.root,source(row),&platform())?;
        let mut registry=self.registry()?;
        self.namespace(row)?;
        registry.entries.insert(row.manifest.id.clone(),row.clone());self.save(&registry)
    }
    pub fn set_enabled(&self,id:&str,enabled:bool) -> Result<Installed,String> {
        let _lock=EDIT_REGISTRY.lock().map_err(|_|"插件安装锁不可用")?;
        self.recover_unlocked()?;
        let mut registry=self.registry()?;
        let row=registry.entries.get_mut(id).ok_or("插件未安装")?;
        if enabled { self.verify(row)?; self.namespace(row)?; }
        row.enabled=enabled; let row=row.clone(); self.save(&registry)?; Ok(row)
    }
    pub fn set_autostart(&self,id:&str,autostart:bool) -> Result<Installed,String> {
        let _lock=EDIT_REGISTRY.lock().map_err(|_|"插件安装锁不可用")?;
        self.recover_unlocked()?;
        let mut registry=self.registry()?;
        let row=registry.entries.get_mut(id).ok_or("插件未安装")?;
        row.autostart=autostart; let row=row.clone(); self.save(&registry)?; Ok(row)
    }
    pub fn uninstall(&self,id:&str) -> Result<(),String> {
        let _lock=EDIT_REGISTRY.lock().map_err(|_|"插件安装锁不可用")?;
        self.recover_unlocked()?;
        let mut registry=self.registry()?;
        if registry.entries.remove(id).is_none() { return Err("插件未安装".into()); }
        self.save(&registry)
    }
    // prune_versions 在升级提交后删掉过期版本目录，只留当前激活版本与它之前的
    // 一个（回退点）。以前每个版本都永久留在 packages/<id>/ 下，一次升级多一份
    // 几十 MB；旧目录还带着旧 manifest，一旦有路径拼错就会读到旧界面。
    // 只删 packages/<id>/ 下一层的版本目录，名字里带分隔符或点段的一律不碰。
    pub fn prune_versions(&self, active:&Installed) -> Result<Vec<String>,String> {
        let base=self.context.host.path().join("packages").join(&active.manifest.id);
        let entries=match std::fs::read_dir(&base) {
            Ok(entries)=>entries,
            Err(error) if error.kind()==std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(_)=>return Err("插件版本目录读取失败".into()),
        };
        let active_version=&active.manifest.version;
        let mut others:Vec<(std::time::SystemTime,String)>=Vec::new();
        for entry in entries {
            let entry=entry.map_err(|_|"插件版本目录条目读取失败")?;
            if !entry.file_type().map_err(|_|"插件版本目录类型读取失败")?.is_dir() { continue; }
            let name=entry.file_name().to_string_lossy().to_string();
            if name.is_empty()||name.contains('/')||name.contains('\\')||name.contains("..") { continue; }
            if &name==active_version { continue; }
            let stamp=entry.metadata().and_then(|meta|meta.modified()).unwrap_or(std::time::UNIX_EPOCH);
            others.push((stamp,name));
        }
        // 最近写入的那个非激活版本就是回退点，留它；更老的没有任何入口。
        others.sort_by(|a,b|b.0.cmp(&a.0).then_with(||b.1.cmp(&a.1)));
        let mut removed=Vec::new();
        for (_,name) in others.into_iter().skip(1) {
            std::fs::remove_dir_all(base.join(&name)).map_err(|_|format!("过期版本目录清理失败：{name}"))?;
            removed.push(name);
        }
        Ok(removed)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use sha2::{Digest,Sha256};
    fn bundle(version:&str,schema:u32)->(Package,String) {
        let payload=b"synthetic backend".to_vec();
        let readme=format!("# 合成插件 {version}").into_bytes();
        let raw=serde_json::json!({"schema":1,"id":"acme.weather","name":"合成天气","version":version,"hostApi":1,"platforms":[platform()],"storage":{"directory":".plugin.acme.weather","dataSchema":schema},"backend":{"runtime":"node","entry":"backend.mjs"},"files":{"backend.mjs":format!("{:x}",Sha256::digest(&payload)),"README.md":format!("{:x}",Sha256::digest(&readme))}});
        let files=BTreeMap::from([("plugin.json".into(),serde_json::to_vec(&raw).unwrap()),("backend.mjs".into(),payload),("README.md".into(),readme)]);
        let bytes=crate::identity_archive::zip(&files).unwrap();
        let digest=format!("{:x}",Sha256::digest(&bytes));
        let package=crate::plugin_package::inspect(&bytes,&digest,&std::env::temp_dir().join("fish-install-inspect"),Source::ThirdParty,&platform()).unwrap();
        (package,digest)
    }
    #[test] fn install_upgrade_failed_schema_recovery_and_uninstall_keep_user_data() {
        let root=std::env::temp_dir().join(format!("fish-install-{}-{}",std::process::id(),crate::now_ms())).join(".fish-dock");
        // 此用例验证安装事务，ACL 由 identity 的独立集成用例验证。
        fn namespace(root:PathBuf,id:&str,directory:&str)->Namespace {
            crate::storage::initialize(&root,id,"1.0.0",1).unwrap();
            let identity=crate::storage_context::Identity{plugin_id:id.into(),directory:directory.into(),plugin_version:"1.0.0".into(),data_schema:1};
            std::fs::write(root.join("storage.json"),serde_json::to_vec(&identity).unwrap()).unwrap();
            Namespace::existing(&root).unwrap().unwrap()
        }
        let host=namespace(root.join("host"),"host","host");
        namespace(root.join(".plugin.acme.weather"),"acme.weather",".plugin.acme.weather");
        let context=Context{root:root.clone(),host}; let manager=Manager{context:&context};
        let (package,digest)=bundle("1.0.0",1); let first=manager.install(package,&digest,false).unwrap();assert!(first.enabled);
        let ns=manager.namespace(&first).unwrap(); ns.write("keys","fixture.json",b"synthetic-key").unwrap();
        manager.set_enabled("acme.weather",true).unwrap();
        let (package,digest)=bundle("1.1.0",1); let next=manager.install(package,&digest,false).unwrap();
        assert!(next.enabled);assert_eq!(manager.namespace(&next).unwrap().read("keys","fixture.json").unwrap().unwrap(),b"synthetic-key");
        let (package,digest)=bundle("2.0.0",2);assert!(manager.install(package,&digest,false).is_err());
        manager.recover().unwrap();
        let active=manager.list().unwrap().remove(0);assert_eq!(active.manifest.version,"1.1.0");manager.verify(&active).unwrap();
        assert!(std::fs::read_to_string(ns.path().join("README.md")).unwrap().contains("1.1.0"));
        // 半写资源使校验失败，保留旧包与用户目录供恢复，避免激活受损版本。
        std::fs::write(manager.directory(&active).unwrap().join("backend.mjs"),b"modified").unwrap();
        assert!(manager.set_enabled("acme.weather",true).is_err());
        manager.uninstall("acme.weather").unwrap();assert!(manager.list().unwrap().is_empty());
        assert_eq!(std::fs::read(ns.path().join("keys/fixture.json")).unwrap(),b"synthetic-key");
        std::fs::remove_dir_all(root.parent().unwrap()).unwrap();
    }
    // 升级提交后只留当前版本与一个回退点：更老的版本目录没有任何入口，
    // 留着既占地方，也让"读到旧界面"这种事故有可乘之机。
    #[test] fn upgrade_keeps_active_and_one_rollback_version() {
        let root=std::env::temp_dir().join(format!("fish-prune-{}-{}",std::process::id(),crate::now_ms())).join(".fish-dock");
        fn namespace(root:PathBuf,id:&str,directory:&str)->Namespace {
            crate::storage::initialize(&root,id,"1.0.0",1).unwrap();
            let identity=crate::storage_context::Identity{plugin_id:id.into(),directory:directory.into(),plugin_version:"1.0.0".into(),data_schema:1};
            std::fs::write(root.join("storage.json"),serde_json::to_vec(&identity).unwrap()).unwrap();
            Namespace::existing(&root).unwrap().unwrap()
        }
        let host=namespace(root.join("host"),"host","host");
        namespace(root.join(".plugin.acme.weather"),"acme.weather",".plugin.acme.weather");
        let context=Context{root:root.clone(),host}; let manager=Manager{context:&context};
        for version in ["1.0.0","1.1.0","1.2.0"] {
            let (package,digest)=bundle(version,1);
            manager.install(package,&digest,false).unwrap();
            // 让目录写入时间有先后，避免依赖文件系统时钟精度来定回退点。
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        let active=manager.list().unwrap().into_iter().find(|row|row.manifest.id=="acme.weather").unwrap();
        assert_eq!(active.manifest.version,"1.2.0");
        let removed=manager.prune_versions(&active).unwrap();
        assert_eq!(removed,vec!["1.0.0".to_string()]);
        let mut left:Vec<String>=std::fs::read_dir(root.join("host").join("packages").join("acme.weather")).unwrap()
            .map(|entry|entry.unwrap().file_name().to_string_lossy().to_string()).collect();
        left.sort();
        assert_eq!(left,vec!["1.1.0".to_string(),"1.2.0".to_string()]);
        // 空目录不是错误：没有旧版本时什么都不用删。
        assert!(manager.prune_versions(&active).unwrap().is_empty());
        std::fs::remove_dir_all(root.parent().unwrap()).unwrap();
    }
}
