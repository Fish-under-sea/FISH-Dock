// 设置窗口只遵循手动置顶偏好；关闭后桌面岛恢复自身层级。
#[derive(Default)]
pub struct AuthFocus { pub until: u64 }
impl AuthFocus { pub fn active(&self, now: u64) -> bool { self.until > now } }
pub fn topmost(open: bool, top: bool) -> bool { !open || top }

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn auth_focus_is_temporary_and_never_extends_after_completion() {
        let mut lease = AuthFocus::default();
        assert!(!lease.active(10));
        lease.until = 100;
        assert!(lease.active(99));
        assert!(!lease.active(100));
        lease.until = 0;
        assert!(!lease.active(20));
    }
    #[test]
    fn open_window_uses_saved_topmost_choice() {
        assert!(!topmost(true, false));
        assert!(topmost(true, true));
    }
    #[test]
    fn closing_does_not_change_saved_choice() {
        for saved in [false, true] {
            assert!(topmost(false, saved));
            assert_eq!(topmost(true, saved), saved);
        }
    }
}
