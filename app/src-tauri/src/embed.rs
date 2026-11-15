// DEC-3：把灵动岛嵌入 Windows 任务栏（SetParent + UIA + 子窗口生命周期）的**安全决策核心**。
//
// 嵌入是平台级子系统（SetParent 到 Shell_TrayWnd、UI Automation 保住可访问性、Explorer 重启
// 后重挂、任务栏自动隐藏/多任务栏）。真正拿窗口句柄、SetParent、UIA 的部分在平台绑定里做；
// 这里只放**可测的安全默认决策**，页面/平台层据此执行——绝不让「嵌入失败」变成白屏或丢交互：
//
//   - 开关：设置 embedTaskbar=true 且 home 允许时才尝试嵌入；
//   - 回落：嵌入任何一步失败（找不到任务栏、SetParent 失败、重挂超时）→ 回独立窗口，
//     不白屏、不丢交互；
//   - 重挂：Explorer 重启后重挂有次数上限与超时，超限即回落独立窗口（不再无限重试）；
//   - 自动隐藏/多任务栏：嵌入在任务栏自动隐藏时不强求常显（让位），多任务栏时只挂主任务栏。
// 这些用纯状态机表达，平台层照做。

/// 嵌入的重挂策略与上限。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EmbedPolicy {
    /// 是否允许嵌入（设置 + 家允许）。
    pub enabled: bool,
    /// 单次重挂等待上限（毫秒），超时即算失败。
    pub reattach_timeout_ms: u64,
    /// 重挂尝试次数上限，达到即回落独立窗口（不再重试）。
    pub max_reattach: u32,
}

impl EmbedPolicy {
    pub fn from_settings(embed: bool, home_takes_taskbar: bool) -> EmbedPolicy {
        EmbedPolicy { enabled: embed && home_takes_taskbar, reattach_timeout_ms: 3_000, max_reattach: 5 }
    }
}

/// 嵌入运行态。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EmbedState {
    /// 未尝试（开关关，或还没到嵌入时机）。
    Off,
    /// 已嵌入任务栏（子窗口在产品任务栏里）。
    Embedded,
    /// 嵌入失败并回落独立窗口（不白屏、不丢交互）。
    Fallback,
}

/// 一次嵌入/重挂尝试的结果（平台层填）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Attempt {
    /// 成功嵌入。
    Ok,
    /// 找不到/无任务栏（Explorer 不在、自动隐藏、多任务栏只有副屏等）。
    NoTaskbar,
    /// SetParent / UIA 失败。
    Failed,
    /// 等待超时。
    Timeout,
}

/// 追踪重挂次数的状态机。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Embedder {
    pub policy: EmbedPolicy,
    pub state: EmbedState,
    /// 已重挂失败次数（Explorer 重启后）。
    pub attempts: u32,
}

impl Embedder {
    pub fn new(policy: EmbedPolicy) -> Embedder {
        Embedder { policy, state: EmbedState::Off, attempts: 0 }
    }

    /// 一次尝试后的新状态。关着 → Off；成功 → Embedded 且重置计数；失败在次数上限内 →
    /// 保持 Off 等下次重挂；到上限 → Fallback（回落独立窗口，不再重试）。
    pub fn on_attempt(&mut self, attempt: Attempt) -> EmbedState {
        if !self.policy.enabled {
            self.state = EmbedState::Off;
            return self.state.clone();
        }
        match attempt {
            Attempt::Ok => {
                self.attempts = 0;
                self.state = EmbedState::Embedded;
            }
            _ => {
                self.attempts += 1;
                self.state = if self.attempts >= self.policy.max_reattach { EmbedState::Fallback } else { EmbedState::Off };
            }
        }
        self.state.clone()
    }

    /// Explorer 重启后要重挂：只有开着、且还没回落时才重挂。
    pub fn should_reattach(&self) -> bool {
        self.policy.enabled && self.state != EmbedState::Fallback
    }

    /// 该不该现在就把独立窗口（岛）藏起来（嵌入成功才藏，否则它自己显示，不白屏）。
    pub fn hide_standalone(&self) -> bool {
        self.state == EmbedState::Embedded
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disabled_never_embeds() {
        let mut e = Embedder::new(EmbedPolicy::from_settings(false, true));
        assert_eq!(e.on_attempt(Attempt::Ok), EmbedState::Off, "关着不嵌入");
        assert!(!e.should_reattach());
        assert!(!e.hide_standalone(), "关着独立窗口一直显示");
    }

    #[test]
    fn needs_home_that_takes_taskbar() {
        // home 不接管任务栏（island/topbar）：不嵌入。
        let p = EmbedPolicy::from_settings(true, false);
        assert!(!p.enabled);
    }

    #[test]
    fn success_embeds_and_hides_standalone() {
        let mut e = Embedder::new(EmbedPolicy::from_settings(true, true));
        assert!(!e.hide_standalone(), "嵌入前独立窗口显示（不白屏）");
        assert_eq!(e.on_attempt(Attempt::Ok), EmbedState::Embedded);
        assert!(e.hide_standalone(), "嵌入成功才藏独立窗口");
        assert!(e.should_reattach());
    }

    #[test]
    fn failures_fallback_after_limit_and_keep_standalone() {
        let p = EmbedPolicy::from_settings(true, true);
        let mut e = Embedder::new(p);
        // 上限 5：前 4 次失败仍 Off（可重试），第 5 次回落。
        for i in 1..p.max_reattach {
            assert_eq!(e.on_attempt(Attempt::Failed), EmbedState::Off, "第 {i} 次失败仍可重试");
            assert!(!e.hide_standalone(), "失败期间独立窗口显示，不白屏");
        }
        assert_eq!(e.on_attempt(Attempt::Timeout), EmbedState::Fallback, "到上限回落");
        assert!(!e.should_reattach(), "回落之后不再重挂");
        assert!(!e.hide_standalone(), "回落是独立窗口，不藏");
    }

    #[test]
    fn no_taskbar_counts_as_failure() {
        let mut e = Embedder::new(EmbedPolicy::from_settings(true, true));
        assert_eq!(e.on_attempt(Attempt::NoTaskbar), EmbedState::Off);
        assert!(!e.hide_standalone());
    }
}
