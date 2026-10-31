// 全局快捷键：Alt+Y 允许 / Alt+N 拒绝 / Alt+W 展开收起岛（上游 v0.3.7
// zhoupengjie/Wakuwaku#86，本仓按语义自研——本仓没有上游的 shortcuts.rs，
// 结构对齐 winkeys.rs：注册在线程自己的消息循环上，热键来了发岛）。上游
// a9f19173 的三件事一并做进来：设置页录制快捷键期间让她自己的组合键让路
// （只松开到设置关上为止，任何退出录制的方式都恢复）；换代时旧副本最长 8 秒
// 才退（任务栏交接更久），注册失败的每秒重试 30 秒；Msg 只在 winkeys.rs
// 声明（GetMessageW 一处签名，clashing_extern_declarations）。
use std::sync::Arc;

#[cfg(windows)]
use crate::Shared;

// 上游的修饰键组合：必须带 Ctrl 或 Alt（只 Shift 的不要），带 Win 的不要
// （Windows 的 Win+W、Win+N 是它自己的），一个主键。
#[cfg(windows)]
const MOD_ALT: u32 = 0x0001;
#[cfg(windows)]
const MOD_CONTROL: u32 = 0x0002;
#[cfg(windows)]
const MOD_SHIFT: u32 = 0x0004;
#[cfg(windows)]
const MOD_NOREPEAT: u32 = 0x4000;

#[cfg(windows)]
static FAILED: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());

#[cfg(windows)]
pub fn failed() -> Vec<String> {
    FAILED.lock().unwrap().clone()
}

#[cfg(not(windows))]
pub fn failed() -> Vec<String> {
    Vec::new()
}

// "Alt+Y"、"Ctrl+Shift+F5" 这样的写法，成（修饰键，虚拟键）；None：不合法。
pub fn parse_shortcut(s: &str) -> Option<(u32, u32)> {
    let trimmed = s.trim();
    if trimmed.is_empty() {
        return None;
    }
    let parts: Vec<&str> = trimmed.split('+').map(str::trim).filter(|p| !p.is_empty()).collect();
    if parts.is_empty() {
        return None;
    }
    let mut mods = MOD_NOREPEAT;
    let mut primary = false;
    let mut key = None;
    for part in parts {
        if part.eq_ignore_ascii_case("alt") {
            mods |= MOD_ALT;
            primary = true;
        } else if part.eq_ignore_ascii_case("ctrl") || part.eq_ignore_ascii_case("control") {
            mods |= MOD_CONTROL;
            primary = true;
        } else if part.eq_ignore_ascii_case("shift") {
            mods |= MOD_SHIFT;
        } else if part.eq_ignore_ascii_case("win") || part.eq_ignore_ascii_case("meta") || part.eq_ignore_ascii_case("super") {
            // Win 组合键归 Windows 自己的（Win+W 通知中心、Win+N），不注册。
            return None;
        } else if key.is_none() {
            key = Some(part);
        } else {
            // 两个主键：不认。
            return None;
        }
    }
    if !primary {
        return None;
    }
    let key = key?;
    let vk = match key {
        // 一个字符：字母数字，大写取值。
        k if k.len() == 1 => {
            let c = k.chars().next()?.to_ascii_uppercase();
            if c.is_ascii_alphanumeric() {
                c as u32
            } else {
                return None;
            }
        }
        k if k.eq_ignore_ascii_case("enter") || k.eq_ignore_ascii_case("return") => 0x0D,
        k if k.eq_ignore_ascii_case("space") => 0x20,
        k if k.eq_ignore_ascii_case("tab") => 0x09,
        k if k.eq_ignore_ascii_case("esc") || k.eq_ignore_ascii_case("escape") => 0x1B,
        // F1–F12。
        k if k.len() >= 2 && (k.starts_with('f') || k.starts_with('F')) => {
            let n: u32 = k[1..].parse().ok()?;
            if (1..=12).contains(&n) {
                0x70 + n - 1
            } else {
                return None;
            }
        }
        _ => return None,
    };
    Some((mods, vk))
}

// 几个快捷键写法里有没有重复（同一个键两种写法也算：Ctrl+Y 与 Control+y）。
pub fn has_duplicates(keys: &[&str]) -> bool {
    let non_empty: Vec<&str> = keys.iter().map(|s| s.trim()).filter(|s| !s.is_empty()).collect();
    for (i, left) in non_empty.iter().enumerate() {
        for right in &non_empty[(i + 1)..] {
            if left.eq_ignore_ascii_case(right) || parse_shortcut(left).is_some_and(|a| parse_shortcut(right) == Some(a)) {
                return true;
            }
        }
    }
    false
}

#[cfg(windows)]
mod imp {
    use std::ptr::null_mut;
    use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
    use std::sync::Arc;
    use std::time::Duration;

    use serde_json::json;
    use tauri::Emitter;

    use super::{parse_shortcut, FAILED};
    use crate::Shared;

    // 换代等待的口径：旧副本退出最长 8 秒（Shared::quit），任务栏交接时更久；
    // 注册失败的每秒重试，30 秒为限（上游原来 5 次不够）。
    const RETRY_SECONDS: usize = 30;

    const WM_HOTKEY: u32 = 0x0312;
    // 线程消息（注册表变了）：WM_USER 起的一段里挑一个不撞车的。
    const WM_RELOAD: u32 = 0x0400 + 101;

    const HOTKEY_APPROVE: i32 = 1;
    const HOTKEY_DENY: i32 = 2;
    const HOTKEY_ISLAND: i32 = 3;

    static THREAD_ID: AtomicU32 = AtomicU32::new(0);
    // 设置页在录制快捷键：组合键松开，让按键到得了页面。只该在设置开着时成立
    // （录制暂停 on；设置关上 register_all 一律恢复）。
    static PAUSED: AtomicBool = AtomicBool::new(false);

    fn unregister_all() {
        // SAFETY: 空窗口的热键，本线程注册的，各一次。
        unsafe {
            UnregisterHotKey(null_mut(), HOTKEY_APPROVE);
            UnregisterHotKey(null_mut(), HOTKEY_DENY);
            UnregisterHotKey(null_mut(), HOTKEY_ISLAND);
        }
    }

    // 注册三个快捷键，返回注册失败的那些。录制暂停且设置还开着：None，热键
    // 全部让路（页面还在录制，最后一个状态先留着）。设置关上了：照常恢复，
    // 页面最后说了什么都不算。
    fn register_all(sh: &Shared) -> Option<Vec<String>> {
        unregister_all();
        if PAUSED.load(Ordering::SeqCst) && sh.island.lock().unwrap().settings_open {
            return None;
        }
        // 勿扰时提示交给终端（island.js 不弹），组合键不必占着；全屏时岛藏了。
        if !sh.flag("shortcutsEnabled") || sh.flag("dnd") || sh.is_fullscreen() {
            return Some(Vec::new());
        }
        let specs = [
            (HOTKEY_APPROVE, "shortcutApprove", "Alt+Y"),
            (HOTKEY_DENY, "shortcutDeny", "Alt+N"),
            (HOTKEY_ISLAND, "shortcutIsland", "Alt+W"),
        ];
        let mut failed = Vec::new();
        for (id, name, default) in specs {
            let spec = sh.setting(name).as_str().unwrap_or(default).to_string();
            if spec.trim().is_empty() {
                continue;
            }
            match parse_shortcut(&spec) {
                Some((mods, vk)) => {
                    // SAFETY: 热键注册到本线程的消息循环（hwnd 为空），id 自己的。
                    if unsafe { RegisterHotKey(null_mut(), id, mods, vk) } == 0 {
                        let err = unsafe { GetLastError() };
                        sh.log(&format!("shortcuts: {name} ({spec}) 没注册上：{err}"));
                        failed.push(name.to_string());
                    }
                }
                None => {
                    sh.log(&format!("shortcuts: {name} ({spec}) 不是合法的组合键"));
                    failed.push(name.to_string());
                }
            }
        }
        Some(failed)
    }

    fn say_failed(sh: &Shared, failed: Vec<String>) {
        *FAILED.lock().unwrap() = failed.clone();
        let _ = sh.app.emit_to("island", "shortcuts:status", json!({ "failed": failed }));
    }

    pub fn start_worker(sh: Arc<Shared>) {
        if THREAD_ID.load(Ordering::SeqCst) != 0 {
            return;
        }
        let worker = sh.clone();
        crate::spawn_named("shortcuts", move || {
            // SAFETY: 本线程自己的 id 与消息队列（GetMessageW 建它）。
            let tid = unsafe { GetCurrentThreadId() };
            THREAD_ID.store(tid, Ordering::SeqCst);
            let mut last_failed = register_all(&worker).unwrap_or_default();
            say_failed(&worker, last_failed.clone());
            // 一开始就没占上（换代时旧副本还在）：每秒重试，30 秒为限。
            if !last_failed.is_empty() {
                retry_registration();
            }
            // 消息循环。Msg 与 GetMessageW 借 winkeys.rs 的：全程序一个签名。
            let mut msg: crate::winkeys::Msg = unsafe { std::mem::zeroed() };
            while crate::winkeys::get_message(&mut msg) > 0 {
                match msg.message {
                    WM_HOTKEY => {
                        let which = match msg.wparam as i32 {
                            HOTKEY_APPROVE => "allow",
                            HOTKEY_DENY => "deny",
                            HOTKEY_ISLAND => "island",
                            _ => continue,
                        };
                        let _ = worker.app.emit_to("island", "island:hotkey", json!({ "which": which }));
                    }
                    WM_RELOAD => {
                        let Some(failed) = register_all(&worker) else { continue };
                        if failed != last_failed {
                            last_failed = failed.clone();
                            say_failed(&worker, failed.clone());
                            worker.push_settings();
                        }
                    }
                    _ => {}
                }
            }
            THREAD_ID.store(0, Ordering::SeqCst);
        });
    }

    pub fn notify_reload() {
        let tid = THREAD_ID.load(Ordering::SeqCst);
        if tid != 0 {
            // SAFETY: 自己线程的消息队列，没有指针。
            unsafe { PostThreadMessageW(tid, WM_RELOAD, 0, 0) };
        }
    }

    // 换代：新副本起来时旧的还在退（最长 8 秒，任务栏交接更久），注册失败的
    // 每秒重试一次，30 秒为限；哪一次空了（都占上了）就停。
    pub fn retry_registration() {
        crate::spawn_named("shortcuts retry", move || {
            for _ in 0..RETRY_SECONDS {
                std::thread::sleep(Duration::from_secs(1));
                if super::failed().is_empty() {
                    break;
                }
                notify_reload();
            }
        });
    }

    // 设置页在录制（on），录完了（off）。状态变了才叫重排；没变的白叫一次也无害
    // （register_all 幂等），省得页面连按两次同一个按钮。
    pub fn pause(on: bool) {
        if PAUSED.swap(on, Ordering::SeqCst) != on {
            notify_reload();
        }
    }

    #[link(name = "user32")]
    extern "system" {
        fn RegisterHotKey(hwnd: *mut std::ffi::c_void, id: i32, mods: u32, vk: u32) -> i32;
        fn UnregisterHotKey(hwnd: *mut std::ffi::c_void, id: i32) -> i32;
        fn PostThreadMessageW(thread: u32, msg: u32, wparam: usize, lparam: isize) -> i32;
        fn GetCurrentThreadId() -> u32;
        fn GetLastError() -> u32;
    }
}

#[cfg(not(windows))]
mod imp {
    use std::sync::Arc;

    use crate::Shared;

    #[allow(unused_variables)]
    pub fn start_worker(_sh: Arc<Shared>) {}
    pub fn notify_reload() {}
    pub fn retry_registration() {}
    pub fn pause(_on: bool) {}
}

// re-export 只给本文件用（start/update 在 Windows 下转手 imp 的），非 Windows
// 下另有同名空函数；unused 的那几个在非 Windows 编译时是活的。
#[allow(unused_imports)]
pub use imp::{notify_reload, pause, retry_registration, start_worker};

pub fn start(sh: &Arc<Shared>) {
    #[cfg(windows)]
    imp::start_worker(sh.clone());
    #[cfg(not(windows))]
    let _ = sh;
}

pub fn update(_sh: &Arc<Shared>) {
    #[cfg(windows)]
    imp::notify_reload();
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOREPEAT_ALT: u32 = MOD_NOREPEAT | MOD_ALT;

    #[test]
    fn shortcut_parser_takes_ctrl_or_alt_only() {
        // 空、纯空格。
        assert_eq!(parse_shortcut(""), None);
        assert_eq!(parse_shortcut("   "), None);
        // 没有修饰键的单键不要。
        assert_eq!(parse_shortcut("Y"), None);
        assert_eq!(parse_shortcut("Space"), None);
        assert_eq!(parse_shortcut("Enter"), None);
        // 只 Shift 的不要。
        assert_eq!(parse_shortcut("Shift+Y"), None);
        assert_eq!(parse_shortcut("Shift+F5"), None);
        // Win 组合键不要（Windows 自己的）。
        assert_eq!(parse_shortcut("Win+W"), None);
        assert_eq!(parse_shortcut("Meta+Y"), None);
        // 合法的 Alt / Ctrl 组合。
        assert_eq!(parse_shortcut("Alt+Y"), Some((NOREPEAT_ALT, b'Y' as u32)));
        assert_eq!(parse_shortcut("alt+n"), Some((NOREPEAT_ALT, b'N' as u32)));
        assert_eq!(parse_shortcut("Ctrl+Shift+F5"), Some((MOD_NOREPEAT | MOD_CONTROL | MOD_SHIFT, 0x74)));
        assert_eq!(parse_shortcut("Alt+Space"), Some((NOREPEAT_ALT, 0x20)));
        assert_eq!(parse_shortcut("Ctrl+Enter"), Some((MOD_NOREPEAT | MOD_CONTROL, 0x0D)));
        // F13、两个主键、三个修饰键不认。
        assert_eq!(parse_shortcut("F13"), None);
        assert_eq!(parse_shortcut("Alt+Y+N"), None);
    }

    #[test]
    fn shortcut_duplicates_by_writing_or_by_key() {
        assert!(has_duplicates(&["Alt+Y", "Alt+y"]));
        assert!(has_duplicates(&["Alt+Y", "Alt+N", "Alt+Y"]));
        assert!(has_duplicates(&["Ctrl+Y", "Control+y"]));
        assert!(!has_duplicates(&["Alt+Y", "Alt+N", "Alt+W"]));
        assert!(!has_duplicates(&["Alt+Y", "", ""]));
        // 写法不同、键相同也算。
        assert!(has_duplicates(&["Ctrl+Y", "Ctrl+Shift+Y"]) == false);
    }
}
