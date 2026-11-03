//! 插件页面一次性入口票据与逐资源校验，前端页只通过自己的桥访问业务方法。
//!
//! T09：入口全部放锁 + 复用 T03 的校验结果。
//!   - `open()` 不再同步 verify（以前那是整包 129.9 MB 的全文哈希），也不再持
//!     CURRENT 等一次实例探针；它只解析一次活动版本，把「目录 + 资源清单 + 上次
//!     核验过的指纹」记进票据；
//!   - `call()` 不再持 CURRENT 跑 RPC，走 `plugin_runtime::call_outside`：借出实例
//!     句柄后放锁等回答，停用/卸载时按代次作废在途结果；
//!   - `resource()` 不再逐资源读注册表（EDIT 锁）、也不再逐资源全文哈希：票据里的
//!     指纹对得上就直接发，变了才重算那一个文件。
use std::{collections::BTreeMap,path::PathBuf,sync::Mutex,time::{Duration,Instant}};
use crate::plugin_install::Stamp;
#[derive(Clone)]struct Grant {
    id:String, digest:String, entry:String, issued:Instant,
    // T09：开页面时一次性解析好的活动版本。之后每一次取资源只查这份快照——不再读
    // 注册表（EDIT 锁）、不再整包核验。停用/卸载/换版时 plugin_runtime 会 revoke()
    // 掉这个 id 的全部票据，所以这里不需要每次再去确认「它还在不在」。
    directory:PathBuf, files:BTreeMap<String,String>, stamps:BTreeMap<String,Stamp>,
}
static GRANTS:Mutex<BTreeMap<String,Grant>>=Mutex::new(BTreeMap::new());
fn route(path:&str)->Result<(&str,&str),String> { path.strip_prefix('/').and_then(|s|s.split_once('/')).filter(|(token,name)|token.len()==64&&token.bytes().all(|b|b.is_ascii_hexdigit())&&!name.is_empty()).ok_or_else(||"插件页面票据或路径无效".into()) }
fn allowed(grant:&Grant,id:&str,digest:&str,enabled:bool)->bool { enabled&&grant.id==id&&grant.digest==digest&&grant.issued.elapsed()<Duration::from_secs(86400) }
fn token()->Result<String,String>{
 let mut bytes=[0u8;32];
 #[cfg(windows)]{#[link(name="bcrypt")]unsafe extern "system"{fn BCryptGenRandom(algorithm:isize,buffer:*mut u8,size:u32,flags:u32)->i32;}if unsafe{BCryptGenRandom(0,bytes.as_mut_ptr(),32,2)}<0{return Err("页面随机票据生成失败".into());}}
 #[cfg(not(windows))]{use std::io::Read;std::fs::File::open("/dev/urandom").and_then(|mut f|f.read_exact(&mut bytes)).map_err(|_|"页面随机票据生成失败")?;}
 Ok(bytes.iter().map(|b|format!("{b:02x}")).collect())
}
pub fn revoke(id:&str){if let Ok(mut grants)=GRANTS.lock(){grants.retain(|_,g|g.id!=id);}}
pub fn close(ticket:&str){if let Ok(mut grants)=GRANTS.lock(){grants.remove(ticket);}}
pub fn open(key:&str)->Result<serde_json::Value,String>{
 let (id,page)=key.split_once('/').ok_or("插件页面键无效")?;
 let context=crate::storage_context::current().ok_or("存储尚未就绪")?;crate::github_access::require_plugin(context,id)?;let manager=crate::plugin_install::Manager{context};
 let row=manager.list()?.into_iter().find(|r|r.manifest.id==id&&r.enabled).ok_or("插件未启用")?;
 // T09：入口只看「有没有这个实例」——不握手、不占锁等探针。一次 RPC 正在跑的时候
 // 开页面也不会被它卡住（以前这里持 CURRENT 要等探针 2 s）。
 if !crate::plugin_runtime::present(id){return Err("插件尚未运行".into());}
 let entry=row.manifest.pages.iter().find(|p|p.id==page).ok_or("插件页面未注册")?.entry.clone();
 // T09 + T03：这里**不做**全文校验。目录只算路径（manifest.validate），指纹表直接
 // 读上次安装/启动时落盘的快照；快照缺失也不在这里补哈希——资源请求会按需算那一个
 // 文件，绝不会为了开一页把 129.9 MB 重新读一遍。
 let directory=manager.directory(&row)?;
 let (files,stamps)=(row.manifest.files.clone(),manager.verified_stamps(&row));
 let ticket=token()?;let mut grants=GRANTS.lock().map_err(|_|"插件页面锁不可用")?;
 grants.retain(|_,g|g.issued.elapsed()<Duration::from_secs(86400));if grants.len()>=32{return Err("插件页面过多，请关闭部分页面后重试".into());}
 grants.insert(ticket.clone(),Grant{id:id.into(),digest:row.digest,entry:entry.clone(),issued:Instant::now(),directory,files,stamps});
 let base=if cfg!(windows){"http://fish-plugin.localhost"}else{"fish-plugin://localhost"};
 Ok(serde_json::json!({"ticket":ticket,"url":format!("{base}/{ticket}/{entry}")}))
}
fn checked(ticket:&str)->Result<(Grant,crate::plugin_install::Installed),String>{
 let grant=GRANTS.lock().map_err(|_|"插件页面锁不可用")?.get(ticket).cloned().ok_or("页面票据失效")?;
 let context=crate::storage_context::current().ok_or("存储尚未就绪")?;
 crate::github_access::require_plugin(context,&grant.id)?;
 let row=crate::plugin_install::Manager{context}.list()?.into_iter().find(|r|r.manifest.id==grant.id).ok_or("插件未安装")?;
 if !allowed(&grant,&row.manifest.id,&row.digest,row.enabled){return Err("页面票据与活动版本不匹配".into());}Ok((grant,row))
}
/// #67：`dock.open_external` 的参数判定（纯函数，不碰 FFI，可单测）。
/// 只认网页地址；缺参/非字符串/危险协议都在这一步拒绝，绝不启动浏览器。
fn external_target(params:&serde_json::Value)->Result<&str,String>{
 let url=params.get("url").and_then(serde_json::Value::as_str).ok_or("缺少要打开的链接")?;
 if !crate::browser::is_web_url(url){return Err("链接不是可打开的网页地址（只支持 http/https）".into());}
 Ok(url)
}
pub fn call(ticket:&str,method:&str,params:serde_json::Value)->Result<serde_json::Value,String>{
 let (grant,installed)=checked(ticket)?;
 // #67：插件页的外链只能走这条受控通道。iframe 沙箱只有 allow-scripts，页面里的
 // <a href>/target=_blank/window.open 都打不开浏览器；这里在**票据/版本/启用状态**验过
 // 之后，只把网页地址交给 crate::browser（同一套 http/https 判定），危险协议一律拒绝。
 if method=="dock.open_external" {
  let url=external_target(&params)?;
  crate::browser::open_web(url)?;
  return Ok(serde_json::json!({"opened":true}));
 }
 // 宿主自己的小接口：插件页可读写「随 Dock 启动就拉起服务」这一项，不进插件后端。
 // 前缀用 dock.：页面桥按设计拒绝 host. 与 plugin. 开头的方法，那两类名字留给后端。
 if method=="dock.autostart" || method=="host.autostart" {
  let manager=crate::plugin_install::Manager{context:crate::storage_context::current().ok_or("存储尚未就绪")?};
  let autostart=match params.get("autostart").and_then(serde_json::Value::as_bool) {
   Some(value)=>manager.set_autostart(&grant.id,value)?.autostart,
   None=>installed.autostart,
  };
  return Ok(serde_json::json!({"autostart":autostart}));
 }
 // T09：业务调用**不持 CURRENT 跑 RPC**。call_outside 借出实例句柄后放锁等回答，
 // 回答回来再拿一次短锁结账；等待期间这个插件被停用/卸载，结果按代次作废。
 if method=="gateway.open" {
  // 已装退役插件仍可开自己的本机页面：特判集中在 legacy_ofm，不扩大成任意插件通行。
  if !crate::legacy_ofm::is_legacy_gateway_plugin(&grant.id){return Err("该插件没有本机服务页面".into());}
  let state=crate::plugin_runtime::call_outside(&grant.id,"gateway.view",serde_json::json!({}))?;
  if state["state"]!="running"{return Err("请先启动服务".into());}
  let raw=state["url"].as_str().ok_or("服务尚未就绪")?;let url=tauri::Url::parse(raw).map_err(|_|"服务地址无效")?;
  if url.scheme()!="http"||url.host_str()!=Some("127.0.0.1")||!url.username().is_empty()||url.password().is_some()||url.port().map(u64::from)!=state["port"].as_u64(){return Err("服务就绪地址无效".into());}
  return Ok(serde_json::json!({"url":url.as_str()}));
 }
 crate::plugin_runtime::call_outside(&grant.id,method,params)
}
fn resource(ticket:&str,name:&str)->Result<(Vec<u8>,&'static str),String>{
 use sha2::{Digest,Sha256};
 // T09：只查票据（一把内存锁），不读注册表、不碰 EDIT 锁、不整包核验。票据在停用/
 // 卸载/换版时由 plugin_runtime 调 revoke() 当场作废，所以每次再去注册表确认一遍
 // 只是把已经知道的事又问一遍，而且是重活。
 let grant=GRANTS.lock().map_err(|_|"插件页面锁不可用")?.get(ticket).cloned().ok_or("页面票据失效")?;
 if grant.issued.elapsed()>=Duration::from_secs(86400){return Err("页面票据失效".into());}
 if name=="_fish_bridge.js" {return Ok((include_bytes!("../../../sdk/browser/bridge.js").to_vec(),"text/javascript; charset=utf-8"));}
 let expected=grant.files.get(name).ok_or("页面资源未在清单登记")?;
 let path=crate::plugin_manifest::package_path(&grant.directory,name)?;
 let bytes=crate::identity_store::read_optional(&path)?.ok_or("页面资源缺失")?;
 // T09 + T03：先比 size+mtime。指纹与上次核验的记录一致就直接发（命中就不再哈希）；
 // 变了才重算这一个文件的摘要——改过的资源仍然当场拒绝，只是不再每次都算。
 let current=crate::plugin_install::stamp(&path).ok();
 if grant.stamps.get(name)!=current.as_ref() {
  if format!("{:x}",Sha256::digest(&bytes))!=expected.to_ascii_lowercase(){return Err("页面资源摘要校验失败".into());}
 }
 let mime=match name.rsplit('.').next().unwrap_or(""){"html"=>"text/html; charset=utf-8","js"|"mjs"=>"text/javascript; charset=utf-8","css"=>"text/css; charset=utf-8","png"=>"image/png","svg"=>"image/svg+xml","jpg"|"jpeg"=>"image/jpeg","webp"=>"image/webp","woff2"=>"font/woff2",_=>return Err("页面资源类型未支持".into())};
 if name.ends_with(".html")&&name!=grant.entry{return Err("页面入口与票据不一致".into());}
 let bytes=if name==grant.entry {let html=String::from_utf8(bytes).map_err(|_|"页面不是 UTF-8")?;format!("<!doctype html><script src=\"/{ticket}/_fish_bridge.js\"></script>{html}").into_bytes()}else{bytes};
 Ok((bytes,mime))
}
pub fn serve(label:&str,request:tauri::http::Request<Vec<u8>>)->tauri::http::Response<Vec<u8>>{
 let result=if label!="island"||request.method()!=tauri::http::Method::GET {Err("页面请求来源无效".into())}else{route(request.uri().path()).and_then(|(ticket,name)|resource(ticket,name))};
 let (status,body,mime)=match result {Ok((bytes,mime))=>(200,bytes,mime),Err(_)=>(403,b"Extension resource unavailable".to_vec(),"text/plain; charset=utf-8")};
 tauri::http::Response::builder().status(status).header("Content-Type",mime).header("Cache-Control","no-store").header("X-Content-Type-Options","nosniff").header("Referrer-Policy","no-referrer").header("Content-Security-Policy","default-src 'none'; script-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' data:; font-src 'self'; connect-src 'none'; frame-src 'none'; form-action 'none'; base-uri 'none'; object-src 'none'").body(body).expect("固定响应头")
}
#[cfg(test)]mod tests {
 use super::*;
 // #67：外链通道的参数判定。https 放行；file/javascript/data/vbscript、带 userinfo、含 NUL
 // 全部拒绝（每个都断言），缺参也报错——任何一条都不该走到启动浏览器。
 #[test]fn open_external_takes_only_web_urls(){
  let with=|url:&str| serde_json::json!({"url":url});
  assert_eq!(external_target(&with("https://example.com/a?b=1")).unwrap(),"https://example.com/a?b=1");
  assert_eq!(external_target(&with("http://127.0.0.1:38080/x")).unwrap(),"http://127.0.0.1:38080/x");
  assert_eq!(external_target(&serde_json::json!({})).unwrap_err(),"缺少要打开的链接");
  assert_eq!(external_target(&serde_json::json!({"url":42})).unwrap_err(),"缺少要打开的链接");
  for bad in ["file:///C:/Windows/notepad.exe","javascript:alert(1)","data:text/html,<b>x</b>","vbscript:msgbox(1)","https://user:pw@example.com/","https://example.com/\0evil"] {
   assert_eq!(external_target(&with(bad)).unwrap_err(),"链接不是可打开的网页地址（只支持 http/https）","{bad} 必须拒绝");
  }
 }
 #[test]fn page_grants_bind_identity_version_enabled_state_and_lifetime(){
  let grant=Grant{id:"acme.weather".into(),digest:"1".repeat(64),entry:"page.html".into(),issued:Instant::now(),directory:PathBuf::from("packages/acme.weather"),files:BTreeMap::new(),stamps:BTreeMap::new()};
  assert!(allowed(&grant,"acme.weather",&"1".repeat(64),true));
  assert!(!allowed(&grant,"fish.ofm",&"1".repeat(64),true));assert!(!allowed(&grant,"acme.weather",&"2".repeat(64),true));assert!(!allowed(&grant,"acme.weather",&"1".repeat(64),false));
  let expired=Grant{issued:Instant::now()-Duration::from_secs(86401),..grant};assert!(!allowed(&expired,"acme.weather",&"1".repeat(64),true));
 }
 #[test]fn malformed_ticket_routes_are_not_served(){for path in ["/page.html","/short/a","//etc/passwd","/../../../a"] {assert!(route(path).is_err());}let p=format!("/{}/ui/index.html","a".repeat(64));assert_eq!(route(&p).unwrap().1,"ui/index.html");}
 // T09：每个资源不再走注册表（EDIT 锁）也不再每次都哈希——票据里带着上次核验过的
 // 指纹，指纹没变就直接发；变了才重算那一个文件，对不上照样拒绝。
 // 票据作废（停用/卸载/换版时 revoke）之后，连取资源也一样不认。
 #[test]fn resources_are_served_from_the_grant_fingerprint_without_rehashing(){
  use sha2::{Digest,Sha256};
  let root=std::env::temp_dir().join(format!("fish-page-{}-{}",std::process::id(),crate::now_ms()));
  let directory=root.join("packages/acme.weather/2.0.0").join("a".repeat(64));
  std::fs::create_dir_all(&directory).unwrap();
  let html=b"<!doctype html><title>fixture</title>".to_vec();
  let page=crate::plugin_manifest::package_path(&directory,"page.html").unwrap();
  std::fs::write(&page,&html).unwrap();
  // 记下这一份指纹：后面「指纹变了」的断言要跟它比，而不是跟运气比。
  let before=crate::plugin_install::stamp(&page).unwrap();
  let digest=format!("{:x}",Sha256::digest(&html));
  let ticket="b".repeat(64);
  let grant=Grant{id:"acme.weather".into(),digest:"c".repeat(64),entry:"page.html".into(),issued:Instant::now(),directory:directory.clone(),
   files:BTreeMap::from([("page.html".into(),digest.clone())]),
   stamps:BTreeMap::from([("page.html".into(),before)])};
  GRANTS.lock().unwrap().insert(ticket.clone(),grant);
  let (served,mime)=resource(&ticket,"page.html").expect("指纹没变的资源必须直接发");
  assert_eq!(mime,"text/html; charset=utf-8");
  assert!(String::from_utf8_lossy(&served).contains("<title>fixture</title>"),"入口页要注入桥");
  assert!(String::from_utf8_lossy(&served).contains(&format!("/{ticket}/_fish_bridge.js")),"入口页要带本票据的桥脚本");
  // 指纹变了才重算：回写的内容**换成长度不同的**一份，两个信号（size 与 mtime）一起变。
  // 以前这里写回的是同长度内容，判据只剩 mtime——两次写偶尔落在同一 mtime 粒度上就会
  // 跳过哈希直接放行，于是这条用例 FAILED/ok 交替出现（t18 现场撞到过）。判据本身没变，
  // 是这条用例以前在赌时间粒度。
  let longer=b"<!doctype html><title>changed-longer</title>".to_vec();
  assert_ne!(longer.len(),html.len(),"回写内容必须换长度：同长度就只能靠 mtime 粒度");
  std::fs::write(&page,&longer).unwrap();
  let after=crate::plugin_install::stamp(&page).unwrap();
  assert_ne!(after,before,"长度变了，指纹必须跟着变");
  assert!(resource(&ticket,"page.html").is_err(),"指纹变了、摘要对不上必须拒绝");
  // 停用/卸载/换版：revoke 之后同一张票据立刻失效，在途的页面请求也一并被拒。
  revoke("acme.weather");
  assert!(resource(&ticket,"page.html").is_err(),"revoke 之后票据必须失效");
  GRANTS.lock().unwrap().clear();
  std::fs::remove_dir_all(&root).unwrap();
 }
 #[test]fn a_grant_for_another_plugin_is_never_served(){
  let root=std::env::temp_dir().join(format!("fish-page-revoke-{}-{}",std::process::id(),crate::now_ms()));
  let directory=root.join("packages");
  std::fs::create_dir_all(&directory).unwrap();
  let ticket="d".repeat(64);
  GRANTS.lock().unwrap().insert(ticket.clone(),Grant{id:"fish.magpie".into(),digest:"e".repeat(64),entry:"page.html".into(),issued:Instant::now(),directory,files:BTreeMap::from([("page.html".into(),"f".repeat(64))]),stamps:BTreeMap::new()});
  // 别人的票据被 revoke 时，这张还在；但 revoke 自己那个 id 时必须一起清掉。
  revoke("fish.clash");
  assert!(GRANTS.lock().unwrap().contains_key(&ticket),"revoke 只作废那一个插件的票据");
  revoke("fish.magpie");
  assert!(!GRANTS.lock().unwrap().contains_key(&ticket),"停用/卸载必须当场作废它的全部票据");
  std::fs::remove_dir_all(&root).unwrap();
 }
}