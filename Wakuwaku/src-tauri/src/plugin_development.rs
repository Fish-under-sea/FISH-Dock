//! 开发预览装载器：只在调试版、明确指定目录时启用，不改变发行版默认安装集合。
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
        if !matches!(entry.directory.as_str(),"feishu"|"clash"|"magpie"|"ofm")||entry.id!=format!("fish.{}",entry.directory){return Err("开发插件身份错误".into());}
        let package=crate::plugin_package::development(&root.join(&entry.directory),&entry.sha256,&context.root)?;
        if package.manifest.id!=entry.id{return Err("开发索引与清单身份不符".into());}
        let previous=manager.list()?.into_iter().find(|row|row.manifest.id==entry.id);
        let fresh=previous.is_none();
        let row=if previous.as_ref().is_some_and(|row|row.digest==entry.sha256){previous.unwrap()}else{runner.install(&manager,package,&entry.sha256,true,node)?};
        let ns=manager.namespace(&row)?;
        if let Some(relative)=entry.executable{
            let executable=crate::plugin_manifest::package_path(&manager.directory(&row)?,&relative)?;
            match entry.directory.as_str(){
                "ofm"|"magpie"=>{if ns.read("config","gateway.json")?.is_none(){let port=if entry.directory=="ofm"{38080}else{3425};ns.write("config","gateway.json",&serde_json::to_vec(&serde_json::json!({"executable":executable,"port":port})).map_err(|_|"开发配置编码失败")?)?;}},
                _=>{},
            }
        }
        if fresh{manager.set_enabled(&entry.id,true)?;}
    }
    Ok(())
}
#[cfg(not(debug_assertions))]
pub fn prepare(_: &crate::storage_context::Context,_:&mut crate::plugin_runtime::Runner,_:Option<&std::path::Path>)->Result<(),String>{Ok(())}
