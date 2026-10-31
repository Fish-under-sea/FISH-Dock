//! 启动解析一次的数据上下文；插件只取得已绑定身份的命名空间。
use std::{fs, path::{Path, PathBuf}, sync::{Mutex, OnceLock}};
use serde::{Deserialize, Serialize};
use crate::storage::{self, Source};

static CURRENT: OnceLock<Context> = OnceLock::new();
pub static EDIT: Mutex<()> = Mutex::new(());
pub const MAX_FILE: usize = 16 * 1024 * 1024;

#[derive(Clone, Debug)]
pub struct Context { pub root: PathBuf, pub host: Namespace }
#[derive(Clone, Debug)]
pub struct Namespace { pub(crate) root: PathBuf, pub(crate) identity: Identity }
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct Identity { pub plugin_id: String, pub directory: String, pub plugin_version: String, pub data_schema: u32 }

pub fn current() -> Option<&'static Context> { CURRENT.get() }
pub fn start(args: &[String]) -> Result<&'static Context, String> {
    if let Some(context) = CURRENT.get() { return Ok(context); }
    let home = storage::profile_directory()?;
    let environment = std::env::var_os("FISH_DOCK_HOME").map(PathBuf::from);
    let root = storage::startup_root(&home, args, environment.as_deref())?;
    let context = Context::open(root, env!("CARGO_PKG_VERSION"))?;
    CURRENT.set(context).map_err(|_| "存储上下文已由另一个启动流程设置")?;
    CURRENT.get().ok_or("存储上下文未就绪".into())
}
impl Context {
    pub fn open(root: PathBuf, host_version: &str) -> Result<Self, String> {
        storage::resolve_root(&root, Some(&root), None)?;
        crate::identity::prepare(&crate::identity::Location { root: root.clone(), legacy: None, warning: None })?;
        storage::initialize_root(&root, host_version)?;
        crate::config_backup::recover(&root)?;
        let host_path = root.join("host");
        let host = match Namespace::existing(&host_path)? {
            Some(previous) => crate::storage_version::change(&previous, host_version, 1)?,
            None => Namespace::bind(host_path, Identity { plugin_id: "host".into(), directory: "host".into(), plugin_version: host_version.into(), data_schema: 1 })?,
        };
        Ok(Self { root, host })
    }
    pub fn plugin(&self, id: &str, directory: &str, source: Source, version: &str, schema: u32) -> Result<Namespace, String> {
        let path = storage::namespace(&self.root, id, directory, source)?;
        Namespace::bind(path, Identity { plugin_id: id.into(), directory: directory.into(), plugin_version: version.into(), data_schema: schema })
    }
}
impl Namespace {
    pub fn existing(root: &Path) -> Result<Option<Self>, String> {
        crate::storage_version::recover(root)?;
        Self::claimed(root)
    }
    // 路径归类只读取原子归属记录；启动和安装入口负责版本恢复。
    // 已持有 EDIT 的归档过程也会走到这里，不能再次进入版本恢复锁。
    pub(crate) fn claimed(root: &Path) -> Result<Option<Self>, String> {
        let Some(bytes) = crate::identity_store::read_optional(&root.join("storage.json"))? else { return Ok(None); };
        let identity: Identity = serde_json::from_slice(&bytes).map_err(|_| "存储归属记录损坏")?;
        if identity.plugin_version.is_empty() || identity.data_schema == 0 { return Err("存储版本记录无效".into()); }
        if identity.plugin_id == "host" {
            if identity.directory != "host" || root.file_name().and_then(|n| n.to_str()) != Some("host") { return Err("宿主存储目录归属不匹配".into()); }
        } else {
            let source = if identity.plugin_id.starts_with("fish.") { Source::OfficialCatalog } else { Source::ThirdParty };
            let parent = root.parent().ok_or("插件目录缺少数据根")?;
            if storage::namespace(parent, &identity.plugin_id, &identity.directory, source)? != root { return Err("插件存储目录归属不匹配".into()); }
        }
        Ok(Some(Self { root: root.into(), identity }))
    }
    fn bind(root: PathBuf, identity: Identity) -> Result<Self, String> {
        let _lock = EDIT.lock().map_err(|_| "存储绑定锁不可用")?;
        let marker = root.join("storage.json");
        let previous = crate::identity_store::read_optional(&marker)?;
        if let Some(bytes) = &previous {
            let owner: Identity = serde_json::from_slice(bytes).map_err(|_| "存储归属记录损坏")?;
            if owner.plugin_id != identity.plugin_id || owner.directory != identity.directory || owner.data_schema != identity.data_schema {
                return Err("目录已属于其他插件或数据 schema 尚未迁移".into());
            }
            if owner.plugin_version != identity.plugin_version { return Err("插件版本变更必须通过安装更新事务，存储绑定不会自动升级".into()); }
        } else if root.exists() {
            // 无归属记录的非空目录只通过导入入口接收，避免接管其他资料。
            if fs::read_dir(&root).map_err(|_| "存储目录读取失败")?.next().is_some() { return Err("目标插件目录非空且缺少归属记录，请先导入或恢复".into()); }
        }
        crate::identity::prepare(&crate::identity::Location { root: root.clone(), legacy: None, warning: None })?;
        storage::initialize(&root, &identity.plugin_id, &identity.plugin_version, identity.data_schema)?;
        let namespace = Self { root, identity };
        if previous.is_none() { namespace.atomic(&marker, &serde_json::to_vec_pretty(&namespace.identity).map_err(|_| "存储归属记录编码失败")?)?; }
        Ok(namespace)
    }
    pub fn path(&self) -> &Path { &self.root }
    pub fn identity(&self) -> &Identity { &self.identity }
    pub fn file(&self, category: &str, name: &str) -> Result<PathBuf, String> { storage::category_file(&self.root, category, name) }
    pub fn read(&self, category: &str, name: &str) -> Result<Option<Vec<u8>>, String> {
        let _lock = EDIT.lock().map_err(|_| "存储读写锁不可用")?;
        crate::storage_transaction::recover_unlocked(self)?;
        crate::identity_store::read_optional(&self.file(category, name)?)
    }
    pub fn write(&self, category: &str, name: &str, bytes: &[u8]) -> Result<(), String> {
        let _lock = EDIT.lock().map_err(|_| "存储读写锁不可用")?;
        crate::storage_transaction::recover_unlocked(self)?;
        self.atomic(&self.file(category, name)?, bytes)
    }
    pub(crate) fn atomic(&self, path: &Path, bytes: &[u8]) -> Result<(), String> {
        if bytes.len() > MAX_FILE || !path.starts_with(&self.root) { return Err("存储文件超过限制或越过命名空间".into()); }
        storage::safe(path)?;
        let parent = path.parent().ok_or("存储文件缺少父目录")?;
        fs::create_dir_all(parent).map_err(|_| "存储子目录创建失败")?;
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let serial = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let temporary = self.root.join("tmp").join(format!("write-{}-{}-{serial}", std::process::id(), crate::now_ms()));
        // 数据根已由 Context::open 限定用户权限，文件继承它；不再次改写文件权限。
        use std::io::Write;
        let mut file = fs::OpenOptions::new().create_new(true).write(true).open(&temporary).map_err(|_| "存储暂存文件创建失败")?;
        file.write_all(bytes).and_then(|_| file.sync_all()).map_err(|_| "存储暂存文件写入失败")?;
        drop(file);
        if crate::identity_store::read_optional(&temporary)?.as_deref() != Some(bytes) { return Err("存储暂存文件校验失败".into()); }
        if let Err(error) = crate::storage::replace(&temporary, path) { return Err(format!("存储替换失败：{error}")); }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn root() -> PathBuf {
        static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        std::env::temp_dir().join(format!("fish-context-{}-{}-{}", std::process::id(), crate::now_ms(), NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed))).join("中文 路径/.fish-dock")
    }
    #[test]
    fn plugin_mount_generates_version_readmes_and_never_preinstalls_others() {
        let root = root();
        let context = Context::open(root.clone(), "1.0.2").unwrap();
        assert!(!root.join(".fishplugin-ourfreemodel").exists());
        let plugin = context.plugin("fish.ofm", ".fishplugin-ourfreemodel", Source::OfficialCatalog, "2.5.3", 1).unwrap();
        plugin.write("credentials", "channel-credentials.json", b"synthetic-only").unwrap();
        assert_eq!(plugin.read("credentials", "channel-credentials.json").unwrap().unwrap(), b"synthetic-only");
        assert!(!context.host.path().join("credentials/channel-credentials.json").exists());
        assert!(fs::read_to_string(plugin.path().join("keys/README.md")).unwrap().contains("2.5.3"));
        assert!(!root.join(".fishplugin-magpie").exists());
        fs::remove_dir_all(root.ancestors().nth(2).unwrap()).unwrap();
    }
    #[test]
    fn an_unclaimed_or_different_owned_directory_is_never_adopted() {
        let root = root();
        let context = Context::open(root.clone(), "1.0.2").unwrap();
        let occupied = root.join(".plugin.acme.weather");
        fs::create_dir_all(&occupied).unwrap(); fs::write(occupied.join("mine"), b"kept").unwrap();
        assert!(context.plugin("acme.weather", ".plugin.acme.weather", Source::ThirdParty, "1.0.0", 1).is_err());
        assert_eq!(fs::read(occupied.join("mine")).unwrap(), b"kept");
        let plugin = context.plugin("fish.ofm", ".fishplugin-ourfreemodel", Source::OfficialCatalog, "2.5.3", 1).unwrap();
        assert!(context.plugin("fish.ofm", ".fishplugin-ourfreemodel", Source::OfficialCatalog, "2.5.4", 2).is_err());
        assert_eq!(plugin.identity().plugin_version, "2.5.3");
        fs::remove_dir_all(root.ancestors().nth(2).unwrap()).unwrap();
    }
}
