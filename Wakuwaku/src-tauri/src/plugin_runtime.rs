//! 外部插件实例注册；所有后台操作串行，停用与卸载只处理本宿主的实例。
use std::{collections::BTreeMap, path::Path, sync::Mutex};
use serde_json::{json,Value};
use crate::{plugin_install::{Installed,Manager},plugin_package::Package,plugin_process::Process};

#[derive(Default)]
pub struct Runner { processes:BTreeMap<String,Process>, failures:BTreeMap<String,String> }
pub static CURRENT:Mutex<Runner>=Mutex::new(Runner{processes:BTreeMap::new(),failures:BTreeMap::new()});
impl Runner {
    pub fn view(&mut self,manager:&Manager<'_>)->Result<Value,String>{
        let mut rows=Vec::new();
        for installed in manager.list()? {
            let id=&installed.manifest.id;
            let running=self.processes.get_mut(id).is_some_and(Process::running);
            if !running && self.processes.remove(id).is_some(){self.failures.insert(id.clone(),"插件进程已退出，请检查配置后重新启用".into());}
            let package_missing=!manager.directory(&installed)?.join("plugin.json").is_file();
            // notice 是上一次没装成被回滚时留下的那句话（没有就是 null），
            // 插件卡上连同 npm 安装方式一起显示（#21）。
            let notice=manager.notice(id);
            rows.push(json!({"plugin":installed,"running":running,"packageMissing":package_missing,"notice":notice,"error":if package_missing {Some("插件程序包缺失，请在官方商店重新安装；原配置已保留")}else{self.failures.get(id).map(String::as_str)}}));
        }
        // 盘上有、注册表里没有的那一版（安装崩在写注册表之前，或者那次写入失败）：
        // 以前它只是消失——插件卡不出现，用户看到的是「装完了，但更新拿不到」，也
        // 没有任何可点的补救（#f3）。这里把它作为 pending 行报上去，前端只给一颗
        // 「重试安装」，不会再出现启用/卸载这类对不存在注册行才有效的动作。
        for pending in manager.pending() {
            let id=pending.manifest.id.clone();
            let notice=manager.notice(&id);
            let mut plugin=serde_json::to_value(&pending).unwrap_or(Value::Null);
            if let Some(map)=plugin.as_object_mut() { map.insert("pending".into(),Value::Bool(true)); }
            rows.push(json!({"plugin":plugin,"running":false,"packageMissing":false,"notice":notice,"error":"安装未完成：这一版已经落地但没登记，点「重试安装」补上"}));
        }
        Ok(json!(rows))
    }
    pub fn set_enabled(&mut self,manager:&Manager<'_>,id:&str,enabled:bool,node:Option<&Path>)->Result<(),String>{
        // 用户点启用 / 停用：这不是自启动，插件不该借这次把服务拉起来。
        self.set_enabled_starting(manager,id,enabled,node,false)
    }
    // autostart：这次拉起是不是「随 Dock 启动」（#22）。resume 传 true，插件读
    // FISH_PLUGIN_AUTOSTART 自己起服务；安装、点启用、回滚都传 false。
    fn set_enabled_starting(&mut self,manager:&Manager<'_>,id:&str,enabled:bool,node:Option<&Path>,autostart:bool)->Result<(),String>{
        let row=manager.list()?.into_iter().find(|row|row.manifest.id==id).ok_or("插件未安装")?;
        if !enabled {
            crate::plugin_page::revoke(id);
            manager.set_enabled(id,false)?;
            if let Some(mut process)=self.processes.remove(id){process.stop();}
            self.failures.remove(id);return Ok(());
        }
        if self.processes.get_mut(id).is_some_and(Process::running) {manager.set_enabled(id,true)?;return Ok(());}
        self.processes.remove(id);
        let mut process=match Process::start(manager,&row,node,autostart){Ok(process)=>process,Err(error)=>{self.failures.insert(id.into(),error.clone());return Err(error)}};
        if let Err(error)=manager.set_enabled(id,true){process.stop();return Err(error);}
        self.processes.insert(id.into(),process);self.failures.remove(id);Ok(())
    }
    pub fn resume(&mut self,manager:&Manager<'_>,node:Option<&Path>)->Result<(),String>{
        manager.recover()?;
        // 与「哪个插件在升级」无关的那一类残留（host/tmp/txn-*、指向已卸载插件的
        // 安装事务、过期下载来源记录）在这里补清：注册表为空时也要收。以前只清了
        // 版本目录，这些暂存与日志文件一次都没人删过（#f3「更新也清理旧文件不干净」）。
        let staging=manager.cleanup_staging();
        if !staging.is_empty() { eprintln!("FISH Dock: 插件事务暂存待下次启动重试：{}",staging.join("；")); }
        // 上次升级的清理没做完（清理失败、或进程在清理前就没了）在这里补一次：
        // 只删「比当前版本旧的东西」，不碰当前版本、不碰用户数据，出错只记日志，
        // 绝不拦住启动、更不会回滚已经装好的升级（#t8）。
        for row in manager.list()? {
            let problems=manager.cleanup(&row);
            if !problems.is_empty() { eprintln!("FISH Dock: 插件残留待下次启动重试（{}）：{}",row.manifest.id,problems.join("；")); }
        }
        // 关掉「自启动」的插件保持启用，但这次启动不拉起它的服务；用户点启用或
        // 插件页的启动服务仍然照常启动。开着的那批：拉起后台，并告诉它这次是
        // 自启动（FISH_PLUGIN_AUTOSTART=1），服务由插件自己决定起不起（#22）。
        for row in manager.list()? {if row.enabled && row.autostart {let _=self.set_enabled_starting(manager,&row.manifest.id,true,node,true);}}
        Ok(())
    }
    pub fn call(&mut self,id:&str,method:&str,params:Value)->Result<Value,String>{
        if method.is_empty()||method.len()>100||method.starts_with("host.")||method.starts_with("plugin.")||!method.bytes().all(|b|b.is_ascii_alphanumeric()||b==b'.'||b==b'-'||b==b'_'){return Err("插件业务方法无效或属于宿主保留方法".into());}
        let process=self.processes.get_mut(id).ok_or("插件尚未运行")?;
        match process.request(method,params,crate::config::plugin_rpc_timeout()) {
            Ok(value)=>Ok(value),
            Err(error) if error==crate::plugin_protocol::OPERATION_ERROR=>Err(error),
            Err(error)=>{if let Some(mut process)=self.processes.remove(id){process.stop();}self.failures.insert(id.into(),error.clone());Err(error)},
        }
    }
    pub fn uninstall(&mut self,manager:&Manager<'_>,id:&str)->Result<(),String>{
        manager.uninstall(id)?;
        crate::plugin_page::revoke(id);
        if let Some(mut process)=self.processes.remove(id){process.stop();}
        self.failures.remove(id);Ok(())
    }
    pub fn install(&mut self,manager:&Manager<'_>,package:Package,digest:&str,official:bool,node:Option<&Path>)->Result<Installed,String>{
        let id=package.manifest.id.clone();
        let old=manager.list()?.into_iter().find(|row|row.manifest.id==id);
        let backup=if let Some(row)=&old {
            let ns=manager.namespace(row)?;
            let categories=["credentials","roster","keys","config"].map(String::from);
            let bytes=crate::storage_archive::export(&ns,&categories)?;
            ns.write("backups",&format!("before-update-{}.zip",crate::now_ms()),&bytes)?;
            Some(bytes)
        }else{None};
        crate::plugin_page::revoke(&id);
        if let Some(mut process)=self.processes.remove(&id){process.stop();}
        let installed=manager.install(package,digest,official);
        let attempt=match installed {
            Ok(row)=>{
                if row.enabled {match Process::start(manager,&row,node,false){Ok(process)=>{self.processes.insert(id.clone(),process);Ok(row)},Err(error)=>Err(error)}}else{Ok(row)}
            },
            Err(error)=>Err(error),
        };
        match attempt {
            Ok(row)=>{
                self.failures.remove(&id);
                // 新版已经激活、回滚点（上一版目录）还在：这时候才删过期版本、
                // 旧摘要目录、暂存文件与越攒越多的回滚点备份。清理失败只记日志
                // 并留一条警告（notice.cleanup），绝不回滚这次升级（#t8）。
                let problems=manager.cleanup(&row);
                if !problems.is_empty(){eprintln!("FISH Dock: 插件清理未完成（{id}）：{}",problems.join("；"));}
                Ok(row)
            },
            Err(error)=>{
                if let Some(previous)=old {
                    let restored=self.restore_previous(manager,&previous,backup.as_deref(),node);
                    if restored.is_err(){self.failures.insert(id,"更新失败，旧版恢复尚未完成，请保留备份并重试".into());return Err("更新失败，旧版恢复尚未完成，数据备份已保留".into());}
                }
                self.failures.insert(id,error.clone());Err(error)
            }
        }
    }
    fn restore_previous(&mut self,manager:&Manager<'_>,row:&Installed,backup:Option<&[u8]>,node:Option<&Path>)->Result<(),String>{
        let available=manager.verify(row).is_ok();
        manager.restore_registration(row)?;
        if let Some(bytes)=backup {let ns=manager.namespace(row)?;let changes=crate::storage_archive::inspect(&ns,bytes)?;if !changes.is_empty(){crate::storage_transaction::apply(&ns,changes)?;}}
        if row.enabled && available {let process=Process::start(manager,row,node,false)?;self.processes.insert(row.manifest.id.clone(),process);}
        Ok(())
    }
    pub fn event(&mut self,params:Value){
        for process in self.processes.values_mut(){
            if process.accepts_events()&&process.running(){
                if process.request("plugin.event",params.clone(),crate::config::plugin_event_timeout()).is_err(){eprintln!("FISH Dock: 插件事件投递失败，请检查插件状态");}
            }
        }
    }
    pub fn is_running(&mut self,id:&str)->bool { self.processes.get_mut(id).is_some_and(Process::running) }
    pub fn stop_all(&mut self){for (_,mut process) in std::mem::take(&mut self.processes){process.stop();}}
}
impl Drop for Runner {fn drop(&mut self){self.stop_all();}}
