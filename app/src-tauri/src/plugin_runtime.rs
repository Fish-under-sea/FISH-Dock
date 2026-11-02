//! 外部插件实例注册；所有后台操作串行，停用与卸载只处理本宿主的实例。
use std::{collections::BTreeMap, fs, path::{Path, PathBuf}, sync::Mutex};
use serde_json::{json,Value};
use crate::{plugin_install::{Installed,Manager},plugin_package::Package,plugin_process::Process,storage_context::Context};

// 插件诊断落盘的两处路径（#53-1/2/3）。都在宿主 data 根下，且都不在配置备份的收录
// 范围里（config_backup.rs 只收 config/keys/credentials/roster 与 plugins/pets）：
// 日志不参与整仓备份，也不会随备份上传。
pub fn log_directory_of(host:&Path)->PathBuf{host.join("plugin-logs")}
pub fn log_file_of(host:&Path,id:&str)->PathBuf{log_directory_of(host).join(format!("{id}.log"))}
pub fn failure_log_of(host:&Path)->PathBuf{host.join("plugin-failures.log")}
pub fn log_directory(context:&Context)->PathBuf{log_directory_of(context.host.path())}
// 日志入口（插件卡的「打开插件日志」、宿主侧的权限检查）都从这里取目录，
// 目录与文件名的规则只有这一处。
pub fn log_file(context:&Context,id:&str)->PathBuf{log_directory(context).join(format!("{id}.log"))}
// pid 复用防线：记录里的启动时刻与这个 pid 的实际启动时刻差这么多就别认。
const ORPHAN_SLACK_MS:u64=5000;

// 一条插件失败（#53-3/5）：插件 id、版本、阶段、退出码、时间、原始错误、日志路径。
// 进内存（插件卡当场显示）之外逐行追加进 `host/plugin-failures.log`，宿主重启后仍可查。
#[derive(Clone,Debug)]
pub struct Failure { pub plugin:String, pub version:String, pub stage:String, pub code:Option<i32>, pub at:u64, pub raw:String, pub log:PathBuf }
impl Failure {
    fn json(&self)->Value{json!({"at":self.at,"plugin":self.plugin,"version":self.version,"stage":self.stage,"exitCode":self.code,"error":self.raw,"log":self.log.to_string_lossy()})}
    // 插件卡上的那一句：带退出码与日志路径（#53-4）。前端只渲染这个字符串。
    pub fn text(&self)->String{
        let head=match self.code{Some(code)=>format!("插件进程已退出（退出码：{code}）"),None=>self.raw.clone()};
        format!("{head} · 日志：{}",self.log.display())
    }
}

// 这一个实例这一轮该报成什么（T01）。判据只此一处，`view()` 照着做：
// 真没了才移除实例、记一条失败；还在但探针没应答只是「未响应」，下一轮重绘
// 自己会好，绝不 remove（remove 会 Drop → kill，连它拉起的服务一起带走）。
#[derive(Clone,Copy,Debug,PartialEq)]
enum Liveness { Gone(Option<i32>), Unresponsive, Running }
fn liveness(exited:Option<Option<i32>>,responsive:bool)->Liveness{
    match exited {
        Some(code)=>Liveness::Gone(code),
        None if responsive=>Liveness::Running,
        None=>Liveness::Unresponsive,
    }
}

#[derive(Default)]
pub struct Runner {
    processes:BTreeMap<String,Process>,
    // 失败从「一句内存里的字符串」改成结构化记录（#53-3）：插件卡要能说清是哪个插件、
    // 哪一版、卡在哪一步、退出码多少、日志在哪。
    failures:BTreeMap<String,Failure>,
    // 上一次升级「本次无回滚点」的说明（#37）：回滚点失败绝不阻断更新，但要在插件卡上
    // 说清楚——是导出失败、写入失败，还是跳过了哪些大文件。成功升级一次就覆盖/清掉。
    rollback_notes:BTreeMap<String,String>,
    // 宿主 data 根，第一次拿到 Manager 时记下：`call()` 这类没有 Manager 参数的路径
    // 也要能把失败写进 plugin-failures.log，并说清是哪一版（#53-3）。
    host:Option<PathBuf>,
    versions:BTreeMap<String,String>,
}
pub static CURRENT:Mutex<Runner>=Mutex::new(Runner{processes:BTreeMap::new(),failures:BTreeMap::new(),rollback_notes:BTreeMap::new(),host:None,versions:BTreeMap::new()});
impl Runner {
    // 记住宿主 data 根：`call()` 这类没有 Manager 参数的路径也要能把失败落盘（#53-3）。
    fn remember(&mut self,manager:&Manager<'_>){self.host=Some(manager.context.host.path().to_path_buf());}
    // 一条失败：先进内存（插件卡当场显示），再逐行追加进 plugin-failures.log。
    // 落盘失败只影响「宿主重启后还能不能查」，绝不影响这次调用本身的返回。
    fn record(&mut self,manager:&Manager<'_>,failure:Failure){
        self.remember(manager);
        self.append(failure)
    }
    fn append(&mut self,failure:Failure){
        if let Some(host)=&self.host {
            use std::io::Write;
            if let Ok(mut file)=fs::OpenOptions::new().create(true).append(true).open(failure_log_of(host)){let _=file.write_all(format!("{}\n",failure.json()).as_bytes());let _=file.flush();}
        }
        self.failures.insert(failure.plugin.clone(),failure);
    }
    // 一条失败记录的骨架：id、版本、阶段、退出码、时间、原文、日志路径。
    fn failure(&self,id:&str,version:&str,stage:&str,code:Option<i32>,raw:&str)->Failure{
        let log=self.host.as_deref().map(|host|log_file_of(host,id)).unwrap_or_default();
        Failure{plugin:id.into(),version:version.into(),stage:stage.into(),code,at:crate::now_ms(),raw:raw.into(),log}
    }
    fn version_of(&self,id:&str)->String{self.versions.get(id).cloned().unwrap_or_default()}
    pub fn view(&mut self,manager:&Manager<'_>)->Result<Value,String>{
        self.remember(manager);
        let mut rows=Vec::new();
        for installed in manager.list()? {
            let id=&installed.manifest.id;
            self.versions.insert(id.clone(),installed.manifest.version.clone());
            // 运行态判据（#14-4）在 Process::running 里：进程存活 + 握手可应答。
            // 但「应答不上」不等于「进程没了」（T01）：前者只报一轮「未响应」，
            // 后者才移除实例、记一条失败。两件事以前被 `!running()` 混成一件，
            // 于是设置页一次重绘碰上一次卡顿就把活着的插件 remove + Drop 杀掉，
            // `plugin-failures.log` 里那批 `exitCode:null` 就是这么来的（#55）。
            let (gone, unresponsive)=match self.processes.get_mut(id){
                None=>(None,false),
                Some(process)=>{
                    let exited=process.exited();
                    // 真的没了就不再握手（alive() 会立刻答否，但省一趟也省心）。
                    let responsive=exited.is_none()&&process.running();
                    match liveness(exited,responsive){
                        Liveness::Gone(code)=>(Some(code),false),
                        Liveness::Unresponsive=>(None,true),
                        Liveness::Running=>(None,false),
                    }
                }
            };
            if let Some(code)=gone {
                self.processes.remove(id);
                // 到这里一定是进程真的没了。退出码取不到时也别冒充「已退出」以外的
                // 判断——说清是问不到退出码，用户才知道该去查什么。
                let raw=if code.is_some(){"插件进程已退出，请检查配置后重新启用"}else{"插件进程已退出（退出码未知），请检查配置后重新启用"};
                self.record(manager,Failure{plugin:id.clone(),version:installed.manifest.version.clone(),stage:"运行".into(),code,at:crate::now_ms(),raw:raw.into(),log:log_file(manager.context,id)});
            }
            let running=self.processes.contains_key(id);
            let package_missing=!manager.directory(&installed)?.join("plugin.json").is_file();
            // notice 是上一次没装成被回滚时留下的那句话（没有就是 null），
            // 插件卡上连同 npm 安装方式一起显示（#21）。
            let notice=manager.notice(id);
            let rollback=self.rollback_notes.get(id).map(String::as_str);
            let failure=self.failures.get(id);
            rows.push(json!({"plugin":installed,"running":running,"packageMissing":package_missing,"notice":notice,"rollbackWarning":rollback,
                // error 仍是字符串（前端直接渲染）：退出码与日志路径都在里面（#53-4）；
                // 退出码、阶段、日志路径同时单独给出，页面要用就不必解析中文。
                "error":if package_missing {Some("插件程序包缺失，请在官方商店重新安装；原配置已保留".to_string())}else{failure.map(Failure::text)},
                "failure":failure.map(Failure::json),
                // 活着但这一轮探针没应答（T01）：页面说「未响应，正在重试」，
                // 下一轮重绘自己会好；它不是失败，所以 error / failure 仍是空。
                "unresponsive":unresponsive,
                "exitCode":failure.and_then(|failure|failure.code),
                "logPath":failure.map(|failure|failure.log.to_string_lossy().into_owned()).unwrap_or_else(||log_file(manager.context,id).to_string_lossy().into_owned())}));
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
    // 运行中拨「随 Dock 启动」开关（#38）。以前这里只写注册表，要退出重进 Dock 才生效。
    // 现在：写注册表之外，**Dock 正在运行就立即用 resume 的单插件路径拉起服务**；
    // 关掉就把当前实例停掉。拉起失败会进 failures（插件卡显示「启动失败」+原因）并
    // 返回错误，前端当场可见，不像以前那样要等下次启动。
    pub fn set_autostart(&mut self,manager:&Manager<'_>,id:&str,on:bool,node:Option<&Path>)->Result<(),String>{
        let row=manager.set_autostart(id,on)?;
        if on {
            if row.enabled && !self.processes.get_mut(id).is_some_and(Process::running) {
                self.set_enabled_starting(manager,id,true,node,true)?;
            }
        } else if let Some(mut process)=self.processes.remove(id) { process.stop(); }
        Ok(())
    }
    // autostart：这次拉起是不是「随 Dock 启动」（#22）。resume 传 true，插件读
    // FISH_PLUGIN_AUTOSTART 自己起服务；安装、点启用、回滚都传 false。
    fn set_enabled_starting(&mut self,manager:&Manager<'_>,id:&str,enabled:bool,node:Option<&Path>,autostart:bool)->Result<(),String>{
        self.remember(manager);
        let row=manager.list()?.into_iter().find(|row|row.manifest.id==id).ok_or("插件未安装")?;
        self.versions.insert(id.into(),row.manifest.version.clone());
        if !enabled {
            crate::plugin_page::revoke(id);
            manager.set_enabled(id,false)?;
            if let Some(mut process)=self.processes.remove(id){process.stop();}
            self.failures.remove(id);return Ok(());
        }
        if self.processes.get_mut(id).is_some_and(Process::running) {manager.set_enabled(id,true)?;return Ok(());}
        self.processes.remove(id);
        let mut process=match Process::start(manager,&row,node,autostart){
            Ok(process)=>process,
            // 启动失败也留一条结构化记录（#53-3）：卡上要能看见是哪一版、哪一步、日志在哪。
            Err(error)=>{let failure=self.failure(id,&row.manifest.version,"启动",None,&error);self.record(manager,failure);return Err(error)}
        };
        if let Err(error)=manager.set_enabled(id,true){process.stop();return Err(error);}
        self.processes.insert(id.into(),process);self.failures.remove(id);Ok(())
    }
    pub fn resume(&mut self,manager:&Manager<'_>,node:Option<&Path>)->Result<(),String>{
        self.remember(manager);
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
        // 上一轮没收干净的孤儿内核（#14-4 宿主侧）：插件把自己的内核 pid 写在
        // config/run-state.json 里，这里先按它收，再拉起新实例——否则就是
        // 「僵尸内核 + 新实例启动失败」。判据见 orphan_pid：pid 被系统复用给别人时不动。
        self.reclaim(manager);
        // 关掉「自启动」的插件保持启用，但这次启动不拉起它的服务；用户点启用或
        // 插件页的启动服务仍然照常启动。开着的那批：拉起后台，并告诉它这次是
        // 自启动（FISH_PLUGIN_AUTOSTART=1），服务由插件自己决定起不起（#22）。
        // 逐插件容错（#38）：单个插件失败只影响它自己——失败进 self.failures（卡片上
        // 显示「启动失败」+原因），其余插件继续拉起；最后汇总一行日志，不再整批跳过。
        let mut failed:Vec<String>=Vec::new();
        for row in manager.list()? {
            if !(row.enabled && row.autostart) { continue; }
            if let Err(error)=self.set_enabled_starting(manager,&row.manifest.id,true,node,true) {
                failed.push(format!("{}：{error}",row.manifest.id));
            }
        }
        if !failed.is_empty() { eprintln!("FISH Dock: 插件自启动失败 {} 个（其余已照常拉起）：{}",failed.len(),failed.join("；")); }
        Ok(())
    }
    pub fn call(&mut self,id:&str,method:&str,params:Value)->Result<Value,String>{
        if method.is_empty()||method.len()>100||method.starts_with("host.")||method.starts_with("plugin.")||!method.bytes().all(|b|b.is_ascii_alphanumeric()||b==b'.'||b==b'-'||b==b'_'){return Err("插件业务方法无效或属于宿主保留方法".into());}
        let process=self.processes.get_mut(id).ok_or("插件尚未运行")?;
        match process.request(method,params,crate::config::plugin_rpc_timeout()) {
            Ok(value)=>Ok(value),
            // 插件活着、只是这次业务调用没成：不回收进程、不写失败记录（t12 的错误码
            // 上行之后，业务错误带着「（错误码 X）」回来，不再是那一条固定常量）。
            Err(error) if crate::plugin_protocol::is_business_error(&error)||error==crate::plugin_protocol::OPERATION_ERROR=>Err(error),
            Err(error)=>{
                let code=if let Some(process)=self.processes.get_mut(id){let code=process.exit_code();code}else{None};
                if let Some(mut process)=self.processes.remove(id){process.stop();}
                let version=self.version_of(id);
                let failure=self.failure(id,&version,"运行",code,&error);
                self.append(failure);
                Err(error)
            },
        }
    }
    pub fn uninstall(&mut self,manager:&Manager<'_>,id:&str)->Result<(),String>{
        // t29/C：先停服务（整树，端口释放）再删文件——反过来的话 Windows 上正在运行的
        // 进程还占着 exe/资源句柄，remove_dir_all 会失败，留下半个 packages/<id>/。
        crate::plugin_page::revoke(id);
        if let Some(mut process)=self.processes.remove(id){process.stop();}
        manager.uninstall(id)?;
        self.failures.remove(id);Ok(())
    }
    pub fn install(&mut self,manager:&Manager<'_>,package:Package,digest:&str,official:bool,node:Option<&Path>)->Result<Installed,String>{
        let id=package.manifest.id.clone();
        let old=manager.list()?.into_iter().find(|row|row.manifest.id==id);
        // 回滚点改为选择性导出（#37）：只装配置类小文件、credentials 默认排除、阈值
        // 单一入口进 config.rs；导出/写入任一步失败都只降级为「本次无回滚点」，
        // **绝不阻断更新**。backup 仅用于本次更新失败时即时恢复旧配置。
        let mut rollback_warning=None;
        let backup=if let Some(row)=&old {
            let point=manager.write_rollback_point(row);
            rollback_warning=point.warning;
            point.archive
        }else{None};
        crate::plugin_page::revoke(&id);
        if let Some(mut process)=self.processes.remove(&id){process.stop();}
        let installed=manager.install(package,digest,official);
        let attempt=match installed {
            Ok(row)=>{
                self.versions.insert(id.clone(),row.manifest.version.clone());
                if row.enabled {match Process::start(manager,&row,node,false){Ok(process)=>{self.processes.insert(id.clone(),process);Ok(row)},Err(error)=>Err(error)}}else{Ok(row)}
            },
            Err(error)=>Err(error),
        };
        match attempt {
            Ok(row)=>{
                self.versions.insert(id.clone(),row.manifest.version.clone());
                self.failures.remove(&id);
                match rollback_warning { Some(note)=>{ self.rollback_notes.insert(id.clone(),note); }, None=>{ self.rollback_notes.remove(&id); } }
                // 新版已经激活、回滚点（上一版配置）还在：这时候才删过期版本、
                // 旧摘要目录、暂存文件与越攒越多的回滚点备份。清理失败只记日志
                // 并留一条警告（notice.cleanup），绝不回滚这次升级（#t8）。
                let problems=manager.cleanup(&row);
                if !problems.is_empty(){eprintln!("FISH Dock: 插件清理未完成（{id}）：{}",problems.join("；"));}
                Ok(row)
            },
            Err(error)=>{
                let message=format!("更新失败（{}阶段）：{error}",failure_step(&error));
                let version=old.as_ref().map(|row|row.manifest.version.clone()).unwrap_or_default();
                if let Some(previous)=old {
                    let restored=self.restore_previous(manager,&previous,backup.as_deref(),node);
                    if restored.is_err(){
                        let failure=self.failure(&id,&version,"更新",None,"更新失败（提交阶段）：旧版恢复尚未完成，请保留备份并重试");
                        self.record(manager,failure);
                        return Err("更新失败（提交阶段）：旧版恢复尚未完成，数据备份已保留".into());
                    }
                    // t38/B1：新版本已经落地、但服务起不来，回滚到旧版后把失败的新版本
                    // 目录也删掉——packages/<id>/ 只留旧版本，不把 2.0.0 留在盘上冒充回滚点。
                    let _=manager.prune_versions(&previous);
                }
                self.rollback_notes.remove(&id);
                let failure=self.failure(&id,&version,"更新",None,&message);
                self.record(manager,failure);
                Err(message)
            }
        }
    }
    fn restore_previous(&mut self,manager:&Manager<'_>,row:&Installed,backup:Option<&[u8]>,node:Option<&Path>)->Result<(),String>{
        let available=manager.verify(row).is_ok();
        manager.restore_registration(row)?;
        // 回滚点只含选择性小文件：恢复时只覆盖归档里的项，绝不把「没进回滚点的大文件」
        // 当成待删除项（那等于毁用户数据）。
        if let Some(bytes)=backup {let ns=manager.namespace(row)?;let changes=crate::storage_archive::inspect_rollback(&ns,bytes)?;if !changes.is_empty(){crate::storage_transaction::apply(&ns,changes)?;}}
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
// 孤儿内核回收的判据是纯函数，便于用合成 run-state 单测：
// 只有「记录的 pid 还活着 + 这个 pid 的实际启动时刻与记录一致（±ORPHAN_SLACK_MS，
// 挡住 pid 被系统复用给别人）+ 不是本进程」才认。返回要回收的 pid。
fn orphan_pid(state:&Value,now:u64,self_pid:u32,age_of:impl Fn(u32)->Option<u64>)->Option<u32>{
    let pid=state.get("pid").and_then(Value::as_u64).filter(|pid|*pid>0&&*pid<=u32::MAX as u64)? as u32;
    if pid==self_pid { return None; }
    // startedAt 按毫秒记；秒口径的旧记录换算成毫秒。
    let started=state.get("startedAt").and_then(Value::as_u64)?;
    let started=if started<100_000_000_000 {started.saturating_mul(1000)}else{started};
    let age=age_of(pid)?;
    (now.saturating_sub(started).abs_diff(age)<=ORPHAN_SLACK_MS).then_some(pid)
}
// 那个内核还听得见吗（run-state 记着 controllerPort）：能连上就是可 attach 的活内核，
// 留给插件的 attach 路径接管——主机在这里杀掉它，「残留内核 attach」那条验收就永远
// 过不了；连不上才是上一轮没收干净的僵尸内核，该收。
fn attachable(state:&Value,probe:impl Fn(u16)->bool)->bool{
    let Some(port)=state.get("controllerPort").and_then(Value::as_u64).filter(|port|*port>0&&*port<=u16::MAX as u64) else {return false};
    probe(port as u16)
}
impl Runner {
    fn reclaim(&mut self,manager:&Manager<'_>){
        for row in manager.list().unwrap_or_default() {
            // 本实例已经拉起（或正在拉起）的插件不碰：它的内核是活的、是自己的。
            if self.processes.contains_key(row.manifest.id.as_str()) { continue; }
            let Ok(namespace)=manager.namespace(&row) else {continue};
            let Ok(Some(bytes))=namespace.read("config","run-state.json") else {continue};
            let Ok(state)=serde_json::from_slice::<Value>(&bytes) else {continue};
            let now=crate::now_ms();
            let Some(pid)=orphan_pid(&state,now,std::process::id(),|pid|crate::shell::running_for(pid).map(|age|age.as_millis() as u64)) else {continue};
            if attachable(&state,|port|std::net::TcpStream::connect_timeout(&std::net::SocketAddr::from(([127,0,0,1],port)),std::time::Duration::from_millis(300)).is_ok()) { continue; }
            // taskkill /T /F：与 dsh_bridge.rs 收子进程树同一个办法，不新造 FFI。
            match std::process::Command::new("taskkill").args(["/T","/F","/PID",&pid.to_string()]).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).spawn() {
                Ok(mut child)=>{let _=child.wait();eprintln!("FISH Dock: 插件 {} 上一轮遗留的内核进程 {pid} 已按 run-state 回收",row.manifest.id);},
                Err(error)=>eprintln!("FISH Dock: 插件 {} 遗留内核进程 {pid} 回收失败：{error}",row.manifest.id),
            }
        }
    }
}
// failure_step 把后端错误归到「导出 / 写入 / 下载 / 校验 / 提交」五步之一（#37 文案
// 分级）：面板上要能看出卡在哪一步，而不是笼统一句「存储文件超过限制」。
fn failure_step(error:&str)->&'static str {
    if error.contains("导出") { "导出" }
    else if error.contains("写入")||error.contains("暂存") { "写入" }
    else if error.contains("下载")||error.contains("获取")||error.contains("超时") { "下载" }
    else if error.contains("校验")||error.contains("SHA-256")||error.contains("摘要")||error.contains("清单") { "校验" }
    else { "提交" }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn root(name:&str)->PathBuf{std::env::temp_dir().join(format!("fish-{name}-{}-{}",std::process::id(),crate::now_ms()))}
    // #53-3/5：失败结构化——文案带退出码与日志路径，并逐行追加进 plugin-failures.log；
    // 宿主重启（新的 Runner）之后盘上那条仍在。
    #[test] fn failures_are_structured_and_survive_a_restart() {
        let root=root("failures");
        let context=Context::open(root.clone(),"2.0.2").unwrap();
        let mut runner=Runner::default();runner.remember(&Manager{context:&context});
        let failure=runner.failure("fish.clash","2.1.2","运行",Some(1),"插件进程已退出，请检查配置后重新启用");
        runner.append(failure);
        let text=runner.failures["fish.clash"].text();
        assert!(text.contains("退出码：1"),"卡片文案要带退出码：{text}");
        assert!(text.contains("fish.clash.log"),"卡片文案要带日志路径：{text}");
        let logged=std::fs::read_to_string(failure_log_of(context.host.path())).unwrap();
        let row:Value=serde_json::from_str(logged.trim()).unwrap();
        assert_eq!(row["plugin"],"fish.clash");
        assert_eq!(row["version"],"2.1.2");
        assert_eq!(row["stage"],"运行");
        assert_eq!(row["exitCode"],1);
        assert!(row["at"].as_u64().is_some(),"时间要在：{row}");
        assert_eq!(row["error"],"插件进程已退出，请检查配置后重新启用");
        assert!(row["log"].as_str().unwrap().replace('\\',"/").ends_with("host/plugin-logs/fish.clash.log"),"{row}");
        // 重启：新 Runner 没有内存记录，但盘上那条还查得到（一行一条 JSON）。
        let mut fresh=Runner::default();fresh.remember(&Manager{context:&context});
        assert!(fresh.failures.get("fish.clash").is_none());
        assert!(std::fs::read_to_string(failure_log_of(context.host.path())).unwrap().contains("\"fish.clash\""));
        std::fs::remove_dir_all(&root).unwrap();
    }
    // 启动失败（没有退出码）也要落一条：卡上写的是宿主原话 + 日志路径。
    #[test] fn a_start_failure_without_an_exit_code_still_gets_a_record() {
        let root=root("start-failure");
        let context=Context::open(root.clone(),"2.0.2").unwrap();
        let mut runner=Runner::default();runner.remember(&Manager{context:&context});
        let failure=runner.failure("fish.magpie","2.1.2","启动",None,"插件启动失败，请检查运行时路径及系统权限");
        runner.append(failure);
        let text=runner.failures["fish.magpie"].text();
        assert!(text.contains("插件启动失败"),"{text}");
        assert!(!text.contains("退出码"),"没有码就不要编一个：{text}");
        assert!(text.contains("plugin-logs"),"{text}");
        let row:Value=serde_json::from_str(std::fs::read_to_string(failure_log_of(context.host.path())).unwrap().trim()).unwrap();
        assert!(row["exitCode"].is_null(),"没有退出码就是 null：{row}");
        std::fs::remove_dir_all(&root).unwrap();
    }
    // #53-4：日志目录与失败日志都不进整仓备份（config_backup 只收配置类资料）。
    #[test] fn plugin_diagnostics_stay_out_of_the_config_backup() {
        let root=root("backup");
        let context=Context::open(root.clone(),"2.0.2").unwrap();
        let mut runner=Runner::default();runner.remember(&Manager{context:&context});
        let failure=runner.failure("fish.clash","2.1.2","运行",Some(1),"插件进程已退出，请检查配置后重新启用");
        runner.append(failure);
        std::fs::create_dir_all(log_directory(&context)).unwrap();
        std::fs::write(log_file(&context,"fish.clash"),b"console.error: synthetic stderr").unwrap();
        let bytes=crate::config_backup::export(&root).unwrap();
        let names=String::from_utf8_lossy(&bytes);
        assert!(!names.contains("plugin-logs"),"plugin-logs/ 不得进整仓备份");
        assert!(!names.contains("plugin-failures.log"),"plugin-failures.log 不得进整仓备份");
        // 备份本身仍然出得来（宿主 namespace 在册），不是靠失败「凑」出来的通过。
        assert!(names.contains("manifest.json"));
        std::fs::remove_dir_all(&root).unwrap();
    }
    // #14-4：只有「记录里的 pid 还活着、启动时刻与记录一致、不是本进程」才回收；
    // pid 被系统复用给别人（启动时刻对不上）时一律不动。
    #[test] fn orphan_reclaim_only_touches_the_recorded_kernel() {
        let now=1_770_000_000_000u64;
        let state=json!({"pid":4242,"startedAt":now-60_000});
        assert_eq!(orphan_pid(&state,now,1,|pid|(pid==4242).then_some(60_000)),Some(4242),"活着且启动时刻相符 → 收");
        assert_eq!(orphan_pid(&state,now,1,|pid|(pid==4242).then_some(300_000)),None,"启动时刻对不上＝pid 复用，不许杀");
        assert_eq!(orphan_pid(&state,now,1,|_|None),None,"进程不在了就没什么可收");
        assert_eq!(orphan_pid(&state,now,4242,|_|Some(60_000)),None,"本进程不动");
        assert_eq!(orphan_pid(&json!({}),now,1,|_|Some(60_000)),None,"没写 pid 的记录不认");
        assert_eq!(orphan_pid(&json!({"pid":4242}),now,1,|_|Some(60_000)),None,"没写启动时刻的记录不认");
        assert_eq!(orphan_pid(&json!({"pid":4242,"startedAt":(now-60_000)/1000}),now,1,|_|Some(60_000)),Some(4242),"秒口径的旧记录也认");
    }
    // 还听得见的内核留给插件 attach（否则「残留内核 attach」那条验收永远过不了）；
    // 只有连不上的僵尸内核才回收。
    #[test] fn a_kernel_that_still_answers_is_left_for_attach() {
        let listening=json!({"pid":4242,"startedAt":1,"controllerPort":9090});
        assert!(attachable(&listening,|port|port==9090),"控制端口通得上＝可 attach，留着");
        assert!(!attachable(&listening,|_|false),"连不上＝僵尸内核，回收");
        assert!(!attachable(&json!({"pid":4242}),|_|true),"run-state 没记控制端口：不当作可 attach");
    }
    // 探针没应答 ≠ 进程没了（T01 / #55，诊断 A1）。以前 `!running()` 被当成
    // 「进程已退出」，于是设置页一次重绘碰上一次卡顿就把活着的插件 remove + Drop
    // 杀掉；现在只有 exited() 说没了才算没了，没应答只报一轮「未响应」。
    #[test] fn an_unresponsive_plugin_is_not_a_dead_one() {
        // 还在（exited = None）但这一轮探针没应答：未响应，不能当成没了。
        assert_eq!(liveness(None,false),Liveness::Unresponsive,"没应答就当没了＝把活插件杀了");
        assert_eq!(liveness(None,true),Liveness::Running);
        // 真的没了：有退出码、问不到退出码，都算没了。
        assert_eq!(liveness(Some(Some(3)),true),Liveness::Gone(Some(3)));
        assert_eq!(liveness(Some(None),false),Liveness::Gone(None));
        assert_eq!(liveness(Some(None),true),Liveness::Gone(None),"已经没了就不再算在跑");
    }
}
