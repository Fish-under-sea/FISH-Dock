//! 邮件重连的指数退避与熔断（t12）。
//!
//! 为什么单独一个模块：退避曲线、抖动、熔断阈值、跨重启的持久，都是纯算术与
//! 纯状态的判断，不该和 IMAP 会话、线程搅在一起；单独出来才能不连网就断言
//! 「时间序列」。
//!
//! 根因记在这里，免得以后又退回固定间隔：旧代码在 `session()`（TCP+TLS+登录）
//! 一成功就把失败计数清零（sync.rs 原来的 `failures = 0`）。服务器「接受连接、
//! 过一会儿在 IDLE 里把连接掐掉」这种最常见的情形，于是每一轮都算「第 1 次失败」
//! → 等待永远是表里的第一档 → 一场几分钟一次的无限重连风暴。现在只有「连接活过
//! STABLE_MS」才清零，短命连接会累计，退避才真的往上爬。
use std::path::Path;
use std::time::Duration;

/// 第一次失败的等待：1 秒。
pub const BASE_MS: u64 = 1_000;
/// 封顶：5 分钟。
pub const CAP_MS: u64 = 300_000;
/// 抖动幅度（百分比）：实际等待 = base × [80%, 120%]。
pub const JITTER_PCT: u64 = 20;
/// 连续失败到这个次数就熔断（暂停自动重连，等手动重试）。
pub const PAUSE_AFTER: u32 = 6;
/// 一条连接活过这么久，就算「健康」：它之后断开不累计失败、立刻重连。
pub const STABLE_MS: u64 = 300_000;

/// 第 `failures` 次连续失败（从 1 起）的退避，不含抖动：
/// 1s, 2s, 4s, 8s, …，封顶 `CAP_MS`。
pub fn base_delay_ms(failures: u32) -> u64 {
    let steps = failures.saturating_sub(1).min(20);
    BASE_MS.saturating_mul(1u64 << steps).min(CAP_MS)
}

/// 带抖动的退避：在 base 的 ±`JITTER_PCT`% 内，由 `seed` 决定落在哪一点
/// （同一 seed 结果固定，测试才能断言）；到顶后不再抖动，免得超过 5 分钟。
pub fn delay_ms(failures: u32, seed: u64) -> u64 {
    let base = base_delay_ms(failures);
    if base >= CAP_MS {
        return CAP_MS;
    }
    let span = base * JITTER_PCT / 100;
    if span == 0 {
        return base;
    }
    let low = base - span;
    let high = base + span;
    (low + seed % (high - low + 1)).min(CAP_MS).max(1)
}

/// 一次失败之后要做什么。
#[derive(Debug, PartialEq, Eq)]
pub enum Step {
    /// 等这么久再试。
    Wait(Duration),
    /// 连续失败够多：暂停自动重连，等手动重试。
    Paused,
}

/// 一个账户的重连状态（可序列化，落盘后重启能接着用）。
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct State {
    #[serde(default)]
    pub failures: u32,
    #[serde(default)]
    pub paused: bool,
    /// 下一次允许尝试的时刻（ms，Unix 纪元）；0 表示「立刻」。
    #[serde(default)]
    pub next_at_ms: i64,
    /// 上一次失败的原因（`Fail::json()` 那一份），重启后 UI 还看得到。
    #[serde(default)]
    pub last_error: Option<serde_json::Value>,
}

impl State {
    /// 连接活够了、或用户手动重试：回到干净状态。
    pub fn healthy(&mut self) {
        *self = State::default();
    }

    /// 记一次失败，返回接下来做什么。
    /// `now_ms` 由调用方给（便于测试），`seed` 决定抖动。
    pub fn fail(&mut self, now_ms: i64, seed: u64) -> Step {
        self.failures = self.failures.saturating_add(1);
        if self.failures >= PAUSE_AFTER {
            self.paused = true;
            self.next_at_ms = 0;
            return Step::Paused;
        }
        let wait = delay_ms(self.failures, seed);
        self.next_at_ms = now_ms.saturating_add(wait as i64);
        Step::Wait(Duration::from_millis(wait))
    }

    /// 距下一次尝试还有多久（暂停时为 0；到点或已过也为 0）。
    pub fn remaining(&self, now_ms: i64) -> Duration {
        if self.paused {
            return Duration::ZERO;
        }
        Duration::from_millis((self.next_at_ms - now_ms).max(0) as u64)
    }

    /// 手动重试：清空退避与熔断，马上可以再试。
    pub fn retry_now(&mut self) {
        *self = State::default();
    }
}

/// 读一个账户的状态；没有或读坏了都当干净状态（不因为读不到就惩罚用户）。
pub fn load(dir: &Path, id: &str) -> State {
    let Ok(bytes) = std::fs::read(path(dir)) else { return State::default() };
    let Ok(map) = serde_json::from_slice::<serde_json::Map<String, serde_json::Value>>(&bytes) else { return State::default() };
    map.get(id).cloned().and_then(|v| serde_json::from_value(v).ok()).unwrap_or_default()
}

/// 存一个账户的状态（原子的：先写临时文件再改名，别把半个 JSON 留盘上）。
pub fn save(dir: &Path, id: &str, state: &State) {
    let path = path(dir);
    let mut map = std::fs::read(&path)
        .ok()
        .and_then(|b| serde_json::from_slice::<serde_json::Map<String, serde_json::Value>>(&b).ok())
        .unwrap_or_default();
    match serde_json::to_value(state) {
        Ok(v) => {
            map.insert(id.to_string(), v);
        }
        Err(_) => return,
    }
    let Ok(bytes) = serde_json::to_vec_pretty(&map) else { return };
    let tmp = path.with_extension("json.tmp");
    if std::fs::write(&tmp, bytes).is_ok() {
        let _ = std::fs::rename(&tmp, &path);
    }
}

fn path(dir: &Path) -> std::path::PathBuf {
    dir.join("mail-backoff.json")
}

#[cfg(test)]
mod tests {
    use super::*;

    // 退避曲线：时间序列断言（不含抖动）。
    #[test]
    fn the_backoff_doubles_from_one_second_and_caps_at_five_minutes() {
        let curve: Vec<u64> = (1..=12).map(base_delay_ms).collect();
        assert_eq!(
            curve,
            vec![1_000, 2_000, 4_000, 8_000, 16_000, 32_000, 64_000, 128_000, 256_000, 300_000, 300_000, 300_000]
        );
        // 单调不减，且永不越顶。
        for n in 2..=40u32 {
            assert!(base_delay_ms(n) >= base_delay_ms(n - 1), "第 {n} 档不该比前一档短");
            assert!(base_delay_ms(n) <= CAP_MS);
        }
    }

    // 抖动：同一档在 ±20% 内，seed 决定落点，且到顶不抖。
    #[test]
    fn jitter_stays_within_twenty_percent_and_is_deterministic() {
        for failures in 1..=9u32 {
            let base = base_delay_ms(failures);
            let low = base - base * JITTER_PCT / 100;
            let high = base + base * JITTER_PCT / 100;
            for seed in 0..50u64 {
                let d = delay_ms(failures, seed);
                assert!(d >= low && d <= high, "第 {failures} 档 seed {seed}: {d} 不在 [{low},{high}]");
                assert_eq!(d, delay_ms(failures, seed), "同一 seed 必须同一结果");
            }
        }
        // 到顶（第 10 档起 base=CAP）不再抖动。
        assert_eq!(delay_ms(10, 7), CAP_MS);
    }

    // 连续失败到 PAUSE_AFTER 就熔断。
    #[test]
    fn consecutive_failures_pause_at_the_limit() {
        let mut s = State::default();
        let mut now = 1_000_000i64;
        for n in 1..PAUSE_AFTER {
            match s.fail(now, 0) {
                Step::Wait(d) => {
                    assert_eq!(s.failures, n);
                    assert!(!s.paused);
                    // 第 n 档的等待落在 base(n) 的 ±20% 内（seed 0 → 下界）。
                    let base = base_delay_ms(n);
                    assert!(d.as_millis() as u64 >= base - base * JITTER_PCT / 100, "第 {n} 档 {d:?} 低于下界");
                    assert!(d.as_millis() as u64 <= base + base * JITTER_PCT / 100, "第 {n} 档 {d:?} 高于上界");
                }
                Step::Paused => panic!("第 {n} 次不该熔断"),
            }
            now = s.next_at_ms;
        }
        assert_eq!(s.fail(now, 0), Step::Paused);
        assert!(s.paused);
        assert_eq!(s.failures, PAUSE_AFTER);
    }

    // 手动重试：清空熔断与计数，下一次又是第 1 档。
    #[test]
    fn a_manual_retry_clears_the_backoff_and_the_pause() {
        let mut s = State::default();
        let mut now = 0i64;
        for _ in 0..PAUSE_AFTER {
            if let Step::Wait(_) = s.fail(now, 0) {
                now = s.next_at_ms;
            }
        }
        assert!(s.paused);
        s.retry_now();
        assert_eq!(s, State::default());
        assert!(!s.paused);
        assert_eq!(s.remaining(now), Duration::ZERO);
        // 重试后第一次失败：第 1 档（1s ±20%）。
        let Step::Wait(d) = s.fail(now, 0) else { panic!("不该熔断") };
        assert!(d.as_millis() as u64 >= 800 && d.as_millis() as u64 <= 1_200, "{d:?} 不在第 1 档内");
        assert_eq!(s.failures, 1);
    }

    // 重启后不立刻风暴：落盘的「还没到点」会被接着等；「已熔断」重启后仍是暂停。
    #[test]
    fn a_restart_carries_the_backoff_and_the_pause_over() {
        let dir = std::env::temp_dir().join(format!("fish-backoff-test-{}-{}", std::process::id(), crate::now_ms()));
        std::fs::create_dir_all(&dir).unwrap();

        let mut s = State::default();
        let now = 5_000_000i64;
        let Step::Wait(_) = s.fail(now, 0) else { panic!() };
        s.last_error = Some(serde_json::json!({ "kind": "connect", "text": "x" }));
        save(&dir, "acct-a", &s);

        let back = load(&dir, "acct-a");
        assert_eq!(back.failures, 1);
        assert!(!back.paused);
        // 重启那一刻还没到点：必须还能算出正的剩余时间（不立刻重连）。
        assert!(back.remaining(now) > Duration::ZERO);

        // 熔断后落盘，重启还是暂停。
        let mut p = State::default();
        let mut t = 0i64;
        for _ in 0..PAUSE_AFTER {
            if let Step::Wait(_) = p.fail(t, 0) {
                t = p.next_at_ms;
            }
        }
        assert!(p.paused);
        save(&dir, "acct-b", &p);
        let pb = load(&dir, "acct-b");
        assert!(pb.paused);
        assert_eq!(pb.remaining(999_999_999), Duration::ZERO);

        // 读不到也是干净状态（不惩罚）。
        assert_eq!(load(&dir, "nobody"), State::default());
        let _ = std::fs::remove_dir_all(&dir);
    }

    // 长连接断开算健康：清零后这次失败又从第 1 档起（而不是越背越久）。
    #[test]
    fn a_long_lived_connection_resets_before_counting_the_drop() {
        let mut s = State::default();
        s.fail(0, 0);
        s.fail(0, 0);
        s.fail(0, 0);
        assert_eq!(s.failures, 3);
        s.healthy();
        assert_eq!(s.failures, 0);
        let Step::Wait(d) = s.fail(0, 0) else { panic!("不该熔断") };
        assert!(d.as_millis() as u64 >= 800 && d.as_millis() as u64 <= 1_200, "{d:?} 不在第 1 档内");
    }
}
