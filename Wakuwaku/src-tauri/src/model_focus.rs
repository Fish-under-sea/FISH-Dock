// 设置窗口只遵循手动置顶偏好；关闭后桌面岛恢复自身层级。
#[derive(Default)]
pub struct AuthFocus { pub until: u64, released: bool, original_side: Option<String> }
impl AuthFocus {
    pub fn active(&self, now:u64)->bool { !self.released && self.until>now }
    pub fn begin(&mut self, until:u64, home:&str, side:&str) {
        if until==0 { self.release(); return; }
        if self.until==until { return; }
        self.until=until; self.released=false;
        self.original_side=if home=="island" && matches!(side,"left"|"center") {Some(side.into())}else{None};
    }
    pub fn release(&mut self){self.released=true;}
    pub fn expire(&mut self, now:u64)->bool{if self.until>0 && now>=self.until && !self.released{self.release();true}else{false}}
    pub fn side<'a>(&self,now:u64,home:&str,saved:&'a str)->&'a str{if self.active(now)&&home=="island"&&self.original_side.is_some(){"right"}else{saved}}
}
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
    fn manual_release_restores_side_without_cancelling_or_rearming_session() {
        for side in ["left", "center", "right"] {
            let mut lease=AuthFocus::default();
            lease.begin(100, "island", side);
            assert_eq!(lease.side(10, "island", side), "right");
            lease.release();
            assert!(!lease.active(10));
            assert_eq!(lease.side(10, "island", side), side);
            lease.begin(100, "island", side);
            assert!(!lease.active(10));
            lease.begin(200, "island", side);
            assert!(lease.active(10));
            assert!(lease.expire(200));
            assert_eq!(lease.side(200, "island", side), side);
        }
        let mut lease=AuthFocus::default();lease.begin(100, "taskbar", "left");
        assert_eq!(lease.side(10, "taskbar", "left"), "left");
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
