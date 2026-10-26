//! 插件页面一次性入口票据与逐资源校验，前端页只通过自己的桥访问业务方法。
use std::{collections::BTreeMap,sync::Mutex,time::{Duration,Instant}};
#[derive(Clone)]struct Grant { id:String, digest:String, entry:String, issued:Instant }
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
 if !crate::plugin_runtime::CURRENT.lock().map_err(|_|"插件运行锁不可用")?.is_running(id){return Err("插件尚未运行".into());}
 let entry=row.manifest.pages.iter().find(|p|p.id==page).ok_or("插件页面未注册")?.entry.clone();manager.verify(&row)?;
 let ticket=token()?;let mut grants=GRANTS.lock().map_err(|_|"插件页面锁不可用")?;
 grants.retain(|_,g|g.issued.elapsed()<Duration::from_secs(86400));if grants.len()>=32{return Err("插件页面过多，请关闭部分页面后重试".into());}
 grants.insert(ticket.clone(),Grant{id:id.into(),digest:row.digest,entry:entry.clone(),issued:Instant::now()});
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
pub fn call(ticket:&str,method:&str,params:serde_json::Value)->Result<serde_json::Value,String>{
 let (grant,_)=checked(ticket)?;let mut runner=crate::plugin_runtime::CURRENT.lock().map_err(|_|"插件运行锁不可用")?;
 if !runner.is_running(&grant.id){return Err("插件尚未运行".into());}
  if method=="gateway.open" {
   if !["fish.ofm","fish.magpie"].contains(&grant.id.as_str()){return Err("该插件没有本机服务页面".into());}
   let state=runner.call(&grant.id,"gateway.view",serde_json::json!({}))?;
   if state["state"]!="running"{return Err("请先启动服务".into());}
   let raw=state["url"].as_str().ok_or("服务尚未就绪")?;let url=tauri::Url::parse(raw).map_err(|_|"服务地址无效")?;
   if url.scheme()!="http"||url.host_str()!=Some("127.0.0.1")||!url.username().is_empty()||url.password().is_some()||url.port().map(u64::from)!=state["port"].as_u64(){return Err("服务就绪地址无效".into());}
   return Ok(serde_json::json!({"url":url.as_str()}));
  }
  runner.call(&grant.id,method,params)
}
fn resource(ticket:&str,name:&str)->Result<(Vec<u8>,&'static str),String>{
 use sha2::{Digest,Sha256};
 let (grant,row)=checked(ticket)?;
 if name=="_fish_bridge.js" {return Ok((include_bytes!("../../../sdk/browser/bridge.js").to_vec(),"text/javascript; charset=utf-8"));}
 let expected=row.manifest.files.get(name).ok_or("页面资源未在清单登记")?;
 let manager=crate::plugin_install::Manager{context:crate::storage_context::current().ok_or("存储尚未就绪")?};
 let path=crate::plugin_manifest::package_path(&manager.directory(&row)?,name)?;
 let bytes=crate::identity_store::read_optional(&path)?.ok_or("页面资源缺失")?;
 if format!("{:x}",Sha256::digest(&bytes))!=expected.to_ascii_lowercase(){return Err("页面资源摘要校验失败".into());}
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
 #[test]fn page_grants_bind_identity_version_enabled_state_and_lifetime(){
  let grant=Grant{id:"acme.weather".into(),digest:"1".repeat(64),entry:"page.html".into(),issued:Instant::now()};
  assert!(allowed(&grant,"acme.weather",&"1".repeat(64),true));
  assert!(!allowed(&grant,"fish.ofm",&"1".repeat(64),true));assert!(!allowed(&grant,"acme.weather",&"2".repeat(64),true));assert!(!allowed(&grant,"acme.weather",&"1".repeat(64),false));
  let expired=Grant{issued:Instant::now()-Duration::from_secs(86401),..grant};assert!(!allowed(&expired,"acme.weather",&"1".repeat(64),true));
 }
 #[test]fn malformed_ticket_routes_are_not_served(){for path in ["/page.html","/short/a","//etc/passwd","/../../../a"] {assert!(route(path).is_err());}let p=format!("/{}/ui/index.html","a".repeat(64));assert_eq!(route(&p).unwrap().1,"ui/index.html");}
}
