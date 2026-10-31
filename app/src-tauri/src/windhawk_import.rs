//! Windhawk 源码编译及同 ID 导入事务；编译成功后先保持停用，启用由独立开关完成。
use super::*;
use crate::storage_context::Namespace;
use sha2::{Digest,Sha256};
use std::{io::Write,sync::Mutex};
static EDIT:Mutex<()>=Mutex::new(());
const ACTIVE:&str="windhawk-import-active.json";
#[derive(Debug)]
struct Plan { compiler:PathBuf,engine:PathBuf,stage:PathBuf,library:String,targets:Vec<(&'static str,&'static str)>,source:String,meta:Metadata,settings:BTreeMap<String,Value>,presets:BTreeMap<String,Value>,options:Vec<String>,architecture:Vec<String> }
#[derive(Clone,Debug,Serialize,Deserialize,PartialEq,Eq)]
#[serde(deny_unknown_fields)]
struct Raw { kind:u32,bytes:Vec<u8> }
type Tree=BTreeMap<String,BTreeMap<String,Raw>>;
#[derive(Clone,Debug,Serialize,Deserialize,PartialEq,Eq)]
#[serde(deny_unknown_fields)]
struct Snapshot { source:Option<Vec<u8>>,ini:Option<Vec<u8>>,registry:Option<Tree> }
#[derive(Serialize,Deserialize)]
#[serde(deny_unknown_fields)]
struct Journal { schema:u32,token:String,id:String,layout:Layout,before:Snapshot,after:Snapshot,rolled_back:bool }
fn token()->String { static NEXT:std::sync::atomic::AtomicU64=std::sync::atomic::AtomicU64::new(0);format!("wh-{}-{}-{}",std::process::id(),crate::now_ms(),NEXT.fetch_add(1,std::sync::atomic::Ordering::Relaxed)) }
fn hash(snapshot:&Snapshot)->Result<String,String>{Ok(format!("{:x}",Sha256::digest(serde_json::to_vec(snapshot).map_err(|_|"mod 摘要编码失败")?)))}
fn optional(path:&Path)->Result<Option<Vec<u8>>,String>{crate::identity_store::read_optional(path)}
fn snapshot(layout:&Layout,id:&str)->Result<Snapshot,String>{
 if !windhawk_source::valid_id(id){return Err("mod ID 无效".into());}
 Ok(Snapshot{source:optional(&layout.data.join("ModsSource").join(format!("{id}.wh.cpp")))?,ini:if layout.portable{optional(&layout.data.join("Engine/Mods").join(format!("{id}.ini")))?}else{None},registry:if layout.portable{None}else{registry_read(layout,id)?}})
}
#[cfg(windows)]
fn registry_read(layout:&Layout,id:&str)->Result<Option<Tree>,String>{
 use winreg::enums::{KEY_READ,KEY_WOW64_64KEY};
 fn visit(key:&winreg::RegKey,name:&str,out:&mut Tree,depth:usize)->Result<(),String>{
  if depth>8||out.len()>64{return Err("mod 注册表层数过多".into());}
  let mut values=BTreeMap::new();for item in key.enum_values(){let(name,value)=item.map_err(|_|"mod 注册表值读取失败")?;if name.contains('\0')||value.bytes.len()>1024*1024||values.len()>4096{return Err("mod 注册表值超出限制".into());}values.insert(name,Raw{kind:value.vtype as u32,bytes:value.bytes.into_owned()});}out.insert(name.into(),values);
  for child in key.enum_keys(){let child=child.map_err(|_|"mod 注册表子项读取失败")?;if child.contains(['\\','/']){return Err("mod 注册表子项无效".into());}let path=if name.is_empty(){child.clone()}else{format!("{name}\\{child}")};visit(&key.open_subkey_with_flags(child,KEY_READ|KEY_WOW64_64KEY).map_err(|_|"mod 注册表子项打开失败")?,&path,out,depth+1)?;}Ok(())
 }
 let(root,path)=registry_root(layout)?;
 match root.open_subkey_with_flags(format!("{path}\\{id}"),KEY_READ|KEY_WOW64_64KEY){Ok(key)=>{let mut out=Tree::new();visit(&key,"",&mut out,0)?;Ok(Some(out))},Err(e)if e.kind()==std::io::ErrorKind::NotFound=>Ok(None),Err(_)=>Err("mod 注册表读取失败".into())}
}
#[cfg(not(windows))]fn registry_read(_: &Layout,_:&str)->Result<Option<Tree>,String>{Err("注册表 mod 仅支持 Windows".into())}
fn path_setting(install:&Path,values:&BTreeMap<String,String>,key:&str)->Result<PathBuf,String>{let path=PathBuf::from(expand(values.get(key).ok_or_else(||format!("Windhawk 缺少 {key}"))?)?);let path=if path.is_absolute(){path}else{install.join(path)};crate::storage::safe(&path)?;Ok(path)}
fn plan(layout:&Layout,host:&Namespace,source:&str,token:&str)->Result<Plan,String>{
 let meta=windhawk_source::metadata(source)?;let info=windhawk_source::compilation(source)?;
 let text=decode(&fs::read(layout.install.join("windhawk.ini")).map_err(|_|"Windhawk 配置读取失败")?)?;let values=ini(&text,"Storage");
 let compiler=path_setting(&layout.install,&values,"CompilerPath")?;let engine=path_setting(&layout.install,&values,"EnginePath")?;
 if !compiler.join("bin/clang++.exe").is_file(){return Err("Windhawk clang++ 编译器缺失，请修复 Windhawk 安装".into());}
 let architecture=if info.architecture.is_empty(){vec!["x86".into(),"x86-64".into()]}else{info.architecture};let mut targets=Vec::new();
 for a in &architecture {let target=match a.as_str(){"x86"=>("32","i686-w64-mingw32"),"x86-64"|"amd64"=>("64","x86_64-w64-mingw32"),"arm64"=>("arm64","aarch64-w64-mingw32"),_=>return Err("mod 编译架构无效".into())};if !engine.join(target.0).join("windhawk.lib").is_file(){return Err(format!("Windhawk 缺少 {} 引擎链接库",target.0));}if !targets.contains(&target){targets.push(target);}}
 let stage=host.file("tmp",token)?;crate::storage::safe(&stage)?;fs::create_dir(&stage).map_err(|_|"编译暂存目录创建失败")?;
 Ok(Plan{compiler,engine,stage,library:format!("{}_{}.dll",meta.id,token),targets,source:source.into(),meta,settings:info.settings,presets:BTreeMap::new(),options:info.options,architecture})
}
fn args(plan:&Plan,folder:&str,target:&str)->Vec<std::ffi::OsString>{
 let mut args:Vec<std::ffi::OsString>=["-std=c++23","-O2","-shared","-DUNICODE","-D_UNICODE","-D__USE_MINGW_ANSI_STDIO=0","-DWH_MOD"].iter().map(Into::into).collect();
 if !(plan.meta.id=="classic-taskdlg-fix"&&plan.meta.version=="1.1.0"){args.extend(["-DWINVER=0x0A00","-D_WIN32_WINNT=0x0A00","-D_WIN32_IE=0x0A00","-DNTDDI_VERSION=0x0A000008"].map(Into::into));}
 args.push(format!("-DWH_MOD_ID=L{}",serde_json::to_string(&plan.meta.id).unwrap()).into());args.push(format!("-DWH_MOD_VERSION=L{}",serde_json::to_string(&plan.meta.version).unwrap()).into());
 args.push(plan.engine.join(folder).join("windhawk.lib").into());args.extend(["-x","c++"].map(Into::into));args.push(plan.stage.join("source.wh.cpp").into());args.extend(["-include","windhawk_api.h","-target",target,"-Wl,--export-all-symbols","-o"].map(Into::into));args.push(plan.stage.join(folder).join(&plan.library).into());args.extend(plan.options.iter().map(Into::into));
 let compatibility:&[&str]=match(plan.meta.id.as_str(),plan.meta.version.as_str()) {("chrome-ui-tweaks","1.0.0")=>&["atomic","optional"],("sib-plusplus-tweaker","0.7.1")=>&["atomic"],("classic-explorer-treeview","1.1.3")|("sysdm-general-tab","1.1")=>&["cmath"],("ce-disable-process-button-flashing","1.0.1")|("windows-7-clock-spacing","1.0.0")=>&["vector"],_=>&[]};for header in compatibility{args.push("-include".into());args.push((*header).into());}args
}
fn compile(plan:&Plan)->Result<(),String>{
 use std::{process::{Command,Stdio},time::{Instant,Duration}};
 fs::write(plan.stage.join("source.wh.cpp"),&plan.source).map_err(|_|"编译源文件暂存失败")?;
 for &(folder,target) in &plan.targets{
  fs::create_dir(plan.stage.join(folder)).map_err(|_|"编译输出目录创建失败")?;
  let log_path=plan.stage.join(format!("{folder}.log"));let log=fs::File::create(&log_path).map_err(|_|"编译日志创建失败")?;
  let mut cmd=Command::new(plan.compiler.join("bin/clang++.exe"));cmd.args(args(plan,folder,target)).current_dir(&plan.compiler).stdin(Stdio::null()).stdout(Stdio::from(log.try_clone().map_err(|_|"编译日志复制失败")?)).stderr(Stdio::from(log));
  #[cfg(windows)]{use std::os::windows::process::CommandExt;cmd.creation_flags(0x08000000);}
  let mut child=cmd.spawn().map_err(|e|format!("编译器启动失败：{e}"))?;
  let owner=match crate::plugin_job::Owner::attach(&child){Ok(owner)=>owner,Err(error)=>{let _=child.kill();let _=child.wait();return Err(error);}};
  let start=Instant::now();let status=loop{if start.elapsed()>Duration::from_secs(120)||fs::metadata(&log_path).map(|m|m.len()>1024*1024).unwrap_or(false){drop(owner);let _=child.kill();let _=child.wait();return Err("编译超时或日志超过 1 MiB，编译任务已结束".into());}if let Some(status)=child.try_wait().map_err(|_|"读取编译器状态失败")?{break status;}std::thread::sleep(Duration::from_millis(25));};drop(owner);
  if !status.success(){let bytes=fs::read(&log_path).unwrap_or_default();let message=String::from_utf8_lossy(&bytes[..bytes.len().min(24*1024)]);return Err(format!("{target} 编译失败：\n{message}"));}
  for(from,to)in [("libc++.dll","libc++.whl"),("libunwind.dll","libunwind.whl"),("windhawk-mod-shim.dll","windhawk-mod-shim.dll")]{let source=plan.compiler.join(target).join("bin").join(from);crate::storage::safe(&source)?;fs::copy(source,plan.stage.join(folder).join(to)).map_err(|_|format!("{target} 编译运行库缺失：{from}"))?;}
 }
 Ok(())
}
fn fields(plan:&Plan)->BTreeMap<String,String>{BTreeMap::from([("LibraryFileName".into(),plan.library.clone()),("Version".into(),plan.meta.version.clone()),("Include".into(),plan.meta.include.join("|")),("Exclude".into(),plan.meta.exclude.join("|")),("Architecture".into(),plan.architecture.join("|")),("Disabled".into(),"1".into())])}
fn raw_string(value:&str)->Raw{Raw{kind:1,bytes:value.encode_utf16().chain(Some(0)).flat_map(u16::to_le_bytes).collect()}}
fn raw_integer(value:i64)->Raw{Raw{kind:4,bytes:(value as u32).to_le_bytes().to_vec()}}
fn prefix(name:&str)->String{name.find('[').map(|at|format!("{}[0]",&name[..at])).unwrap_or_else(||name.into())}
fn desired(before:&Snapshot,plan:&Plan,portable:bool)->Result<Snapshot,String>{
 let mut out=before.clone();out.source=Some(plan.source.as_bytes().to_vec());
 if portable{
  let text=before.ini.as_deref().map(decode).transpose()?.unwrap_or_default();let mut sections=BTreeMap::<String,BTreeMap<String,String>>::new();let mut name=String::new();
  for row in text.lines(){let row=row.trim().trim_start_matches('\u{feff}');if row.starts_with('[')&&row.ends_with(']'){name=row[1..row.len()-1].into();sections.entry(name.clone()).or_default();}else if !row.starts_with([';','#']){if let Some((k,v))=row.split_once('='){sections.entry(name.clone()).or_default().insert(k.trim().into(),v.trim().into());}}}
  let mod_key=sections.keys().find(|k|k.eq_ignore_ascii_case("Mod")).cloned().unwrap_or("Mod".into());sections.entry(mod_key).or_default().extend(fields(plan));
  let settings_key=sections.keys().find(|k|k.eq_ignore_ascii_case("Settings")).cloned().unwrap_or("Settings".into());let settings=sections.entry(settings_key).or_default();let existing:std::collections::BTreeSet<_>=settings.keys().map(|k|prefix(k)).collect();for(k,v)in &plan.settings{if !existing.contains(&prefix(k)){settings.insert(k.clone(),v.as_str().map(str::to_owned).unwrap_or_else(||v.to_string()));}}
  for(k,v)in &plan.presets{settings.insert(k.clone(),v.as_str().map(str::to_owned).unwrap_or_else(||v.to_string()));}
   let mut text=String::new();for(section,values)in sections{if !section.is_empty(){text.push_str(&format!("[{section}]\r\n"));}for(k,v)in values{text.push_str(&format!("{k}={v}\r\n"));}text.push_str("\r\n");}out.ini=Some([vec![255,254],text.encode_utf16().flat_map(u16::to_le_bytes).collect()].concat());
 }else{
  let mut tree=before.registry.clone().unwrap_or_default();let values=tree.entry(String::new()).or_default();for(k,v)in fields(plan){values.insert(k.clone(),if k=="Disabled"{raw_integer(1)}else{raw_string(&v)});}
  let settings=tree.entry("Settings".into()).or_default();let existing:std::collections::BTreeSet<_>=settings.keys().map(|k|prefix(k)).collect();for(k,v)in &plan.settings{if !existing.contains(&prefix(k)){settings.insert(k.clone(),if let Some(v)=v.as_str(){raw_string(v)}else{raw_integer(v.as_i64().ok_or("设置数值无效")?)});}}for(k,v)in &plan.presets{settings.insert(k.clone(),if let Some(v)=v.as_str(){raw_string(v)}else{raw_integer(v.as_i64().ok_or("设置数值无效")?)});}out.registry=Some(tree);
 }
 Ok(out)
}
fn put(path:&Path,bytes:Option<&[u8]>)->Result<(),String>{
 crate::storage::safe(path)?;
 if let Some(bytes)=bytes{fs::create_dir_all(path.parent().ok_or("mod 路径缺少目录")?).map_err(|_|"mod 目录创建失败")?;let temporary=path.with_file_name(format!(".fish-{}.tmp",token()));let mut file=fs::OpenOptions::new().create_new(true).write(true).open(&temporary).map_err(|_|"mod 暂存文件创建失败")?;file.write_all(bytes).and_then(|_|file.sync_all()).map_err(|_|"mod 暂存文件写入失败")?;drop(file);crate::storage::replace(&temporary,path).map_err(|_|"mod 文件提交失败")?;
 }else{match fs::remove_file(path){Ok(())=>{},Err(e)if e.kind()==std::io::ErrorKind::NotFound=>{},Err(_)=>return Err("mod 旧文件撤回失败".into())}}Ok(())
}
#[cfg(windows)]fn registry_write(layout:&Layout,id:&str,tree:Option<&Tree>)->Result<(),String>{
 use winreg::enums::*;
 let(root,path)=registry_root(layout)?;let(parent,_)=root.create_subkey_with_flags(path,KEY_READ|KEY_WRITE|KEY_WOW64_64KEY).map_err(|_|"mod 注册表需要写入权限")?;
 match parent.delete_subkey_all(id){Ok(())=>{},Err(e)if e.kind()==std::io::ErrorKind::NotFound=>{},Err(_)=>return Err("mod 注册表替换失败".into())};
 if let Some(tree)=tree{for(name,values)in tree{if name.split('\\').any(|p|p==".."||p=="."||p.contains('/')){return Err("mod 备份子项无效".into());}let name=if name.is_empty(){id.into()}else{format!("{id}\\{name}")};let(key,_)=parent.create_subkey_with_flags(name,KEY_WRITE|KEY_WOW64_64KEY).map_err(|_|"mod 注册表子项创建失败")?;for(name,value)in values{let vtype=match value.kind{0=>REG_NONE,1=>REG_SZ,2=>REG_EXPAND_SZ,3=>REG_BINARY,4=>REG_DWORD,5=>REG_DWORD_BIG_ENDIAN,6=>REG_LINK,7=>REG_MULTI_SZ,8=>REG_RESOURCE_LIST,9=>REG_FULL_RESOURCE_DESCRIPTOR,10=>REG_RESOURCE_REQUIREMENTS_LIST,11=>REG_QWORD,_=>return Err("mod 备份值类型无效".into())};key.set_raw_value(name,&winreg::RegValue{bytes:value.bytes.clone().into(),vtype}).map_err(|_|"mod 注册表值提交失败")?;}}}Ok(())
}
#[cfg(not(windows))]fn registry_write(_: &Layout,_:&str,_:Option<&Tree>)->Result<(),String>{Err("注册表 mod 仅支持 Windows".into())}
fn apply_snapshot(layout:&Layout,id:&str,state:&Snapshot)->Result<(),String>{
 if !windhawk_source::valid_id(id){return Err("mod ID 无效".into());}
 if layout.portable{put(&layout.data.join("Engine/Mods").join(format!("{id}.ini")),state.ini.as_deref())?;}else{registry_write(layout,id,state.registry.as_ref())?;}
 put(&layout.data.join("ModsSource").join(format!("{id}.wh.cpp")),state.source.as_deref())?;
 if snapshot(layout,id)?!=*state{return Err("mod 提交后的内容校验失败".into());}Ok(())
}
fn valid_dll(bytes:&[u8],folder:&str)->bool{if bytes.len()<64||&bytes[..2]!=b"MZ"{return false;}let offset=u32::from_le_bytes(bytes[60..64].try_into().unwrap())as usize;let Some(header)=offset.checked_add(24).and_then(|end|bytes.get(offset..end))else{return false;};let machine=u16::from_le_bytes([header[4],header[5]]);let expected=match folder{"32"=>0x14c,"64"=>0x8664,"arm64"=>0xaa64,_=>return false};&header[..4]==b"PE\0\0"&&machine==expected&&u16::from_le_bytes([header[22],header[23]])&0x2000!=0}
fn binary(plan:&Plan,layout:&Layout)->Result<(),String>{
 for &(folder,_)in &plan.targets{let source=plan.stage.join(folder).join(&plan.library);let bytes=optional(&source)?.ok_or("编译成功但输出 DLL 缺失")?;if !valid_dll(&bytes,folder){return Err("编译输出不是目标架构的 Windows DLL".into());}
  let directory=layout.data.join("Engine/Mods").join(folder);crate::storage::safe(&directory)?;fs::create_dir_all(&directory).map_err(|_|"mod DLL 目录创建失败")?;
  for name in ["libc++.whl","libunwind.whl","windhawk-mod-shim.dll"]{let source=plan.stage.join(folder).join(name);let target=directory.join(name);let bytes=optional(&source)?.ok_or("编译运行库暂存缺失")?;if let Some(old)=optional(&target)?{if old!=bytes{return Err("Windhawk 运行库与编译器版本不同，请先更新 Windhawk".into());}}else{put(&target,Some(&bytes))?;}}
  let target=directory.join(&plan.library);if target.exists(){return Err("mod DLL 文件名冲突".into());}put(&target,Some(&bytes))?;
 }Ok(())
}
fn journal_save(host:&Namespace,journal:&Journal)->Result<(),String>{host.write("backups",&format!("{}.json",journal.token),&serde_json::to_vec(journal).map_err(|_|"mod 备份编码失败")?)}
fn clear_active(host:&Namespace)->Result<(),String>{let path=host.file("config",ACTIVE)?;match fs::remove_file(path){Ok(())=>Ok(()),Err(e)if e.kind()==std::io::ErrorKind::NotFound=>Ok(()),Err(_)=>Err("mod 导入记录清理失败".into())}}
fn install_with(layout:&Layout,host:&Namespace,source:&str,expected:Option<&str>,run:impl FnMut(&Plan)->Result<(),String>)->Result<Value,String>{install_with_settings(layout,host,source,expected,&BTreeMap::new(),run)}
fn install_with_settings(layout:&Layout,host:&Namespace,source:&str,expected:Option<&str>,presets:&BTreeMap<String,Value>,mut run:impl FnMut(&Plan)->Result<(),String>)->Result<Value,String>{
 let _lock=EDIT.lock().map_err(|_|"mod 导入锁不可用")?;
 if host.read("config",ACTIVE)?.is_some(){return Err("存在未完成的 mod 导入，请先恢复备份".into());}
 let meta=windhawk_source::metadata(source)?;let before=snapshot(layout,&meta.id)?;if expected.is_some_and(|expected|hash(&before).as_deref()!=Ok(expected)){return Err("mod 已在预览后发生变化，请重新预览".into());}
 crate::windhawk_collection::validate_settings(presets)?;let token=token();let mut plan=plan(layout,host,source,&token)?;crate::windhawk_collection::validate_settings(&plan.settings)?;plan.presets=presets.clone();run(&plan)?;
 if snapshot(layout,&meta.id)?!=before{return Err("编译期间 mod 已变化，请重新预览".into());}
 let after=desired(&before,&plan,layout.portable)?;binary(&plan,layout)?;
 let journal=Journal{schema:1,token:token.clone(),id:meta.id.clone(),layout:layout.clone(),before,after,rolled_back:false};journal_save(host,&journal)?;host.write("config",ACTIVE,&serde_json::to_vec(&journal).map_err(|_|"mod 事务编码失败")?)?;
 if let Err(error)=apply_snapshot(layout,&meta.id,&journal.after){return match apply_snapshot(layout,&meta.id,&journal.before){Ok(())=>{clear_active(host)?;Err(format!("{error}；旧配置已恢复"))},Err(_)=>Err(format!("{error}；恢复待完成，请保留备份并点击恢复"))};}
 clear_active(host)?;host.write("config","windhawk-last-import.json",&serde_json::to_vec(&token).map_err(|_|"mod 记录编码失败")?)?;
 Ok(json!({"id":meta.id,"token":token,"disabled":true,"version":meta.version,"fingerprint":hash(&journal.after)?}))
}
pub(super)fn status(host:&Namespace)->Result<Value,String>{
 let pending=host.read("config",ACTIVE)?.is_some();let mut last=Value::Null;
 if let Some(bytes)=host.read("config","windhawk-last-import.json")?{let token:String=serde_json::from_slice(&bytes).map_err(|_|"mod 导入记录损坏")?;if !token.starts_with("wh-")||token.len()>100||!token.bytes().all(|b|b.is_ascii_alphanumeric()||b==b'-'){return Err("mod 备份标识无效".into());}if let Some(bytes)=host.read("backups",&format!("{token}.json"))?{let journal:Journal=serde_json::from_slice(&bytes).map_err(|_|"mod 导入备份损坏")?;last=json!({"token":token,"id":journal.id,"rolledBack":journal.rolled_back});}}
 Ok(json!({"pendingImport":pending,"lastImport":last}))
}
pub(super)fn preview(layout:&Layout,source:&str)->Result<String,String>{hash(&snapshot(layout,&windhawk_source::metadata(source)?.id)?)}
pub(super)fn install(layout:&Layout,host:&Namespace,source:&str,expected:&str)->Result<Value,String>{install_with(layout,host,source,Some(expected),compile)}
pub(super)fn install_preset(layout:&Layout,host:&Namespace,source:&str,expected:&str,settings:&BTreeMap<String,Value>)->Result<Value,String>{install_with_settings(layout,host,source,Some(expected),settings,compile)}
pub(super)fn rollback(layout:&Layout,host:&Namespace,requested:Option<&str>)->Result<Value,String>{
 let _lock=EDIT.lock().map_err(|_|"mod 回滚锁不可用")?;
 let active=host.read("config",ACTIVE)?;let bytes=if let Some(bytes)=&active{bytes.clone()}else{let token=if let Some(token)=requested{token.into()}else{serde_json::from_slice::<String>(&host.read("config","windhawk-last-import.json")?.ok_or("尚无可恢复的导入")?).map_err(|_|"mod 导入记录损坏")?};if !token.starts_with("wh-")||token.len()>100||!token.bytes().all(|b|b.is_ascii_alphanumeric()||b==b'-'){return Err("mod 备份标识无效".into());}host.read("backups",&format!("{token}.json"))?.ok_or("mod 导入备份缺失")?};
 let mut journal:Journal=serde_json::from_slice(&bytes).map_err(|_|"mod 导入备份损坏")?;
 if journal.schema!=1||!windhawk_source::valid_id(&journal.id)||journal.layout.install!=layout.install||journal.layout.data!=layout.data||journal.layout.portable!=layout.portable||journal.layout.registry!=layout.registry{return Err("mod 备份与当前 Windhawk 安装不符".into());}
 let current=snapshot(layout,&journal.id)?;let consistent=if active.is_some(){(current.source==journal.before.source||current.source==journal.after.source)&&(current.ini==journal.before.ini||current.ini==journal.after.ini)&&(current.registry==journal.before.registry||current.registry==journal.after.registry)}else{current==journal.after||current==journal.before};if !consistent{return Err("mod 在导入后已有新修改，恢复前请先保存当前内容".into());}
 apply_snapshot(layout,&journal.id,&journal.before)?;journal.rolled_back=true;journal_save(host,&journal)?;clear_active(host)?;Ok(json!({"id":journal.id,"restored":true}))
}
#[cfg(test)]mod tests{
 use super::*;
 fn fixture()->(Layout,Namespace){let root=std::env::temp_dir().join(format!("fish-wh-import-{}",token()));fs::create_dir_all(root.join("windhawk/data/Engine/Mods")).unwrap();fs::create_dir_all(root.join("windhawk/data/ModsSource")).unwrap();fs::create_dir_all(root.join("windhawk/compiler/bin")).unwrap();fs::create_dir_all(root.join("windhawk/engine/64")).unwrap();fs::write(root.join("windhawk/windhawk.exe"),b"fixture").unwrap();fs::write(root.join("windhawk/compiler/bin/clang++.exe"),b"fixture").unwrap();fs::write(root.join("windhawk/engine/64/windhawk.lib"),b"fixture").unwrap();fs::write(root.join("windhawk/windhawk.ini"),b"[Storage]\nAppDataPath=data\nPortable=1\nCompilerPath=compiler\nEnginePath=engine\n").unwrap();
  let host_root=root.join(".fish-dock/host");for name in ["tmp","config","backups"]{fs::create_dir_all(host_root.join(name)).unwrap();}let host=Namespace{root:host_root,identity:crate::storage_context::Identity{plugin_id:"host".into(),directory:"host".into(),plugin_version:"1.0.2".into(),data_schema:1}};(layout(&root.join("windhawk")).unwrap(),host)}
 fn source()->String{"// ==WindhawkMod==\n// @id sample\n// @name 示例\n// @version 1.2\n// @architecture x86-64\n// @include explorer.exe\n// ==/WindhawkMod==\nBOOL Wh_ModInit(){return TRUE;}".into()}
 #[test]fn compile_failure_keeps_previous_mod_and_source(){let(layout,host)=fixture();let config=layout.data.join("Engine/Mods/sample.ini");let previous=b"[Mod]\nLibraryFileName=old.dll\nVersion=1.0\nDisabled=0\n";fs::write(&config,previous).unwrap();fs::write(layout.data.join("ModsSource/sample.wh.cpp"),b"old source").unwrap();let mut ran=false;let got=install_with(&layout,&host,&source(),None,|_|{ran=true;Err("synthetic compiler failed".into())});assert!(ran);assert!(got.unwrap_err().contains("synthetic compiler"));assert_eq!(fs::read(&config).unwrap(),previous);assert_eq!(fs::read(layout.data.join("ModsSource/sample.wh.cpp")).unwrap(),b"old source");fs::remove_dir_all(layout.install.parent().unwrap()).unwrap();}
 fn compiled(plan:&Plan)->Result<(),String>{for &(folder,_)in &plan.targets{fs::create_dir_all(plan.stage.join(folder)).unwrap();let mut bytes=vec![0;512];bytes[..2].copy_from_slice(b"MZ");bytes[60..64].copy_from_slice(&128u32.to_le_bytes());bytes[128..132].copy_from_slice(b"PE\0\0");bytes[132..134].copy_from_slice(&(if folder=="64"{0x8664u16}else{0x14cu16}).to_le_bytes());bytes[150..152].copy_from_slice(&0x2000u16.to_le_bytes());fs::write(plan.stage.join(folder).join(&plan.library),bytes).unwrap();for name in ["libc++.whl","libunwind.whl","windhawk-mod-shim.dll"]{fs::write(plan.stage.join(folder).join(name),b"synthetic-runtime").unwrap();}}Ok(())}
 #[test]fn successful_update_stays_disabled_and_rollback_restores_exact_bytes(){let(layout,host)=fixture();let previous=b"; keep exact original\n[Mod]\nLibraryFileName=old.dll\nVersion=1.0\nDisabled=0\nIncludeCustom=custom.exe\n[Settings]\ncount=99\n";fs::write(layout.data.join("Engine/Mods/sample.ini"),previous).unwrap();fs::write(layout.data.join("ModsSource/sample.wh.cpp"),b"old source").unwrap();let before=preview(&layout,&source()).unwrap();let source=source()+"\n// ==WindhawkModSettings==\n/*\n- count: 4\n- color: red\n*/\n// ==/WindhawkModSettings==";let got=install_with(&layout,&host,&source,Some(&before),compiled).unwrap();let config=decode(&fs::read(layout.data.join("Engine/Mods/sample.ini")).unwrap()).unwrap();assert_eq!(ini(&config,"Mod").get("Disabled").unwrap(),"1");assert_eq!(ini(&config,"Mod").get("IncludeCustom").unwrap(),"custom.exe");assert_eq!(ini(&config,"Settings").get("count").unwrap(),"99");assert_eq!(ini(&config,"Settings").get("color").unwrap(),"red");rollback(&layout,&host,got["token"].as_str()).unwrap();assert_eq!(fs::read(layout.data.join("Engine/Mods/sample.ini")).unwrap(),previous);assert_eq!(fs::read(layout.data.join("ModsSource/sample.wh.cpp")).unwrap(),b"old source");fs::remove_dir_all(layout.install.parent().unwrap()).unwrap();}
 #[test]fn preview_conflict_never_runs_compiler(){let(layout,host)=fixture();let fingerprint=preview(&layout,&source()).unwrap();fs::write(layout.data.join("ModsSource/sample.wh.cpp"),b"external change").unwrap();let got=install_with(&layout,&host,&source(),Some(&fingerprint),|_|panic!("stale preview compiled"));assert!(got.unwrap_err().contains("发生变化"));fs::remove_dir_all(layout.install.parent().unwrap()).unwrap();}
 #[test]fn explicit_collection_presets_are_applied_and_original_is_restorable(){let(layout,host)=fixture();let original=b"[Mod]\nLibraryFileName=old.dll\nDisabled=1\n[Settings]\ncount=99\n";fs::write(layout.data.join("Engine/Mods/sample.ini"),original).unwrap();let presets=BTreeMap::from([("count".into(),json!(7))]);let result=install_with_settings(&layout,&host,&source(),None,&presets,compiled).unwrap();let config=decode(&fs::read(layout.data.join("Engine/Mods/sample.ini")).unwrap()).unwrap();assert_eq!(ini(&config,"Settings").get("count").unwrap(),"7");rollback(&layout,&host,result["token"].as_str()).unwrap();assert_eq!(fs::read(layout.data.join("Engine/Mods/sample.ini")).unwrap(),original);fs::remove_dir_all(layout.install.parent().unwrap()).unwrap();}
 #[test]fn interrupted_import_preserves_later_external_edits(){let(layout,host)=fixture();let before=snapshot(&layout,"sample").unwrap();let mut after=before.clone();after.source=Some(b"imported source".to_vec());let journal=Journal{schema:1,token:token(),id:"sample".into(),layout:layout.clone(),before,after,rolled_back:false};host.write("config",ACTIVE,&serde_json::to_vec(&journal).unwrap()).unwrap();fs::write(layout.data.join("ModsSource/sample.wh.cpp"),b"later user edit").unwrap();assert!(rollback(&layout,&host,None).is_err(),"interrupted recovery overwrote a later user edit");assert_eq!(fs::read(layout.data.join("ModsSource/sample.wh.cpp")).unwrap(),b"later user edit");fs::remove_dir_all(layout.install.parent().unwrap()).unwrap();}
 #[test]fn mz_header_without_pe_is_never_registered(){let(layout,host)=fixture();let got=install_with(&layout,&host,&source(),None,|plan|{compiled(plan)?;fs::write(plan.stage.join("64").join(&plan.library),[b'M',b'Z'].into_iter().chain([0;510]).collect::<Vec<_>>()).unwrap();Ok(())});assert!(got.is_err(),"MZ text was accepted as a compiled DLL");assert!(!layout.data.join("Engine/Mods/sample.ini").exists());fs::remove_dir_all(layout.install.parent().unwrap()).unwrap();}
}
