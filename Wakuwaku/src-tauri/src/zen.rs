//! The Zen desktop page's business: Windhawk's installed mods, seen and
//! switched, and ZenDesktop's nine mods put in with one press (v4 设计 §4).
//!
//! Nothing here rewrites Windhawk: the engine watches its own registry keys
//! (`RegNotifyChangeKeyValue`), so writing `Disabled` under
//! `HKLM\SOFTWARE\Windhawk\Engine\Mods\<id>` is the whole switch, and it takes
//! effect without a restart. What needs elevation writes it through a second
//! run of her own exe (`--wakuwaku-zen`), one UAC for one diff.

use serde::Serialize;
use serde_json::{json, Value};
use std::io::Write;
use std::path::PathBuf;

/// One mod as the page sees it (§4.3): its four states decide the checkbox.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ZenMod {
    pub id: String,
    pub disabled: bool,
    pub version: String,
    pub include: String,
    pub exclude: String,
    /// Empty = not compiled yet (the checkbox turns into "compile once in Windhawk").
    pub library_file: String,
    /// true = `ModsSource\<id>.wh.cpp` is there, so compiling is possible.
    pub has_source: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ZenState {
    pub installed: bool,
    pub engine_running: bool,
    pub install_dir: Option<String>,
    pub data_dir: Option<String>,
    pub mods: Vec<ZenMod>,
}

/// v4 §8: the page's failure, a code and a word.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ZenFailure {
    code: String,
    message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    detail: Option<String>,
}

impl ZenFailure {
    fn new(code: &str, message: impl Into<String>) -> Self {
        Self { code: code.into(), message: message.into(), detail: None }
    }

    fn detail(mut self, detail: impl Into<String>) -> Self {
        let detail = detail.into();
        if !detail.trim().is_empty() {
            self.detail = Some(detail);
        }
        self
    }
}

// --- Windhawk 探测（§4.2，deploy.ps1:84-107 的顺序） -----------------------------------

fn windhawk_install_dir() -> Option<PathBuf> {
    #[cfg(windows)]
    {
        use winreg::enums::HKEY_LOCAL_MACHINE;
        use winreg::RegKey;
        for key in [
            r"SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\Windhawk",
            r"SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall\Windhawk",
        ] {
            if let Ok(opened) = RegKey::predef(HKEY_LOCAL_MACHINE).open_subkey(key) {
                if let Ok(location) = opened.get_value::<String, _>("InstallLocation") {
                    let dir = PathBuf::from(location);
                    if dir.join("windhawk.exe").is_file() {
                        return Some(dir);
                    }
                }
            }
        }
    }
    for base in ["ProgramFiles", "ProgramFiles(x86)"] {
        if let Some(dir) = std::env::var_os(base) {
            let dir = PathBuf::from(dir).join("Windhawk");
            if dir.join("windhawk.exe").is_file() {
                return Some(dir);
            }
        }
    }
    None
}

/// 数据根（§4.2，deploy.ps1:133-170）：portable 布局或 `%ProgramData%\Windhawk`。
fn windhawk_data_dir(install: &std::path::Path) -> PathBuf {
    #[cfg(windows)]
    {
        // windhawk.ini 的 AppDataPath = ...（相对路径拼在安装目录下）。
        if let Ok(text) = std::fs::read_to_string(install.join("windhawk.ini")) {
            for line in text.lines() {
                let Some(rest) = line.strip_prefix("AppDataPath") else { continue };
                let value = rest.trim_start_matches([' ', '=']).trim();
                if value.is_empty() {
                    continue;
                }
                let expanded = expand_env(value);
                let path = PathBuf::from(&expanded);
                return if path.is_absolute() { path } else { install.join(path) };
            }
        }
    }
    let _ = install;
    PathBuf::from(std::env::var_os("ProgramData").unwrap_or_default()).join("Windhawk")
}

#[cfg(windows)]
fn expand_env(text: &str) -> String {
    // %VAR% 展开；不认识的保持原样。
    let mut out = String::new();
    let mut rest = text;
    while let Some(at) = rest.find('%') {
        out.push_str(&rest[..at]);
        rest = &rest[at + 1..];
        match rest.find('%') {
            Some(end) => {
                let name = &rest[..end];
                match std::env::var(name) {
                    Ok(value) => out.push_str(&value),
                    Err(_) => out.push_str(&format!("%{name}%")),
                }
                rest = &rest[end + 1..];
            }
            None => {
                out.push('%');
                break;
            }
        }
    }
    out.push_str(rest);
    out
}

// --- 注册表读取（§4.3） ---------------------------------------------------------------

#[cfg(windows)]
const MODS_KEY: &str = r"SOFTWARE\Windhawk\Engine\Mods";

/// 引擎在跑吗：`HKLM\...\Engine\Mods` 可读即认为装了；进程表另看服务。
fn mods_installed() -> bool {
    #[cfg(windows)]
    {
        use winreg::enums::{HKEY_LOCAL_MACHINE, KEY_READ};
        winreg::RegKey::predef(HKEY_LOCAL_MACHINE).open_subkey_with_flags(MODS_KEY, KEY_READ).is_ok()
    }
    #[cfg(not(windows))]
    false
}

fn engine_running() -> bool {
    #[cfg(windows)]
    {
        // 服务名照 Windhawk 的安装（windhawk service）；查不到服务就按没跑算，
        // 页面只影响文案，不挡启停（写注册表本身不需要服务活着）。
        use std::os::windows::process::CommandExt;
        use std::process::Command;
        Command::new("sc")
            .args(["query", "Windhawk"])
            .creation_flags(0x0800_0000)
            .output()
            .map(|out| String::from_utf8_lossy(&out.stdout).contains("RUNNING"))
            .unwrap_or(false)
    }
    #[cfg(not(windows))]
    false
}

#[cfg(windows)]
fn creation_flags(command: &mut std::process::Command) {
    use std::os::windows::process::CommandExt;
    command.creation_flags(0x0800_0000);
}

/// 引擎表里的每个 mod：只读这六个值，写回的只有 Disabled（§4.4）。
#[cfg(windows)]
fn read_mods() -> Vec<ZenMod> {
    use winreg::enums::{HKEY_LOCAL_MACHINE, KEY_READ};
    use winreg::RegKey;
    let Ok(root) = RegKey::predef(HKEY_LOCAL_MACHINE).open_subkey_with_flags(MODS_KEY, KEY_READ) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for id in root.enum_keys().flatten() {
        let Ok(key) = root.open_subkey(&id) else { continue };
        let data_dir = windhawk_data_dir(&windhawk_install_dir().unwrap_or_default());
        let source = data_dir.join("ModsSource").join(format!("{id}.wh.cpp"));
        out.push(ZenMod {
            library_file: key.get_value("LibraryFileName").unwrap_or_default(),
            disabled: key.get_value::<u32, _>("Disabled").unwrap_or(1) != 0,
            version: key.get_value("Version").unwrap_or_default(),
            include: key.get_value("Include").unwrap_or_default(),
            exclude: key.get_value("Exclude").unwrap_or_default(),
            id,
            has_source: source.is_file(),
        });
    }
    out.sort_by(|a, b| a.id.cmp(&b.id));
    out
}

#[cfg(not(windows))]
fn read_mods() -> Vec<ZenMod> {
    Vec::new()
}

// --- 提权写入（§4.4） -----------------------------------------------------------------

/// 提权子进程的请求：`--wakuwaku-zen apply <request.json>`；改的只有这一个值。
#[derive(Clone, Debug, PartialEq, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ZenChange {
    pub id: String,
    pub disabled: bool,
}

/// 请求与结果 JSON 的形状（§4.4）：只认 `changes: [{id, disabled}]`，id 限
/// Windhawk 的字符集，其余一律拒绝。
pub fn parse_request(text: &str) -> Result<Vec<ZenChange>, ZenFailure> {
    let value: Value = serde_json::from_str(text)
        .map_err(|error| ZenFailure::new("REQUEST_BAD", "请求的 JSON 不对").detail(error.to_string()))?;
    let list = value.get("changes").and_then(Value::as_array).ok_or_else(|| ZenFailure::new("REQUEST_BAD", "请求里没有 changes 数组"))?;
    if list.len() > 64 {
        return Err(ZenFailure::new("REQUEST_BAD", "一次至多改 64 个 mod"));
    }
    let mut out = Vec::new();
    for item in list {
        let id = item.get("id").and_then(Value::as_str).ok_or_else(|| ZenFailure::new("REQUEST_BAD", "一个 change 缺 id"))?;
        if id.is_empty() || id.len() > 128 || !id.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'@' | b'.' | b'-' | b'_' | b' ')) {
            return Err(ZenFailure::new("REQUEST_BAD", format!("id 不合规矩：{id}")));
        }
        let disabled = item.get("disabled").and_then(Value::as_bool).ok_or_else(|| ZenFailure::new("REQUEST_BAD", "一个 change 缺 disabled"))?;
        out.push(ZenChange { id: id.to_string(), disabled });
    }
    Ok(out)
}

/// 提权子进程干的全部事：逐个写 `Mods\<id>\Disabled`（§4.4）。返回给父进程的
/// 结果：每项一个 ok，或整包拒绝。
pub fn admin_apply(changes: &[ZenChange]) -> Result<(), String> {
    #[cfg(windows)]
    {
        use winreg::enums::{HKEY_LOCAL_MACHINE, KEY_SET_VALUE};
        use winreg::RegKey;
        let root = RegKey::predef(HKEY_LOCAL_MACHINE).open_subkey_with_flags(MODS_KEY, winreg::enums::KEY_SET_VALUE).map_err(|e| format!("打不开引擎表：{e}"))?;
        for change in changes {
            let key = root.open_subkey_with_flags(&change.id, KEY_SET_VALUE).map_err(|e| format!("{}：{e}", change.id))?;
            key.set_value("Disabled", &(u32::from(change.disabled))).map_err(|e| format!("{}：{e}", change.id))?;
        }
        Ok(())
    }
    #[cfg(not(windows))]
    {
        let _ = changes;
        Err("本设计只覆盖 Windows".into())
    }
}

/// 非提权路径：把请求写成临时文件，用 `runas` 拉起她自己的 exe，等结果文件。
fn elevate_and_apply(sh: &crate::Shared, changes: &[ZenChange]) -> Result<(), ZenFailure> {
    let temp = std::env::temp_dir();
    let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(0);
    let request = temp.join(format!("wakuwaku-zen-{}-{stamp}.json", std::process::id()));
    let result = temp.join(format!("wakuwaku-zen-{}-{stamp}.result", std::process::id()));
    std::fs::write(&request, json!({ "changes": changes.iter().map(|c| json!({ "id": c.id, "disabled": c.disabled })).collect::<Vec<_>>() }).to_string())
        .map_err(|error| ZenFailure::new("REQUEST_BAD", "请求文件没能落盘").detail(error.to_string()))?;
    let _ = std::fs::remove_file(&result);

    let exe = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("wakuwaku.exe"));
    let params = format!("--wakuwaku-zen apply \"{}\" \"{}\"", request.display(), result.display());
    let asked = crate::lhm_setup::elevate(sh, &exe, &params, &temp, Some(60_000));
    let _ = std::fs::remove_file(&request);
    if !matches!(asked, crate::lhm_setup::Asked::Ran(_)) {
        let why = match asked {
            crate::lhm_setup::Asked::Declined => "提权没有被答应：注册表没改".to_string(),
            crate::lhm_setup::Asked::Failed(error) => format!("提权没能进行：{error}"),
            crate::lhm_setup::Asked::Ran(_) => unreachable!(),
        };
        return Err(ZenFailure::new("ELEVATION_REFUSED", why));
    }
    let answer = std::fs::read_to_string(&result).map_err(|_| ZenFailure::new("ELEVATION_REFUSED", "提权子进程没有留下结果"))?;
    let _ = std::fs::remove_file(&result);
    if answer.trim() == "ok" {
        Ok(())
    } else {
        Err(ZenFailure::new("WRITE_FAILED", "注册表没写成").detail(answer))
    }
}

/// 这个进程现在能不能直接改引擎表（= 已提权）：试着开 KEY_SET_VALUE。
/// lhm_setup::is_elevated 是测试专用（#[cfg(test)]），这里以真实权限为准。
fn can_write_mods() -> bool {
    #[cfg(windows)]
    {
        use winreg::enums::{HKEY_LOCAL_MACHINE, KEY_SET_VALUE};
        winreg::RegKey::predef(HKEY_LOCAL_MACHINE).open_subkey_with_flags(MODS_KEY, KEY_SET_VALUE).is_ok()
    }
    #[cfg(not(windows))]
    false
}

// --- 对外的命令 -----------------------------------------------------------------------

/// 只读的整页状态（§4.1 的状态卡与 mod 列表）。
#[tauri::command]
pub fn zen_state() -> ZenState {
    let install = windhawk_install_dir();
    let installed = install.is_some() || mods_installed();
    let data = install.as_ref().map(|dir| windhawk_data_dir(dir).to_string_lossy().to_string());
    ZenState { installed, engine_running: installed && engine_running(), install_dir: install.map(|d| d.to_string_lossy().to_string()), data_dir: data, mods: if installed { read_mods() } else { Vec::new() } }
}

/// 应用一个 diff（§4.4）：已提权直接写；否则一次 UAC。
#[tauri::command]
pub fn zen_apply(app: tauri::AppHandle, changes: Value) -> Result<Value, ZenFailure> {
    let sh = crate::shared(&app);
    let list: Vec<ZenChange> = serde_json::from_value(changes).map_err(|error| ZenFailure::new("REQUEST_BAD", "changes 不合形状").detail(error.to_string()))?;
    if list.is_empty() {
        return Ok(json!({ "applied": 0 }));
    }
    if can_write_mods() {
        admin_apply(&list).map_err(|error| ZenFailure::new("WRITE_FAILED", "注册表没写成").detail(error))?;
    } else {
        elevate_and_apply(&sh, &list)?;
    }
    Ok(json!({ "applied": list.len() }))
}

/// 命令行分支：提权子进程 `--wakuwaku-zen apply <request.json> <result.json>`。
/// 只做三件事（§4.4）：解析、写、留结果。
pub fn admin(args: &[String]) -> i32 {
    let Some(request) = args.first() else {
        eprintln!("wakuwaku-zen: no request file");
        return 2;
    };
    let Some(result) = args.get(1) else {
        eprintln!("wakuwaku-zen: no result file");
        return 2;
    };
    let outcome = std::fs::read_to_string(request)
        .map_err(|e| format!("请求读不到：{e}"))
        .and_then(|text| parse_request(&text).map_err(|f| format!("{}：{}", f.code, f.message)))
        .and_then(|changes| admin_apply(&changes).map_err(|e| e));
    let mut file = match std::fs::File::create(result) {
        Ok(file) => file,
        Err(error) => {
            eprintln!("wakuwaku-zen: 结果文件写不了：{error}");
            return 2;
        }
    };
    let code = match outcome {
        Ok(()) => {
            let _ = file.write_all(b"ok");
            0
        }
        Err(error) => {
            let _ = file.write_all(error.as_bytes());
            1
        }
    };
    code
}

// --- 测试（§9.2：纯函数，不碰真注册表） -------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_parses_and_rejects_the_rest() {
        let good = r#"{ "changes": [ { "id": "local@zen-taskbar-acrylic", "disabled": false }, { "id": "windows-animations", "disabled": true } ] }"#;
        let parsed = parse_request(good).unwrap();
        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[0], ZenChange { id: "local@zen-taskbar-acrylic".into(), disabled: false });
        assert_eq!(parsed[1], ZenChange { id: "windows-animations".into(), disabled: true });

        assert!(parse_request(r#"{}"#).is_err(), "没有 changes 数组");
        assert!(parse_request(r#"{ "changes": [{}] }"#).is_err(), "缺 id");
        assert!(parse_request(r#"{ "changes": [{ "id": "x" }] }"#).is_err(), "缺 disabled");
        assert!(parse_request(r#"{ "changes": [{ "id": "x..\\..\\y", "disabled": true }] }"#).is_err(), "id 不合字符集");
        assert!(parse_request(r#"{ "changes": [{ "id": "x", "disabled": 1 }] }"#).is_err(), "disabled 要是布尔");
        let many = format!(r#"{{ "changes": [{}] }}"#, vec!["x"; 65].iter().enumerate().map(|(i, _)| format!(r#"{{ "id": "m{i}", "disabled": true }}"#)).collect::<Vec<_>>().join(","));
        assert!(parse_request(&many).is_err(), "65 个超上限");
    }

    #[test]
    fn id_charset_is_what_windhawk_uses() {
        assert!(parse_request(r#"{ "changes": [{ "id": "local@a-b_c.wh.cpp 9", "disabled": true }] }"#).is_ok(), "@ . - _ 空格 数字都可以");
    }

    #[cfg(windows)]
    #[test]
    fn appdata_path_expands_and_joins() {
        assert_eq!(expand_env("%ProgramData%\\Windhawk"), format!("{}\\Windhawk", std::env::var("ProgramData").unwrap_or_default()));
        assert_eq!(expand_env("no-vars-here"), "no-vars-here");
        assert_eq!(expand_env("%NOPE_NOT_SET%x"), "%NOPE_NOT_SET%x");
    }

    #[test]
    fn admin_state_says_installed_false_before_anything() {
        // 没装 Windhawk 的机器（本机实测）：installed=false，mods 空，不报错。
        let state = zen_state();
        if !state.installed {
            assert!(state.mods.is_empty());
            assert!(!state.engine_running);
        }
    }
}
