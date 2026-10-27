//! 可恢复安装注册表：先核验资源，最后切换活动版本；卸载保留用户命名空间。
use std::{collections::BTreeMap, path::PathBuf};
use serde::{Deserialize, Serialize};
use crate::{plugin_manifest::Manifest, plugin_package::Package, storage::Source, storage_context::{Context, Namespace}};
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all="camelCase")]
pub struct Installed { pub manifest: Manifest, pub digest: String, pub official: bool, pub enabled: bool }
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
        let row=Installed { manifest:package.manifest,digest:digest.to_ascii_lowercase(),official,enabled:previous.as_ref().map_or(true,|r|r.enabled) };
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
    pub fn uninstall(&self,id:&str) -> Result<(),String> {
        let _lock=EDIT_REGISTRY.lock().map_err(|_|"插件安装锁不可用")?;
        self.recover_unlocked()?;
        let mut registry=self.registry()?;
        if registry.entries.remove(id).is_none() { return Err("插件未安装".into()); }
        self.save(&registry)
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
}
