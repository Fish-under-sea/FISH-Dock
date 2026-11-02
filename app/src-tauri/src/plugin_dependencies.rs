//! 插件 Node 发现：正式版默认只信任已校验随包资源；显式覆盖优先。
use std::path::{Path,PathBuf};
use serde::Serialize;
#[derive(Clone,Debug,Serialize)]#[serde(rename_all="camelCase")]
pub struct Candidate {pub path:PathBuf,pub source:String,pub version:Option<String>,pub error:Option<String>}
#[derive(Clone,Debug,Serialize)]#[serde(rename_all="camelCase")]
pub struct Discovery {pub path:Option<PathBuf>,pub source:Option<String>,pub version:Option<String>,pub candidates:Vec<Candidate>,pub warning:Option<String>,pub installed:bool}
#[cfg(test)]mod tests {
 use super::*;
 #[test]fn stale_override_does_not_mask_bundled_runtime(){let got=select(true,vec![("saved",PathBuf::from("old")),("bundled",PathBuf::from("bundle")),("path",PathBuf::from("external"))],|p|if p==Path::new("old"){Err("路径不存在".into())}else{Ok("v24.14.0".into())});assert_eq!(got.source.as_deref(),Some("bundled"));assert!(got.warning.is_some());}
 #[test]fn release_never_silently_uses_path_and_keeps_valid_user_override(){for saved in [false,true]{let mut rows=vec![];if saved{rows.push(("saved",PathBuf::from("custom 中文/node.exe")))}rows.push(("path",PathBuf::from("path/node.exe")));let got=select(true,rows,|_|Ok("v24.13.0".into()));assert_eq!(got.path.is_some(),saved);}}
 #[test]fn development_discovers_path_and_rejects_incompatible_versions(){let got=select(false,vec![("path",PathBuf::from("old")),("path",PathBuf::from("new"))],|p|Ok(if p==Path::new("old"){"v20.1.0"}else{"v24.13.0"}.into()));assert_eq!(got.path,Some(PathBuf::from("new")));assert!(got.candidates[0].error.is_some());}
 // 探测缓存（T05）：同一个二进制不重复起进程；文件一变（换版本、原地升级）就重探。
 #[test]fn a_probed_node_is_remembered_until_the_binary_changes(){
  let path=std::env::temp_dir().join(format!("fish-dock-node-probe-{}.txt",std::process::id()));
  std::fs::write(&path,b"one").unwrap();
  assert_eq!(cached(&path),None,"没探过就不该有缓存");
  remember(&path,"v24.14.0");
  assert_eq!(cached(&path).as_deref(),Some("v24.14.0"));
  // 内容变了（长度也变）：旧结果作废，不能被当成同一个二进制继续用。
  std::fs::write(&path,b"one-and-a-half").unwrap();
  assert_eq!(cached(&path),None,"二进制变了还认旧版本就是缓存骗人");
  remember(&path,"v24.15.0");
  assert_eq!(cached(&path).as_deref(),Some("v24.15.0"),"同一路径只留最新一条");
  let _=std::fs::remove_file(&path);
 }
}

pub fn select(installed:bool, rows:Vec<(&str,PathBuf)>, mut probe:impl FnMut(&Path)->Result<String,String>)->Discovery {
 let mut result=Discovery{path:None,source:None,version:None,candidates:Vec::new(),warning:None,installed};
 for (source,path) in rows {
  let checked=probe(&path).and_then(|v|{let major=v.trim().trim_start_matches('v').split('.').next().and_then(|s|s.parse::<u32>().ok());if major==Some(24){Ok(v)}else{Err(format!("需要 Node.js 24，检测为 {}",v.trim()))}});
  let (version,error)=match checked{Ok(v)=>(Some(v),None),Err(e)=>(None,Some(e))};
  let eligible=!(installed&&matches!(source,"path"|"development"));
  if source=="saved"&&error.is_some(){result.warning=Some("已保存的 Node 路径失效或版本不兼容，已尝试其他运行时；可重新选择或清除覆盖。".into());}
  if result.path.is_none()&&eligible&&error.is_none(){result.path=Some(path.clone());result.source=Some(source.into());result.version=version.clone();}
  result.candidates.push(Candidate{path,source:source.into(),version,error});
 }
 result
}
pub fn probe(path:&Path)->Result<String,String>{
 crate::storage::safe(path)?;
 if !path.is_absolute()||!path.is_file(){return Err("Node 可执行文件路径不存在或不是绝对路径".into())}
 // 探测结果缓存在进程生命周期里（T05）：设置页每次重绘都会走 discover()，
 // 以前每轮都现起一次 `node --version`——冷态下第一次能到秒级，连开十次设置页
 // 就是十个进程。键里带上 (长度, 修改时刻)：换了二进制或原地升级自动失效。
 if let Some(hit)=cached(path){return Ok(hit)}
 let mut command=std::process::Command::new(path);command.arg("--version").env_remove("NODE_OPTIONS").stdin(std::process::Stdio::null()).stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::null());
 #[cfg(windows)]{use std::os::windows::process::CommandExt;command.creation_flags(0x08000000);}
 // 冷启放宽到 8 s（T05）：开机自启那阵子 Defender 正在扫、磁盘还在预热，2 s
 // 不是被 Node 用完的，是被系统占走的；放宽只影响「本来就慢」的那一次。
 let mut child=command.spawn().map_err(|_|"Node 版本检测启动失败")?;let deadline=std::time::Instant::now()+std::time::Duration::from_secs(8);
 loop{match child.try_wait(){Ok(Some(status))=>{if !status.success(){return Err("Node 版本检测失败".into())}let mut value=String::new();use std::io::Read;if let Some(stdout)=child.stdout.take(){stdout.take(256).read_to_string(&mut value).map_err(|_|"Node 版本输出无效")?;}let value=value.trim().to_owned();remember(path,&value);return Ok(value)},Err(_)=>{let _=child.kill();let _=child.wait();return Err("Node 状态检测失败".into())},_=>{}}if std::time::Instant::now()>=deadline{let _=child.kill();let _=child.wait();return Err("Node 版本检测超时".into())}std::thread::sleep(std::time::Duration::from_millis(10));}
}
// (路径, 长度, 修改时刻毫秒) → 版本。只存成功的探测；失败每次都重探（用户装好
// Node 之后下一次重绘就该认出来）。
static PROBED:std::sync::Mutex<Vec<(PathBuf,u64,u64,String)>>=std::sync::Mutex::new(Vec::new());
fn stamp(path:&Path)->Option<(u64,u64)>{
 let meta=std::fs::metadata(path).ok()?;
 let mtime=meta.modified().ok()?.duration_since(std::time::UNIX_EPOCH).ok()?.as_millis() as u64;
 Some((meta.len(),mtime))
}
fn cached(path:&Path)->Option<String>{
 let (len,mtime)=stamp(path)?;
 let entries=PROBED.lock().ok()?;
 entries.iter().find(|(at,size,when,_)|at==path&&*size==len&&*when==mtime).map(|(_,_,_,version)|version.clone())
}
fn remember(path:&Path,version:&str){
 let Some((len,mtime))=stamp(path) else{return};
 let Ok(mut entries)=PROBED.lock() else{return};
 entries.retain(|(at,_,_,_)|at!=path);
 entries.push((path.to_path_buf(),len,mtime,version.to_owned()));
}
pub fn discover(saved:Option<PathBuf>)->Discovery{
 let installed=cfg!(feature="installed");let mut rows=Vec::new();
 if let Some(path)=saved{rows.push(("saved",path));}
 #[cfg(debug_assertions)]if let Some(path)=std::env::var_os("FISH_DOCK_DEV_NODE"){rows.push(("development",PathBuf::from(path)));}
 #[cfg(feature="installed")]if let Ok(root)=crate::runtime::component("node"){rows.push(("bundled",root.join(if cfg!(windows){"node.exe"}else{"node"})));}
 if !installed{if let Some(paths)=std::env::var_os("PATH"){for directory in std::env::split_paths(&paths){let path=directory.join(if cfg!(windows){"node.exe"}else{"node"});if path.is_file()&&!rows.iter().any(|(_,p)|p==&path){rows.push(("path",path));}}}}
 let mut result=select(installed,rows,probe);if result.path.is_none()&&result.warning.is_none(){result.warning=Some(if installed{"随包 Node 缺失、损坏或不兼容，请修复安装或显式选择 Node.js 24。"}else{"开发构建未找到兼容 Node.js 24；请配置完整路径或系统 PATH。"}.into());}result
}
