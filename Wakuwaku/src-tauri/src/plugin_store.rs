//! 固定官方仓库目录与发行下载；缓存离线可用，包身份仍由受控来源授予。
//!
//! 所有目录地址、下载源、超时、重定向上限都来自 crate::config（红线 5：单一配置
//! 入口，URL/超时不许写死）。本文件只负责「怎么下、怎么校验」，不再持有地址默认值。
use serde::{Deserialize,Serialize};
use std::collections::{BTreeMap,HashSet};
use std::path::Path;
pub const MAX_DOWNLOAD:u64=crate::plugin_package::TOTAL_LIMIT as u64;
// ── 官方插件的下载来源（#f3）────────────────────────────────────────────
// 用户原话：「官方插件能不能走 npm 获取（第一次拉取失败想办法走镜像拉取）」。
// 顺序固定：npm 官方 registry → npm 镜像（npmmirror）→ GitHub Release 资产。
// 三条路拿到的是同一份发行归档，摘要一律用官方目录里的 sha256 —— 所以今天从 npm
// 装、明天从 GitHub 装，落地的摘要完全一样：重装幂等、不会多出一份版本目录。
// 地址/超时默认值与 env 覆盖都在 crate::config 的 url.* / timeout.* 键里。
pub const NPM_PACKAGE_PREFIX:&str="@fish-under-sea/fish-dock-plugin-";
/// 一条下载来源的记录：哪条路成了、每条路花了多久、失败的那条为什么失败。
#[derive(Clone,Copy,Debug,PartialEq,Eq,Serialize,Deserialize)]
#[serde(rename_all="camelCase")]
pub enum Origin { NpmRegistry, NpmMirror, GithubRelease }
impl Origin {
    pub fn label(self)->&'static str { match self { Origin::NpmRegistry=>"npm 官方源", Origin::NpmMirror=>"npm 镜像", Origin::GithubRelease=>"GitHub 发行包" } }
    fn slug(self)->&'static str { match self { Origin::NpmRegistry=>"npm-registry", Origin::NpmMirror=>"npm-mirror", Origin::GithubRelease=>"github-release" } }
}
#[derive(Clone,Debug,Serialize,Deserialize)]
#[serde(rename_all="camelCase")]
pub struct Attempt { pub origin:Origin,pub url:String,pub ok:bool,pub elapsed_ms:u64,
    #[serde(default,skip_serializing_if="Option::is_none")] pub error:Option<String> }
// FetchRecord 是「这次安装的包是从哪儿来的」：插件卡上要能看见用的是哪条源、
// 花了多久（用户要求 UI 可见）。落盘在 config/plugin-fetch.json，同一插件只留
// 最后一次，最多 20 条。
#[derive(Clone,Debug,Serialize,Deserialize)]
#[serde(rename_all="camelCase")]
pub struct FetchRecord { pub id:String,pub version:String,pub origin:Origin,pub url:String,
    pub elapsed_ms:u64,pub at:u64,pub attempts:Vec<Attempt> }
#[derive(Clone,Debug,Serialize)]
#[serde(rename_all="camelCase")]
pub struct Entry {
    pub id:String,pub name:String,pub description:String,pub authors:Vec<String>,#[serde(skip_serializing_if="Option::is_none")] pub note:Option<String>,
    pub version:String,pub host_api:u32,pub platform:String,pub url:String,pub sha256:String,pub bytes:u64,
    #[serde(default)] pub dependencies:Vec<String>,
}
/// 只用于反序列化：认新结构（authors/note），也认旧目录（author 字符串 + maintainer）。
#[derive(Deserialize)]
#[serde(deny_unknown_fields,rename_all="camelCase")]
struct RawEntry {
    id:String,name:String,description:String,
    #[serde(default)] authors:Option<Vec<String>>,#[serde(default)] author:Option<String>,
    #[serde(default,rename="maintainer")] legacy_maintainer:Option<String>,
    #[serde(default)] note:Option<String>,
    version:String,host_api:u32,platform:String,url:String,sha256:String,bytes:u64,
    #[serde(default)] dependencies:Vec<String>,
}
impl<'de>Deserialize<'de> for Entry {
    fn deserialize<D:serde::Deserializer<'de>>(decoder:D)->Result<Self,D::Error>{
        let raw=RawEntry::deserialize(decoder)?;
        let authors=match (raw.authors,raw.author){
            (Some(authors),_) if !authors.is_empty()=>authors,
            (_,Some(author))=>vec![author],
            (Some(_),None)=>return Err(serde::de::Error::custom("官方条目作者列表不能为空")),
            (None,None)=>return Err(serde::de::Error::missing_field("authors")),
        };
        let _=raw.legacy_maintainer;
        Ok(Entry{id:raw.id,name:raw.name,description:raw.description,authors,note:raw.note.filter(|note|!note.trim().is_empty()),version:raw.version,host_api:raw.host_api,platform:raw.platform,url:raw.url,sha256:raw.sha256,bytes:raw.bytes,dependencies:raw.dependencies})
    }
}
#[derive(Clone,Debug,Serialize,Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Catalog {pub schema:u32,pub plugins:Vec<Entry>}
pub fn parse(bytes:&[u8])->Result<Catalog,String>{
    if bytes.len()>crate::config::catalog_max_bytes(){return Err("官方目录超过大小限制".into());}
    let catalog:Catalog=serde_json::from_slice(bytes).map_err(|_|"官方目录格式无效")?;
    if catalog.schema!=1||catalog.plugins.len()>100{return Err("官方目录版本或条目数无效".into());}
    let mut seen=HashSet::new();
    for row in &catalog.plugins{
        if !["fish.feishu","fish.clash","fish.magpie","fish.ofm"].contains(&row.id.as_str())||!seen.insert((&row.id,&row.platform)){return Err("官方目录身份无效或重复".into());}
        crate::plugin_manifest::version(&row.version)?;
        if row.host_api!=crate::plugin_manifest::API||row.platform.len()>40||!row.platform.bytes().all(|b|b.is_ascii_alphanumeric()||b==b'-'||b==b'_'){return Err("官方目录兼容版本或平台无效".into());}
        if [&row.name,&row.description].iter().any(|s|s.trim().is_empty()||s.len()>2048||s.chars().any(char::is_control))||row.authors.is_empty()||row.authors.len()>16||row.authors.iter().any(|s|s.trim().is_empty()||s.len()>2048||s.chars().any(char::is_control))||row.note.as_ref().is_some_and(|s|s.len()>2048||s.chars().any(char::is_control))||row.dependencies.len()>16||row.dependencies.iter().any(|s|s.is_empty()||s.len()>200||s.chars().any(char::is_control)){return Err("官方条目说明或依赖无效".into());}
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
// setting 是「配置优先、内置默认兜底」的唯一入口：环境变量空串视为没设。
// updates.rs 的公告源地址也从这里读（一处配置读取，不各写一份）。
pub(crate) fn setting(name:&str,fallback:&str)->String{
    std::env::var(name).ok().map(|value|value.trim().trim_end_matches('/').to_string()).filter(|value|!value.is_empty()).unwrap_or_else(||fallback.to_string())
}
pub fn npm_registry()->String{crate::config::plugin_npm_registry()}
pub fn npm_mirror()->String{crate::config::plugin_npm_mirror()}
fn download_timeout()->std::time::Duration{crate::config::plugin_download_timeout()}
// 官方目录的三个读取口（raw / api / 环境覆盖）都在 crate::config 的 url.* 键里定，别处不许再写字面量。
fn catalog_urls()->Vec<String>{vec![crate::config::plugin_catalog_raw(),crate::config::plugin_catalog_api()]}
fn download(url:&str,limit:u64)->Result<Vec<u8>,String>{
    let mut last="官方商店连接失败，请检查网络后重试".to_string();
    for way in crate::proxy::web_ways(url){
        let Some(agent)=way.agent_with(download_timeout(),|config|config.max_redirects(0).http_status_as_error(false))else{continue};
        match download_with(&agent,url,limit){Ok(bytes)=>return Ok(bytes),Err(error)=>last=error}
    }
    Err(last)
}
fn download_with(agent:&ureq::Agent,url:&str,limit:u64)->Result<Vec<u8>,String>{
    let mut current=secure_url(url)?;
    for _ in 0..crate::config::plugin_redirect_hops(){
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
    for url in catalog_urls(){
        match fetch(&url).and_then(|bytes|parse(&bytes).map(|catalog|(bytes,catalog))){
            Ok(result)=>return Ok(result),
            Err(error)=>last=error,
        }
    }
    Err(format!("官方目录各来源均未就绪：{last}。可在网络设置中检查代理，已安装插件仍可使用。"))
}
pub fn refresh(context:&crate::storage_context::Context)->Result<serde_json::Value,String>{
    let attempt=fetch_catalog_with(|url|download(url,crate::config::catalog_max_bytes() as u64)).and_then(|(bytes,catalog)|{context.host.write("config","official-plugin-catalog.json",&bytes)?;Ok(catalog)});
    match attempt{
        Ok(catalog)=>Ok(serde_json::json!({"catalog":catalog,"cached":false})),
        Err(error)=>match context.host.read("config","official-plugin-catalog.json")?{
            Some(bytes)=>Ok(serde_json::json!({"catalog":parse(&bytes)?,"cached":true,"error":error})),
            None=>Err(error),
        }
    }
}
// npm 单包名：官方插件 fish.magpie → @fish-under-sea/fish-dock-plugin-magpie
// （plugins/tools/build-npm-packages.mjs 发布的就是这个名字）。
fn npm_package(id:&str)->String{
    let slug=id.rsplit('.').next().filter(|slug|!slug.is_empty()).unwrap_or(id);
    format!("{NPM_PACKAGE_PREFIX}{slug}")
}
// npm 与 npmmirror 的 tarball 路径约定：/@scope/name/-/name-版本.tgz（文件名不带 scope）。
fn npm_tarball(base:&str,id:&str,version:&str)->Result<String,String>{
    let slug=id.rsplit('.').next().filter(|slug|!slug.is_empty()).unwrap_or(id);
    if !slug.bytes().all(|b|b.is_ascii_lowercase()||b.is_ascii_digit()||b==b'-') { return Err("官方插件 ID 无法对应 npm 包名".into()); }
    let package=npm_package(id);
    let file=package.rsplit('/').next().unwrap_or(package.as_str()).to_string();
    Ok(format!("{base}/{package}/-/{file}-{version}.tgz"))
}
// 从 npm tarball 里取出那份插件归档并校验。归档必须与官方目录里记的字节数、sha256
// 一致：npm 包只是发行归档的另一种封装，不是另一份内容——两者对上才算官方插件。
fn package_from_npm(bytes:&[u8],entry:&Entry,root:&Path)->Result<(crate::plugin_package::Package,String),String>{
    use sha2::{Digest,Sha256};
    let files=crate::plugin_thirdparty::unpack(bytes)?;
    let name=files.keys().find(|name|name.ends_with(".fish-plugin")&&!name.contains('/')).cloned().ok_or("npm 包不含插件归档（*.fish-plugin），不是 FISH Dock 官方插件包")?;
    let archive=files.get(&name).ok_or("npm 包里的插件归档读取失败")?;
    if archive.len() as u64!=entry.bytes{return Err(format!("npm 包里的插件归档大小与官方目录不一致（{} ≠ {}）",archive.len(),entry.bytes));}
    if !format!("{:x}",Sha256::digest(archive)).eq_ignore_ascii_case(&entry.sha256){return Err("npm 包里的插件归档摘要与官方目录不一致".into());}
    let package=crate::plugin_package::inspect(archive,&entry.sha256,root,crate::storage::Source::OfficialCatalog,&crate::plugin_install::platform())?;
    if package.manifest.id!=entry.id||package.manifest.version!=entry.version{return Err("插件包身份或版本与官方目录不一致".into());}
    Ok((package,entry.sha256.to_ascii_lowercase()))
}
// sources 是这次要依次尝试的（来源，地址）；fetch 注入成闭包，测试才不需要网络。
// 成功一次就停；每次尝试的成败与耗时都进 FetchRecord，第一次失败的原因原样留着。
fn fetch_from(entry:&Entry,sources:&[(Origin,String)],root:&Path,mut fetch:impl FnMut(&str,u64)->Result<Vec<u8>,String>)->Result<(crate::plugin_package::Package,String,FetchRecord),String>{
    let mut attempts=Vec::new();
    // 三条来源的失败原因都留着：只报最后一条会让用户以为是 GitHub 的问题，
    // 而真正卡住的常常是第一条（公司网挡 npm、镜像没同步）。
    let mut failures:Vec<String>=Vec::new();
    for (origin,url) in sources {
        let began=std::time::Instant::now();
        let outcome=match origin {
            // GitHub 资产就是一个 .fish-plugin：按目录里的大小与摘要校验。
            Origin::GithubRelease=>fetch(url,entry.bytes+1).and_then(|bytes|{
                if bytes.len() as u64!=entry.bytes{return Err("下载的插件包大小与官方目录不一致".to_string());}
                crate::plugin_package::inspect(&bytes,&entry.sha256,root,crate::storage::Source::OfficialCatalog,&crate::plugin_install::platform())
                    .and_then(|package|{
                        if package.manifest.id!=entry.id||package.manifest.version!=entry.version{return Err("插件包身份或版本与官方目录不一致".to_string());}
                        Ok((package,entry.sha256.to_ascii_lowercase()))
                    })
            }),
            // npm / 镜像：先下 tarball，再从里面取归档核验（package_from_npm）。
            _=>fetch(url,MAX_DOWNLOAD+crate::config::npm_tarball_slack_bytes() as u64).and_then(|bytes|package_from_npm(&bytes,entry,root)),
        };
        let elapsed_ms=began.elapsed().as_millis() as u64;
        match outcome {
            Ok((package,digest))=>{
                attempts.push(Attempt{origin:*origin,url:url.clone(),ok:true,elapsed_ms,error:None});
                let record=FetchRecord{id:entry.id.clone(),version:entry.version.clone(),origin:*origin,url:url.clone(),elapsed_ms,at:crate::now_ms(),attempts};
                return Ok((package,digest,record));
            }
            Err(error)=>{
                eprintln!("FISH Dock: 官方插件下载来源失败（{} {}）：{error}",origin.slug(),url);
                attempts.push(Attempt{origin:*origin,url:url.clone(),ok:false,elapsed_ms,error:Some(error.clone())});
                failures.push(format!("{}：{error}",origin.label()));
            }
        }
    }
    Err(format!("官方插件各下载来源均未成功：{}。请检查网络或代理后重试",failures.join("；")))
}
// save_fetch 记下这次「从哪条源装的」给插件卡显示；记不下来不该让已经到手的
// 包白丢，所以调用方把它当尽力而为。
fn save_fetch(context:&crate::storage_context::Context,record:&FetchRecord)->Result<(),String>{
    let mut records=fetch_records(context);
    records.insert(record.id.clone(),record.clone());
    while records.len()>crate::config::plugin_fetch_log_keep(){if let Some(first)=records.keys().next().cloned(){records.remove(&first);}else{break;}}
    save_records(context,&records)
}
/// save_records 是这份来源记录唯一的写入口（宿主清理与安装都走这里，不各写一遍）。
pub fn save_records(context:&crate::storage_context::Context,records:&BTreeMap<String,FetchRecord>)->Result<(),String>{
    context.host.write("config","plugin-fetch.json",&serde_json::to_vec_pretty(records).map_err(|_|"插件下载来源编码失败")?)
}
/// fetch_records 是每个插件最近一次安装用的是哪条下载源（插件卡上显示）。
pub fn fetch_records(context:&crate::storage_context::Context)->BTreeMap<String,FetchRecord>{
    let Ok(Some(bytes))=context.host.read("config","plugin-fetch.json") else {return BTreeMap::new();};
    // 记录坏了不该拦住插件页：读不出来就当没有。
    serde_json::from_slice(&bytes).unwrap_or_default()
}
pub fn obtain(context:&crate::storage_context::Context,id:&str)->Result<(crate::plugin_package::Package,String),String>{
    obtain_traced(context,id).map(|(package,digest,_)|(package,digest))
}
// obtain_traced 是带来源记录的对照口：插件卡要显示「这条源 + 耗时」，安装本身
// 只要前两项。三条来源的先后顺序就是用户要的「npm 优先、失败走镜像、再不行回落
// GitHub」；任何一条成功都直接返回，不再碰后面的。
pub fn obtain_traced(context:&crate::storage_context::Context,id:&str)->Result<(crate::plugin_package::Package,String,FetchRecord),String>{
    let bytes=context.host.read("config","official-plugin-catalog.json")?.ok_or("请先刷新官方插件商店")?;
    let entry=parse(&bytes)?.plugins.into_iter().find(|row|row.id==id&&row.platform==crate::plugin_install::platform()).ok_or("官方目录没有适用于当前平台的插件版本")?;
    let sources=[(Origin::NpmRegistry,npm_tarball(&npm_registry(),&entry.id,&entry.version)?),
        (Origin::NpmMirror,npm_tarball(&npm_mirror(),&entry.id,&entry.version)?),
        (Origin::GithubRelease,entry.url.clone())];
    let (package,digest,record)=fetch_from(&entry,&sources,&context.root,|url,limit|download(url,limit))?;
    if let Err(error)=save_fetch(context,&record){eprintln!("FISH Dock: 插件下载来源记录写入失败（{}）：{error}",entry.id);}
    Ok((package,digest,record))
}
#[cfg(test)]mod tests{
    use super::*;
    fn entry()->serde_json::Value{serde_json::json!({"id":"fish.ofm","name":"OFM","description":"本地模型","author":"FISH","maintainer":"FISH","version":"2.5.3","hostApi":1,"platform":crate::plugin_install::platform(),"url":"https://github.com/Fish-under-sea/FISH-DOCK-Plugins/releases/download/ofm-v2.5.3/ofm.fish-plugin","sha256":"a".repeat(64),"bytes":4096,"dependencies":["Node 24"]})}
    fn new_entry()->serde_json::Value{serde_json::json!({"id":"fish.ofm","name":"OFM","description":"本地模型","authors":["Elysia395","FISH"],"note":"此插件于 10 月 25 日暂停维护","version":"2.5.3","hostApi":1,"platform":crate::plugin_install::platform(),"url":"https://github.com/Fish-under-sea/FISH-DOCK-Plugins/releases/download/ofm-v2.5.3/ofm.fish-plugin","sha256":"a".repeat(64),"bytes":4096,"dependencies":["Node 24"]})}
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
    #[test]fn plugin_store_parses_authors_array_and_note(){
        let catalog=parse(&bytes(vec![new_entry()])).expect("新结构 catalog 应可解析");
        let row=serde_json::to_value(&catalog.plugins[0]).unwrap();
        assert_eq!(row["authors"],serde_json::json!(["Elysia395","FISH"]),"authors 应为数组且顺序不变");
        assert_eq!(row["note"],serde_json::json!("此插件于 10 月 25 日暂停维护"),"note 应原样保留");
        assert!(row.get("author").is_none()&&row.get("maintainer").is_none(),"输出不应再出现 author/maintainer");
    }
    #[test]fn plugin_store_treats_blank_note_as_absent(){
        let mut row=new_entry();row["note"]="   ".into();
        let catalog=parse(&bytes(vec![row])).expect("空白 note 不应让整份目录失效");
        assert_eq!(serde_json::to_value(&catalog.plugins[0]).unwrap().get("note"),None,"空白 note 应视为没有 note");
    }
    #[test]fn plugin_store_falls_back_to_legacy_author_and_drops_maintainer(){
        let catalog=parse(&bytes(vec![entry()])).expect("旧结构 catalog 应继续可用");
        let row=serde_json::to_value(&catalog.plugins[0]).unwrap();
        assert_eq!(row["authors"],serde_json::json!(["FISH"]),"旧 author 字符串应回退成单元素数组");
        assert!(row.get("note").is_none()||row["note"].is_null(),"旧结构没有 note 时应为空");
        assert!(row.get("maintainer").is_none(),"maintainer 字段应被忽略");
    }
    // ── 官方插件下载来源（#f3）────────────────────────────────────────────
    use sha2::{Digest,Sha256};
    // 一份真实的 .fish-plugin（zip）与装它的 npm tarball，用来跑三条来源的真实校验。
    fn plugin_archive()->Vec<u8>{
        let backend=b"console.log('ofm fixture')".to_vec();
        let readme=b"# OFM 2.0.0".to_vec();
        let raw=serde_json::json!({"schema":1,"id":"fish.ofm","name":"OFM 本地模型","version":"2.0.0","hostApi":1,"platforms":[crate::plugin_install::platform()],"storage":{"directory":".fishplugin-ourfreemodel","dataSchema":1},"backend":{"runtime":"node","entry":"backend.mjs"},"behaviors":{"autostart":true},"files":{"backend.mjs":format!("{:x}",Sha256::digest(&backend)),"README.md":format!("{:x}",Sha256::digest(&readme))}});
        crate::identity_archive::zip(&BTreeMap::from([("plugin.json".into(),serde_json::to_vec(&raw).unwrap()),("backend.mjs".into(),backend),("README.md".into(),readme)])).unwrap()
    }
    // 目录条目走真实的 parse：下载地址、摘要、字节数都按官方目录的规矩校验一遍。
    fn catalog_entry(archive:&[u8])->Entry{
        let row=serde_json::json!({"id":"fish.ofm","name":"OFM","description":"本地模型","authors":["FISH"],"version":"2.0.0","hostApi":1,"platform":crate::plugin_install::platform(),"url":"https://github.com/Fish-under-sea/FISH-DOCK-Plugins/releases/download/v2.0.0/ofm.fish-plugin","sha256":format!("{:x}",Sha256::digest(archive)),"bytes":archive.len()});
        parse(&bytes(vec![row])).unwrap().plugins.remove(0)
    }
    // npm 的 tarball：ustar（512 字节头 + 内容）+ gzip，内容都在 package/ 下。
    fn tarball(files:&[(&str,Vec<u8>)])->Vec<u8>{
        let mut plain=Vec::new();
        for (name,body) in files {
            let full=format!("package/{name}");
            assert!(full.len()<100,"ustar 头里的名字字段只有 100 字节");
            let mut header=[0u8;512];
            header[..full.len()].copy_from_slice(full.as_bytes());
            let size=format!("{:o}",body.len());
            header[124..124+size.len()].copy_from_slice(size.as_bytes());
            header[156]=b'0';
            plain.extend_from_slice(&header);
            plain.extend_from_slice(body);
            plain.resize(plain.len()+(512-body.len()%512)%512,0);
        }
        plain.extend_from_slice(&[0u8;1024]);
        let mut encoder=flate2::write::GzEncoder::new(Vec::new(),flate2::Compression::fast());
        std::io::Write::write_all(&mut encoder,&plain).unwrap();
        encoder.finish().unwrap()
    }
    fn npm_package_of(archive:&[u8])->Vec<u8>{tarball(&[("ofm.fish-plugin",archive.to_vec()),("package.json",b"{\"name\":\"@fish-under-sea/fish-dock-plugin-ofm\"}".to_vec())])}
    fn sources(entry:&Entry)->Vec<(Origin,String)>{
        vec![(Origin::NpmRegistry,"https://registry.npmjs.org/@fish-under-sea/fish-dock-plugin-ofm/-/fish-dock-plugin-ofm-2.0.0.tgz".into()),
            (Origin::NpmMirror,"https://registry.npmmirror.com/@fish-under-sea/fish-dock-plugin-ofm/-/fish-dock-plugin-ofm-2.0.0.tgz".into()),
            (Origin::GithubRelease,entry.url.clone())]
    }
    fn fixture_root(tag:&str)->std::path::PathBuf{std::env::temp_dir().join(format!("fish-source-{tag}-{}-{}",std::process::id(),crate::now_ms()))}
    // npm 官方源成功：只发一次请求，镜像与 GitHub 一次都不碰。
    #[test]fn official_plugin_comes_from_the_npm_registry_first(){
        let archive=plugin_archive(); let entry=catalog_entry(&archive); let tgz=npm_package_of(&archive);
        let root=fixture_root("npm");
        let mut asked=Vec::new();
        let (package,digest,record)=fetch_from(&entry,&sources(&entry),&root,|url,_limit|{asked.push(url.to_string());if url.contains("registry.npmjs.org"){Ok(tgz.clone())}else{Err("fixture: 不该走到后面那条源".into())}}).unwrap();
        assert_eq!(asked.len(),1,"npm 成了就不该再请求镜像与 GitHub");
        assert_eq!(record.origin,Origin::NpmRegistry);
        assert_eq!(record.attempts.len(),1);
        assert!(record.attempts[0].ok);
        assert_eq!((package.manifest.id.as_str(),package.manifest.version.as_str()),("fish.ofm","2.0.0"));
        assert_eq!(digest,entry.sha256.to_ascii_lowercase(),"npm 与 GitHub 装出来的摘要必须一模一样（重装才幂等）");
        println!("npm 来源：{} ms，尝试 {} 次",record.elapsed_ms,record.attempts.len());
        let _=std::fs::remove_dir_all(root);
    }
    // 用户原话里的「第一次拉取失败想办法走镜像拉取」：registry 失败 → 自动换 npmmirror。
    #[test]fn a_failing_npm_registry_falls_back_to_the_mirror(){
        let archive=plugin_archive(); let entry=catalog_entry(&archive); let tgz=npm_package_of(&archive);
        let root=fixture_root("mirror");
        let mut asked=Vec::new();
        let (package,digest,record)=fetch_from(&entry,&sources(&entry),&root,|url,_limit|{asked.push(url.to_string());if url.contains("registry.npmjs.org"){Err("fixture: 官方源连不上".into())}else if url.contains("registry.npmmirror.com"){Ok(tgz.clone())}else{Err("fixture: 不该走到 GitHub".into())}}).unwrap();
        assert_eq!(asked.len(),2,"官方源失败后应立刻换镜像，不越级");
        assert!(asked[1].contains("registry.npmmirror.com"));
        assert_eq!(record.origin,Origin::NpmMirror);
        assert_eq!(record.attempts.len(),2);
        assert!(!record.attempts[0].ok&&record.attempts[0].error.as_deref().is_some_and(|error|error.contains("官方源")));
        assert!(record.attempts[1].ok);
        assert_eq!((package.manifest.id.as_str(),digest.as_str()),("fish.ofm",entry.sha256.to_ascii_lowercase().as_str()));
        let _=std::fs::remove_dir_all(root);
    }
    // 两条 npm 都不通：回落 GitHub Release 资产，三条尝试都留在记录里。
    #[test]fn both_npm_sources_failing_falls_back_to_the_github_release(){
        let archive=plugin_archive(); let entry=catalog_entry(&archive);
        let root=fixture_root("github");
        let mut asked=Vec::new();
        let (package,digest,record)=fetch_from(&entry,&sources(&entry),&root,|url,_limit|{asked.push(url.to_string());if url.contains("github.com"){Ok(archive.clone())}else{Err("fixture: npm 全挂".into())}}).unwrap();
        assert_eq!(asked.len(),3,"npm 官方源、镜像、GitHub 各试一次");
        assert_eq!(record.origin,Origin::GithubRelease);
        assert_eq!(record.attempts.iter().filter(|attempt|attempt.ok).count(),1);
        assert_eq!(package.manifest.id,"fish.ofm");
        assert_eq!(digest,entry.sha256.to_ascii_lowercase());
        let _=std::fs::remove_dir_all(root);
    }
    // npm 包里的归档与官方目录对不上（被人重打包过）：不许当官方插件装，三条都失败要报清楚。
    #[test]fn a_tarball_that_does_not_carry_the_cataloged_archive_is_refused(){
        let archive=plugin_archive(); let entry=catalog_entry(&archive);
        let wrong=npm_package_of(b"not the cataloged archive");
        let root=fixture_root("wrong");
        let error=match fetch_from(&entry,&sources(&entry),&root,|url,_limit|{if url.contains("github.com"){Err("fixture: GitHub 也断了".into())}else{Ok(wrong.clone())}}){Ok(_)=>panic!("摘要对不上就不许装成官方插件"),Err(error)=>error};
        assert!(error.contains("各下载来源均未成功"),"{error}");
        assert!(error.contains("大小与官方目录不一致")||error.contains("摘要与官方目录不一致"),"{error}");
        let _=std::fs::remove_dir_all(root);
    }
    // 地址与超时不许写死：环境变量优先，空串视为没设；npm 包名/下载路径按约定拼。
    #[test]fn source_addresses_come_from_configuration_not_literals(){
        std::env::set_var("FISH_DOCK_TEST_PLUGIN_SOURCE","https://mirror.fixture/");
        assert_eq!(setting("FISH_DOCK_TEST_PLUGIN_SOURCE","https://default.fixture"),"https://mirror.fixture","末尾斜杠要去掉");
        std::env::set_var("FISH_DOCK_TEST_PLUGIN_SOURCE","   ");
        assert_eq!(setting("FISH_DOCK_TEST_PLUGIN_SOURCE","https://default.fixture"),"https://default.fixture","空串或空白视为没设");
        std::env::remove_var("FISH_DOCK_TEST_PLUGIN_SOURCE");
        assert_eq!(npm_package("fish.magpie"),"@fish-under-sea/fish-dock-plugin-magpie");
        assert_eq!(npm_tarball("https://registry.npmjs.org","fish.magpie","2.0.0").unwrap(),"https://registry.npmjs.org/@fish-under-sea/fish-dock-plugin-magpie/-/fish-dock-plugin-magpie-2.0.0.tgz");
        assert!(npm_tarball("https://registry.npmjs.org","fish.OFM","2.0.0").is_err(),"插件 ID 的末段必须能当 npm 文件名片");
        assert_eq!(Origin::NpmMirror.slug(),"npm-mirror","日志里的来源名要稳定");
    }
}
