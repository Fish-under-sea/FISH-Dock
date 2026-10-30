// System notifications. Windows shows a toast only for an app it knows, so
// the first time this registers her id (HKCU\Software\Classes\AppUserModelId),
// with a name and an icon: nothing to install.
use std::path::Path;

// 通知归属的 AUMID 与应用标识一致（#t18：两者都是 com.fish.dock；留旧值等于再造
// 一套身份，正是本批要清掉的东西）。代价：Action Center 里旧 AUMID
// com.fish.dshfish 名下的历史通知仍归旧名，不随新名合并——旧通知归历史，
// 新通知一律走这个新 AUMID。branding.test.mjs 有一条断言盯着「APP_ID == identifier」。
pub const APP_ID: &str = "com.fish.dock";

#[cfg(windows)]
pub fn register(dir: &Path) {
    use winreg::enums::HKEY_CURRENT_USER;
    use winreg::RegKey;

    let icon = dir.join("fish-dock-user-icon-v1.png");
    if !icon.exists() {
        let _ = std::fs::create_dir_all(dir);
        let _ = std::fs::write(&icon, include_bytes!("../icons/fish-dock.png"));
    }
    if let Ok((key, _)) = RegKey::predef(HKEY_CURRENT_USER).create_subkey(format!(r"Software\Classes\AppUserModelId\{APP_ID}")) {
        let _ = key.set_value("DisplayName", &"FISH Dock");
        let _ = key.set_value("IconUri", &icon.to_string_lossy().into_owned());
    }
}

#[cfg(windows)]
pub fn show(body: &str) -> Result<(), String> {
    tauri_winrt_notification::Toast::new(APP_ID)
        .title("FISH Dock")
        .text1(body)
        .sound(None)
        .show()
        .map_err(|e| e.to_string())
}

#[cfg(not(windows))]
pub fn register(_dir: &Path) {}

#[cfg(not(windows))]
pub fn show(_body: &str) -> Result<(), String> {
    Err("not on this system".into())
}
