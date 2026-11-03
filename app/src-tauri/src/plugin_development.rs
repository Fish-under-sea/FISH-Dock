//! 开发预览装载器：只在调试版、明确指定目录时启用，不改变发行版默认安装集合。
//! 2.0.4 起不再接受退役的 OFM 目录：宿主不代写 38080 网关配置，也不自动安装退役业务。
#[cfg(debug_assertions)]
pub fn prepare(context:&crate::storage_context::Context,runner:&mut crate::plugin_runtime::Runner,node:Option<&std::path::Path>)->Result<(),String>{
    use std::path::PathBuf;
    use serde::Deserialize;
    #[derive(Deserialize)]#[serde(deny_unknown_fields)]struct Index{schema:u32,plugins:Vec<Entry>}
    #[derive(Deserialize)]#[serde(deny_unknown_fields)]struct Entry{id:String,directory:String,sha256:String,executable:Option<String>}
    let Some(root)=std::env::var_os("FISH_DOCK_DEV_PLUGINS").map(PathBuf::from)else{return Ok(())};
    if !root.is_absolute(){return Err("开发预览目录须为绝对路径".into());}
    crate::storage::safe(&root)?;
    let bytes=crate::identity_store::read_optional(&root.join("development.json"))?.ok_or("开发预览目录缺少索引")?;
    let index:Index=serde_json::from_slice(&bytes).map_err(|_|"开发预览索引格式错误")?;
    if index.schema!=1||index.plugins.len()>20{return Err("开发预览索引版本或数量错误".into());}
    let manager=crate::plugin_install::Manager{context};
    for entry in index.plugins{
        if crate::legacy_ofm::is_retired_dev_directory(&entry.directory){return Err("开发预览不再接受退役的 OFM 目录；旧独立插件请走插件管理装卸".into());}
        if !matches!(entry.directory.as_str(),"feishu"|"clash"|"magpie")||entry.id!=format!("fish.{}",entry.directory){return Err("开发插件身份错误".into());}
        let package=crate::plugin_package::development(&root.join(&entry.directory),&entry.sha256,&context.root)?;
        if package.manifest.id!=entry.id{return Err("开发索引与清单身份不符".into());}
        let previous=manager.list()?.into_iter().find(|row|row.manifest.id==entry.id);
        let fresh=previous.is_none();
        let row=if previous.as_ref().is_some_and(|row|row.digest==entry.sha256){previous.unwrap()}else{runner.install(&manager,package,&entry.sha256,true,node)?};
        let ns=manager.namespace(&row)?;
        if let Some(relative)=entry.executable{
            let executable=crate::plugin_manifest::package_path(&manager.directory(&row)?,&relative)?;
            // 只剩 Magpie 的开发网关默认端口；OFM 的 38080 随业务一起退役，不再写盘。
            if entry.directory=="magpie"&&ns.read("config","gateway.json")?.is_none(){
                ns.write("config","gateway.json",&serde_json::to_vec(&serde_json::json!({"executable":executable,"port":3425})).map_err(|_|"开发配置编码失败")?)?;
            }
        }
        if fresh{manager.set_enabled(&entry.id,true)?;}
    }
    Ok(())
}
#[cfg(not(debug_assertions))]
pub fn prepare(_: &crate::storage_context::Context,_:&mut crate::plugin_runtime::Runner,_:Option<&std::path::Path>)->Result<(),String>{Ok(())}

/// 开发预览接受的目录白名单；退役目录一律拒收（与发行版官方目录保持一致）。
pub fn accepted_directory(directory: &str) -> bool {
    !crate::legacy_ofm::is_retired_dev_directory(directory) && matches!(directory, "feishu" | "clash" | "magpie")
}

#[cfg(test)]
mod tests {
    #[test]
    fn retired_ofm_directory_is_not_a_development_target() {
        assert!(super::accepted_directory("magpie"));
        assert!(!super::accepted_directory("ofm"), "退役 OFM 不再接受开发自动安装");
        assert!(!super::accepted_directory("acme"));
    }
}
