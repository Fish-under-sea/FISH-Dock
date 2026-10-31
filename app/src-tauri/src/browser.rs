//! 网页交给 Windows URL 启动服务，不让浏览器继承宿主的环境与 Job。
fn web_url(url: &str) -> bool {
    tauri::Url::parse(url).is_ok_and(|u| matches!(u.scheme(), "https" | "http") && u.host_str().is_some() && u.username().is_empty() && u.password().is_none() && !url.contains('\0'))
}

pub fn open(url: &str) -> Result<(), String> {
    if !web_url(url) { return Err("浏览器链接无效".into()); }
    #[cfg(windows)] {
        let url = url.to_owned();
        return std::thread::Builder::new().name("fish-browser".into()).spawn(move || desktop_open(&url))
            .map_err(|_| "浏览器启动任务创建失败".to_string())?.join()
            .map_err(|_| "浏览器启动任务意外结束".to_string())?
            .map_err(|e| format!("默认浏览器打开失败（{}），请重试或检查默认浏览器设置。", e.code()));
    }
    #[cfg(not(windows))] {
        if crate::jump::open_url(url) { Ok(()) } else { Err("默认浏览器打开失败，请检查默认浏览器设置。".into()) }
    }
}

#[cfg(windows)]
fn desktop_open(url: &str) -> windows_core::Result<()> {
    use windows::{Foundation::Uri, System::Launcher, Win32::System::WinRT::{RoInitialize, RoUninitialize, RO_INIT_MULTITHREADED}};
    struct Apartment;
    impl Drop for Apartment { fn drop(&mut self) { unsafe { RoUninitialize() } } }
    // 独立 MTA 线程等待系统启动结果，设置窗口的消息循环保持运行。
    unsafe { RoInitialize(RO_INIT_MULTITHREADED)?; }
    let _apartment = Apartment;
    let uri = Uri::CreateUri(&windows_core::HSTRING::from(url))?;
    if Launcher::LaunchUriAsync(&uri)?.join()? { Ok(()) }
    else { Err(windows_core::Error::from_hresult(windows_core::HRESULT(0x80004005u32 as i32))) }
}

pub fn copy_device_code(owner: isize, code: &str) -> bool {
    if !valid_device_code(code) { return false; }
    #[cfg(windows)] {
        use std::ffi::c_void;
        #[link(name="user32")] unsafe extern "system" {
            fn OpenClipboard(owner: isize) -> i32;
            fn CloseClipboard() -> i32;
            fn EmptyClipboard() -> i32;
            fn SetClipboardData(format: u32, data: isize) -> isize;
        }
        #[link(name="kernel32")] unsafe extern "system" {
            fn GlobalAlloc(flags: u32, bytes: usize) -> isize;
            fn GlobalLock(memory: isize) -> *mut c_void;
            fn GlobalUnlock(memory: isize) -> i32;
            fn GlobalFree(memory: isize) -> isize;
        }
        if owner == 0 { return false; }
        let value: Vec<u16> = code.encode_utf16().chain(Some(0)).collect();
        unsafe {
            let memory = GlobalAlloc(0x0002, value.len() * 2);
            if memory == 0 { return false; }
            let data = GlobalLock(memory);
            if data.is_null() { GlobalFree(memory); return false; }
            std::ptr::copy_nonoverlapping(value.as_ptr(), data.cast::<u16>(), value.len());
            GlobalUnlock(memory);
            if OpenClipboard(owner) == 0 { GlobalFree(memory); return false; }
            let copied = EmptyClipboard() != 0 && SetClipboardData(13, memory) != 0;
            CloseClipboard();
            // 成功后内存由系统持有；失败时由宿主释放。
            if !copied { GlobalFree(memory); }
            copied
        }
    }
    #[cfg(not(windows))] { let _ = owner; false }
}
fn valid_device_code(code: &str) -> bool {
    code.len() == 9 && code.as_bytes()[4] == b'-' && code.bytes().enumerate().all(|(i,b)| i == 4 || b.is_ascii_alphanumeric())
}

#[cfg(test)]
mod tests {
    #[test] fn clipboard_accepts_only_a_device_user_code() {
        assert!(super::valid_device_code("ABCD-EFGH"));
        for code in ["", "ABCD-EFGH\n", "gho_secret", "ABCD&EFGH", "设备验证码", "ABCD-EFGH-IJKL"] { assert!(!super::valid_device_code(code)); }
    }
    #[test] fn browser_accepts_only_web_links() {
        for url in ["https://github.com/Fish-under-sea/FISH-Dock", "https://github.com/login/device"] { assert!(super::web_url(url)); }
        for url in ["file:///C:/Windows/notepad.exe", "javascript:alert(1)", "https://u:p@github.com/", "https://github.com/\0evil", ""] { assert!(!super::web_url(url)); }
    }
    #[test] fn desktop_browser_integration() {
        if std::env::var("FISH_TEST_OPEN_BROWSER").as_deref() != Ok("1") { return; }
        super::open("https://github.com/Fish-under-sea/FISH-Dock").unwrap();
    }
}
