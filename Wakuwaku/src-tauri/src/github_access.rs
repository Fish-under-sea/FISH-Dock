//! GitHub 登录凭据仅驻留宿主；公开视图不含令牌或设备密钥。
use std::{sync::Mutex, time::Duration};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use crate::storage_context::Context;
const DAY:u64=86_400_000;
// 主仓 Star 的复核回执：一次联网核验管一周，离线时保留上次结论，不当作未登录。
const STAR_WINDOW:u64=7*DAY;
// 凭据记录自身的版本：只增字段，不删字段、不改类型，旧记录照旧读。
const SCHEMA:u32=2;
const VAULT:&str="github-access.dpapi";
const VAULT_BACKUP:&str="github-access.dpapi.bak";
const API:&str="https://api.github.com";
// 桌面 Device flow 使用公开 Client ID，不携带 Client Secret；显式配置仍可覆盖。
const DEFAULT_CLIENT_ID:&str="Ov23lisbBQRqCOm57AC6";
#[derive(Default,Clone,Serialize,Deserialize)]
#[serde(default)]
struct Account {
 schema_version:u32,
 token:String, refresh_token:String, client_id:String,
 login:String, user_id:u64, star_at:u64, star_checked_at:u64,
 expires_at:u64, refresh_expires_at:u64,
}
const REFRESH_AHEAD:u64=300_000;
impl Account {
 fn access_live(&self,now:u64)->bool{!self.token.is_empty()&&(self.expires_at==0||self.expires_at>now)}
 fn refresh_live(&self,now:u64)->bool{!self.refresh_token.is_empty()&&self.refresh_expires_at>now}
 fn session_live(&self,now:u64)->bool{self.access_live(now)||self.refresh_live(now)}
 fn needs_refresh(&self,now:u64)->bool{self.refresh_live(now)&&self.expires_at>0&&self.expires_at<=now.saturating_add(REFRESH_AHEAD)}
 fn last_star_check(&self)->u64{self.star_checked_at.max(self.star_at)}
}
struct Pending { root:std::path::PathBuf, client:String, code:String, user_code:String, expires_at:u64, next_at:u64, interval:u64 }
static DEVICE:Mutex<Option<Pending>>=Mutex::new(None);
static EDIT:Mutex<()>=Mutex::new(());
fn receipt_valid(star_at:u64, now:u64)->bool{star_at>0&&now>=star_at&&now-star_at<STAR_WINDOW}
fn star_status(status:u16)->Result<bool,String>{match status{204=>Ok(true),404=>Ok(false),_=>Err(format!("GitHub Star 校验未完成（HTTP {status}）"))}}
fn client(context:&Context)->Result<Option<String>,String>{
 let configured=context.host.read("config","github-oauth.json")?.map(|bytes|serde_json::from_slice::<Value>(&bytes).map_err(|_|"GitHub 登录设置格式无效")).transpose()?;
 let value=configured.and_then(|v|v.get("clientId").and_then(Value::as_str).map(str::to_owned))
  .or_else(||std::env::var("FISH_DOCK_GITHUB_CLIENT_ID").ok())
  .or_else(||option_env!("FISH_DOCK_GITHUB_CLIENT_ID").map(str::to_owned))
  .or_else(||Some(DEFAULT_CLIENT_ID.to_owned()));
 match value {Some(s) if !s.is_empty()&&s.len()<=100&&s.bytes().all(|b|b.is_ascii_alphanumeric())=>Ok(Some(s)),Some(_)=>Err("GitHub OAuth clientId 无效".into()),None=>Ok(None)}
}
fn load(context:&Context)->Result<Account,String>{
 let Some(bytes)=context.host.read("keys",VAULT)? else{return Ok(Account::default())};
 let plain=crate::mail_secrets::unprotect(&bytes)?;
 match serde_json::from_slice::<Account>(&plain){
  Ok(account)=>Ok(account),
  // 新版本留下的字段、或某个字段写成了别的类型：先把原文件留一份，再按空会话处理。
  // 读不懂凭据不构成删除凭据的理由，下一次成功登录会覆盖它。
  Err(_)=>{let _=context.host.write("keys",VAULT_BACKUP,&bytes);Ok(Account::default())}
 }
}
fn save(context:&Context,account:&Account)->Result<(),String>{
 let mut account=account.clone();
 account.schema_version=SCHEMA;
 let bytes=serde_json::to_vec(&account).map_err(|_|"GitHub 凭据编码失败")?;
 let protected=crate::mail_secrets::protect(&bytes)?;
 context.host.write("keys",VAULT,&protected)
}
fn public(context:&Context,account:&Account)->Result<Value,String>{
 let now=crate::now_ms();
 let live=account.session_live(now);
 let last_star_check=account.last_star_check();
 let needs_star_check=live&&(last_star_check==0||!receipt_valid(last_star_check,now));
 let requires_login=!live&&(!account.token.is_empty()||!account.login.is_empty());
 let device=DEVICE.lock().map_err(|_|"GitHub 设备登录锁不可用")?;
 let pending=device.as_ref().filter(|p|p.root==context.root&&p.expires_at>now).map(|p|json!({"userCode":p.user_code,"verificationUri":"https://github.com/login/device","expiresAt":p.expires_at,"interval":p.interval/1000}));
 Ok(json!({"configured":client(context)?.is_some(),"authenticated":live,"login":account.login,"verified":live&&account.user_id>0&&receipt_valid(account.star_at,now),"checkedAt":account.star_at,"lastCheckedAt":last_star_check,"expiresAt":account.expires_at,"needsRefresh":account.needs_refresh(now),"refreshable":account.refresh_live(now),"needsStarCheck":needs_star_check,"requiresLogin":requires_login,"pending":pending}))
}
pub fn view(context:&Context)->Result<Value,String>{let _guard=EDIT.lock().map_err(|_|"GitHub 登录锁不可用")?;public(context,&load(context)?)}
// 固定域名、禁止重定向且限制响应大小，避免认证头随跳转外送。
fn request(url:&str,token:Option<&str>,form:Option<&[(&str,&str)]>)->Result<(u16,Vec<u8>),String>{
 for way in crate::proxy::web_ways(url){
  let Some(agent)=way.agent_with(Duration::from_secs(20),|c|c.max_redirects(0).http_status_as_error(false))else{continue};
  let result=if let Some(form)=form{
   agent.post(url).header("User-Agent","FISH-Dock").header("Accept","application/json").send_form(form.iter().copied())
  }else{
   let mut req=agent.get(url).header("User-Agent","FISH-Dock").header("Accept","application/vnd.github+json");
   if let Some(token)=token{req=req.header("Authorization",format!("Bearer {token}"));}
   req.call()
  };
  if let Ok(mut response)=result {
   let status=response.status().as_u16();
   let body=response.body_mut().with_config().limit(64*1024).read_to_vec().map_err(|_|"GitHub 响应超过限制或读取中断")?;
   return Ok((status,body));
  }
 }
 Err("GitHub 连接失败，请检查 Dock 网络代理后重试".into())
}
fn object(status:u16,bytes:&[u8])->Result<Value,String>{
 if status!=200{return Err(format!("GitHub 登录请求未完成（HTTP {status}）"));}
 serde_json::from_slice(bytes).map_err(|_|"GitHub 登录响应格式无效".into())
}
pub fn begin(context:&Context)->Result<Value,String>{
 let _guard=EDIT.lock().map_err(|_|"GitHub 登录锁不可用")?;
 let client=client(context)?.ok_or("此构建尚未配置 FISH Dock 自有 GitHub OAuth 应用 clientId；需维护者配置并启用 Device flow")?;
 let(status,bytes)=request("https://github.com/login/device/code",None,Some(&[("client_id",&client),("scope","read:user")] ))?;
 let data=object(status,&bytes)?;
 if data.get("error").is_some(){return Err("GitHub 尚未允许设备登录，请检查 OAuth 应用的 Device flow 设置".into());}
 let code=data["device_code"].as_str().filter(|s|!s.is_empty()&&s.len()<=200).ok_or("GitHub 设备码缺失")?.to_owned();
 let user_code=data["user_code"].as_str().filter(|s|!s.is_empty()&&s.len()<=32&&s.bytes().all(|b|b.is_ascii_alphanumeric()||b==b'-')).ok_or("GitHub 验证码无效")?.to_owned();
 if data["verification_uri"].as_str()!=Some("https://github.com/login/device"){return Err("GitHub 验证地址不匹配".into());}
 let now=crate::now_ms();let interval=data["interval"].as_u64().unwrap_or(5).clamp(5,60)*1000;
 *DEVICE.lock().map_err(|_|"GitHub 设备登录锁不可用")?=Some(Pending{root:context.root.clone(),client,code,user_code,expires_at:now+data["expires_in"].as_u64().unwrap_or(900).clamp(1,900)*1000,next_at:now+interval,interval});
 public(context,&load(context)?)
}
fn poll(context:&Context)->Result<Option<Account>,String>{
 let mut pending=DEVICE.lock().map_err(|_|"GitHub 设备登录锁不可用")?;
 let Some(p)=pending.as_mut().filter(|p|p.root==context.root)else{return Ok(None)};
 let now=crate::now_ms();
 if now>=p.expires_at{*pending=None;return Err("GitHub 设备码已过期，请重新登录".into());}
 if now<p.next_at{return Err("请按 GitHub 规定的间隔稍后再检查登录".into());}
 p.next_at=now+p.interval;
 let(status,bytes)=request("https://github.com/login/oauth/access_token",None,Some(&[("client_id",&p.client),("device_code",&p.code),("grant_type","urn:ietf:params:oauth:grant-type:device_code")]))?;
 let data=object(status,&bytes)?;
 match data["error"].as_str(){
  Some("authorization_pending")=>return Err("正在等待 GitHub 授权，请在浏览器确认后重试".into()),
  Some("slow_down")=>{p.interval=(p.interval+5000).min(120_000);p.next_at=now+p.interval;return Err("GitHub 要求降低检查频率，请稍后重试".into());},
  Some(_)=>{*pending=None;return Err("GitHub 设备授权已结束，请重新登录".into());},None=>{},
 }
 let account=token_response(&data,now,&p.client)?;
 *pending=None;
 Ok(Some(account))
}
fn token_response(data:&Value,now:u64,client:&str)->Result<Account,String>{
 fn credential(data:&Value,key:&str)->Result<String,String>{data[key].as_str().filter(|s|!s.is_empty()&&s.len()<=4096&&s.bytes().all(|b|b.is_ascii_graphic())).map(str::to_owned).ok_or_else(||format!("GitHub {key} 字段无效"))}
 let token=credential(data,"access_token")?;
 let expires_at=match data.get("expires_in"){Some(v)=>{let seconds=v.as_u64().filter(|s|*s>0).ok_or("GitHub 令牌有效期无效")?;now.saturating_add(seconds.saturating_mul(1000))},None=>0};
 let (refresh_token,refresh_expires_at)=if data.get("refresh_token").is_some(){let value=credential(data,"refresh_token")?;let seconds=data["refresh_token_expires_in"].as_u64().filter(|s|*s>0).ok_or("GitHub 刷新令牌有效期缺失")?;(value,now.saturating_add(seconds.saturating_mul(1000)))}else{(String::new(),0)};
 Ok(Account{schema_version:SCHEMA,token,expires_at,refresh_token,refresh_expires_at,client_id:client.into(),..Account::default()})
}
type Transport<'a> = dyn FnMut(&str,Option<&str>,Option<&[(&str,&str)]>)->Result<(u16,Vec<u8>),String> + 'a;
fn invalidate(context:&Context,account:&mut Account)->Result<(),String>{
 // 只在服务端明确确认凭据无效后清理令牌，保留账号名以提示重新登录。
 account.token.clear();account.refresh_token.clear();account.expires_at=0;account.refresh_expires_at=0;account.star_at=0;account.star_checked_at=0;
 save(context,account)
}
fn refresh_with(context:&Context,account:&mut Account,now:u64,http:&mut Transport<'_>)->Result<(),String>{
 if !account.refresh_live(now){return Err("GitHub 登录需要重新授权；此账号没有可用的续期凭据".into());}
 let configured=client(context)?.ok_or("GitHub 登录应用未配置")?;
 let owner=if account.client_id.is_empty(){configured.as_str()}else{account.client_id.as_str()};
 if owner!=configured{return Err("GitHub 登录应用已变更，请重新登录".into());}
 let(status,bytes)=http("https://github.com/login/oauth/access_token",None,Some(&[("client_id",owner),("grant_type","refresh_token"),("refresh_token",&account.refresh_token)]))?;
 let value:Value=serde_json::from_slice(&bytes).map_err(|_|"GitHub 续期响应格式无效，原登录资料已保留")?;
 if status<500 && matches!(value["error"].as_str(),Some("bad_refresh_token"|"expired_token"|"invalid_grant"|"invalid_refresh_token")){
  invalidate(context,account)?;return Err("GitHub 续期凭据已失效，请重新登录".into());
 }
 if status!=200 || value.get("error").is_some(){return Err(format!("GitHub 暂未完成登录续期（HTTP {status}），原登录资料已保留"));}
 let rotated=token_response(&value,now,&configured)?;
 if rotated.refresh_token.is_empty(){return Err("GitHub 续期响应缺少新刷新令牌，原登录资料已保留".into());}
 account.token=rotated.token;account.expires_at=rotated.expires_at;
 account.refresh_token=rotated.refresh_token;account.refresh_expires_at=rotated.refresh_expires_at;account.client_id=rotated.client_id;
 // GitHub 会同时轮换并作废旧令牌。新令牌必须先持久化，再请求个人身份和 Star。
 save(context,account)
}
fn ensure_access(context:&Context,account:&mut Account,now:u64,http:&mut Transport<'_>)->Result<bool,String>{
 if account.needs_refresh(now) || !account.access_live(now){
  if account.refresh_live(now){refresh_with(context,account,now,http)?;return Ok(true);}
  if account.token.is_empty(){return Err("请先登录 GitHub".into());}
  return Err("GitHub 登录已到期，需重新登录一次以保存续期凭据".into());
 }
 Ok(false)
}
fn authorized_get(context:&Context,account:&mut Account,now:u64,url:&str,refreshed:&mut bool,http:&mut Transport<'_>)->Result<(u16,Vec<u8>),String>{
 let mut answer=http(url,Some(&account.token),None)?;
 if answer.0!=401{return Ok(answer);}
 if !*refreshed && account.refresh_live(now){
  refresh_with(context,account,now,http)?;*refreshed=true;
  answer=http(url,Some(&account.token),None)?;
  if answer.0!=401{return Ok(answer);}
 }
 invalidate(context,account)?;
 Err("GitHub 登录已失效，请重新登录".into())
}
fn verify_with(context:&Context,account:&mut Account,now:u64,http:&mut Transport<'_>)->Result<(),String>{
 let mut refreshed=ensure_access(context,account,now,http)?;
 let(status,bytes)=authorized_get(context,account,now,&format!("{API}/user"),&mut refreshed,http)?;
 let identity=object(status,&bytes)?;
 let login=identity["login"].as_str().filter(|s|!s.is_empty()&&s.len()<=100&&s.bytes().all(|b|b.is_ascii_alphanumeric()||b==b'-')).ok_or("GitHub 用户身份无效")?;
 let id=identity["id"].as_u64().filter(|id|*id>0).ok_or("GitHub 用户 ID 无效")?;
 if account.user_id!=id{account.star_at=0;account.star_checked_at=0;}
 account.login=login.into();account.user_id=id;
 // Star 接口断连时仍保留已确认的个人身份，不把它误报为未登录。
 save(context,account)?;
 let(status,_)=authorized_get(context,account,now,&format!("{API}/user/starred/Fish-under-sea/FISH-Dock"),&mut refreshed,http)?;
 let starred=star_status(status)?;account.star_at=if starred{now}else{0};account.star_checked_at=now;save(context,account)?;
 if !starred{return Err("已登录 GitHub，但尚未检测到主仓 Star，请点亮后重新验证".into());}Ok(())
}
fn verify(context:&Context,account:&mut Account)->Result<(),String>{verify_with(context,account,crate::now_ms(),&mut request)}
// 每次启动（含更新到新版本后）先自己把会话续上、把身份与主仓 Star 复核一次：
// 用户不必再点任何按钮。任何一步失败都只让状态停在「待复核」，凭据原样保留。
pub(crate) fn resume_with(context:&Context,now:u64,http:&mut Transport<'_>)->Result<bool,String>{
 let mut account=load(context)?;
 if !account.session_live(now){return Ok(false);}
 let mut acted=false;
 if account.needs_refresh(now)||!account.access_live(now){
  if account.refresh_live(now){let _=refresh_with(context,&mut account,now,http);acted=true;}
 }
 if account.session_live(now){
  let last=account.last_star_check();
  if account.user_id==0||last==0||!receipt_valid(last,now){let _=verify_with(context,&mut account,now,http);acted=true;}
 }
 Ok(acted)
}
pub fn resume(context:&Context)->Result<(),String>{
 let _guard=EDIT.lock().map_err(|_|"GitHub 登录锁不可用")?;
 resume_with(context,crate::now_ms(),&mut request).map(|_|())
}
pub fn check(context:&Context)->Result<Value,String>{
 let _guard=EDIT.lock().map_err(|_|"GitHub 登录锁不可用")?;
 let mut account=match poll(context)?{Some(account)=>{save(context,&account)?;account},None=>load(context)?};
 verify(context,&mut account)?;public(context,&account)
}
pub fn require(context:&Context)->Result<(),String>{
 let _guard=EDIT.lock().map_err(|_|"GitHub 登录锁不可用")?;
 let mut account=load(context)?;let now=crate::now_ms();
 if account.user_id>0&&receipt_valid(account.star_at,now){ensure_access(context,&mut account,now,&mut request)?;return Ok(());}
 verify(context,&mut account)
}
pub fn require_plugin(context:&Context,id:&str)->Result<(),String>{
 if matches!(id,"fish.feishu"|"fish.clash"|"fish.magpie"|"fish.ofm"){require(context)}else{Ok(())}
}
pub fn cancel(context:&Context)->Result<Value,String>{
 let _guard=EDIT.lock().map_err(|_|"GitHub 登录锁不可用")?;
 let mut device=DEVICE.lock().map_err(|_|"GitHub 设备登录锁不可用")?;
 if device.as_ref().is_some_and(|p|p.root==context.root){*device=None;}drop(device);public(context,&load(context)?)
}
pub fn logout(context:&Context)->Result<Value,String>{
 let _guard=EDIT.lock().map_err(|_|"GitHub 登录锁不可用")?;
 *DEVICE.lock().map_err(|_|"GitHub 设备登录锁不可用")?=None;let account=Account::default();save(context,&account)?;public(context,&account)
}
#[cfg(test)]
#[path="github_session_tests.rs"]
mod session_tests;
#[cfg(test)]mod tests{
 use super::*;
 #[test]fn device_token_response_keeps_rotating_credentials_across_restart(){
  let now=crate::now_ms();let token=token_response(&json!({"access_token":"synthetic-access","expires_in":28800,"refresh_token":"synthetic-refresh","refresh_token_expires_in":15897600}),now,DEFAULT_CLIENT_ID).unwrap();
  let encoded=serde_json::to_value(&token).unwrap();
  assert_eq!(encoded["refresh_token"],"synthetic-refresh","设备登录响应中的刷新令牌必须保存");
  assert_eq!(encoded["refresh_expires_at"],now+15897600000u64);
  let root=std::env::temp_dir().join(format!("fish-session-restart-{}-{}",std::process::id(),crate::now_ms()));
  let context=Context::open(root,env!("CARGO_PKG_VERSION")).unwrap();save(&context,&token).unwrap();
  assert_eq!(serde_json::to_value(load(&context).unwrap()).unwrap()["refresh_token"],"synthetic-refresh");
  assert!(!String::from_utf8_lossy(&context.host.read("keys",VAULT).unwrap().unwrap()).contains("synthetic-refresh"));
 }
 #[test]fn expired_access_with_refresh_is_a_recoverable_session_not_a_logout(){
  let now=crate::now_ms();let mut account=token_response(&json!({"access_token":"synthetic-access","expires_in":28800,"refresh_token":"synthetic-refresh","refresh_token_expires_in":15897600}),now,DEFAULT_CLIENT_ID).unwrap();
  account.expires_at=now-1;account.login="fixture-user".into();account.user_id=42;account.star_at=now;
  let root=std::env::temp_dir().join(format!("fish-session-expired-{}-{}",std::process::id(),crate::now_ms()));let context=Context::open(root,env!("CARGO_PKG_VERSION")).unwrap();
  let state=public(&context,&account).unwrap();assert_eq!(state["authenticated"],true);assert_eq!(state["needsRefresh"],true);
  for key in ["token","refresh_token","refreshToken","clientSecret"]{assert!(state.get(key).is_none());}
 }
 #[test]fn official_build_has_a_public_client_without_personal_setup(){
  let root=std::env::temp_dir().join(format!("fish-github-client-{}-{}",std::process::id(),crate::now_ms()));
  let context=Context::open(root,"1.1.0").unwrap();
  assert_eq!(client(&context).unwrap().as_deref(),Some(DEFAULT_CLIENT_ID));
  let state=view(&context).unwrap();assert_eq!(state["configured"],true);assert_eq!(state["authenticated"],false);
  assert!(state.get("token").is_none());assert!(state.get("clientSecret").is_none());
 }
 #[test]fn star_requires_authenticated_204_and_never_network_success_alone(){
  assert!(star_status(204).unwrap());assert!(!star_status(404).unwrap());
  for status in [200,401,403,429,500]{assert!(star_status(status).is_err(),"HTTP {status}");}
 }
 #[test]fn official_access_requires_host_receipt_but_third_party_stays_independent(){
  let root=std::env::temp_dir().join(format!("fish-star-policy-{}-{}",std::process::id(),crate::now_ms()));
  let context=Context::open(root,"1.1.0").unwrap();
  for id in ["fish.feishu","fish.clash","fish.magpie","fish.ofm"]{assert!(require_plugin(&context,id).unwrap_err().contains("登录 GitHub"));}
  assert!(require_plugin(&context,"acme.weather").is_ok());
 }
 #[test]fn offline_receipt_holds_a_week_and_future_timestamps_are_not_trusted(){
  assert!(receipt_valid(100,101));assert!(!receipt_valid(0,101));assert!(!receipt_valid(102,101));
  assert!(receipt_valid(100,100+6*DAY),"一周内沿用上次复核结论");assert!(!receipt_valid(100,100+STAR_WINDOW));
 }
}
