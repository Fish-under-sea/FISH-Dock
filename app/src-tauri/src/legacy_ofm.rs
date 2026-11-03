//! 退役 OFM 业务的唯一事实源：命名空间、备份格式、资源前缀、开发目录与网关插件。
//!
//! 2.0.4 起宿主不再携带 OFM 服务载荷、开发自动安装、38080 网关配置、渠道注册或账号业务。
//! 这里只做识别与隔离：已安装的独立 `fish.ofm` 插件不自动停用、卸载、改名或迁移，用户资料
//! 不复制、不删除、不改写。新增官方目录不再提供它，安装器仍按前缀拒绝旧载荷回流。

/// 已从官方商店下架、但仍可能装在用户机器上的插件 ID。
pub const RETIRED_PLUGIN_ID: &str = "fish.ofm";
/// 旧安装使用的数据命名空间（只识别，不改写、不自动迁移）。
pub const RETIRED_NAMESPACES: &[&str] = &[".fishplugin-ourfreemodel"];
/// 旧宿主/旧插件导出的身份备份格式（只识别，不当作当前宿主导入路径）。
pub const RETIRED_BACKUP_FORMATS: &[&str] = &["fish-dock/ofm-backup"];
/// 安装资源里不得再出现的旧载荷前缀（与 `app/scripts/installer.mjs` 同值）。
pub const RETIRED_RESOURCE_PREFIXES: &[&str] = &["ofm/", "dsh-our-free-model/", "our-free-model/"];
/// 开发预览不再接受的目录名。
pub const RETIRED_DEV_DIRECTORIES: &[&str] = &["ofm"];
/// 允许打开本机 loopback 服务页面的已装插件；仍受票据、loopback 与身份校验约束。
pub const LEGACY_GATEWAY_PLUGINS: &[&str] = &["fish.ofm", "fish.magpie"];
/// 旧 OFM 服务写入 `service.lock` 的产品标识；仅用于迁移边界的占用判断。
pub const LEGACY_SERVICE_PRODUCT: &str = "our-free-model-standalone";
/// 旧 OFM 服务互斥管道的名字前缀；仅用于迁移边界的占用判断。
pub const LEGACY_SERVICE_LOCK: &str = "ofm-service-lock-";
/// OFM 业务资料文件名：只在识别旧命名空间/旧备份时使用，宿主目录不写、不生成。
pub const LEGACY_IDENTITY_FILES: &[&str] = &[
    "settings.json", "stats.json", "availability.json", "catalog.json", "channel-credentials.json", "eac-user.json",
    "channel-pack/state.json", "channel-pack/token-ledger.json", "channel-pack/permanent-locks.json",
    "channel-pack/dead-models.json", "channel-pack/ui-preferences.json", "channel-pack/auto-checkin.json", "channel-pack/gemini-sigs.json",
];

pub fn is_legacy_plugin_id(id: &str) -> bool { id == RETIRED_PLUGIN_ID }

pub fn is_legacy_namespace(directory: &str) -> bool {
    RETIRED_NAMESPACES.iter().any(|known| known.eq_ignore_ascii_case(directory))
}

pub fn is_legacy_format(format: &str) -> bool { RETIRED_BACKUP_FORMATS.contains(&format) }

pub fn is_retired_prefix(path: &str) -> bool {
    RETIRED_RESOURCE_PREFIXES.iter().any(|prefix| path.len() >= prefix.len() && path[..prefix.len()].eq_ignore_ascii_case(prefix))
}

pub fn is_retired_dev_directory(directory: &str) -> bool { RETIRED_DEV_DIRECTORIES.contains(&directory) }

pub fn is_legacy_gateway_plugin(id: &str) -> bool { LEGACY_GATEWAY_PLUGINS.contains(&id) }

/// 旧业务资料名：只用于识别，宿主备份/导出不得写出这些名字。
pub fn is_legacy_identity_file(name: &str) -> bool { LEGACY_IDENTITY_FILES.contains(&name) }

/// 旧服务锁的归属产品标识是否为本产品退役前写入的那一份。
pub fn is_legacy_service_product(product: Option<&str>) -> bool { product == Some(LEGACY_SERVICE_PRODUCT) }

/// 唯一退役命名空间；调用方不得再自己拼字符串。
pub fn retired_namespace() -> &'static str { RETIRED_NAMESPACES[0] }

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn identity_predicates_cover_the_retired_surface() {
        assert!(is_legacy_plugin_id("fish.ofm"));
        assert!(!is_legacy_plugin_id("fish.magpie"));
        assert!(is_legacy_namespace(".fishplugin-ourfreemodel"));
        assert!(is_legacy_namespace(".FishPlugin-OurFreeModel"), "Windows 路径不区分大小写");
        assert!(!is_legacy_namespace(".fishplugin-magpie"));
        assert!(is_legacy_format("fish-dock/ofm-backup"));
        assert!(!is_legacy_format("fish-dock/identity-backup"));
        assert!(is_retired_prefix("ofm/node/x"), "旧载荷前缀按大小写不敏感匹配");
        assert!(is_retired_prefix("DSH-our-free-model/model.bin"));
        assert!(is_retired_prefix("our-free-model/"));
        assert!(!is_retired_prefix("ofm"), "只有目录名、没有分隔符不算前缀命中");
        assert!(!is_retired_prefix("node/ofm/x"));
        assert!(is_retired_dev_directory("ofm"));
        assert!(!is_retired_dev_directory("magpie"));
        assert!(is_legacy_gateway_plugin("fish.ofm") && is_legacy_gateway_plugin("fish.magpie"));
        assert!(!is_legacy_gateway_plugin("acme.weather"));
        assert!(is_legacy_identity_file("channel-pack/state.json"));
        assert!(!is_legacy_identity_file("mail-accounts.json"), "邮件资料属于宿主，不是退役业务");
        assert!(is_legacy_service_product(Some("our-free-model-standalone")));
        assert!(!is_legacy_service_product(Some("fish-dock")));
        assert!(!is_legacy_service_product(None));
        assert_eq!(retired_namespace(), ".fishplugin-ourfreemodel");
    }
    // 安装器与宿主各有一份前缀表；两边必须同值，否则宿主放行的路径会被打包入口拒，
    // 或者打包入口挡住的东西宿主以为还能用。
    #[test]
    fn installer_prefixes_are_the_same_list() {
        let installer = include_str!("../../scripts/installer.mjs");
        for prefix in RETIRED_RESOURCE_PREFIXES { assert!(installer.contains(&format!("'{prefix}'")), "安装器缺少前缀 {prefix}"); }
        let count = installer.matches("'ofm/'").count() + installer.matches("'dsh-our-free-model/'").count() + installer.matches("'our-free-model/'").count();
        assert_eq!(count, RETIRED_RESOURCE_PREFIXES.len(), "安装器前缀表出现重复或额外项");
    }
}