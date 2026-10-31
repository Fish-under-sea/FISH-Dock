//! 邮件身份归口；旧系统凭据只用于首次迁移，后续仅写统一身份目录。
use std::{collections::BTreeMap, path::Path};
use serde_json::{json, Value};

#[derive(Debug, Clone)]
pub struct Snapshot { pub accounts: Vec<Value>, pub missing: Vec<String> }

pub fn validate(accounts: &Value) -> Result<Vec<Value>, String> {
    let accounts = accounts.as_array().ok_or("邮件账号列表不是数组")?;
    if accounts.len() > 1000 { return Err("邮件账号数量超过限制".into()); }
    let mut ids = std::collections::HashSet::new();
    for account in accounts {
        let id = account["id"].as_str().filter(|id| crate::mail_secrets::valid_id(id)).ok_or("邮件账号 ID 无效")?;
        if !ids.insert(id) { return Err("邮件账号 ID 重复".into()); }
        if account.as_object().is_none_or(|a| a.keys().any(|key| !["id", "address", "name", "host", "port", "security", "username", "auth", "smtp", "on", "proxy"].contains(&key.as_str()))) { return Err("邮件账号包含未允许的字段".into()); }
        for key in ["address", "host", "username"] {
            if account[key].as_str().is_none_or(|s| s.is_empty() || s.len() > 1024 || s.chars().any(char::is_control)) { return Err("邮件账号字段无效".into()); }
        }
        if account["port"].as_u64().is_none_or(|p| p == 0 || p > 65535) || !["ssl", "starttls", "plain"].contains(&account["security"].as_str().unwrap_or("")) { return Err("邮件服务器端口或安全方式无效".into()); }
        if serde_json::to_vec(account).map_err(|_| "邮件账号格式无效")?.len() > 32 * 1024 { return Err("邮件账号资料超出大小限制".into()); }
    }
    Ok(accounts.clone())
}

fn load_unlocked(root: &Path) -> Result<Snapshot, String> {
    crate::identity_store::recover_unlocked(root)?;
    let bytes = crate::identity_store::read_optional(&crate::identity_store::path(root, "mail-accounts.json")?)?.ok_or("邮件身份尚未初始化")?;
    let saved: Value = serde_json::from_slice(&bytes).map_err(|_| "邮件账号文件格式无效")?;
    if saved["format"] != "fish-dock/mail-accounts" || saved["version"] != 1 { return Err("邮件账号文件版本无效".into()); }
    let accounts = validate(&saved["accounts"])?;
    let secrets = crate::mail_secrets::snapshot(root)?;
    let missing = accounts.iter().filter_map(|a| {
        let id = a["id"].as_str().unwrap();
        (!secrets.contains_key(id)).then(|| id.to_string())
    }).collect();
    Ok(Snapshot { accounts, missing })
}
pub fn load(root: &Path) -> Result<Snapshot, String> {
    let _lock = crate::identity_store::EDIT.lock().map_err(|_| "身份操作锁不可用")?;
    load_unlocked(root)
}

pub fn changes(accounts: &[Value], secrets: &BTreeMap<String, String>) -> Result<crate::identity_store::Changes, String> {
    let accounts = validate(&json!(accounts))?;
    let ids: std::collections::HashSet<&str> = accounts.iter().map(|a| a["id"].as_str().unwrap()).collect();
    if secrets.keys().any(|id| !ids.contains(id.as_str())) { return Err("邮件密码引用了未包含的账号".into()); }
    let missing: Vec<&str> = ids.iter().copied().filter(|id| !secrets.contains_key(*id)).collect();
    Ok(BTreeMap::from([
        ("mail-accounts.json".into(), Some(serde_json::to_vec(&json!({"format":"fish-dock/mail-accounts","version":1,"accounts":accounts,"missingCredentials":missing})).map_err(|_| "无法编码邮件账号")?)),
        ("mail-secrets.dpapi".into(), Some(crate::mail_secrets::encode(secrets)?)),
        ("identity-state.json".into(), Some(serde_json::to_vec(&json!({"format":"fish-dock/identity-state","version":1,"mailMigrated":true,"missingMailCredentials":missing.len()})).map_err(|_| "无法编码身份状态")?)),
    ]))
}

pub fn initialize(root: &Path, legacy: &Value, mut read_legacy: impl FnMut(&str) -> Result<Option<String>, String>) -> Result<Snapshot, String> {
    let _lock = crate::identity_store::EDIT.lock().map_err(|_| "身份操作锁不可用")?;
    crate::identity_store::recover_unlocked(root)?;
    if crate::identity_store::read_optional(&crate::identity_store::path(root, "mail-accounts.json")?)?.is_some() { return load_unlocked(root); }
    if crate::identity_store::read_optional(&crate::identity_store::path(root, "mail-secrets.dpapi")?)?.is_some() { return Err("邮件凭据存在但账号文件缺失，请先恢复身份副本".into()); }
    let empty = json!([]);
    let accounts = validate(if legacy.is_null() { &empty } else { legacy })?;
    let mut secrets = BTreeMap::new();
    for account in &accounts {
        let id = account["id"].as_str().unwrap();
        if let Some(secret) = read_legacy(id)? { secrets.insert(id.to_string(), secret); }
    }
    crate::identity_store::apply_unlocked(root, changes(&accounts, &secrets)?)?;
    load_unlocked(root)
}

pub fn replace(root: &Path, accounts: &[Value], updated: Option<(&str, &str)>) -> Result<Snapshot, String> {
    let _lock = crate::identity_store::EDIT.lock().map_err(|_| "身份操作锁不可用")?;
    crate::identity_store::recover_unlocked(root)?;
    let mut secrets = crate::mail_secrets::snapshot(root)?;
    let ids: std::collections::HashSet<&str> = accounts.iter().filter_map(|a| a["id"].as_str()).collect();
    secrets.retain(|id, _| ids.contains(id.as_str()));
    if let Some((id, secret)) = updated { secrets.insert(id.to_string(), secret.to_string()); }
    crate::identity_store::apply_unlocked(root, changes(accounts, &secrets)?)?;
    load_unlocked(root)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    fn root() -> std::path::PathBuf {
        static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let next = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!("fish-dock-mail-{}-{}-{next}", std::process::id(), crate::now_ms()));
        fs::create_dir(&path).unwrap(); path
    }
    fn account(id: &str) -> Value { json!({"id":id,"address":"fixture@example.test","host":"imap.example.test","port":993,"security":"ssl","username":"fixture@example.test","auth":"auto","on":true,"smtp":null,"proxy":null,"name":"合成账号"}) }
    #[test]
    fn migrates_nonempty_accounts_and_reads_back_encrypted_passwords() {
        let root = root();
        let original = json!([account("fixture-a"), account("fixture-b")]);
        let state = initialize(&root, &original, |id| Ok((id == "fixture-a").then(|| "synthetic-only".to_owned()))).unwrap();
        assert_eq!(state.accounts, original.as_array().unwrap().clone());
        assert_eq!(state.missing, vec!["fixture-b"]);
        assert_eq!(crate::mail_secrets::read(&root, "fixture-a").unwrap().as_deref(), Some("synthetic-only"));
        assert!(!String::from_utf8_lossy(&fs::read(root.join("mail-secrets.dpapi")).unwrap()).contains("synthetic-only"));
        let restored = initialize(&root, &json!([]), |_| panic!("旧库不能再次读取")).unwrap();
        assert_eq!(restored.accounts.len(), 2);
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn credential_read_failure_does_not_commit_partial_mail_identity() {
        let root = root();
        let result = initialize(&root, &json!([account("fixture-a")]), |_| Err("合成凭据读取失败".into()));
        assert!(result.is_err());
        assert!(!root.join("mail-accounts.json").exists());
        assert!(!root.join("mail-secrets.dpapi").exists());
        fs::remove_dir_all(root).unwrap();
    }
}
