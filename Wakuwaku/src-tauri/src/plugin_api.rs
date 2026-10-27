//! 仅设置窗口可调用的外部插件管理入口；网络与进程操作在阻塞工作线程执行。
use std::path::PathBuf;
use serde_json::{json,Value};
use tauri::{WebviewWindow,Manager as _};
use crate::{plugin_install::Manager,storage_context::Context};
fn trusted(window:&WebviewWindow)->Result<(),String>{
    let url=window.url().map_err(|_|"设置窗口来源读取失败")?;
    if !local_source(window.label(),&url){return Err("插件管理仅允许本机设置窗口调用".into());}Ok(())
}
fn local_source(label:&str,url:&tauri::Url)->bool{
    label=="island"&&match url.scheme(){
        "tauri"=>url.host_str()==Some("localhost"),
        "http"=>match url.host_str(){Some("tauri.localhost")=>url.port().is_none(),Some("localhost"|"127.0.0.1")=>cfg!(debug_assertions),_=>false},
        _=>false,
    }
}
#[derive(Default,serde::Serialize,serde::Deserialize)]
#[serde(deny_unknown_fields,rename_all="camelCase")]
struct Dependencies {node:Option<PathBuf>}
fn dependencies(context:&Context)->Result<Dependencies,String>{
    context.host.read("config","plugin-dependencies.json")?.map(|bytes|serde_json::from_slice(&bytes).map_err(|_|"插件依赖设置损坏".into())).unwrap_or_else(||Ok(Dependencies::default()))
}
fn discovery(context:&Context)->Result<crate::plugin_dependencies::Discovery,String>{Ok(crate::plugin_dependencies::discover(dependencies(context)?.node))}
pub fn node(context:&Context)->Result<Option<PathBuf>,String>{Ok(discovery(context)?.path)}
fn runtime_config(context:&Context,path:Option<PathBuf>)->Result<(),String>{
    if let Some(path)=&path {let version=crate::plugin_dependencies::probe(path)?;let selected=crate::plugin_dependencies::select(false,vec![("saved",path.clone())], |_|Ok(version.clone()));if selected.path.is_none(){return Err(selected.candidates.first().and_then(|c|c.error.clone()).unwrap_or_else(||"Node 版本不兼容".into()));}}
    context.host.write("config","plugin-dependencies.json",&serde_json::to_vec(&Dependencies{node:path}).map_err(|_|"插件依赖编码失败")?)
}
fn view(context:&Context)->Result<Value,String>{
    let star=crate::github_access::view(context)?;
    let manager=Manager{context};
    let runtime=discovery(context)?;
    let mut runner=crate::plugin_runtime::CURRENT.lock().map_err(|_|"插件运行锁不可用")?;
    Ok(json!({"installed":runner.view(&manager)?,"node":runtime.path,"runtime":runtime,"root":context.root,"star":star}))
}
#[tauri::command]
pub async fn extension_view(window:WebviewWindow)->Result<Value,String>{
    trusted(&window)?;
    tauri::async_runtime::spawn_blocking(||{view(crate::storage_context::current().ok_or("存储上下文未就绪")?)}).await.map_err(|_|"插件列表任务意外结束")?
}
#[tauri::command]
pub async fn extension_catalog(window:WebviewWindow)->Result<Value,String>{
    trusted(&window)?;
    tauri::async_runtime::spawn_blocking(||{crate::plugin_store::refresh(crate::storage_context::current().ok_or("存储上下文未就绪")?)}).await.map_err(|_|"插件商店任务意外结束")?
}
#[tauri::command]
pub async fn extension_manage(window:WebviewWindow,action:String,id:Option<String>,enabled:Option<bool>,node_path:Option<String>,confirm:bool)->Result<Value,String>{
    trusted(&window)?;
    if !confirm{return Err("请确认插件安装或管理操作".into());}
    tauri::async_runtime::spawn_blocking(move||{
        let context=crate::storage_context::current().ok_or("存储上下文未就绪")?;
        if action=="runtime"{runtime_config(context,node_path.filter(|p|!p.trim().is_empty()).map(PathBuf::from))?;return view(context);}
        let id=id.ok_or("插件操作缺少 ID")?;
        // 下载不占用实例锁，网络失败不阻塞已安装插件的停用。
        if action=="enabled"&&enabled==Some(true){crate::github_access::require_plugin(context,&id)?;}
        let package=if action=="install"{crate::github_access::require(context)?;Some(crate::plugin_store::obtain(context,&id)?)}else{None};
        let manager=Manager{context};let node=node(context)?;
        let mut runner=crate::plugin_runtime::CURRENT.lock().map_err(|_|"插件运行锁不可用")?;
        match action.as_str(){
            "install"=>{let (package,digest)=package.ok_or("插件包尚未就绪")?;runner.install(&manager,package,&digest,true,node.as_deref())?;},
            "enabled"=>runner.set_enabled(&manager,&id,enabled.ok_or("插件启停操作缺少目标状态")?,node.as_deref())?,
            "uninstall"=>runner.uninstall(&manager,&id)?,
            _=>return Err("插件管理操作无效".into()),
        }
        let _=runner.view(&manager)?;drop(runner);
        view(context)
    }).await.map_err(|_|"插件管理任务意外结束")?
}
#[tauri::command]
pub async fn extension_auth(window:WebviewWindow,action:String)->Result<Value,String>{
    trusted(&window)?;
    let sh = crate::shared(window.app_handle());
    if action=="release" { sh.auth_focus(0); return Ok(json!({"released":true})); }
    if matches!(action.as_str(), "cancel" | "logout") { sh.auth_focus(0); }
    tauri::async_runtime::spawn_blocking(move||{
        let context=crate::storage_context::current().ok_or("存储上下文未就绪")?;
        let result = match action.as_str(){"login"=>crate::github_access::begin(context),"check"=>crate::github_access::check(context),"logout"=>crate::github_access::logout(context),"cancel"=>crate::github_access::cancel(context),_=>Err("GitHub 登录操作无效".into())};
        if action == "login" && result.is_ok() {
            let until = result.as_ref().ok().and_then(|v| v["pending"]["expiresAt"].as_u64()).unwrap_or(0);
            sh.auth_focus(until);
        } else if action == "logout" || action == "cancel" || action == "check" {
            // Star 尚未点亮也可能已经完成设备登录，此时同样释放临时状态。
            if action == "logout" || action == "cancel" || crate::github_access::view(context).is_ok_and(|v| v["pending"].is_null()) { sh.auth_focus(0); }
        }
        result
    }).await.map_err(|_|"GitHub 登录任务意外结束")?
}
#[tauri::command]
pub async fn extension_call(window:WebviewWindow,id:String,method:String,params:Value)->Result<Value,String>{
    trusted(&window)?;
    tauri::async_runtime::spawn_blocking(move||{let context=crate::storage_context::current().ok_or("存储上下文未就绪")?;crate::github_access::require_plugin(context,&id)?;crate::plugin_runtime::CURRENT.lock().map_err(|_|"插件运行锁不可用")?.call(&id,&method,params)}).await.map_err(|_|"插件请求任务意外结束")?
}
#[tauri::command]
pub async fn extension_page(window:WebviewWindow,key:String)->Result<Value,String>{trusted(&window)?;tauri::async_runtime::spawn_blocking(move||crate::plugin_page::open(&key)).await.map_err(|_|"插件页面任务意外结束")?}
#[tauri::command]
pub fn extension_page_close(window:WebviewWindow,ticket:String)->Result<(),String>{trusted(&window)?;crate::plugin_page::close(&ticket);Ok(())}
#[tauri::command]
pub async fn extension_page_call(window:WebviewWindow,ticket:String,method:String,params:Value)->Result<Value,String>{trusted(&window)?;tauri::async_runtime::spawn_blocking(move||crate::plugin_page::call(&ticket,&method,params)).await.map_err(|_|"插件页面请求意外结束")?}

pub fn resume(){
    std::thread::spawn(||{
        let Some(context)=crate::storage_context::current()else{return};
        let result:Result<(),String>=(||{let node=node(context)?;let mut runner=crate::plugin_runtime::CURRENT.lock().map_err(|_|"插件运行锁不可用")?;crate::plugin_development::prepare(context,&mut runner,node.as_deref())?;runner.resume(&Manager{context},node.as_deref())})();
        if let Err(error)=result{eprintln!("FISH Dock: 插件恢复失败：{error}");}
    });
}
pub fn shutdown(){if let Ok(mut runner)=crate::plugin_runtime::CURRENT.lock(){runner.stop_all();}}
#[cfg(test)]mod tests{
    use super::*;
    #[test]fn management_is_bound_to_local_settings_window(){
        for (label,url) in [("plugin-fish-ofm","http://tauri.localhost/"),("island","https://example.test/"),("island","http://tauri.localhost:5000/"),("island","http://127.0.0.1.example.test/")] {assert!(!local_source(label,&tauri::Url::parse(url).unwrap()));}
        assert!(local_source("island",&tauri::Url::parse("http://tauri.localhost/").unwrap()));
        assert!(local_source("island",&tauri::Url::parse("tauri://localhost/").unwrap()));
    }
}
