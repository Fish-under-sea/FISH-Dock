//! 插件自启调度（T-E / A02–A04）。
//!
//! 一次自启 = 一个调度器 + 至多两槽并发：
//! - **一次尝试一个共享 deadline**：`host.hello` 与 `plugin.activate`（以及业务服务探测）
//!   共用剩余预算，冷态默认 60 s、热态默认 10 s。以前两个阶段各自计时，单插件理论上限
//!   是「重试次数 × 阶段预算」的乘法（A02）。
//! - **整轮生命周期一个有界预算**：默认 180 s，从调度开始时计时，排队、校验、退避、
//!   重试都算在内；到期不再开新尝试，在途的那次结果只用于记账（`expire`）。
//! - **至多首次 + 3 次重试**，退避 0.5 / 2 / 5 s；只有瞬时错误才重试，身份协议不匹配、
//!   摘要失败、非法配置这类永久错误立即终止。
//! - **每插件只有一个在途 token**：id + generation + version + digest。重复入队按「已在途」
//!   处理；代次变了（停用/卸载/换版）就作废旧 token，迟到的结果只回填到它自己那一代。
//! - **两槽公平**：一次最多两个插件同时启动，慢插件不挡其他候选的首次尝试。
//!
//! 调度器本身是纯状态机（时间由调用方注入），所以生命周期、退避、槽位、代次与取消都能
//! 用注入时钟单测，不必真起插件进程，也不必 sleep。

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use serde_json::{json, Value};

use crate::plugin_install::Installed;

/// 每插件最多尝试次数：首次 + 3 次重试（A02）。
pub const MAX_ATTEMPTS: u32 = 4;
/// 同时启动的插件数上限（A02：慢插件不得阻塞其他候选的首次启动）。
pub const MAX_START_PARALLEL: usize = 2;
/// 重试退避序列：第 1/2/3 次重试之前分别歇这么久。
pub const BACKOFF: [Duration; 3] = [
    Duration::from_millis(500),
    Duration::from_secs(2),
    Duration::from_secs(5),
];
/// 调度循环一次等待的下限：没有到期任务时也别空转成 0 等待。
const DRIVE_WAIT_FLOOR: Duration = Duration::from_millis(10);
/// 调度循环单次最长等待（任务归来随时被 channel 唤醒）。
const DRIVE_WAIT_SLICE: Duration = Duration::from_millis(250);

/// 一次启动尝试的不可变身份：绑定 id、代次、包版本与安装摘要（A04）。
///
/// `generation` 来自 `plugin_runtime::Runner::generation`：停用、卸载、换版、进程真的没了
/// 都会 +1。回填时只做代次比较与登记，绝不碰之后拉起的新实例。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Token {
    pub id: String,
    pub generation: u64,
    pub version: String,
    pub digest: String,
}

impl Token {
    pub fn of(row: &Installed, generation: u64) -> Self {
        Token {
            id: row.manifest.id.clone(),
            generation,
            version: row.manifest.version.clone(),
            digest: row.digest.clone(),
        }
    }
}

/// 启动阶段：日志与失败原因都按它分档（A03）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    Verify,
    Spawn,
    Hello,
    Activate,
    Service,
}

impl Phase {
    pub fn as_str(self) -> &'static str {
        match self {
            Phase::Verify => "verify",
            Phase::Spawn => "spawn",
            Phase::Hello => "hello",
            Phase::Activate => "activate",
            Phase::Service => "service",
        }
    }
    /// 插件卡与日志用的人话阶段名。
    pub fn label(self) -> &'static str {
        match self {
            Phase::Verify => "校验",
            Phase::Spawn => "启动进程",
            Phase::Hello => "握手",
            Phase::Activate => "激活",
            Phase::Service => "业务服务",
        }
    }
}

/// 生命周期状态：稳定枚举，desktop 只读、不得自造字面量（contracts §5.1）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    Stopped,
    Starting,
    Retrying,
    Ready,
    Failed,
}

impl State {
    pub fn as_str(self) -> &'static str {
        match self {
            State::Stopped => "stopped",
            State::Starting => "starting",
            State::Retrying => "retrying",
            State::Ready => "ready",
            State::Failed => "failed",
        }
    }
}

/// 一次尝试的失败性质：瞬时 → 可重试；永久 → 立即终止；取消 → 不做任何回填。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Failure {
    Transient(String),
    Permanent(String),
    Cancelled(String),
}

impl Failure {
    pub fn text(&self) -> &str {
        match self {
            Failure::Transient(text) | Failure::Permanent(text) | Failure::Cancelled(text) => text,
        }
    }
}

/// 一次尝试的结果（由启动执行方给出）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// 起来了；`service` 是业务服务健康判据（"ready" / "unknown" / "down"）。
    Ready { service: &'static str },
    Failed { phase: Phase, failure: Failure },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AttemptFailure {
    pub phase: Phase,
    pub failure: Failure,
}

impl Outcome {
    pub fn failed(phase: Phase, failure: Failure) -> Self {
        Outcome::Failed { phase, failure }
    }
    pub fn transient(phase: Phase, message: impl Into<String>) -> Self {
        Outcome::failed(phase, Failure::Transient(message.into()))
    }
    pub fn permanent(phase: Phase, message: impl Into<String>) -> Self {
        Outcome::failed(phase, Failure::Permanent(message.into()))
    }
    pub fn cancelled(message: impl Into<String>) -> Self {
        Outcome::failed(Phase::Verify, Failure::Cancelled(message.into()))
    }
}

/// 调度器交出去的一次尝试：身份、第几次、轮次、这次尝试可用的共享预算。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Assignment {
    pub token: Token,
    pub attempt: u32,
    /// 这一轮调度的编号（begin 每次 +1）：旧轮的 worker 回来时不许回填新轮（F3）。
    pub round: u64,
    pub budget: Duration,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Enqueue {
    pub started: bool,
    /// 同一 id 已在途（同代次）而被忽略。
    pub already: bool,
    /// 代次变了，旧 job 已作废。
    pub replaced: bool,
}

struct Job {
    token: Token,
    state: State,
    phase: Option<Phase>,
    attempts: u32,
    running: bool,
    permanent: bool,
    /// 结果在整轮生命周期到期之后才回来（F3）：显式记录，不悄悄当正常 ready。
    late: bool,
    /// 到期时这次尝试还在途（t46 F3-1）：让迟到 Ready 走 late 记账，而不是被终态守卫
    /// 直接吞掉；任何结果回来一次就清掉。
    expired_in_flight: bool,
    reason: Option<String>,
    retry_at: Option<Instant>,
    service: Option<&'static str>,
}

/// 自启调度器：纯状态机，时间一律由调用方传入。
pub struct Scheduler {
    jobs: BTreeMap<String, Job>,
    deadline: Instant,
    cold: bool,
    cold_budget: Duration,
    warm_budget: Duration,
    lifecycle: Duration,
    /// 这一轮的编号；`finish` 只认同一轮的 worker（F3）。
    round: u64,
    /// 重试退避；生产用 [`BACKOFF`]，测试可压成 0（F5 的 panic 回收用例）。
    backoff: [Duration; 3],
}

impl Scheduler {
    pub fn new(now: Instant, cold: bool, lifecycle: Duration, cold_budget: Duration, warm_budget: Duration) -> Self {
        Scheduler {
            jobs: BTreeMap::new(),
            deadline: now + lifecycle,
            cold,
            cold_budget,
            warm_budget,
            lifecycle,
            round: 1,
            backoff: BACKOFF,
        }
    }

    /// 测试/多轮调度用：绑定轮次编号。
    pub fn with_round(mut self, round: u64) -> Self {
        self.round = round;
        self
    }

    /// 测试用：把重试退避压短，让「worker panic → 重试 → 终态」能在秒内跑完。
    pub fn with_backoff(mut self, backoff: [Duration; 3]) -> Self {
        self.backoff = backoff;
        self
    }

    pub fn round(&self) -> u64 {
        self.round
    }

    pub fn deadline(&self) -> Instant {
        self.deadline
    }

    pub fn enqueue(&mut self, token: Token, now: Instant) -> Enqueue {
        let mut replaced = false;
        if let Some(job) = self.jobs.get(&token.id) {
            if job.token.generation == token.generation {
                return Enqueue { started: false, already: true, replaced: false };
            }
            replaced = true;
        }
        // 到期之后不再排队：直接记终态，别让 180 s 之后的调度假装还在跑。
        let expired = now >= self.deadline;
        self.jobs.insert(
            token.id.clone(),
            Job {
                token,
                state: if expired { State::Failed } else { State::Starting },
                phase: None,
                attempts: 0,
                running: false,
                permanent: false,
                late: false,
                expired_in_flight: false,
                reason: expired.then(|| format!("自启生命周期 {} 秒已用完，未再尝试", self.lifecycle.as_secs())),
                retry_at: (!expired).then_some(now),
                service: None,
            },
        );
        Enqueue { started: !expired, already: false, replaced }
    }

    /// 取走此刻可以开始的尝试（至多把空闲槽填满）。槽位按「正在跑」的 job 数算。
    pub fn take(&mut self, now: Instant) -> Vec<Assignment> {
        let running = self.jobs.values().filter(|job| job.running).count();
        let mut free = MAX_START_PARALLEL.saturating_sub(running);
        let mut out = Vec::new();
        if free == 0 {
            return out;
        }
        for job in self.jobs.values_mut() {
            if free == 0 {
                break;
            }
            if job.running || job.permanent || job.state == State::Ready || job.state == State::Stopped {
                continue;
            }
            if job.attempts >= MAX_ATTEMPTS || now >= self.deadline {
                continue;
            }
            let Some(at) = job.retry_at else { continue };
            if at > now {
                continue;
            }
            job.running = true;
            job.state = State::Starting;
            job.phase = None;
            job.attempts += 1;
            job.retry_at = None;
            out.push(Assignment {
                token: job.token.clone(),
                attempt: job.attempts,
                round: self.round,
                budget: if self.cold { self.cold_budget } else { self.warm_budget },
            });
            free -= 1;
        }
        out
    }

    /// 回填一次尝试的结果。迟到的代次或早已取消的 job 一律不认（A04）。
    pub fn finish(&mut self, assignment: &Assignment, outcome: Outcome, now: Instant) -> State {
        let Some(job) = self.jobs.get_mut(&assignment.token.id) else {
            return State::Stopped;
        };
        if job.token.generation != assignment.token.generation || job.attempts != assignment.attempt || assignment.round != self.round {
            // 上一代/上一轮/上一轮次的迟到结果：不动这一轮的状态。
            return job.state;
        }
        // t46/F3-1：到期时仍在途的那次尝试回来了。expire 已把它记 Failed，这里**先于**终态
        // 守卫处理，让迟到的 Ready 走 late 记账，而不是被当成普通终态直接吞掉。
        if job.expired_in_flight {
            job.expired_in_flight = false;
            job.running = false;
            if let Outcome::Ready { service } = outcome {
                job.late = true;
                job.phase = Some(Phase::Service);
                job.reason = Some(format!(
                    "启动在自启生命周期 {} 秒之后才完成，未计入正常就绪",
                    self.lifecycle.as_secs()
                ));
                job.permanent = false;
                job.retry_at = None;
                job.service = Some(service);
            }
            return job.state;
        }
        // 已取消（Stopped）或已判终态（Failed）的 job 不再接受任何结果：取消之后迟到的
        // Ready 不许把它重新标成 ready（A04）。
        if matches!(job.state, State::Stopped | State::Failed) {
            return job.state;
        }
        job.running = false;
        match outcome {
            Outcome::Ready { service } => {
                // F3：整轮生命周期到期之后才起来的结果不许静默变正常 ready——按到期失败记账，
                // 但把 late 标记与原因留下，供卡片/日志说明「确实是超预算才成功的」。
                if now >= self.deadline {
                    job.state = State::Failed;
                    job.late = true;
                    job.phase = Some(Phase::Service);
                    job.reason = Some(format!(
                        "启动在自启生命周期 {} 秒之后才完成，未计入正常就绪",
                        self.lifecycle.as_secs()
                    ));
                    job.permanent = false;
                    job.retry_at = None;
                    job.service = Some(service);
                } else {
                    job.state = State::Ready;
                    job.phase = None;
                    job.reason = None;
                    job.permanent = false;
                    job.late = false;
                    job.retry_at = None;
                    job.service = Some(service);
                }
            }
            Outcome::Failed { phase, failure } => {
                job.phase = Some(phase);
                match failure {
                    Failure::Cancelled(reason) => {
                        job.state = State::Stopped;
                        job.reason = Some(reason);
                        job.retry_at = None;
                        job.service = None;
                    }
                    Failure::Permanent(reason) => {
                        job.state = State::Failed;
                        job.permanent = true;
                        job.reason = Some(reason);
                        job.retry_at = None;
                        job.service = None;
                    }
                    Failure::Transient(reason) => {
                        let backoff = self.backoff.get(job.attempts.saturating_sub(1) as usize).copied();
                        let next = backoff.map(|wait| now + wait);
                        // 三次重试用完、或退避之后已越过生命周期：不再开新尝试，但把实际
                        // 尝试次数与原因留在终态里（不承诺「最终一定起来」）。
                        let exhausted = job.attempts >= MAX_ATTEMPTS;
                        let past_lifecycle = next.is_none_or(|at| at >= self.deadline);
                        if exhausted || past_lifecycle || now >= self.deadline {
                            job.state = State::Failed;
                            job.permanent = false;
                            job.reason = Some(format!(
                                "{reason}（已尝试 {} 次{}）",
                                job.attempts,
                                if past_lifecycle || now >= self.deadline { "，自启生命周期已用完" } else { "，重试与生命周期均已用完" }
                            ));
                            job.retry_at = None;
                        } else {
                            job.state = State::Retrying;
                            job.reason = Some(reason);
                            job.retry_at = next;
                        }
                        job.service = None;
                    }
                }
            }
        }
        job.state
    }

    /// 取消一个插件的在途/待重试启动（停用、卸载、换版、宿主退出）。
    ///
    /// 2.0.4 t33（F2）：**Ready / Failed 也要清成 Stopped** —— 手动启停成功之后，卡片上
    /// 不许再挂着上一轮自启的「失败/已就绪」理由；这些终态只属于那一次调度。
    pub fn cancel(&mut self, id: &str, reason: &str) {
        if let Some(job) = self.jobs.get_mut(id) {
            job.state = State::Stopped;
            job.running = false;
            job.reason = Some(reason.to_string());
            job.permanent = false;
            job.late = false;
            job.retry_at = None;
            job.phase = None;
            job.service = None;
        }
    }

    /// 手动启动/停用已经落到实例表：把这一轮的调度记录整个摘掉，卡片改看实时 running。
    pub fn forget(&mut self, id: &str) {
        self.jobs.remove(id);
    }

    pub fn cancel_all(&mut self, reason: &str) {
        let ids: Vec<String> = self.jobs.keys().cloned().collect();
        for id in ids {
            self.cancel(&id, reason);
        }
    }

    /// 生命周期到期的收尾：不再开新尝试；在途结果不再可能把状态改成正常 ready。
    pub fn expire(&mut self, now: Instant) {
        if now < self.deadline {
            return;
        }
        for job in self.jobs.values_mut() {
            if job.permanent || job.state == State::Ready || job.state == State::Stopped || job.state == State::Failed {
                continue;
            }
            job.retry_at = None;
            // 在途的那次先记到期失败并清 running（F5-2：不能再显示「仍在跑」）；同时留下
            // expired_in_flight，迟到 Ready 由 finish 的到期分支记 late（t46 F3-1）。
            let was_running = job.running;
            job.running = false;
            job.expired_in_flight = was_running;
            job.state = State::Failed;
            job.reason = Some(format!(
                "自启生命周期 {} 秒内未能启动完成（已尝试 {} 次）",
                self.lifecycle.as_secs(),
                job.attempts
            ));
        }
    }

    pub fn pending(&self) -> bool {
        self.jobs.values().any(|job| matches!(job.state, State::Starting | State::Retrying))
    }

    pub fn running(&self) -> usize {
        self.jobs.values().filter(|job| job.running).count()
    }

    /// 下一次需要醒来的时刻：待重试的最早一个，或生命周期终点。
    pub fn next_wake(&self) -> Instant {
        self.jobs
            .values()
            .filter(|job| !job.running && matches!(job.state, State::Starting | State::Retrying))
            .filter_map(|job| job.retry_at)
            .min()
            .unwrap_or(self.deadline)
            .min(self.deadline)
    }

    pub fn state_of(&self, id: &str) -> Option<State> {
        self.jobs.get(id).map(|job| job.state)
    }

    /// 插件卡消费的生命周期载荷（contracts §5.1 的字段名与稳定枚举）。
    pub fn snapshot(&self, now: Instant) -> Value {
        let rows: Vec<Value> = self
            .jobs
            .values()
            .map(|job| {
                json!({
                    "id": job.token.id,
                    "state": job.state.as_str(),
                    "phase": job.phase.map(Phase::as_str),
                    "attempt": job.attempts,
                    "maxAttempts": MAX_ATTEMPTS,
                    "reason": job.reason,
                    "retryInMs": job.retry_at.map(|at| at.saturating_duration_since(now).as_millis() as u64),
                    "permanent": job.permanent,
                    "late": job.late,
                    "service": job.service,
                })
            })
            .collect();
        json!(rows)
    }

    /// 一条给人读的收尾统计。
    pub fn summary(&self) -> String {
        let ready = self.jobs.values().filter(|job| job.state == State::Ready).count();
        let failed = self.jobs.values().filter(|job| job.state == State::Failed).count();
        let stopped = self.jobs.values().filter(|job| job.state == State::Stopped).count();
        let attempts: u32 = self.jobs.values().map(|job| job.attempts).sum();
        format!("ready={ready} failed={failed} stopped={stopped} attempts={attempts}")
    }
}

// —— 进程入口的单调时钟（冷态判据的第二个锚点）——
static STARTED: OnceLock<Instant> = OnceLock::new();

/// 在进程入口标记一次（只认第一次）：冷态判据 `since_process_start` 从此刻算。
pub fn mark_process_start() {
    let _ = STARTED.set(Instant::now());
}

/// 进程启动至今的秒数；没标记过（测试里直接调 getter）就是 None，不当 0。
pub fn since_process_start() -> Option<f64> {
    STARTED.get().map(|at| at.elapsed().as_secs_f64())
}

// —— 全局调度器（生产路径）——
static SCHEDULER: Mutex<Option<Scheduler>> = Mutex::new(None);
// 轮次编号（F3）：每次 begin 都换一轮；旧轮的 worker 回来时 `finish` 按 round 丢弃。
static ROUND: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// 开一轮自启调度：清掉上一轮、记下生命周期起点，并返回新的轮次编号。
pub fn begin(cold: bool, lifecycle: Duration, cold_budget: Duration, warm_budget: Duration) -> u64 {
    let round = ROUND.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1;
    let mut slot = SCHEDULER.lock().unwrap_or_else(|poison| poison.into_inner());
    *slot = Some(Scheduler::new(Instant::now(), cold, lifecycle, cold_budget, warm_budget).with_round(round));
    round
}

pub fn enqueue(token: Token) {
    let mut slot = SCHEDULER.lock().unwrap_or_else(|poison| poison.into_inner());
    if let Some(scheduler) = slot.as_mut() {
        scheduler.enqueue(token, Instant::now());
    }
}

/// 插件卡用：没有调度轮次时就是空数组。
pub fn snapshot() -> Value {
    let slot = SCHEDULER.lock().unwrap_or_else(|poison| poison.into_inner());
    match slot.as_ref() {
        Some(scheduler) => scheduler.snapshot(Instant::now()),
        None => json!([]),
    }
}

pub fn cancel(id: &str, reason: &str) {
    let mut slot = SCHEDULER.lock().unwrap_or_else(|poison| poison.into_inner());
    if let Some(scheduler) = slot.as_mut() {
        scheduler.cancel(id, reason);
    }
}

pub fn cancel_all(reason: &str) {
    let mut slot = SCHEDULER.lock().unwrap_or_else(|poison| poison.into_inner());
    if let Some(scheduler) = slot.as_mut() {
        scheduler.cancel_all(reason);
    }
}

/// 手动启动/停用已经由实例表定案：摘掉这一轮的调度记录，卡片改看实时 running（F2）。
pub fn forget(id: &str) {
    let mut slot = SCHEDULER.lock().unwrap_or_else(|poison| poison.into_inner());
    if let Some(scheduler) = slot.as_mut() {
        scheduler.forget(id);
    }
}

/// 这一轮是否还在生命周期内（F3）：拉起成功之后、登记实例之前再问一次；到期后的
/// 成功结果按「丢弃」处理并停掉刚起的进程，不让它变成调度外运行的健康实例。
pub fn accepting() -> bool {
    let slot = SCHEDULER.lock().unwrap_or_else(|poison| poison.into_inner());
    slot.as_ref().is_some_and(|scheduler| Instant::now() < scheduler.deadline)
}

/// 结果是否属于当前这一轮（t46 F1-1）：install_instance 除了 stopping+generation，
/// 还要看轮次——旧轮 worker 无论先到后到都不能回填/替换新轮的实例。
pub fn is_current_round(round: u64) -> bool {
    let slot = SCHEDULER.lock().unwrap_or_else(|poison| poison.into_inner());
    slot.as_ref().is_some_and(|scheduler| scheduler.round == round)
}

/// 跑完整轮调度：至多两槽并发，结果回填后再排下一次。
///
/// `attempt` 在锁外执行（可以校验、spawn、握手），只有「取任务/回填」拿短锁。
/// t14 repair F3：`host` 用于终态落盘（ready/stopped 写 plugin-failures.log）。
pub fn drive(host: Option<&Path>, attempt: impl Fn(&Assignment) -> Outcome + Sync) {
    drive_in(&SCHEDULER, host, attempt);
}

/// worker 的一层兜底（F5）：`attempt` panic 也必须在**槽位回收并回填失败**之后才退出，
/// 否则 `running` 只增不减、调度线程空转、卡片永远「启动中」。panic 归为瞬时错误，
/// 由调度器按既有次数/生命周期继续决定重试或终态。
fn guarded_outcome(attempt: &impl Fn(&Assignment) -> Outcome, assignment: &Assignment) -> Outcome {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| attempt(assignment))) {
        Ok(outcome) => outcome,
        Err(_) => Outcome::transient(Phase::Spawn, "启动 worker 异常退出（已回收槽位，将按次数重试）"),
    }
}

/// `drive` 的真实循环：调度器由调用方给（生产是全局 SCHEDULER，测试用本地实例）。
/// t14 repair F3：`host` 用于终态落盘（ready/stopped 写 plugin-failures.log）。
fn drive_in(slot: &Mutex<Option<Scheduler>>, host: Option<&Path>, attempt: impl Fn(&Assignment) -> Outcome + Sync) {
    // 一个 scope 罩住整轮循环：两槽 worker 与调度循环同时活着，每轮的 join 在整轮结尾。
    std::thread::scope(|scope| {
        let (tell, heard) = std::sync::mpsc::channel::<(Assignment, Outcome)>();
        let attempt = &attempt;
        let mut running = 0usize;
        let mut warned_abandoned = false;
        loop {
            let now = Instant::now();
            let (dispatch, pending, wake, overdue) = {
                let mut locked = slot.lock().unwrap_or_else(|poison| poison.into_inner());
                match locked.as_mut() {
                    Some(scheduler) => {
                        scheduler.expire(now);
                        (scheduler.take(now), scheduler.pending(), scheduler.next_wake(), now >= scheduler.deadline())
                    }
                    None => break,
                }
            };
            for assignment in dispatch {
                running += 1;
                let tell = tell.clone();
                scope.spawn(move || {
                    let outcome = guarded_outcome(attempt, &assignment);
                    let _ = tell.send((assignment, outcome));
                });
            }
            if running == 0 && !pending {
                break;
            }
            // t46/F5-3：到期后 drive 不会强杀在途 worker（Rust 没有安全的线程强杀），
            // `thread::scope` 最终仍要 join 它们。这里不再假装「已放弃」，而是阻塞等结果：
            // worker 回来时 finish 走到期/late 分支；真卡死时整轮同样卡在 join——如实口径。
            let abandoned = running > 0 && !pending && overdue;
            if abandoned && !warned_abandoned {
                eprintln!("FISH Dock: 插件自启到期，仍有 {running} 个在途 worker；无法强杀，等待其结束后按迟到结果记账");
                warned_abandoned = true;
            }
            let received = if abandoned {
                heard.recv().map_err(|_| std::sync::mpsc::RecvTimeoutError::Disconnected)
            } else {
                let wait = wake.saturating_duration_since(Instant::now()).min(DRIVE_WAIT_SLICE);
                heard.recv_timeout(wait.max(DRIVE_WAIT_FLOOR))
            };
            match received {
                Ok((assignment, outcome)) => {
                    warned_abandoned = false;
                    running = running.saturating_sub(1);
                    let late_ready = matches!(&outcome, Outcome::Ready { .. });
                    let state = {
                        let mut locked = slot.lock().unwrap_or_else(|poison| poison.into_inner());
                        locked.as_mut()
                            .map(|scheduler| scheduler.finish(&assignment, outcome, Instant::now()))
                            .unwrap_or(State::Stopped)
                    };
                    if late_ready && state == State::Failed {
                        // 到期之后才登记成功的实例：结果已按到期失败记账，进程也要收掉。
                        crate::plugin_runtime::discard_late(&assignment.token.id, assignment.token.generation);
                    }
                    log_finish(host, &assignment, state);
                }
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => continue,
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
            }
        }
    });
    let summary = {
        let locked = slot.lock().unwrap_or_else(|poison| poison.into_inner());
        locked.as_ref().map(Scheduler::summary).unwrap_or_default()
    };
    eprintln!("FISH Dock: 插件自启一轮结束 {summary}");
}

// t14 repair F3：终态落盘。failed 已在 scheduled_attempt 的失败分支写过（带 phase/elapsed），
// 这里补 ready/stopped/late 三种终态，让 plugin-failures.log 成为完整的启动事件日志。
// state 字段区分成功与失败：state="ready" 表示启动成功，state="stopped"/"failed" 表示失败/取消。
fn log_finish(host: Option<&Path>, assignment: &Assignment, state: State) {
    eprintln!(
        "FISH Dock: [plugin-start] id={} attempt={}/{} state={}",
        assignment.token.id,
        assignment.attempt,
        MAX_ATTEMPTS,
        state.as_str()
    );
    // failed 已经在 scheduled_attempt 的失败分支通过 record_standalone_failure 写过，不重复。
    if state == State::Failed {
        return;
    }
    // ready/stopped 也落一条，复用 append_failure_row 同一结构。
    if let Some(host) = host {
        let state_str = state.as_str().to_string();
        let budget_cold = Some(crate::config::plugin_startup_is_cold());
        let raw = match state {
            State::Ready => "启动成功",
            State::Stopped => "启动取消",
            _ => "启动终态",
        };
        crate::plugin_runtime::record_standalone_failure(
            host,
            &assignment.token.id,
            &assignment.token.version,
            "启动",
            None,
            Some(assignment.attempt),
            None,
            Some(state_str),
            budget_cold,
            None,
            raw,
        );
    }
}

#[cfg(test)]
pub(crate) static GLOBAL_ROUND_TEST: Mutex<()> = Mutex::new(());

#[cfg(test)]
mod tests {
    use super::*;

    fn token(id: &str, generation: u64) -> Token {
        Token { id: id.into(), generation, version: "1.0.0".into(), digest: "synthetic-digest".into() }
    }
    fn scheduler(now: Instant, cold: bool) -> Scheduler {
        Scheduler::new(now, cold, Duration::from_secs(180), Duration::from_secs(60), Duration::from_secs(10))
    }
    fn transient() -> Outcome {
        Outcome::transient(Phase::Spawn, "合成瞬时失败")
    }

    // A02：一次尝试只拿一个共享预算（冷 60 / 热 10），不是每个阶段各一份。
    #[test]
    fn one_attempt_gets_one_shared_budget() {
        let t0 = Instant::now();
        let mut cold = scheduler(t0, true);
        cold.enqueue(token("fish.clash", 0), t0);
        let cold_take = cold.take(t0);
        assert_eq!(cold_take.len(), 1);
        assert_eq!(cold_take[0].budget, Duration::from_secs(60), "冷态一次尝试共享 60 秒");

        let mut warm = scheduler(t0, false);
        warm.enqueue(token("fish.clash", 0), t0);
        let warm_take = warm.take(t0);
        assert_eq!(warm_take[0].budget, Duration::from_secs(10), "热态一次尝试共享 10 秒");
    }

    // A02：至多首次 + 3 次重试，退避 0.5 / 2 / 5 秒；第 4 次失败就是终态。
    #[test]
    fn at_most_three_retries_with_the_fixed_backoff() {
        let t0 = Instant::now();
        let mut s = scheduler(t0, true);
        s.enqueue(token("fish.clash", 0), t0);
        let first = s.take(t0);
        assert_eq!(first[0].attempt, 1);
        assert_eq!(s.finish(&first[0], transient(), t0), State::Retrying);
        assert!(s.take(t0 + Duration::from_millis(400)).is_empty(), "500 ms 之内不许重试");

        let second = s.take(t0 + Duration::from_millis(500));
        assert_eq!(second[0].attempt, 2, "第 1 次重试在 500 ms 之后");
        let at2 = t0 + Duration::from_millis(500);
        assert_eq!(s.finish(&second[0], transient(), at2), State::Retrying);
        assert!(s.take(at2 + Duration::from_millis(1999)).is_empty(), "2 秒之内不许重试");

        let third = s.take(at2 + Duration::from_secs(2));
        assert_eq!(third[0].attempt, 3, "第 2 次重试在 2 秒之后");
        let at3 = at2 + Duration::from_secs(2);
        assert_eq!(s.finish(&third[0], transient(), at3), State::Retrying);
        assert!(s.take(at3 + Duration::from_millis(4999)).is_empty(), "5 秒之内不许重试");

        let fourth = s.take(at3 + Duration::from_secs(5));
        assert_eq!(fourth[0].attempt, 4, "第 3 次重试在 5 秒之后");
        let at4 = at3 + Duration::from_secs(5);
        assert_eq!(s.finish(&fourth[0], transient(), at4), State::Failed, "首次 + 3 次重试后就该停");
        assert!(!s.pending());
        assert!(s.take(at4 + Duration::from_secs(3600)).is_empty(), "终态不再开新尝试");
        let row = &s.snapshot(at4)[0];
        assert_eq!(row["attempt"], json!(4));
        assert_eq!(row["permanent"], false);
        assert!(row["reason"].as_str().unwrap().contains("已尝试 4 次"), "{row}");
    }

    // A02：整轮 180 秒到点不再开新尝试，并把实际尝试次数写进终态。
    #[test]
    fn the_lifecycle_budget_stops_new_attempts() {
        let t0 = Instant::now();
        let mut s = scheduler(t0, true);
        s.enqueue(token("fish.magpie", 0), t0);
        let first = s.take(t0);
        let late = t0 + Duration::from_secs(179);
        assert_eq!(s.finish(&first[0], transient(), late), State::Retrying, "179 秒时还能排一次重试");
        let retry = s.take(late + Duration::from_millis(500));
        assert_eq!(retry.len(), 1, "退避之后仍在 180 秒内");
        let at = late + Duration::from_millis(500);
        assert_eq!(s.finish(&retry[0], transient(), at), State::Failed);
        assert!(!s.pending());
        assert!(s.snapshot(at)[0]["reason"].as_str().unwrap().contains("自启生命周期"), "{}", s.snapshot(at));
        assert!(s.take(t0 + Duration::from_secs(200)).is_empty(), "180 秒之后不许开新尝试");
    }

    // A02/A04：两槽并发；慢插件（一直在跑）不挡其他候选的首次尝试。
    #[test]
    fn two_plugins_start_at_a_time_and_a_slow_one_does_not_block_others() {
        let t0 = Instant::now();
        let mut s = scheduler(t0, true);
        for id in ["a", "b", "c", "d", "e"] {
            s.enqueue(token(id, 0), t0);
        }
        let first = s.take(t0);
        assert_eq!(first.len(), MAX_START_PARALLEL, "一次最多两个");
        assert!(s.take(t0).is_empty(), "两槽都占着时不许再开");
        assert_eq!(s.running(), 2);
        // a 很慢（还没回），b 立刻失败 → 一个槽空出来，c 可以开始。
        let b = first.iter().find(|a| a.token.id == "b").unwrap().clone();
        assert_eq!(s.finish(&b, transient(), t0), State::Retrying);
        let next = s.take(t0);
        assert_eq!(next.len(), 1, "空出来的一个槽给下一个候选");
        assert_ne!(next[0].token.id, "b", "b 在退避，不该立刻再抢槽");
        assert_eq!(s.running(), 2);
    }

    // A04：每插件只有一个在途 token；同代次重复入队按「已在途」忽略。
    #[test]
    fn a_plugin_has_only_one_attempt_in_flight() {
        let t0 = Instant::now();
        let mut s = scheduler(t0, true);
        let first = s.enqueue(token("fish.clash", 3), t0);
        assert!(first.started && !first.already);
        let again = s.enqueue(token("fish.clash", 3), t0 + Duration::from_millis(1));
        assert!(!again.started && again.already, "同代次重复入队必须被忽略");
        let taken = s.take(t0 + Duration::from_millis(2));
        assert_eq!(taken.len(), 1);
        assert!(s.take(t0 + Duration::from_millis(3)).is_empty(), "在途期间不许第二次 spawn");
        assert_eq!(taken[0].token.generation, 3);
    }

    // A04：代次变了（停用/卸载/换版）旧 token 作废，迟到的结果不许回填。
    #[test]
    fn a_new_generation_drops_a_late_result() {
        let t0 = Instant::now();
        let mut s = scheduler(t0, true);
        s.enqueue(token("fish.clash", 0), t0);
        let old = s.take(t0)[0].clone();
        let replaced = s.enqueue(token("fish.clash", 1), t0 + Duration::from_millis(10));
        assert!(replaced.replaced && replaced.started);
        let fresh = s.take(t0 + Duration::from_millis(20));
        assert_eq!(fresh.len(), 1);
        assert_eq!(fresh[0].token.generation, 1);
        // 旧代次的结果迟到：不许把新代次标成 ready，也不许清掉在途状态。
        let after_old = s.finish(&old, Outcome::Ready { service: "ready" }, t0 + Duration::from_millis(30));
        assert_eq!(after_old, State::Starting);
        assert_eq!(s.snapshot(t0)[0]["state"], "starting");
        // 新代次自己的结果照常回填。
        assert_eq!(s.finish(&fresh[0], Outcome::Ready { service: "ready" }, t0 + Duration::from_millis(40)), State::Ready);
    }

    // 永久错误（身份协议不匹配、摘要失败、非法配置）立即终止，不重试。
    #[test]
    fn permanent_errors_are_not_retried() {
        let t0 = Instant::now();
        let mut s = scheduler(t0, true);
        s.enqueue(token("fish.clash", 0), t0);
        let assignment = s.take(t0)[0].clone();
        let state = s.finish(
            &assignment,
            Outcome::permanent(Phase::Hello, "插件握手身份或协议不匹配"),
            t0,
        );
        assert_eq!(state, State::Failed);
        assert!(s.take(t0 + Duration::from_secs(60)).is_empty(), "永久错误不许重试");
        let row = &s.snapshot(t0)[0];
        assert_eq!(row["permanent"], true);
        assert_eq!(row["phase"], "hello");
        assert_eq!(row["state"], "failed");
    }

    // 取消（停用/卸载/退出）作废在途尝试，迟到的结果只记在已取消的 job 上。
    #[test]
    fn cancel_stops_a_pending_retry_and_a_late_result() {
        let t0 = Instant::now();
        let mut s = scheduler(t0, true);
        s.enqueue(token("fish.clash", 0), t0);
        let assignment = s.take(t0)[0].clone();
        s.cancel("fish.clash", "用户已停用");
        assert_eq!(s.state_of("fish.clash"), Some(State::Stopped));
        assert!(!s.pending());
        assert_eq!(s.finish(&assignment, Outcome::Ready { service: "ready" }, t0), State::Stopped);
        assert!(s.take(t0 + Duration::from_secs(1)).is_empty());
        s.cancel_all("宿主退出");
        assert!(!s.pending());
    }

    // 插件卡载荷：字段名与稳定枚举一次定死（contracts §5.1）。
    #[test]
    fn the_card_payload_has_the_stable_field_names() {
        let t0 = Instant::now();
        let mut s = scheduler(t0, true);
        s.enqueue(token("fish.clash", 0), t0);
        let row = s.snapshot(t0)[0].clone();
        for field in ["id", "state", "phase", "attempt", "maxAttempts", "reason", "retryInMs", "permanent"] {
            assert!(row.get(field).is_some(), "缺字段 {field}：{row}");
        }
        assert_eq!(row["id"], "fish.clash");
        assert_eq!(row["state"], "starting");
        assert_eq!(row["phase"], Value::Null, "还没失败时阶段是 null");
        assert_eq!(row["attempt"], json!(0));
        assert_eq!(row["maxAttempts"], json!(MAX_ATTEMPTS));
        assert_eq!(row["retryInMs"], json!(0));
        assert_eq!(row["permanent"], false);
    }

    // 进程入口时钟：标记之前是 None（未知不当 0），标记之后是正数。
    #[test]
    fn the_process_clock_is_unknown_until_marked() {
        // 别的用例可能已经标记过；这里只验证「标记之后能读到」。
        mark_process_start();
        let since = since_process_start().expect("标记之后必须读得到");
        assert!(since >= 0.0);
    }

    // —— 2.0.4 t33（F2/F3/F5）：终态清理、轮次、到期与 worker 异常 ——

    // F2：Ready/Failed 是**上一轮**的终态；手动启停之后必须清成 Stopped 并清掉理由。
    #[test]
    fn cancel_clears_a_ready_or_failed_terminal_state() {
        let t0 = Instant::now();
        for outcome in [Outcome::Ready { service: "ready" }, Outcome::permanent(Phase::Hello, "合成永久失败")] {
            let mut s = scheduler(t0, true);
            s.enqueue(token("fish.clash", 0), t0);
            let assignment = s.take(t0)[0].clone();
            let terminal = s.finish(&assignment, outcome, t0);
            assert!(matches!(terminal, State::Ready | State::Failed));
            s.cancel("fish.clash", "用户手动停用");
            let row = s.snapshot(t0)[0].clone();
            assert_eq!(row["state"], "stopped", "终态也要清成 stopped：{row}");
            assert_eq!(row["permanent"], false, "旧失败理由不许挂在新状态上");
            assert_eq!(row["reason"], "用户手动停用");
            assert_eq!(row["retryInMs"], Value::Null);
            assert_eq!(row["service"], Value::Null);
        }
    }

    // F2：手动启动成功后 forget 掉这一轮记录，卡片回落实时 running（不再显示旧 failed）。
    #[test]
    fn forget_removes_the_scheduling_record_so_the_card_follows_the_instance() {
        let t0 = Instant::now();
        let mut s = scheduler(t0, true);
        s.enqueue(token("fish.clash", 0), t0);
        let assignment = s.take(t0)[0].clone();
        s.finish(&assignment, Outcome::permanent(Phase::Verify, "合成失败"), t0);
        assert_eq!(s.state_of("fish.clash"), Some(State::Failed));
        s.forget("fish.clash");
        assert_eq!(s.state_of("fish.clash"), None, "记录摘掉后 view 只能回落 running");
        assert_eq!(s.snapshot(t0).as_array().unwrap().len(), 0);
    }

    // F3：轮次不同的迟到结果不许回填新轮。
    #[test]
    fn a_worker_from_an_older_round_cannot_fill_the_new_round() {
        let t0 = Instant::now();
        let old = scheduler(t0, true).with_round(1);
        let mut old = old;
        old.enqueue(token("fish.clash", 0), t0);
        let stale = old.take(t0)[0].clone();
        // begin 换了一轮：同一 id/代次/attempt 编号在新轮里再取一次。
        let mut fresh = scheduler(t0, true).with_round(2);
        fresh.enqueue(token("fish.clash", 0), t0);
        let assignment = fresh.take(t0)[0].clone();
        assert_eq!(assignment.round, 2);
        let state = fresh.finish(&stale, Outcome::Ready { service: "ready" }, t0);
        assert_eq!(state, State::Starting, "旧轮结果不许把新轮标成 ready");
        assert_eq!(fresh.snapshot(t0)[0]["state"], "starting");
    }

    // F3：生命周期之后才 Ready 的结果必须留 late 标记，不能静默变成正常 ready。
    #[test]
    fn a_ready_that_arrives_after_the_lifecycle_is_recorded_as_late_not_ready() {
        let t0 = Instant::now();
        let mut s = scheduler(t0, true);
        s.enqueue(token("fish.magpie", 0), t0);
        let assignment = s.take(t0)[0].clone();
        let late = t0 + Duration::from_secs(181);
        let state = s.finish(&assignment, Outcome::Ready { service: "ready" }, late);
        assert_eq!(state, State::Failed, "超预算成功不能当正常 ready");
        let row = s.snapshot(late)[0].clone();
        assert_eq!(row["late"], true, "要留下 late 标记：{row}");
        assert!(row["reason"].as_str().unwrap().contains("生命周期"), "{row}");
    }

    // F5：到期时在途 job 也记终态，`pending` 收敛（不会永远显示「启动中」）。
    #[test]
    fn expiry_marks_an_in_flight_attempt_failed_and_pending_stops() {
        let t0 = Instant::now();
        let mut s = scheduler(t0, true);
        s.enqueue(token("fish.magpie", 0), t0);
        let _assignment = s.take(t0)[0].clone();
        s.expire(t0 + Duration::from_secs(181));
        assert!(!s.pending(), "到期后不再有待重试");
        assert_eq!(s.snapshot(t0)[0]["state"], "failed");
        assert_eq!(s.running(), 0, "到期后在途标记要清掉，不能一直显示仍在跑（t46 F5-2）");
        assert!(s.take(t0 + Duration::from_secs(200)).is_empty());
    }

    // F3：全局调度到点之后 `accepting()` 必须为假，拉起成功的实例不许再登记。
    #[test]
    fn accepting_reflects_the_global_lifecycle_deadline() {
        let _serial = GLOBAL_ROUND_TEST.lock().unwrap_or_else(|poison| poison.into_inner());
        begin(true, Duration::from_secs(60), Duration::from_secs(60), Duration::from_secs(10));
        assert!(accepting(), "刚开一轮的 60 秒生命周期内应当接受结果");
        begin(true, Duration::ZERO, Duration::from_secs(60), Duration::from_secs(10));
        assert!(!accepting(), "生命周期为 0 的轮次必须拒绝登记");
    }
    // t46/F1-1：install_instance 的轮次守卫依据——旧轮编号不得被认作当前轮。
    #[test]
    fn an_older_round_is_not_current() {
        let _serial = GLOBAL_ROUND_TEST.lock().unwrap_or_else(|poison| poison.into_inner());
        let first = begin(true, Duration::from_secs(60), Duration::from_secs(60), Duration::from_secs(10));
        assert!(is_current_round(first), "刚开始的轮次是当前轮");
        let second = begin(true, Duration::from_secs(60), Duration::from_secs(60), Duration::from_secs(10));
        assert_ne!(first, second);
        assert!(!is_current_round(first), "旧轮 worker 回来后不许被当成当前轮（t46 F1-1）");
        assert!(is_current_round(second));
    }
    // F3（t46）：真实 drive→expire→迟到 Ready 的路径也必须记 late=true；不能只直调 finish。
    #[test]
    fn a_late_ready_through_drive_and_expire_is_recorded_late() {
        let t0 = Instant::now();
        let slot = Mutex::new(Some(
            Scheduler::new(t0, true, Duration::from_millis(120), Duration::from_secs(10), Duration::from_secs(5))
                .with_round(5),
        ));
        {
            let mut locked = slot.lock().unwrap();
            locked.as_mut().unwrap().enqueue(token("fish.magpie", 0), Instant::now());
        }
        drive_in(&slot, None, |_assignment| {
            std::thread::sleep(Duration::from_millis(250));
            Outcome::Ready { service: "ready" }
        });
        let locked = slot.lock().unwrap();
        let scheduler = locked.as_ref().unwrap();
        let row = scheduler.snapshot(Instant::now())[0].clone();
        assert_eq!(row["state"], "failed", "超预算成功不能当正常 ready：{row}");
        assert_eq!(row["late"], true, "真实 expire→迟到 Ready 路径必须留 late 标记：{row}");
        assert!(row["reason"].as_str().unwrap().contains("生命周期"), "{row}");
        assert_eq!(scheduler.running(), 0, "到期后在途标记要清掉");
        assert!(!scheduler.pending());
    }
    // F5：worker panic 也必须回填失败并回收槽位；重试用完即终态。drive 不再空转，但真正
    // 卡死的 worker 无法强杀（thread::scope 仍会 join），见 pending.md 的口径。
    #[test]
    fn a_panicking_worker_is_reclaimed_and_the_round_terminates() {
        let t0 = Instant::now();
        let slot = Mutex::new(Some(
            Scheduler::new(t0, true, Duration::from_secs(30), Duration::from_secs(10), Duration::from_secs(5))
                .with_round(7)
                .with_backoff([Duration::ZERO, Duration::ZERO, Duration::ZERO]),
        ));
        {
            let mut locked = slot.lock().unwrap();
            locked.as_mut().unwrap().enqueue(token("fish.clash", 0), Instant::now());
        }
        let calls = std::sync::atomic::AtomicU32::new(0);
        let began = Instant::now();
        drive_in(&slot, None, |_assignment| {
            calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            panic!("合成 worker 崩溃");
        });
        assert!(began.elapsed() < Duration::from_secs(2), "with_backoff 必须真正生效（不再等 0.5+2+5 s）：{:?}", began.elapsed());
        assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), MAX_ATTEMPTS, "panic 也要按次数重试到终态");
        let locked = slot.lock().unwrap();
        let scheduler = locked.as_ref().unwrap();
        assert!(!scheduler.pending(), "panic 之后不许永远「启动中」");
        assert_eq!(scheduler.state_of("fish.clash"), Some(State::Failed));
    }
    // t14 repair F3：终态落盘测试。验证 ready/stopped 状态写入 plugin-failures.log。
    #[test]
    fn log_finish_writes_ready_and_stopped_to_failure_log() {
        use std::io::Read;
        let temp_dir = std::env::temp_dir().join(format!("fish-log-finish-test-{}", std::process::id()));
        std::fs::create_dir_all(&temp_dir).unwrap();
        let host_path = temp_dir.as_path();
        // 测试 ready 状态落盘。
        let assignment = Assignment {
            token: Token {
                id: "test.ready".into(),
                generation: 1,
                version: "1.0.0".into(),
                digest: "test-digest".into(),
            },
            attempt: 1,
            round: 1,
            budget: Duration::from_secs(60),
        };
        log_finish(Some(host_path), &assignment, State::Ready);
        // 验证文件存在且包含 ready 状态。
        let log_file = host_path.join("plugin-failures.log");
        assert!(log_file.exists(), "plugin-failures.log 必须存在");
        let mut content = String::new();
        std::fs::File::open(&log_file).unwrap().read_to_string(&mut content).unwrap();
        assert!(content.contains("\"state\":\"ready\""), "必须包含 state=ready：{}", content);
        assert!(content.contains("\"plugin\":\"test.ready\""), "必须包含 plugin id：{}", content);
        // 测试 stopped 状态落盘。
        let assignment2 = Assignment {
            token: Token {
                id: "test.stopped".into(),
                generation: 1,
                version: "1.0.0".into(),
                digest: "test-digest".into(),
            },
            attempt: 1,
            round: 1,
            budget: Duration::from_secs(60),
        };
        log_finish(Some(host_path), &assignment2, State::Stopped);
        content.clear();
        std::fs::File::open(&log_file).unwrap().read_to_string(&mut content).unwrap();
        assert!(content.contains("\"state\":\"stopped\""), "必须包含 state=stopped：{}", content);
        // 测试 failed 状态不重复写入（已在 scheduled_attempt 写过）。
        let assignment3 = Assignment {
            token: Token {
                id: "test.failed".into(),
                generation: 1,
                version: "1.0.0".into(),
                digest: "test-digest".into(),
            },
            attempt: 1,
            round: 1,
            budget: Duration::from_secs(60),
        };
        let before_len = std::fs::metadata(&log_file).unwrap().len();
        log_finish(Some(host_path), &assignment3, State::Failed);
        let after_len = std::fs::metadata(&log_file).unwrap().len();
        assert_eq!(before_len, after_len, "failed 状态不应重复写入");
        // 清理。
        std::fs::remove_dir_all(&temp_dir).ok();
    }
}
