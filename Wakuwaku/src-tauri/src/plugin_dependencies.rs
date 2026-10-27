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
 let mut command=std::process::Command::new(path);command.arg("--version").env_remove("NODE_OPTIONS").stdin(std::process::Stdio::null()).stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::null());
 #[cfg(windows)]{use std::os::windows::process::CommandExt;command.creation_flags(0x08000000);}
 let mut child=command.spawn().map_err(|_|"Node 版本检测启动失败")?;let deadline=std::time::Instant::now()+std::time::Duration::from_secs(2);
 loop{match child.try_wait(){Ok(Some(status))=>{if !status.success(){return Err("Node 版本检测失败".into())}let mut value=String::new();use std::io::Read;if let Some(stdout)=child.stdout.take(){stdout.take(256).read_to_string(&mut value).map_err(|_|"Node 版本输出无效")?;}return Ok(value.trim().to_owned())},Err(_)=>{let _=child.kill();let _=child.wait();return Err("Node 状态检测失败".into())},_=>{}}if std::time::Instant::now()>=deadline{let _=child.kill();let _=child.wait();return Err("Node 版本检测超时".into())}std::thread::sleep(std::time::Duration::from_millis(10));}
}
pub fn discover(saved:Option<PathBuf>)->Discovery{
 let installed=cfg!(feature="installed");let mut rows=Vec::new();
 if let Some(path)=saved{rows.push(("saved",path));}
 #[cfg(debug_assertions)]if let Some(path)=std::env::var_os("FISH_DOCK_DEV_NODE"){rows.push(("development",PathBuf::from(path)));}
 #[cfg(feature="installed")]if let Ok(root)=crate::runtime::component("node"){rows.push(("bundled",root.join(if cfg!(windows){"node.exe"}else{"node"})));}
 if !installed{if let Some(paths)=std::env::var_os("PATH"){for directory in std::env::split_paths(&paths){let path=directory.join(if cfg!(windows){"node.exe"}else{"node"});if path.is_file()&&!rows.iter().any(|(_,p)|p==&path){rows.push(("path",path));}}}}
 let mut result=select(installed,rows,probe);if result.path.is_none()&&result.warning.is_none(){result.warning=Some(if installed{"随包 Node 缺失、损坏或不兼容，请修复安装或显式选择 Node.js 24。"}else{"开发构建未找到兼容 Node.js 24；请配置完整路径或系统 PATH。"}.into());}result
}
