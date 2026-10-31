//! 启动迁移只复制宿主身份与配置，插件数据保留在原地等待显式导入。
use std::{collections::BTreeMap, path::Path};
use serde_json::{json, Value};
use crate::storage_context::Namespace;

pub fn prepare(host: &Namespace, legacy_data: &Path, legacy_identity: &Path, mut legacy_secret: impl FnMut(&str) -> Result<Option<String>, String>) -> Result<(), String> {
    if host.read("config", "migration-v1.json")?.is_some() { return Ok(()); }
    if host.read("roster", "mail-accounts.json")?.is_some() || host.read("config", "config.json")?.is_some() {
        return Err("宿主目录已有资料但迁移标记缺失，请先恢复或导入，避免覆盖现有资料".into());
    }
    let mut settings: Value = match crate::identity_store::read_optional(&legacy_data.join("config.json"))? {
        Some(bytes) => serde_json::from_slice(&bytes).map_err(|_| "旧宿主配置 JSON 无效")?,
        None => json!({}),
    };
    if !settings.is_object() { return Err("旧宿主配置不是对象".into()); }
    let (accounts, secrets) = if crate::identity_store::read_optional(&legacy_identity.join("mail-accounts.json"))?.is_some() {
        let snapshot = crate::mail_identity::load(legacy_identity)?;
        (snapshot.accounts, crate::mail_secrets::snapshot(legacy_identity)?)
    } else {
        let accounts = crate::mail_identity::validate(settings.get("mail").unwrap_or(&json!([])))?;
        let mut secrets = BTreeMap::new();
        for account in &accounts {
            let id = account["id"].as_str().ok_or("旧邮件账号缺少标识")?;
            if let Some(secret) = legacy_secret(id)? { secrets.insert(id.to_owned(), secret); }
        }
        (accounts, secrets)
    };
    settings.as_object_mut().unwrap().remove("mail");
    let mut changes = BTreeMap::from([
        ("config/config.json".into(), Some(serde_json::to_vec_pretty(&settings).map_err(|_| "宿主配置编码失败")?)),
        ("config/migration-v1.json".into(), Some(serde_json::to_vec(&json!({"version":1,"sourceRetained":true})).map_err(|_| "迁移标记编码失败")?)),
    ]);
    for (name, bytes) in crate::mail_identity::changes(&accounts, &secrets)? {
        changes.insert(crate::identity_store::classified_name(&name)?, bytes);
    }
    crate::storage_transaction::apply(host, changes)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{fs, path::PathBuf};
    #[test]
    fn copies_host_only_and_restarts_without_touching_legacy_ofm_or_existing_target() {
        let root: PathBuf = std::env::temp_dir().join(format!("fish-bootstrap-{}-{}", std::process::id(), crate::now_ms()));
        let host_root = root.join(".fish-dock/host");
        crate::storage::initialize(&host_root, "host", "1.0.2", 1).unwrap();
        let owner = crate::storage_context::Identity { plugin_id:"host".into(), directory:"host".into(), plugin_version:"1.0.2".into(), data_schema:1 };
        fs::write(host_root.join("storage.json"), serde_json::to_vec(&owner).unwrap()).unwrap();
        let host = Namespace::existing(&host_root).unwrap().unwrap();
        let legacy = root.join("legacy"); fs::create_dir(&legacy).unwrap();
        fs::write(legacy.join("config.json"), br#"{"display":"island","mail":[]}"#).unwrap();
        fs::write(legacy.join("channel-credentials.json"), b"never-copy-ofm").unwrap();
        prepare(&host, &legacy, &legacy, |_| unreachable!()).unwrap();
        assert!(host.read("config", "config.json").unwrap().is_some());
        assert!(!host.path().join("credentials/channel-credentials.json").exists());
        assert_eq!(fs::read(legacy.join("channel-credentials.json")).unwrap(), b"never-copy-ofm");
        host.write("config", "config.json", b"{\"display\":\"taskbar\"}").unwrap();
        prepare(&host, &legacy, &legacy, |_| unreachable!()).unwrap();
        assert_eq!(host.read("config", "config.json").unwrap().unwrap(), b"{\"display\":\"taskbar\"}");
        fs::remove_dir_all(root).unwrap();
    }
}
