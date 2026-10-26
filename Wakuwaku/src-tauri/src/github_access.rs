//! GitHub 登录凭据仅驻留宿主；公开视图不含令牌或设备密钥。
use std::{sync::Mutex, time::Duration};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use crate::storage_context::Context;
const DAY:u64=86_400_000;
const VAULT:&str="github-access.dpapi";
const API:&str="https://api.github.com";
// 桌面 Device flow 使用公开 Client ID，不携带 Client Secret；显式配置仍可覆盖。
const DEFAULT_CLIENT_ID:&str="Ov23lisbBQRqCOm57AC6";
#[derive(Default,Serialize,Deserialize)]
#[serde(default,deny_unknown_fields)]
struct Account { token:String, login:String, user_id:u64, star_at:u64, expires_at:u64 }
struct Pending { root:std::path::PathBuf, client:String, code:String, user_code:String, expires_at:u64, next_at:u64, interval:u64 }
static DEVICE:Mutex<Option<Pending>>=Mutex::new(None);
static EDIT:Mutex<()>=Mutex::new(());
fn receipt_valid(star_at:u64, now:u64)->bool{star_at>0&&now>=star_at&&now-star_at<DAY}
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
 match context.host.read("keys",VAULT)?{
  None=>Ok(Account::default()),
  Some(bytes)=>serde_json::from_slice(&crate::mail_secrets::unprotect(&bytes)?).map_err(|_|"GitHub 宿主凭据格式无效".into()),
 }
}
fn save(context:&Context,account:&Account)->Result<(),String>{
 let bytes=serde_json::to_vec(account).map_err(|_|"GitHub 凭据编码失败")?;
 let protected=crate::mail_secrets::protect(&bytes)?;
 context.host.write("keys",VAULT,&protected)
}
fn public(context:&Context,account:&Account)->Result<Value,String>{
 let now=crate::now_ms();
 let live=!account.token.is_empty()&&(account.expires_at==0||account.expires_at>now);
 let device=DEVICE.lock().map_err(|_|"GitHub 设备登录锁不可用")?;
 let pending=device.as_ref().filter(|p|p.root==context.root&&p.expires_at>now).map(|p|json!({"userCode":p.user_code,"verificationUri":"https://github.com/login/device","expiresAt":p.expires_at,"interval":p.interval/1000}));
 Ok(json!({"configured":client(context)?.is_some(),"authenticated":live,"login":account.login,"verified":live&&account.user_id>0&&receipt_valid(account.star_at,now),"checkedAt":account.star_at,"expiresAt":account.expires_at,"pending":pending}))
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
 let token=data["access_token"].as_str().filter(|s|!s.is_empty()&&s.len()<=4096&&!s.chars().any(char::is_control)).ok_or("GitHub 未返回有效登录令牌")?.to_owned();
 let expires_at=data["expires_in"].as_u64().map(|seconds|now.saturating_add(seconds.saturating_mul(1000))).unwrap_or(0);
 *pending=None;
 Ok(Some(Account{token,expires_at,..Account::default()}))
}
fn verify(context:&Context,account:&mut Account)->Result<(),String>{
 if account.token.is_empty(){return Err("请先登录 GitHub".into());}
 let(status,bytes)=request(&format!("{API}/user"),Some(&account.token),None)?;
 if status==401{*account=Account::default();save(context,account)?;return Err("GitHub 登录已失效，请重新登录".into());}
 let identity=object(status,&bytes)?;
 let login=identity["login"].as_str().filter(|s|!s.is_empty()&&s.len()<=100&&s.bytes().all(|b|b.is_ascii_alphanumeric()||b==b'-')).ok_or("GitHub 用户身份无效")?;
 let id=identity["id"].as_u64().filter(|id|*id>0).ok_or("GitHub 用户 ID 无效")?;
 if account.user_id!=id{account.star_at=0;}
 account.login=login.into();account.user_id=id;
 let(status,_)=request(&format!("{API}/user/starred/Fish-under-sea/FISH-Dock"),Some(&account.token),None)?;
 if status==401{*account=Account::default();save(context,account)?;return Err("GitHub 登录已失效，请重新登录".into());}
 let starred=star_status(status)?;account.star_at=if starred{crate::now_ms()}else{0};save(context,account)?;
 if !starred{return Err("尚未检测到主仓 Star，请在 GitHub 点亮后重新验证".into());}Ok(())
}
pub fn check(context:&Context)->Result<Value,String>{
 let _guard=EDIT.lock().map_err(|_|"GitHub 登录锁不可用")?;
 let mut account=poll(context)?.unwrap_or(load(context)?);
 // 新令牌先加密落盘，后续网络失败仍可复核，不丢失已完成的设备授权。
 save(context,&account)?;verify(context,&mut account)?;public(context,&account)
}
pub fn require(context:&Context)->Result<(),String>{
 let _guard=EDIT.lock().map_err(|_|"GitHub 登录锁不可用")?;
 let mut account=load(context)?;let now=crate::now_ms();
 if !account.token.is_empty()&&account.user_id>0&&receipt_valid(account.star_at,now)&&(account.expires_at==0||account.expires_at>now){return Ok(());}
 verify(context,&mut account)
}
pub fn require_plugin(context:&Context,id:&str)->Result<(),String>{
 if matches!(id,"fish.feishu"|"fish.clash"|"fish.magpie"|"fish.ofm"){require(context)}else{Ok(())}
}
pub fn logout(context:&Context)->Result<Value,String>{
 let _guard=EDIT.lock().map_err(|_|"GitHub 登录锁不可用")?;
 *DEVICE.lock().map_err(|_|"GitHub 设备登录锁不可用")?=None;let account=Account::default();save(context,&account)?;public(context,&account)
}
#[cfg(test)]mod tests{
 use super::*;
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
 #[test]fn offline_receipt_expires_and_future_timestamps_are_not_trusted(){
  assert!(receipt_valid(100,101));assert!(!receipt_valid(0,101));assert!(!receipt_valid(102,101));assert!(!receipt_valid(100,100+DAY));
 }
}
