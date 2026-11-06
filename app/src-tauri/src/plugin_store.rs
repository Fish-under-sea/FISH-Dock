//! 固定官方仓库目录与发行下载；缓存离线可用，包身份仍由受控来源授予。
//! 【#68】三源全灭时按来源回退上一可用版本：版本候选只来自官方目录（当前条目 +
//! 可选历史条目）或随宿主发版写死的内置已验证表，摘要校验一律不因来源放宽。
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
// ── 官方身份白名单（唯一一份字面量）──────────────────────────────────────
// 目录里允许出现的身份就是这三个：插件仓目前维护的三个官方插件。目录校验读它，
// 第三方页的「排除官方」也读它（#72）——两处各抄一份数组就等于把「官方」定义了
// 两次，迟早有一天不一致。
pub const OFFICIAL_IDS:[&str;3]=["fish.feishu","fish.clash","fish.magpie"];
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
    // 【#68】探活与完整下载共用同一份 attempt：url + httpStatus + phase + elapsed_ms。
    // phase 只取五词之一（连接/跳转/HTTP/校验/摘要），成功行写「完成」——与
    // plugin_failure::phase 同一口径，前端不必再自己猜阶段。
    #[serde(default,skip_serializing_if="Option::is_none")] pub http_status:Option<u16>,
    #[serde(default)] pub phase:String,
    #[serde(default,skip_serializing_if="Option::is_none")] pub error:Option<String> }
// FetchRecord 是「这次安装的包是从哪儿来的」：插件卡上要能看见用的是哪条源、
// 花了多久（用户要求 UI 可见）。落盘在 config/plugin-fetch.json，同一插件只留
// 最后一次，最多 20 条。
#[derive(Clone,Debug,Serialize,Deserialize)]
#[serde(rename_all="camelCase")]
pub struct FetchRecord { pub id:String,pub version:String,pub origin:Origin,pub url:String,
    pub elapsed_ms:u64,pub at:u64,pub attempts:Vec<Attempt>,
    // 【#68】目录里当时的最新版。与 version 不同就说明这次是按来源回退装上的更早
    // 版本：插件卡要写「已安装 X；目录最新 Y 暂不可用」，目录本身仍显示新版。
    #[serde(default,skip_serializing_if="Option::is_none")] pub catalog_latest:Option<String> }
impl FetchRecord {
    /// 回退过就返回目录里的最新版（谁把版本降下来的、该提示什么，都由这一处判定）。
    pub fn fell_back(&self)->Option<&str>{
        self.catalog_latest.as_deref().filter(|latest|*latest!=self.version)
    }
}
#[derive(Clone,Debug,Serialize)]
#[serde(rename_all="camelCase")]
pub struct Entry {
    pub id:String,pub name:String,pub description:String,pub authors:Vec<String>,#[serde(skip_serializing_if="Option::is_none")] pub note:Option<String>,
    pub version:String,pub host_api:u32,pub platform:String,pub url:String,pub sha256:String,pub bytes:u64,
    #[serde(default)] pub dependencies:Vec<String>,
    // 【#68】目录自带的历史版本条目（最近 N 个已发布版本）。插件仓的 stable.json 目前
    // 还没有这个字段——**目录 schema 待插件仓配合补 versions**；字段缺席时一律回退
    // 内置已验证表（见 KNOWN_GOOD），所以这里的 `default` 不是可选装饰，是兼容口。
    #[serde(default,skip_serializing_if="Vec::is_empty")] pub versions:Vec<CatalogVersion>,
}
/// 一条历史版本：地址、摘要、字节数三者与当前条目同一套校验口径（见 parse）。
#[derive(Clone,Debug,Serialize,Deserialize)]
#[serde(rename_all="camelCase")]
pub struct CatalogVersion { pub version:String,pub url:String,pub sha256:String,pub bytes:u64 }
/// 只用于反序列化：认新结构（authors/note/versions），也认旧目录（author 字符串 + maintainer）。
#[derive(Deserialize)]
#[serde(deny_unknown_fields,rename_all="camelCase")]
struct RawEntry {
    id:String,name:String,description:String,
    #[serde(default)] authors:Option<Vec<String>>,#[serde(default)] author:Option<String>,
    #[serde(default,rename="maintainer")] legacy_maintainer:Option<String>,
    #[serde(default)] note:Option<String>,
    version:String,host_api:u32,platform:String,url:String,sha256:String,bytes:u64,
    #[serde(default)] dependencies:Vec<String>,
    #[serde(default)] versions:Option<Vec<CatalogVersion>>,
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
        Ok(Entry{id:raw.id,name:raw.name,description:raw.description,authors,note:raw.note.filter(|note|!note.trim().is_empty()),version:raw.version,host_api:raw.host_api,platform:raw.platform,url:raw.url,sha256:raw.sha256,bytes:raw.bytes,dependencies:raw.dependencies,versions:raw.versions.unwrap_or_default()})
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
        if !OFFICIAL_IDS.contains(&row.id.as_str())||!seen.insert((&row.id,&row.platform)){return Err("官方目录身份无效或重复".into());}
        crate::plugin_manifest::version(&row.version)?;
        if row.host_api!=crate::plugin_manifest::API||row.platform.len()>40||!row.platform.bytes().all(|b|b.is_ascii_alphanumeric()||b==b'-'||b==b'_'){return Err("官方目录兼容版本或平台无效".into());}
        if [&row.name,&row.description].iter().any(|s|s.trim().is_empty()||s.len()>2048||s.chars().any(char::is_control))||row.authors.is_empty()||row.authors.len()>16||row.authors.iter().any(|s|s.trim().is_empty()||s.len()>2048||s.chars().any(char::is_control))||row.note.as_ref().is_some_and(|s|s.len()>2048||s.chars().any(char::is_control))||row.dependencies.len()>16||row.dependencies.iter().any(|s|s.is_empty()||s.len()>200||s.chars().any(char::is_control)){return Err("官方条目说明或依赖无效".into());}
        release_url(&row.url)?;
        if row.bytes==0||row.bytes>MAX_DOWNLOAD||row.sha256.len()!=64||!row.sha256.bytes().all(|b|b.is_ascii_hexdigit()){return Err("官方条目包大小或摘要无效".into());}
        // 【#68】历史版本条目：一旦带上就逐条按同一套规矩校验——地址必须是官方发行
        // 资源、版本可解析、大小与摘要格式有效。回退版本的可信度不能低于当前版本，
        // 否则「回退」就等于把版本选择权和摘要校验一起交出去。
        if row.versions.len()>16{return Err("官方条目历史版本过多".into());}
        for history in &row.versions{
            crate::plugin_manifest::version(&history.version)?;
            release_url(&history.url)?;
            if history.bytes==0||history.bytes>MAX_DOWNLOAD||history.sha256.len()!=64||!history.sha256.bytes().all(|b|b.is_ascii_hexdigit()){return Err("官方条目历史版本大小或摘要无效".into());}
        }
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
fn download(url:&str,limit:u64,id:Option<&str>)->Result<Vec<u8>,String>{
    // 2.0.4 T-F（resources，审计 A05/A07）：原地址 + 镜像候选统一在 sources 展开，
    // 每候选再按代理路走；错误聚合保留每来源的原因。
    crate::sources::fetch_bytes_with(url,download_timeout(),limit,|candidate,limit|single_download(candidate,limit,id))
        .map_err(|fail|format!("官方商店下载失败（每来源都未成）：{}",fail.reason()))
}
fn single_download(url:&str,limit:u64,id:Option<&str>)->Result<Vec<u8>,String>{
    let mut last="官方商店连接失败，请检查网络后重试".to_string();
    for way in crate::proxy::web_ways(url){
        let Some(agent)=way.agent_with(download_timeout(),|config|config.max_redirects(0).http_status_as_error(false))else{continue};
        match download_with(&agent,url,limit,id){Ok(bytes)=>return Ok(bytes),Err(error)=>last=error}
    }
    Err(last)
}
// 跳转的唯一判定处（下载与探活共用）。允许两类目标：
//   ① 官方发行资产的落地域（GitHub Release 的 302 都到这里，与来源无关）；
//   ② **与当前请求同站点域**的目标——复用 plugin_thirdparty::host_within_site 的判据
//      （完全相同 / 源主机的子域 / ≥3 段源主机去掉最左一段后的站点域）。所以
//      registry.npmmirror.com → cdn.npmmirror.com 放行（镜像源自己 302 到自有 CDN，
//      #68 里「跳转被拒绝」的真正来源），而两段源主机不放宽到整个 TLD，evil.example
//      这类跨站目标仍然被拒——不是「任意 https」。
// 仍必须先过 secure_url：只许 HTTPS、不许带身份信息/端口/片段。
fn hop(current:&tauri::Url,location:Option<&str>)->Result<tauri::Url,String>{
    let location=location.ok_or("插件下载跳转缺少地址")?;
    let next=current.join(location).map_err(|_|"插件下载跳转地址无效")?;secure_url(next.as_str())?;
    let host=next.host_str().unwrap_or("").to_ascii_lowercase();
    let from=current.host_str().unwrap_or("").to_ascii_lowercase();
    let release_asset=["release-assets.githubusercontent.com","objects.githubusercontent.com"].contains(&host.as_str());
    let same_site=!from.is_empty()&&crate::plugin_thirdparty::host_within_site(&host,&from);
    if !release_asset&&!same_site{return Err("插件下载跳转不属于官方发行资源服务".into());}
    Ok(next)
}
fn download_with(agent:&ureq::Agent,url:&str,limit:u64,id:Option<&str>)->Result<Vec<u8>,String>{
    let mut current=secure_url(url)?;
    for _ in 0..crate::config::plugin_redirect_hops(){
        let accept=if current.host_str()==Some("api.github.com"){"application/vnd.github.raw+json"}else{"application/octet-stream"};
        let mut response=agent.get(current.as_str()).header("User-Agent","FISH-Dock-Plugin-Store").header("Accept",accept).call().map_err(|_|"官方商店连接失败，请检查网络后重试")?;
        let status=response.status().as_u16();
        if matches!(status,301|302|303|307|308){
            current=hop(&current,response.headers().get("Location").and_then(|v|v.to_str().ok()))?;continue;
        }
        if status!=200{return Err(format!("官方商店请求失败（HTTP {status}）"));}
        // t29：按 64 KiB 累计读取并报进度；总量取 Content-Length（未知给 null）。
        // limit 仍由 BodyWithConfig.limit + read_chunked 双重把关，不因分块放宽。
        let total=response.headers().get("Content-Length").and_then(|value|value.to_str().ok()).and_then(|value|value.parse::<u64>().ok());
        if let Some(id)=id { crate::plugin_progress::know_total(id,total); }
        let mut reader=response.body_mut().with_config().limit(limit).reader();
        return crate::plugin_progress::read_chunked(id,&mut reader,limit).map_err(|error| if error=="已取消"||error.contains("超过大小限制"){error}else{"插件下载中断或超过大小限制".into()});
    }
    Err("插件下载跳转次数超过限制".into())
}
// ── S2 探活（HEAD，不读 body）────────────────────────────────────────────
// 与下载走同一条代理/镜像路径（sources::candidates 展开镜像 → proxy::web_ways 逐路
// 尝试），只把「这个来源这个版本到底有没有制品」问清楚；404/403/410 连完整下载都不
// 再发起（#68 的根因就是拿完整下载去换一个 404）。
fn head_with(agent:&ureq::Agent,url:&str)->Result<u16,String>{
    let mut current=secure_url(url)?;
    for _ in 0..crate::config::plugin_redirect_hops(){
        let response=agent.head(current.as_str()).header("User-Agent","FISH-Dock-Plugin-Store").call().map_err(|_|"官方商店连接失败，请检查网络后重试")?;
        let status=response.status().as_u16();
        if matches!(status,301|302|303|307|308){
            current=hop(&current,response.headers().get("Location").and_then(|v|v.to_str().ok()))?;continue;
        }
        return Ok(status);
    }
    Err("插件下载跳转次数超过限制".into())
}
fn fetch_head(url:&str)->Result<u16,String>{
    let mut last="官方商店连接失败，请检查网络后重试".to_string();
    for pick in crate::sources::candidates(url){
        for way in crate::proxy::web_ways(&pick.url){
            let Some(agent)=way.agent_with(download_timeout(),|config|config.max_redirects(0).http_status_as_error(false))else{continue};
            match head_with(&agent,&pick.url){Ok(status)=>return Ok(status),Err(error)=>last=error}
        }
    }
    Err(last)
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
    let attempt=fetch_catalog_with(|url|download(url,crate::config::catalog_max_bytes() as u64,None)).and_then(|(bytes,catalog)|{context.host.write("config","official-plugin-catalog.json",&bytes)?;Ok(catalog)});
    match attempt{
        Ok(catalog)=>Ok(serde_json::json!({"catalog":catalog,"cached":false})),
        Err(error)=>match context.host.read("config","official-plugin-catalog.json")?{
            Some(bytes)=>Ok(serde_json::json!({"catalog":parse(&bytes)?,"cached":true,"error":error})),
            None=>Err(error),
        }
    }
}
// npm 单包名：官方插件 fish.magpie → @fish-under-sea/fish-dock-plugin-magpie
// （plugins/tools/build-npm-packages.mjs 发布的就是这个名字）。第三方页的「官方
// 包排除」也调这一处（#72）：同一份构造，不另数一份前缀。
pub(crate) fn npm_package(id:&str)->String{
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
// ── S1 版本候选（#68）──────────────────────────────────────────────────
// 内置已验证版本表（S1-C，保底来源）：随宿主发版写死「已经发布过、且官方目录里记过
// 真实 sha256/bytes」的几个版本。摘要来路与目录条目相同（都是官方目录的历史提交），
// 不是下载结果——所以回退版本照样要过 SHA-256、大小、manifest 身份三重比对。
//
// 为什么要这张表：目录的 `versions` 历史条目 schema **待插件仓配合**（stable.json
// 目前只有一条当前条目）。在插件仓补上之前，目录一旦「抢跑」（写了还没发布的版本，
// 见 issue #68），宿主至少还有上一版可用；补上之后这张表只是后备。
// 维护口径：每次插件仓发版，把最近 3–5 个已验证版本按「新→旧」加进来；同一版号
// 重打包过的（历史上有两个摘要）不要收录——那等于把「哪份才是官方」变成运气。
// 表里只存 (id, version, 摘要, 字节数)：地址由 release_asset() 按固定前缀拼，既不把
// 同一段前缀重复 12 遍，也让「发行地址只可能来自官方基址」由代码保证而不是靠抄写。
static KNOWN_GOOD:&[(&str,&str,&str,u64)]=&[
    ("fish.magpie","2.2.0","6bdd24353fa015b951cb859bf7d8ceb597e47c5e5b36a730bd8df391145584ec",33861986),
    ("fish.magpie","2.1.9","6f7addd5187839261704d5a0c7aac84c46aaf8006f01a7fe8c8209e3aee85f2c",33739638),
    ("fish.magpie","2.1.8","dad2735add920918f79c203c65596856d7a592aa986ce0398b927d4a5fc75d6b",33574926),
    ("fish.magpie","2.1.7","81d9308d3964f9d89ba7635ce738a8ef11edaa24438b37ef675686ccd7efd0b1",33307148),
    ("fish.magpie","2.1.6","7ed3dcad28ae5cbb20dbaeb6bead96fd4f20916c096b0b09854b7d8e19614057",33279603),
    ("fish.clash","2.2.0","df71fd3f1a4bf950f2c2aac99fb88ad3371164839f49af7d94dafb071038547b",17178572),
    ("fish.clash","2.1.9","51a4671bfaea989203352114c1ff8d58b77d189c5a65aff80496f3061c810a28",17178570),
    ("fish.clash","2.1.8","eea1f3f222f45b9c554dd4b5cd73ed44bbdb7ce4ea36dc816a19c5fd2e0c2e56",17177667),
    ("fish.clash","2.1.5","61f69e416155e365b8986bbd6705a80da747536ec18b50de0ac7beeb818a918e",17173401),
    ("fish.feishu","2.2.0","39aeda0afb7cc945dcfb7ea0eacb969672f10d2d69b6c6410fe73faef59f0bd8",36368),
    ("fish.feishu","2.1.9","a5735cbf6f24057471617ceed73f00ef67f883eae0517c4506ed00a9f8bc9605",36370),
    ("fish.feishu","2.1.5","227163f29521285b75200dd85691a68b0c0def6c560b46b188e46b0a8c68c2af",33530),
];
/// 官方发行资产地址：`<PLUGIN_RELEASE_BASE>/v<version>/<slug>.fish-plugin`。
/// 基址是 crate::config 里的唯一常量（红线 5），这里只拼版本与插件名。
fn release_asset(id:&str,version:&str)->String{
    let slug=id.strip_prefix("fish.").unwrap_or(id);
    format!("{}/v{version}/{slug}.fish-plugin",crate::config::PLUGIN_RELEASE_BASE)
}
// 本机已装的这个插件版本（没装过就 None）。回退只允许停在这一档以上；
// 读注册表失败也不拦着装——把「读不到」变成「装不了」是更糟的错。
fn local_version(context:&crate::storage_context::Context,id:&str)->Option<[u32;3]>{
    let manager=crate::plugin_install::Manager{context};
    let row=manager.list().ok()?.into_iter().find(|row|row.manifest.id==id)?;
    crate::plugin_manifest::version(&row.manifest.version).ok()
}
/// S1 版本候选：目录当前条目 + 目录历史条目（有则用）或内置已验证表（没有则用），
/// 按版本由新到旧排、按版本去重；第一条就是目录里的最新版。
fn version_candidates(catalog:&Catalog,id:&str,platform:&str,floor:Option<[u32;3]>)->Result<Vec<Entry>,String>{
    let latest=catalog.plugins.iter().find(|row|row.id==id&&row.platform==platform).cloned().ok_or("官方目录没有适用于当前平台的插件版本")?;
    let ceiling=crate::plugin_manifest::version(&latest.version)?;
    let mut fallbacks:Vec<(String,String,String,u64)>=Vec::new();
    if latest.versions.is_empty(){
        // 目录 schema 尚无 versions 字段（待插件仓配合）：回退内置已验证表。
        fallbacks.extend(KNOWN_GOOD.iter().filter(|row|row.0==id).map(|row|(row.1.to_string(),release_asset(row.0,row.1),row.2.to_string(),row.3)));
    }else{
        fallbacks.extend(latest.versions.iter().map(|row|(row.version.clone(),row.url.clone(),row.sha256.clone(),row.bytes)));
    }
    fallbacks.sort_by_key(|row|crate::plugin_manifest::version(&row.0).unwrap_or([0;3]));
    fallbacks.reverse();
    let mut candidates=vec![latest.clone()];
    for (version,url,sha256,bytes) in fallbacks{
        let Ok(release)=crate::plugin_manifest::version(&version) else { continue };
        // 比目录当前版本还新的候选不作回退目标：版本选择权留在目录里，宿主只负责
        // 「这一版下不到就退一步」，不负责替目录宣布谁是最新。
        if release>=ceiling||candidates.iter().any(|row|row.version==version) { continue; }
        // 2.1.4：也不许退到比本机已装版本更老的版本——那叫降级，不是回退。
        // 用户升级前用的是 2.2.x 时，回退应该停在 2.2.x 那一档，而不是一路退到内置表里
        // 还能下载的最老一版（历史现场：装成了 2.1.6，比升级前还旧）。
        if floor.is_some_and(|floor| release<floor) { continue }
        candidates.push(Entry{version,url,sha256,bytes,versions:Vec::new(),..latest.clone()});
    }
    Ok(candidates)
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
// S3 按来源分别降级（#68）：对每条来源，按候选版本由新到旧找「该来源最近一个可用
// 版本」。成功一次就停，返回的 effective_version 是**实际装上的版本**；安装提示据此
// 写「已安装 X；目录最新 Y 暂不可用」，而插件卡仍显示目录里的最新版（界面保留新版）。
// 校验不放宽：每个候选都用它自己的目录摘要（大小 + sha256 + manifest 身份三重比对），
// 任一候选校验失败只算该来源该候选失败，继续下一条，绝不拿下载结果冒充期望摘要。
// probe / fetch 都是注入点，测试不碰真网。
fn fetch_from(candidates:&[Entry],registry:&str,mirror:&str,root:&Path,
    mut probe:impl FnMut(&str)->Result<u16,String>,
    mut fetch:impl FnMut(&str,u64)->Result<Vec<u8>,String>,
)->Result<(crate::plugin_package::Package,String,FetchRecord,String),String>{
    let latest=candidates.first().map(|row|row.version.clone());
    // 回退说明用的版本：候选里的第一条更早版本（三源连它也下不到时，hint 要说清
    // 「已回退到过哪一版也没成」，而不是继续劝用户查代理）。
    let fallback=candidates.get(1).map(|row|row.version.clone());
    let mut attempts=Vec::new();
    // 每条来源只留一行失败原因（取最新候选的失败）：面板上一源一行，不因历史候选
    // 变成一堵墙；**逐候选的 attempts 在全失败时也一并交出去**（t18 F4：download_failed
    // 收下它塞进 JSON 的 attempts 字段，再随 recent.log 落盘），所以这句「可诊断性不丢」
    // 现在是成真的，不是空头承诺。
    let mut failures:Vec<crate::plugin_failure::SourceFailure>=Vec::new();
    for origin in [Origin::NpmRegistry,Origin::NpmMirror,Origin::GithubRelease] {
        let mut reported=false;
        for entry in candidates {
            let url=match origin {
                Origin::NpmRegistry=>npm_tarball(registry,&entry.id,&entry.version)?,
                Origin::NpmMirror=>npm_tarball(mirror,&entry.id,&entry.version)?,
                Origin::GithubRelease=>entry.url.clone(),
            };
            let began=std::time::Instant::now();
            // S2 探活：只有 404（不存在）/ 410（已下架）= 该来源该版本没有制品，完整下载
            // 一次都不发起。判据与 plugin_failure 的「制品缺失」共用 artifact_missing_status。
            // 403 不在其中（t18 F6）：限流/鉴权/CDN 拒绝 HEAD 都会给 403，它证明不了「这一版
            // 没有制品」，所以按「不定论」照旧完整下载，让真实错误串决定分类。
            if let Ok(status)=probe(&url) {
                if crate::plugin_failure::artifact_missing_status(Some(status)) {
                    let error=format!("官方商店请求失败（HTTP {status}）");
                    // 与下面失败路径同款的一行 stderr：探活判死的来源也要在控制台看得见，
                    // 否则「哪一源哪一版被跳过」只活在 JSON 里。
                    eprintln!("FISH Dock: 官方插件探活判死（{} {} HTTP {status}）：{url}",origin.slug(),entry.version);
                    attempts.push(Attempt{origin,url:url.clone(),ok:false,elapsed_ms:began.elapsed().as_millis() as u64,http_status:Some(status),phase:crate::plugin_failure::phase(&error).into(),error:Some(error.clone())});
                    if !reported { reported=true; failures.push(source_failure(origin,entry,&url,Some(status),&error)); }
                    continue;
                }
            }
            let outcome=match origin {
                // GitHub 资产就是一个 .fish-plugin：按该候选目录里的大小与摘要校验。
                Origin::GithubRelease=>fetch(&url,entry.bytes+1).and_then(|bytes|{
                    if bytes.len() as u64!=entry.bytes{return Err("下载的插件包大小与官方目录不一致".to_string());}
                    crate::plugin_package::inspect(&bytes,&entry.sha256,root,crate::storage::Source::OfficialCatalog,&crate::plugin_install::platform())
                        .and_then(|package|{
                            if package.manifest.id!=entry.id||package.manifest.version!=entry.version{return Err("插件包身份或版本与官方目录不一致".to_string());}
                            Ok((package,entry.sha256.to_ascii_lowercase()))
                        })
                }),
                // npm / 镜像：先下 tarball，再从里面取归档核验（package_from_npm）。
                _=>fetch(&url,MAX_DOWNLOAD+crate::config::npm_tarball_slack_bytes() as u64).and_then(|bytes|package_from_npm(&bytes,entry,root)),
            };
            let elapsed_ms=began.elapsed().as_millis() as u64;
            match outcome {
                Ok((package,digest))=>{
                    attempts.push(Attempt{origin,url:url.clone(),ok:true,elapsed_ms,http_status:Some(200),phase:"完成".into(),error:None});
                    let record=FetchRecord{id:entry.id.clone(),version:entry.version.clone(),origin,url:url.clone(),elapsed_ms,at:crate::now_ms(),attempts,catalog_latest:latest.clone()};
                    return Ok((package,digest,record,entry.version.clone()));
                }
                Err(error)=>{
                    eprintln!("FISH Dock: 官方插件下载来源失败（{} {} {}）：{error}",origin.slug(),entry.version,url);
                    let status=crate::plugin_failure::http_status(&error);
                    attempts.push(Attempt{origin,url:url.clone(),ok:false,elapsed_ms,http_status:status,phase:crate::plugin_failure::phase(&error).into(),error:Some(error.clone())});
                    if !reported { reported=true; failures.push(source_failure(origin,entry,&url,status,&error)); }
                }
            }
        }
    }
    // 三源（含各自的历史候选）全失败：面板拿 sources（每源一行），诊断拿 attempts（逐次）。
    let attempts_json:Vec<serde_json::Value>=attempts.iter().map(|attempt|serde_json::to_value(attempt).unwrap_or_default()).collect();
    Err(crate::plugin_failure::download_failed(&failures,latest.as_deref(),fallback.as_deref(),&attempts_json))
}
// 一条给面板看的失败行：来源名 + 版本 + 地址 + HTTP 状态码 + 失败阶段 + 短原因。
// JSON 键名 url / httpStatus / phase 是与前端约定的字段契约（ui-cards 按此渲染）。
fn source_failure(origin:Origin,entry:&Entry,url:&str,http_status:Option<u16>,error:&str)->crate::plugin_failure::SourceFailure{
    crate::plugin_failure::SourceFailure{origin:origin.label().into(),version:entry.version.clone(),url:url.to_string(),http_status,phase:crate::plugin_failure::phase(error).into(),short:crate::plugin_failure::source_short(error).to_string(),error:error.to_string()}
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
/// 只要「包 + 摘要」的简写口：安装本身走 obtain_traced（它还要那份来源记录，
/// #68 的回退提示也靠它带回实际装到的版本）。离线验收夹具仍用这一口，非测试
/// 构建里没有调用方，所以显式允许 dead_code。
#[allow(dead_code)]
pub fn obtain(context:&crate::storage_context::Context,id:&str)->Result<(crate::plugin_package::Package,String),String>{
    obtain_traced(context,id).map(|(package,digest,_)|(package,digest))
}
// obtain_traced 是带来源记录的对照口：插件卡要显示「这条源 + 耗时」，安装本身
// 只要前两项。三条来源的先后顺序就是用户要的「npm 优先、失败走镜像、再不行回落
// GitHub」；任何一条成功都直接返回，不再碰后面的。
// 【#68】目录里的最新版下不到时，这里按来源回退到上一可用版本，并把「实际装到的
// 版本 + 目录里的最新版」都写进 record（catalog_latest/fell_back），安装提示与
// 插件卡据此说「已安装 X；目录最新 Y 暂不可用」——目录数据本身一律不改写。
pub fn obtain_traced(context:&crate::storage_context::Context,id:&str)->Result<(crate::plugin_package::Package,String,FetchRecord),String>{
    let bytes=context.host.read("config","official-plugin-catalog.json")?.ok_or("请先刷新官方插件商店")?;
    let catalog=parse(&bytes)?;
    let candidates=version_candidates(&catalog,id,&crate::plugin_install::platform(),local_version(context,id))?;
    let latest=candidates.first().cloned().ok_or("官方目录没有适用于当前平台的插件版本")?;
    crate::plugin_progress::know_total(id,Some(latest.bytes));
    crate::plugin_progress::report(id,"校验",0,Some(latest.bytes))?;
    let registry=npm_registry();let mirror=npm_mirror();
    let (package,digest,record,effective)=fetch_from(&candidates,&registry,&mirror,&context.root,fetch_head,|url,limit|download(url,limit,Some(id)))?;
    let installed=candidates.iter().find(|row|row.version==effective).map_or(latest.bytes,|row|row.bytes);
    crate::plugin_progress::report(id,"提交",0,Some(installed))?;
    if let Err(error)=save_fetch(context,&record){eprintln!("FISH Dock: 插件下载来源记录写入失败（{}）：{error}",record.id);}
    Ok((package,digest,record))
}
#[cfg(test)]mod tests{
    use super::*;
    fn entry()->serde_json::Value{serde_json::json!({"id":"fish.magpie","name":"Magpie","description":"网关","author":"FISH","maintainer":"FISH","version":"2.5.3","hostApi":1,"platform":crate::plugin_install::platform(),"url":"https://github.com/Fish-under-sea/FISH-DOCK-Plugins/releases/download/magpie-v2.5.3/magpie.fish-plugin","sha256":"a".repeat(64),"bytes":4096,"dependencies":["Node 24"]})}
    fn new_entry()->serde_json::Value{serde_json::json!({"id":"fish.magpie","name":"Magpie","description":"网关","authors":["Elysia395","FISH"],"note":"此插件于 10 月 25 日暂停维护","version":"2.5.3","hostApi":1,"platform":crate::plugin_install::platform(),"url":"https://github.com/Fish-under-sea/FISH-DOCK-Plugins/releases/download/magpie-v2.5.3/magpie.fish-plugin","sha256":"a".repeat(64),"bytes":4096,"dependencies":["Node 24"]})}
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
        let catalog=parse(&bytes(vec![entry()])).unwrap();assert_eq!(catalog.plugins[0].id,"fish.magpie");
    }
    #[test]fn official_catalog_rejects_forgery_duplicates_and_foreign_downloads(){
        for (key,value) in [("id","acme.weather"),("id","fish.ofm"),("version","latest"),("url","https://example.test/magpie.fish-plugin"),("url","https://github.com/other/repo/releases/download/v1/p.fish-plugin"),("url","http://github.com/Fish-under-sea/FISH-DOCK-Plugins/releases/download/v1/p.fish-plugin"),("url","https://user@github.com/Fish-under-sea/FISH-DOCK-Plugins/releases/download/v1/p.fish-plugin")] {
            let mut row=entry();row[key]=value.into();assert!(parse(&bytes(vec![row])).is_err(),"{key}: {value}");
        }
        assert!(parse(&bytes(vec![entry(),entry()])).is_err());
        let mut row=entry();row["bytes"]=(MAX_DOWNLOAD+1).into();assert!(parse(&bytes(vec![row])).is_err());
    }
    // OFM 正式从官方商店下架（用户 2026-10-31）：宿主白名单不再认它，目录里也不会再有；
    // 仍在维护的三个官方插件照常可解析。
    #[test]fn official_catalog_no_longer_lists_or_accepts_a_retired_ofm(){
        let mut retired=entry(); retired["id"]="fish.ofm".into();
        assert!(parse(&bytes(vec![retired])).is_err(),"退役的 fish.ofm 不再是官方条目（白名单已同步移除）");
        let mut feishu=entry(); feishu["id"]="fish.feishu".into();
        let mut clash=entry(); clash["id"]="fish.clash".into();
        let magpie=entry();
        let catalog=parse(&bytes(vec![feishu,clash,magpie])).unwrap();
        let ids:Vec<&str>=catalog.plugins.iter().map(|row|row.id.as_str()).collect();
        assert_eq!(ids,vec!["fish.feishu","fish.clash","fish.magpie"],"商店只剩仍在维护的三个");
        assert!(!ids.contains(&"fish.ofm"),"商店列表里不再出现 OFM");
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
    fn plugin_archive_at(version:&str)->Vec<u8>{
        let backend=format!("console.log('magpie fixture {version}')").into_bytes();
        let readme=format!("# Magpie {version}").into_bytes();
        let raw=serde_json::json!({"schema":1,"id":"fish.magpie","name":"Magpie 网关","version":version,"hostApi":1,"platforms":[crate::plugin_install::platform()],"storage":{"directory":".fishplugin-magpie","dataSchema":1},"backend":{"runtime":"node","entry":"backend.mjs"},"behaviors":{"autostart":true},"files":{"backend.mjs":format!("{:x}",Sha256::digest(&backend)),"README.md":format!("{:x}",Sha256::digest(&readme))}});
        crate::identity_archive::zip(&BTreeMap::from([("plugin.json".into(),serde_json::to_vec(&raw).unwrap()),("backend.mjs".into(),backend),("README.md".into(),readme)])).unwrap()
    }
    fn plugin_archive()->Vec<u8>{plugin_archive_at("2.0.0")}
    // 目录条目走真实的 parse：下载地址、摘要、字节数与历史条目都按官方目录的规矩校验一遍。
    fn catalog_entry_at(archive:&[u8],version:&str,history:Vec<serde_json::Value>)->Entry{
        let mut row=serde_json::json!({"id":"fish.magpie","name":"Magpie","description":"网关","authors":["FISH"],"version":version,"hostApi":1,"platform":crate::plugin_install::platform(),
            "url":format!("https://github.com/Fish-under-sea/FISH-DOCK-Plugins/releases/download/v{version}/magpie.fish-plugin"),"sha256":format!("{:x}",Sha256::digest(archive)),"bytes":archive.len()});
        if !history.is_empty(){row["versions"]=serde_json::Value::Array(history);}
        parse(&bytes(vec![row])).unwrap().plugins.remove(0)
    }
    fn catalog_entry(archive:&[u8])->Entry{catalog_entry_at(archive,"2.0.0",Vec::new())}
    // 一条历史版本条目：版本/地址/摘要/大小，宿主只会从这种「官方目录形状」里取候选。
    fn history_of(archive:&[u8],version:&str)->serde_json::Value{
        serde_json::json!({"version":version,"url":format!("https://github.com/Fish-under-sea/FISH-DOCK-Plugins/releases/download/v{version}/magpie.fish-plugin"),"sha256":format!("{:x}",Sha256::digest(archive)),"bytes":archive.len()})
    }
    fn candidates(entry:&Entry)->Vec<Entry>{version_candidates(&Catalog{schema:1,plugins:vec![entry.clone()]},"fish.magpie",&crate::plugin_install::platform(),None).unwrap()}
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
    fn npm_package_of(archive:&[u8])->Vec<u8>{tarball(&[("magpie.fish-plugin",archive.to_vec()),("package.json",b"{\"name\":\"@fish-under-sea/fish-dock-plugin-magpie\"}".to_vec())])}
    const REGISTRY:&str="https://registry.npmjs.org";
    const MIRROR:&str="https://registry.npmmirror.com";
    fn fixture_root(tag:&str)->std::path::PathBuf{std::env::temp_dir().join(format!("fish-source-{tag}-{}-{}",std::process::id(),crate::now_ms()))}
    // 探活注入点：这个夹具里所有候选都「制品存在」。
    fn alive(_url:&str)->Result<u16,String>{Ok(200)}
    // npm 官方源成功：只发一次请求，镜像与 GitHub 一次都不碰。
    #[test]fn official_plugin_comes_from_the_npm_registry_first(){
        let archive=plugin_archive(); let entry=catalog_entry(&archive); let tgz=npm_package_of(&archive);
        let root=fixture_root("npm");
        let mut asked=Vec::new();
        let (package,digest,record,_effective)=fetch_from(&[entry.clone()],REGISTRY,MIRROR,&root,alive,|url,_limit|{asked.push(url.to_string());if url.contains("registry.npmjs.org"){Ok(tgz.clone())}else{Err("fixture: 不该走到后面那条源".into())}}).unwrap();
        assert_eq!(asked.len(),1,"npm 成了就不该再请求镜像与 GitHub");
        assert_eq!(record.origin,Origin::NpmRegistry);
        assert_eq!(record.attempts.len(),1);
        assert!(record.attempts[0].ok);
        // 成功行也要带两份面板要的字段：地址 + 状态码 + 阶段 + 耗时。
        assert_eq!(record.attempts[0].http_status,Some(200));
        assert_eq!(record.attempts[0].phase,"完成");
        assert!(record.attempts[0].url.contains("registry.npmjs.org"));
        assert_eq!((package.manifest.id.as_str(),package.manifest.version.as_str()),("fish.magpie","2.0.0"));
        assert_eq!(digest,entry.sha256.to_ascii_lowercase(),"npm 与 GitHub 装出来的摘要必须一模一样（重装才幂等）");
        assert_eq!(record.catalog_latest.as_deref(),Some("2.0.0"));
        assert!(record.fell_back().is_none(),"装的就是目录最新版，不该报回退");
        println!("npm 来源：{} ms，尝试 {} 次",record.elapsed_ms,record.attempts.len());
        let _=std::fs::remove_dir_all(root);
    }
    // 用户原话里的「第一次拉取失败想办法走镜像拉取」：registry 失败 → 自动换 npmmirror。
    #[test]fn a_failing_npm_registry_falls_back_to_the_mirror(){
        let archive=plugin_archive(); let entry=catalog_entry(&archive); let tgz=npm_package_of(&archive);
        let root=fixture_root("mirror");
        let mut asked=Vec::new();
        let (package,digest,record,_effective)=fetch_from(&[entry.clone()],REGISTRY,MIRROR,&root,alive,|url,_limit|{asked.push(url.to_string());if url.contains("registry.npmjs.org"){Err("fixture: 官方源连不上".into())}else if url.contains("registry.npmmirror.com"){Ok(tgz.clone())}else{Err("fixture: 不该走到 GitHub".into())}}).unwrap();
        assert_eq!(asked.len(),2,"官方源失败后应立刻换镜像，不越级");
        assert!(asked[1].contains("registry.npmmirror.com"));
        assert_eq!(record.origin,Origin::NpmMirror);
        assert_eq!(record.attempts.len(),2);
        assert!(!record.attempts[0].ok&&record.attempts[0].error.as_deref().is_some_and(|error|error.contains("官方源")));
        assert_eq!(record.attempts[0].phase,"连接","连不上是连接阶段");
        assert!(record.attempts[1].ok);
        assert_eq!((package.manifest.id.as_str(),digest.as_str()),("fish.magpie",entry.sha256.to_ascii_lowercase().as_str()));
        let _=std::fs::remove_dir_all(root);
    }
    // 两条 npm 都不通：回落 GitHub Release 资产，三条尝试都留在记录里。
    #[test]fn both_npm_sources_failing_falls_back_to_the_github_release(){
        let archive=plugin_archive(); let entry=catalog_entry(&archive);
        let root=fixture_root("github");
        let mut asked=Vec::new();
        let (package,digest,record,_effective)=fetch_from(&[entry.clone()],REGISTRY,MIRROR,&root,alive,|url,_limit|{asked.push(url.to_string());if url.contains("github.com"){Ok(archive.clone())}else{Err("fixture: npm 全挂".into())}}).unwrap();
        assert_eq!(asked.len(),3,"npm 官方源、镜像、GitHub 各试一次");
        assert_eq!(record.origin,Origin::GithubRelease);
        assert_eq!(record.attempts.iter().filter(|attempt|attempt.ok).count(),1);
        assert_eq!(package.manifest.id,"fish.magpie");
        assert_eq!(digest,entry.sha256.to_ascii_lowercase());
        let _=std::fs::remove_dir_all(root);
    }
    // npm 包里的归档与官方目录对不上（被人重打包过）：不许当官方插件装，三条都失败要报清楚。
    #[test]fn a_tarball_that_does_not_carry_the_cataloged_archive_is_refused(){
        let archive=plugin_archive(); let entry=catalog_entry(&archive);
        let wrong=npm_package_of(b"not the cataloged archive");
        let root=fixture_root("wrong");
        let error=match fetch_from(&[entry.clone()],REGISTRY,MIRROR,&root,alive,|url,_limit|{if url.contains("github.com"){Err("fixture: GitHub 也断了".into())}else{Ok(wrong.clone())}}){Ok(_)=>panic!("摘要对不上就不许装成官方插件"),Err(error)=>error};
        // t45：错误改结构化——一句人话 + 三源逐行短原因，原始串留 sources[].error。
        assert!(crate::plugin_failure::is_structured(&error),"{error}");
        let value: serde_json::Value=serde_json::from_str(&error).unwrap();
        assert!(!value["message"].as_str().unwrap().contains('；'),"message 不许拼三源：{error}");
        let rows=value["sources"].as_array().unwrap();
        assert_eq!(rows.len(),3,"三条来源各一行");
        let joined: String=rows.iter().map(|r|r["error"].as_str().unwrap_or("")).collect::<Vec<_>>().join(" | ");
        assert!(joined.contains("大小与官方目录不一致")||joined.contains("摘要与官方目录不一致"),"{joined}");
        // #68：逐源行还要给出地址与失败阶段，前端不再「只有一句短原因」。
        assert!(rows.iter().all(|r|r["url"].as_str().is_some_and(|url|url.starts_with("https://"))),"{value}");
        assert_eq!(rows[0]["phase"],"校验","大小/摘要不一致属校验阶段");
        assert!(rows[0]["httpStatus"].is_null(),"本地校验失败没有 HTTP 状态码");
        let _=std::fs::remove_dir_all(root);
    }
    // ── #68 版本候选 / 探活 / 按来源降级 ───────────────────────────────────
    // 一份「目录抢跑」夹具：目录当前版 2.0.1 的资产还没发布，历史条目 2.0.0 有。
    fn drift_catalog()->(Entry,Vec<u8>,Vec<u8>){
        let young=plugin_archive_at("2.0.1"); let old=plugin_archive_at("2.0.0");
        let entry=catalog_entry_at(&young,"2.0.1",vec![history_of(&old,"2.0.0")]);
        (entry,young,old)
    }
    #[test]fn three_sources_missing_the_new_version_fall_back_to_the_previous_one(){
        let (entry,young,old)=drift_catalog();
        let root=fixture_root("fallback");
        let mut downloaded=Vec::new();
        // 三源的最新版（2.0.1）都还没有制品；npm 两源连旧版也没同步，GitHub 的 2.0.0 有。
        let (package,digest,record,effective)=fetch_from(&candidates(&entry),REGISTRY,MIRROR,&root,
            |url|Ok(if url.contains("2.0.1"){404}else{200}),
            |url,_limit|{downloaded.push(url.to_string());
                if url.contains("2.0.1"){return Err(format!("fixture: 最新版还没有制品（{url}）"));}
                if url.contains("github.com"){Ok(old.clone())}else{Err("fixture: npm 两源都没同步 2.0.0".into())}}).unwrap();
        assert_eq!(effective,"2.0.0","三源的最新版全灭后必须回退到上一可用版本");
        assert_eq!(record.version,"2.0.0");
        assert_eq!(record.origin,Origin::GithubRelease,"三条来源各自的最近可用版本都被走过一遍");
        assert_eq!(record.catalog_latest.as_deref(),Some("2.0.1"),"目录里的最新版要留在记录里（界面仍显示新版）");
        assert_eq!(record.fell_back(),Some("2.0.1"));
        assert_eq!(package.manifest.version,"2.0.0");
        assert_eq!(digest,format!("{:x}",Sha256::digest(&old)),"回退版本仍用目录摘要校验");
        assert_ne!(digest,format!("{:x}",Sha256::digest(&young)),"回退版本的摘要不是最新版的");
        // 探活命中就跳过：完整下载一次都没碰 2.0.1。
        assert!(downloaded.iter().all(|url|!url.contains("2.0.1")),"探活 404 的候选不该再跑完整下载：{downloaded:?}");
        let skipped:Vec<&Attempt>=record.attempts.iter().filter(|attempt|attempt.http_status==Some(404)).collect();
        assert_eq!(skipped.len(),3,"三条来源各记一条 404（面板要能逐源显示）");
        assert!(skipped.iter().all(|attempt|attempt.phase=="HTTP"&&!attempt.ok&&attempt.url.contains("2.0.1")));
        assert_eq!(record.attempts.last().map(|attempt|(attempt.ok,attempt.http_status,attempt.phase.as_str())),Some((true,Some(200),"完成")));
        let _=std::fs::remove_dir_all(root);
    }
    // 验收④：探活命中 404 时，该来源该版本的**完整下载调用次数 = 0**。
    #[test]fn a_probe_miss_costs_zero_full_downloads(){
        let (entry,_young,_old)=drift_catalog();
        let root=fixture_root("probe");
        let mut full=0usize;
        let _=fetch_from(&candidates(&entry),REGISTRY,MIRROR,&root,|url|Ok(if url.contains("2.0.1"){404}else{200}),
            |url,_limit|{if url.contains("2.0.1"){full+=1;}Err(format!("fixture: 不该下载（{url}）"))});
        assert_eq!(full,0,"探活已经说了没有制品，完整下载次数必须是 0");
        let _=std::fs::remove_dir_all(root);
    }
    // t18 F6 选项①：403 不是「这一版没制品」（限流/鉴权/CDN 拒 HEAD），所以**不跳过**，
    // 照旧完整下载，让真实错误串决定分类。这条锁住语义，防止有人再把 403 塞回跳过名单。
    #[test]fn a_probe_403_does_not_skip_the_download(){
        let (entry,young,old)=drift_catalog();
        let root=fixture_root("probe-403");
        let mut full=0usize;
        let archive=plugin_archive_at("2.0.1");
        // 探活：npm 官方源给 403，其它给 200；完整下载：官方源 2.0.1 成功 → 直接装最新版。
        let (package,_digest,record,effective)=fetch_from(&candidates(&entry),REGISTRY,MIRROR,&root,
            |url|Ok(if url.contains("registry.npmjs.org")&&url.contains("2.0.1"){403}else{200}),
            |url,_limit|{full+=1;if url.contains("2.0.1"){Ok(npm_package_of(&archive))}else{Ok(npm_package_of(&old))}}).unwrap();
        assert_eq!(full,1,"403 不定论：该来源该版本仍要完整下载一次");
        assert_eq!(effective,"2.0.1","探活 403 不该把可用的最新版判死");
        assert_eq!(package.manifest.version,"2.0.1");
        assert_eq!(record.origin,Origin::NpmRegistry);
        let _=std::fs::remove_dir_all(root);
    }
    // t18 F3：三源（含各自候选）全 404 → 结构化 JSON 交出去后要能落 recent.log，
    // 里面读得到 404 与逐源 url（plugin_api 的 map_err 走的就是 log_structured 这一口）。
    #[test]fn a_three_source_404_failure_reaches_recent_log_with_urls(){
        let (entry,_young,_old)=drift_catalog();
        let root=fixture_root("recent-log");
        let data=std::env::temp_dir().join(format!("fish-store-log-{}-{}",std::process::id(),crate::now_ms()));
        std::fs::create_dir_all(&data).unwrap();
        let error=match fetch_from(&candidates(&entry),REGISTRY,MIRROR,&root,|url|Ok(if url.contains("2.0.1"){404}else{200}),|url,_limit|Err(format!("fixture: 没有制品（{url}）"))){Ok(_)=>panic!("三源全 404 必须报错"),Err(error)=>error};
        crate::plugin_failure::log_structured(&data,"download",&error);
        let logged=std::fs::read_to_string(data.join("recent.log")).unwrap_or_default();
        for needle in ["HTTP 404","registry.npmjs.org","registry.npmmirror.com","github.com","fishFailure","attempts"] {
            assert!(logged.contains(needle),"{needle} 没进 recent.log：{logged}");
        }
        assert_eq!(logged.matches("[plugin] download:").count(),1,"同一份失败只记一行");
        let _=std::fs::remove_dir_all(root);
        let _=std::fs::remove_dir_all(&data);
    }
    // 验收③：按来源分别降级——npm 官方源只有旧版，就装旧版；新版仍从能给的来源拿。
    #[test]fn sources_choose_their_own_newest_usable_version(){
        let (entry,young,old)=drift_catalog();
        let tgz_young=npm_package_of(&young); let tgz_old=npm_package_of(&old);
        let root=fixture_root("per-source-1");
        // npm 官方源：2.0.1 的 tarball 404；镜像与 GitHub 都有最新版。
        let (package,_digest,record,effective)=fetch_from(&candidates(&entry),REGISTRY,MIRROR,&root,
            |url|Ok(if url.contains("registry.npmjs.org")&&url.contains("2.0.1"){404}else{200}),
            |url,_limit|match (url.contains("registry.npmjs.org"),url.contains("2.0.1")){
                (true,true)=>Err("fixture: 官方源还没发这一版".into()),
                (true,false)=>Ok(tgz_old.clone()),
                (_,true)=>Ok(tgz_young.clone()),
                (_,false)=>Ok(tgz_old.clone()),
            }).unwrap();
        assert_eq!(record.origin,Origin::NpmRegistry,"来源顺序不变：npm 先给得出就不越级");
        assert_eq!(effective,"2.0.0","官方源退到它自己最近可用的 2.0.0");
        assert_eq!(package.manifest.version,"2.0.0");
        assert_eq!(record.attempts.len(),2,"官方源：2.0.1 探活命中后直接取 2.0.0");
        assert_eq!(record.attempts[0].http_status,Some(404));
        let _=std::fs::remove_dir_all(root);
        // 两条 npm 都一个可用版本都没有，但 GitHub 有最新版：不许因为「前面两条源没货」
        // 就把能拿到的新版弄丢（降级只发生在**同一条来源内部**，来源顺序不变）。
        let root=fixture_root("per-source-2");
        let (package,_digest,record,effective)=fetch_from(&candidates(&entry),REGISTRY,MIRROR,&root,
            |url|Ok(if url.contains("github.com"){200}else{404}),
            |url,_limit|{if url.contains("github.com"){Ok(young.clone())}else{Err("fixture: npm 两源都没同步这一版".into())}}).unwrap();
        assert_eq!(record.origin,Origin::GithubRelease);
        assert_eq!(effective,"2.0.1","GitHub 有最新版就装最新版，不盲目降级");
        assert_eq!(package.manifest.version,"2.0.1");
        assert!(record.fell_back().is_none());
        let _=std::fs::remove_dir_all(root);
    }
    // 验收⑤：摘要校验不放宽——目录给 2.0.1 记了错摘要时，那一版不许装，
    // 更不许拿下载到的包自算摘要顶上；同一批里 2.0.0 仍按目录摘要装上。
    #[test]fn a_wrong_catalog_digest_never_becomes_the_installed_digest(){
        let young=plugin_archive_at("2.0.1"); let old=plugin_archive_at("2.0.0");
        let mut row=serde_json::json!({"id":"fish.magpie","name":"Magpie","description":"网关","authors":["FISH"],"version":"2.0.1","hostApi":1,"platform":crate::plugin_install::platform(),
            "url":"https://github.com/Fish-under-sea/FISH-DOCK-Plugins/releases/download/v2.0.1/magpie.fish-plugin","sha256":"b".repeat(64),"bytes":young.len(),"versions":[history_of(&old,"2.0.0")]});
        let entry=parse(&bytes(vec![row.clone()])).unwrap().plugins.remove(0);
        let tgz=npm_package_of(&young);
        let root=fixture_root("wrong-digest");
        let (package,digest,_record,effective)=fetch_from(&candidates(&entry),REGISTRY,MIRROR,&root,alive,
            |url,_limit|{if url.contains("2.0.1"){if url.contains("github.com"){Ok(young.clone())}else{Ok(tgz.clone())}}else if url.contains("github.com"){Ok(old.clone())}else{Ok(npm_package_of(&old))}}).unwrap();
        assert_eq!(effective,"2.0.0","错摘要的那一版不许装");
        assert_eq!(package.manifest.version,"2.0.0");
        assert_eq!(digest,format!("{:x}",Sha256::digest(&old)),"装上的摘要必须来自目录历史条目");
        assert_ne!(digest,format!("{:x}",Sha256::digest(&young)),"绝不许用下载到的包自算摘要");
        assert_ne!(digest,"b".repeat(64),"目录里那条错摘要更不许被当成事实");
        let _=std::fs::remove_dir_all(root);
        // 连历史条目也没有时，三源全灭：报目录类失败，且一句「不用改网络」。
        row["versions"]=serde_json::json!([]);
        let entry=parse(&bytes(vec![row])).unwrap().plugins.remove(0);
        let root=fixture_root("wrong-digest-only");
        let error=match fetch_from(&candidates(&entry),REGISTRY,MIRROR,&root,alive,|_url,_limit|Ok(tgz.clone())){Ok(_)=>panic!("错摘要不许装成任何版本"),Err(error)=>error};
        let value: serde_json::Value=serde_json::from_str(&error).unwrap();
        assert_eq!(value["kind"],"mismatch","包下到了但与目录对不上：不是目录抢跑，也不是网络：{value}");
        assert!(!value["hint"].as_str().unwrap().contains("检查网络"),"摘要不符不许支用户去查代理：{value}");
        let rows=value["sources"].as_array().unwrap();
        assert_eq!(rows.len(),3,"三条来源各一行");
        assert!(rows.iter().all(|r|r["phase"]=="摘要"||r["phase"]=="校验"),"错摘要记在校验/摘要阶段：{value}");
        let _=std::fs::remove_dir_all(root);
    }
    // 验收①：版本候选优先读目录的历史条目（摘要与目录同源），没有才回退内置表。
    #[test]fn version_candidates_prefer_the_catalog_history_then_the_builtin_table(){
        let prev=plugin_archive_at("2.4.0"); let entry=catalog_entry_at(&plugin_archive_at("2.5.0"),"2.5.0",vec![history_of(&prev,"2.4.0")]);
        let catalog=Catalog{schema:1,plugins:vec![entry.clone()]};
        let rows=version_candidates(&catalog,"fish.magpie",&crate::plugin_install::platform(),None).unwrap();
        let versions:Vec<&str>=rows.iter().map(|row|row.version.as_str()).collect();
        assert_eq!(versions,vec!["2.5.0","2.4.0"],"目录历史条目在手时不再叠内置表（内置表最高的 2.2.0 都不该出现）");
        assert_eq!(rows[1].sha256,format!("{:x}",Sha256::digest(&prev)),"历史条目的摘要原样带过来");
        assert!(rows[1].versions.is_empty(),"候选不再带着整份历史（避免递归）");
        // 目录没有 versions 字段（当前 stable.json 的形态）→ 内置已验证表顶上。
        let catalog=Catalog{schema:1,plugins:vec![catalog_entry_at(&plugin_archive_at("2.3.0"),"2.3.0",Vec::new())]};
        let rows=version_candidates(&catalog,"fish.magpie",&crate::plugin_install::platform(),None).unwrap();
        assert_eq!(rows[0].version,"2.3.0");
        let builtin:Vec<&str>=rows[1..].iter().map(|row|row.version.as_str()).collect();
        assert!(builtin.contains(&"2.2.0")&&builtin.contains(&"2.1.6"),"内置表要有上一可用版本：{builtin:?}");
        assert!(builtin.windows(2).all(|pair|crate::plugin_manifest::version(pair[0]).unwrap()>crate::plugin_manifest::version(pair[1]).unwrap()),"内置候选按新→旧排：{builtin:?}");
        assert!(rows.iter().all(|row|crate::plugin_manifest::version(&row.version).unwrap()<=crate::plugin_manifest::version("2.3.0").unwrap()),"内置表里比目录更新的版本不作回退目标");
        assert!(version_candidates(&catalog,"fish.ofm",&crate::plugin_install::platform(),None).is_err(),"目录里没有的 id 不给候选");
    }
    // 内置表的每一条都必须是官方发行资源 + 合法摘要：它决定「装哪一版」，不能靠运气。
    // 地址不再逐行抄写：release_asset() 用 config::PLUGIN_RELEASE_BASE 拼，所以这里同时
    // 锁住「基址是唯一配置入口」与「拼出来的地址必须过官方发行校验」。
    #[test]fn the_builtin_known_good_table_only_holds_official_artifacts(){
        assert!(!KNOWN_GOOD.is_empty());
        for (id,version,sha,_bytes) in KNOWN_GOOD{
            assert!(["fish.feishu","fish.clash","fish.magpie"].contains(id),"内置表只收官方插件：{id}");
            let url=release_asset(id,version);
            assert!(url.starts_with(crate::config::PLUGIN_RELEASE_BASE),"地址必须从 config 的基址拼：{url}");
            assert!(url.contains(&format!("/v{version}/{}.fish-plugin",id.trim_start_matches("fish."))),"{url}");
            release_url(&url).unwrap_or_else(|error|panic!("{id} {version} 的地址不是官方发行资源：{error}"));
            assert_eq!(sha.len(),64,"{id} {version} 的摘要长度不对");
            assert!(sha.bytes().all(|b|b.is_ascii_hexdigit()),"{id} {version} 的摘要不是十六进制");
            crate::plugin_manifest::version(version).unwrap();
        }
        // 同一版号只许有一个摘要（历史上「同版重打包」的版本不收录）。
        for (id,version,sha,_) in KNOWN_GOOD{
            assert_eq!(KNOWN_GOOD.iter().filter(|row|row.0==*id&&row.1==*version).count(),1,"{id} {version} 重复");
            let _=sha;
        }
    }
    // t18 F2：跳转判据 = 官方发行资产域 或「与当前请求同站点域」；跨站一律拒。
    #[test]fn a_redirect_may_only_land_on_the_same_site_or_a_release_asset_host(){
        let from=|url:&str|tauri::Url::parse(url).unwrap();
        // ① 镜像源自有 CDN：registry.npmmirror.com → cdn.npmmirror.com 放行（#68 的真实断点）
        let host=from("https://registry.npmmirror.com/@fish-under-sea/fish-dock-plugin-magpie/-/fish-dock-plugin-magpie-2.1.7.tgz");
        let next=hop(&host,Some("https://cdn.npmmirror.com/packages/@fish-under-sea/fish-dock-plugin-magpie/-/fish-dock-plugin-magpie-2.1.7.tgz")).expect("同站点域必须放行");
        assert_eq!(next.host_str(),Some("cdn.npmmirror.com"));
        // ② GitHub 发行资产：github.com → release-assets / objects 仍然放行
        let github=from("https://github.com/Fish-under-sea/FISH-DOCK-Plugins/releases/download/v2.1.7/magpie.fish-plugin");
        for asset in ["https://release-assets.githubusercontent.com/x","https://objects.githubusercontent.com/y"] {
            assert_eq!(hop(&github,Some(asset)).unwrap().host_str(),tauri::Url::parse(asset).unwrap().host_str(),"{asset}");
        }
        // ③ 无关域仍然拒：既不是发行资产域，也不同站点
        for evil in ["https://evil.example/magpie.tgz","https://npmmirror.com.evil.example/x.tgz"] {
            let error=hop(&host,Some(evil)).expect_err("{evil} 必须被拒");
            assert!(error.contains("跳转不属于官方发行资源服务"),"{evil} → {error}");
        }
        // ④ 非 HTTPS 一律拒（哪怕同站点域）
        let error=hop(&host,Some("http://cdn.npmmirror.com/x.tgz")).expect_err("http 必须被拒");
        assert!(error.contains("HTTPS")||error.contains("跳过"));
        // ⑤ 带身份信息/端口/片段的地址也拒
        assert!(hop(&host,Some("https://cdn.npmmirror.com:8443/x.tgz")).is_err());
    }
    // 目录的历史条目同样要过一遍官方校验：地址、摘要、大小、数量都按当前条目的规矩。
    #[test]fn catalog_history_entries_are_validated_like_the_current_one(){
        let old=plugin_archive_at("2.0.0");
        for (key,value) in [("url",serde_json::json!("https://example.test/magpie.fish-plugin")),
            ("url",serde_json::json!("https://github.com/other/repo/releases/download/v2.0.0/magpie.fish-plugin")),
            ("sha256",serde_json::json!("not-a-digest")),
            ("bytes",serde_json::json!(0)),
            ("version",serde_json::json!("latest"))] {
            let mut history=history_of(&old,"2.0.0"); history[key]=value.clone();
            let row=serde_json::json!({"id":"fish.magpie","name":"Magpie","description":"网关","authors":["FISH"],"version":"2.0.1","hostApi":1,"platform":crate::plugin_install::platform(),
                "url":"https://github.com/Fish-under-sea/FISH-DOCK-Plugins/releases/download/v2.0.1/magpie.fish-plugin","sha256":"a".repeat(64),"bytes":4096,"versions":[history]});
            assert!(parse(&bytes(vec![row])).is_err(),"历史条目 {key}={value} 必须被拒");
        }
        let mut many:Vec<serde_json::Value>=(0..17).map(|index|history_of(&old,&format!("2.0.{index}"))).collect();
        let mut row=serde_json::json!({"id":"fish.magpie","name":"Magpie","description":"网关","authors":["FISH"],"version":"3.0.0","hostApi":1,"platform":crate::plugin_install::platform(),
            "url":"https://github.com/Fish-under-sea/FISH-DOCK-Plugins/releases/download/v3.0.0/magpie.fish-plugin","sha256":"a".repeat(64),"bytes":4096,"versions":many.clone()});
        assert!(parse(&bytes(vec![row.clone()])).is_err(),"历史条目过多要拒");
        many.truncate(2);
        row["versions"].clone_from(&serde_json::json!(many));
        assert!(parse(&bytes(vec![row])).is_ok(),"两条历史条目是正常形态");
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
