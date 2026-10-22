//! 原生身份导入导出事务；取消、失败与旧副本都有明确结果。
use std::{path::{Path,PathBuf},fs,io::Write};

pub fn save(path:Option<&Path>, bytes:&[u8])->Result<Option<PathBuf>,String> {
    let Some(path)=path else{return Ok(None)};
    if !path.is_absolute() || bytes.len()>32*1024*1024{return Err("导出路径或文件大小无效".into())}
    crate::identity_store::safe(path)?;
    let mut file=fs::OpenOptions::new().write(true).create_new(true).open(path).map_err(|_|"无法创建 ZIP；已有文件不会被覆盖，请选择新文件名")?;
    // 先限制空文件权限，再写入敏感内容；权限失败时文件仍为空。
    crate::identity::protect_file(path)?;
    file.write_all(bytes).and_then(|_|file.sync_all()).map_err(|_|"ZIP 保存未完成；请勿使用不完整的文件")?;
    let saved=crate::identity_store::read_optional(path)?.ok_or("ZIP 保存后无法回读")?;
    if saved!=bytes{return Err("ZIP 保存后校验失败".into())}
    Ok(Some(path.into()))
}
pub fn commit(root:&Path, changes:crate::identity_store::Changes, activate:impl FnMut()->Result<(),String>)->Result<PathBuf,String> {
    commit_with(root,changes,activate,||crate::identity::operation_guard(root))
}
fn commit_with(root:&Path, changes:crate::identity_store::Changes, mut activate:impl FnMut()->Result<(),String>, mut guard:impl FnMut()->Result<Box<dyn std::any::Any>,String>)->Result<PathBuf,String>{
    let backup={let _guard=guard()?;crate::identity_store::apply(root,changes)?};
    match activate(){
        Ok(())=>Ok(backup),
        Err(_)=>{
            {let _guard=guard()?;crate::identity_store::restore(root,&backup).map_err(|_|"新身份激活失败，自动恢复未完成；恢复副本仍在，请暂停身份功能并从副本恢复")?;}
            activate().map_err(|_|"已恢复原身份文件，但原服务仍未启动；请重新启动 FISH Dock")?;
            Err("新身份激活失败，已恢复导入前的身份与服务；恢复副本保留在 .backups".into())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn root()->PathBuf {
        static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let next = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let path=std::env::temp_dir().join(format!("fish-dock-transfer-{}-{}-{next}",std::process::id(),crate::now_ms()));
        fs::create_dir(&path).unwrap();path
    }
    #[test]
    fn cancelling_save_creates_nothing_and_existing_file_is_never_overwritten(){
        let root=root();
        assert_eq!(save(None,b"sensitive fixture").unwrap(),None);
        let file=root.join("existing.zip");fs::write(&file,b"kept").unwrap();
        assert!(save(Some(&file),b"replacement").is_err());
        assert_eq!(fs::read(&file).unwrap(),b"kept");
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn activation_failure_restores_target_and_reactivates_previous_state(){
        let root=root();fs::write(root.join("settings.json"),b"old settings").unwrap();
        let mut activations=0;
        let result=commit_with(&root,std::collections::BTreeMap::from([("settings.json".into(),Some(b"new settings".to_vec()))]),||{
            activations+=1;
            let expected=if activations==1{b"new settings".as_slice()}else{b"old settings".as_slice()};
            assert_eq!(fs::read(root.join("settings.json")).unwrap(),expected);
            if activations==1{Err("synthetic service start failure".into())}else{Ok(())}
        },||Ok(Box::new(())));
        assert!(result.err().unwrap().contains("已恢复"));
        assert_eq!(activations,2);
        assert_eq!(fs::read(root.join("settings.json")).unwrap(),b"old settings");
        assert!(root.join(".backups").is_dir());
        fs::remove_dir_all(root).unwrap();
    }
}
