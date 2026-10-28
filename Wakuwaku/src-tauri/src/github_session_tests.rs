//! 使用合成令牌和可控响应验证会话续期；不访问真实 GitHub 账号。
use super::*;
use std::{fs, path::PathBuf, sync::atomic::{AtomicUsize,Ordering}};
struct Fixture { context:Context, root:PathBuf }
impl Fixture {
 fn new()->Self{static NEXT:AtomicUsize=AtomicUsize::new(0);let root=std::env::temp_dir().join(format!("fish-session-contract-{}-{}-{}",std::process::id(),crate::now_ms(),NEXT.fetch_add(1,Ordering::Relaxed)));let context=Context::open(root.clone(),env!("CARGO_PKG_VERSION")).unwrap();Self{context,root}}
}
impl Drop for Fixture{fn drop(&mut self){let _=fs::remove_dir_all(&self.root);}}
fn account(now:u64)->Account{Account{token:"synthetic-old-access".into(),refresh_token:"synthetic-old-refresh".into(),client_id:DEFAULT_CLIENT_ID.into(),login:"fixture-user".into(),user_id:42,star_at:now,star_checked_at:now,expires_at:now+3_600_000,refresh_expires_at:now+DAY,..Account::default()}}
fn answer(status:u16,value:Value)->Result<(u16,Vec<u8>),String>{Ok((status,serde_json::to_vec(&value).unwrap()))}
fn rotated()->Value{json!({"access_token":"synthetic-new-access","expires_in":28800,"refresh_token":"synthetic-new-refresh","refresh_token_expires_in":15897600})}
fn identity()->Value{json!({"login":"fixture-user","id":42})}
#[test]
fn refresh_rotation_is_saved_before_a_later_profile_network_failure_and_survives_restart(){
 let f=Fixture::new();let now=crate::now_ms();let mut a=account(now);a.expires_at=now-1;save(&f.context,&a).unwrap();let mut refreshes=0;
 let mut http=|url:&str,_token:Option<&str>,form:Option<&[(&str,&str)]>|{
  if url.ends_with("access_token"){
   refreshes+=1;let form=form.unwrap();assert!(form.contains(&("grant_type","refresh_token")));assert!(form.contains(&("client_id",DEFAULT_CLIENT_ID)));assert!(!form.iter().any(|(k,_)|*k=="client_secret"));answer(200,rotated())
  }else{Err("synthetic network failure".into())}
 };
 assert!(verify_with(&f.context,&mut a,now,&mut http).is_err());assert_eq!(refreshes,1);
 let mut restored=load(&f.context).unwrap();assert_eq!(restored.token,"synthetic-new-access");assert_eq!(restored.refresh_token,"synthetic-new-refresh");assert_eq!(restored.login,"fixture-user");
 let mut http=|url:&str,token:Option<&str>,_form:Option<&[(&str,&str)]>|{
  assert_eq!(token,Some("synthetic-new-access"));
  if url.ends_with("/user"){answer(200,identity())}else{answer(204,json!({}))}
 };
 verify_with(&f.context,&mut restored,now,&mut http).unwrap();assert_eq!(public(&f.context,&restored).unwrap()["verified"],true);
}
#[test]
fn transient_refresh_failure_preserves_the_entire_encrypted_session(){
 let f=Fixture::new();let now=crate::now_ms();let mut a=account(now);a.expires_at=now-1;save(&f.context,&a).unwrap();let before=f.context.host.read("keys",VAULT).unwrap().unwrap();
 assert!(verify_with(&f.context,&mut a,now,&mut |_,_,_|Err("synthetic timeout".into())).is_err());
 assert_eq!(f.context.host.read("keys",VAULT).unwrap().unwrap(),before);assert_eq!(a.refresh_token,"synthetic-old-refresh");
 let state=public(&f.context,&a).unwrap();assert_eq!(state["authenticated"],true);assert_eq!(state["requiresLogin"],false);
}
#[test]
fn unauthorized_access_retries_once_using_the_rotated_token(){
 let f=Fixture::new();let now=crate::now_ms();let mut a=account(now);save(&f.context,&a).unwrap();let mut refreshes=0;
 let mut http=|url:&str,token:Option<&str>,_form:Option<&[(&str,&str)]>|{
  if url.ends_with("access_token"){refreshes+=1;return answer(200,rotated());}
  if token==Some("synthetic-old-access"){return answer(401,json!({}));}
  assert_eq!(token,Some("synthetic-new-access"));if url.ends_with("/user"){answer(200,identity())}else{answer(204,json!({}))}
 };
 verify_with(&f.context,&mut a,now,&mut http).unwrap();assert_eq!(refreshes,1);assert_eq!(load(&f.context).unwrap().refresh_token,"synthetic-new-refresh");
}
#[test]
fn explicit_invalid_refresh_requires_reauthentication_without_looping(){
 let f=Fixture::new();let now=crate::now_ms();let mut a=account(now);a.expires_at=now-1;save(&f.context,&a).unwrap();let mut calls=0;
 let mut http=|_:&str,_:Option<&str>,_:Option<&[(&str,&str)]>|{calls+=1;answer(200,json!({"error":"bad_refresh_token"}))};
 assert!(verify_with(&f.context,&mut a,now,&mut http).is_err());assert_eq!(calls,1);let a=load(&f.context).unwrap();assert!(a.token.is_empty());assert!(a.refresh_token.is_empty());assert_eq!(a.login,"fixture-user");assert_eq!(public(&f.context,&a).unwrap()["requiresLogin"],true);
}
#[test]
fn star_not_found_is_not_a_logout_and_does_not_immediately_recheck(){
 let f=Fixture::new();let now=crate::now_ms();let mut a=account(now);let mut http=|url:&str,_:Option<&str>,_:Option<&[(&str,&str)]>|if url.ends_with("/user"){answer(200,identity())}else{answer(404,json!({}))};
 assert!(verify_with(&f.context,&mut a,now,&mut http).is_err());let state=public(&f.context,&load(&f.context).unwrap()).unwrap();assert_eq!(state["authenticated"],true);assert_eq!(state["verified"],false);assert_eq!(state["needsStarCheck"],false);
}
#[test]
fn star_rate_limit_preserves_login_and_existing_verification(){
 let f=Fixture::new();let now=crate::now_ms();let mut a=account(now);let previous=a.star_at;
 let mut http=|url:&str,_:Option<&str>,_:Option<&[(&str,&str)]>|if url.ends_with("/user"){answer(200,identity())}else{answer(429,json!({}))};
 assert!(verify_with(&f.context,&mut a,now,&mut http).is_err());assert_eq!(a.star_at,previous);let state=public(&f.context,&load(&f.context).unwrap()).unwrap();assert_eq!(state["authenticated"],true);assert_eq!(state["verified"],true);
}
#[test]
fn old_records_are_readable_and_old_expired_tokens_request_one_new_login(){
 let f=Fixture::new();let now=crate::now_ms();let mut a:Account=serde_json::from_value(json!({"token":"synthetic-legacy","login":"fixture-user","user_id":42,"star_at":now,"expires_at":now-1})).unwrap();
 assert!(a.refresh_token.is_empty());let mut calls=0;let mut http=|_:&str,_:Option<&str>,_:Option<&[(&str,&str)]>|{calls+=1;Err("should not request".into())};
 assert!(verify_with(&f.context,&mut a,now,&mut http).unwrap_err().contains("重新登录一次"));assert_eq!(calls,0);let state=public(&f.context,&a).unwrap();assert_eq!(state["authenticated"],false);assert_eq!(state["requiresLogin"],true);assert_eq!(state["login"],"fixture-user");
}
#[test]
fn repeated_unauthorized_after_rotation_does_not_retry_indefinitely(){
 let f=Fixture::new();let now=crate::now_ms();let mut a=account(now);a.expires_at=now-1;let mut refreshes=0;
 let mut http=|url:&str,_:Option<&str>,_:Option<&[(&str,&str)]>|if url.ends_with("access_token"){refreshes+=1;answer(200,rotated())}else{answer(401,json!({}))};
 assert!(verify_with(&f.context,&mut a,now,&mut http).is_err());assert_eq!(refreshes,1);assert!(load(&f.context).unwrap().token.is_empty());
}
#[test]
fn incomplete_refresh_response_keeps_existing_credentials(){
 let f=Fixture::new();let now=crate::now_ms();let mut a=account(now);a.expires_at=now-1;save(&f.context,&a).unwrap();
 assert!(verify_with(&f.context,&mut a,now,&mut |_,_,_|answer(200,json!({"access_token":"synthetic-incomplete","expires_in":28800}))).is_err());
 let stored=load(&f.context).unwrap();assert_eq!(stored.token,"synthetic-old-access");assert_eq!(stored.refresh_token,"synthetic-old-refresh");
}
// 应用侧关掉令牌过期之后，GitHub 换发的是不带 expires_in、也不带 refresh_token 的
// 长期令牌：这时的续期响应必须被接受，并把不再需要的刷新凭据清掉，而不是报错。
#[test]
fn a_refresh_after_the_app_stopped_expiring_tokens_keeps_the_new_permanent_access(){
 let f=Fixture::new();let now=crate::now_ms();let mut a=account(now);a.expires_at=now-1;save(&f.context,&a).unwrap();
 let mut http=|url:&str,_:Option<&str>,_:Option<&[(&str,&str)]>|{
  if url.ends_with("access_token"){return answer(200,json!({"access_token":"synthetic-permanent","token_type":"bearer","scope":"read:user"}));}
  if url.ends_with("/user"){answer(200,identity())}else{answer(204,json!({}))}
 };
 verify_with(&f.context,&mut a,now,&mut http).unwrap();
 let stored=load(&f.context).unwrap();
 assert_eq!(stored.token,"synthetic-permanent");
 assert_eq!(stored.expires_at,0,"不再过期的令牌按永久处理");
 assert!(stored.access_live(now+365*DAY));
 assert!(stored.refresh_token.is_empty(),"不再需要刷新凭据");
 assert_eq!(public(&f.context,&stored).unwrap()["requiresLogin"],false);
}
// 反过来：响应仍带 8 小时有效期却没有新刷新令牌，是异常，保留原凭据不动。
#[test]
fn a_short_lived_refresh_without_a_new_refresh_token_keeps_the_old_session(){
 let f=Fixture::new();let now=crate::now_ms();let mut a=account(now);a.expires_at=now-1;save(&f.context,&a).unwrap();
 assert!(verify_with(&f.context,&mut a,now,&mut |_,_,_|answer(200,json!({"access_token":"synthetic-short","expires_in":28800}))).is_err());
 let stored=load(&f.context).unwrap();
 assert_eq!(stored.token,"synthetic-old-access");assert_eq!(stored.refresh_token,"synthetic-old-refresh");
}
// 新版写入的记录多出字段时必须照旧读成同一份会话：一次登录在版本之间不变。
#[test]
fn a_newer_record_with_extra_fields_keeps_the_same_session(){
 let f=Fixture::new();let now=crate::now_ms();
 let stored=json!({"schema_version":2,"token":"synthetic-access","refresh_token":"synthetic-refresh","client_id":DEFAULT_CLIENT_ID,"login":"fixture-user","user_id":42,"star_at":now,"star_checked_at":now,"expires_at":now+3_600_000,"refresh_expires_at":now+DAY,"scope":"read:user","token_type":"bearer"});
 f.context.host.write("keys",VAULT,&crate::mail_secrets::protect(&serde_json::to_vec(&stored).unwrap()).unwrap()).unwrap();
 let a=load(&f.context).unwrap();
 assert_eq!(a.login,"fixture-user");assert_eq!(a.token,"synthetic-access");assert_eq!(a.refresh_token,"synthetic-refresh");
 let state=public(&f.context,&a).unwrap();assert_eq!(state["authenticated"],true);assert_eq!(state["requiresLogin"],false);
}
// 令牌不过期的账号（OAuth 应用关闭过期后签发）永远不需要再登录一次。
#[test]
fn a_session_without_an_expiry_never_asks_for_a_new_login(){
 let f=Fixture::new();let now=crate::now_ms();
 let a=token_response(&json!({"access_token":"synthetic-permanent","scope":"read:user"}),now,DEFAULT_CLIENT_ID).unwrap();
 assert_eq!(a.expires_at,0);assert!(a.access_live(now+365*DAY));assert!(!a.needs_refresh(now+365*DAY));
 save(&f.context,&a).unwrap();
 let state=public(&f.context,&load(&f.context).unwrap()).unwrap();
 assert_eq!(state["authenticated"],true);assert_eq!(state["requiresLogin"],false);
}
// 启动检查（更新或重启之后）：该续期就续期、该复核就复核；任何失败都不清凭据。
#[test]
fn startup_resume_refreshes_and_verifies_without_clearing_on_failure(){
 let f=Fixture::new();let now=crate::now_ms();let mut a=account(now);a.expires_at=now-1;a.star_at=0;a.star_checked_at=0;save(&f.context,&a).unwrap();
 let before=f.context.host.read("keys",VAULT).unwrap().unwrap();
 resume_with(&f.context,now,&mut |_,_,_|Err("synthetic offline".into())).unwrap();
 assert_eq!(f.context.host.read("keys",VAULT).unwrap().unwrap(),before);
 let state=public(&f.context,&load(&f.context).unwrap()).unwrap();
 assert_eq!(state["authenticated"],true);assert_eq!(state["requiresLogin"],false);assert_eq!(state["verified"],false);
}
// 联网恢复后同一次启动检查把会话与 Star 一起补齐。
#[test]
fn startup_resume_completes_the_session_and_the_star_check(){
 let f=Fixture::new();let now=crate::now_ms();let mut a=account(now);a.expires_at=now-1;a.star_at=0;a.star_checked_at=0;save(&f.context,&a).unwrap();
 let mut http=|url:&str,token:Option<&str>,_form:Option<&[(&str,&str)]>|{
  if url.ends_with("access_token"){return answer(200,rotated());}
  assert_eq!(token,Some("synthetic-new-access"));
  if url.ends_with("/user"){answer(200,identity())}else{answer(204,json!({}))}
 };
 assert!(resume_with(&f.context,now,&mut http).unwrap());
 let state=public(&f.context,&load(&f.context).unwrap()).unwrap();
 assert_eq!(state["authenticated"],true);assert_eq!(state["verified"],true);
 assert_eq!(load(&f.context).unwrap().refresh_token,"synthetic-new-refresh");
}
