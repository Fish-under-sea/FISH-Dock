//! 内置便携引擎：程序资源只读，模组和设置存入当前数据根。
use super::*;
const FILES:&[&str]=&["windhawk.exe","windhawk-x64-helper.exe","Engine/32/windhawk.dll","Engine/32/windhawk.lib","Engine/64/windhawk.dll","Engine/64/windhawk.lib","Compiler/bin/clang++.exe","Compiler/include/windhawk_api.h"];
static PREPARE:std::sync::Mutex<()>=std::sync::Mutex::new(());
pub(super)fn prepare(resources:&Path,root:&Path)->Result<Option<Layout>,String>{
 let _lock=PREPARE.lock().map_err(|_|"内置引擎准备锁不可用")?;
 if !resources.exists(){return Ok(None);}
 crate::storage::safe(resources)?;crate::storage::safe(root)?;
 for file in FILES{let target=resources.join(file);crate::storage::safe(&target)?;if !target.is_file(){return Err(format!("内置 Windhawk 资源缺失：{file}，请修复安装"));}}
 fs::create_dir_all(root.join("data")).map_err(|_|"内置引擎数据目录创建失败")?;
 for file in ["windhawk.exe","windhawk-x64-helper.exe"]{let source=fs::read(resources.join(file)).map_err(|_|"内置引擎读取失败")?;let target=root.join(file);crate::storage::safe(&target)?;if crate::identity_store::read_optional(&target)?.as_deref()!=Some(source.as_slice()){fs::write(&target,&source).map_err(|_|"内置引擎更新失败，请停止模组后重试")?;}}
 let engine=resources.join("Engine");let compiler=resources.join("Compiler");
 for path in [&engine,&compiler]{if path.to_string_lossy().contains(['%','\r','\n']){return Err("内置引擎资源路径含不支持的字符".into());}}
 let text=format!("[Storage]\r\nEnginePath={}\r\nCompilerPath={}\r\nUIPath=unused\r\nAppDataPath=data\r\nPortable=1\r\n",engine.display(),compiler.display());
 let bytes:[Vec<u8>;2]=[vec![255,254],text.encode_utf16().flat_map(u16::to_le_bytes).collect()];let bytes=bytes.concat();let file=root.join("windhawk.ini");
 if crate::identity_store::read_optional(&file)?.as_deref()!=Some(bytes.as_slice()){fs::write(&file,bytes).map_err(|_|"内置引擎配置写入失败")?;}
 layout(root).map(Some)
}
fn resources()->Result<PathBuf,String>{
 #[cfg(feature="installed")]{return crate::runtime::resources_directory().map(|p|p.join("windhawk"));}
 #[cfg(not(feature="installed"))]{Ok(std::env::var_os("DSH_FISH_RESOURCES").map(PathBuf::from).unwrap_or_else(||PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../resources")).join("windhawk"))}
}
pub(super)fn configured()->Result<Option<Layout>,String>{let context=crate::storage_context::current().ok_or("存储上下文未就绪")?;prepare(&resources()?,&context.host.path().join("config/windhawk"))}
pub(super)fn is_bundled(layout:&Layout)->bool{crate::storage_context::current().is_some_and(|c|layout.install==c.host.path().join("config/windhawk"))}
struct Daemon{child:std::process::Child,_owner:crate::plugin_job::Owner,install:PathBuf}
static DAEMON:std::sync::Mutex<Option<Daemon>>=std::sync::Mutex::new(None);
pub(super)fn ensure_running(layout:&Layout)->Result<(),String>{
 if !is_bundled(layout){return Ok(());}
 let mut running=DAEMON.lock().map_err(|_|"内置引擎进程锁不可用")?;
 if let Some(daemon)=running.as_mut(){if daemon.child.try_wait().map_err(|_|"内置引擎状态读取失败")?.is_none(){if daemon.install==layout.install{return Ok(());}return Err("切换数据目录前请先退出当前模组引擎".into());}}
 running.take();let mut command=std::process::Command::new(layout.install.join("windhawk.exe"));command.arg("-tray-only").current_dir(&layout.install).stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null());
 #[cfg(windows)]{use std::os::windows::process::CommandExt;command.creation_flags(0x08000000);}
 let mut child=command.spawn().map_err(|_|"内置模组引擎启动失败")?;let owner=match crate::plugin_job::Owner::attach(&child){Ok(owner)=>owner,Err(error)=>{let _=child.kill();let _=child.wait();return Err(error);}};
 std::thread::sleep(std::time::Duration::from_millis(300));if child.try_wait().map_err(|_|"内置引擎状态读取失败")?.is_some(){return Err("内置模组引擎已退出，请检查是否有其他 Windhawk 实例正在运行".into());}
 *running=Some(Daemon{child,_owner:owner,install:layout.install.clone()});Ok(())
}
pub(super)fn running()->bool{DAEMON.lock().ok().and_then(|mut d|d.as_mut().and_then(|d|d.child.try_wait().ok().map(|s|s.is_none()))).unwrap_or(false)}
#[cfg(test)]mod tests{
 use super::*;
 #[test]fn bundled_engine_is_prepared_without_an_external_install_and_preserves_mods(){
  let root=std::env::temp_dir().join(format!("fish-wh-bundle-{}-{}",std::process::id(),crate::now_ms()));
  let resources=root.join("resources/windhawk");let data=root.join("host/config/windhawk");
  for file in FILES{let p=resources.join(file);fs::create_dir_all(p.parent().unwrap()).unwrap();fs::write(p,b"fixture").unwrap();}
  let prepared=prepare(&resources,&data).unwrap().expect("内置资源应在首次打开时就绪，无需外部安装");
  assert!(prepared.portable);assert_eq!(prepared.install,data);assert_eq!(prepared.data,data.join("data"));
  let values=ini(&decode(&fs::read(data.join("windhawk.ini")).unwrap()).unwrap(),"Storage");
  assert_eq!(PathBuf::from(&values["EnginePath"]),resources.join("Engine"));assert_eq!(PathBuf::from(&values["CompilerPath"]),resources.join("Compiler"));
  fs::create_dir_all(prepared.data.join("ModsSource")).unwrap();fs::write(prepared.data.join("ModsSource/keep.wh.cpp"),b"original mod").unwrap();
  prepare(&resources,&data).unwrap();assert_eq!(fs::read(prepared.data.join("ModsSource/keep.wh.cpp")).unwrap(),b"original mod");
  fs::remove_dir_all(root).unwrap();
 }
 #[test]fn missing_bundle_remains_missing_and_incomplete_bundle_is_reported(){
  let root=std::env::temp_dir().join(format!("fish-wh-incomplete-{}-{}",std::process::id(),crate::now_ms()));
  assert!(prepare(&root.join("missing"),&root.join("data")).unwrap().is_none());
  fs::create_dir_all(root.join("broken")).unwrap();fs::write(root.join("broken/windhawk.exe"),b"fixture").unwrap();
  assert!(prepare(&root.join("broken"),&root.join("data")).is_err(),"残缺引擎不得伪装成已就绪");fs::remove_dir_all(root).unwrap();
 }
}
