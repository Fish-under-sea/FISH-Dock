//! 统一身份 ZIP：内存生成、固定说明、格式预览及白名单导入。
use std::{collections::{BTreeMap, HashSet}, io::Read, path::Path};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

const LIMIT: usize = 32 * 1024 * 1024;
const FILE_LIMIT: usize = 16 * 1024 * 1024;
const README: &[u8] = include_bytes!("identity-backup-readme.md");
pub struct Archive { pub bytes: Vec<u8>, pub missing: usize }
pub struct Preview { pub format: String, pub ofm_accounts: usize, pub mail_accounts: Option<usize>, pub missing: usize, pub files: BTreeMap<String, Vec<u8>> }
fn sha(bytes: &[u8]) -> String { format!("{:x}", Sha256::digest(bytes)) }
fn allowed(name: &str) -> bool {
    crate::identity_store::OFM_FILES.contains(&name) || ["README.md", "README.txt", "manifest.json", "mail-accounts.json", "mail-secrets.json", "channel-pack/accounts-backup.json"].contains(&name)
}
fn encoded(value: &Value) -> Result<Vec<u8>, String> { serde_json::to_vec_pretty(value).map_err(|_| "无法编码身份资料".into()) }
fn document(files: &BTreeMap<String, Vec<u8>>, name: &str) -> Result<Option<Value>, String> {
    files.get(name).map(|bytes| {
        let value: Value = serde_json::from_slice(bytes).map_err(|_| "备份中的 JSON 格式无效")?;
        if !value.is_object() { return Err("备份中的身份资料不是 JSON 对象".into()); }
        Ok(value)
    }).transpose()
}
fn counts(files: &BTreeMap<String, Vec<u8>>, full: bool) -> Result<(usize, Option<usize>, usize), String> {
    for name in files.keys().filter(|name| name.ends_with(".json")) { document(files, name)?; }
    let state = document(files, "channel-pack/state.json")?.unwrap_or_else(|| json!({"accounts":[]}));
    let accounts = state["accounts"].as_array().ok_or("OFM 账号索引格式无效")?;
    if accounts.len() > 10000 || accounts.iter().any(|a| !a.is_object()) { return Err("OFM 账号索引超出限制或格式无效".into()); }
    let credentials = document(files, "channel-credentials.json")?.unwrap_or_else(|| json!({}));
    if credentials.as_object().unwrap().iter().any(|(id, value)| !crate::mail_secrets::valid_id(id) || value.as_str().is_none_or(|s| s.len() > 1024 * 1024)) { return Err("OFM 凭据映射格式无效".into()); }
    let mut missing = accounts.iter().filter(|a| a["credentialRef"].as_str().is_some_and(|id| credentials.get(id).is_none())).count();
    if let Some(eac) = document(files, "eac-user.json")? { if eac["token"].as_str().is_none_or(|s| s.is_empty() || s.len() > 1024 * 1024) { return Err("EAC 授权格式无效".into()); } }
    let mail_count = if full {
        let mail = document(files, "mail-accounts.json")?.ok_or("完整身份包缺少邮件账号快照")?;
        if mail["format"] != "fish-dock/mail-accounts" || mail["version"] != 1 { return Err("邮件账号快照版本无效".into()); }
        let mail = crate::mail_identity::validate(&mail["accounts"])?;
        let secrets = document(files, "mail-secrets.json")?.ok_or("完整身份包缺少邮件凭据快照")?;
        if secrets["format"] != "fish-dock/mail-secrets" || secrets["version"] != 1 { return Err("邮件凭据快照版本无效".into()); }
        let entries = secrets["entries"].as_object().ok_or("邮件凭据映射格式无效")?;
        let ids: HashSet<&str> = mail.iter().map(|a| a["id"].as_str().unwrap()).collect();
        if entries.iter().any(|(id, secret)| !ids.contains(id.as_str()) || secret.as_str().is_none_or(|s| s.is_empty() || s.len() > 16384)) { return Err("邮件凭据包含无效或未关联的账号".into()); }
        missing += ids.iter().filter(|id| !entries.contains_key(**id)).count();
        Some(mail.len())
    } else {
        if files.contains_key("mail-accounts.json") || files.contains_key("mail-secrets.json") { return Err("旧 OFM 包不得包含邮件资料".into()); }
        None
    };
    Ok((accounts.len(), mail_count, missing))
}
fn manifest(files: &BTreeMap<String, Vec<u8>>, format: &str, missing: usize) -> Value {
    json!({"format":format,"version":1,"product":"FISH Dock","exportedAtUnixMs":crate::now_ms(),"encrypted":false,"missingCredentialCount":missing,"files":files.iter().map(|(name,bytes)|json!({"path":name,"bytes":bytes.len(),"sha256":sha(bytes)})).collect::<Vec<_>>()})
}
pub fn export(root: &Path) -> Result<Archive, String> {
    let _lock = crate::identity_store::EDIT.lock().map_err(|_| "身份操作锁不可用")?;
    crate::identity_store::recover_unlocked(root)?;
    let mut files = BTreeMap::from([("README.md".into(), README.to_vec())]);
    for name in crate::identity_store::OFM_FILES.iter().chain(["mail-accounts.json"].iter()) {
        if let Some(bytes) = crate::identity_store::read_optional(&root.join(name))? { files.insert((*name).into(), bytes); }
    }
    let secrets = crate::mail_secrets::snapshot(root)?;
    files.insert("mail-secrets.json".into(), encoded(&json!({"format":"fish-dock/mail-secrets","version":1,"entries":secrets}))?);
    let (_, _, missing) = counts(&files, true)?;
    files.insert("manifest.json".into(), encoded(&manifest(&files, "fish-dock/identity-backup", missing))?);
    Ok(Archive { bytes: zip(&files)?, missing })
}
pub fn inspect(bytes: &[u8]) -> Result<Preview, String> {
    let files = unzip(bytes)?;
    let manifest = document(&files, "manifest.json")?.ok_or("备份缺少清单")?;
    let format = manifest["format"].as_str().ok_or("备份格式标识无效")?;
    let full = match format { "fish-dock/identity-backup" => true, "fish-dock/ofm-backup" => false, _ => return Err("不支持的身份备份格式".into()) };
    if manifest["version"] != 1 || manifest["encrypted"] != false { return Err("不支持的身份备份版本或加密格式".into()); }
    if !files.contains_key("README.md") && !files.contains_key("README.txt") { return Err("备份缺少说明文件".into()); }
    if !full && files.values().map(Vec::len).sum::<usize>() > FILE_LIMIT { return Err("旧 OFM 包超过 16 MiB".into()); }
    let rows = manifest["files"].as_array().ok_or("备份清单文件列表无效")?;
    if rows.len() + 1 != files.len() { return Err("备份清单与文件数量不一致".into()); }
    let mut seen = HashSet::new();
    for row in rows {
        let name = row["path"].as_str().ok_or("备份清单路径无效")?;
        if name == "manifest.json" || !seen.insert(name) { return Err("备份清单包含重复路径".into()); }
        let file = files.get(name).ok_or("备份清单引用了缺失文件")?;
        if row["bytes"].as_u64() != Some(file.len() as u64) || row["sha256"].as_str() != Some(sha(file).as_str()) { return Err("备份文件大小或 SHA-256 校验失败".into()); }
    }
    let (ofm_accounts, mail_accounts, missing) = counts(&files, full)?;
    Ok(Preview { format: format.into(), ofm_accounts, mail_accounts, missing, files })
}
impl Preview {
    pub fn changes(&self) -> Result<crate::identity_store::Changes, String> {
        // 完整包替换白名单范围；旧 OFM 包不触碰邮件。不存在的可选项只在本范围内删除。
        let mut changes = crate::identity_store::OFM_FILES.iter().map(|name| ((*name).into(), self.files.get(*name).cloned())).collect::<crate::identity_store::Changes>();
        if self.mail_accounts.is_some() {
            let accounts = document(&self.files, "mail-accounts.json")?.ok_or("邮件账号缺失")?;
            let accounts = crate::mail_identity::validate(&accounts["accounts"])?;
            let secrets = document(&self.files, "mail-secrets.json")?.ok_or("邮件凭据缺失")?;
            let secrets = secrets["entries"].as_object().ok_or("邮件凭据格式无效")?.iter().map(|(id,v)| Ok((id.clone(), v.as_str().ok_or("邮件凭据不是字符串")?.to_owned()))).collect::<Result<BTreeMap<_,_>, String>>()?;
            changes.extend(crate::mail_identity::changes(&accounts, &secrets)?);
        }
        Ok(changes)
    }
}
fn short(bytes: &[u8], at: usize) -> Result<u16, String> { bytes.get(at..at+2).and_then(|b| b.try_into().ok()).map(u16::from_le_bytes).ok_or_else(|| "ZIP 数据被截断".into()) }
fn long(bytes: &[u8], at: usize) -> Result<u32, String> { bytes.get(at..at+4).and_then(|b| b.try_into().ok()).map(u32::from_le_bytes).ok_or_else(|| "ZIP 数据被截断".into()) }
fn unzip(bytes: &[u8]) -> Result<BTreeMap<String, Vec<u8>>, String> {
    if bytes.len() > LIMIT || bytes.len() < 22 { return Err("ZIP 文件大小无效或超过 32 MiB".into()); }
    let end = (bytes.len().saturating_sub(65557)..=bytes.len()-22).rev().find(|&p| long(bytes,p).ok() == Some(0x06054b50) && short(bytes,p+20).is_ok_and(|n| p+22+n as usize == bytes.len())).ok_or("ZIP 缺少有效目录")?;
    let n = short(bytes,end+10)? as usize;
    let start = long(bytes,end+16)? as usize;
    if short(bytes,end+4)? != 0 || short(bytes,end+6)? != 0 || short(bytes,end+8)? as usize != n || n == 0 || n > 128 || start.checked_add(long(bytes,end+12)? as usize) != Some(end) { return Err("ZIP 分卷、目录或条目数量无效".into()); }
    let mut p = start;
    let mut seen = HashSet::new();
    let mut files = BTreeMap::new();
    let mut total = 0usize;
    for _ in 0..n {
        if long(bytes,p)? != 0x02014b50 { return Err("ZIP 目录条目无效".into()); }
        let flags = short(bytes,p+8)?;
        let method = short(bytes,p+10)?;
        let packed = long(bytes,p+20)? as usize;
        let size = long(bytes,p+24)? as usize;
        let len = short(bytes,p+28)? as usize;
        let extra = short(bytes,p+30)? as usize;
        let comment = short(bytes,p+32)? as usize;
        let local = long(bytes,p+42)? as usize;
        let attr = long(bytes,p+38)?;
        let kind = (attr >> 16) & 0o170000;
        let raw = bytes.get(p+46..p+46+len).ok_or("ZIP 文件名被截断")?;
        let name = std::str::from_utf8(raw).map_err(|_| "ZIP 文件名不是 UTF-8")?;
        if !allowed(name) || !seen.insert(name.to_ascii_lowercase()) || flags & !0x808 != 0 || ![0,8].contains(&method) || short(bytes,p+34)? != 0 || ![0,0o100000].contains(&kind) || attr & 0x10 != 0 { return Err("ZIP 包含加密、链接、重复、越界或未允许的条目".into()); }
        total = total.checked_add(size).ok_or("ZIP 展开大小溢出")?;
        if size > FILE_LIMIT || total > LIMIT || size > packed.saturating_mul(1000).saturating_add(1024) { return Err("ZIP 展开大小或压缩比超过限制".into()); }
        if long(bytes,local)? != 0x04034b50 || short(bytes,local+6)? != flags || short(bytes,local+8)? != method { return Err("ZIP 本地头与目录不一致".into()); }
        let ln = short(bytes,local+26)? as usize;
        let lx = short(bytes,local+28)? as usize;
        if bytes.get(local+30..local+30+ln) != Some(raw) { return Err("ZIP 本地文件名与目录不一致".into()); }
        let at = local.checked_add(30+ln+lx).ok_or("ZIP 偏移溢出")?;
        let stop = at.checked_add(packed).filter(|&end| end <= start).ok_or("ZIP 内容越过目录")?;
        let packed = bytes.get(at..stop).ok_or("ZIP 内容被截断")?;
        let file = if method == 0 { packed.to_vec() } else {
            let mut out = Vec::with_capacity(size.min(65536));
            flate2::read::DeflateDecoder::new(packed).take(size as u64 + 1).read_to_end(&mut out).map_err(|_| "ZIP 解压失败")?; out
        };
        if file.len() != size || crc32fast::hash(&file) != long(bytes,p+16)? { return Err("ZIP 文件大小或 CRC 校验失败".into()); }
        files.insert(name.into(), file);
        p = p.checked_add(46+len+extra+comment).filter(|&p| p <= end).ok_or("ZIP 目录超出边界")?;
    }
    if p != end { return Err("ZIP 目录存在未声明条目".into()); }
    Ok(files)
}
fn zip(files: &BTreeMap<String, Vec<u8>>) -> Result<Vec<u8>, String> {
    if files.len() > 128 || files.values().any(|b| b.len() > FILE_LIMIT) || files.values().map(Vec::len).sum::<usize>() > LIMIT - 65536 { return Err("完整身份备份超过大小限制".into()); }
    let mut out = Vec::new();
    let mut directory = Vec::new();
    for (name, bytes) in files {
        let at = out.len() as u32;
        let crc = crc32fast::hash(bytes);
        let size = bytes.len() as u32;
        out.extend_from_slice(&0x04034b50u32.to_le_bytes());
        for n in [20u16,0x800,0,0,0] { out.extend_from_slice(&n.to_le_bytes()); }
        for n in [crc,size,size] { out.extend_from_slice(&n.to_le_bytes()); }
        out.extend_from_slice(&(name.len() as u16).to_le_bytes()); out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(name.as_bytes()); out.extend_from_slice(bytes);
        directory.extend_from_slice(&0x02014b50u32.to_le_bytes());
        for n in [20u16,20,0x800,0,0,0] { directory.extend_from_slice(&n.to_le_bytes()); }
        for n in [crc,size,size] { directory.extend_from_slice(&n.to_le_bytes()); }
        for n in [name.len() as u16,0,0,0,0] { directory.extend_from_slice(&n.to_le_bytes()); }
        directory.extend_from_slice(&0u32.to_le_bytes()); directory.extend_from_slice(&at.to_le_bytes()); directory.extend_from_slice(name.as_bytes());
    }
    let start = out.len() as u32;
    out.extend_from_slice(&directory);
    out.extend_from_slice(&0x06054b50u32.to_le_bytes());
    for n in [0u16,0,files.len() as u16,files.len() as u16] { out.extend_from_slice(&n.to_le_bytes()); }
    out.extend_from_slice(&(directory.len() as u32).to_le_bytes()); out.extend_from_slice(&start.to_le_bytes()); out.extend_from_slice(&0u16.to_le_bytes());
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    fn root() -> std::path::PathBuf {
        let p = std::env::temp_dir().join(format!("fish-dock-archive-{}-{}", std::process::id(), crate::now_ms()));
        fs::create_dir_all(&p).unwrap(); p
    }
    fn account() -> Value { json!({"id":"fixture-mail","address":"fixture@example.test","host":"imap.example.test","port":993,"security":"ssl","username":"fixture@example.test","on":false,"smtp":null,"name":"合成邮件"}) }
    #[test]
    fn unified_zip_roundtrip_has_plain_migration_secret_but_no_local_dpapi_or_external_files() {
        let root = root();
        fs::write(root.join("settings.json"), br#"{"forwardKey":"fixture-api","standaloneLanEnabled":false}"#).unwrap();
        fs::write(root.join("channel-credentials.json"), br#"{"fixture-ref":"fixture-token"}"#).unwrap();
        fs::create_dir(root.join("channel-pack")).unwrap();
        fs::write(root.join("channel-pack/state.json"), br#"{"accounts":[{"id":"fixture-ofm","credentialRef":"fixture-ref","provider":"buddy"}]}"#).unwrap();
        fs::write(root.join("stats.json"), br#"{"version":3,"days":{}}"#).unwrap();
        fs::write(root.join("debug.log"), b"never-export-this").unwrap();
        crate::mail_identity::initialize(&root, &json!([account()]), |_| Ok(Some("fixture-password".into()))).unwrap();
        let archive = export(&root).unwrap();
        let parsed = inspect(&archive.bytes).unwrap();
        assert_eq!(parsed.format, "fish-dock/identity-backup");
        assert_eq!(parsed.ofm_accounts, 1);
        assert_eq!(parsed.mail_accounts, Some(1));
        assert!(parsed.files.contains_key("README.md"));
        assert!(parsed.files.contains_key("stats.json"));
        assert!(!parsed.files.contains_key("mail-secrets.dpapi"));
        assert!(!parsed.files.contains_key("debug.log"));
        let secrets: Value = serde_json::from_slice(&parsed.files["mail-secrets.json"]).unwrap();
        assert_eq!(secrets["entries"]["fixture-mail"], "fixture-password");
        assert!(!String::from_utf8_lossy(&archive.bytes).contains("never-export-this"));
        assert_eq!(archive.missing, 0);
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn old_ofm_backup_only_changes_ofm_and_keeps_mail_byte_identical() {
        let root = root();
        crate::mail_identity::initialize(&root, &json!([account()]), |_| Ok(Some("kept-mail-password".into()))).unwrap();
        let before = fs::read(root.join("mail-secrets.dpapi")).unwrap();
        let mut files = BTreeMap::from([
            ("README.txt".into(), b"old fixed README".to_vec()),
            ("settings.json".into(), br#"{"forwardKey":"legacy-fixture"}"#.to_vec()),
            ("channel-pack/state.json".into(), br#"{"accounts":[]}"#.to_vec()),
            ("channel-credentials.json".into(), br#"{}"#.to_vec()),
        ]);
        files.insert("manifest.json".into(), encoded(&manifest(&files,"fish-dock/ofm-backup",0)).unwrap());
        let preview = inspect(&zip(&files).unwrap()).unwrap();
        assert_eq!(preview.mail_accounts,None);
        let changes = preview.changes().unwrap();
        assert!(!changes.contains_key("mail-accounts.json"));
        assert!(!changes.contains_key("mail-secrets.dpapi"));
        crate::identity_store::apply(&root,changes).unwrap();
        assert_eq!(fs::read(root.join("mail-secrets.dpapi")).unwrap(),before);
        assert_eq!(crate::mail_secrets::read(&root,"fixture-mail").unwrap().as_deref(),Some("kept-mail-password"));
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn forbidden_paths_links_size_bombs_and_checksum_changes_are_rejected() {
        for name in ["../settings.json","/settings.json","C:/settings.json","channel-pack/../settings.json","mail-secrets.dpapi",".credentials.yaml","Settings.json"] {
            assert!(inspect(&zip(&BTreeMap::from([(name.into(),b"{}".to_vec())])).unwrap()).is_err(),"{name}");
        }
        let files=BTreeMap::from([("README.md".into(),b"fixture".to_vec())]);
        let original=zip(&files).unwrap();
        let central=original.windows(4).position(|w|w==0x02014b50u32.to_le_bytes()).unwrap();
        let mut symlink=original.clone();
        symlink[central+38..central+42].copy_from_slice(&(0o120777u32<<16).to_le_bytes());
        assert!(unzip(&symlink).is_err());
        let mut bomb=original.clone();
        bomb[central+24..central+28].copy_from_slice(&((FILE_LIMIT+1)as u32).to_le_bytes());
        assert!(unzip(&bomb).is_err());
        let mut corrupt=original.clone();
        corrupt[30+"README.md".len()]^=1;
        assert!(unzip(&corrupt).is_err());
        let mut listed=BTreeMap::from([("README.md".into(),b"original".to_vec())]);
        listed.insert("manifest.json".into(),encoded(&manifest(&listed,"fish-dock/ofm-backup",0)).unwrap());
        listed.insert("README.md".into(),b"modified".to_vec());
        assert!(inspect(&zip(&listed).unwrap()).err().unwrap().contains("SHA-256"));
    }
    #[test]
    fn random_or_truncated_data_is_rejected_before_changes() {
        assert!(inspect(b"not a zip").is_err());
        assert!(inspect(&[0u8; 64]).is_err());
    }
}
