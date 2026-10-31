//! 插件下载进度（t29）：官方商城与第三方走同一条按块读取的路径，读到的字节数经
//! IPC 推给设置页的进度弹窗；取消是一个协作式的标志，下载循环每个块检查一次。
//!
//! 只保存「当前这一个」下载任务：插件的下载在取得实例锁之前进行，界面同一时刻只
//! 允许一个安装/更新操作（busy 时按钮全部禁用），所以一个槽位就够，也避免两份进度
//! 互相覆盖。测试（没装 Tauri AppHandle）里 emit 是空操作，其余状态照常可测。
use std::io::Read;
use std::sync::{Mutex, OnceLock};
use serde_json::{json, Value};

#[derive(Clone, Default)]
struct State {
    active: bool,
    id: String,
    phase: &'static str,
    received: u64,
    total: Option<u64>,
    cancelled: bool,
}
static STATE: Mutex<State> = Mutex::new(State { active: false, id: String::new(), phase: "", received: 0, total: None, cancelled: false });
static APP: OnceLock<tauri::AppHandle> = OnceLock::new();

/// 记下设置窗口的 AppHandle，之后每次进度都往它推事件（只设一次）。
pub fn set_app(app: &tauri::AppHandle) {
    let _ = APP.set(app.clone());
}

fn emit() {
    let Some(app) = APP.get() else { return };
    let s = STATE.lock().unwrap().clone();
    use tauri::Emitter;
    let _ = app.emit("plugin-progress", json!({
        "id": s.id, "phase": s.phase, "received": s.received, "total": s.total,
        "cancelled": s.cancelled, "active": s.active,
    }));
}

/// 一次下载开始时调用：记下插件 id，清空上一轮的进度与取消标志。
pub fn begin(id: &str) {
    *STATE.lock().unwrap() = State { active: true, id: id.to_string(), phase: "下载", received: 0, total: None, cancelled: false };
    emit();
}

/// 总量已知时（Content-Length / 目录里的 bytes）先报一次 total，前端才能画百分比。
pub fn know_total(total: Option<u64>) {
    let mut s = STATE.lock().unwrap();
    if !s.active { return; }
    s.total = total;
    drop(s);
    emit();
}

/// 每读完一块报一次。phase 至少区分「下载 / 校验 / 解包 / 提交」。
pub fn report(phase: &'static str, received: u64, total: Option<u64>) {
    {
        let mut s = STATE.lock().unwrap();
        if !s.active { return; }
        s.phase = phase;
        s.received = received;
        if total.is_some() { s.total = total; }
    }
    emit();
}

/// 取消当前下载（按 id 只取消这一次；id 为空则取消当前任务）。
pub fn cancel(id: &str) {
    let mut s = STATE.lock().unwrap();
    if s.active && (id.is_empty() || s.id == id) { s.cancelled = true; }
    drop(s);
    emit();
}

pub fn cancelled() -> bool { let s = STATE.lock().unwrap(); s.active && s.cancelled }

/// 一次安装/更新结束（成功或失败都一样）：清掉进度，弹窗随之关掉。
pub fn finish() {
    let mut s = STATE.lock().unwrap();
    s.active = false;
    s.phase = "";
    drop(s);
    emit();
}

/// RAII 守卫：一次安装/更新在作用域结束（无论成功、失败还是提前 return）都调用
/// finish，避免异常路径把进度弹窗永远留在「下载中」。语法糖，不改变行为。
pub struct Guard;
impl Guard {
    pub fn start(id: &str) -> Self { begin(id); Guard }
}
impl Drop for Guard {
    fn drop(&mut self) { finish(); }
}

/// 前端轮询 / 事件缺省时用：当前进度快照。
pub fn snapshot() -> Value {
    let s = STATE.lock().unwrap().clone();
    json!({ "id": s.id, "phase": s.phase, "received": s.received, "total": s.total, "cancelled": s.cancelled, "active": s.active })
}

/// 按 64 KiB 逐块读完，累计字节报进度；超 limit 立即失败（上限不因分块放宽），
/// 取消则尽快返回。读到的就是内存里的完整字节，没有半截临时文件要清。
pub fn read_chunked(reader: &mut impl Read, limit: u64) -> Result<Vec<u8>, String> {
    let mut out: Vec<u8> = Vec::new();
    let mut buf = [0u8; 64 * 1024];
    loop {
        if cancelled() { return Err("已取消".into()); }
        let n = reader.read(&mut buf).map_err(|_| "插件下载中断".to_string())?;
        if n == 0 { break; }
        if out.len() as u64 + n as u64 > limit { return Err("插件下载超过大小限制".into()); }
        out.extend_from_slice(&buf[..n]);
        report("下载", out.len() as u64, None);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    // 进度是进程级单例；两条测试串行跑，互不串扰。
    static TEST_LOCK: Mutex<()> = Mutex::new(());

    // 分块读取：小字节流原样返回，进度 received 一路累加到最后；超过 limit 立即拒。
    #[test]
    fn chunked_read_reports_bytes_and_enforces_the_limit() {
        let _guard = TEST_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        begin("acme.weather");
        know_total(Some(10));
        let mut input: &[u8] = b"0123456789";
        let out = read_chunked(&mut input, 10).unwrap();
        assert_eq!(out, b"0123456789");
        let s = snapshot();
        assert_eq!((s["id"].as_str(), s["phase"].as_str()), (Some("acme.weather"), Some("下载")));
        assert_eq!((s["received"].as_u64(), s["total"].as_u64()), (Some(10), Some(10)));
        finish();
        assert_eq!(snapshot()["active"], false);

        begin("acme.weather");
        let mut too_big: &[u8] = b"0123456789A";
        assert!(read_chunked(&mut too_big, 10).unwrap_err().contains("超过大小限制"));
        finish();
    }

    // 取消：置标志后下一次读取立刻返回「已取消」，不再往下读；只取消指定 id。
    #[test]
    fn cancelling_stops_the_reader() {
        let _guard = TEST_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        begin("fish.magpie");
        cancel("fish.other"); // 别人的取消不该影响这一份
        assert!(!cancelled());
        cancel("fish.magpie");
        assert!(cancelled());
        let mut input: &[u8] = b"still here";
        assert_eq!(read_chunked(&mut input, 1024).unwrap_err(), "已取消");
        finish();
        assert!(!cancelled());
    }
}
