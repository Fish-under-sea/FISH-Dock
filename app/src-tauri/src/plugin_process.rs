//! 插件后台 JSON 行会话；读写管道在专属线程中，业务请求和停用有期限。
use std::{fs,io::{BufRead,BufReader,Write},path::{Path,PathBuf},process::{Child,ChildStderr,Command,Stdio},sync::mpsc::{self,Receiver,SyncSender},time::{Duration,Instant}};
use serde_json::{json,Value};
use crate::{plugin_install::{Installed,Manager},storage_context::Namespace};
// stderr 落盘（#53-1/2）：插件进程的错误输出接管成管道，由宿主线程写进
// `<data>/host/plugin-logs/<plugin-id>.log`；单文件 1 MB 滚动保留 2 份
// （`<id>.log` 与 `<id>.log.1`），每行先过 crash/redact.rs 脱敏、再单行截断。
pub const LOG_LIMIT:u64=1024*1024;
pub const LOG_KEEP:usize=2;
// 一次读取的上限：插件一行写多长都不让宿主无限吃内存。
const STDERR_CHUNK:u64=8192;
// 运行态判据（#14-4）里「握手可应答」的时窗：一次探针最多等这么久，答过之后
// 缓存这么久不再重复探——设置页每次重绘都探一遍会把界面拖慢。
const PROBE_TIMEOUT:Duration=Duration::from_secs(2);
const PROBE_TTL:Duration=Duration::from_secs(2);
// 回调帧的待办队列深度：满了就让读线程阻塞（背压回插件的 stdout），不无限攒内存。
// 每帧上限 1 MB，32 帧即最坏 32 MB/插件。
const CALLBACK_BACKLOG:usize=32;
// 业务服务探测的轮询间隔（T-E）：插件说「还没就绪」时最多歇这么久再看，整段仍在
// 本次尝试的共享 deadline 之内。
const HEALTH_POLL:Duration=Duration::from_millis(500);
// 测试用的尝试计数（t18）：证明一次 Process::start 只尝试一次——重试由自启调度器
// （plugin_start）负责，进程层不再自己重试，避免「重试次数 × 阶段预算」的乘法。
// t46/F2-1：改为**线程本地**计数；默认并行 cargo test 下其他用例的 start 不会插进
// 本用例的 before/after 窗口（以前是进程级 AtomicUsize，并行时 3 次里 2~3 次假红）。
#[cfg(test)]
thread_local! { static START_ATTEMPTS: std::cell::Cell<usize> = std::cell::Cell::new(0); }
#[cfg(test)]
fn start_attempts()->usize { START_ATTEMPTS.with(|cell|cell.get()) }
#[cfg(test)]
fn note_start_attempt() { START_ATTEMPTS.with(|cell|cell.set(cell.get()+1)); }

// 一次启动尝试分阶段的耗时（A03）：日志按 verify / spawn / hello / activate / service
// 分别落，出问题能看出卡在哪一步，而不是只看见「启动失败」。
#[derive(Clone,Copy,Debug,Default,PartialEq,Eq)]
pub struct Stages { pub verify_ms:u64, pub spawn_ms:u64, pub hello_ms:u64, pub activate_ms:u64, pub service_ms:u64 }

// 业务服务健康（A03 / T-E）：宿主实例存在不等于网关就绪，两者分开报。
#[derive(Clone,Debug,PartialEq,Eq)]
pub enum Service {
    /// 插件用 plugin.health 明确回了 ready:true。
    Ready,
    /// 插件没有 health 接口（UNKNOWN_METHOD）：服务状态未知，不当成健康。
    Unknown(String),
    /// 明确没就绪 / 探测失败。
    Down(String),
}
impl Service {
    pub fn as_str(&self)->&'static str { match self { Service::Ready=>"ready", Service::Unknown(_)=>"unknown", Service::Down(_)=>"down" } }
    pub fn detail(&self)->Option<&str> { match self { Service::Ready=>None, Service::Unknown(text)|Service::Down(text)=>Some(text) } }
}

// 一次启动失败：阶段 + 人话原因 + 是否永久（身份/协议/摘要/配置错误不重试）。
#[derive(Clone,Debug)]
pub struct StartError { pub phase:crate::plugin_start::Phase, pub message:String, pub stages:Stages, pub permanent:bool }
impl StartError {
    fn new(phase:crate::plugin_start::Phase,message:impl Into<String>,stages:Stages,permanent:bool)->Self { Self{phase,message:message.into(),stages,permanent} }
    /// 插件卡上的一句：先说是哪一步，再说原因。
    pub fn text(&self)->String { format!("{}阶段：{}",self.phase.label(),self.message) }
}
impl std::fmt::Display for StartError { fn fmt(&self,f:&mut std::fmt::Formatter<'_>)->std::fmt::Result { f.write_str(&self.text()) } }

// 一次成功的启动：进程 + 分阶段耗时 + 业务服务健康。
pub struct Started { pub process:Process, pub stages:Stages, pub service:Service }
pub struct Process { child:Child, owner:Option<crate::plugin_job::Owner>, output:Receiver<Result<Value,String>>, input:SyncSender<Value>, next:u64, namespace:Namespace, capabilities:Vec<String>, id:String, probe:Option<Instant>,
    // 会话在握手前建立，实例停止或 Drop 时只撤销它自己的窗口与订阅。
    hook_session:crate::plugin_window::Session }
// autostart_flag 是传给插件的那个环境变量值（#22）：只有 1 是「随 Dock 启动」，
// 其余一律 0，插件据此决定要不要自己把服务拉起来。
fn autostart_flag(autostart:bool)->&'static str { if autostart{"1"}else{"0"} }
impl Process {
    // autostart：这次拉起是不是「随 Dock 启动」（#22）。宿主只在开机自启时传
    // true，安装、启用、回滚都传 false——插件读这个信号自己决定要不要把本机服务
    // 拉起来，宿主不再替它猜。
    //
    // T-E / A02：一次调用只尝试一次，冷/热共用一个 deadline（见 start_with_budget）；
    // 重试、退避与整轮生命周期由 plugin_start 的调度器统一管，进程层不再自己重试。
    pub fn start(manager:&Manager<'_>,row:&Installed,node:Option<&Path>,autostart:bool)->Result<Self,String> {
        let budget=crate::config::plugin_start_budget();
        Self::start_with_budget(manager,row,node,autostart,budget).map(|started|started.process).map_err(|error|{log_error(&row.manifest.id,budget,&error);error.text()})
    }
    // start_with_budget 是唯一实现点：hello、activate 与业务服务探测共用同一个 deadline。
    // 以前两个阶段各拿一份 plugin_activate_budget()，四次重试的理论上限能到 487.5 s（A02）。
    pub fn start_with_budget(manager:&Manager<'_>,row:&Installed,node:Option<&Path>,autostart:bool,budget:Duration)->Result<Started,StartError> {
        use crate::plugin_start::Phase;
        let began=Instant::now();
        let deadline=began+budget;
        let mut stages=Stages::default();
        #[cfg(test)] note_start_attempt();
        // —— verify：包校验 + 命名空间 + 入口文件（阻塞 I/O 也在这次尝试的预算里）——
        let phase_verify=began;
        manager.verify_kind(row).map_err(|fail|StartError::new(Phase::Verify,fail.text(),stages,fail.permanent()))?;
        let namespace=manager.namespace(row).map_err(|error|StartError::new(Phase::Verify,error,stages,true))?;
        let directory=manager.directory(row).map_err(|error|StartError::new(Phase::Verify,error,stages,true))?;
        let entry=crate::plugin_manifest::package_path(&directory,&row.manifest.backend.entry).map_err(|error|StartError::new(Phase::Verify,error,stages,true))?;
        stages.verify_ms=phase_verify.elapsed().as_millis() as u64;
        // —— spawn：组命令、拉起进程、接管管道 ——
        let phase_spawn=Instant::now();
        let mut command=match row.manifest.backend.runtime.as_str() {
            "node"=> {let node=node.ok_or_else(||StartError::new(Phase::Spawn,"插件需要 Node 运行时，请先在插件依赖设置中选择",stages,true))?;crate::storage::safe(node).map_err(|error|StartError::new(Phase::Spawn,error,stages,true))?;if !node.is_absolute()||!node.is_file(){return Err(StartError::new(Phase::Spawn,"插件 Node 运行时路径无效",stages,true));}let mut c=Command::new(node);c.arg(&entry);c},
            "native"=>Command::new(&entry),
            _=>return Err(StartError::new(Phase::Spawn,"插件运行时不受支持",stages,true)),
        };
        command.args(&row.manifest.backend.args).current_dir(&directory).env_remove("NODE_OPTIONS").env_remove("NODE_PATH").env_remove("ELECTRON_RUN_AS_NODE").env("FISH_PLUGIN_ID",&row.manifest.id).env("FISH_PLUGIN_AUTOSTART",autostart_flag(autostart)).env("FISH_PLUGIN_DATA",namespace.path()).env("TMP",namespace.path().join("tmp")).env("TEMP",namespace.path().join("tmp")).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
        #[cfg(windows)] {use std::os::windows::process::CommandExt;command.creation_flags(0x08000000);}
        let mut child=command.spawn().map_err(|error|StartError::new(Phase::Spawn,format!("插件启动失败，请检查运行时路径及系统权限（{error}）"),stages,false))?;
        let owner=match crate::plugin_job::Owner::attach(&child) {Ok(owner)=>owner,Err(error)=>{let _=child.kill();let _=child.wait();return Err(StartError::new(Phase::Spawn,error,stages,false))}};
        let Some(stdout)=child.stdout.take() else{let _=child.kill();return Err(StartError::new(Phase::Spawn,"插件缺少输出管道",stages,false))};
        let Some(mut stdin)=child.stdin.take() else{let _=child.kill();return Err(StartError::new(Phase::Spawn,"插件缺少输入管道",stages,false))};
        let Some(stderr)=child.stderr.take() else{let _=child.kill();return Err(StartError::new(Phase::Spawn,"插件缺少错误输出管道",stages,false))};
        watch_stderr(row.manifest.id.clone(),stderr,manager.context.root.clone(),crate::plugin_runtime::log_file(manager.context,&row.manifest.id));
        // 宿主回执和窗口事件共用有界写队列；预留突发空间，同时不阻塞 UI 事件线程。
        let (input,read)=mpsc::sync_channel::<Value>(32);
        std::thread::spawn(move||{for value in read {let Ok(mut bytes)=serde_json::to_vec(&value) else{break};bytes.push(b'\n');if stdin.write_all(&bytes).and_then(|_|stdin.flush()).is_err(){break;}}});
        // A2（2.1.6）：插件 → 宿主的回调（带 method 的帧）由**独立线程**在任何时刻服务，
        // 不再要求「宿主正好有一次在途请求」。旧实现只有 request/handshake 在等响应的
        // 循环里顺手处理回调，自启流程一结束就没有线程读 stdout：插件在 activate 之后
        // 的每一次 storage 调用都只能干等到 SDK 超时（冷态 30 s），自启动必然失败。
        // 有界队列 + 阻塞发送即背压：插件狂发回调时堵住的是它自己的 stdout，不涨宿主内存。
        // 代次在握手前确定，activate 内也可使用 Hook；回调线程一直绑定这次会话。
        let hook_session=crate::plugin_window::begin_session(&row.manifest.id,input.clone());
        let generation=hook_session.generation;
        let (callbacks,callback_rx)=mpsc::sync_channel::<Value>(CALLBACK_BACKLOG);
        {
            let input=input.clone();
            let namespace=namespace.clone();
            let capabilities=row.manifest.capabilities.clone();
            let id=row.manifest.id.clone();
            let context=manager.context.clone();
            std::thread::spawn(move||{
                for value in callback_rx {
                    let method=value.get("method").and_then(Value::as_str).unwrap_or_default();
                    let params=&value["params"];
                    // 窗口与定向拖入是宿主能力：走 plugin_window 的同一套权限、代次与配额
                    // 检查；其余（存储）仍走 storage_call。回调线程不持插件实例锁。
                    let window_call=method.starts_with("host.windows.")||method.starts_with("host.fileDrop.");
                    let result=if window_call{crate::plugin_window::backend(&context,&id,generation,method,params)}else{storage_call(&namespace,&capabilities,method,params)};
                    let response=match result{
                        Ok(result)=>json!({"id":value["id"],"result":result}),
                        Err(error) if window_call=>json!({"id":value["id"],"error":crate::plugin_window::failure(&error)}),
                        Err(_)=>json!({"id":value["id"],"error":{"code":"HOST_OPERATION_FAILED","message":"宿主操作失败，请检查存储类别与声明能力"}})
                    };
                    if input.send(response).is_err(){break}
                }
            });
        }
        let (tell,output)=mpsc::sync_channel(8);
        std::thread::spawn(move||{let mut reader=BufReader::new(stdout);loop{match crate::plugin_protocol::line(&mut reader){
            Ok(Some(value))=>{
                // 带 method 的是回调：交给回调线程，别排在响应队列里等下一次请求。
                if value.get("method").is_some(){if callbacks.send(value).is_err(){break}}
                else if value.get("id").and_then(Value::as_str).is_some_and(|id|id.starts_with("window-event-")) { continue; }
                else if tell.send(Ok(value)).is_err(){break}
            },
            Ok(None)=>break,
            Err(error)=>{let _=tell.send(Err(error));break},
        }}});
        stages.spawn_ms=phase_spawn.elapsed().as_millis() as u64;
        let mut process=Self{child,owner:Some(owner),output,input,next:0,namespace,capabilities:row.manifest.capabilities.clone(),id:row.manifest.id.clone(),probe:None,hook_session};
        // —— hello + activate：同一份 deadline，谁先花完谁就把后者挤掉 ——
        let hello=json!({"protocol":1,"pluginId":row.manifest.id,"hostVersion":env!("CARGO_PKG_VERSION"),"dataDirectory":process.namespace.path(),"storage":process.namespace.identity(),"capabilities":row.manifest.capabilities,"hostCallTimeoutMs":crate::config::plugin_host_call_timeout_ms_of(budget)});
        handshake_with(&row.manifest.id,hello,deadline,Instant::now,&mut stages,|method,params,timeout|process.request(method,params,timeout))?;
        // —— service：业务服务健康与「进程还在」分开判（A03）——
        let phase_service=Instant::now();
        let service=probe_service_with(deadline,Instant::now,|method,params,timeout|process.request(method,params,timeout));
        stages.service_ms=phase_service.elapsed().as_millis() as u64;
        if let Service::Down(reason)=&service {
            process.stop_quick();
            return Err(StartError::new(Phase::Service,format!("{reason}（业务服务未就绪，宿主实例不算通过）"),stages,false));
        }
        log_stages(&row.manifest.id,budget,&stages,&service);
        Ok(Started{process,stages,service})
    }
    fn send(&self,value:Value)->Result<(),String> {
        if serde_json::to_vec(&value).map_err(|_|"插件请求编码失败")?.len() as u64>crate::plugin_protocol::MAX_LINE-1 {return Err("插件请求超过协议限制".into());}
        self.input.try_send(value).map_err(|_|"插件输入阻塞或进程已退出".into())
    }
    // request 只等**自己这条**响应：回调帧已由常驻线程处理（A2），这里不再兼职。
    pub fn request(&mut self,method:&str,params:Value,timeout:Duration)->Result<Value,String> {
        self.next=self.next.checked_add(1).ok_or("插件请求编号已耗尽")?;let id=self.next;
        self.send(json!({"id":id,"method":method,"params":params}))?;
        let deadline=Instant::now()+timeout;
        loop {
            let left=deadline.saturating_duration_since(Instant::now());if left.is_zero(){return Err("插件响应超时".into());}
            let value=match self.output.recv_timeout(left){
                Ok(value)=>value?,
                Err(mpsc::RecvTimeoutError::Timeout)=>return Err("插件响应超时".into()),
                Err(mpsc::RecvTimeoutError::Disconnected)=>{let code=self.child.try_wait().ok().flatten().and_then(|status|status.code());return Err(format!("插件进程已退出（退出码：{code:?}）"));}
            };
            // 上一条请求超时后才到的回执会把编号串位：丢掉它继续等自己的编号，
            // 不把一次瞬时超时升级成「ID 不匹配 → 杀掉插件」。
            if value.get("id").and_then(Value::as_u64)==Some(id){ return crate::plugin_protocol::answer(value,id); }
        }
    }
    /// 页面调用还必须匹配这个借出实例的 Hook 会话，不能只检查同 ID 当前会话。
    pub fn request_session(&mut self,generation:u64,method:&str,params:Value,timeout:Duration)->Result<Value,String>{
        if generation==0||self.hook_session.generation!=generation||crate::plugin_window::current_generation(&self.id)!=generation{return Err("页面会话已撤销".into());}
        self.request(method,params,timeout)
    }
    pub fn accepts_events(&self)->bool{self.capabilities.iter().any(|c|c=="events")}
    // 进程退出码（#53-3）：还活着是 None；已经退出就是它带走的那个码
    // （被强杀时是 Windows 的码，Task Manager 杀进程是 1）。
    pub fn exit_code(&mut self)->Option<i32>{match self.child.try_wait(){Ok(Some(status))=>status.code(),_=>None}}
    // 进程是不是真的还在：只看 try_wait，不看应答（T01）。探针超时只说明它此刻
    // 答不上——长任务、GC、被 Defender 扫着——不等于进程没了。以前两者是一件事
    // （`!running()` 就当没了），于是设置页一次重绘碰上一次卡顿，就把它从
    // `CURRENT` 里 remove 掉，`Drop` 顺手 kill，连它拉起的 magpie.exe / mihomo.exe
    // 一起带走（#55，诊断 A1）。
    pub fn alive(&mut self)->bool{matches!(self.child.try_wait(),Ok(None))}
    // 真的没了：Some(退出码)，问不出退出码也是 Some(None)；还活着：None。
    pub fn exited(&mut self)->Option<Option<i32>>{
        match self.child.try_wait(){Ok(None)=>None,Ok(Some(status))=>Some(status.code()),Err(_)=>Some(None)}
    }
    // 运行态判据（#14-4 宿主侧）：「进程存活 + 握手可应答」双条件。
    // 只看 try_wait 会把「进程还在、事件循环卡死」的假活当成运行中——那正是 #14
    // 现象 C 里「内核已停止 + 代理有效」的另一半根因。探针走 host.hello：SDK 对
    // 重复握手回 BAD_HANDSHAKE，那也是一条应答，所以只要编号对得上就说明它还听得
    // 见；探过之后缓存 PROBE_TTL，设置页每次重绘不会各探一轮。
    // 调用方要分「没应答」和「没了」的，用 alive() / exited()，别拿它当死亡证明。
    pub fn running(&mut self)->bool{
        if !self.alive() { self.probe=None; return false; }
        if self.probe.is_some_and(|at|at.elapsed()<PROBE_TTL) { return true; }
        if self.handshake() { self.probe=Some(Instant::now()); true } else { self.probe=None; false }
    }
    // 一次探针：发一条请求，等这个编号上的**任何**应答（result 或 error 都算——
    // 我们要的是「它还听得见」，不是那条方法本身的结果）。
    fn handshake(&mut self)->bool{
        let Some(id)=self.next.checked_add(1) else{return false};
        self.next=id;
        if self.send(json!({"id":id,"method":"host.hello","params":{"protocol":1,"pluginId":self.id,"hostVersion":env!("CARGO_PKG_VERSION"),"dataDirectory":self.namespace.path(),"storage":self.namespace.identity(),"capabilities":self.capabilities}})).is_err(){return false}
        let deadline=Instant::now()+PROBE_TIMEOUT;
        loop {
            let left=deadline.saturating_duration_since(Instant::now());if left.is_zero(){return false}
            let value=match self.output.recv_timeout(left){Ok(Ok(value))=>value,_=>return false};
            // 回调由常驻线程处理（A2）；探针只认自己这个编号，别的（迟到响应）丢掉重等。
            if value.get("id").and_then(Value::as_u64)==Some(id) { return true }
        }
    }
    pub fn stop(&mut self){
        // 用户点的「停用」：值得等它把内核与系统代理收干净（2.1.8 起单独一份预算，
        // 不再复用激活的 5 秒）。
        self.stop_with(crate::config::plugin_deactivate_timeout());
    }
    // 宿主退出、丢弃迟到的实例、启动失败清理：这些路径上没人盯着，不能让用户
    // 关个程序等 30 秒。代理那一步在 stop_with 里先做，6 秒足够。
    pub fn stop_quick(&mut self){
        self.stop_with(std::time::Duration::from_secs(6));
    }
    fn stop_with(&mut self,budget:std::time::Duration){
        self.hook_session.revoke();
        // 只要进程还活着就发告别：探针 2 s 超时说明它「忙」（一条慢命令排着队），
        // 不等于「可以跳过 deactivate」。跳过就等于直接 kill——clash 的系统代理租约
        // 会留在注册表里指向已经死掉的端口，用户看到的是整机断网。
        if self.alive(){let _=self.request("plugin.deactivate",json!({}),budget);}
        // 告别没赶上（超时、插件卡死、或它根本没实现 deactivate）时，代理只能由宿主
        // 自己修：此刻内核还在跑就什么都不做，端口已经没了就写回接管前的那一份。
        // 必须在 take/kill 之前——Job 一关，插件与它拉起的 mihomo 一起消失。
        self.heal_proxy();
        // 正常停用后也回收本实例创建的残留子进程。
        self.owner.take();let _=self.child.kill();let _=self.child.wait();
    }
    // 插件命名空间里的系统代理租约（插件侧 `createProxyLease` 写的那一份）。
    fn heal_proxy(&self)->crate::proxy_heal::Outcome{let lease=self.namespace.path().join("config").join("system-proxy.json");crate::proxy_heal::heal_one(&lease)}
}
impl Drop for Process {fn drop(&mut self){self.heal_proxy();self.owner.take();let _=self.child.kill();let _=self.child.wait();}}
// 一次握手 + 激活：**整段共享一个 deadline**（A02）。hello 用掉的时间从 activate 里扣，
// 反之亦然；预算在握手之前或握手之后花完都明确报「哪一步没时间了」，不静默降级。
fn handshake_with<C>(plugin_id:&str,hello:Value,deadline:Instant,mut now:impl FnMut()->Instant,stages:&mut Stages,mut call:C)->Result<(),StartError>
where C: FnMut(&str,Value,Duration)->Result<Value,String> {
    let phase_hello=now();
    let left=deadline.saturating_duration_since(now());
    if left.is_zero() { return Err(StartError::new(crate::plugin_start::Phase::Hello,"启动预算在握手之前就用完了（校验或进程创建耗时过长）",*stages,false)); }
    let reply=call("host.hello",hello,left).map_err(|error|StartError::new(crate::plugin_start::Phase::Hello,error,*stages,false))?;
    stages.hello_ms=now().saturating_duration_since(phase_hello).as_millis() as u64;
    if reply["type"]!="plugin.hello" || reply["protocol"]!=1 || reply["pluginId"]!=plugin_id { return Err(StartError::new(crate::plugin_start::Phase::Hello,"插件握手身份或协议不匹配",*stages,true)); }
    let phase_activate=now();
    let left=deadline.saturating_duration_since(now());
    if left.is_zero() { return Err(StartError::new(crate::plugin_start::Phase::Activate,"握手已用完本次启动预算，没有时间激活",*stages,false)); }
    call("plugin.activate",json!({}),left).map_err(|error|StartError::new(crate::plugin_start::Phase::Activate,error,*stages,false))?;
    stages.activate_ms=now().saturating_duration_since(phase_activate).as_millis() as u64;
    Ok(())
}
// 业务服务健康探测（A03）：插件用 `plugin.health` 报 `{ready:true}`；没实现这个方法
// （UNKNOWN_METHOD）就如实报「未知」，绝不把「实例在」当成「服务健康」。
fn probe_service_with<C>(deadline:Instant,mut now:impl FnMut()->Instant,mut call:C)->Service
where C: FnMut(&str,Value,Duration)->Result<Value,String> {
    loop {
        let left=deadline.saturating_duration_since(now());
        if left.is_zero() { return Service::Down("业务服务未在启动预算内就绪".into()); }
        let slice=left.min(HEALTH_POLL*2);
        match call("plugin.health",json!({}),slice) {
            Ok(reply) => match reply.get("ready").and_then(Value::as_bool) {
                Some(true)=>return Service::Ready,
                Some(false)=> {
                    let detail=reply.get("detail").and_then(Value::as_str).unwrap_or("插件报告业务服务尚未就绪").to_string();
                    let left=deadline.saturating_duration_since(now());
                    if left.is_zero() { return Service::Down(detail); }
                    std::thread::sleep(HEALTH_POLL.min(left));
                }
                // 回了结果却没有 ready 字段：协议上没有可判的结论，按未知处理而不是猜健康。
                None=>return Service::Unknown("插件 health 结果缺少 ready 字段".into()),
            },
            Err(error) => {
                return match crate::plugin_protocol::split_error(&error) {
                    Some(("UNKNOWN_METHOD",_))=>Service::Unknown("插件未提供 plugin.health，业务服务状态未知".into()),
                    _=>Service::Down(error),
                };
            }
        }
    }
}
fn log_stages(id:&str,budget:Duration,stages:&Stages,service:&Service) {
    let detail=service.detail().map(|text|format!(" detail={text}")).unwrap_or_default();
    eprintln!("FISH Dock: [plugin-start] id={id} budget_ms={} verify_ms={} spawn_ms={} hello_ms={} activate_ms={} service_ms={} service={}{detail}",
        budget.as_millis(),stages.verify_ms,stages.spawn_ms,stages.hello_ms,stages.activate_ms,stages.service_ms,service.as_str());
}
fn log_error(id:&str,budget:Duration,error:&StartError) {
    eprintln!("FISH Dock: [plugin-start] id={id} budget_ms={} failed_at={} permanent={} verify_ms={} spawn_ms={} hello_ms={} activate_ms={} service_ms={} error={}",
        budget.as_millis(),error.phase.as_str(),error.permanent,error.stages.verify_ms,error.stages.spawn_ms,error.stages.hello_ms,error.stages.activate_ms,error.stages.service_ms,error.message);
}
// 插件 stderr 的落盘线程（#53-1/2）：读到一行就脱敏、截断、写一行。插件退出把
// 管道关掉，循环随之结束；写不了就把 sink 丢掉，绝不因此拖住插件本体。
fn watch_stderr(id:String,stderr:ChildStderr,data:PathBuf,log:PathBuf){
    std::thread::spawn(move||{
        let redactor=crate::crash::Redactor::here(&data,&[]);
        let mut sink=Log::open(&log,LOG_LIMIT,LOG_KEEP);
        let mut reader=BufReader::new(stderr);
        loop {
            let mut bytes=Vec::new();
            // 读到行尾为止，单次最多 STDERR_CHUNK 字节：插件写超长一行时分段读，
            // 每段各成一条日志（反正每条都会被截断），宿主内存不会被它撑爆。
            match std::io::Read::take(&mut reader,STDERR_CHUNK).read_until(b'\n',&mut bytes){Ok(0)=>break,Ok(_)=>{},Err(_)=>break}
            let text=String::from_utf8_lossy(&bytes);
            let line=stderr_line(&redactor,&id,&text);
            match sink.as_mut(){Some(file)=>file.line(&line),None=>break}
        }
    });
}
// 一行日志的形状：`<ms> [plugin] <id>: <脱敏后的原文>`。前缀是 ASCII、好 grep；
// G1 报告要按插件 id 过滤，所以 id 单独放在固定位置上。
fn stderr_line(redactor:&crate::crash::Redactor,id:&str,raw:&str)->String{
    let text=raw.trim_end_matches(['\r','\n']);
    let (shielded,held)=shield(text);
    // crash/redact.rs 的 Redactor 负责令牌、路径、邮箱、IP、主机名与单行截断。
    let redacted=redactor.text(&shielded);
    format!("{} [plugin] {id}: {}",crate::now_ms(),restore(&redacted,&held))
}
// Redactor 是写给公开崩溃报告的：双引号里的整段会变 ‹…›，非 ASCII 词也会变 ‹…›。
// 插件日志是本机诊断用，中文报错与 JSON 里的 msg 必须读得出来，所以先把引号换成
// 控制字符、把非 ASCII 词段换成占位，过完 Redactor 再原样还原——令牌、路径、邮箱、
// IP、主机名全在 ASCII 段里，照旧被脱敏（这一层不改 Redactor，只是给它让路）。
// 原文里本来就有的这两个控制字符先剔掉，免得插件的输出伪造占位。
const QUOTE_HOLD:char='\u{1}';
const TEXT_HOLD:char='\u{2}';
fn shield(text:&str)->(String,Vec<String>){
    let mut out=String::with_capacity(text.len());
    let mut held:Vec<String>=Vec::new();
    let mut run=String::new();
    let flush=|out:&mut String,held:&mut Vec<String>,run:&mut String|{if run.is_empty(){return}held.push(std::mem::take(run));out.push(TEXT_HOLD);out.push_str(&(held.len()-1).to_string());out.push(TEXT_HOLD);};
    for c in text.chars(){
        if c==QUOTE_HOLD||c==TEXT_HOLD {continue}
        if c=='"'{flush(&mut out,&mut held,&mut run);out.push(QUOTE_HOLD);}
        else if c.is_ascii(){flush(&mut out,&mut held,&mut run);out.push(c);}
        else {run.push(c);}
    }
    flush(&mut out,&mut held,&mut run);
    (out,held)
}
fn restore(text:&str,held:&[String])->String{
    let mut out=String::with_capacity(text.len());
    let mut chars=text.chars().peekable();
    while let Some(c)=chars.next(){
        if c==QUOTE_HOLD {out.push('"');continue}
        if c==TEXT_HOLD {
            let mut digits=String::new();
            while chars.peek().is_some_and(|d|d.is_ascii_digit()) {if let Some(d)=chars.next(){digits.push(d)}}
            if chars.peek()==Some(&TEXT_HOLD){chars.next();}
            match digits.parse::<usize>().ok().and_then(|index|held.get(index)){Some(text)=>out.push_str(text),None=>out.push_str(&digits)}
            continue;
        }
        out.push(c);
    }
    out
}
// 单文件 1 MB、保留 2 份的滚动写（#53-1）。`<id>.log` 正在写，`<id>.log.1` 是上一份；
// 写满就删最旧的那份、把当前这份改名成 .1、开一份新的。失败就放弃 sink，不重试、
// 不影响插件运行。
struct Log { file:fs::File, size:u64, path:PathBuf, limit:u64, keep:usize }
impl Log {
    fn open(path:&Path,limit:u64,keep:usize)->Option<Self>{
        if let Some(parent)=path.parent(){fs::create_dir_all(parent).ok()?;}
        let file=fs::OpenOptions::new().create(true).append(true).open(path).ok()?;
        let size=file.metadata().map(|meta|meta.len()).unwrap_or(0);
        Some(Self{file,size,path:path.to_path_buf(),limit,keep:keep.max(2)})
    }
    fn line(&mut self,line:&str){
        let mut bytes=line.as_bytes().to_vec();if !bytes.ends_with(b"\n"){bytes.push(b'\n');}
        if self.size>0&&self.size+bytes.len() as u64>self.limit&&!self.rotate() {
            // 滚动失败（被占用、没权限）：就地把当前这份继续写，别丢这一行。
            if let Ok(file)=fs::OpenOptions::new().create(true).append(true).open(&self.path){self.file=file;self.size=0;}
        }
        if self.file.write_all(&bytes).and_then(|_|self.file.flush()).is_ok(){self.size+=bytes.len() as u64;}
    }
    fn rotate(&mut self)->bool{
        let _=self.file.flush();
        let roll=|index:usize|PathBuf::from(format!("{}.{index}",self.path.to_string_lossy()));
        let _=fs::remove_file(roll(self.keep-1));
        for index in (1..self.keep.saturating_sub(1)).rev(){let _=fs::rename(roll(index),roll(index+1));}
        if fs::rename(&self.path,roll(1)).is_err(){return false}
        match fs::OpenOptions::new().create(true).append(true).open(&self.path){Ok(file)=>{self.file=file;self.size=0;true},Err(_)=>false}
    }
}
fn storage_call(ns:&Namespace,capabilities:&[String],method:&str,params:&Value)->Result<Value,String>{
    if !capabilities.iter().any(|c|c=="storage"){return Err("插件未声明存储能力".into());}
    let category=params["category"].as_str().ok_or("存储调用缺少类别")?;
    let name=params["name"].as_str().ok_or("存储调用缺少文件名")?;
    match method {
        "host.storage.read"=>{let value=ns.read(category,name)?.map(String::from_utf8).transpose().map_err(|_|"存储内容不是 UTF-8")?;Ok(json!({"value":value}))},
        "host.storage.write"=>{let value=params["value"].as_str().ok_or("存储写入内容应为文本")?;ns.write(category,name,value.as_bytes())?;Ok(json!({"written":true}))},
        "host.storage.writeSecret"=>{
            if !["credentials","keys"].contains(&category){return Err("加密凭据类别无效".into());}
            let value=params["value"].as_str().ok_or("加密凭据应为文本")?;
            if value.len()>16384{return Err("加密凭据超过大小限制".into());}
            let encrypted=crate::mail_secrets::protect(value.as_bytes())?;
            ns.write(category,name,&encrypted)?;Ok(json!({"written":true}))
        },
        "host.storage.readSecret"=>{
            if !["credentials","keys"].contains(&category){return Err("加密凭据类别无效".into());}
            let value=ns.read(category,name)?.map(|bytes|{
                if bytes.len()>16384+65536{return Err("加密凭据超过大小限制".into());}
                String::from_utf8(crate::mail_secrets::unprotect(&bytes)?).map_err(|_|"加密凭据内容不是 UTF-8".to_string())
            }).transpose()?;
            Ok(json!({"value":value}))
        },
        _=>Err("插件调用了未知宿主方法".into()),
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    use sha2::{Digest,Sha256};
    // t30：独立验收矩阵（只读调用实现；证据在 build/crash-win/t30-apphome）。
    #[path = "../../plugin_t30_acceptance.rs"]
    mod t30;
    // t29 的矩阵测试用：一处建好隔离数据根里的宿主/插件命名空间。
    fn t29_namespace(root:std::path::PathBuf,id:&str,directory:&str,version:&str)->Namespace {
        crate::storage::initialize(&root,id,version,1).unwrap();
        let identity=crate::storage_context::Identity{plugin_id:id.into(),directory:directory.into(),plugin_version:version.into(),data_schema:1};
        std::fs::write(root.join("storage.json"),serde_json::to_vec(&identity).unwrap()).unwrap();
        Namespace::existing(&root).unwrap().unwrap()
    }
    // 造一个能跟宿主握手/激活的合成插件包（SDK 后端），版本不同字节就不同。
    fn t29_package(id:&str,name:&str,directory:&str,version:&str,official:bool)->(crate::plugin_package::Package,String){
        let backend=format!("import {{createPlugin}} from './sdk.mjs';createPlugin({{id:'{id}',version:'{version}',commands:{{ping:()=> 'pong'}}}});// {version}").into_bytes();
        let sdk=include_bytes!("../../../sdk/node/plugin.mjs").to_vec();
        let mut files=BTreeMap::from([("backend.mjs".to_string(),backend),("sdk.mjs".to_string(),sdk),("README.md".to_string(),format!("# {name} {version}").into_bytes())]);
        let hashes:BTreeMap<_,_>=files.iter().map(|(key,value)|(key.clone(),format!("{:x}",Sha256::digest(value)))).collect();
        let raw=json!({"schema":1,"id":id,"name":name,"version":version,"hostApi":1,"platforms":[crate::plugin_install::platform()],"storage":{"directory":directory,"dataSchema":1},"backend":{"runtime":"node","entry":"backend.mjs"},"behaviors":{"autostart":true},"files":hashes});
        files.insert("plugin.json".into(),serde_json::to_vec(&raw).unwrap());
        let bytes=crate::identity_archive::zip(&files).unwrap();
        let digest=format!("{:x}",Sha256::digest(&bytes));
        let source=if official{crate::storage::Source::OfficialCatalog}else{crate::storage::Source::ThirdParty};
        let package=crate::plugin_package::inspect(&bytes,&digest,&std::env::temp_dir(),source,&crate::plugin_install::platform()).unwrap();
        (package,digest)
    }
    // 一轮的「进程 / 版本目录」快照（端口：合成 SDK 后端不监听任何端口，故为 0）。
    fn t29_snapshot(manager:&Manager<'_>,id:&str,running:bool)->String {
        let rows=manager.list().unwrap();
        let version=rows.iter().find(|row|row.manifest.id==id).map(|row|row.manifest.version.clone()).unwrap_or_default();
        let base=manager.context.host.path().join("packages").join(id);
        let mut dirs:Vec<String>=std::fs::read_dir(&base).map(|entries|entries.flatten().filter(|e|e.file_type().map(|t|t.is_dir()).unwrap_or(false)).map(|e|e.file_name().to_string_lossy().to_string()).collect()).unwrap_or_default();
        dirs.sort();
        format!("version={version} running={running} port=0 dirs={dirs:?}")
    }
    #[test]
    #[ignore = "需 FISH_DOCK_TEST_NODE；t29 热更新 5 轮×2 + 自启动 10/10 + 重启 5 + 卸载快照"]
    fn t29_hot_update_autostart_uninstall_matrices() {
        let node=std::env::var_os("FISH_DOCK_TEST_NODE").map(std::path::PathBuf::from).expect("设置 FISH_DOCK_TEST_NODE");
        let root=std::env::temp_dir().join(format!("fish-t29-{}-{}",std::process::id(),crate::now_ms())).join(".fish-dock");
        let host=t29_namespace(root.join("host"),"host","host","1.0.0");
        let plugins=[("fish.magpie","Magpie",".fishplugin-magpie",true),("acme.weather","Weather",".plugin.acme.weather",false)];
        for (id,_name,dir,_official) in plugins { t29_namespace(root.join(dir),id,dir,"1.0.0"); }
        let context=crate::storage_context::Context{root:root.clone(),host};
        let manager=Manager{context:&context};
        let mut runner=crate::plugin_runtime::Runner::default();
        let mut failures=0u32;
        // B：官方与第三方各 5 轮更新（停→换→起→删旧），每轮给快照。
        for (id,name,dir,official) in plugins {
            let (package,digest)=t29_package(id,name,dir,"1.0.0",official);
            manager.install(package,&digest,official).unwrap();
            runner.set_enabled(&manager,id,true,Some(&node)).unwrap();
            println!("[B] {id} 起点 {}",t29_snapshot(&manager,id,runner.is_running(id)));
            for round in 1..=5 {
                let version=format!("1.{round}.0");
                let before=t29_snapshot(&manager,id,runner.is_running(id));
                let (package,digest)=t29_package(id,name,dir,&version,official);
                match runner.install(&manager,package,&digest,official,Some(&node)) {
                    Ok(row)=>{
                        let after=t29_snapshot(&manager,id,runner.is_running(id));
                        println!("[B] {id} 轮{round} {before} -> v{} @{} {after}",row.manifest.version,crate::now_ms());
                        if row.manifest.version!=version||!runner.is_running(id) { failures+=1; }
                        if std::fs::read_dir(manager.context.host.path().join("packages").join(id)).unwrap().flatten().filter(|e|e.file_type().map(|t|t.is_dir()).unwrap_or(false)).count()!=1 { failures+=1; }
                    }
                    Err(error)=>{ failures+=1; println!("[B] {id} 轮{round} 失败：{error}"); }
                }
            }
        }
        // D：开/关各 10 次（关→当场停、开→当场起），再模拟 Dock 重启 5 次。
        let mut toggle_mismatch=0u32;
        for _ in 0..10 {
            runner.set_autostart(&manager,"fish.magpie",false,Some(&node)).unwrap();
            if runner.is_running("fish.magpie") { toggle_mismatch+=1; }
            runner.set_autostart(&manager,"fish.magpie",true,Some(&node)).unwrap();
            if !runner.is_running("fish.magpie") { toggle_mismatch+=1; }
        }
        let mut restart_mismatch=0u32;
        for _ in 0..5 {
            runner.stop_all();
            let mut fresh=crate::plugin_runtime::Runner::default();
            fresh.resume(&manager,Some(&node)).unwrap();
            for row in manager.list().unwrap() {
                if fresh.is_running(&row.manifest.id)!=(row.enabled&&row.autostart) { restart_mismatch+=1; }
            }
            runner=fresh;
        }
        // C：卸载两台 → 目录/注册表/进程/暂存清空，而用户配置保留。
        for (id,_name,_dir,_official) in plugins {
            let row=manager.list().unwrap().into_iter().find(|row|row.manifest.id==id).unwrap();
            let ns=manager.namespace(&row).unwrap();
            ns.write("keys","keep.json",b"synthetic-key").unwrap();
            runner.uninstall(&manager,id).unwrap();
            let package_dir=manager.context.host.path().join("packages").join(id).exists();
            assert!(!package_dir,"{id} packages/<id>/ 必须消失");
            assert!(manager.list().unwrap().iter().all(|row|row.manifest.id!=id),"{id} 注册表条目必须消失");
            assert!(!runner.is_running(id),"{id} 进程必须为 0");
            assert!(manager.pending().is_empty(),"{id} 不得留成 pending");
            assert_eq!(std::fs::read(ns.path().join("keys/keep.json")).unwrap(),b"synthetic-key","{id} 用户配置必须保留");
            println!("[C] {id} 卸载后 packageDir={package_dir} 注册表=0 进程=0 pending=0 用户配置=保留");
        }
        println!("[T29] B失败轮数={failures}/10  D开关不一致={toggle_mismatch}/20  D重启不一致={restart_mismatch}/次");
        assert_eq!((failures,toggle_mismatch,restart_mismatch),(0,0,0),"t29 矩阵要求全为 0");
        let _=std::fs::remove_dir_all(root.parent().unwrap());
    }
    #[test]
    #[ignore = "需通过 FISH_DOCK_TEST_NODE 指定测试运行时；仅操作合成插件"]
    fn node_sdk_roundtrip_and_owned_process_shutdown() {
        use std::collections::BTreeMap;
        use sha2::{Digest,Sha256};
        let node=std::env::var_os("FISH_DOCK_TEST_NODE").map(std::path::PathBuf::from).expect("设置 FISH_DOCK_TEST_NODE");
        let root=std::env::temp_dir().join(format!("fish-process-{}-{}",std::process::id(),crate::now_ms())).join(".fish-dock");
        fn namespace(root:std::path::PathBuf,id:&str,directory:&str)->Namespace {
            crate::storage::initialize(&root,id,"1.0.0",1).unwrap();
            let identity=crate::storage_context::Identity{plugin_id:id.into(),directory:directory.into(),plugin_version:"1.0.0".into(),data_schema:1};
            std::fs::write(root.join("storage.json"),serde_json::to_vec(&identity).unwrap()).unwrap();
            Namespace::existing(&root).unwrap().unwrap()
        }
        let host=namespace(root.join("host"),"host","host");
        namespace(root.join(".plugin.acme.weather"),"acme.weather",".plugin.acme.weather");
        let context=crate::storage_context::Context{root:root.clone(),host};let manager=Manager{context:&context};
        let backend=b"import {createPlugin} from './sdk.mjs';createPlugin({id:'acme.weather',commands:{invalid:()=>{throw Error('synthetic validation error')},save:async(_,ctx)=>{await ctx.storage.write('keys','test.json','synthetic');return ctx.storage.read('keys','test.json')}}});".to_vec();
        let sdk=include_bytes!("../../../sdk/node/plugin.mjs").to_vec();
        let mut files=BTreeMap::from([("backend.mjs".to_string(),backend),("sdk.mjs".to_string(),sdk.clone()),("README.md".to_string(),b"# synthetic".to_vec())]);
        let hashes:BTreeMap<_,_>=files.iter().map(|(key,value)|(key.clone(),format!("{:x}",Sha256::digest(value)))).collect();
        let raw=json!({"schema":1,"id":"acme.weather","name":"合成天气","version":"1.0.0","hostApi":1,"platforms":[crate::plugin_install::platform()],"storage":{"directory":".plugin.acme.weather","dataSchema":1},"backend":{"runtime":"node","entry":"backend.mjs"},"capabilities":["storage"],"files":hashes});
        files.insert("plugin.json".into(),serde_json::to_vec(&raw).unwrap());
        let bytes=crate::identity_archive::zip(&files).unwrap();let digest=format!("{:x}",Sha256::digest(&bytes));
        let package=crate::plugin_package::inspect(&bytes,&digest,&root,crate::storage::Source::ThirdParty,&crate::plugin_install::platform()).unwrap();
        let row=manager.install(package,&digest,false).unwrap();
        let mut process=Process::start(&manager,&row,Some(&node),false).unwrap();assert!(process.running());
        assert_eq!(process.request("save",json!({}),Duration::from_secs(2)).unwrap(),"synthetic");
        assert_eq!(std::fs::read(root.join(".plugin.acme.weather/keys/test.json")).unwrap(),b"synthetic");
        process.stop();assert!(!process.running());drop(process);
        let mut runner=crate::plugin_runtime::Runner::default();
        for _ in 0..10 {
            runner.set_enabled(&manager,"acme.weather",true,Some(&node)).unwrap();
            assert!(manager.list().unwrap()[0].enabled);
            assert!(runner.call("acme.weather","invalid",json!({})).is_err());
            assert!(runner.is_running("acme.weather"),"业务校验失败应保留插件实例");
            assert_eq!(runner.call("acme.weather","save",json!({})).unwrap(),"synthetic");
            runner.set_enabled(&manager,"acme.weather",false,Some(&node)).unwrap();
            assert!(!manager.list().unwrap()[0].enabled);
            assert!(runner.call("acme.weather","save",json!({})).is_err());
        }
        runner.set_enabled(&manager,"acme.weather",true,Some(&node)).unwrap();
        runner.stop_all();assert!(manager.list().unwrap()[0].enabled);
        runner.resume(&manager,Some(&node)).unwrap();
        // #38：Dock 运行中拨「随 Dock 启动」要当场生效——关闭即停、打开即起，且 3 秒内，
        // 不需要退出重进 Dock。
        runner.set_autostart(&manager,"acme.weather",false,Some(&node)).unwrap();
        assert!(!runner.is_running("acme.weather"),"关闭自启动要当场停掉服务");
        let started=std::time::Instant::now();
        runner.set_autostart(&manager,"acme.weather",true,Some(&node)).unwrap();
        assert!(runner.is_running("acme.weather"),"拨到随 Dock 启动要当场把服务拉起来");
        assert!(started.elapsed()<std::time::Duration::from_secs(3),"自启动拉起要在 3 秒内完成");
        let ns=manager.namespace(&row).unwrap();ns.write("config","prior.json",b"synthetic-original").unwrap();
        let broken=b"import {createPlugin} from './sdk.mjs';createPlugin({id:'acme.weather',activate:async c=>{await c.storage.write('config','prior.json','changed');await c.storage.write('keys','introduced.json','synthetic-update');throw Error('synthetic activation failure')}});".to_vec();
        let mut upgraded=row.manifest.clone();upgraded.version="1.1.0".into();
        let mut files=BTreeMap::from([("backend.mjs".to_string(),broken),("sdk.mjs".to_string(),sdk.clone()),("README.md".to_string(),b"# synthetic 1.1.0".to_vec())]);
        upgraded.files=files.iter().map(|(key,value)|(key.clone(),format!("{:x}",Sha256::digest(value)))).collect();
        files.insert("plugin.json".into(),serde_json::to_vec(&upgraded).unwrap());
        let bytes=crate::identity_archive::zip(&files).unwrap();let digest=format!("{:x}",Sha256::digest(&bytes));
        let package=crate::plugin_package::inspect(&bytes,&digest,&root,crate::storage::Source::ThirdParty,&crate::plugin_install::platform()).unwrap();
        assert!(runner.install(&manager,package,&digest,false,Some(&node)).is_err());
        assert_eq!(manager.list().unwrap()[0].manifest.version,"1.0.0");
        assert_eq!(ns.read("config","prior.json").unwrap().unwrap(),b"synthetic-original");
        assert!(ns.read("keys","introduced.json").unwrap().is_none());
        assert!(std::fs::read_to_string(ns.path().join("README.md")).unwrap().contains("1.0.0"));
        assert_eq!(runner.call("acme.weather","save",json!({})).unwrap(),"synthetic");
        runner.stop_all();
        let config_bytes=crate::config_backup::export(&context.root).unwrap();
        let restored_context=crate::storage_context::Context::open(root.parent().unwrap().join("restored-config"),env!("CARGO_PKG_VERSION")).unwrap();
        crate::config_backup::restore(&restored_context.root,&config_bytes,||Ok(())).unwrap();
        let restored_manager=Manager{context:&restored_context};let mut repaired=crate::plugin_runtime::Runner::default();
        assert_eq!(repaired.view(&restored_manager).unwrap()[0]["packageMissing"],true);
        let old=manager.list().unwrap().remove(0);let directory=manager.directory(&old).unwrap();
        let mut package_files=BTreeMap::new();
        for name in old.manifest.files.keys().map(String::as_str).chain(std::iter::once("plugin.json")){package_files.insert(name.into(),std::fs::read(directory.join(name)).unwrap());}
        repaired.install(&restored_manager,crate::plugin_package::Package{manifest:old.manifest.clone(),files:package_files},&old.digest,false,Some(&node)).unwrap();
        assert!(repaired.is_running("acme.weather"));
        assert_eq!(restored_manager.namespace(&old).unwrap().read("config","prior.json").unwrap().unwrap(),b"synthetic-original");
        repaired.stop_all();
        runner.uninstall(&manager,"acme.weather").unwrap();assert!(manager.list().unwrap().is_empty());
        assert_eq!(ns.read("keys","test.json").unwrap().unwrap(),b"synthetic");
        std::fs::remove_dir_all(root.parent().unwrap()).unwrap();
    }
    #[test]fn callbacks_are_bound_to_capability_category_and_namespace(){
        let root=std::env::temp_dir().join(format!("fish-callback-{}-{}",std::process::id(),crate::now_ms())).join(".plugin.acme.weather");
        crate::storage::initialize(&root,"acme.weather","1.0.0",1).unwrap();
        let ns=Namespace{root:root.clone(),identity:crate::storage_context::Identity{plugin_id:"acme.weather".into(),directory:".plugin.acme.weather".into(),plugin_version:"1.0.0".into(),data_schema:1}};
        let caps=vec!["storage".into()];let params=json!({"category":"keys","name":"fixture.json","value":"synthetic"});
        assert!(storage_call(&ns,&[],"host.storage.write",&params).is_err());assert!(!root.join("keys/fixture.json").exists());
        storage_call(&ns,&caps,"host.storage.write",&params).unwrap();assert_eq!(storage_call(&ns,&caps,"host.storage.read",&params).unwrap()["value"],"synthetic");
        for name in ["../escape","C:/foreign","README.md"]{assert!(storage_call(&ns,&caps,"host.storage.write",&json!({"category":"keys","name":name,"value":"synthetic"})).is_err());}
        assert!(storage_call(&ns,&caps,"host.exec",&params).is_err());
        #[cfg(windows)] {
            let secret=json!({"category":"credentials","name":"robot.dpapi","value":"synthetic-private-value"});
            storage_call(&ns,&caps,"host.storage.writeSecret",&secret).unwrap();
            let encrypted=std::fs::read(root.join("credentials/robot.dpapi")).unwrap();
            assert!(!String::from_utf8_lossy(&encrypted).contains("synthetic-private-value"));
            assert_eq!(storage_call(&ns,&caps,"host.storage.readSecret",&secret).unwrap()["value"],"synthetic-private-value");
            assert!(storage_call(&ns,&caps,"host.storage.writeSecret",&json!({"category":"config","name":"no","value":"secret"})).is_err());
        }
        std::fs::remove_dir_all(root.parent().unwrap()).unwrap();
    }
    // A2（2.1.6）：宿主不在「等这个插件自己的响应」时，插件发来的宿主调用也必须当场被处理。
    // 旧实现把回调帧塞进 output，只有 request/handshake 在等响应时才顺手服务；自启流程一结束
    // 就没有任何线程读它，插件的 storage 调用只能干等到 SDK 超时，自启动必然失败。
    #[test]
    #[ignore = "需 FISH_DOCK_TEST_NODE；验证空闲窗口里的回调"]
    fn a_callback_is_served_while_no_request_is_in_flight() {
        let node=std::env::var_os("FISH_DOCK_TEST_NODE").map(std::path::PathBuf::from).expect("设置 FISH_DOCK_TEST_NODE");
        let root=std::env::temp_dir().join(format!("fish-idle-call-{}-{}",std::process::id(),crate::now_ms())).join(".fish-dock");
        let host=t29_namespace(root.join("host"),"host","host","1.0.0");
        t29_namespace(root.join(".plugin.acme.weather"),"acme.weather",".plugin.acme.weather","1.0.0");
        let context=crate::storage_context::Context{root:root.clone(),host};let manager=Manager{context:&context};
        // activate 返回 60 ms 之后（宿主这边已经没有在途请求）再写一次存储。
        let backend=b"import {createPlugin} from './sdk.mjs';createPlugin({id:'acme.weather',activate:async context=>{setTimeout(()=>{void context.storage.write('config','late.json','served')},60)}});".to_vec();
        let sdk=include_bytes!("../../../sdk/node/plugin.mjs").to_vec();
        let mut files=BTreeMap::from([("backend.mjs".to_string(),backend),("sdk.mjs".to_string(),sdk),("README.md".to_string(),b"# synthetic".to_vec())]);
        let hashes:BTreeMap<_,_>=files.iter().map(|(key,value)|(key.clone(),format!("{:x}",Sha256::digest(value)))).collect();
        let raw=json!({"schema":1,"id":"acme.weather","name":"合成天气","version":"1.0.0","hostApi":1,"platforms":[crate::plugin_install::platform()],"storage":{"directory":".plugin.acme.weather","dataSchema":1},"backend":{"runtime":"node","entry":"backend.mjs"},"capabilities":["storage"],"files":hashes});
        files.insert("plugin.json".into(),serde_json::to_vec(&raw).unwrap());
        let bytes=crate::identity_archive::zip(&files).unwrap();let digest=format!("{:x}",Sha256::digest(&bytes));
        let package=crate::plugin_package::inspect(&bytes,&digest,&root,crate::storage::Source::ThirdParty,&crate::plugin_install::platform()).unwrap();
        let row=manager.install(package,&digest,false).unwrap();
        let process=Process::start(&manager,&row,Some(&node),false).unwrap();
        let ns=manager.namespace(&row).unwrap();
        let deadline=std::time::Instant::now()+Duration::from_secs(3);
        let mut served=false;
        while std::time::Instant::now()<deadline {
            if ns.read("config","late.json").unwrap().is_some(){served=true;break}
            std::thread::sleep(Duration::from_millis(25));
        }
        drop(process);
        std::fs::remove_dir_all(root.parent().unwrap()).ok();
        assert!(served,"activate 返回后发出的宿主调用，在没有在途请求时也必须当场被服务（A2）");
    }
    // t38/B1：新包已落地但服务起不来 → 注册表回滚到 1.0.0 之外，packages/<id>/ 也只
    // 留旧版本（失败的新版本目录一并删掉），旧服务仍在运行。
    #[test]
    #[ignore = "需 FISH_DOCK_TEST_NODE；B1 回滚目录终态"]
    fn t38_b1_failed_start_removes_the_new_version_directory() {
        let node=std::env::var_os("FISH_DOCK_TEST_NODE").map(std::path::PathBuf::from).expect("设置 FISH_DOCK_TEST_NODE");
        let root=std::env::temp_dir().join(format!("fish-t38-b1-{}-{}",std::process::id(),crate::now_ms())).join(".fish-dock");
        let host=t29_namespace(root.join("host"),"host","host","1.0.0");
        t29_namespace(root.join(".plugin.acme.weather"),"acme.weather",".plugin.acme.weather","1.0.0");
        let context=crate::storage_context::Context{root:root.clone(),host};
        let manager=Manager{context:&context};
        let mut runner=crate::plugin_runtime::Runner::default();
        let (old,digest)=t29_package("acme.weather","Weather",".plugin.acme.weather","1.0.0",false);
        manager.install(old,&digest,false).unwrap();
        runner.set_enabled(&manager,"acme.weather",true,Some(&node)).unwrap();
        assert!(runner.is_running("acme.weather"),"旧服务应先跑起来");
        // 新版本：native 后端指向一个不存在的 exe —— manager.install 会成功落地，
        // Process::start 立刻失败（不用等握手超时）。
        let readme=b"# broken 2.0.0".to_vec();
        let mut files=BTreeMap::from([("README.md".to_string(),readme)]);
        let hashes:BTreeMap<_,_>=files.iter().map(|(key,value)|(key.clone(),format!("{:x}",Sha256::digest(value)))).collect();
        let raw=json!({"schema":1,"id":"acme.weather","name":"Weather","version":"2.0.0","hostApi":1,"platforms":[crate::plugin_install::platform()],"storage":{"directory":".plugin.acme.weather","dataSchema":1},"backend":{"runtime":"native","entry":"README.md"},"behaviors":{"autostart":true},"files":hashes});
        files.insert("plugin.json".into(),serde_json::to_vec(&raw).unwrap());
        let bytes=crate::identity_archive::zip(&files).unwrap();let new_digest=format!("{:x}",Sha256::digest(&bytes));
        let package=crate::plugin_package::inspect(&bytes,&new_digest,&root,crate::storage::Source::ThirdParty,&crate::plugin_install::platform()).unwrap();
        assert!(runner.install(&manager,package,&new_digest,false,Some(&node)).is_err(),"新版本服务起不来应报错并回滚");
        let rows=manager.list().unwrap();
        assert_eq!(rows.iter().find(|row|row.manifest.id=="acme.weather").unwrap().manifest.version,"1.0.0","注册表回滚到旧版");
        let mut dirs:Vec<String>=std::fs::read_dir(root.join("host/packages/acme.weather")).unwrap().flatten().filter(|e|e.file_type().map(|t|t.is_dir()).unwrap_or(false)).map(|e|e.file_name().to_string_lossy().to_string()).collect();
        dirs.sort();
        assert_eq!(dirs,vec!["1.0.0".to_string()],"回滚后只留旧版本目录，2.0.0 必须删掉");
        assert!(runner.is_running("acme.weather"),"旧服务保持 running");
        let _=std::fs::remove_dir_all(root.parent().unwrap());
    }
    // #22：只有「随 Dock 启动」那一次是 1；安装、点启用、回滚都是 0。插件靠这个
    // 值决定要不要自己把本机服务拉起来，写反了就是每次启用都顺带起服务。
    #[test] fn autostart_flag_is_one_only_when_started_with_the_dock() {
        assert_eq!(autostart_flag(true),"1");
        assert_eq!(autostart_flag(false),"0");
    }
    // #53-1/2：stderr 落盘。脱敏照旧（令牌不落明文），但中文报错与 JSON 的 msg 必须
    // 留得住——Redactor 的公开报告口径会把它们换成 ‹…›，所以 shield/restore 先给
    // 引号与非 ASCII 词让路。单行截断，1 MB 只留 2 份。
    #[test] fn plugin_stderr_is_redacted_clipped_and_rolled_in_two_files() {
        let root=std::env::temp_dir().join(format!("fish-stderr-{}-{}",std::process::id(),crate::now_ms()));
        let log=root.join("host/plugin-logs/fish.clash.log");
        let redactor=crate::crash::Redactor::here(&root,&[]);
        let line=stderr_line(&redactor,"fish.clash","console.error: 内核启动失败 token=ghp_abcdefghijklmnopqrstuvwxyz0123456789\n");
        assert!(line.contains("console.error"),"console.error 要查得到：{line}");
        assert!(line.contains("fish.clash"),"每行带插件 id：{line}");
        assert!(line.contains("内核启动失败"),"中文报错不许被整段抹掉：{line}");
        assert!(!line.contains("ghp_abcdefghijklmnopqrstuvwxyz0123456789"),"令牌不许落明文：{line}");
        assert!(line.contains("<token>"),"{line}");
        // 未捕获异常（node 打到 stderr 的原样文本）也要能查到栈位置。
        let thrown=stderr_line(&redactor,"fish.clash","Uncaught Error: synthetic failure at backend.mjs:12:5");
        assert!(thrown.contains("Uncaught Error")&&thrown.contains("backend.mjs:12:5"),"{thrown}");
        // JSON 日志行（引擎侧常见）：引号与 msg 都在，不塌成 ‹…›。
        let json=stderr_line(&redactor,"fish.clash","{\"level\":\"error\",\"msg\":\"连接失败\"}");
        assert!(json.contains("\"level\":\"error\"")&&json.contains("连接失败"),"{json}");
        // 单行截断：4000 字符的一行只留 ~300 字符（redact.rs MAX_LINE）。
        let long=stderr_line(&redactor,"fish.clash",&"x".repeat(4000));
        assert!(long.chars().count()<=512,"单行必须截断：{}",long.chars().count());
        // 1 MB × 2 滚动：写 3 MB 之后只剩两块文件、总量 ≤ 2 MB。
        let mut sink=Log::open(&log,LOG_LIMIT,LOG_KEEP).unwrap();
        for _ in 0..(3*1024) { sink.line(&"y".repeat(1023)); }
        let folder=log.parent().unwrap();
        let mut names:Vec<String>=std::fs::read_dir(folder).unwrap().flatten().map(|entry|entry.file_name().to_string_lossy().to_string()).collect();
        names.sort();
        let total:u64=std::fs::read_dir(folder).unwrap().flatten().map(|entry|entry.metadata().map(|meta|meta.len()).unwrap_or(0)).sum();
        assert_eq!(names,vec!["fish.clash.log".to_string(),"fish.clash.log.1".to_string()],"只留当前 + 上一份：{names:?}");
        assert!(total<=2*LOG_LIMIT,"滚动后总量要 ≤ 2 MB，实际 {total}");
        std::fs::remove_dir_all(&root).unwrap();
    }
    // shield/restore 不许把普通行改样：短句子原样进、原样出（只有 Redactor 会动它）。
    #[test] fn shield_and_restore_round_trip_ordinary_text() {
        for text in ["plugin ready on port 9090","{\"ok\":true}","配置无效，请重试","mixed 中英 混排 ok"] {
            let (shielded,held)=shield(text);
            assert!(shielded.is_ascii(),"占位之后必须是纯 ASCII：{shielded}");
            assert_eq!(restore(&shielded,&held),text);
        }
    }
    // —— t18′（2.0.4 T-E）：重试搬到调度器；进程层一次调用只尝试一次 ——
    // 冷态「重试一次」的旧行为（进程内退避 + 重拉）已由 plugin_start 的统一调度取代：
    // 进程层不再自己重试，避免「重试次数 × 阶段预算」的乘法（A02）。这里锁定新契约：
    // 一次 Process::start 只尝试一次、不退避，错误要能说清卡在哪一步。
    #[test] fn a_process_start_is_one_bounded_attempt_without_the_old_cold_sleep() {
        let root=std::env::temp_dir().join(format!("fish-one-attempt-{}-{}",std::process::id(),crate::now_ms())).join(".fish-dock");
        let host=t29_namespace(root.join("host"),"host","host","1.0.0");
        t29_namespace(root.join(".plugin.acme.weather"),"acme.weather",".plugin.acme.weather","1.0.0");
        let context=crate::storage_context::Context{root:root.clone(),host};
        let manager=Manager{context:&context};
        let (package,digest)=t29_package("acme.weather","Weather",".plugin.acme.weather","1.0.0",false);
        let row=Installed{manifest:package.manifest,digest,official:false,enabled:true,autostart:true};
        assert!(manager.verify(&row).is_err(),"夹具必须注定失败——这一版还没装进 packages/");
        let before=start_attempts();
        let started=Instant::now();
        let error=Process::start(&manager,&row,None,true).err().expect("注定失败的 start 必须报错");
        assert_eq!(start_attempts()-before,1,"一次 Process::start 只尝试一次（重试归调度器；before={before} after={}）",start_attempts());
        assert!(started.elapsed()<Duration::from_millis(400),"进程层不再自己退避 500 ms：{:?}",started.elapsed());
        assert!(error.contains("校验阶段"),"错误要说清卡在哪一步：{error}");
        let _=std::fs::remove_dir_all(root.parent().unwrap());
    }
    // t33/F4：清单/摘要类校验失败是永久错误（不重试），错误结构里要说清阶段与 permanent。
    #[test] fn a_missing_package_is_a_permanent_verify_failure() {
        let root=std::env::temp_dir().join(format!("fish-permanent-{}-{}",std::process::id(),crate::now_ms())).join(".fish-dock");
        let host=t29_namespace(root.join("host"),"host","host","1.0.0");
        t29_namespace(root.join(".plugin.acme.weather"),"acme.weather",".plugin.acme.weather","1.0.0");
        let context=crate::storage_context::Context{root:root.clone(),host};
        let manager=Manager{context:&context};
        let (package,digest)=t29_package("acme.weather","Weather",".plugin.acme.weather","1.0.0",false);
        let row=Installed{manifest:package.manifest,digest,official:false,enabled:true,autostart:true};
        let error=Process::start_with_budget(&manager,&row,None,true,Duration::from_secs(5)).err().expect("没装进 packages/ 必须失败");
        assert_eq!(error.phase,crate::plugin_start::Phase::Verify);
        assert!(error.permanent,"清单/资源缺失属于永久错误，不该重试：{error:?}");
        assert!(error.text().contains("校验阶段"),"{}",error.text());
        let _=std::fs::remove_dir_all(root.parent().unwrap());
    }
    // A02：hello 与 activate 共用一个 deadline——hello 花掉 25 s，activate 只剩 35 s。
    #[test] fn hello_and_activate_share_one_deadline() {
        let base=Instant::now();
        let deadline=base+Duration::from_secs(60);
        let simulated=std::cell::Cell::new(0u64);
        let mut calls:Vec<(String,Duration)>=Vec::new();
        let mut stages=Stages::default();
        handshake_with("acme.weather",json!({"protocol":1,"pluginId":"acme.weather"}),deadline,||base+Duration::from_secs(simulated.get()),&mut stages,|method,_params,timeout|{
            calls.push((method.to_string(),timeout));
            simulated.set(simulated.get()+25);
            Ok(if method=="host.hello"{json!({"type":"plugin.hello","protocol":1,"pluginId":"acme.weather"})}else{json!({"active":true})})
        }).expect("合成握手必须成功");
        assert_eq!(calls[0],("host.hello".to_string(),Duration::from_secs(60)),"hello 拿整份预算");
        assert_eq!(calls[1],("plugin.activate".to_string(),Duration::from_secs(35)),"activate 只剩 hello 之后的时间");
        assert_eq!(stages.hello_ms,25_000);
        assert_eq!(stages.activate_ms,25_000);
    }
    // A02：hello 把预算花完 → 不许再发 activate，且报「哪一步没时间了」。
    #[test] fn a_hello_that_eats_the_budget_leaves_no_time_to_activate() {
        let base=Instant::now();
        let deadline=base+Duration::from_secs(10);
        let simulated=std::cell::Cell::new(0u64);
        let mut stages=Stages::default();
        let error=handshake_with("acme.weather",json!({"protocol":1,"pluginId":"acme.weather"}),deadline,||base+Duration::from_secs(simulated.get()),&mut stages,|method,_params,_timeout|{
            simulated.set(simulated.get()+10);
            Ok(if method=="host.hello"{json!({"type":"plugin.hello","protocol":1,"pluginId":"acme.weather"})}else{json!({"active":true})})
        }).expect_err("预算用完不许再发 activate");
        assert_eq!(error.phase,crate::plugin_start::Phase::Activate);
        assert!(error.text().contains("预算"),"{}",error.text());
        assert!(!error.permanent,"超时是瞬时错误，可重试");
    }
    // A04：身份/协议不匹配是永久错误，调度器不会重试。
    #[test] fn an_identity_mismatch_is_a_permanent_failure() {
        let base=Instant::now();
        let deadline=base+Duration::from_secs(10);
        let mut stages=Stages::default();
        let error=handshake_with("acme.weather",json!({"protocol":1,"pluginId":"acme.weather"}),deadline,Instant::now,&mut stages,|_method,_params,_timeout|{
            Ok(json!({"type":"plugin.hello","protocol":1,"pluginId":"someone.else"}))
        }).expect_err("身份不匹配必须失败");
        assert_eq!(error.phase,crate::plugin_start::Phase::Hello);
        assert!(error.permanent,"身份协议不匹配是永久错误");
    }
    // A03：插件没有 plugin.health → 业务服务状态「未知」，绝不当成健康。
    #[test] fn a_service_without_a_health_method_is_unknown_not_healthy() {
        let service=probe_service_with(Instant::now()+Duration::from_secs(5),Instant::now,|_method,_params,_timeout|{
            Err(crate::plugin_protocol::business_error("UNKNOWN_METHOD","插件方法未注册"))
        });
        assert_eq!(service.as_str(),"unknown");
        assert!(service.detail().unwrap().contains("plugin.health"),"{:?}",service.detail());
    }
    // A03：服务在预算内就绪 → ready，且每次「未就绪」之间会等一小段再看。
    #[test] fn a_service_that_becomes_ready_inside_the_budget_passes() {
        let deadline=Instant::now()+Duration::from_secs(5);
        let mut calls=0;
        let service=probe_service_with(deadline,Instant::now,|_method,_params,_timeout|{
            calls+=1;
            Ok(if calls<3{json!({"ready":false,"detail":"还在起"})}else{json!({"ready":true})})
        });
        assert_eq!(service,Service::Ready);
        assert_eq!(calls,3);
    }
    // A03：预算内一直没就绪 → down（这次尝试按业务服务阶段失败，交给调度器重试）。
    #[test] fn a_service_never_ready_fails_as_down() {
        let deadline=Instant::now()+Duration::from_millis(600);
        let service=probe_service_with(deadline,Instant::now,|_method,_params,_timeout|{
            Ok(json!({"ready":false,"detail":"永远不"}))
        });
        assert_eq!(service.as_str(),"down","{}",service.detail().unwrap_or(""));
    }
}
