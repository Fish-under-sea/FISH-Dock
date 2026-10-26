//! 插件通知事件仅携带白名单上下文，不转发原始工具载荷或凭据。
use serde_json::{json,Value};
pub fn alert(mood:&str,project:&str,message:&Value)->Option<Value>{
 if !["waiting","done","error"].contains(&mood){return None;}
 fn limited(value:&str)->String{value.chars().filter(|c|!c.is_control()).take(1024).collect()}
 let workspace=message["workspace"].as_str().unwrap_or(project);
 let repo=message["repo"].as_str().unwrap_or_else(||project.trim_end_matches(['/', '\\']).rsplit(['/', '\\']).next().unwrap_or(project));
 Some(json!({"type":"agent.alert","mood":mood,"project":limited(project),"workspace":limited(workspace),"repo":limited(repo),"session":limited(message["session"].as_str().unwrap_or("")),"agent":limited(message["agent"].as_str().unwrap_or("")),"at":crate::now_ms()}))
}
pub fn publish(row:Value){
 use std::sync::{OnceLock,mpsc};
 static SENDER:OnceLock<mpsc::SyncSender<Value>>=OnceLock::new();
 let sender=SENDER.get_or_init(||{
  let (send,receive)=mpsc::sync_channel::<Value>(64);
  std::thread::spawn(move||for row in receive{if let Ok(mut runner)=crate::plugin_runtime::CURRENT.lock(){runner.event(row);}});
  send
 });
 if sender.try_send(row).is_err(){eprintln!("FISH Dock: 插件通知队列繁忙，本条通知已跳过");}
}
#[cfg(test)]mod tests{
 use super::*;
 #[test]fn notification_projection_excludes_private_payload_and_keeps_workspace_and_repo(){
  let input=json!({"workspace":"D:/work/repo","repo":"fish/repo","session":"session-a","agent":"dsh","token":"synthetic-secret","reply":"private-answer"});
  let row=alert("waiting","D:/work/repo",&input).unwrap();assert_eq!(row["workspace"],"D:/work/repo");assert_eq!(row["repo"],"fish/repo");assert_eq!(row["mood"],"waiting");assert!(!row.to_string().contains("synthetic-secret"));assert!(row.get("reply").is_none());
  let fallback=alert("done","D:\\work\\repo",&json!({})).unwrap();assert_eq!(fallback["workspace"],"D:\\work\\repo");assert_eq!(fallback["repo"],"repo");
  assert!(alert("working","project",&input).is_none());assert!(alert("error","project",&json!({"session":"s"})).is_some());
 }
}
