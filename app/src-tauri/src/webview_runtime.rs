//! 启动前确认系统 WebView2 可用（#85）。
//!
//! Tauri 在构建运行时的那一刻只问一次 `GetAvailableCoreWebView2BrowserVersionString`，
//! 结果缓存进上下文；之后建第一个窗口时如果当时为否，就在 setup 钩子里 panic。
//! 现场（2026-11-09，Win11 24H2）：登录 6 分钟后 EdgeUpdate 正把 Evergreen 从
//! 156 换到 157，注册表还写着 156、旧版本目录已被换走，Dock 恰好此时启动就崩了；
//! 几分钟后再开一切正常。所以这里在交给 Tauri 之前等它稳定下来，真没有才明确报错。
//!
//! 另一个会让同一句「找不到运行时」出现的原因：2.1.8 及更早在自己进程里设置
//! `WEBVIEW2_BROWSER_EXECUTABLE_FOLDER` 指向随包的 `resources/webview2`；升级交接时
//! 子进程继承了它，而新版已不带这个目录。指向的目录里没有 `msedgewebview2.exe`
//! 时这个变量只会让加载器失败，清掉它退回系统 Evergreen。指向有效目录（用户或
//! 企业自己固定的运行时）则原样保留。
use std::{ffi::OsStr, path::Path, time::Duration};

pub const BROWSER_FOLDER: &str = "WEBVIEW2_BROWSER_EXECUTABLE_FOLDER";
/// 运行时换版本通常几秒内完成；最多等约 20 秒，不让真正缺失的机器卡太久。
const ATTEMPTS: u32 = 20;
const PAUSE: Duration = Duration::from_millis(1000);

/// 变量指向的目录不存在可用的 WebView2 执行文件：只会让加载器失败。
pub fn stale_browser_folder(value: Option<&OsStr>) -> bool {
    match value {
        None => false,
        Some(value) if value.is_empty() => true,
        Some(value) => !Path::new(value).join("msedgewebview2.exe").is_file(),
    }
}

/// 反复问运行时版本，直到给出答案或用完次数。
pub fn wait_until(mut probe: impl FnMut() -> Result<String, String>, attempts: u32, pause: Duration, mut sleep: impl FnMut(Duration)) -> Result<(String, u32), String> {
    let mut last = String::new();
    for attempt in 1..=attempts.max(1) {
        match probe() {
            Ok(version) => return Ok((version, attempt)),
            Err(error) => last = error,
        }
        if attempt < attempts { sleep(pause); }
    }
    Err(last)
}

#[cfg(windows)]
pub fn probe() -> Result<String, String> {
    use webview2_com::Microsoft::Web::WebView2::Win32::GetAvailableCoreWebView2BrowserVersionString;
    use windows_core::{PCWSTR, PWSTR};
    let mut info = PWSTR::null();
    // SAFETY: 加载器按文档写出一段 CoTaskMem 字符串，由 take_pwstr 读取并释放。
    unsafe { GetAvailableCoreWebView2BrowserVersionString(PCWSTR::null(), &mut info) }.map_err(|e| format!("{e}"))?;
    let version = webview2_com::take_pwstr(info);
    if version.is_empty() { Err("WebView2 未返回版本".into()) } else { Ok(version) }
}
#[cfg(not(windows))]
pub fn probe() -> Result<String, String> { Ok(String::new()) }

/// 启动前调用：清除失效的固定运行时变量，并等系统 WebView2 就绪。日志行交给调用方写。
pub fn ensure(log: &mut dyn FnMut(String)) -> Result<String, String> {
    if stale_browser_folder(std::env::var_os(BROWSER_FOLDER).as_deref()) {
        log(format!("webview: {BROWSER_FOLDER} points to no WebView2 (left by an older Dock); cleared for the system runtime"));
        std::env::remove_var(BROWSER_FOLDER);
    }
    let (version, attempt) = wait_until(probe, ATTEMPTS, PAUSE, std::thread::sleep)?;
    if attempt > 1 { log(format!("webview: the system runtime answered on try {attempt} ({version}); it was being updated")); }
    Ok(version)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    #[test]
    fn a_folder_without_the_runtime_is_stale() {
        let dir = std::env::temp_dir().join(format!("fish-webview-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        assert!(!stale_browser_folder(None), "未设置时不动");
        assert!(stale_browser_folder(Some(OsStr::new(""))));
        assert!(stale_browser_folder(Some(dir.join("gone").as_os_str())), "2.1.8 留下的 resources/webview2 已不存在");
        assert!(stale_browser_folder(Some(dir.as_os_str())), "目录在但没有执行文件");
        std::fs::write(dir.join("msedgewebview2.exe"), b"synthetic").unwrap();
        assert!(!stale_browser_folder(Some(dir.as_os_str())), "用户固定的有效运行时保留");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_runtime_being_updated_is_waited_for() {
        // 现场：前几次问不到（换版本中），随后正常。旧代码只问一次就崩。
        let calls = Cell::new(0);
        let slept = Cell::new(0);
        let probe = || { calls.set(calls.get() + 1); if calls.get() < 4 { Err("0x80070002".to_string()) } else { Ok("157.0.4341.42".to_string()) } };
        assert_eq!(wait_until(probe, 20, PAUSE, |_| slept.set(slept.get() + 1)), Ok(("157.0.4341.42".into(), 4)));
        assert_eq!(slept.get(), 3);
    }

    #[test]
    fn a_missing_runtime_still_fails_without_waiting_forever() {
        let calls = Cell::new(0);
        let result = wait_until(|| { calls.set(calls.get() + 1); Err::<String, _>("not installed".to_string()) }, 3, PAUSE, |_| {});
        assert_eq!(result, Err("not installed".into()));
        assert_eq!(calls.get(), 3);
    }

    // 真实加载器：失效的固定目录让它找不到运行时（与 #85 同一句报错的前提），清掉后
    // 能问到系统 Evergreen。只读注册表，不建窗口；本机没装 WebView2 时只验前半。
    #[cfg(windows)]
    #[test]
    fn the_loader_fails_on_a_stale_folder_and_recovers_without_it() {
        let saved = std::env::var_os(BROWSER_FOLDER);
        let gone = std::env::temp_dir().join(format!("fish-webview-gone-{}", std::process::id()));
        std::env::set_var(BROWSER_FOLDER, &gone);
        assert!(probe().is_err(), "失效目录下加载器应报找不到");
        let mut lines = Vec::new();
        let answer = ensure(&mut |line| lines.push(line));
        assert!(std::env::var_os(BROWSER_FOLDER).is_none());
        assert!(lines.iter().any(|l| l.contains("cleared")), "{lines:?}");
        let installed = std::env::var_os("ProgramFiles(x86)").map(|p| Path::new(&p).join(r"Microsoft\EdgeWebView\Application").is_dir()).unwrap_or(false);
        if installed { assert!(answer.is_ok(), "{answer:?}"); }
        match saved { Some(v) => std::env::set_var(BROWSER_FOLDER, v), None => std::env::remove_var(BROWSER_FOLDER) }
    }
}
