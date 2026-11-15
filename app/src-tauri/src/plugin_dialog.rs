//! 2.3.4 宿主能力：插件后端选文件夹（host.dialogs.chooseFolder）。
//!
//! 对应 docs/specs/2026-11-15-host-dialogs-design.md（下称「规格」）。要点：
//! - 只有声明 `dialogs` 能力的后端能调用；页面（sdk/browser）不提供；
//! - 参数在插件侧与宿主侧都校验（title 1–80 字无控制字符、parent 只能是 'main'）；
//! - 全局同一时间只允许一个对话框（DIALOG_BUSY，不排队）；
//! - 在独立工作线程里弹框、用同一 id 写回响应，不占插件回调线程，宿主不设超时；
//! - 成功 `{dir:path}`，取消 `{dir:null}`，COM 失败 DIALOG_OPERATION_FAILED，
//!   非 Windows DIALOG_UNSUPPORTED；日志只记 shown/chosen/cancelled，不记路径。
use serde_json::{json, Value};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::SyncSender;

use crate::storage_context::Context;

/// 全局对话框互斥：一次一个。抢不到就回 DIALOG_BUSY，不排队（排队只会让后来的
/// 调用等一个已过期的意图）。
static DIALOG_BUSY: AtomicBool = AtomicBool::new(false);

const TITLE_MAX: usize = 80;

/// 参数校验：parent 缺省为 'main'，只接受 'main'；title 可缺省（用宿主 i18n 默认），
/// 给了就必须是 1–80 字、无控制字符。未知字段拒绝。纯逻辑，便于单测。
/// 返回 Ok(None) = 用默认标题；Ok(Some(title)) = 插件标题；Err = 拒绝理由。
pub fn validate_params(params: &Value) -> Result<Option<String>, String> {
    let Some(object) = params.as_object() else { return Err("选文件夹参数应为对象".into()); };
    for key in object.keys() {
        if !["title", "parent"].contains(&key.as_str()) {
            return Err("选文件夹参数含未知字段".into());
        }
    }
    if let Some(parent) = object.get("parent") {
        let parent = parent.as_str().ok_or("parent 只能是 'main'")?;
        if parent != "main" {
            return Err("parent 只能是 'main'".into());
        }
    }
    match object.get("title") {
        None => Ok(None),
        Some(Value::String(title)) => validate_title(title).map(|_| Some(title.clone())),
        Some(_) => Err("title 必须是字符串".into()),
    }
}

/// title 校验：去两端空白后非空、字符数 ≤ 80、不含控制字符。
pub fn validate_title(title: &str) -> Result<(), String> {
    let trimmed = title.trim();
    if trimmed.is_empty() {
        return Err("title 不能为空".into());
    }
    if trimmed.chars().count() > TITLE_MAX {
        return Err("title 最长 80 字".into());
    }
    // 控制字符（含 \n、\r、\t 与 C0/C1）：拒绝。这是规格里「去控制字符」的落点，
    // 也是「title 控制字符矛盾」的解决：校验时直接拒绝，而非静默剥离后放行。
    if trimmed.chars().any(char::is_control) {
        return Err("title 不能包含控制字符".into());
    }
    Ok(())
}

/// 平台无关的映射：把「选中 / 取消 / COM 失败」三态从 paper 的 Result 翻成宿主错误码。
/// 纯函数，便于单测——修的是「production pick_folder 永不返回 Err，COM 失败被当取消」的缺口。
fn map_pick(result: Result<Option<String>, ()>) -> Result<Option<String>, String> {
    result.map_err(|()| "DIALOG_OPERATION_FAILED".to_string())
}

/// 平台相关：真正弹框。成功返回 Some(路径)，取消返回 None，COM 失败返回 Err。
/// Windows 走 paper::pick_reason(true, title, "", owner)（已 STA/IFileOpenDialog/FOS_PICKFOLDERS）；
/// 用 pick_reason 而非 pick，才能把「COM 对象创建失败」从「用户取消」里分出来
/// （DLG-2 要求 COM 失败回 DIALOG_OPERATION_FAILED，不是 {dir:null}）。
/// 非 Windows 直接 DIALOG_UNSUPPORTED（不复用 paper 的空实现去冒充「取消」）。
#[cfg(windows)]
fn pick_folder(title: &str, owner: isize) -> Result<Option<String>, String> {
    map_pick(crate::paper::pick_reason(true, title, "", owner))
}

#[cfg(not(windows))]
fn pick_folder(_title: &str, _owner: isize) -> Result<Option<String>, String> {
    Err("DIALOG_UNSUPPORTED".into())
}

// 可注入的 opener 替身：测试里记录参数、返回固定路径/None/失败，覆盖三条路径，
// 不让自动测试真弹模态框（规格 §6.1）。生产用 pick_folder。
type Opener<'a> = dyn Fn(&str, isize) -> Result<Option<String>, String> + 'a;

/// 用给定 opener 执行一次选文件夹并组装结果帧（工作线程与测试共用同一实现，
/// 只差 opener 是真实弹框还是替身）。可在任意线程调用，便于单测覆盖三条路径。
fn choose_with(id: &Value, plugin: &str, title: &str, owner: isize, opener: &Opener) -> Value {
    log("shown", plugin);
    match opener(title, owner) {
        Ok(Some(dir)) => { log("chosen", plugin); json!({"id": id, "result": {"dir": dir}}) }
        Ok(None) => { log("cancelled", plugin); json!({"id": id, "result": {"dir": Value::Null}}) }
        Err(code) => json!({"id": id, "error": failure(&code)}),
    }
}

/// 校验后端身份与能力（与 plugin_window::backend 同源）：代次新鲜、插件已安装、
/// 声明了 dialogs 能力。返回插件标题（可选）。
fn validate(context: &Context, plugin: &str, generation: u64, params: &Value) -> Result<Option<String>, String> {
    if generation == 0 || crate::plugin_window::current_generation(plugin) != generation {
        return Err("插件实例代次已变化，本次对话框请求已丢弃".into());
    }
    let row = crate::plugin_install::Manager { context }
        .list()?
        .into_iter()
        .find(|row| row.manifest.id == plugin)
        .ok_or_else(|| "插件未安装".to_string())?;
    if !row.manifest.capabilities.iter().any(|c| c == "dialogs") {
        return Err("插件未声明 dialogs 能力".into());
    }
    validate_params(params)
}

/// 组装回包的错误形状：码固定，文案只在可安全上行时原样给（长度/控制字符判据
/// 照 plugin_window::failure），否则退回一句通用说明——绝不夹带路径。
pub fn failure(error: &str) -> Value {
    let (code, fallback) = match error {
        "DIALOG_BUSY" => ("DIALOG_BUSY", "已有一个对话框打开着，请先关掉它"),
        "DIALOG_OPERATION_FAILED" => ("DIALOG_OPERATION_FAILED", "打开文件夹选择框失败"),
        "DIALOG_UNSUPPORTED" => ("DIALOG_UNSUPPORTED", "当前系统不支持文件夹选择"),
        "插件未声明 dialogs 能力" => ("DIALOG_CAPABILITY_REQUIRED", "插件未声明 dialogs 能力"),
        _ => ("DIALOG_OPERATION_FAILED", "打开文件夹选择框失败"),
    };
    let message = if matches!(error, "DIALOG_BUSY" | "DIALOG_OPERATION_FAILED" | "DIALOG_UNSUPPORTED" | "插件未声明 dialogs 能力") {
        fallback.to_string()
    } else if !error.is_empty() && error.len() <= 600 && !error.chars().any(char::is_control) {
        // 参数类错误（title/parent/未知字段）可安全上行，便于插件排查。
        error.to_string()
    } else {
        fallback.to_string()
    };
    json!({"code": code, "message": message})
}

/// 日志行（不记路径）：`dialog: <plugin> chooseFolder shown|chosen|cancelled`。
fn log(kind: &str, plugin: &str) {
    if let Ok(app) = crate::plugin_window::app_handle() {
        crate::shared(app).log(&format!("dialog: {plugin} chooseFolder {kind}"));
    }
}

/// 默认标题（插件没给时）：宿主 i18n 的 dialog.chooseFolder。
fn default_title() -> String {
    let lang = crate::plugin_window::app_handle().map(|app| crate::shared(app).lang()).unwrap_or("zh");
    crate::i18n::t(lang, "dialog.chooseFolder")
}

/// 后端回调线程收到 host.dialogs.* 时调用：同步校验，成功就把弹框丢给独立工作线程，
/// 自己立刻返回（回调用同一 id 由工作线程写回）。
pub fn dispatch(context: &Context, plugin: &str, generation: u64, id: Value, method: &str, params: Value, reply: SyncSender<Value>) {
    if method != "host.dialogs.chooseFolder" {
        let _ = reply.send(json!({"id": id, "error": failure("主岛方法无效")}));
        return;
    }
    let title = match validate(context, plugin, generation, &params) {
        Ok(title) => title,
        Err(error) => {
            let _ = reply.send(json!({"id": id, "error": failure(&error)}));
            return;
        }
    };
    // 互斥：抢不到立刻 DIALOG_BUSY。
    if DIALOG_BUSY.compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire).is_err() {
        let _ = reply.send(json!({"id": id, "error": failure("DIALOG_BUSY")}));
        return;
    }
    let plugin = plugin.to_string();
    std::thread::spawn(move || {
        // guard：无论走哪条路径，线程结束时释放互斥（会话在框开着时被停用也一样）。
        struct Guard;
        impl Drop for Guard { fn drop(&mut self) { DIALOG_BUSY.store(false, Ordering::Release); } }
        let _guard = Guard;
        let title = title.unwrap_or_else(default_title);
        let owner = owner_handle();
        let response = choose_with(&id, &plugin, &title, owner, &pick_folder);
        // 会话在框开着时被停用/卸载/换版：写回队列已关，忽略即可，新会话不受影响。
        let _ = reply.send(response);
    });
}

/// 对话框 owner：主岛窗口句柄；为 0（主岛不可见/未建）时退回无 owner，日志记一行。
fn owner_handle() -> isize {
    let owner = crate::plugin_window::app_handle().map(|app| crate::island::hwnd(&crate::shared(app))).unwrap_or(0);
    if owner == 0 {
        if let Ok(app) = crate::plugin_window::app_handle() {
            crate::shared(app).log("dialog: 主岛不可见，选文件夹对话框没有 owner");
        }
    }
    owner
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn params_validate_title_parent_and_unknown_fields() {
        assert_eq!(validate_params(&json!({})), Ok(None), "空参数用默认标题");
        assert_eq!(validate_params(&json!({"title": "选择备份文件夹"})), Ok(Some("选择备份文件夹".into())));
        assert_eq!(validate_params(&json!({"title": "选择", "parent": "main"})), Ok(Some("选择".into())));
        // parent 不是 'main'
        assert!(validate_params(&json!({"parent": "window"})).is_err());
        assert!(validate_params(&json!({"parent": 3})).is_err());
        // title 空、81 字、含 \n、非字符串
        assert!(validate_params(&json!({"title": ""})).is_err());
        assert!(validate_params(&json!({"title": "   "})).is_err());
        assert!(validate_params(&json!({"title": "字".repeat(81)})).is_err());
        assert!(validate_params(&json!({"title": "含\n换行"})).is_err());
        assert!(validate_params(&json!({"title": 12})).is_err());
        // 80 字刚好通过
        assert!(validate_params(&json!({"title": "字".repeat(80)})).is_ok());
        // 未知字段
        assert!(validate_params(&json!({"title": "x", "bogus": 1})).is_err());
        // 非对象
        assert!(validate_params(&json!([])).is_err());
    }

    #[test]
    fn title_control_characters_are_rejected_not_stripped() {
        // 「title 控制字符矛盾」的解决：拒绝含控制字符的标题，而非静默去掉后放行。
        for bad in ["a\tb", "a\rb", "a\u{7}b", "a\u{0}b"] {
            assert!(validate_title(bad).is_err(), "{bad:?} 应拒绝");
        }
        assert!(validate_title("正常的标题").is_ok());
    }

    #[test]
    fn choose_with_covers_success_cancel_and_failure_paths() {
        let id = json!(7);
        // 成功：返回用户选的路径。
        let chosen = choose_with(&id, "acme.weather", "选择备份文件夹", 0, &|_title, _owner| Ok(Some("C:\\Users\\Fish\\Backups".into())));
        assert_eq!(chosen["result"]["dir"], "C:\\Users\\Fish\\Backups");
        // 取消：dir 为 null，不报错。
        let cancelled = choose_with(&id, "acme.weather", "选择备份文件夹", 0, &|_title, _owner| Ok(None));
        assert_eq!(cancelled["result"]["dir"], Value::Null);
        assert!(cancelled.get("error").is_none(), "取消不报错");
        // COM 失败：DIALOG_OPERATION_FAILED。
        let failed = choose_with(&id, "acme.weather", "选择备份文件夹", 0, &|_title, _owner| Err("DIALOG_OPERATION_FAILED".into()));
        assert_eq!(failed["error"]["code"], "DIALOG_OPERATION_FAILED");
    }

    #[test]
    fn failure_shapes_carry_fixed_codes() {
        assert_eq!(failure("DIALOG_BUSY")["code"], "DIALOG_BUSY");
        assert_eq!(failure("DIALOG_OPERATION_FAILED")["code"], "DIALOG_OPERATION_FAILED");
        assert_eq!(failure("DIALOG_UNSUPPORTED")["code"], "DIALOG_UNSUPPORTED");
        assert_eq!(failure("插件未声明 dialogs 能力")["code"], "DIALOG_CAPABILITY_REQUIRED");
        // 参数类错误可安全上行；路径等内容不该被这条误用，但拒错兜底不带路径。
        let bad = failure("title 不能为空");
        assert_eq!(bad["code"], "DIALOG_OPERATION_FAILED");
        assert!(bad["message"].as_str().unwrap().contains("title"));
    }

    // 验证用的回归：DLG-2 要求 COM 对象创建失败映射为 DIALOG_OPERATION_FAILED，
    // 不能当成取消返回 {dir:null}。修复前 pick_folder 直接用 paper::pick（永不返回 Err），
    // COM 失败会被静默当成取消；现在经 map_pick 把 Err(()) 翻成 DIALOG_OPERATION_FAILED。
    #[test]
    fn com_failure_is_operation_failed_not_a_cancel() {
        // paper 三态 → map_pick 的映射：选中、取消、COM 失败各归其位。
        assert_eq!(map_pick(Ok(Some("C:\\Backups".into()))).unwrap(), Some("C:\\Backups".into()));
        assert_eq!(map_pick(Ok(None)).unwrap(), None, "取消仍是 None，不报错");
        assert_eq!(map_pick(Err(())), Err("DIALOG_OPERATION_FAILED".to_string()), "COM 失败不是取消");
        // 端到端：pick_folder 的失败（Err）经 choose_with 组帧后是 DIALOG_OPERATION_FAILED，
        // 而取消是 {dir:null}——两者不可混同。
        let id = json!(1);
        let via_com_fail = choose_with(&id, "p", "t", 0, &|_t, _o| map_pick(Err(())));
        assert_eq!(via_com_fail["error"]["code"], "DIALOG_OPERATION_FAILED");
        let via_cancel = choose_with(&id, "p", "t", 0, &|_t, _o| map_pick(Ok(None)));
        assert_eq!(via_cancel["result"]["dir"], Value::Null);
        assert!(via_cancel.get("error").is_none());
    }
}
