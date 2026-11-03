//! 可扩展 Windhawk 集合；清单引用源码，摘要和元数据匹配后才进入编译确认。
use serde::{Deserialize,Serialize};
use serde_json::Value;
use std::collections::{BTreeMap,BTreeSet};
use sha2::{Digest,Sha256};

#[derive(Clone,Debug,Serialize,Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Collection {pub schema:u32,pub id:String,pub name:String,pub mods:Vec<Member>}
#[derive(Clone,Debug,Serialize,Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Member {pub id:String,pub version:String,pub sha256:String,pub source:Source,#[serde(default)]pub settings:BTreeMap<String,Value>}
#[derive(Clone,Debug,Serialize,Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Source {pub file:Option<String>,pub url:Option<String>}
#[derive(Clone,Debug,Serialize,Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceFile {pub name:String,pub source:String}
#[derive(Clone,Debug,Serialize,Deserialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
pub struct Prepared {pub id:String,pub name:String,pub mods:Vec<PreparedMod>}
#[derive(Clone,Debug,Serialize,Deserialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
pub struct PreparedMod {pub id:String,pub version:String,pub sha256:String,pub source:String,pub settings:BTreeMap<String,Value>}

fn local_name(name:&str)->bool{!name.is_empty()&&name.len()<=180&&!name.contains(['/', '\\', ':'])&&name.ends_with(".wh.cpp")&&!name.chars().any(char::is_control)}
pub fn secure_url(value:&str)->Result<tauri::Url,String>{let url=tauri::Url::parse(value).map_err(|_|"mod 源码地址无效")?;if value.len()>4096||url.scheme()!="https"||url.host_str().is_none()||!url.username().is_empty()||url.password().is_some()||url.fragment().is_some(){return Err("mod 源码需使用无身份信息的 HTTPS 地址".into());}Ok(url)}
pub fn validate_settings(values:&BTreeMap<String,Value>)->Result<(),String>{if values.len()>512{return Err("集合设置项过多".into());}for(k,v)in values{if k.is_empty()||k.len()>128||!k.bytes().all(|b|b.is_ascii_alphanumeric()||b"_.-[]".contains(&b)){return Err("集合设置键无效".into());}let valid=match v{Value::String(s)=>s.len()<=8192&&!s.chars().any(char::is_control),Value::Number(n)=>n.as_i64().is_some_and(|n|(-2147483648..=4294967295).contains(&n)),_=>false};if !valid{return Err("集合设置仅支持单行文本或 32 位整数".into());}}Ok(())}
pub fn prepare(manifest:&str,files:&[SourceFile],mut download:impl FnMut(&str)->Result<String,String>)->Result<Prepared,String>{
 if manifest.len()>512*1024||files.len()>64{return Err("集合清单或源文件数量超过限制".into());}
 let collection:Collection=serde_json::from_str(manifest).map_err(|_|"集合清单格式无效")?;
 if collection.schema!=1||!crate::windhawk_source::valid_id(&collection.id)||collection.name.trim().is_empty()||collection.name.len()>256||collection.name.chars().any(char::is_control)||collection.mods.is_empty()||collection.mods.len()>64{return Err("集合身份、版本或成员数无效".into());}
 let mut ids=BTreeSet::new();let mut names=BTreeSet::new();let mut total=0usize;
 for file in files{if !local_name(&file.name)||!names.insert(&file.name)||file.source.len()>2*1024*1024{return Err("集合本地源文件名重复、无效或超过限制".into());}total=total.checked_add(file.source.len()).ok_or("集合源码过大")?;}
 if total>16*1024*1024{return Err("集合源码总量超过 16 MiB".into());}
 for member in &collection.mods{
  if !crate::windhawk_source::valid_id(&member.id)||!ids.insert(&member.id)||member.version.trim().is_empty()||member.version.len()>128||member.sha256.len()!=64||!member.sha256.bytes().all(|b|b.is_ascii_hexdigit()){return Err("集合 mod 身份、版本或摘要无效或重复".into());}
  validate_settings(&member.settings)?;
  match (&member.source.file,&member.source.url){(Some(file),None)if local_name(file)=>{if !files.iter().any(|f|&f.name==file){return Err(format!("集合缺少本地源文件：{file}"));}},(None,Some(url))=>{secure_url(url)?;},_=>return Err("每个 mod 需要一个本地源码文件名或 HTTPS 来源".into())}
 }
 let mut prepared=Vec::new();let mut loaded=0usize;
 for member in collection.mods{
  let source=match (&member.source.file,&member.source.url){(Some(file),None)=>files.iter().find(|f|&f.name==file).unwrap().source.clone(),(None,Some(url))=>download(url)?,_=>unreachable!()};loaded+=source.len();
  if source.len()>2*1024*1024||loaded>16*1024*1024{return Err("集合源码超过大小限制".into());}
  if !format!("{:x}",Sha256::digest(source.as_bytes())).eq_ignore_ascii_case(&member.sha256){return Err(format!("{} 源码摘要与集合清单不符",member.id));}
  let metadata=crate::windhawk_source::metadata(&source)?;crate::windhawk_source::compilation(&source)?;
  if metadata.id!=member.id||metadata.version!=member.version{return Err(format!("{} 源码 ID 或版本与集合清单不符",member.id));}
  prepared.push(PreparedMod{id:member.id,version:member.version,sha256:member.sha256.to_ascii_lowercase(),source,settings:member.settings});
 }
 Ok(Prepared{id:collection.id,name:collection.name,mods:prepared})
}
// 2.0.4 T-D / 方案 §5（审计 A05）：远程源码不再自建 ureq agent，统一走
// `sources`（原地址 + 镜像候选 + 每候选的代理路 + 一次总预算 + 手动重定向）。
// 上限仍是 2 MiB，错误仍是一句人话（每候选原因由 sources 聚合）。
pub fn download_source(value:&str)->Result<String,String>{
 secure_url(value).map_err(|e|format!("mod 源码地址无效：{e}"))?;
 let bytes=crate::sources::fetch_bytes(value,crate::config::plugin_download_timeout(),2*1024*1024)
  .map_err(|fail|format!("mod 源码下载失败：{}",fail.reason()))?;
 String::from_utf8(bytes).map_err(|_|"mod 源码不是 UTF-8".into())
}

#[cfg(test)]mod tests {
 use super::*;
 fn source()->String{"// ==WindhawkMod==\n// @id sample\n// @name 示例\n// @version 1.2\n// @architecture x86-64\n// ==/WindhawkMod==\nBOOL Wh_ModInit(){return TRUE;}".into()}
 fn manifest()->String{serde_json::json!({"schema":1,"id":"desktop-set","name":"桌面集合","mods":[{"id":"sample","version":"1.2","sha256":format!("{:x}",Sha256::digest(source().as_bytes())),"source":{"file":"sample.wh.cpp"},"settings":{"count":4}}]}).to_string()}
 #[test]fn source_identity_and_digest_are_verified_before_returning_preview(){let got=prepare(&manifest(),&[SourceFile{name:"sample.wh.cpp".into(),source:source()}],|_|panic!("local source fetched remotely")).unwrap();assert_eq!(got.mods[0].id,"sample");assert_eq!(got.mods[0].settings["count"],4);let wrong=manifest().replace("\"version\":\"1.2\"","\"version\":\"1.3\"");assert!(prepare(&wrong,&[SourceFile{name:"sample.wh.cpp".into(),source:source()}],|_|panic!()).is_err());assert!(prepare(&manifest(),&[SourceFile{name:"sample.wh.cpp".into(),source:source()+"changed"}],|_|panic!()).is_err());}
 #[test]fn whole_manifest_is_checked_before_any_remote_fetch(){let mut value:Value=serde_json::from_str(&manifest()).unwrap();value["mods"][0]["source"]=serde_json::json!({"url":"https://example.com/sample.wh.cpp"});let duplicate=value["mods"][0].clone();value["mods"].as_array_mut().unwrap().push(duplicate);let result=prepare(&value.to_string(),&[],|_|panic!("duplicate manifest caused network access"));assert!(result.is_err());value["mods"].as_array_mut().unwrap().truncate(1);value["mods"][0]["source"]=serde_json::json!({"file":"../sample.wh.cpp"});assert!(prepare(&value.to_string(),&[],|_|panic!()).is_err());}
 #[test]fn remote_source_is_hash_pinned_and_credentials_are_excluded(){let mut value:Value=serde_json::from_str(&manifest()).unwrap();value["mods"][0]["source"]=serde_json::json!({"url":"https://example.com/sample.wh.cpp"});assert!(prepare(&value.to_string(),&[],|url|{assert_eq!(url,"https://example.com/sample.wh.cpp");Ok(source())}).is_ok());value["mods"][0]["source"]=serde_json::json!({"url":"https://user:secret@example.com/sample.wh.cpp"});assert!(prepare(&value.to_string(),&[],|_|panic!("credential URL reached downloader")).is_err());}
}
