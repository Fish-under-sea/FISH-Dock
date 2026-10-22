// 本地模型面板的临时锁定状态；不写入用户长期偏好。
use std::time::{Duration, Instant};

#[derive(Default)]
pub struct ModelFocus {
    pub flags: Option<(bool, bool)>,
    active: bool,
    full: bool,
    asked: bool,
    serial: u64,
    prompt: Option<u64>,
    since: Option<Instant>,
}

impl ModelFocus {
    pub fn enter(&mut self, active: bool, now: Instant) {
        self.active = active;
        self.flags = Some((active, active));
        self.asked = false;
        self.prompt = None;
        self.since = None;
        self.serial += 1;
        self.observe(None, now);
    }
    pub fn set(&mut self, pin: Option<bool>, top: Option<bool>, now: Instant) {
        if let Some((locked, above)) = self.flags.as_mut() {
            if let Some(value) = pin { *locked = value; }
            if let Some(value) = top { *above = value; }
        }
        self.observe(None, now);
    }
    pub fn observe(&mut self, full: Option<bool>, now: Instant) -> bool {
        let before = (self.flags, self.prompt);
        if let Some(full) = full {
            if !full { self.asked = false; }
            self.full = full;
        }
        let locked = self.flags.is_some_and(|(pin, _)| pin);
        let top = self.flags.is_some_and(|(_, top)| top);
        if !self.active || !self.full || !locked {
            self.prompt = None;
            self.since = None;
        } else if top {
            self.since = None;
            if !self.asked {
                self.serial += 1;
                self.prompt = Some(self.serial);
                self.asked = true;
            }
        } else {
            // 关闭置顶后不挡全屏窗口；原生时钟连续计时，条件中断即作废。
            self.prompt = None;
            let since = *self.since.get_or_insert(now);
            if now.saturating_duration_since(since) >= Duration::from_secs(2) {
                self.flags = Some((false, false));
                self.since = None;
            }
        }
        before != (self.flags, self.prompt)
    }
    pub fn active(&self) -> bool { self.active }
    pub fn prompt(&self) -> Option<u64> { self.prompt }
    pub fn answer(&mut self, id: u64, cancel: bool, now: Instant) -> bool {
        if self.prompt != Some(id) || !self.active || !self.full { return false; }
        self.prompt = None;
        if cancel { self.flags = Some((false, false)); }
        self.observe(None, now);
        true
    }
    pub fn wait(&self, now: Instant) -> Duration {
        self.since.map(|at| Duration::from_secs(2).saturating_sub(now.saturating_duration_since(at))).unwrap_or(Duration::from_millis(1500))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn started() -> (ModelFocus, Instant) {
        let now = Instant::now();
        let mut state = ModelFocus::default();
        state.enter(true, now);
        (state, now)
    }
    #[test]
    fn fullscreen_asks_once_and_keep_preserves_flags() {
        let (mut s, now) = started();
        assert!(s.observe(Some(true), now));
        let id = s.prompt().expect("全屏应询问");
        assert!(s.answer(id, false, now));
        assert_eq!(s.flags, Some((true, true)));
        s.observe(Some(true), now + Duration::from_secs(8));
        assert_eq!(s.prompt(), None, "同一轮全屏不反复询问");
        s.observe(Some(false), now + Duration::from_secs(9));
        s.observe(Some(true), now + Duration::from_secs(10));
        assert_ne!(s.prompt(), Some(id));
        assert!(s.prompt().is_some());
    }
    #[test]
    fn confirm_cancels_both_and_stale_answer_is_ignored() {
        let (mut s, now) = started();
        s.observe(Some(true), now);
        let id = s.prompt().unwrap();
        assert!(s.answer(id, true, now));
        assert_eq!(s.flags, Some((false, false)));
        s.enter(true, now);
        assert!(!s.answer(id, true, now));
        assert_eq!(s.flags, Some((true, true)));
    }
    #[test]
    fn locked_without_top_unlocks_only_after_two_continuous_seconds() {
        let (mut s, now) = started();
        s.set(None, Some(false), now);
        s.observe(Some(true), now);
        s.observe(None, now + Duration::from_millis(1999));
        assert_eq!(s.flags, Some((true, false)));
        assert_eq!(s.wait(now + Duration::from_millis(1999)), Duration::from_millis(1));
        s.observe(None, now + Duration::from_secs(2));
        assert_eq!(s.flags, Some((false, false)));
        assert_eq!(s.prompt(), None);
    }
    #[test]
    fn exit_fullscreen_or_reenable_top_resets_timer() {
        let (mut s, now) = started();
        s.set(None, Some(false), now);
        s.observe(Some(true), now);
        s.observe(Some(false), now + Duration::from_secs(1));
        s.observe(Some(true), now + Duration::from_millis(1900));
        s.observe(None, now + Duration::from_secs(2));
        assert_eq!(s.flags, Some((true, false)));
        s.set(None, Some(true), now + Duration::from_millis(2200));
        s.observe(None, now + Duration::from_secs(10));
        assert_eq!(s.flags, Some((true, true)));
    }
    #[test]
    fn leave_or_reopen_does_not_inherit_old_deadline_or_prompt() {
        let (mut s, now) = started();
        s.set(None, Some(false), now);
        s.observe(Some(true), now);
        s.enter(false, now + Duration::from_secs(1));
        s.observe(None, now + Duration::from_secs(4));
        assert_eq!(s.prompt(), None);
        assert_eq!(s.flags, Some((false, false)));
        s.enter(true, now + Duration::from_secs(5));
        s.observe(None, now + Duration::from_secs(10));
        assert_eq!(s.flags, Some((true, true)));
    }
    #[test]
    fn other_tabs_never_auto_unlock_or_prompt() {
        let mut s = ModelFocus::default();
        let now = Instant::now();
        s.observe(Some(true), now);
        assert_eq!(s.prompt(), None);
        assert_eq!(s.flags, None);
        s.enter(false, now);
        s.set(Some(true), Some(false), now);
        s.observe(None, now + Duration::from_secs(8));
        assert_eq!(s.flags, Some((true, false)));
    }
}
