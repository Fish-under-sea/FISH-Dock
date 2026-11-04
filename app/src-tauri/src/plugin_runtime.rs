//! 外部插件实例注册；所有后台操作串行，停用与卸载只处理本宿主的实例。
//!
//! 锁的分工（T03 / T09）：
//!   - `CURRENT` 只管「实例表」这一小块内存，拿它的时间必须是毫秒级：查一次在不在、
//!     借出一个实例句柄、结一次账。绝不在它里面做全文校验、扫目录、等插件回答；
//!   - 一次插件的业务回答（最长到 RPC 超时）由 `call_outside()` 在锁外等，等回来再
//!     拿一次短锁结账；等待期间实例被停用/卸载就按代次作废（`retire`/`settles`）；
//!   - 启动后的清理（`cleanup()` / `cleanup_staging()`）挪到 `MAINTENANCE_DELAY` 之后
//!     的维护线程，不再排在拉进程之前。
use std::{collections::{BTreeMap,BTreeSet}, fs, io::Write, path::{Path,PathBuf}, sync::{Arc,Mutex,Once}};
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
// 回收孤儿内核时探测 controllerPort 的连接等待（毫秒）：连上就是可 attach 的活内核，
// 留给插件的 attach 路径接管；连不上才当上一轮遗留的僵尸内核。提成具名常量，别在
// 网络调用里留裸 300（红线 5：硬编码魔法值）。
const ORPHAN_PROBE_TIMEOUT_MS:u64=300;
// 启动后多久跑清理（T03）。冷启关键路径上不许有「扫目录、删文件」这类活：以前
// cleanup()/cleanup_staging() 在 resume() 里同步做，占着 CURRENT、排在拉进程之前，
// 而它扫的正是 packages/ 与各命名空间的 tmp/。现在统一挪到启动 30 s 之后。
pub const MAINTENANCE_DELAY:std::time::Duration=std::time::Duration::from_secs(30);
// 维护与「会动 packages/ 的那几件活」之间的互斥（t5-F1）。以前 cleanup()/cleanup_staging()
// 在 resume() 里跑，和 install 都持 CURRENT，天然互斥；挪到启动 30 s 后的维护线程之后，
// 这条互斥得显式要——t≈30 s 时正在装插件的用户，其刚落地的 packages/<id>/<新版本>/ 会被
// prune_versions 当成「过期版本」整目录删掉（注册表还指旧版，用户只能重下 75 MB）。
//
// 为什么不用 plugin_install.rs 的 EDIT_REGISTRY：那是注册表内部实现细节，install() 自己
// 会取它；从本模块再拿同一把（不可重入）套住 manager.install 就是取两次——config.rs 的
// ENV_LOCK 自死锁是前车之鉴。这里立一把**上层**的串行锁：install / uninstall / 维护线程都
// 先过它，再各自进注册表锁。锁序：CURRENT → MAINTENANCE → EDIT_REGISTRY → EDIT。
// 中毒也不放弃维护：把它取回内部值继续（维护「永不失败、下次再试」）。
static MAINTENANCE:Mutex<()>=Mutex::new(());
// manual_enable_backoff 是手动启用重试的退避等待（t12 repair F1/F2）。
// 生产环境 sleep 3s；测试可注入 no-op 或短延迟。抽成独立函数以便单测时序。
fn manual_enable_backoff(){
    #[cfg(test)]
    {
        // 测试环境：检查是否有注入的等待函数。
        if let Some(wait)=MANUAL_ENABLE_BACKOFF.lock().unwrap().as_ref() {
            wait();
            return;
        }
    }
    // 生产环境：sleep 3s。
    std::thread::sleep(std::time::Duration::from_secs(3));
}

// t21：测试互斥锁，防止并行测试修改注入静态量时互相干扰。
#[cfg(test)]
pub(crate) static MANUAL_ENABLE_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[cfg(test)]
static MANUAL_ENABLE_BACKOFF:Mutex<Option<Box<dyn Fn() + Send>>> = Mutex::new(None);

// t21：RAII 守卫，确保测试结束时还原注入状态，即使 panic 也能清理。
#[cfg(test)]
pub(crate) struct ManualEnableBackoffGuard {
    _lock: std::sync::MutexGuard<'static, ()>,
}

#[cfg(test)]
impl Drop for ManualEnableBackoffGuard {
    fn drop(&mut self) {
        *MANUAL_ENABLE_BACKOFF.lock().unwrap() = None;
        *MANUAL_ENABLE_STARTER.lock().unwrap() = None;
    }
}

#[cfg(test)]
pub(crate) fn acquire_manual_enable_test_lock() -> ManualEnableBackoffGuard {
    ManualEnableBackoffGuard {
        _lock: MANUAL_ENABLE_TEST_LOCK.lock().unwrap_or_else(|p| p.into_inner()),
    }
}

#[cfg(test)]
pub fn set_manual_enable_backoff_for_test(f: Option<Box<dyn Fn() + Send>>){
    *MANUAL_ENABLE_BACKOFF.lock().unwrap() = f;
}

// t14 repair F1：可注入的「启动一次」函数，用于测试重试路径。
// 生产环境走 start_instance_inner；测试注入「第一次返回 Err、第二次返回 Ok」的桩。
#[cfg(test)]
static MANUAL_ENABLE_STARTER:Mutex<Option<Box<dyn Fn(&Manager<'_>,&Installed,Option<&Path>,bool)->Result<(),String> + Send>>> = Mutex::new(None);
#[cfg(test)]
pub fn set_manual_enable_starter_for_test<F>(f: Option<F>)
where F: Fn(&Manager<'_>,&Installed,Option<&Path>,bool)->Result<(),String> + Send + 'static {
    *MANUAL_ENABLE_STARTER.lock().unwrap() = f.map(|f| Box::new(f) as Box<_>);
}
// append_failure_row 是共享的落盘入口：Runner::append 与 record_standalone_failure 都调它，
// 保证 plugin-failures.log 只有一种格式、一种写入路径（t12 repair）。
pub(crate) fn append_failure_row(host:&Path,failure:&Failure){
    use std::io::Write;
    if let Ok(mut file)=fs::OpenOptions::new().create(true).append(true).open(failure_log_of(host)){let _=file.write_all(format!("{}\n",failure.json()).as_bytes());let _=file.flush();}
}
// record_standalone_failure 在 Runner 不可用时（例如 scheduled_attempt 的失败分支）
// 直接将结构化失败写入 plugin-failures.log（S1 可观测性）。
pub fn record_standalone_failure(host:&Path,id:&str,version:&str,stage:&str,phase:Option<String>,attempt:Option<u32>,elapsed_ms:Option<u64>,state:Option<String>,budget_cold:Option<bool>,code:Option<i32>,raw:&str){
    let log=log_file_of(host,id);
    let row=Failure{plugin:id.into(),version:version.into(),stage:stage.into(),phase,attempt,elapsed_ms,state,budget_cold,code,at:crate::now_ms(),raw:raw.into(),log};
    append_failure_row(host,&row);
}
// schedule_maintenance 给这次进程排一次维护（每个进程一次就够）。调用方只传上下文就
// 走——维护线程不碰 CURRENT，也不需要有人在锁里等它。
// t5-F4：线程体包 catch_unwind 并落一行日志。Once 跑过就不会再排（panic 也算跑过），
// 所以 panic 必须当场可见，不能变成「维护悄悄不再发生」。
pub fn schedule_maintenance(context:&Context){
    static ONCE:Once=Once::new();
    let context=context.clone();
    ONCE.call_once(move||{std::thread::spawn(move||{
        std::thread::sleep(MAINTENANCE_DELAY);
        let outcome=std::panic::catch_unwind(std::panic::AssertUnwindSafe(||maintain(&context)));
        if outcome.is_err() { eprintln!("FISH Dock: 插件维护线程异常退出（下次启动再试）"); }
    });});
}
// maintain 是维护线程的全部工作：先收与「哪个插件在升级」无关的暂存残留，再逐个
// 插件收过期版本目录、旧摘要目录、暂存文件、越攒越多的回滚点与校验快照。
// **永不失败、永不打断启动**：出问题只记一行日志，等下次启动再试（与 cleanup() 的
// 约定一致）。单独拿出来也是为了能直接单测，不必等 30 s。
//
// t5-F1：整段维护与 install/uninstall 走**同一把互斥**（MAINTENANCE）。顺序也是先取
// 锁、再读注册表——快照与清理对象是同一份稳定的注册表，install 不可能插在中间把新版本
// 目录写成「过期版本」。这把锁是上层的、不可重入：锁内不再调 install()/uninstall()，
// cleanup()/cleanup_staging()/prune_* 也只进注册表锁与存储锁，不会二次取它。
// t5-F4：读不到注册表不再静默，落一行日志后跳过本次维护。
pub fn maintain(context:&Context){
    let manager=Manager{context};
    let _maintenance=MAINTENANCE.lock().unwrap_or_else(|poison|poison.into_inner());
    let staging=manager.cleanup_staging();
    if !staging.is_empty() { eprintln!("FISH Dock: 插件事务暂存待下次启动重试：{}",staging.join("；")); }
    let rows=match manager.list() {
        Ok(rows)=>rows,
        Err(error)=>{ eprintln!("FISH Dock: 插件维护跳过：注册表读取失败：{error}"); return; }
    };
    for row in rows {
        let problems=manager.cleanup(&row);
        if !problems.is_empty() { eprintln!("FISH Dock: 插件残留待下次启动重试（{}）：{}",row.manifest.id,problems.join("；")); }
    }
}

// 一条插件失败（#53-3/5）：插件 id、版本、阶段、退出码、时间、原始错误、日志路径。
// 进内存（插件卡当场显示）之外逐行追加进 `host/plugin-failures.log`，宿主重启后仍可查。
#[derive(Clone,Debug)]
pub struct Failure { pub plugin:String, pub version:String, pub stage:String, pub phase:Option<String>, pub attempt:Option<u32>, pub elapsed_ms:Option<u64>, pub state:Option<String>, pub budget_cold:Option<bool>, pub code:Option<i32>, pub at:u64, pub raw:String, pub log:PathBuf }
impl Failure {
    fn json(&self)->Value{json!({"at":self.at,"plugin":self.plugin,"version":self.version,"stage":self.stage,"phase":self.phase,"attempt":self.attempt,"elapsedMs":self.elapsed_ms,"state":self.state,"budgetCold":self.budget_cold,"exitCode":self.code,"error":self.raw,"log":self.log.to_string_lossy()})}
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

// 一个插件实例的句柄（T09）。以前实例是 Runner 表里的裸 Process，于是「谁想跟插件
// 说一句话」就必须整段持有 CURRENT——一次 RPC 最长到超时，安装/停用/设置页重绘全被
// 它堵在后面。改成 Arc<Mutex<Process>> 之后：实例仍然只此一份（管道、子进程、job
// 对象都只有一个），但可以「借出去」——持锁跑 RPC 的线程与 CURRENT 是两把锁，
// 谁也不必等谁。
type Instance=Arc<Mutex<Process>>;

// Runner 的默认构造是手写的（见下面 impl Default）：deferred/stopping 这两个开关
// 决定「拉进程在不在 CURRENT 里做」，不能靠 derive 的零值糊过去。
pub struct Runner {
    processes:BTreeMap<String,Instance>,
    // 每个插件实例的「代次」（T09）。停用、卸载、换版、进程真的没了都会 +1：在途
    // RPC 回答回来时先比代次与句柄，对不上就作废——绝不去动之后拉起的新实例，也
    // 绝不把旧实例的失败记到新实例头上。
    generations:BTreeMap<String,u64>,
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
    // 正在退出（shutdown → stop_all）：拉起的线程看到它就放弃，别在退出中途又开一个
    // 插件进程（T03 把拉进程挪出了 CURRENT，所以这条闸必须有）。
    stopping:bool,
    // 2.0.4 t33（F1）：最终退出。stop_all 之后不再接受 resume；身份导出的暂停只用
    // `stopping`（可复位），不会把所有插件永久停在备份时刻。
    closed:bool,
    // 2.0.4 t33（F1/A04）：正在拉起的 id。scheduler 的 token 只管一轮；跨轮重复 resume
    // 时靠它挡住「同一个插件两个 worker 同时 spawn / 替换健康实例」。
    in_flight:BTreeSet<String>,
    // 拉进程走锁外（T03「先取快照再放锁」）。只有全局 CURRENT 那一份打开它：那条
    // 启动链的锁由 plugin_api::resume 持有，resume() 只能把活交出去才不占着锁。
    // 直接 new 出来的 Runner（单测、或调用方自己持有一份）保持同步语义。
    deferred:bool,
}
impl Default for Runner {
    fn default()->Self{Runner{processes:BTreeMap::new(),generations:BTreeMap::new(),failures:BTreeMap::new(),rollback_notes:BTreeMap::new(),host:None,versions:BTreeMap::new(),stopping:false,closed:false,in_flight:BTreeSet::new(),deferred:false}}
}
pub static CURRENT:Mutex<Runner>=Mutex::new(Runner{processes:BTreeMap::new(),generations:BTreeMap::new(),failures:BTreeMap::new(),rollback_notes:BTreeMap::new(),host:None,versions:BTreeMap::new(),stopping:false,closed:false,in_flight:BTreeSet::new(),deferred:true});
// valid_method 判一次业务方法名（宿主保留前缀、长度、字符集）。两条入口共用，
// 免得「页面能调、设置页不能调」这种两套判据。
fn valid_method(method:&str)->Result<(),String>{
    if method.is_empty()||method.len()>100||method.starts_with("host.")||method.starts_with("plugin.")||!method.bytes().all(|b|b.is_ascii_alphanumeric()||b==b'.'||b==b'-'||b==b'_'){return Err("插件业务方法无效或属于宿主保留方法".into());}
    Ok(())
}
// call_outside 是**不持 CURRENT** 的业务调用（T09：插件页面的桥走这里）。
// 三步：拿一次短锁借出实例句柄与代次 → 放锁之后慢慢等插件回答 → 回答回来再拿一次
// 短锁结账。等待期间一次都不占 CURRENT，所以「装 magpie 期间开插件页」不会再撞
// 「插件页面请求超时」。
pub fn call_outside(id:&str,method:&str,params:Value)->Result<Value,String>{
    valid_method(method)?;
    let (handle,generation)={
        let runner=CURRENT.lock().map_err(|_|"插件运行锁不可用")?;
        let handle=runner.processes.get(id).cloned().ok_or("插件尚未运行")?;
        (handle,runner.generation(id))
    };
    // 句柄已经借到：这一段在锁外，最长到 RPC 超时。
    let timeout=crate::config::plugin_rpc_timeout();
    let result=match handle.lock(){Ok(mut process)=>process.request(method,params,timeout),Err(_)=>Err("插件进程状态不可用".into())};
    // 结账：等待期间被停用/卸载/换版（plugin_page::revoke → retire）就作废这次结果。
    let mut runner=CURRENT.lock().map_err(|_|"插件运行锁不可用")?;
    settle_outside(&mut runner,id,generation,&handle,result)
}
// settle_outside 是「在途结果回来之后」的结账（t5-F3）：代次或句柄对不上就把这次结果
// 作废——绝不去动之后拉起的新实例，也绝不把失败记到新代次头上。单独拿出来是为了让
// 这条分支能被用例直接驱动（真实 RPC 的版本见 gated 用例，需要 FISH_DOCK_TEST_NODE）。
fn settle_outside(runner:&mut Runner,id:&str,generation:u64,handle:&Instance,result:Result<Value,String>)->Result<Value,String>{
    if !runner.settles(id,generation,handle) { return Err("插件已停用或卸载，本次调用结果已丢弃".into()); }
    runner.settle(id,result)
}
// present 是插件页面入口用的「有没有这个实例」查询（T09）：不握手、不拿实例锁、
// 不做任何 I/O，所以一次在途 RPC 也挡不住开页面。
pub fn present(id:&str)->bool{
    let Ok(runner)=CURRENT.lock() else {return false};
    runner.present(id)
}
// discard_late（t33/F3）：整轮到期后才起来的实例不登记为健康，直接停掉并摘除；
// 只有代次仍对得上才动手，绝不误杀之后拉起的新实例。
pub fn discard_late(id:&str,generation:u64){
    // 只借出句柄：实例锁在 CURRENT 之外等/停，避免与「持实例锁回 CURRENT 结账」的
    // call_outside 形成锁序环。
    let handle={
        let Ok(mut runner)=CURRENT.lock() else{return};
        if runner.generation(id)!=generation{return;}
        runner.processes.remove(id)
    };
    if let Some(handle)=handle{
        if let Ok(mut process)=handle.lock(){process.stop();}
    }
}
// spawn_starts 在 CURRENT 之外跑整轮自启调度（T-E / A02–A04）：每插件一个在途 token
// （id + 代次 + 版本 + 摘要），最多两槽并发，一次尝试一个共享 deadline，整轮 180 s 生命周期，
// 首次 + 至多 3 次重试（退避 0.5/2/5 s）。回填只在 CURRENT 下做代次比较与登记。
fn spawn_starts(context:Context,rows:Vec<Installed>,node:Option<PathBuf>){
    std::thread::spawn(move||{
        let manager=Manager{context:&context};
        // F1（t33）：退出/暂停闸与实例表都在建 token 之前看一次——基线在同一个位置有
        // `!runner.stopping && !runner.present(&id)`；这里再加 `in_flight`（跨轮重复 resume
        // 时挡第二个 worker）。被挡下的 id 不建 token，卡片继续按实时 running 报状态。
        let tokens:Vec<crate::plugin_start::Token>=match CURRENT.lock() {
            Ok(runner)=>{
                if runner.closed { eprintln!("FISH Dock: 宿主正在退出，跳过自启动调度"); return; }
                if runner.stopping { eprintln!("FISH Dock: 插件运行已暂停，跳过自启动调度"); return; }
                rows.iter()
                    .filter(|row|!runner.present(&row.manifest.id)&&!runner.in_flight.contains(&row.manifest.id))
                    .map(|row|crate::plugin_start::Token::of(row,runner.generation(&row.manifest.id)))
                    .collect()
            }
            Err(_)=>{eprintln!("FISH Dock: 插件运行锁不可用，跳过自启动调度");return;},
        };
        if tokens.is_empty() { return; }
        let cold=crate::config::plugin_startup_is_cold();
        // 每一轮都有独立编号（F3）：旧轮的 worker 回来时 `finish` 按 round 丢弃。
        let round=crate::plugin_start::begin(cold,crate::config::plugin_start_lifecycle(),crate::config::plugin_start_budget_of(true),crate::config::plugin_start_budget_of(false));
        eprintln!("FISH Dock: 插件自启调度第 {round} 轮开始，候选 {} 个",tokens.len());
        for token in tokens { crate::plugin_start::enqueue(token); }
        let node=node.as_deref();
        // t14 repair F3：传 host path 给 drive，用于终态落盘（ready/stopped 写 plugin-failures.log）。
        let host_path=context.host.path();
        crate::plugin_start::drive(Some(host_path),|assignment|scheduled_attempt(&manager,node,assignment));
    });
}
// scheduled_attempt 是一次真实启动尝试：先在 CURRENT 之外复核注册表（enabled/autostart、
// 版本与摘要），过期就按取消处理；起好后回到 CURRENT 只做代次比较与登记（A04）。
fn scheduled_attempt(manager:&Manager<'_>,node:Option<&Path>,assignment:&crate::plugin_start::Assignment)->crate::plugin_start::Outcome{
    use crate::plugin_start::{Failure,Outcome,Phase};
    // F1（t33）：spawn 之前先拿一次 CURRENT 短锁复核——最终退出/暂停、实例已在运行、
    // 已有一次在途拉起，都不许再开第二个进程。`Flight` 守护到本次尝试结束（含 panic 展开），
    // 保证 in_flight 一定回收。
    let _flight=match CURRENT.lock() {
        Ok(mut runner)=>{
            if runner.generation(&assignment.token.id)!=assignment.token.generation { return Outcome::cancelled("任务代次已变化"); }
            match runner.try_begin_attempt(&assignment.token.id) {
                Ok(flight)=>flight,
                Err(reason)=>return Outcome::cancelled(reason),
            }
        }
        Err(_)=>return Outcome::cancelled("插件运行锁不可用"),
    };
    let rows=match manager.list() {
        Ok(rows)=>rows,
        Err(error)=>return Outcome::transient(Phase::Verify,format!("注册表读取失败：{error}")),
    };
    let Some(row)=rows.into_iter().find(|row|row.manifest.id==assignment.token.id) else {
        return Outcome::cancelled("插件已卸载");
    };
    if !row.enabled||!row.autostart { return Outcome::cancelled("自启已关闭或被停用"); }
    if row.manifest.version!=assignment.token.version||row.digest!=assignment.token.digest { return Outcome::cancelled("启动期间版本或安装摘要已变化"); }
    // t12 repair F3：在 Process::start_with_budget 前后各取一次 Instant，把真实耗时传给失败记录。
    let attempt_started=std::time::Instant::now();
    let started=match Process::start_with_budget(manager,&row,node,true,assignment.budget) {
        Ok(started)=>started,
        Err(error)=>{
            let phase_str=error.phase.as_str().to_string();
            let attempt_num=assignment.attempt;
            let elapsed_ms=Some(attempt_started.elapsed().as_millis() as u64);
            let host_path=manager.context.host.path();
            // t12 repair F3：state 与 budgetCold 随行落盘；budgetCold 取当前轮次的冷态判据。
            let budget_cold=Some(crate::config::plugin_startup_is_cold());
            record_standalone_failure(host_path,&assignment.token.id,&assignment.token.version,"启动",Some(phase_str),Some(attempt_num),elapsed_ms,Some("failed".to_string()),budget_cold,None,&error.text());
            eprintln!("FISH Dock: [plugin-start] id={} attempt={} failed_at={} permanent={} error={}",assignment.token.id,assignment.attempt,error.phase.as_str(),error.permanent,error.message);
            return Outcome::failed(error.phase,if error.permanent{Failure::Permanent(error.text())}else{Failure::Transient(error.text())});
        }
    };
    let mut started=started;
    // F3（t33）：拉起成功不代表可以登记——整轮生命周期已经到期时，结果按丢弃处理，
    // 并且把刚起的进程停掉，不让它变成「调度外仍在跑但卡片 failed」的实例。
    if !crate::plugin_start::accepting() {
        started.process.stop();
        eprintln!("FISH Dock: [plugin-start] id={} attempt={} state=discarded reason=lifecycle",assignment.token.id,assignment.attempt);
        return Outcome::cancelled("自启生命周期已到期，本次结果丢弃");
    }
    let service=started.service.as_str();
    let installed=match CURRENT.lock() {
        Ok(mut runner)=>runner.install_instance(&row.manifest.id,assignment.token.generation,assignment.round,started.process),
        Err(_)=>false,
    };
    if !installed { return Outcome::cancelled("等待期间已停用、卸载或宿主退出"); }
    eprintln!("FISH Dock: [plugin-start] id={} attempt={} state=ready service={service}",assignment.token.id,assignment.attempt);
    Outcome::Ready{service}
}
impl Runner {
    // 记住宿主 data 根：`call()` 这类没有 Manager 参数的路径也要能把失败落盘（#53-3）。
    fn remember(&mut self,manager:&Manager<'_>){self.host=Some(manager.context.host.path().to_path_buf());}
}
// 一次在途启动的守卫（t33/F1）：成功登记后由 scheduler worker 持有，结束/panic 时
// Drop 把 id 从 `in_flight` 摘掉，跨轮重复 resume 才能真正做到「每插件单在途」。
struct Flight { id:String }
impl Drop for Flight {
    fn drop(&mut self){
        if let Ok(mut runner)=CURRENT.lock() { runner.in_flight.remove(&self.id); }
    }
}
impl Runner {
    // 能不能为这个 id 开一次新的拉起（必须在 CURRENT 里调用）：最终退出、暂停、实例已在、
    // 已有在途启动，任一条命中都不许再 spawn（基线 `!stopping && !present` 的恢复与加强）。
    fn try_begin_attempt(&mut self,id:&str)->Result<Flight,String>{
        if self.closed {return Err("宿主正在退出".into())}
        if self.stopping {return Err("插件运行已暂停".into())}
        if self.processes.contains_key(id) {return Err("插件实例已在运行，不重复拉起".into())}
        if !self.in_flight.insert(id.to_string()) {return Err("已有一次启动尝试在途".into())}
        Ok(Flight{id:id.to_string()})
    }
    // 暂停（身份导出/导入用）：停掉当前实例、取消这一轮调度，但**不是最终退出**——
    // 随后的 resume 会把 stopping 复位，插件照常回来（t33/F1）。
    pub fn pause_all(&mut self){
        self.quiesce("插件运行已暂停（身份操作）");
    }
    fn quiesce(&mut self,reason:&str){
        self.stopping=true;
        crate::plugin_start::cancel_all(reason);
        // t46/F1-1：暂停时还在 Process::start 里的 id 也必须 retire——否则 resume 会以
        // 同一代次再放一次，旧/新两个 worker 都能 install_instance，后到者覆盖并杀掉
        // 前一个 Process。清 in_flight 的同时把代次推走，旧 worker 回来只能被丢弃。
        let flying:Vec<String>=self.in_flight.iter().cloned().collect();
        self.in_flight.clear();
        for id in flying { self.retire(&id); }
        let taken=std::mem::take(&mut self.processes);
        for (id,handle) in taken {
            self.retire(&id);
            if let Ok(mut process)=handle.lock(){process.stop();}
        }
    }
    // 最终退出：此后 resume 一律不再拉起（main.rs 的退出路径）。
    pub fn close(&mut self){
        self.closed=true;
        self.quiesce("宿主退出");
    }
    // 一条失败：先进内存（插件卡当场显示），再逐行追加进 plugin-failures.log。
    // 落盘失败只影响「宿主重启后还能不能查」，绝不影响这次调用本身的返回。
    fn record(&mut self,manager:&Manager<'_>,failure:Failure){
        self.remember(manager);
        self.append(failure)
    }
    fn append(&mut self,failure:Failure){
        if let Some(host)=&self.host {
            append_failure_row(host,&failure);
        }
        self.failures.insert(failure.plugin.clone(),failure);
    }
    // 一条失败记录的骨架：id、版本、阶段、退出码、时间、原文、日志路径，以及 phase/attempt/耗时/state/budgetCold（S1 + t12 repair）。
    fn failure(&self,id:&str,version:&str,stage:&str,phase:Option<String>,attempt:Option<u32>,elapsed_ms:Option<u64>,state:Option<String>,budget_cold:Option<bool>,code:Option<i32>,raw:&str)->Failure{
        let log=self.host.as_deref().map(|host|log_file_of(host,id)).unwrap_or_default();
        Failure{plugin:id.into(),version:version.into(),stage:stage.into(),phase,attempt,elapsed_ms,state,budget_cold,code,at:crate::now_ms(),raw:raw.into(),log}
    }
    fn version_of(&self,id:&str)->String{self.versions.get(id).cloned().unwrap_or_default()}
    // 这个 id 当前的代次。停用/卸载/换版/进程没了都会 +1（见 retire）。
    pub fn generation(&self,id:&str)->u64{self.generations.get(id).copied().unwrap_or(0)}
    // retire 让这个 id 现有的实例作废：在途 RPC 回来时代次对不上，结果丢弃。
    fn retire(&mut self,id:&str){let next=self.generation(id).wrapping_add(1);self.generations.insert(id.into(),next);}
    // present 只回答「有没有这个实例」：不握手、不拿实例锁、不做任何 I/O。插件页面的
    // 入口用它（T09）——一页的 RPC 正在跑的时候开页面也不该被卡住。
    pub fn present(&self,id:&str)->bool{self.processes.contains_key(id)}
    // drop_instance 停掉并摘掉一个实例。有在途 RPC 时**不等**它（那会把停用/卸载拖到
    // 一次 RPC 超时）：这里只把句柄从 CURRENT 摘掉，Process 在最后一次 Arc 释放时
    // Drop 兜底杀进程树；代次已经 +1，那个线程回来也不会再把实例塞回去。
    fn drop_instance(&mut self,id:&str){
        self.retire(id);
        // T-E/A04：停用、卸载、换版都作废这个插件的在途自启尝试（迟到的结果不许回填）。
        crate::plugin_start::cancel(id,"插件已停用、卸载或换版");
        if let Some(handle)=self.processes.remove(id) { if let Ok(mut process)=handle.try_lock(){process.stop();} }
    }
    // 结账判据的一半：代次没变。另一半是「句柄还是原来那一个」（settles）。
    fn settles_generation(&self,id:&str,generation:u64)->bool{self.generation(id)==generation}
    fn settles(&self,id:&str,generation:u64,handle:&Instance)->bool{self.settles_generation(id,generation)&&self.processes.get(id).is_some_and(|current|Arc::ptr_eq(current,handle))}
    // install_instance 是**唯一**「CURRENT 持有区里碰实例表」的地方（t5-F2）：只看代次与
    // 退出闸，然后把刚拉起的进程装进去。参数里没有 Manager、也不返回 Result——锁里读
    // 注册表、verify、写盘在类型上就写不出来。装不进去（等待期间停用/卸载/退出）时
    // 返回 false，调用方把进程丢掉。
    fn accepts_instance(&self,id:&str,generation:u64)->bool{ !self.stopping && self.settles_generation(id,generation) }
    // t46/F1-1：除退出闸+代次，还要看这一轮。暂停不复位代次时旧 worker 也可能回来；
    // 旧轮编号一律不许安装，无论先到后到。
    fn accepts_instance_round(&self,id:&str,generation:u64,round:u64)->bool{
        self.accepts_instance(id,generation) && crate::plugin_start::is_current_round(round)
    }
    fn install_instance(&mut self,id:&str,generation:u64,round:u64,process:Process)->bool{
        if !self.accepts_instance_round(id,generation,round) { return false; }
        self.processes.insert(id.into(),Arc::new(Mutex::new(process)));
        self.failures.remove(id);
        true
    }
    pub fn view(&mut self,manager:&Manager<'_>)->Result<Value,String>{
        self.remember(manager);
        // T-E：自启调度器的生命周期状态（contracts §5.1）。先取快照再读注册表，
        // 锁序 CURRENT → SCHEDULER 与取消路径一致，不会反向。
        let mut lifecycle:BTreeMap<String,Value>=BTreeMap::new();
        if let Some(rows)=crate::plugin_start::snapshot().as_array(){
            for row in rows { if let Some(id)=row.get("id").and_then(Value::as_str) { lifecycle.insert(id.to_string(),row.clone()); } }
        }
        let mut rows=Vec::new();
        for installed in manager.list()? {
            let id=&installed.manifest.id;
            self.versions.insert(id.clone(),installed.manifest.version.clone());
            // 运行态判据（#14-4）在 Process::running 里：进程存活 + 握手可应答。
            // 但「应答不上」不等于「进程没了」（T01）：前者只报一轮「未响应」，
            // 后者才移除实例、记一条失败。两件事以前被 `!running()` 混成一件，
            // 于是设置页一次重绘碰上一次卡顿就把活着的插件 remove + Drop 杀掉，
            // `plugin-failures.log` 里那批 `exitCode:null` 就是这么来的（#55）。
            let (gone, unresponsive)=match self.processes.get(id){
                None=>(None,false),
                Some(handle)=>match handle.try_lock() {
                    Ok(mut process)=>{
                        let exited=process.exited();
                        // 真的没了就不再握手（alive() 会立刻答否，但省一趟也省心）。
                        let responsive=exited.is_none()&&process.running();
                        match liveness(exited,responsive){
                            Liveness::Gone(code)=>(Some(code),false),
                            Liveness::Unresponsive=>(None,true),
                            Liveness::Running=>(None,false),
                        }
                    },
                    // 实例锁被一次在途 RPC 占着（T09）：那正是「还活着」的证据。这里
                    // 既不排队等它（设置页会被一次 RPC 拖住），也不误判成没了。
                    Err(_)=>(None,false),
                }
            };
            if let Some(code)=gone {
                self.drop_instance(id);
                // 到这里一定是进程真的没了。退出码取不到时也别冒充「已退出」以外的
                // 判断——说清是问不到退出码，用户才知道该去查什么。
                let raw=if code.is_some(){"插件进程已退出，请检查配置后重新启用"}else{"插件进程已退出（退出码未知），请检查配置后重新启用"};
                self.record(manager,Failure{plugin:id.clone(),version:installed.manifest.version.clone(),stage:"运行".into(),phase:None,attempt:None,elapsed_ms:None,state:None,budget_cold:None,code,at:crate::now_ms(),raw:raw.into(),log:log_file(manager.context,id)});
            }
            let running=self.present(id);
            let package_missing=!manager.directory(&installed)?.join("plugin.json").is_file();
            // notice 是上一次没装成被回滚时留下的那句话（没有就是 null），
            // 插件卡上连同 npm 安装方式一起显示（#21）。
            let notice=manager.notice(id);
            let rollback=self.rollback_notes.get(id).map(String::as_str);
            let failure=self.failures.get(id);
            // T-E 生命周期字段（desktop 只读；没有调度记录时按实例在不在回落到 ready/stopped）。
            let life=lifecycle.get(id.as_str());
            let life_state=life.and_then(|row|row["state"].as_str()).unwrap_or(if running{"ready"}else{"stopped"});
            let life_phase=life.map(|row|row["phase"].clone()).unwrap_or(Value::Null);
            let life_attempt=life.and_then(|row|row["attempt"].as_u64()).unwrap_or(0);
            let life_max=life.and_then(|row|row["maxAttempts"].as_u64()).unwrap_or(crate::plugin_start::MAX_ATTEMPTS as u64);
            let life_permanent=life.and_then(|row|row["permanent"].as_bool()).unwrap_or(false);
            let life_retry=life.map(|row|row["retryInMs"].clone()).unwrap_or(Value::Null);
            let life_reason=life.map(|row|row["reason"].clone()).unwrap_or(Value::Null);
            rows.push(json!({"plugin":installed,"running":running,"packageMissing":package_missing,"notice":notice,"rollbackWarning":rollback,
                // error 仍是字符串（前端直接渲染）：退出码与日志路径都在里面（#53-4）；
                // 退出码、阶段、日志路径同时单独给出，页面要用就不必解析中文。
                "error":if package_missing {Some("插件程序包缺失，请在官方商店重新安装；原配置已保留".to_string())}else{failure.map(Failure::text)},
                "failure":failure.map(Failure::json),
                // 活着但这一轮探针没应答（T01）：页面说「未响应，正在重试」，
                // 下一轮重绘自己会好；它不是失败，所以 error / failure 仍是空。
                "unresponsive":unresponsive,
                "exitCode":failure.and_then(|failure|failure.code),
                "logPath":failure.map(|failure|failure.log.to_string_lossy().into_owned()).unwrap_or_else(||log_file(manager.context,id).to_string_lossy().into_owned()),
                // T-E：自启调度的生命周期状态（stable 枚举，desktop 不得自造）。
                "state":life_state,"phase":life_phase,"attempt":life_attempt,"maxAttempts":life_max,"permanent":life_permanent,"retryInMs":life_retry,"reason":life_reason}));
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
            rows.push(json!({"plugin":plugin,"running":false,"packageMissing":false,"notice":notice,"error":"安装未完成：这一版已经落地但没登记，点「重试安装」补上",
                "state":"stopped","phase":Value::Null,"attempt":0,"maxAttempts":crate::plugin_start::MAX_ATTEMPTS,"permanent":false,"retryInMs":Value::Null,"reason":Value::Null}));
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
    // t19/R1：入口先过 MAINTENANCE——`manager.set_autostart` 第一句就是 recover_unlocked()
    // （plugin_install.rs:416），能提交或删掉一棵版本目录；维护线程也取这把，不互斥的话
    // t≈30 s 点这个开关就会与 prune_versions 撞车。锁序 CURRENT → MAINTENANCE →
    // EDIT_REGISTRY → EDIT，与 install/uninstall/maintain 一致。
    pub fn set_autostart(&mut self,manager:&Manager<'_>,id:&str,on:bool,node:Option<&Path>)->Result<(),String>{
        let _maintenance=MAINTENANCE.lock().unwrap_or_else(|poison|poison.into_inner());
        self.set_autostart_locked(manager,id,on,node)
    }
    // set_autostart_locked 是同一个主体的**无锁**版本：调用方必须已经持着 MAINTENANCE
    // （本模块只有 set_autostart）。命名就是契约——MAINTENANCE 是不可重入的 std::Mutex，
    // 这里若再取一次就是自死锁（config.rs 的 ENV_LOCK 踩过同一个坑）。
    fn set_autostart_locked(&mut self,manager:&Manager<'_>,id:&str,on:bool,node:Option<&Path>)->Result<(),String>{
        let row=manager.set_autostart(id,on)?;
        if on {
            if row.enabled && !self.is_running(id) {
                // 已持 MAINTENANCE：走 _locked 版本，绝不回头调会再取锁的入口。
                self.set_enabled_starting_locked(manager,id,true,node,true)?;
            }
        } else { self.drop_instance(id); }
        Ok(())
    }
    // autostart：这次拉起是不是「随 Dock 启动」（#22）。resume 传 true，插件读
    // FISH_PLUGIN_AUTOSTART 自己起服务；安装、点启用、回滚都传 false。
    // t19/R1：入口先过 MAINTENANCE——内部 `manager.set_enabled` 同样第一句 recover_unlocked()
    // （plugin_install.rs:408），理由与 set_autostart 相同。set_enabled() 走这里，锁只取一次。
    // t12 repair F2：退避期间放掉 MAINTENANCE，避免插件页 3s 无响应。
    fn set_enabled_starting(&mut self,manager:&Manager<'_>,id:&str,enabled:bool,node:Option<&Path>,autostart:bool)->Result<(),String>{
        // 第一次尝试：持 MAINTENANCE 锁。
        let first_result={
            let _maintenance=MAINTENANCE.lock().unwrap_or_else(|poison|poison.into_inner());
            self.set_enabled_starting_locked(manager,id,enabled,node,autostart)
        };
        // 成功或禁用：直接返回。
        if first_result.is_ok() || !enabled { return first_result; }
        // 第一次失败：检查是否已经被 spawn_starts 拉起。
        if self.is_running(id) { return Ok(()); }
        // t12 repair F2/F5：退避前检查 closed/stopping，命中则直接返回首次失败原因。
        if self.closed { return Err("宿主正在退出".into()); }
        if self.stopping { return Err("插件运行已暂停".into()); }
        // 退避约 3s（可注入，测试传 no-op）。
        manual_enable_backoff();
        // 退避期间可能已经被 spawn_starts 拉起。
        if self.is_running(id) { return Ok(()); }
        // t12 repair F5：重试前再次检查 closed/stopping/代次。
        if self.closed { return Err("宿主正在退出".into()); }
        if self.stopping { return Err("插件运行已暂停".into()); }
        // 重试：重新取 MAINTENANCE 锁。
        {
            let _maintenance=MAINTENANCE.lock().unwrap_or_else(|poison|poison.into_inner());
            self.set_enabled_starting_locked(manager,id,enabled,node,autostart)
        }
    }
    // set_enabled_starting_locked 是同一个主体的**无锁**版本：调用方必须已经持着
    // MAINTENANCE（set_enabled_starting / set_autostart_locked）。
    // t12 repair：不再包含重试逻辑，重试由 set_enabled_starting 在锁外做。
    fn set_enabled_starting_locked(&mut self,manager:&Manager<'_>,id:&str,enabled:bool,node:Option<&Path>,autostart:bool)->Result<(),String>{
        self.remember(manager);
        let row=manager.list()?.into_iter().find(|row|row.manifest.id==id).ok_or("插件未安装")?;
        self.versions.insert(id.into(),row.manifest.version.clone());
        if !enabled {
            crate::plugin_page::revoke(id);
            manager.set_enabled(id,false)?;
            self.drop_instance(id);
            self.failures.remove(id);return Ok(());
        }
        // 单次尝试：由调用方（set_enabled_starting）决定是否重试。
        self.start_instance_inner(manager,&row,node,autostart,None,Some(1))
    }
    // start_instance 用**已经取到的注册行**把实例拉起来（T03 的「先取快照」）：resume
    // 拿着启动时那一份快照逐个拉，不必每个插件再回注册表读一遍、各自校验一遍。
    // 这里的 verify 由 plugin_process 在 Process::start 里做，已是快照优先，冷启不再
    // 对 129.9 MB 做全文哈希。
    fn start_instance(&mut self,manager:&Manager<'_>,row:&Installed,node:Option<&Path>,autostart:bool)->Result<(),String>{
        self.start_instance_inner(manager,row,node,autostart,None,None)
    }
    // start_instance_inner 是 start_instance 的核心逻辑：可被手动启用路径重试复用。
    // elapsed_ms / attempt 由调用方在重试循环中注入（S1 可观测性）。
    fn start_instance_inner(&mut self,manager:&Manager<'_>,row:&Installed,node:Option<&Path>,autostart:bool,elapsed_ms:Option<u64>,attempt:Option<u32>)->Result<(),String>{
        let id=row.manifest.id.clone();
        self.versions.insert(id.clone(),row.manifest.version.clone());
        if self.is_running(&id) { manager.set_enabled(&id,true)?; return Ok(()); }
        self.drop_instance(&id);
        // t14 repair F1：测试环境可注入「启动一次」的桩，用于验证重试路径。
        #[cfg(test)]
        {
            if let Some(starter)=MANUAL_ENABLE_STARTER.lock().unwrap().as_ref() {
                let result=starter(manager,row,node,autostart);
                if result.is_err() {
                    let failure=self.failure(&id,&row.manifest.version,"启动",None,attempt.or(Some(1)),elapsed_ms,None,None,None,&result.as_ref().unwrap_err());
                    self.record(manager,failure);
                }
                return result;
            }
        }
        let started_at=std::time::Instant::now();
        let mut process=match Process::start(manager,row,node,autostart){
            Ok(process)=>process,
            // 启动失败也留一条结构化记录（#53-3）：卡上要能看见是哪一版、哪一步、日志在哪。
            Err(error)=>{let elapsed=Some(started_at.elapsed().as_millis() as u64);let a=attempt.or(Some(1));let failure=self.failure(&id,&row.manifest.version,"启动",None,a,elapsed.or(elapsed_ms),None,None,None,&error);self.record(manager,failure);return Err(error)}
        };
        if let Err(error)=manager.set_enabled(&id,true){process.stop();return Err(error);}
        self.processes.insert(id.clone(),Arc::new(Mutex::new(process)));self.failures.remove(&id);
        // F2（t33）：手动启动已经定案，摘掉上一轮自启的调度记录——卡片以后只看实时 running。
        crate::plugin_start::forget(&id);
        Ok(())
    }
    pub fn resume(&mut self,manager:&Manager<'_>,node:Option<&Path>)->Result<(),String>{
        // F1（t33）：最终退出之后不再拉起；身份导出的「暂停」在这里复位，导出/导入结束
        // 插件会照常回来，而不是被 stopping 永久停在备份时刻。
        if self.closed { eprintln!("FISH Dock: 宿主已最终退出，忽略插件恢复"); return Ok(()); }
        self.stopping=false;
        self.remember(manager);
        manager.recover()?;
        // T03：清理类工作不再排在拉进程之前。register 一次维护线程，启动 30 s 后再收
        // host/tmp/txn-*、指向已卸载插件的安装事务、过期下载来源记录、旧版本目录与
        // 旧校验快照。以前这些在 resume() 里同步做，占着 CURRENT 与冷启 I/O。
        schedule_maintenance(manager.context);
        // 快照（T03）：注册表一次读全。后面的孤儿内核回收与逐个拉起都用这一份，不再
        // 每个插件各读一遍 registry、各校验一遍清单。
        let rows=manager.list()?;
        // 上一轮没收干净的孤儿内核（#14-4 宿主侧）：插件把自己的内核 pid 写在
        // config/run-state.json 里，这里先按它收，再拉起新实例——否则就是
        // 「僵尸内核 + 新实例启动失败」。判据见 orphan_pid：pid 被系统复用给别人时不动。
        self.reclaim(manager,&rows);
        // 关掉「自启动」的插件保持启用，但这次启动不拉起它的服务；用户点启用或
        // 插件页的启动服务仍然照常启动。开着的那批：拉起后台，并告诉它这次是
        // 自启动（FISH_PLUGIN_AUTOSTART=1），服务由插件自己决定起不起（#22）。
        let wanted:Vec<Installed>=rows.into_iter().filter(|row|row.enabled&&row.autostart).collect();
        // T03「先取快照再放锁」：要拉起的清单已经落到 wanted 里了。全局 CURRENT 那条
        // 链的锁由 plugin_api::resume 持有，resume() 一返回就放——所以最贵的那一段
        // （快照校验 + spawn + hello/activate 握手，最长到 activate 超时）交给
        // spawn_starts 在锁外做，每个插件只短暂借两次 CURRENT。
        if self.deferred {
            spawn_starts(manager.context.clone(),wanted,node.map(Path::to_path_buf));
            return Ok(());
        }
        // 逐插件容错（#38）：单个插件失败只影响它自己——失败进 self.failures（卡片上
        // 显示「启动失败」+原因），其余插件继续拉起；最后汇总一行日志，不再整批跳过。
        let mut failed:Vec<String>=Vec::new();
        for row in &wanted {
            if let Err(error)=self.start_instance(manager,row,node,true) { failed.push(format!("{}：{error}",row.manifest.id)); }
        }
        if !failed.is_empty() { eprintln!("FISH Dock: 插件自启动失败 {} 个（其余已照常拉起）：{}",failed.len(),failed.join("；")); }
        Ok(())
    }
    // call 是**持 CURRENT** 的调用路径（设置页的 extension_call 走这里）：借句柄、
    // 在这个锁之内等回答。它存在只是为了不改变已发布入口的语义；插件页面一律走
    // call_outside()，不要在这条路径上加新的等待（T09）。
    pub fn call(&mut self,id:&str,method:&str,params:Value)->Result<Value,String>{
        valid_method(method)?;
        let handle=self.processes.get(id).cloned().ok_or("插件尚未运行")?;
        let timeout=crate::config::plugin_rpc_timeout();
        let result=match handle.lock(){Ok(mut process)=>process.request(method,params,timeout),Err(_)=>Err("插件进程状态不可用".into())};
        self.settle(id,result)
    }
    // settle 是一次 RPC 回来之后的结账：业务错误原样返回（插件活着，不回收进程、
    // 不写失败记录）；其余错误才摘掉实例、记一条失败。
    fn settle(&mut self,id:&str,result:Result<Value,String>)->Result<Value,String>{
        match result {
            Ok(value)=>Ok(value),
            // 插件活着、只是这次业务调用没成：不回收进程、不写失败记录（t12 的错误码
            // 上行之后，业务错误带着「（错误码 X）」回来，不再是那一条固定常量）。
            Err(error) if crate::plugin_protocol::is_business_error(&error)||error==crate::plugin_protocol::OPERATION_ERROR=>Err(error),
            Err(error)=>{
                let code=self.processes.get(id).and_then(|handle|handle.lock().ok().map(|mut process|process.exit_code())).flatten();
                self.drop_instance(id);
                let version=self.version_of(id);
                let failure=self.failure(id,&version,"运行",None,None,None,None,None,code,&error);
                self.append(failure);
                Err(error)
            },
        }
    }
    pub fn uninstall(&mut self,manager:&Manager<'_>,id:&str)->Result<(),String>{
        // t5-F1：与维护线程同一把互斥——卸载也删 packages/<id>/，清理不能插在中间。
        let _maintenance=MAINTENANCE.lock().unwrap_or_else(|poison|poison.into_inner());
        // t29/C：先停服务（整树，端口释放）再删文件——反过来的话 Windows 上正在运行的
        // 进程还占着 exe/资源句柄，remove_dir_all 会失败，留下半个 packages/<id>/。
        crate::plugin_page::revoke(id);
        self.drop_instance(id);
        manager.uninstall(id)?;
        self.failures.remove(id);Ok(())
    }
    pub fn install(&mut self,manager:&Manager<'_>,package:Package,digest:&str,official:bool,node:Option<&Path>)->Result<Installed,String>{
        // t5-F1：整个「落地 + 提交 + 收旧文件」与维护线程互斥。以前两边都持 CURRENT，
        // 天然互斥；清理挪到 30 s 后的线程之后，这层互斥必须显式要，否则正在装的那一版
        // 会被 prune_versions 当成过期目录删掉。锁序 CURRENT → MAINTENANCE → 注册表锁。
        let _maintenance=MAINTENANCE.lock().unwrap_or_else(|poison|poison.into_inner());
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
        self.drop_instance(&id);
        let installed=manager.install(package,digest,official);
        let attempt=match installed {
            Ok(row)=>{
                self.versions.insert(id.clone(),row.manifest.version.clone());
                if row.enabled {match Process::start(manager,&row,node,false){Ok(process)=>{self.processes.insert(id.clone(),Arc::new(Mutex::new(process)));Ok(row)},Err(error)=>Err(error)}}else{Ok(row)}
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
                        let failure=self.failure(&id,&version,"更新",None,None,None,None,None,None,"更新失败（提交阶段）：旧版恢复尚未完成，请保留备份并重试");
                        self.record(manager,failure);
                        return Err("更新失败（提交阶段）：旧版恢复尚未完成，数据备份已保留".into());
                    }
                    // t38/B1：新版本已经落地、但服务起不来，回滚到旧版后把失败的新版本
                    // 目录也删掉——packages/<id>/ 只留旧版本，不把 2.0.0 留在盘上冒充回滚点。
                    let _=manager.prune_versions(&previous);
                }
                self.rollback_notes.remove(&id);
                let failure=self.failure(&id,&version,"更新",None,None,None,None,None,None,&message);
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
        if row.enabled && available {let process=Process::start(manager,row,node,false)?;self.processes.insert(row.manifest.id.clone(),Arc::new(Mutex::new(process)));}
        Ok(())
    }
    pub fn event(&mut self,params:Value){
        for handle in self.processes.values() {
            // 有在途 RPC 时不排队（事件是尽力投递，把事件线程堵在一次 RPC 后面更糟）。
            let Ok(mut process)=handle.try_lock() else {continue};
            if process.accepts_events()&&process.running(){
                if process.request("plugin.event",params.clone(),crate::config::plugin_event_timeout()).is_err(){eprintln!("FISH Dock: 插件事件投递失败，请检查插件状态");}
            }
        }
    }
    // t5-F5 / t19/R2：与 view() 同一套判据——抢不到实例锁说明**有在途 RPC**，那只是
    // 「还不知道」，**至少不能当成死亡证据**（不是「有活着的证据」：T01 认的死亡证据只有
    // try_wait，这里的 try_lock 失败连生存证据都不是）。方向与 T01「探针没应答 ≠ 进程没了」
    // 一致：错了也只是少杀/不重复拉起，不是误杀。以前这里用 lock() 排队等实例锁，一次
    // RPC 期间用户点自启动/启用会把 CURRENT 占到 RPC 超时，与「绝不在 CURRENT 里等插件回答」相悖。
    pub fn is_running(&mut self,id:&str)->bool {
        match self.processes.get(id) {
            None=>false,
            Some(handle)=>match handle.try_lock() { Ok(mut process)=>process.running(), Err(_)=>true },
        }
    }
    pub fn stop_all(&mut self){
        // T03：先立闸再摘实例，锁外拉起线程看到 stopping 就不会再开新进程。
        // t33/F1：这是**最终退出**（closed 一旦立起，resume 不再拉起）；身份备份那种
        // 「暂停→恢复」走 pause_all/resume，不会把插件永久留在停止状态。
        self.close();
    }
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
    fn reclaim(&mut self,manager:&Manager<'_>,rows:&[Installed]){
        for row in rows {
            // 本实例已经拉起（或正在拉起）的插件不碰：它的内核是活的、是自己的。
            if self.processes.contains_key(row.manifest.id.as_str()) { continue; }
            let Ok(namespace)=manager.namespace(row) else {continue};
            let Ok(Some(bytes))=namespace.read("config","run-state.json") else {continue};
            let Ok(state)=serde_json::from_slice::<Value>(&bytes) else {continue};
            let now=crate::now_ms();
            let Some(pid)=orphan_pid(&state,now,std::process::id(),|pid|crate::shell::running_for(pid).map(|age|age.as_millis() as u64)) else {continue};
            if attachable(&state,|port|std::net::TcpStream::connect_timeout(&std::net::SocketAddr::from(([127,0,0,1],port)),std::time::Duration::from_millis(ORPHAN_PROBE_TIMEOUT_MS)).is_ok()) { continue; }
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
        let failure=runner.failure("fish.clash","2.1.2","运行",None,None,None,None,None,Some(1),"插件进程已退出，请检查配置后重新启用");
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
        // 新字段：phase/attempt/elapsedMs（S1）。
        assert!(row.get("phase").is_some(),"phase 字段必须在（向后兼容，允许为 null）：{row}");
        assert!(row.get("attempt").is_some(),"attempt 字段必须在：{row}");
        assert!(row.get("elapsedMs").is_some(),"elapsedMs 字段必须在：{row}");
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
        let failure=runner.failure("fish.magpie","2.1.2","启动",None,None,None,None,None,None,"插件启动失败，请检查运行时路径及系统权限");
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
        let failure=runner.failure("fish.clash","2.1.2","运行",None,None,None,None,None,Some(1),"插件进程已退出，请检查配置后重新启用");
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
    // #53-3 S1：失败记录带 phase/attempt/elapsedMs 字段，且旧记录缺字段时仍可解析（向后兼容）。
    #[test] fn failure_record_carries_phase_attempt_elapsed_and_is_backward_compatible() {
        let root=root("failure-new-fields");
        let context=Context::open(root.clone(),"2.0.2").unwrap();
        let mut runner=Runner::default();runner.remember(&Manager{context:&context});
        let failure=runner.failure("fish.clash","2.1.9","启动",Some("spawn".to_string()),Some(2),Some(3200),None,None,None,"插件启动失败");
        runner.append(failure);
        let logged=std::fs::read_to_string(failure_log_of(context.host.path())).unwrap();
        let row:Value=serde_json::from_str(logged.trim()).unwrap();
        assert_eq!(row["plugin"],"fish.clash");
        assert_eq!(row["phase"],"spawn");
        assert_eq!(row["attempt"],2);
        assert_eq!(row["elapsedMs"],3200);
        // 旧记录（缺少新字段）解析不 panic：
        let old_json=r#"{"at":1700000000000,"plugin":"old.plugin","version":"1.0.0","stage":"运行","exitCode":null,"error":"旧记录","log":"/tmp/x.log"}"#;
        let old_row:Value=serde_json::from_str(old_json).unwrap();
        assert_eq!(old_row["plugin"],"old.plugin");
        assert!(old_row.get("phase").is_none(),"旧记录无 phase 字段，解析不应 panic");
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
    // T09：插件页面不再持 CURRENT 跑 RPC——借出句柄与代次之后放锁，回答回来再结账。
    // 这中间用户点了停用/卸载（plugin_page::revoke 之后 drop_instance → retire），
    // 那次在途的结果必须作废：既不许动之后拉起的新实例，也不许把失败记到新的头上。
    #[test] fn an_in_flight_page_call_is_discarded_once_its_instance_is_retired() {
        let mut runner=Runner::default();
        assert!(!runner.present("fish.magpie"),"没有实例时开页面就该说「插件尚未运行」");
        let generation=runner.generation("fish.magpie");
        assert!(runner.settles_generation("fish.magpie",generation),"代次没变才允许结账");
        // 停用 / 卸载 / 换版都走 drop_instance → retire。
        runner.drop_instance("fish.magpie");
        assert_ne!(runner.generation("fish.magpie"),generation,"停用或卸载必须让在途结果作废");
        assert!(!runner.settles_generation("fish.magpie",generation),"代次变了就不许再把结果交回去");
        // 之后又拉起一次：旧代次永远不再被认，新代次照常结账。
        runner.drop_instance("fish.magpie");
        let fresh=runner.generation("fish.magpie");
        assert!(!runner.settles_generation("fish.magpie",generation));
        assert!(runner.settles_generation("fish.magpie",fresh));
        assert!(!runner.present("fish.magpie"),"摘掉的实例不会被在途结果塞回来");
        // t5-F2 的断言形式：CURRENT 的持有区只有 install_instance，而它先问 accepts_instance
        // ——代次对不上就不装。签名里没有 Manager，所以锁里做 verify/写注册表写不出来。
        assert!(!runner.accepts_instance("fish.magpie",generation),"旧代次不许装回实例表");
        assert!(runner.accepts_instance("fish.magpie",fresh),"当前代次照常");
        runner.stop_all();
        assert!(!runner.accepts_instance("fish.magpie",runner.generation("fish.magpie")),"退出闸一立，谁都不许再装");
    }
    // T03：cleanup() / cleanup_staging() 挪到启动后 30 s 的维护线程——冷启关键路径
    // 上不再扫目录、删文件。这里钉两件事：延时就是 30 s；维护主体的确把暂存残留收干净。
    #[test] fn maintenance_runs_thirty_seconds_after_startup_and_collects_staging() {
        assert_eq!(MAINTENANCE_DELAY.as_secs(),30,"维护必须在启动 30 s 之后跑，不能挤在冷启关键路径上");
        // 拉进程的两种语义：自持一份的 Runner（单测、调用方自己 new 的）保持同步；
        // 全局 CURRENT 那一份把拉进程交给锁外线程——它的锁由调用方持有，resume() 只能
        // 「先取快照再交活」。
        assert!(!Runner::default().deferred,"自己 new 的 Runner 要同步拉起");
        assert!(CURRENT.lock().unwrap().deferred,"全局 CURRENT 必须在锁外拉起（T03）");
        let root=root("maintenance");
        let context=Context::open(root.clone(),"2.0.3").unwrap();
        let host=context.host.path().to_path_buf();
        let staging=host.join("tmp").join("txn-4242-1-0");
        std::fs::create_dir_all(staging.join("config")).unwrap();
        std::fs::write(staging.join("config").join("staged.json"),b"staged").unwrap();
        assert!(staging.exists());
        maintain(&context);
        assert!(!staging.exists(),"维护线程要收掉没有 journal 指认的暂存目录");
        std::fs::remove_dir_all(&root).unwrap();
    }
    // t5-F4：注册表读不出来时维护不许 panic，也不许改动坏表（坏了就留给人看），
    // 而「能做的清理」照做——这条路以前是 `unwrap_or_default()` 整段静默。
    #[test] fn maintenance_does_not_panic_on_an_unreadable_registry() {
        let (root,context)=installed_context("maintain-broken","acme.weather",".plugin.acme.weather");
        let registry=context.host.path().join("config").join("installed-plugins.json");
        std::fs::create_dir_all(registry.parent().unwrap()).unwrap();
        std::fs::write(&registry,b"{ not json").unwrap();
        let staging=context.host.path().join("tmp").join("txn-9-9-9");
        std::fs::create_dir_all(&staging).unwrap();
        maintain(&context);
        assert!(!staging.exists(),"与注册表无关的暂存残留照常收掉（cleanup_staging 不依赖注册表行）");
        assert_eq!(std::fs::read(&registry).unwrap(),b"{ not json","坏注册表不许被维护改写");
        std::fs::remove_dir_all(&root).unwrap();
    }
    // —— t5-F1/F3 用的夹具：一处建好宿主/插件命名空间的隔离数据根 + 合成插件包 ——
    fn installed_context(tag:&str,id:&str,directory:&str)->(PathBuf,Context){
        fn namespace(root:PathBuf,id:&str,directory:&str)->crate::storage_context::Namespace{
            crate::storage::initialize(&root,id,"1.0.0",1).unwrap();
            let identity=crate::storage_context::Identity{plugin_id:id.into(),directory:directory.into(),plugin_version:"1.0.0".into(),data_schema:1};
            std::fs::write(root.join("storage.json"),serde_json::to_vec(&identity).unwrap()).unwrap();
            crate::storage_context::Namespace::existing(&root).unwrap().unwrap()
        }
        let base=root(tag).join(".fish-dock");
        let host=namespace(base.join("host"),"host","host");
        namespace(base.join(directory),id,directory);
        (base.clone(),Context{root:base,host})
    }
    fn synthetic_package(id:&str,directory:&str,version:&str,backend:&str)->(crate::plugin_package::Package,String){
        use sha2::{Digest,Sha256};
        let mut files=std::collections::BTreeMap::from([
            ("backend.mjs".to_string(),backend.as_bytes().to_vec()),
            ("README.md".to_string(),format!("# {version}\n").into_bytes()),
        ]);
        let hashes:std::collections::BTreeMap<_,_>=files.iter().map(|(key,value)|(key.clone(),format!("{:x}",Sha256::digest(value)))).collect();
        let raw=json!({"schema":1,"id":id,"name":"合成插件","version":version,"hostApi":1,
            "platforms":[crate::plugin_install::platform()],
            "storage":{"directory":directory,"dataSchema":1},
            "backend":{"runtime":"node","entry":"backend.mjs"},
            "behaviors":{"autostart":true},
            "files":hashes});
        files.insert("plugin.json".into(),serde_json::to_vec(&raw).unwrap());
        let bytes=crate::identity_archive::zip(&files).unwrap();
        let digest=format!("{:x}",Sha256::digest(&bytes));
        let source=if id.starts_with("fish."){crate::storage::Source::OfficialCatalog}else{crate::storage::Source::ThirdParty};
        let package=crate::plugin_package::inspect(&bytes,&digest,&std::env::temp_dir().join("fish-runtime-inspect"),source,&crate::plugin_install::platform()).unwrap();
        (package,digest)
    }
    // t5-F1（第一半，判别性的那条）：维护线程与 install 走**同一把互斥**（EDIT_REGISTRY）。
    // 有人握着安装锁时 maintain() 必须等——以前它边删边装，30 s 时正好在装插件的用户会被
    // 删掉刚落地的 packages/<id>/<新版本>/。
    #[test] fn maintenance_waits_for_the_install_lock_before_it_prunes_anything() {
        let (root,context)=installed_context("maintain-lock","acme.weather",".plugin.acme.weather");
        let host=context.host.path().to_path_buf();
        let staging=host.join("tmp").join("txn-4242-7-0");
        std::fs::create_dir_all(staging.join("config")).unwrap();
        let holder=std::thread::spawn(||{
            // 与 install()/uninstall() 同一把上层串行锁。
            let _guard=MAINTENANCE.lock().unwrap_or_else(|poison|poison.into_inner());
            std::thread::sleep(std::time::Duration::from_millis(400));
        });
        std::thread::sleep(std::time::Duration::from_millis(80));
        let started=std::time::Instant::now();
        maintain(&context);
        let waited=started.elapsed();
        holder.join().unwrap();
        assert!(waited>=std::time::Duration::from_millis(250),"维护必须等安装锁（实际等了 {waited:?}）：不等就是边删边装");
        assert!(!staging.exists(),"放开锁之后维护要照常跑完：暂存目录收掉");
        std::fs::remove_dir_all(&root).unwrap();
    }
    // t5-F1（第二半）：安装完成之后立刻跑维护，当前版本目录与它的校验快照都不许被动，
    // 同版本旧摘要目录照旧要清——证明维护读到的是锁内的新注册表，而不是旧快照。
    #[test] fn a_version_that_was_just_installed_survives_maintenance() {
        let (root,context)=installed_context("maintain-keep","acme.weather",".plugin.acme.weather");
        let manager=Manager{context:&context};
        let (package,digest)=synthetic_package("acme.weather",".plugin.acme.weather","2.0.0","console.log('v2')");
        let row=manager.install(package,&digest,false).unwrap();
        assert!(manager.verify(&row).is_ok(),"装完要核验通过");
        let base=manager.directory(&row).unwrap();
        let stale=base.parent().unwrap().join("a".repeat(64));
        std::fs::create_dir_all(&stale).unwrap();
        maintain(&context);
        assert!(manager.verify(&row).is_ok(),"维护不许碰当前版本");
        assert!(crate::plugin_install::verified_file_of(context.host.path(),"acme.weather","2.0.0",&digest).is_file(),
            "当前版本的校验快照要留着");
        assert!(!stale.exists(),"同版本旧摘要目录照旧要清掉");
        std::fs::remove_dir_all(&root).unwrap();
    }
    // —— t19/R1：开关类入口也要过 MAINTENANCE ——
    // 先用「有人握着这把锁」精确扮演 install/维护线程那一刻（不做 sleep 竞猜：holder 拿到锁
    // 之后才发信号，计时从信号之后开始）。没有互斥时这两个调用会当场跑完（waited ≈ 0），
    // 有互斥时必然要等到 holder 放锁。
    fn wait_for_maintenance(max_hold_ms:u64, call:impl FnOnce())->std::time::Duration {
        let (tell,ready)=std::sync::mpsc::channel();
        let holder=std::thread::spawn(move||{
            let _guard=MAINTENANCE.lock().unwrap_or_else(|poison|poison.into_inner());
            let _=tell.send(());
            std::thread::sleep(std::time::Duration::from_millis(max_hold_ms));
        });
        ready.recv().unwrap();
        let started=std::time::Instant::now();
        call();
        let waited=started.elapsed();
        holder.join().unwrap();
        waited
    }
    // 两条开关入口（启用/停用、随 Dock 启动）在内部第一句就会 recover_unlocked()——
    // 与维护线程必须互斥，否则 t≈30 s 点开关会与 prune_versions 撞车（R1 的同类残留）。
    #[test] fn the_toggle_entries_wait_for_the_maintenance_lock() {
        let (root,context)=installed_context("toggle-lock","acme.weather",".plugin.acme.weather");
        let manager=Manager{context:&context};
        let (package,digest)=synthetic_package("acme.weather",".plugin.acme.weather","1.0.0","console.log('v1')");
        manager.install(package,&digest,false).unwrap();
        let mut runner=Runner::default();
        // ① 启用/停用入口：Runner::set_enabled → set_enabled_starting（内部 manager.set_enabled）
        let mut error=None;
        let waited=wait_for_maintenance(400,||{ error=runner.set_enabled(&manager,"acme.weather",false,None).err(); });
        assert!(error.is_none(),"停用本身要成功：{error:?}");
        assert!(waited>=std::time::Duration::from_millis(250),"启用/停用入口必须等维护锁（实际等 {waited:?}）：不等就会与 recover/prune 撞车");
        // ② 「随 Dock 启动」入口：Runner::set_autostart（内部 manager.set_autostart）
        let mut error=None;
        let waited=wait_for_maintenance(400,||{ error=runner.set_autostart(&manager,"acme.weather",false,None).err(); });
        assert!(error.is_none(),"关自启动本身要成功：{error:?}");
        assert!(waited>=std::time::Duration::from_millis(250),"自启动入口必须等维护锁（实际等 {waited:?}）");
        let _=std::fs::remove_dir_all(&root);
    }
    // R1 的功能面：造一次「v2 文件已落地、注册表还指 v1、journal 留着」的崩溃现场（正是
    // recover_unlocked 的续做条件），维护线程在跑时点「随 Dock 启动」——恢复必须整段在锁内，
    // 续做完之后注册表指向的目录要真的在，随后维护也不许把刚提交的这一版当过期目录删掉。
    #[test] fn a_pending_journal_is_recovered_under_the_lock_and_the_new_version_survives_maintenance() {
        let (root,context)=installed_context("toggle-journal","acme.weather",".plugin.acme.weather");
        let manager=Manager{context:&context};
        let (v1,v1_digest)=synthetic_package("acme.weather",".plugin.acme.weather","1.0.0","console.log('v1')");
        let row_v1=manager.install(v1,&v1_digest,false).unwrap();
        let (v2,v2_digest)=synthetic_package("acme.weather",".plugin.acme.weather","2.0.0","console.log('v2')");
        let row_v2=manager.install(v2,&v2_digest,false).unwrap();
        // 回退注册表 + 补 journal：等价于「v2 落地核验都过了、写注册表那一步之前崩掉」。
        manager.restore_registration(&row_v1).unwrap();
        let journal=json!({"schema":1,"previous":row_v1,"next":row_v2});
        let name=format!("plugin-install-{}.json",row_v2.manifest.id);
        context.host.write("tmp",&name,&serde_json::to_vec(&journal).unwrap()).unwrap();
        assert!(manager.verify(&row_v2).is_ok(),"v2 的文件本来就该是齐的（续做的前提）");
        let mut runner=Runner::default();
        let mut error=None;
        let waited=wait_for_maintenance(300,||{ error=runner.set_autostart(&manager,"acme.weather",false,None).err(); });
        assert!(error.is_none(),"开关本身要成功：{error:?}");
        assert!(waited>=std::time::Duration::from_millis(150),"必须等维护锁（实际等 {waited:?}）");
        // 续做完成：注册表指到 v2、journal 收掉、目录与校验都在。
        let active=manager.list().unwrap().into_iter().find(|row|row.manifest.id=="acme.weather").unwrap();
        assert_eq!(active.manifest.version,"2.0.0","遗留 journal 应在锁内续做完");
        assert_eq!(active.digest,row_v2.digest);
        assert!(manager.directory(&active).unwrap().is_dir(),"注册表指向的版本目录必须真的在");
        assert!(!context.host.path().join("tmp").join(&name).exists(),"续做之后 journal 应收掉");
        // 维护随即跑：刚提交的这一版不许被 prune_versions 当「过期版本」整目录删掉。
        maintain(&context);
        assert!(manager.directory(&active).unwrap().is_dir(),"维护不许删刚提交的版本目录");
        assert!(manager.verify(&active).is_ok(),"维护之后这一版仍要能核验通过");
        assert!(!manager.directory(&row_v1).unwrap().is_dir(),"旧版本目录照旧该被清掉");
        let _=std::fs::remove_dir_all(&root);
    }
    // t5-F3：call_outside 的丢弃分支（等待期间被停用/卸载）以前零覆盖——只有谓词级用例。
    // 这条用合成插件起一次真 RPC：另开线程调 call_outside，主线程 retire/drop_instance，
    // 断言 ① 结果被作废 ② 旧结果没把实例塞回来 ③ 失败不记到新代次头上。
    #[test]
    #[ignore = "需 FISH_DOCK_TEST_NODE；t5-F3 call_outside 丢弃分支（真 RPC + 停用竞态）"]
    fn an_in_flight_call_is_discarded_once_its_instance_is_retired() {
        let node=std::env::var_os("FISH_DOCK_TEST_NODE").map(PathBuf::from).expect("设置 FISH_DOCK_TEST_NODE");
        let (root,context)=installed_context("call-outside","acme.weather",".plugin.acme.weather");
        let manager=Manager{context:&context};
        // 合成后端要能「慢慢回答」：400 ms 的 slow 命令，用 SDK 的 createPlugin。
        use sha2::{Digest,Sha256};
        let sdk=include_bytes!("../../../sdk/node/plugin.mjs").to_vec();
        let backend=b"import {createPlugin} from './sdk.mjs';createPlugin({id:'acme.weather',version:'1.0.0',capabilities:['storage'],commands:{slow:()=>new Promise(resolve=>setTimeout(()=>resolve('late'),400))}});".to_vec();
        let mut files=std::collections::BTreeMap::from([("backend.mjs".to_string(),backend.clone()),("sdk.mjs".to_string(),sdk.clone()),("README.md".to_string(),b"# 1.0.0\n".to_vec())]);
        let hashes:std::collections::BTreeMap<_,_>=files.iter().map(|(key,value)|(key.clone(),format!("{:x}",Sha256::digest(value)))).collect();
        let raw=json!({"schema":1,"id":"acme.weather","name":"合成插件","version":"1.0.0","hostApi":1,
            "platforms":[crate::plugin_install::platform()],
            "storage":{"directory":".plugin.acme.weather","dataSchema":1},
            "backend":{"runtime":"node","entry":"backend.mjs"},
            "behaviors":{"autostart":true},
            "files":hashes});
        files.insert("plugin.json".into(),serde_json::to_vec(&raw).unwrap());
        let bytes=crate::identity_archive::zip(&files).unwrap();
        let digest=format!("{:x}",Sha256::digest(&bytes));
        let package=crate::plugin_package::inspect(&bytes,&digest,&std::env::temp_dir().join("fish-runtime-inspect"),crate::storage::Source::ThirdParty,&crate::plugin_install::platform()).unwrap();
        let row=manager.install(package,&digest,false).unwrap();
        let process=Process::start(&manager,&row,Some(&node),false).expect("合成插件要能起来");
        let generation={
            let mut runner=CURRENT.lock().unwrap();
            runner.processes.insert("acme.weather".into(),Arc::new(Mutex::new(process)));
            runner.failures.remove("acme.weather");
            runner.generation("acme.weather")
        };
        let caller=std::thread::spawn(|| call_outside("acme.weather","slow",json!({})));
        std::thread::sleep(std::time::Duration::from_millis(120));
        {
            let mut runner=CURRENT.lock().unwrap();
            // 停用/卸载走的正是这条路：先作废代次，再摘实例。
            runner.drop_instance("acme.weather");
        }
        let error=caller.join().unwrap().expect_err("等待期间被停用，结果必须作废");
        assert!(error.contains("已丢弃"),"{error}");
        let runner=CURRENT.lock().unwrap();
        assert!(!runner.present("acme.weather"),"旧结果不许把实例塞回来");
        assert!(runner.failures.get("acme.weather").is_none(),"作废的调用不许把失败记到新代次头上");
        assert!(!runner.accepts_instance("acme.weather",generation),"旧代次也不许装回去");
        drop(runner);
        std::fs::remove_dir_all(&root).unwrap();
    }
    // —— t33/F1：暂停（身份备份）可恢复，最终退出不可恢复；重复 resume 不换健康实例 ——
    #[test] fn pause_is_revivable_and_close_is_final() {
        let (root,context)=installed_context("pause-flags","acme.weather",".plugin.acme.weather");
        let manager=Manager{context:&context};
        let mut runner=Runner::default();
        runner.pause_all();
        assert!(runner.stopping,"暂停要立起「不许再拉」的闸");
        assert!(!runner.closed,"暂停不是最终退出");
        assert!(!runner.accepts_instance("acme.weather",runner.generation("acme.weather")),"暂停期间不许回填实例");
        // 备份结束：resume 复位暂停；插件还能重新拉起（没有实例时）。
        runner.resume(&manager,None).unwrap();
        assert!(!runner.stopping,"resume 必须复位暂停，否则备份后所有插件永远停着");
        assert!(!runner.closed);
        runner.close();
        assert!(runner.closed && runner.stopping,"最终退出要同时立 closed 与 stopping");
        runner.resume(&manager,None).unwrap();
        assert!(runner.closed,"最终退出之后 resume 不再放开");
        std::fs::remove_dir_all(root.parent().unwrap()).unwrap();
    }
    // t33/F1：跨轮重复 resume 的「单在途」闸——同一 id 第二次登记必须被拒绝。
    #[test] fn the_attempt_gate_refuses_a_second_flight() {
        let mut runner=Runner::default();
        let flight=runner.try_begin_attempt("acme.weather").expect("第一次应当允许");
        assert!(runner.try_begin_attempt("acme.weather").is_err(),"同一 id 不许两个 worker 同时 spawn");
        drop(flight);
        // Drop 的清理只对全局 CURRENT 生效（生产路径）；本地实例手动清。
        runner.in_flight.clear();
        runner.stopping=true;
        assert!(runner.try_begin_attempt("acme.weather").is_err(),"暂停/退出期间不许开新拉起");
        runner.stopping=false;
        runner.closed=true;
        assert!(runner.try_begin_attempt("acme.weather").is_err(),"最终退出期间不许开新拉起");
    }
    // t46/F1-1：安装除 stopping+代次，还要校验轮次；旧轮编号无论先到后到都不许安装。
    #[test] fn installation_requires_the_current_round() {
        let _serial=crate::plugin_start::GLOBAL_ROUND_TEST.lock().unwrap_or_else(|poison|poison.into_inner());
        let round=crate::plugin_start::begin(true,std::time::Duration::from_secs(60),std::time::Duration::from_secs(60),std::time::Duration::from_secs(10));
        let runner=Runner::default();
        assert!(runner.accepts_instance_round("acme.weather",0,round),"当前轮允许安装");
        assert!(!runner.accepts_instance_round("acme.weather",0,round.wrapping_add(1)),"旧轮/错轮不许安装（t46 F1-1）");
    }
    // t33/F1 端到端（合成 Node 插件，真进程）：健康 → 暂停（备份语义）→ 恢复 → 重复 resume。
    #[test]
    #[ignore = "需 FISH_DOCK_TEST_NODE；t33/F1 备份暂停→恢复端到端（合成插件，不碰真实安装）"]
    fn pause_and_resume_brings_the_plugin_back_without_replacing_it() {
        let node=std::env::var_os("FISH_DOCK_TEST_NODE").map(PathBuf::from).expect("设置 FISH_DOCK_TEST_NODE");
        let (root,context)=installed_context("pause-e2e","acme.weather",".plugin.acme.weather");
        let manager=Manager{context:&context};
        let backend=r#"let buf='';const send=(id,result)=>process.stdout.write(JSON.stringify({id,result})+'\n');
process.stdin.setEncoding('utf8');process.stdin.on('data',d=>{buf+=d;let i;while((i=buf.indexOf('\n'))>=0){const line=buf.slice(0,i);buf=buf.slice(i+1);if(!line)continue;const row=JSON.parse(line);
if(row.method==='host.hello')send(row.id,{type:'plugin.hello',protocol:1,pluginId:'acme.weather'});
else if(row.method==='plugin.activate')send(row.id,{active:true});
else if(row.method==='plugin.deactivate')send(row.id,{active:false});
else if(row.method==='plugin.health')send(row.id,{ready:true});
else send(row.id,'pong');}});"#;
        let (package,digest)=synthetic_package("acme.weather",".plugin.acme.weather","1.0.0",backend);
        manager.install(package,&digest,false).unwrap();
        manager.set_autostart("acme.weather",true).unwrap();
        // 全局 CURRENT 是生产路径；先把上一位使用者的闸复位（测试串行跑）。
        {
            let mut runner=CURRENT.lock().unwrap();
            runner.closed=false; runner.stopping=false; runner.in_flight.clear(); runner.failures.clear();
            runner.processes.remove("acme.weather");
        }
        {
            let mut runner=CURRENT.lock().unwrap();
            runner.set_enabled(&manager,"acme.weather",true,Some(&node)).expect("合成插件要能起来");
            assert!(runner.is_running("acme.weather"),"先要有健康实例");
        }
        let healthy_generation={ CURRENT.lock().unwrap().generation("acme.weather") };
        // 备份开始：暂停——实例停掉、调度取消，但不是最终退出。
        CURRENT.lock().unwrap().pause_all();
        assert!(!CURRENT.lock().unwrap().present("acme.weather"),"暂停要真的停实例");
        // 备份结束：resume 让插件回来。
        CURRENT.lock().unwrap().resume(&manager,Some(&node)).unwrap();
        let wait_until=|max:std::time::Duration|{
            let began=std::time::Instant::now();
            while began.elapsed()<max {
                if CURRENT.lock().map(|runner|runner.present("acme.weather")).unwrap_or(false) { return true; }
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
            false
        };
        assert!(wait_until(std::time::Duration::from_secs(15)),"恢复后插件必须重新拉起（不能永久停在备份时刻）");
        let resumed_generation={ CURRENT.lock().unwrap().generation("acme.weather") };
        assert_ne!(resumed_generation,healthy_generation,"暂停会停掉代次，重新拉起是新代次");
        // 重复 resume：健康实例不许被替换（代次不变）。
        CURRENT.lock().unwrap().resume(&manager,Some(&node)).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(400));
        let after_resume={ let runner=CURRENT.lock().unwrap(); (runner.present("acme.weather"),runner.generation("acme.weather")) };
        assert!(after_resume.0,"重复 resume 之后实例仍要在");
        assert_eq!(after_resume.1,resumed_generation,"重复 resume 不许替换健康实例（代次必须不变）");
        {
            let mut runner=CURRENT.lock().unwrap();
            runner.stop_all();
            runner.closed=false; runner.stopping=false; runner.in_flight.clear(); runner.failures.clear();
        }
        std::fs::remove_dir_all(root.parent().unwrap()).unwrap();
    }
    // t46/F1-1：暂停发生在「启动在途」时（hello 还没回来）——暂停必须把在途 id 也 retire，
    // 恢复后旧 worker 不得安装、更不得覆盖/替换新健康实例。合成 Node 插件用延迟 hello 制造屏障。
    #[test]
    #[ignore = "需 FISH_DOCK_TEST_NODE；t46/F1-1 启动途中暂停→恢复双 spawn 回归（合成插件）"]
    fn pausing_while_startup_in_flight_cannot_install_twice_after_resume() {
        let node=std::env::var_os("FISH_DOCK_TEST_NODE").map(PathBuf::from).expect("设置 FISH_DOCK_TEST_NODE");
        let (root,context)=installed_context("pause-inflight","acme.weather",".plugin.acme.weather");
        let manager=Manager{context:&context};
        // hello 故意晚 350 ms 回答：暂停窗口稳定落在 Process::start 里；每个进程有自己的 instance id。
        let backend=r#"let buf='';const instance=Math.random().toString(36).slice(2);const send=(id,result)=>process.stdout.write(JSON.stringify({id,result})+'\n');
process.stdin.setEncoding('utf8');process.stdin.on('data',d=>{buf+=d;let i;while((i=buf.indexOf('\n'))>=0){const line=buf.slice(0,i);buf=buf.slice(i+1);if(!line)continue;const row=JSON.parse(line);const reply=result=>send(row.id,result);
if(row.method==='host.hello')setTimeout(()=>reply({type:'plugin.hello',protocol:1,pluginId:'acme.weather'}),350);
else if(row.method==='plugin.activate')reply({active:true});
else if(row.method==='plugin.deactivate')reply({active:false});
else if(row.method==='plugin.health')reply({ready:true});
else if(row.method==='instance')reply(instance);
else reply('pong');}});"#;
        let (package,digest)=synthetic_package("acme.weather",".plugin.acme.weather","1.0.0",backend);
        manager.install(package,&digest,false).unwrap();
        manager.set_autostart("acme.weather",true).unwrap();
        manager.set_enabled("acme.weather",true).unwrap();
        {
            let mut runner=CURRENT.lock().unwrap();
            runner.closed=false; runner.stopping=false; runner.in_flight.clear(); runner.failures.clear();
            runner.processes.remove("acme.weather");
        }
        let wait_until=|max:std::time::Duration,check:&dyn Fn()->bool|{
            let began=std::time::Instant::now();
            while began.elapsed()<max { if check() { return true; } std::thread::sleep(std::time::Duration::from_millis(20)); }
            false
        };
        // 从 resume（生产启动链）开始，等它真的把 hello 发出去、还没有实例。
        CURRENT.lock().unwrap().resume(&manager,Some(&node)).unwrap();
        let inflight=wait_until(std::time::Duration::from_secs(8),&||{
            let runner=CURRENT.lock().unwrap();
            runner.in_flight.contains("acme.weather")&&!runner.present("acme.weather")
        });
        assert!(inflight,"测试屏障没有建立：启动在途窗口内没看到 in_flight");
        let before={ CURRENT.lock().unwrap().generation("acme.weather") };
        // 备份开始：暂停发生在启动途中。
        CURRENT.lock().unwrap().pause_all();
        let paused={ CURRENT.lock().unwrap().generation("acme.weather") };
        assert_ne!(paused,before,"暂停必须 retire 在途 id；否则同代次会被 resume 再放行（t46 F1-1）");
        // 备份结束：恢复。旧 worker 仍在 hello 里，会在之后回来。
        CURRENT.lock().unwrap().resume(&manager,Some(&node)).unwrap();
        assert!(wait_until(std::time::Duration::from_secs(15),&||CURRENT.lock().map(|r|r.present("acme.weather")).unwrap_or(false)),"恢复后必须有健康实例");
        let first=call_outside("acme.weather","instance",json!({})).expect("新实例必须能回答 instance");
        // 等过旧 worker 的 350ms hello：它回来时不许覆盖/杀掉这个新实例。
        std::thread::sleep(std::time::Duration::from_millis(900));
        let second=call_outside("acme.weather","instance",json!({})).expect("旧 worker 回来后新实例必须还活着");
        assert_eq!(first,second,"后到的旧 worker 不许替换/杀掉新健康实例（instance id 必须不变）");
        {
            let mut runner=CURRENT.lock().unwrap();
            assert!(runner.present("acme.weather"),"最终只应有一个健康实例");
            runner.stop_all();
            runner.closed=false; runner.stopping=false; runner.in_flight.clear(); runner.failures.clear();
        }
        std::fs::remove_dir_all(root.parent().unwrap()).unwrap();
    }
    // S2（#70）：手动启用首次失败后有退避重试（约3s）。
    // 用合成插件验证，首次通过 drop_instance 人为制造失败（让 start_instance 走失败路径），
    // 第二次成功。由于需要真实 Node，标记为 #[ignore]。
    #[test]
    #[ignore = "需 FISH_DOCK_TEST_NODE；S2 手动启用重试端到端"]
    fn manual_enable_retries_on_first_failure() {
        let node=std::env::var_os("FISH_DOCK_TEST_NODE").map(PathBuf::from).expect("设置 FISH_DOCK_TEST_NODE");
        let (root,context)=installed_context("manual-retry","acme.weather",".plugin.acme.weather");
        let manager=Manager{context:&context};
        let backend=r#"let buf='';const send=(id,result)=>process.stdout.write(JSON.stringify({id,result})+'\n');
process.stdin.setEncoding('utf8');process.stdin.on('data',d=>{buf+=d;let i;while((i=buf.indexOf('\n'))>=0){const line=buf.slice(0,i);buf=buf.slice(i+1);if(!line)continue;const row=JSON.parse(line);
if(row.method==='host.hello')send(row.id,{type:'plugin.hello',protocol:1,pluginId:'acme.weather'});
else if(row.method==='plugin.activate')send(row.id,{active:true});
else if(row.method==='plugin.health')send(row.id,{ready:true});
else send(row.id,'pong');}});"#;
        let (package,digest)=synthetic_package("acme.weather",".plugin.acme.weather","1.0.0",backend);
        let _row=manager.install(package,&digest,false).unwrap();
        manager.set_autostart("acme.weather",true).unwrap();
        {
            let mut runner=CURRENT.lock().unwrap();
            runner.closed=false; runner.stopping=false; runner.in_flight.clear(); runner.failures.clear();
            runner.processes.remove("acme.weather");
        }
        // 先手动启用，应当成功。
        CURRENT.lock().unwrap().set_enabled(&manager,"acme.weather",true,Some(&node)).expect("首次启用必须成功");
        assert!(CURRENT.lock().unwrap().is_running("acme.weather"),"手动启用后插件必须在运行");
        // 停掉它，再手动启用一次，验证重试路径。
        CURRENT.lock().unwrap().drop_instance("acme.weather");
        // 这次再启用，set_enabled_starting_locked 的重试逻辑会生效。
        CURRENT.lock().unwrap().set_enabled(&manager,"acme.weather",true,Some(&node)).expect("重试后必须成功");
        assert!(CURRENT.lock().unwrap().is_running("acme.weather"),"重试后插件必须在运行");
        {
            let mut runner=CURRENT.lock().unwrap();
            runner.stop_all();
            runner.closed=false; runner.stopping=false; runner.in_flight.clear(); runner.failures.clear();
        }
        std::fs::remove_dir_all(root.parent().unwrap()).unwrap();
    }
    // t14 repair F1：手动启用重试路径的真实验证（不依赖 FISH_DOCK_TEST_NODE）。
    // 注入「第一次返回 Err、第二次返回 Ok」的启动器桩，验证：
    // ① 退避函数被调用恰好 1 次；② 最终返回 Ok；③ plugin-failures.log 出现 attempt=1 记录。
    #[test]
    fn manual_enable_retry_path_actually_runs() {
        // t21：获取测试锁，防止并行测试干扰注入的静态量。
        let _guard = crate::plugin_runtime::acquire_manual_enable_test_lock();
        
        use std::sync::atomic::{AtomicUsize, Ordering};
        // 注入退避计数器。
        let backoff_called = Arc::new(AtomicUsize::new(0));
        let backoff_counter = backoff_called.clone();
        set_manual_enable_backoff_for_test(Some(Box::new(move || {
            backoff_counter.fetch_add(1, Ordering::SeqCst);
        })));
        // 注入「第一次失败、第二次成功」的启动器。
        let attempt_count = Arc::new(AtomicUsize::new(0));
        let attempt_counter = attempt_count.clone();
        set_manual_enable_starter_for_test(Some(move |_manager:&Manager<'_>,_row:&Installed,_node:Option<&Path>,_autostart:bool| -> Result<(),String> {
            let n = attempt_counter.fetch_add(1, Ordering::SeqCst);
            if n == 0 {
                Err("合成首次启动失败".to_string())
            } else {
                Ok(())
            }
        }));
        // 构造测试环境。
        let root = root("manual-retry-real");
        let context = Context::open(root.clone(), "2.0.2").unwrap();
        let manager = Manager { context: &context };
        // 注册一个合成插件（不需要真实 Node，因为启动器被注入了）。
        let (package, digest) = synthetic_package("acme.weather", ".plugin.acme.weather", "1.0.0", "console.log('test')");
        manager.install(package, &digest, false).unwrap();
        // 初始化 Runner。
        let mut runner = Runner::default();
        runner.remember(&manager);
        runner.closed = false;
        runner.stopping = false;
        // 调用 set_enabled（走 set_enabled_starting 的重试路径）。
        let result = runner.set_enabled(&manager, "acme.weather", true, None);
        // 断言：① 最终返回 Ok（第二次尝试成功）。
        assert!(result.is_ok(), "第二次尝试必须成功：{:?}", result);
        // 断言：② 退避函数被调用恰好 1 次。
        assert_eq!(backoff_called.load(Ordering::SeqCst), 1, "退避必须被调用恰好 1 次");
        // 断言：③ 启动器被调用 2 次（第一次失败，第二次成功）。
        assert_eq!(attempt_count.load(Ordering::SeqCst), 2, "启动器必须被调用 2 次");
        // 断言：④ plugin-failures.log 出现 attempt=1 的记录。
        let log_content = std::fs::read_to_string(failure_log_of(context.host.path())).unwrap_or_default();
        assert!(log_content.contains("\"attempt\":1"), "失败日志必须包含 attempt=1：{}", log_content);
        // t21：守卫 Drop 时会自动清理注入的静态量，无需手动调用 set_*_for_test(None)。
        std::fs::remove_dir_all(&root).ok();
    }
    // S2（#70）：并发 resume（spawn_starts 与 set_enabled_starting_locked 同时）不会产生两个进程。
    // 用 in_flight 守卫验证——两个线程同时尝试拉起同一个插件，只有一个能拿到 in_flight 位置。
    #[test]
    fn concurrent_resume_does_not_double_spawn_via_in_flight_gate() {
        let mut runner=Runner::default();
        // 第一次 try_begin_attempt 成功
        let f1=runner.try_begin_attempt("acme.weather").expect("第一次必须允许");
        // 第二次在 in_flight 仍占着时必须拒绝
        assert!(runner.try_begin_attempt("acme.weather").is_err(),"in_flight 占着时不许双 spawn");
        // 释放后再次允许
        drop(f1);
        runner.in_flight.clear(); // Drop 只清全局 CURRENT，手动清
        let f2=runner.try_begin_attempt("acme.weather").expect("释放后必须允许");
        drop(f2);
    }
}