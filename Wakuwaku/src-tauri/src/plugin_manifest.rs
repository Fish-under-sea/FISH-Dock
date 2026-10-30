//! 外部插件公开清单；官方身份来自商店来源，不接受插件自称官方。
use std::{collections::{BTreeMap, HashSet}, path::{Path, PathBuf}};
use serde::{Deserialize, Serialize};
use crate::storage::Source;
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct Manifest {
    pub schema: u32, pub id: String, pub name: String, pub version: String, pub host_api: u32,
    pub platforms: Vec<String>, pub storage: Storage, pub backend: Backend,
    #[serde(default)] pub capabilities: Vec<String>, #[serde(default)] pub pages: Vec<Page>,
    // 行为钩子（#22）：插件声明它接受哪些宿主提供的官方开关；插件自己不必
    // 再画一遍。旧包没有这一段，按默认（全 false）读。
    #[serde(default)] pub behaviors: Behaviors,
    pub files: BTreeMap<String, String>,
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
        if self.capabilities.len() > 16 || self.capabilities.iter().any(|c| !["storage", "events", "network", "services", "system-proxy"].contains(&c.as_str())) { return Err("插件声明了当前宿主不支持的能力".into()); }
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
}
