//! 固定官方仓库目录与发行下载；缓存离线可用，包身份仍由受控来源授予。
use serde::{Deserialize,Serialize};
use std::collections::HashSet;
pub const CATALOG_URL:&str="https://raw.githubusercontent.com/Fish-under-sea/FISH-DOCK-Plugins/main/catalog/stable.json";
const CATALOG_API:&str="https://api.github.com/repos/Fish-under-sea/FISH-DOCK-Plugins/contents/catalog/stable.json?ref=main";
pub const MAX_DOWNLOAD:u64=crate::plugin_package::TOTAL_LIMIT as u64;
#[derive(Clone,Debug,Serialize,Deserialize)]
#[serde(deny_unknown_fields,rename_all="camelCase")]
pub struct Entry {
    pub id:String,pub name:String,pub description:String,pub author:String,pub maintainer:String,
    pub version:String,pub host_api:u32,pub platform:String,pub url:String,pub sha256:String,pub bytes:u64,
    #[serde(default)] pub dependencies:Vec<String>,
}
#[derive(Clone,Debug,Serialize,Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Catalog {pub schema:u32,pub plugins:Vec<Entry>}
pub fn parse(bytes:&[u8])->Result<Catalog,String>{
    if bytes.len()>512*1024{return Err("官方目录超过大小限制".into());}
    let catalog:Catalog=serde_json::from_slice(bytes).map_err(|_|"官方目录格式无效")?;
    if catalog.schema!=1||catalog.plugins.len()>100{return Err("官方目录版本或条目数无效".into());}
    let mut seen=HashSet::new();
    for row in &catalog.plugins{
        if !["fish.feishu","fish.clash","fish.magpie","fish.ofm"].contains(&row.id.as_str())||!seen.insert((&row.id,&row.platform)){return Err("官方目录身份无效或重复".into());}
        crate::plugin_manifest::version(&row.version)?;
        if row.host_api!=crate::plugin_manifest::API||row.platform.len()>40||!row.platform.bytes().all(|b|b.is_ascii_alphanumeric()||b==b'-'||b==b'_'){return Err("官方目录兼容版本或平台无效".into());}
        if [&row.name,&row.description,&row.author,&row.maintainer].iter().any(|s|s.trim().is_empty()||s.len()>2048||s.chars().any(char::is_control))||row.dependencies.len()>16||row.dependencies.iter().any(|s|s.is_empty()||s.len()>200||s.chars().any(char::is_control)){return Err("官方条目说明或依赖无效".into());}
        release_url(&row.url)?;
        if row.bytes==0||row.bytes>MAX_DOWNLOAD||row.sha256.len()!=64||!row.sha256.bytes().all(|b|b.is_ascii_hexdigit()){return Err("官方条目包大小或摘要无效".into());}
    }
    Ok(catalog)
}
fn secure_url(value:&str)->Result<tauri::Url,String>{
    let url=tauri::Url::parse(value).map_err(|_|"插件下载地址无效")?;
    if url.scheme()!="https"||!url.username().is_empty()||url.password().is_some()||url.port().is_some()||url.fragment().is_some(){return Err("插件下载必须使用无身份信息的标准 HTTPS 地址".into());}Ok(url)
}
fn release_url(value:&str)->Result<(),String>{
    let url=secure_url(value)?;
    if url.host_str()!=Some("github.com")||!url.path().starts_with("/Fish-under-sea/FISH-DOCK-Plugins/releases/download/")||!url.path().ends_with(".fish-plugin")||url.query().is_some(){return Err("插件包必须来自官方插件仓的固定发行资源".into());}Ok(())
}
fn download(url:&str,limit:u64)->Result<Vec<u8>,String>{
    let mut last="官方商店连接失败，请检查网络后重试".to_string();
    for way in crate::proxy::web_ways(url){
        let Some(agent)=way.agent_with(std::time::Duration::from_secs(30),|config|config.max_redirects(0).http_status_as_error(false))else{continue};
        match download_with(&agent,url,limit){Ok(bytes)=>return Ok(bytes),Err(error)=>last=error}
    }
    Err(last)
}
fn download_with(agent:&ureq::Agent,url:&str,limit:u64)->Result<Vec<u8>,String>{
    let mut current=secure_url(url)?;
    for _ in 0..5{
        let accept=if current.host_str()==Some("api.github.com"){"application/vnd.github.raw+json"}else{"application/octet-stream"};
        let mut response=agent.get(current.as_str()).header("User-Agent","FISH-Dock-Plugin-Store").header("Accept",accept).call().map_err(|_|"官方商店连接失败，请检查网络后重试")?;
        let status=response.status().as_u16();
        if matches!(status,301|302|303|307|308){
            let location=response.headers().get("Location").and_then(|v|v.to_str().ok()).ok_or("插件下载跳转缺少地址")?;
            let next=current.join(location).map_err(|_|"插件下载跳转地址无效")?;secure_url(next.as_str())?;
            if !["release-assets.githubusercontent.com","objects.githubusercontent.com"].contains(&next.host_str().unwrap_or("")){return Err("插件下载跳转不属于官方发行资源服务".into());}
            current=next;continue;
        }
        if status!=200{return Err(format!("官方商店请求失败（HTTP {status}）"));}
        return response.body_mut().with_config().limit(limit).read_to_vec().map_err(|_|"插件下载中断或超过大小限制".into());
    }
    Err("插件下载跳转次数超过限制".into())
}
fn fetch_catalog_with(mut fetch:impl FnMut(&str)->Result<Vec<u8>,String>)->Result<(Vec<u8>,Catalog),String>{
    let mut last="官方目录暂未就绪".to_string();
    for url in [CATALOG_URL,CATALOG_API]{
        match fetch(url).and_then(|bytes|parse(&bytes).map(|catalog|(bytes,catalog))){
            Ok(result)=>return Ok(result),
            Err(error)=>last=error,
        }
    }
    Err(format!("官方目录各来源均未就绪：{last}。可在网络设置中检查代理，已安装插件仍可使用。"))
}
pub fn refresh(context:&crate::storage_context::Context)->Result<serde_json::Value,String>{
    let attempt=fetch_catalog_with(|url|download(url,512*1024)).and_then(|(bytes,catalog)|{context.host.write("config","official-plugin-catalog.json",&bytes)?;Ok(catalog)});
    match attempt{
        Ok(catalog)=>Ok(serde_json::json!({"catalog":catalog,"cached":false})),
        Err(error)=>match context.host.read("config","official-plugin-catalog.json")?{
            Some(bytes)=>Ok(serde_json::json!({"catalog":parse(&bytes)?,"cached":true,"error":error})),
            None=>Err(error),
        }
    }
}
pub fn obtain(context:&crate::storage_context::Context,id:&str)->Result<(crate::plugin_package::Package,String),String>{
    let bytes=context.host.read("config","official-plugin-catalog.json")?.ok_or("请先刷新官方插件商店")?;
    let entry=parse(&bytes)?.plugins.into_iter().find(|row|row.id==id&&row.platform==crate::plugin_install::platform()).ok_or("官方目录没有适用于当前平台的插件版本")?;
    let bytes=download(&entry.url,entry.bytes+1)?;
    if bytes.len() as u64!=entry.bytes{return Err("下载的插件包大小与官方目录不一致".into());}
    let package=crate::plugin_package::inspect(&bytes,&entry.sha256,&context.root,crate::storage::Source::OfficialCatalog,&crate::plugin_install::platform())?;
    if package.manifest.id!=entry.id||package.manifest.version!=entry.version{return Err("插件包身份或版本与官方目录不一致".into());}
    Ok((package,entry.sha256))
}
#[cfg(test)]mod tests{
    use super::*;
    fn entry()->serde_json::Value{serde_json::json!({"id":"fish.ofm","name":"OFM","description":"本地模型","author":"FISH","maintainer":"FISH","version":"2.5.3","hostApi":1,"platform":crate::plugin_install::platform(),"url":"https://github.com/Fish-under-sea/FISH-DOCK-Plugins/releases/download/ofm-v2.5.3/ofm.fish-plugin","sha256":"a".repeat(64),"bytes":4096,"dependencies":["Node 24"]})}
    fn bytes(entries:Vec<serde_json::Value>)->Vec<u8>{serde_json::to_vec(&serde_json::json!({"schema":1,"plugins":entries})).unwrap()}
    #[test]fn catalog_uses_another_official_source_after_network_or_invalid_content(){
        for invalid in [false,true]{
            let mut asked=Vec::new();
            let result=fetch_catalog_with(|url|{asked.push(url.to_string());if asked.len()==1{if invalid{Ok(b"not json".to_vec())}else{Err("fixture network failure".into())}}else{Ok(bytes(vec![entry()]))}});
            assert!(result.is_ok(),"首源失败后应读取其余官方源");
            assert_eq!(asked.len(),2);
            assert!(asked[1].starts_with("https://api.github.com/repos/Fish-under-sea/FISH-DOCK-Plugins/"));
        }
    }
    #[test]fn official_catalog_accepts_empty_and_versioned_entries(){
        assert!(parse(&bytes(vec![])).unwrap().plugins.is_empty());
        let catalog=parse(&bytes(vec![entry()])).unwrap();assert_eq!(catalog.plugins[0].id,"fish.ofm");
    }
    #[test]fn official_catalog_rejects_forgery_duplicates_and_foreign_downloads(){
        for (key,value) in [("id","acme.weather"),("version","latest"),("url","https://example.test/ofm.fish-plugin"),("url","https://github.com/other/repo/releases/download/v1/p.fish-plugin"),("url","http://github.com/Fish-under-sea/FISH-DOCK-Plugins/releases/download/v1/p.fish-plugin"),("url","https://user@github.com/Fish-under-sea/FISH-DOCK-Plugins/releases/download/v1/p.fish-plugin")] {
            let mut row=entry();row[key]=value.into();assert!(parse(&bytes(vec![row])).is_err(),"{key}: {value}");
        }
        assert!(parse(&bytes(vec![entry(),entry()])).is_err());
        let mut row=entry();row["bytes"]=(MAX_DOWNLOAD+1).into();assert!(parse(&bytes(vec![row])).is_err());
    }
}
