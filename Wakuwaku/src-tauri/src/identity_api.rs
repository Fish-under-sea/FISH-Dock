//! 仅本地设置窗口可调用的身份文件操作；不向 OFM iframe 暴露邮件凭据。
use std::{path::PathBuf,sync::{Arc,Mutex,atomic::Ordering}};
use serde_json::{json,Value};
use sha2::{Digest,Sha256};
use tauri::{Emitter,WebviewWindow,AppHandle};

static OPERATION:Mutex<()>=Mutex::new(());
struct Pending{path:PathBuf,hash:String,ticket:String,at:u64}
static PENDING:Mutex<Option<Pending>>=Mutex::new(None);
fn hash(bytes:&[u8])->String{format!("{:x}",Sha256::digest(bytes))}
fn trusted(window:&WebviewWindow)->Result<String,String>{
    let url=window.url().map_err(|_|"无法确认设置窗口来源")?;
    let local=match url.scheme(){
        "http"=>match url.host_str(){Some("tauri.localhost")=>url.port().is_none(),Some("localhost"|"127.0.0.1")=>cfg!(debug_assertions),_=>false},
        "tauri"=>url.host_str()==Some("localhost"),
        _=>false,
    };
    if window.label()!="island" || !local{return Err("身份操作仅允许本机设置窗口调用".into())}
    Ok(url.origin().ascii_serialization())
}
fn owner(window:&WebviewWindow)->isize{
    #[cfg(windows)]{window.hwnd().map(|h|h.0 as isize).unwrap_or(0)}
    #[cfg(not(windows))]{let _=window;0}
}
struct Paused{sh:Arc<crate::Shared>}
impl Paused{
    fn new(sh:Arc<crate::Shared>,_origin:String)->Result<Self,String>{
        if sh.identity_busy.swap(true,Ordering::SeqCst){return Err("另一个身份操作尚未完成".into())}
        sh.identity_generation.fetch_add(1,Ordering::SeqCst);
        crate::mail::stop_all(&sh);
        crate::plugin_api::shutdown();
        Ok(Self{sh})
    }
    fn activate(&self)->Result<(),String>{
        let snapshot=crate::mail_identity::load(&self.sh.identity_dir)?;
        crate::mail::accept_snapshot(&self.sh,snapshot);
        *self.sh.identity_error.lock().unwrap()=None;
        Ok(())
    }
}
impl Drop for Paused{
    fn drop(&mut self){
        self.sh.identity_busy.store(false,Ordering::SeqCst);
        if self.sh.identity_error.lock().unwrap().is_none(){
            crate::plugin_api::resume();
            crate::mail::sync(&self.sh);
        }
        self.sh.redraw();
        let _=self.sh.app.emit_to("island","identity:changed",json!({"generation":self.sh.identity_generation.load(Ordering::SeqCst)}));
    }
}
#[tauri::command]
pub async fn identity_view(app:AppHandle,window:WebviewWindow)->Result<Value,String>{
    trusted(&window)?;
    let sh=crate::shared(&app);
    let directory=crate::identity_dialog::export_directory(sh.setting("identityExportDirectory").as_str()).map(|p|p.to_string_lossy().into_owned());
    Ok(json!({"root":crate::storage_context::current().map(|c| &c.root).unwrap_or(&sh.identity_dir),"directory":directory.as_ref().ok(),"directoryError":directory.err(),"error":sh.identity_error.lock().unwrap().clone(),"missingMailCredentials":sh.mail_missing.lock().unwrap().len(),"busy":sh.identity_busy.load(Ordering::SeqCst)}))
}
#[tauri::command]
pub async fn identity_export(app:AppHandle,window:WebviewWindow,directory:String,confirm:bool)->Result<Value,String>{
    let origin=trusted(&window)?;
    if !confirm{return Err("导出包含明文凭据，需要明确确认".into())}
    let sh=crate::shared(&app);let owner=owner(&window);
    tauri::async_runtime::spawn_blocking(move||{
        let _operation=OPERATION.try_lock().map_err(|_|"另一个身份操作尚未完成")?;
        if sh.identity_error.lock().unwrap().is_some(){return Err("身份迁移尚未完成，请先解决设置页中的错误".into())}
        let directory=crate::identity_dialog::export_directory(Some(&directory))?;
        let name=format!("FISH-Dock-config-{}.zip",crate::now_ms());
        let Some(path)=crate::identity_dialog::save(owner,&directory,&name)? else{return Ok(json!({"cancelled":true}))};
        let _editing=crate::mail::EDITING.lock().map_err(|_|"邮件身份操作不可用")?;
        let _paused=Paused::new(sh.clone(),origin)?;
        let _guard=crate::identity::operation_guard(&sh.identity_dir)?;
        let context=crate::storage_context::current().ok_or("配置仓未就绪")?;
        let (bytes,opaque)=crate::config_backup::export_with_report(&context.root)?;
        crate::identity_transfer::save(Some(&path),&bytes)?;
        sh.change(json!({"identityExportDirectory":directory.to_string_lossy()}));
        Ok(json!({"saved":true,"path":path,"bytes":bytes.len(),"sha256":hash(&bytes),"missingCredentials":opaque}))
    }).await.map_err(|_|"身份导出任务意外结束")?
}
#[tauri::command]
pub async fn identity_preview(app:AppHandle,window:WebviewWindow)->Result<Value,String>{
    trusted(&window)?;let owner=owner(&window);let sh=crate::shared(&app);
    tauri::async_runtime::spawn_blocking(move||{
        let _operation=OPERATION.try_lock().map_err(|_|"另一个身份操作尚未完成")?;
        *PENDING.lock().unwrap()=None;
        let Some(path)=crate::identity_dialog::open(owner)? else{return Ok(json!({"cancelled":true}))};
        let bytes=crate::config_backup::read_zip(&path)?;
        let context=crate::storage_context::current().ok_or("配置仓未就绪")?;
        let configuration=if crate::config_backup::is_config(&bytes)? {Some(crate::config_backup::inspect(&context.root,&bytes)?)}else{None};
        let legacy=if configuration.is_none(){Some(crate::identity_archive::inspect_for(&sh.identity_dir,&bytes)?)}else{None};
        let digest=hash(&bytes);let at=crate::now_ms();
        let ticket=hash(format!("{digest}-{at}-{}",std::process::id()).as_bytes());
        let view=json!({"ticket":ticket,"format":legacy.as_ref().map(|p|p.format.as_str()).unwrap_or("fish-dock/config-backup"),"ofmAccounts":legacy.as_ref().map_or(0,|p|p.ofm_accounts),"mailAccounts":legacy.as_ref().and_then(|p|p.mail_accounts),"missingCredentials":legacy.as_ref().map_or(0,|p|p.missing),"files":configuration.as_ref().map(|p|p.files).unwrap_or_else(||legacy.as_ref().map_or(0,|p|p.files.len())),"configuration":configuration,"name":path.file_name().unwrap_or_default().to_string_lossy()});
        *PENDING.lock().unwrap()=Some(Pending{path,hash:digest,ticket,at});
        Ok(view)
    }).await.map_err(|_|"身份预览任务意外结束")?
}
#[tauri::command]
pub async fn identity_import(app:AppHandle,window:WebviewWindow,ticket:String,confirm:bool)->Result<Value,String>{
    let origin=trusted(&window)?;
    if !confirm{return Err("替换身份需要明确确认".into())}
    let sh=crate::shared(&app);
    tauri::async_runtime::spawn_blocking(move||{
        let _operation=OPERATION.try_lock().map_err(|_|"另一个身份操作尚未完成")?;
        let pending=PENDING.lock().unwrap().take().ok_or("请重新选择并预览身份 ZIP")?;
        if pending.ticket!=ticket || crate::now_ms().saturating_sub(pending.at)>5*60*1000{return Err("预览已过期，请重新选择 ZIP".into())}
        let bytes=crate::config_backup::read_zip(&pending.path)?;
        if hash(&bytes)!=pending.hash{return Err("预览后 ZIP 发生变化，请重新选择".into())}
        let configuration=crate::config_backup::is_config(&bytes)?;
        let preview=if configuration{None}else{Some(crate::identity_archive::inspect_for(&sh.identity_dir,&bytes)?)};
        let _editing=crate::mail::EDITING.lock().map_err(|_|"邮件身份操作不可用")?;
        let paused=Paused::new(sh.clone(),origin)?;
        let result=if configuration {
            let context=crate::storage_context::current().ok_or("配置仓未就绪")?;
            let _guard=crate::identity::operation_guard(&sh.identity_dir)?;
            crate::config_backup::restore(&context.root,&bytes,|| { paused.activate()?; let saved=crate::data::load(&sh.dir); *sh.settings.lock().map_err(|_|"设置锁不可用")?=saved; sh.change(json!({})); Ok(()) })
        }else{crate::identity_transfer::commit(&sh.identity_dir,preview.as_ref().ok_or("旧备份预览缺失")?.changes()?,||paused.activate())};
        match result{
            Ok(backup)=>Ok(json!({"imported":true,"backup":backup,"missingCredentials":preview.as_ref().map_or(0,|p|p.missing)})),
            Err(error)=>{
                if let Err(load_error)=crate::mail_identity::load(&sh.identity_dir){*sh.identity_error.lock().unwrap()=Some(load_error);}
                Err(error)
            }
        }
    }).await.map_err(|_|"身份导入任务意外结束")?
}
