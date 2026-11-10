//! 外部插件公开清单；官方身份来自商店来源，不接受插件自称官方。
use std::{collections::{BTreeMap, HashSet}, path::{Path, PathBuf}};
use serde::{Deserialize, Serialize};
use crate::storage::Source;

/// 2.1.x 及更早版本的清单字段，2.2.0 删掉子岛后已退役。
///
/// 已安装的插件清单里**还留着**这些键（`islandSub` / `islandPanel`）。注册表是
/// 一条 JSON，一个条目的未知字段就会让整张表反序列化失败，表现为「插件安装
/// 注册表损坏」、插件列表为空、所有插件都不启动（2.2.x 的实际故障）。宿主升级
/// 不能让已装的旧插件失效，所以这些键必须容忍：读进来丢掉，不写回。
///
/// 只白名单这两个，**不是**放宽成「接受任意未知字段」——拼错的字段名仍然报错，
/// 免得插件「看起来装上了却不工作」。以后再退役字段，往这里加一行。
const RETIRED_FIELDS: &[&str] = &["islandSub", "islandPanel"];

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(try_from = "ManifestWire", rename_all = "camelCase")]
pub struct Manifest {
    pub schema: u32, pub id: String, pub name: String, pub version: String, pub host_api: u32,
    pub platforms: Vec<String>, pub storage: Storage, pub backend: Backend,
    #[serde(default)] pub capabilities: Vec<String>, #[serde(default)] pub pages: Vec<Page>,
    // 行为钩子（#22）：插件声明它接受哪些宿主提供的官方开关；插件自己不必
    // 再画一遍。旧包没有这一段，按默认（全 false）读。
    #[serde(default)] pub behaviors: Behaviors,
    pub files: BTreeMap<String, String>,
}

/// 反序列化的中间形状：字段与 `Manifest` 一致，另外用 `flatten` 把**所有**未知
/// 字段接住，交给 `TryFrom` 按退役白名单核对。这样清单读取的每一条路径（新装包、
/// 注册表、备份恢复）都自动受保护，不必逐个入口记得调校验。
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ManifestWire {
    schema: u32, id: String, name: String, version: String, host_api: u32,
    platforms: Vec<String>, storage: Storage, backend: Backend,
    #[serde(default)] capabilities: Vec<String>, #[serde(default)] pages: Vec<Page>,
    #[serde(default)] behaviors: Behaviors,
    files: BTreeMap<String, String>,
    #[serde(flatten)] unknown: BTreeMap<String, serde_json::Value>,
}

impl TryFrom<ManifestWire> for Manifest {
    type Error = String;
    fn try_from(wire: ManifestWire) -> Result<Self, Self::Error> {
        // 退役字段丢弃即可；其余未知字段一律拒绝，并指名道姓。
        if let Some(key) = wire.unknown.keys().find(|key| !RETIRED_FIELDS.contains(&key.as_str())) {
            return Err(format!("plugin.json 含未知字段 {key}"));
        }
        Ok(Self {
            schema: wire.schema, id: wire.id, name: wire.name, version: wire.version,
            host_api: wire.host_api, platforms: wire.platforms, storage: wire.storage,
            backend: wire.backend, capabilities: wire.capabilities, pages: wire.pages,
            behaviors: wire.behaviors, files: wire.files,
        })
    }
}
// Behaviors 是插件声明接的宿主钩子。autostart：接受「随 Dock 启动就拉起本机
// 服务」——宿主在它的插件卡上画那颗气泡开关，并在拉起后台时把
// FISH_PLUGIN_AUTOSTART=1 传下去。以后再加别的行为也往这里长。
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct Behaviors { #[serde(default)] pub autostart: bool }
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct Storage { pub directory: String, pub data_schema: u32 }
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Backend { pub runtime: String, pub entry: String, #[serde(default)] pub args: Vec<String> }
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Page { pub id: String, pub title: String, pub entry: String }
pub const API: u32 = 1;
pub fn version(value: &str) -> Result<[u32; 3], String> {
    let parts: Vec<_> = value.split('.').collect();
    if parts.len() != 3 { return Err("插件版本必须为 major.minor.patch".into()); }
    let mut numbers = [0; 3];
    for (i, part) in parts.iter().enumerate() {
        if part.is_empty() || (part.len() > 1 && part.starts_with('0')) || !part.bytes().all(|b| b.is_ascii_digit()) { return Err("插件版本格式无效".into()); }
        numbers[i] = part.parse().map_err(|_| "插件版本数字过大")?;
    }
    Ok(numbers)
}
pub fn package_path(root: &Path, name: &str) -> Result<PathBuf, String> {
    if name.is_empty() || name.len() > 240 || name.contains('\\') { return Err("插件资源相对路径无效".into()); }
    for part in name.split('/') {
        let stem = part.split('.').next().unwrap_or("").to_ascii_uppercase();
        if part.is_empty() || part == "." || part == ".." || part.ends_with(['.', ' ']) || part.chars().any(|c| c.is_control() || "<>:\"|?*".contains(c)) || ["CON", "PRN", "AUX", "NUL", "CONIN$", "CONOUT$"].contains(&stem.as_str()) || ["COM", "LPT"].iter().any(|p| stem.strip_prefix(p).is_some_and(|n| n.len() == 1 && matches!(n.as_bytes()[0], b'1'..=b'9'))) { return Err("插件资源路径包含保留名称或上级跳转".into()); }
    }
    let path = root.join(name);
    crate::storage::safe(&path)?;
    Ok(path)
}
impl Manifest {
    pub fn parse(bytes: &[u8], data_root: &Path, source: Source, platform: &str) -> Result<Self, String> {
        if bytes.len() > 256 * 1024 { return Err("插件清单超过大小限制".into()); }
        let manifest: Self = serde_json::from_slice(bytes).map_err(|_| "plugin.json 格式无效或含未知字段")?;
        manifest.validate(data_root, source, platform)?;
        Ok(manifest)
    }
    pub fn validate(&self, data_root: &Path, source: Source, platform: &str) -> Result<(), String> {
        if self.schema != 1 || self.host_api != API || self.storage.data_schema == 0 { return Err("插件清单、宿主 API 或数据 schema 不兼容".into()); }
        version(&self.version)?;
        if self.name.trim().is_empty() || self.name.len() > 100 || self.name.chars().any(char::is_control) { return Err("插件显示名称无效".into()); }
        crate::storage::namespace(data_root, &self.id, &self.storage.directory, source)?;
        if self.platforms.is_empty() || self.platforms.len() > 8 || !self.platforms.iter().any(|p| p == platform) { return Err("插件不支持当前系统与架构".into()); }
        if !["node", "native"].contains(&self.backend.runtime.as_str()) || self.backend.args.len() > 32 || self.backend.args.iter().any(|s| s.len() > 1024 || s.contains('\0')) { return Err("插件后端启动声明无效".into()); }
        // 2.2.0：`windows`（外部原生窗口）与 `file-drop`（接收文件拖入路径）都是独立
        // 能力，不与 `storage` 捆绑；没声明的插件既开不了窗也收不到路径。
        if self.capabilities.len() > 16 || self.capabilities.iter().any(|c| !["storage", "events", "network", "services", "system-proxy", "windows", "file-drop", "dock"].contains(&c.as_str())) { return Err("插件声明了当前宿主不支持的能力".into()); }
        let mut seen = HashSet::new();
        if self.capabilities.iter().any(|s| !seen.insert(s)) { return Err("插件能力重复".into()); }
        let mut names = HashSet::new();
        if self.files.is_empty() || self.files.len() > 4096 { return Err("插件资源清单为空或过大".into()); }
        for (name, hash) in &self.files {
            package_path(data_root, name)?;
            if name == "plugin.json" || !names.insert(name.to_ascii_lowercase()) || hash.len() != 64 || !hash.bytes().all(|b| b.is_ascii_hexdigit()) { return Err("插件资源清单摘要或路径无效".into()); }
        }
        if !self.files.contains_key(&self.backend.entry) { return Err("插件后端入口未列入资源清单".into()); }
        let mut ids = HashSet::new();
        if self.pages.len() > 10 { return Err("插件页面数量超过限制".into()); }
        for page in &self.pages {
            if page.id.is_empty() || page.id.len() > 64 || !page.id.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-') || !ids.insert(&page.id) || page.title.trim().is_empty() || page.title.len() > 80 || page.title.chars().any(char::is_control) || !self.files.contains_key(&page.entry) || !page.entry.ends_with(".html") { return Err("插件页面入口、标题或 ID 无效".into()); }
        }
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> Manifest { Manifest { schema: 1, id: "acme.weather".into(), name: "合成天气".into(), version: "1.2.3".into(), host_api: 1, platforms: vec!["windows-x86_64".into()], storage: Storage { directory: ".plugin.acme.weather".into(), data_schema: 1 }, backend: Backend { runtime: "node".into(), entry: "backend.mjs".into(), args: vec![] }, capabilities: vec!["storage".into()], pages: vec![Page { id: "weather".into(), title: "天气".into(), entry: "page.html".into() }], behaviors: Behaviors::default(), files: BTreeMap::from([("backend.mjs".into(), "0".repeat(64)), ("page.html".into(), "1".repeat(64))]) } }
    #[test] fn valid_manifest_and_paths_are_portable() {
        let root = std::env::temp_dir().join("fish-manifest-contract");
        let manifest = fixture();
        assert!(manifest.validate(&root, Source::ThirdParty, "windows-x86_64").is_ok());
        for bad in ["../escape", "a/../b", "/absolute", "C:/data", "CON.txt", "a\\b", "file.", "a//b"] { assert!(package_path(&root, bad).is_err(), "{bad}"); }
    }
    #[test] fn forged_official_names_and_incompatible_versions_fail_before_creation() {
        let root = std::env::temp_dir().join(format!("fish-manifest-{}", crate::now_ms()));
        let mut manifest = fixture(); manifest.storage.directory = ".fishplugin-ourfreemodel".into();
        assert!(manifest.validate(&root, Source::ThirdParty, "windows-x86_64").is_err());
        manifest = fixture(); manifest.host_api = 2;
        assert!(manifest.validate(&root, Source::ThirdParty, "windows-x86_64").is_err());
        manifest = fixture(); manifest.files.insert("PAGE.HTML".into(), "2".repeat(64));
        assert!(manifest.validate(&root, Source::ThirdParty, "windows-x86_64").is_err());
        assert!(!root.exists());
    }

    // 2.1.x 的插件清单里带着 islandSub / islandPanel（旧子岛的入口）。2.2.0 把
    // 子岛整个删掉后，这两个键就成了「未知字段」；deny_unknown_fields 会因此把
    // **整张注册表**读不出来（注册表是一条 JSON，一个条目坏就全坏），表现是
    // 插件列表为空、所有插件都不启动。已安装的旧插件不能因为宿主升级就失效。
    #[test] fn retired_2_1_island_fields_do_not_break_the_manifest() {
        let json = serde_json::json!({
            "schema": 1, "id": "fish.clash", "name": "Clash", "version": "2.2.0", "hostApi": 1,
            "platforms": ["windows-x86_64"],
            "storage": {"directory": ".fishplugin-clash", "dataSchema": 1},
            "backend": {"runtime": "node", "entry": "backend.mjs"},
            "capabilities": ["storage"],
            "files": {"backend.mjs": "0".repeat(64)},
            "islandSub": null,
            "islandPanel": null
        });
        let manifest: Manifest = serde_json::from_value(json).expect("2.1.x 留下的退役字段不该让清单读不出来");
        assert_eq!(manifest.id, "fish.clash");
        assert_eq!(manifest.capabilities, vec!["storage".to_string()], "其余字段照常读入");
        // 写回去时不再带这两个键：留下它们只会让下一版再踩一次。
        let written = serde_json::to_value(&manifest).unwrap();
        assert!(written.get("islandSub").is_none(), "退役字段不写回");
        assert!(written.get("islandPanel").is_none(), "退役字段不写回");
    }

    // 但真正的未知字段仍要拒绝：拼错的字段名不能让插件「看起来装上了却不工作」。
    #[test] fn genuinely_unknown_fields_are_still_rejected() {
        let json = serde_json::json!({
            "schema": 1, "id": "acme.notes", "name": "笔记", "version": "1.0.0", "hostApi": 1,
            "platforms": ["windows-x86_64"],
            "storage": {"directory": ".plugin.acme.notes", "dataSchema": 1},
            "backend": {"runtime": "node", "entry": "backend.mjs"},
            "files": {"backend.mjs": "0".repeat(64)},
            "capabilites": ["storage"]
        });
        assert!(serde_json::from_value::<Manifest>(json).is_err(), "拼错的字段名必须报错");
    }
}
