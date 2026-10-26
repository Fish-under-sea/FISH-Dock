//! 插件包校验与安装；发行文件和用户资料分别存放。
use std::{collections::BTreeMap, path::Path};
use sha2::{Digest, Sha256};
use crate::{plugin_manifest::Manifest, storage::Source};
pub const FILE_LIMIT:usize=128*1024*1024;
pub const TOTAL_LIMIT:usize=256*1024*1024;
pub fn read(path:&Path)->Result<Option<Vec<u8>>,String>{
 crate::storage::safe(path)?;
 match std::fs::metadata(path){Ok(meta) if meta.is_file()&&meta.len()<=FILE_LIMIT as u64=>{use std::io::Read;let mut bytes=Vec::new();std::fs::File::open(path).and_then(|file|file.take(FILE_LIMIT as u64+1).read_to_end(&mut bytes)).map_err(|_|"插件资源读取失败")?;if bytes.len()>FILE_LIMIT{return Err("插件资源超过大小限制".into());}Ok(Some(bytes))},Err(error) if error.kind()==std::io::ErrorKind::NotFound=>Ok(None),_=>Err("插件资源不是文件或超过大小限制".into())}
}
pub fn write(root:&Path,path:&Path,bytes:&[u8])->Result<(),String>{
 if !path.starts_with(root)||bytes.len()>FILE_LIMIT{return Err("插件文件越界或过大".into());}crate::storage::safe(path)?;
 let parent=path.parent().ok_or("插件文件路径无效")?;std::fs::create_dir_all(parent).map_err(|_|"插件资源目录创建失败")?;
 let temporary=parent.join(format!(".install-{}-{}",std::process::id(),crate::now_ms()));
 use std::io::Write;let mut file=std::fs::OpenOptions::new().write(true).create_new(true).open(&temporary).map_err(|_|"插件暂存失败")?;file.write_all(bytes).and_then(|_|file.sync_all()).map_err(|_|"插件写入失败")?;drop(file);
 if read(&temporary)?.as_deref()!=Some(bytes){return Err("插件写入校验失败".into());}crate::storage::replace(&temporary,path).map_err(|_|"插件资源替换失败".to_string())
}
pub struct Package { pub manifest: Manifest, pub files: BTreeMap<String, Vec<u8>> }
#[cfg(debug_assertions)]
pub fn development(directory:&Path,sha256:&str,data_root:&Path)->Result<Package,String>{
    if !directory.is_absolute(){return Err("开发目录须为绝对路径".into());}
    crate::storage::safe(directory)?;
    let bytes=read(&directory.join("plugin.json"))?.ok_or("开发目录缺少清单")?;
    if format!("{:x}",Sha256::digest(&bytes))!=sha256{return Err("开发清单摘要不符".into());}
    let manifest=Manifest::parse(&bytes,data_root,Source::OfficialCatalog,&crate::plugin_install::platform())?;
    let mut files=BTreeMap::from([("plugin.json".into(),bytes)]);let mut total=files["plugin.json"].len();
    for(name,digest)in &manifest.files{
        let file=crate::plugin_manifest::package_path(directory,name)?;
        let content=read(&file)?.ok_or("开发目录资源缺失")?;total+=content.len();
        if total>TOTAL_LIMIT||!format!("{:x}",Sha256::digest(&content)).eq_ignore_ascii_case(digest){return Err(format!("开发资源校验失败：{name}"));}
        files.insert(name.clone(),content);
    }
    if !files.contains_key("README.md"){return Err("开发目录缺少版本说明".into());}
    Ok(Package{manifest,files})
}
pub fn inspect(bytes: &[u8], sha256: &str, root: &Path, source: Source, platform: &str) -> Result<Package, String> {
    if sha256.len() != 64 || !sha256.bytes().all(|b| b.is_ascii_hexdigit()) || !format!("{:x}", Sha256::digest(bytes)).eq_ignore_ascii_case(sha256) { return Err("插件包 SHA-256 校验失败".into()); }
    let files = crate::identity_archive::unzip_limits(bytes, |name| crate::plugin_manifest::package_path(root, name).is_ok(),TOTAL_LIMIT,FILE_LIMIT,4096)?;
    let raw = files.get("plugin.json").ok_or("插件包缺少 plugin.json")?;
    let manifest = Manifest::parse(raw, root, source, platform)?;
    if files.len() != manifest.files.len() + 1 { return Err("插件包包含未声明的文件或缺少文件".into()); }
    for (name, digest) in &manifest.files {
        let content = files.get(name).ok_or("插件包缺少声明的资源")?;
        if !format!("{:x}",Sha256::digest(content)).eq_ignore_ascii_case(digest) { return Err(format!("插件资源校验失败：{name}")); }
    }
    if !files.contains_key("README.md") { return Err("插件包缺少当前版本 README.md".into()); }
    Ok(Package { manifest, files })
}
#[cfg(test)]
mod tests {
    use super::*;
    fn bytes() -> Vec<u8> {
        let backend = b"console.log('fixture')".to_vec();
        let page = b"<!doctype html><title>fixture</title>".to_vec();
        let readme = b"# Fixture 1.0.0".to_vec();
        let manifest = serde_json::json!({"schema":1,"id":"acme.weather","name":"合成天气","version":"1.0.0","hostApi":1,"platforms":["windows-x86_64"],"storage":{"directory":".plugin.acme.weather","dataSchema":1},"backend":{"runtime":"node","entry":"backend.mjs"},"capabilities":["storage"],"pages":[{"id":"main","title":"天气","entry":"page.html"}],"files":{"backend.mjs":format!("{:x}",Sha256::digest(&backend)),"page.html":format!("{:x}",Sha256::digest(&page)),"README.md":format!("{:x}",Sha256::digest(&readme))}});
        crate::identity_archive::zip(&BTreeMap::from([("plugin.json".into(), serde_json::to_vec(&manifest).unwrap()), ("backend.mjs".into(), backend), ("page.html".into(), page), ("README.md".into(), readme)])).unwrap()
    }
    #[test] fn official_release_archives_use_the_host_installation_validator() {
        let Some(folder) = std::env::var_os("FISH_PLUGIN_RELEASE_DIR") else { return; };
        let root = std::env::temp_dir().join("fish-official-release-contract");
        for key in ["feishu", "clash", "magpie", "ofm"] {
            let bytes = std::fs::read(Path::new(&folder).join(format!("{key}.fish-plugin"))).unwrap();
            let digest = format!("{:x}", Sha256::digest(&bytes));
            let package = inspect(&bytes, &digest, &root, Source::OfficialCatalog, "windows-x86_64").unwrap();
            assert_eq!(package.manifest.id, format!("fish.{key}"));
            assert_eq!(package.manifest.version, "1.0.0");
            println!("{key}: {} resources verified", package.files.len());
        }
    }
    #[test] fn package_requires_both_archive_and_resource_checksums() {
        let root = std::env::temp_dir().join("fish-package-contract");
        let bytes = bytes(); let digest = format!("{:x}", Sha256::digest(&bytes));
        let package = inspect(&bytes, &digest, &root, Source::ThirdParty, "windows-x86_64").unwrap();
        assert_eq!(package.manifest.id, "acme.weather");
        assert!(inspect(&bytes, &"0".repeat(64), &root, Source::ThirdParty, "windows-x86_64").is_err());
        let mut files = crate::identity_archive::unzip_with(&bytes, |_| true).unwrap();
        files.insert("backend.mjs".into(), b"changed".to_vec());
        let changed = crate::identity_archive::zip(&files).unwrap();
        assert!(inspect(&changed, &format!("{:x}", Sha256::digest(&changed)), &root, Source::ThirdParty, "windows-x86_64").is_err());
    }
    #[test] fn traversal_and_undeclared_payloads_fail_before_unpacking() {
        let root = std::env::temp_dir().join(format!("fish-package-bad-{}",crate::now_ms()));
        for name in ["../escape", "unlisted-secret.json"] {
            let mut files = crate::identity_archive::unzip_with(&bytes(), |_| true).unwrap();
            files.insert(name.into(), b"synthetic".to_vec());
            let bytes = crate::identity_archive::zip(&files).unwrap();
            assert!(inspect(&bytes, &format!("{:x}",Sha256::digest(&bytes)), &root, Source::ThirdParty,"windows-x86_64").is_err());
            assert!(!root.exists());
        }
    }
}
