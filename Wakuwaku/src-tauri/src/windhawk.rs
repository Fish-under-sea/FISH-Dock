//! Windhawk 1.7.x 的受控适配；存储布局依据官方 c165997c 源码。
use std::{collections::BTreeMap,fs,path::{Path,PathBuf}};
use serde::{Deserialize,Serialize};
use serde_json::{json,Value};
use crate::windhawk_source::{self,Metadata};
#[path="windhawk_import.rs"] mod importer;
#[path="windhawk_bundle.rs"] mod bundle;

#[derive(Clone,Debug,Serialize,Deserialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
pub struct Layout { pub install:PathBuf,pub data:PathBuf,pub portable:bool,pub registry:String }
#[derive(Clone,Debug,Serialize)]
#[serde(rename_all="camelCase")]
pub struct Mod { pub id:String,pub disabled:bool,pub library_file:String,pub version:String,pub include:String,pub exclude:String,pub metadata:Option<Metadata> }
#[derive(Clone,Debug,Serialize,Deserialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
pub struct Change { pub id:String,pub disabled:bool }
#[derive(Serialize,Deserialize)]
#[serde(rename_all="camelCase")]
pub struct Outcome { pub id:String,pub ok:bool,pub disabled:Option<bool>,pub error:Option<String> }

fn decode(bytes:&[u8])->Result<String,String>{
 if bytes.starts_with(&[255,254]) {if bytes.len()%2!=0{return Err("INI UTF-16 内容损坏".into());}String::from_utf16(&bytes[2..].chunks_exact(2).map(|c|u16::from_le_bytes([c[0],c[1]])).collect::<Vec<_>>()).map_err(|_|"INI UTF-16 内容损坏".into())}
 else{String::from_utf8(bytes.to_vec()).map_err(|_|"INI 编码不受支持".into())}
}
fn ini(text:&str,section:&str)->BTreeMap<String,String>{
 let mut active=false;let mut out=BTreeMap::new();
 for row in text.lines(){let row=row.trim().trim_start_matches('\u{feff}');if row.starts_with('[')&&row.ends_with(']'){active=row[1..row.len()-1].eq_ignore_ascii_case(section);continue;}if !active||row.starts_with([';','#']){continue;}if let Some((key,value))=row.split_once('='){out.insert(key.trim().into(),value.trim().into());}}
 out
}
fn expand(value:&str)->Result<String,String>{
 let mut out=String::new();let mut rest=value;
 while let Some(at)=rest.find('%'){out.push_str(&rest[..at]);rest=&rest[at+1..];let end=rest.find('%').ok_or("环境变量标记未闭合")?;out.push_str(&std::env::var(&rest[..end]).map_err(|_|"Windhawk 路径环境变量未定义")?);rest=&rest[end+1..];}out.push_str(rest);Ok(out)
}
fn layout(install:&Path)->Result<Layout,String>{
 crate::storage::safe(install)?;
 if !install.is_absolute()||!install.join("windhawk.exe").is_file(){return Err("请选择含 windhawk.exe 的绝对安装目录".into());}
 let text=decode(&fs::read(install.join("windhawk.ini")).map_err(|_|"读取 windhawk.ini 失败")?)?;
 let values=ini(&text,"Storage");let data=expand(values.get("AppDataPath").ok_or("配置缺少 AppDataPath")?)?;let data=PathBuf::from(data);let data=if data.is_absolute(){data}else{install.join(data)};
 crate::storage::safe(&data)?;
 let portable=values.get("Portable").map(|v|v!="0").unwrap_or(false);let registry=values.get("RegistryKey").cloned().unwrap_or_default();
 if !portable&&!registry.starts_with("HKLM\\")&&!registry.starts_with("HKEY_LOCAL_MACHINE\\")&&!registry.starts_with("HKCU\\")&&!registry.starts_with("HKEY_CURRENT_USER\\"){return Err("Windhawk RegistryKey 不受支持".into());}
 Ok(Layout{install:install.into(),data,portable,registry})
}
fn configured()->Result<Option<Layout>,String>{
 let context=crate::storage_context::current().ok_or("存储上下文未就绪")?;
 if let Some(bytes)=context.host.read("config","windhawk.json")?{let path:PathBuf=serde_json::from_slice(&bytes).map_err(|_|"Windhawk 路径配置损坏")?;return layout(&path).map(Some);}
 if let Some(bundled)=bundle::configured()?{return Ok(Some(bundled));}
 #[cfg(windows)]{
  use winreg::{RegKey,enums::HKEY_LOCAL_MACHINE};
  for key in [r"SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\Windhawk",r"SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall\Windhawk"]{if let Ok(key)=RegKey::predef(HKEY_LOCAL_MACHINE).open_subkey(key){if let Ok(path)=key.get_value::<String,_>("InstallLocation"){if Path::new(&path).join("windhawk.exe").is_file(){return layout(Path::new(&path)).map(Some);}}}}
 }
 for key in ["ProgramFiles","ProgramFiles(x86)"]{if let Some(base)=std::env::var_os(key){let path=PathBuf::from(base).join("Windhawk");if path.join("windhawk.exe").is_file(){return layout(&path).map(Some);}}}
 Ok(None)
}
fn source_info(layout:&Layout,id:&str)->Option<Metadata>{let path=layout.data.join("ModsSource").join(format!("{id}.wh.cpp"));crate::storage::safe(&path).ok()?;windhawk_source::metadata(&fs::read_to_string(path).ok()?).ok()}
fn row(layout:&Layout,id:String,v:BTreeMap<String,String>)->Mod{Mod{metadata:source_info(layout,&id),id,disabled:v.get("Disabled").map(|v|v!="0").unwrap_or(true),library_file:v.get("LibraryFileName").cloned().unwrap_or_default(),version:v.get("Version").cloned().unwrap_or_default(),include:v.get("Include").cloned().unwrap_or_default(),exclude:v.get("Exclude").cloned().unwrap_or_default()}}
#[cfg(windows)]fn registry_root(layout:&Layout)->Result<(winreg::RegKey,String),String>{use winreg::{RegKey,enums::{HKEY_LOCAL_MACHINE,HKEY_CURRENT_USER}};let (root,path)=layout.registry.split_once('\\').ok_or("注册表根无效")?;let key=match root{"HKLM"|"HKEY_LOCAL_MACHINE"=>HKEY_LOCAL_MACHINE,"HKCU"|"HKEY_CURRENT_USER"=>HKEY_CURRENT_USER,_=>return Err("注册表根无效".into())};Ok((RegKey::predef(key),format!("{path}\\Engine\\Mods")))}
fn mods(layout:&Layout)->Result<Vec<Mod>,String>{
 let mut out=Vec::new();
 if layout.portable{let path=layout.data.join("Engine/Mods");crate::storage::safe(&path)?;if !path.exists(){return Ok(out);}
 for entry in fs::read_dir(path).map_err(|_|"读取便携 mod 目录失败")?{let entry=entry.map_err(|_|"读取 mod 失败")?;let path=entry.path();if path.extension().and_then(|v|v.to_str())!=Some("ini"){continue;}crate::storage::safe(&path)?;let id=path.file_stem().and_then(|v|v.to_str()).ok_or("mod 文件名无效")?;if !windhawk_source::valid_id(id){continue;}let values=ini(&decode(&fs::read(&path).map_err(|_|"读取 mod INI 失败")?)?,"Mod");out.push(row(layout,id.into(),values));}}
 else{#[cfg(windows)]{let (root,key)=registry_root(layout)?;let root=root.open_subkey(key).map_err(|_|"读取 Windhawk mod 注册表失败")?;for id in root.enum_keys(){let id=id.map_err(|_|"枚举 mod 注册表失败")?;if !windhawk_source::valid_id(&id){continue;}let key=root.open_subkey(&id).map_err(|_|"读取 mod 注册表失败")?;let mut values=BTreeMap::new();for name in ["LibraryFileName","Version","Include","Exclude"]{values.insert(name.into(),key.get_value::<String,_>(name).unwrap_or_default());}values.insert("Disabled".into(),key.get_value::<u32,_>("Disabled").unwrap_or(1).to_string());out.push(row(layout,id,values));}}
 #[cfg(not(windows))]return Err("注册表 mod 仅支持 Windows".into());}
 out.sort_by(|a,b|a.id.cmp(&b.id));Ok(out)
}
fn validate_changes(changes:&[Change])->Result<(),String>{if changes.len()>64{return Err("一次最多修改 64 个 mod".into());}let mut ids=std::collections::BTreeSet::new();for c in changes{if !windhawk_source::valid_id(&c.id)||!ids.insert(&c.id){return Err("mod ID 无效或重复".into());}}Ok(())}
fn write_disabled(layout:&Layout,change:&Change)->Result<(),String>{
 if layout.portable{let path=layout.data.join("Engine/Mods").join(format!("{}.ini",change.id));crate::storage::safe(&path)?;if !path.is_file(){return Err("mod 配置不存在".into());}
 #[cfg(windows)]{use std::os::windows::ffi::OsStrExt;#[link(name="kernel32")]unsafe extern "system"{fn WritePrivateProfileStringW(section:*const u16,key:*const u16,value:*const u16,file:*const u16)->i32;}
 let section:Vec<u16>="Mod\0".encode_utf16().collect();let key:Vec<u16>="Disabled\0".encode_utf16().collect();let value:Vec<u16>=format!("{}\0",u8::from(change.disabled)).encode_utf16().collect();let file:Vec<u16>=path.as_os_str().encode_wide().chain(Some(0)).collect();if unsafe{WritePrivateProfileStringW(section.as_ptr(),key.as_ptr(),value.as_ptr(),file.as_ptr())}==0{return Err("便携 mod 配置写入失败".into());}}
 #[cfg(not(windows))]return Err("mod 写入仅支持 Windows".into());
 }else{#[cfg(windows)]{use winreg::enums::KEY_SET_VALUE;let(root,key)=registry_root(layout)?;let key=root.open_subkey_with_flags(format!("{key}\\{}",change.id),KEY_SET_VALUE).map_err(|_|"mod 配置写入权限不足，请使用提权操作")?;key.set_value("Disabled",&u32::from(change.disabled)).map_err(|_|"mod 开关写入失败")?;}
 #[cfg(not(windows))]return Err("注册表写入仅支持 Windows".into());}
 Ok(())
}
fn apply(layout:&Layout,changes:&[Change])->Result<Vec<Outcome>,String>{validate_changes(changes)?;let before=mods(layout)?;
 Ok(changes.iter().map(|change|{let result=(||{let old=before.iter().find(|m|m.id==change.id).ok_or("mod 尚未安装")?;if !change.disabled&&old.library_file.is_empty(){return Err("mod 尚未编译，请先导入源码编译".into());}if !change.disabled{bundle::ensure_running(layout)?;}write_disabled(layout,change)?;let actual=mods(layout)?.into_iter().find(|m|m.id==change.id).ok_or("写入后 mod 消失")?;if actual.disabled!=change.disabled{return Err("mod 状态读取与请求不一致".into());}Ok(actual.disabled)})();match result{Ok(disabled)=>Outcome{id:change.id.clone(),ok:true,disabled:Some(disabled),error:None},Err(error)=>Outcome{id:change.id.clone(),ok:false,disabled:None,error:Some(error)}}}).collect())
}
pub fn resume_bundled()->Result<(),String>{if let Some(layout)=configured()?{if bundle::is_bundled(&layout)&&mods(&layout)?.iter().any(|m|!m.disabled){bundle::ensure_running(&layout)?;}}Ok(())}
#[tauri::command]pub fn windhawk_state()->Result<Value,String>{let host=&crate::storage_context::current().ok_or("存储上下文未就绪")?.host;let mut state=match configured()?{Some(layout)=>json!({"installed":true,"bundled":bundle::is_bundled(&layout),"running":bundle::running(),"mods":mods(&layout)?,"layout":layout}),None=>json!({"installed":false,"mods":[]})};if let Some(fields)=importer::status(host)?.as_object(){for(k,v)in fields{state[k]=v.clone();}}Ok(state)}
#[tauri::command]pub fn windhawk_select(path:String)->Result<Value,String>{let selected=layout(Path::new(&path))?;let context=crate::storage_context::current().ok_or("存储上下文未就绪")?;context.host.write("config","windhawk.json",&serde_json::to_vec(&selected.install).map_err(|_|"路径编码失败")?)?;windhawk_state()}
pub const ADMIN_FLAG:&str="--windhawk-apply";
fn elevated_apply(app:&tauri::AppHandle,changes:&[Change])->Result<Vec<Outcome>,String>{
 validate_changes(changes)?;let context=crate::storage_context::current().ok_or("存储上下文未就绪")?;
 let token=format!("windhawk-{}-{}",std::process::id(),crate::now_ms());let request=format!("{token}.request.json");let result=format!("{token}.result.json");
 context.host.write("tmp",&request,&serde_json::to_vec(changes).map_err(|_|"提权请求编码失败")?)?;
 let exe=std::env::current_exe().map_err(|_|"当前程序路径不可用")?;
 let root=context.root.to_string_lossy();if root.contains('"'){return Err("数据根含无效引号".into());}
 let params=format!("{ADMIN_FLAG} {token} --data-root \"{root}\"");
 let sh=crate::shared(app);let asked=crate::lhm_setup::elevate(&sh,&exe,&params,&context.host.path().join("tmp"),Some(60_000));
 let answer=context.host.read("tmp",&result)?;
 for name in [&request,&result]{let file=context.host.file("tmp",name)?;let _=fs::remove_file(file);}
 match asked{crate::lhm_setup::Asked::Ran(_)=>{},crate::lhm_setup::Asked::Declined=>return Err("系统提权已取消，未应用批次".into()),crate::lhm_setup::Asked::Failed(error)=>return Err(format!("系统提权失败：{error}"))}
 serde_json::from_slice(&answer.ok_or("提权进程未返回结果，请刷新查看实际状态")?).map_err(|_|"提权结果格式错误".into())
}
pub fn admin(token:&str,args:&[String])->i32{
 let run=||->Result<(),String>{if !token.starts_with("windhawk-")||token.len()>100||!token.bytes().all(|c|c.is_ascii_alphanumeric()||c==b'-'){return Err("提权请求标识无效".into());}
 let context=crate::storage_context::start(args)?;let bytes=context.host.read("tmp",&format!("{token}.request.json"))?.ok_or("提权请求不存在")?;
 let changes:Vec<Change>=serde_json::from_slice(&bytes).map_err(|_|"提权请求格式无效")?;validate_changes(&changes)?;
 let outcomes=apply(&configured()?.ok_or("Windhawk 未就绪")?,&changes)?;
 context.host.write("tmp",&format!("{token}.result.json"),&serde_json::to_vec(&outcomes).map_err(|_|"提权结果编码失败")?)?;Ok(())};
 match run(){Ok(())=>0,Err(error)=>{eprintln!("Windhawk: {error}");1}}
}
#[tauri::command]pub fn windhawk_apply(app:tauri::AppHandle,changes:Vec<Change>,elevated:Option<bool>)->Result<Vec<Outcome>,String>{if elevated==Some(true){elevated_apply(&app,&changes)}else{apply(&configured()?.ok_or("请先安装或选择 Windhawk")?,&changes)}}
#[tauri::command]pub fn windhawk_open()->Result<(),String>{let layout=configured()?.ok_or("请先安装或选择 Windhawk")?;let mut cmd=std::process::Command::new(layout.install.join("windhawk.exe"));cmd.arg("-run-ui").current_dir(&layout.install);#[cfg(windows)]{use std::os::windows::process::CommandExt;cmd.creation_flags(0x08000000);}cmd.spawn().map(|_|()).map_err(|_|"Windhawk 设置启动失败".into())}
#[tauri::command]pub fn windhawk_preview(source:String)->Result<Value,String>{let meta=windhawk_source::metadata(&source)?;let info=windhawk_source::compilation(&source)?;let current=configured()?;let existing=if let Some(layout)=&current{mods(layout)?.into_iter().find(|m|m.id==meta.id)}else{None};let fingerprint=current.as_ref().map(|layout|importer::preview(layout,&source)).transpose()?;Ok(json!({"metadata":meta,"existing":existing,"bytes":source.len(),"fingerprint":fingerprint,"architecture":info.architecture,"defaults":info.settings}))}
#[tauri::command]pub async fn windhawk_import(source:String,fingerprint:String)->Result<Value,String>{let layout=configured()?.ok_or("请先安装或选择 Windhawk")?;let host=crate::storage_context::current().ok_or("存储上下文未就绪")?.host.clone();tauri::async_runtime::spawn_blocking(move||importer::install(&layout,&host,&source,&fingerprint)).await.map_err(|_|"mod 导入任务意外结束".to_string())?}
#[tauri::command]pub async fn windhawk_rollback(token:Option<String>)->Result<Value,String>{let layout=configured()?.ok_or("请先安装或选择 Windhawk")?;let host=crate::storage_context::current().ok_or("存储上下文未就绪")?.host.clone();tauri::async_runtime::spawn_blocking(move||importer::rollback(&layout,&host,token.as_deref())).await.map_err(|_|"mod 恢复任务意外结束".to_string())?}

#[tauri::command]pub async fn windhawk_fetch_source(url:String)->Result<Value,String>{tauri::async_runtime::spawn_blocking(move||{let source=crate::windhawk_collection::download_source(&url)?;let preview=windhawk_preview(source.clone())?;Ok(json!({"source":source,"preview":preview}))}).await.map_err(|_|"mod 下载任务意外结束".to_string())?}
#[tauri::command]pub async fn windhawk_collection_preview(manifest:String,files:Vec<crate::windhawk_collection::SourceFile>)->Result<Value,String>{tauri::async_runtime::spawn_blocking(move||{let collection=crate::windhawk_collection::prepare(&manifest,&files,crate::windhawk_collection::download_source)?;let mut rows=Vec::new();for member in collection.mods{let preview=windhawk_preview(member.source.clone())?;rows.push(json!({"member":member,"preview":preview}));}Ok(json!({"id":collection.id,"name":collection.name,"mods":rows}))}).await.map_err(|_|"mod 集合预览任务意外结束".to_string())?}
#[derive(Deserialize)]#[serde(deny_unknown_fields)]pub struct CollectionChoice{pub member:crate::windhawk_collection::PreparedMod,pub fingerprint:String}
#[tauri::command]pub async fn windhawk_collection_import(members:Vec<CollectionChoice>)->Result<Value,String>{let layout=configured()?.ok_or("请先安装或选择 Windhawk")?;let host=crate::storage_context::current().ok_or("存储上下文未就绪")?.host.clone();tauri::async_runtime::spawn_blocking(move||{
 use sha2::{Digest,Sha256};if members.is_empty()||members.len()>64{return Err("集合成员数无效".into());}let mut ids=std::collections::BTreeSet::new();let mut total=0usize;
 for row in &members{let member=&row.member;total+=member.source.len();if !ids.insert(&member.id)||member.source.len()>2*1024*1024||total>16*1024*1024{return Err("集合重复或超过大小限制".into());}crate::windhawk_collection::validate_settings(&member.settings)?;let meta=windhawk_source::metadata(&member.source)?;windhawk_source::compilation(&member.source)?;if meta.id!=member.id||meta.version!=member.version||format!("{:x}",Sha256::digest(member.source.as_bytes()))!=member.sha256{return Err("集合源文件已在预览后变化".into());}if importer::preview(&layout,&member.source)?!=row.fingerprint{return Err("集合中的 mod 已变化，请重新预览".into());}}
 let mut results=Vec::new();for row in members{let member=row.member;let result=importer::install_preset(&layout,&host,&member.source,&row.fingerprint,&member.settings);results.push(match result{Ok(answer)=>json!({"id":member.id,"ok":true,"answer":answer}),Err(error)=>json!({"id":member.id,"ok":false,"error":error})});}
 host.write("config","windhawk-last-collection.json",&serde_json::to_vec(&results).map_err(|_|"集合结果编码失败")?)?;Ok(json!({"results":results}))
 }).await.map_err(|_|"mod 集合导入任务意外结束".to_string())?}
#[cfg(test)]mod tests{use super::*;
 #[test]fn ini_handles_sections_and_unicode(){let values=ini("[Other]\nAppDataPath=bad\n[Storage]\nAppDataPath=数据\\Windhawk\nPortable=1\n;注释\n","Storage");assert_eq!(values.get("AppDataPath").unwrap(),"数据\\Windhawk");assert_eq!(values.get("Portable").unwrap(),"1");let bytes:[u8;8]=[255,254,65,0,45,78,10,0];assert_eq!(decode(&bytes).unwrap(),"A中\n");}
 #[test]fn batch_rejects_duplicate_and_path_ids(){assert!(validate_changes(&[Change{id:"../escape".into(),disabled:false}]).is_err());let c=Change{id:"test-mod".into(),disabled:true};assert!(validate_changes(&[c.clone(),c.clone()]).is_err());assert!(validate_changes(&[c]).is_ok());}
}
