//! 插件下载进度（t29；t38 改为按安装目标 id 键化）：官方商城与第三方走同一条按块
//! 读取的路径，读到的字节数经 IPC 推给设置页的进度弹窗；取消是一个按 id 分开的
//! 协作式标志，下载循环每个块检查自己那一个。
//!
//! t30 验收复现过单例串台：`begin(A)` 后 `begin(B)` 覆盖同一个槽；`cancel("")`
//! 会命中当前那个；`finish` 会把别人的弹窗一起关掉。现在每个安装目标一份进度，
//! **空 id 一律拒绝**——不允许「空 id 表示当前那个」这种会误伤别人的写法。
use std::collections::HashMap;
use std::io::Read;
use std::sync::{Mutex, OnceLock};
use serde_json::{json, Value};

#[derive(Clone)]
struct Progress {
    phase: &'static str,
    received: u64,
    total: Option<u64>,
    cancelled: bool,
}
static MAP: Mutex<Option<HashMap<String, Progress>>> = Mutex::new(None);
static APP: OnceLock<tauri::AppHandle> = OnceLock::new();

/// 记下设置窗口的 AppHandle，之后每次进度都往它推事件（只设一次）。
pub fn set_app(app: &tauri::AppHandle) {
    let _ = APP.set(app.clone());
}

fn with_map<T>(f: impl FnOnce(&mut HashMap<String, Progress>) -> T) -> T {
    let mut slot = MAP.lock().unwrap();
    f(slot.get_or_insert_with(HashMap::new))
}

fn reject(id: &str) -> Result<(), String> {
    if id.trim().is_empty() { Err("插件进度缺少安装目标 id".into()) } else { Ok(()) }
}

fn emit() {
    let Some(app) = APP.get() else { return };
    use tauri::Emitter;
    let _ = app.emit("plugin-progress", snapshot());
}

/// 一次下载开始：记下这个安装目标，清空它上一轮的进度与取消标志。
pub fn begin(id: &str) -> Result<(), String> {
    reject(id)?;
    with_map(|map| { map.insert(id.to_string(), Progress { phase: "下载", received: 0, total: None, cancelled: false }); });
    emit();
    Ok(())
}

/// 总量已知时（Content-Length / 目录里的 bytes）报一次，前端才能画百分比。
pub fn know_total(id: &str, total: Option<u64>) {
    with_map(|map| { if let Some(p) = map.get_mut(id) { p.total = total; } });
    emit();
}

/// 每读完一块报一次。phase 至少区分「下载 / 校验 / 解包 / 提交」。
pub fn report(id: &str, phase: &'static str, received: u64, total: Option<u64>) -> Result<(), String> {
    reject(id)?;
    with_map(|map| {
        if let Some(p) = map.get_mut(id) {
            p.phase = phase;
            p.received = received;
            if total.is_some() { p.total = total; }
        }
    });
    emit();
    Ok(())
}

/// 取消指定安装目标；空 id 直接报错（绝不退化成「取消当前那个」）。
pub fn cancel(id: &str) -> Result<(), String> {
    reject(id)?;
    with_map(|map| { if let Some(p) = map.get_mut(id) { p.cancelled = true; } });
    emit();
    Ok(())
}

pub fn cancelled(id: &str) -> bool {
    with_map(|map| map.get(id).is_some_and(|p| p.cancelled))
}

/// 这个安装目标结束（成功或失败）：只清它自己的进度，不动别人的弹窗。
/// 空 id 报错（与 begin/cancel 一致：不允许「空 id 表示当前那个」）。
pub fn finish(id: &str) -> Result<(), String> {
    reject(id)?;
    with_map(|map| { map.remove(id); });
    emit();
    Ok(())
}

/// RAII 守卫：某个安装目标结束（成功、失败或提前 return）时只清它自己。
pub struct Guard { id: String }
impl Guard {
    pub fn start(id: &str) -> Result<Self, String> { begin(id)?; Ok(Guard { id: id.to_string() }) }
}
impl Drop for Guard {
    fn drop(&mut self) { let _ = finish(&self.id); }
}

/// 前端轮询 / 事件缺省时用：当前所有活跃进度的快照（按 id 各一条）。
pub fn snapshot() -> Value {
    let items: Vec<Value> = with_map(|map| map.iter().map(|(id, p)| json!({
        "id": id, "phase": p.phase, "received": p.received, "total": p.total,
        "cancelled": p.cancelled, "active": true,
    })).collect());
    json!({ "items": items })
}

/// 某安装目标的进度（测试/内部用）。
pub fn progress_of(id: &str) -> Option<Value> {
    with_map(|map| map.get(id).map(|p| json!({
        "id": id, "phase": p.phase, "received": p.received, "total": p.total, "cancelled": p.cancelled, "active": true,
    })))
}

/// 按 64 KiB 逐块读完。id 给出时按这个安装目标报进度/检查取消，None 时只受 limit
/// 约束（目录等非安装下载）。超 limit 立即失败——上限不因分块放宽。
pub fn read_chunked(id: Option<&str>, reader: &mut impl Read, limit: u64) -> Result<Vec<u8>, String> {
    let mut out: Vec<u8> = Vec::new();
    let mut buf = [0u8; 64 * 1024];
    loop {
        if let Some(id) = id { if cancelled(id) { return Err("已取消".into()); } }
        let n = reader.read(&mut buf).map_err(|_| "插件下载中断".to_string())?;
        if n == 0 { break; }
        if out.len() as u64 + n as u64 > limit { return Err("插件下载超过大小限制".into()); }
        out.extend_from_slice(&buf[..n]);
        if let Some(id) = id { let _ = report(id, "下载", out.len() as u64, None); }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    static TEST_LOCK: Mutex<()> = Mutex::new(());

    fn reset() { with_map(|map| map.clear()); }

    // A5-①：同时 begin 两个 id，各自 received 递增互不干扰。
    #[test]
    fn a5_two_installs_keep_their_own_progress() {
        let _g = TEST_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        reset();
        begin("official-A").unwrap();
        begin("third-B").unwrap();
        report("official-A", "下载", 100, Some(1000)).unwrap();
        report("third-B", "下载", 50, Some(500)).unwrap();
        report("official-A", "下载", 200, Some(1000)).unwrap();
        assert_eq!(progress_of("official-A").unwrap()["received"], 200);
        assert_eq!(progress_of("third-B").unwrap()["received"], 50);
        assert_eq!(progress_of("official-A").unwrap()["id"], "official-A");
        assert_eq!(progress_of("third-B").unwrap()["id"], "third-B");
        println!("[A5-1] A={} B={} 独立", progress_of("official-A").unwrap(), progress_of("third-B").unwrap());
        reset();
    }

    // A5-②：cancel("") 必须报错，且两个安装都没被取消。
    #[test]
    fn a5_empty_id_cancel_is_rejected_and_touches_nobody() {
        let _g = TEST_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        reset();
        begin("official-A").unwrap();
        begin("third-B").unwrap();
        assert!(cancel("").is_err(), "空 id 取消必须报错");
        assert!(cancel("   ").is_err(), "空白 id 取消必须报错");
        assert!(begin("").is_err(), "空 id 开进度必须报错");
        assert!(finish("").is_err(), "空 id 结束必须报错");
        assert!(!cancelled("official-A"));
        assert!(!cancelled("third-B"));
        println!("[A5-2] cancel(\"\")->{:?} A_cancelled={} B_cancelled={}", cancel("").unwrap_err(), cancelled("official-A"), cancelled("third-B"));
        reset();
    }

    // A5-③：cancel(A) 之后 B 仍继续读（取消只影响被点的那一个）。
    #[test]
    fn a5_cancelling_one_does_not_stop_the_other() {
        let _g = TEST_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        reset();
        begin("official-A").unwrap();
        begin("third-B").unwrap();
        cancel("official-A").unwrap();
        assert!(cancelled("official-A"));
        assert!(!cancelled("third-B"), "A 的取消不得命中 B");
        let mut a: &[u8] = b"official payload";
        assert_eq!(read_chunked(Some("official-A"), &mut a, 1024).unwrap_err(), "已取消");
        let mut b: &[u8] = b"third payload";
        let b_read = read_chunked(Some("third-B"), &mut b, 1024).unwrap();
        assert_eq!(b_read, b"third payload", "B 必须继续读完");
        println!("[A5-3] cancel(A) 后 A_cancelled={} B_cancelled={} B_read={:?}", cancelled("official-A"), cancelled("third-B"), std::str::from_utf8(&b_read));
        reset();
    }

    // A5-④：finish(A) 只关 A 的弹窗，B 仍存活。
    #[test]
    fn a5_finishing_one_keeps_the_other_popup_alive() {
        let _g = TEST_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        reset();
        begin("official-A").unwrap();
        begin("third-B").unwrap();
        finish("official-A").unwrap();
        assert!(progress_of("official-A").is_none(), "A 完成应关闭 A 的弹窗");
        assert!(progress_of("third-B").is_some(), "A 完成不得影响 B 的弹窗");
        let snap = snapshot();
        let ids: Vec<&str> = snap["items"].as_array().unwrap().iter().filter_map(|i| i["id"].as_str()).collect();
        assert_eq!(ids, vec!["third-B"]);
        println!("[A5-4] finish(A) 后 snapshot={}", snapshot());
        reset();
    }

    // 分块读取：小字节流原样返回、received 累加、超 limit 拒绝。
    #[test]
    fn chunked_read_reports_bytes_and_enforces_the_limit() {
        let _g = TEST_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        reset();
        begin("acme.weather").unwrap();
        know_total("acme.weather", Some(10));
        let mut input: &[u8] = b"0123456789";
        assert_eq!(read_chunked(Some("acme.weather"), &mut input, 10).unwrap(), b"0123456789");
        let p = progress_of("acme.weather").unwrap();
        assert_eq!((p["received"].as_u64(), p["total"].as_u64()), (Some(10), Some(10)));
        let mut too_big: &[u8] = b"0123456789A";
        assert!(read_chunked(Some("acme.weather"), &mut too_big, 10).unwrap_err().contains("超过大小限制"));
        finish("acme.weather").unwrap();
        assert!(snapshot()["items"].as_array().unwrap().is_empty());
        reset();
    }
}
