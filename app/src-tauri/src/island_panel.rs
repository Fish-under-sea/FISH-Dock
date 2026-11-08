//! 声明式岛子面板 + 2.2.0 通用 Hook：插件窗口（windows）与拖放订阅（fileDrop）。
//!
//! 设计原则（对齐 native-window-hook.md）：宿主**不轮询、不自动开窗、无常驻小球**；
//! 只有插件页面（经 bridge → dock.windows.*）或插件后端（经 Node SDK → host.windows.*）
//! 显式请求时才创建独立 WebView2 窗口。窗口句柄不透明（label 字符串）。停用/卸载/
//! 换版/Dock 退出时回收窗口与订阅。
use std::{collections::HashMap, sync::{mpsc, Mutex, OnceLock}};
use serde_json::{json, Value};
use tauri::{AppHandle, Emitter, Manager, PhysicalPosition, PhysicalSize, WebviewUrl, WebviewWindow, WebviewWindowBuilder};

// ===================== 旧自研 islandPanel（兼容保留，最终移除） =====================
pub struct Panel { ticket: String, win: WebviewWindow }

fn dbg(msg: &str) {
    use std::io::Write;
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open("F:\\AI REP\\ds\\_island_debug.log") {
        let _ = writeln!(f, "{} {}", crate::now_ms(), msg);
    }
}

fn panels() -> &'static Mutex<HashMap<String, Panel>> {
    static P: OnceLock<Mutex<HashMap<String, Panel>>> = OnceLock::new();
    P.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Tauri 窗口 label 只允许可打印 ASCII；插件 id 里的点号等替换成下划线。
fn label_of(id: &str) -> String {
    format!("island-panel-{}", safe(id))
}

/// 旧：开（或前置）某插件的岛子面板窗口（基于 manifest.islandPanel）。保留兼容。
pub fn open(window: &WebviewWindow, id: &str) -> Result<Value, String> {
    dbg(&format!("open({id}) enter"));
    let __r = (|| -> Result<Value, String> {
    let context = crate::storage_context::current().ok_or("存储尚未就绪")?;
    crate::github_access::require_plugin(context, id)?;
    if !crate::plugin_runtime::present(id) { return Err("插件尚未运行".into()); }
    let manager = crate::plugin_install::Manager { context };
    let row = manager.list()?.into_iter().find(|r| r.manifest.id == id && r.enabled).ok_or("插件未启用")?;
    let cfg = row.manifest.island_panel.clone().ok_or("插件未声明岛子面板")?;
    if let Some(p) = panels().lock().map_err(|_| "面板锁不可用")?.get(id) {
        let _ = p.win.show();
        return Ok(json!({ "ticket": p.ticket }));
    }
    let grant = crate::plugin_page::open(&format!("{id}/{}", cfg.page))?;
    let ticket = grant.get("ticket").and_then(Value::as_str).ok_or("面板票据缺失")?.to_string();
    let url = tauri::Url::parse(grant.get("url").and_then(Value::as_str).ok_or("面板地址缺失")?).map_err(|_| "面板地址无效")?;
    let label = label_of(id);
    let w = cfg.width.unwrap_or(240) as f64;
    let h = cfg.height.unwrap_or(120) as f64;
    let (px, py) = match (window.outer_position(), window.outer_size()) {
        (Ok(p), Ok(s)) => ((p.x - w as i32 - 8).max(0), (p.y + (s.height as i32 - h as i32) / 2).max(0)),
        _ => (100, 100),
    };
    let win = WebviewWindowBuilder::new(window.app_handle(), &label, WebviewUrl::External(url))
        .title("FISH Dock")
        .position(px as f64, py as f64)
        .inner_size(w, h)
        .decorations(false)
        .resizable(false)
        .maximizable(false)
        .minimizable(false)
        .skip_taskbar(true)
        .shadow(false)
        .always_on_top(true)
        .visible_on_all_workspaces(true)
        .focused(false)
        .visible(true)
        .build()
        .map_err(|e| format!("面板窗口创建失败：{e}"))?;
    let id2 = id.to_string();
    let ticket2 = ticket.clone();
    let _ = win.on_window_event(move |event| {
        if matches!(event, tauri::WindowEvent::Destroyed) {
            crate::plugin_page::close(&ticket2);
            if let Ok(mut map) = panels().lock() { map.remove(&id2); }
        }
    });
    panels().lock().map_err(|_| "面板锁不可用")?.insert(id.to_string(), Panel { ticket: ticket.clone(), win });
    dbg(&format!("open({id}) OK label={label}"));
    Ok(json!({ "ticket": ticket }))
    })();
    if let Err(ref e) = __r { dbg(&format!("open({id}) ERR: {e}")); }
    __r
}

/// 关某插件的岛子面板（旧）。
pub fn close(id: &str) {
    let panel = panels().lock().ok().and_then(|mut m| m.remove(id));
    if let Some(p) = panel {
        crate::plugin_page::close(&p.ticket);
        let _ = p.win.close();
    }
    // 同时回收新 windows 协议里属于该插件的窗口 / 订阅。
    let labels: Vec<String> = wins().lock().map(|m| m.values().filter(|x| x.plugin == id).map(|x| x.label.clone()).collect()).unwrap_or_default();
    for label in labels {
        if let Ok(mut m) = wins().lock() {
            if let Some(w) = m.remove(&label) { crate::plugin_page::close(&w.ticket); let _ = w.win.close(); }
        }
    }
    if let Ok(mut ix) = windex().lock() { ix.retain(|(p, _), _| p != id); }
    if let Ok(mut d) = drops().lock() { d.remove(id); }
}

/// 把一次拖放转发给该插件的（旧）面板窗口。
pub fn forward_drop(id: &str, at: Value, paths: Value) {
    if let Ok(map) = panels().lock() {
        if let Some(p) = map.get(id) { let _ = p.win.emit("fish:island-drop", json!({ "at": at, "paths": paths })); }
    }
}

/// 主岛锚点变化时更新（旧）面板窗口的位置/尺寸（屏幕物理像素）。
pub fn reposition(id: &str, x: i32, y: i32, w: u32, h: u32) {
    if let Ok(map) = panels().lock() {
        if let Some(p) = map.get(id) {
            let _ = p.win.set_position(PhysicalPosition::new(x, y));
            let _ = p.win.set_size(PhysicalSize::new(w, h));
        }
    }
}

/// Dock 退出：关闭全部窗口（旧 + 新协议）。
pub fn shutdown() {
    if let Ok(mut map) = panels().lock() {
        for (_, p) in map.drain() { crate::plugin_page::close(&p.ticket); let _ = p.win.close(); }
    }
    if let Ok(mut m) = wins().lock() {
        for (_, w) in m.drain() { crate::plugin_page::close(&w.ticket); let _ = w.win.close(); }
    }
    if let Ok(mut ix) = windex().lock() { ix.clear(); }
    if let Ok(mut d) = drops().lock() { d.clear(); }
}

/// 停用/卸载/换版某插件时回收它的窗口。
pub fn close_for(id: &str) { close(id); }

/// 面板页主动 close（按票据）：找到对应 id 关窗并作废票据。
pub fn close_by_ticket(ticket: &str) {
    let id = panels().lock().ok().and_then(|m| m.iter().find(|(_, p)| p.ticket == ticket).map(|(k, _)| k.clone()));
    if let Some(id) = id { close(&id); return; }
    // 新协议窗口按票据关闭。
    let label = wins().lock().ok().and_then(|m| m.values().find(|x| x.ticket == ticket).map(|x| x.label.clone()));
    if let Some(label) = label {
        if let Ok(mut m) = wins().lock() {
            if let Some(w) = m.remove(&label) { crate::plugin_page::close(&w.ticket); let _ = w.win.close(); }
        }
        if let Ok(mut ix) = windex().lock() { ix.retain(|_, l| l != &label); }
    }
}

/// 列出声明了 islandPanel 且已启用的插件（旧，供 island 页决定是否开面板）。
pub fn list_panels() -> Result<Value, String> {
    dbg("list_panels enter");
    let context = crate::storage_context::current().ok_or("存储尚未就绪")?;
    let manager = crate::plugin_install::Manager { context };
    let mut out = Vec::new();
    for row in manager.list()? {
        if !row.enabled { continue; }
        if let Some(cfg) = row.manifest.island_panel.clone() {
            out.push(json!({ "id": row.manifest.id, "anchor": cfg.anchor, "width": cfg.width, "height": cfg.height }));
        }
    }
    dbg(&format!("list_panels => {} panels", out.len()));
    Ok(Value::Array(out))
}

/// 主岛收到 drop 时：按订阅协议通知订阅者（后端进程 + 已开窗口），再兼容旧面板。
pub fn forward_drop_all(paths: &[String]) {
    let subscribers: Vec<String> = drops().lock().map(|d| d.iter().filter(|(_, s)| s.contains_key("dock")).map(|(p, _)| p.clone()).collect()).unwrap_or_default();
    let payload = json!({ "event": "fileDrop", "phase": "drop", "paths": paths });
    // 通知已开的订阅窗口（页面侧 onEvent）。
    if let Ok(map) = wins().lock() {
        for w in map.values() {
            if subscribers.contains(&w.plugin) { let _ = w.win.emit("plugin.windowEvent", payload.clone()); }
        }
    }
    // 通知订阅插件的后端（Node SDK onEvent → 可 windows.open 等）：独立线程，避免阻塞拖放线程。
    if !subscribers.is_empty() {
        let subs = subscribers.clone();
        let pl = payload.clone();
        std::thread::spawn(move || {
            if let Ok(mut rt) = crate::plugin_runtime::CURRENT.lock() {
                for p in subs { let _ = rt.call(&p, "plugin.event", pl.clone()); }
            }
        });
    }
    // 旧自研面板兼容。
    if let Ok(map) = panels().lock() {
        for p in map.values() { let _ = p.win.emit("fish:island-drop", json!({ "paths": paths, "at": Value::Null })); }
    }
}

// ===================== 2.2.0 通用 Hook：windows / fileDrop =====================
static APP: OnceLock<AppHandle> = OnceLock::new();
pub fn init(app: &AppHandle) { let _ = APP.set(app.clone()); }
fn host() -> Result<&'static AppHandle, String> { APP.get().ok_or_else(|| "宿主尚未就绪".to_string()) }

struct Win { label: String, plugin: String, key: String, page: String, ticket: String, win: WebviewWindow }
fn wins() -> &'static Mutex<HashMap<String, Win>> {
    static W: OnceLock<Mutex<HashMap<String, Win>>> = OnceLock::new();
    W.get_or_init(|| Mutex::new(HashMap::new()))
}
fn windex() -> &'static Mutex<HashMap<(String, String), String>> {
    static I: OnceLock<Mutex<HashMap<(String, String), String>>> = OnceLock::new();
    I.get_or_init(|| Mutex::new(HashMap::new()))
}
/// plugin → (scope → handle)：拖放订阅（scope: dock | window）。
fn drops() -> &'static Mutex<HashMap<String, HashMap<String, String>>> {
    static D: OnceLock<Mutex<HashMap<String, HashMap<String, String>>>> = OnceLock::new();
    D.get_or_init(|| Mutex::new(HashMap::new()))
}

fn safe(s: &str) -> String {
    s.chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '_' }).collect()
}
fn clampf(v: f64, lo: f64, hi: f64) -> f64 { if !v.is_finite() { lo } else { v.max(lo).min(hi) } }
fn as_f(v: &Value, k: &str, def: f64) -> f64 { v.get(k).and_then(Value::as_f64).unwrap_or(def) }
fn need_cap(caps: &[String], cap: &str) -> Result<(), String> {
    if caps.iter().any(|c| c == cap) { Ok(()) } else { Err(format!("插件未声明 {cap} 能力")) }
}

/// 依 anchor + offset 计算屏幕物理坐标（本机 DPI 100% 时逻辑=物理；工作区原点近似 (0,0)）。
fn anchor_pos(app: &AppHandle, anchor: &str, ox: f64, oy: f64, w: f64, h: f64) -> (f64, f64) {
    let island = app.get_webview_window("island");
    let (ix, iy, iw, ih) = match island.and_then(|win| Some((win.outer_position().ok()?, win.outer_size().ok()?))) {
        Some((p, s)) => (p.x as f64, p.y as f64, s.width as f64, s.height as f64),
        None => (100.0, 100.0, 0.0, 0.0),
    };
    let (mut px, mut py) = match anchor {
        "dock-bottom" => (ix + (iw - w) / 2.0 + ox, iy + ih + 8.0 + oy),
        "screen" => (ox, oy),
        _ => (ix - w - 8.0 + ox, iy + (ih - h) / 2.0 + oy), // dock-left 默认
    };
    if !px.is_finite() { px = 0.0; }
    if !py.is_finite() { py = 0.0; }
    (px.max(0.0), py.max(0.0))
}

/// 在主线程创建 WebView2 窗口（同步命令/任意线程安全）：阻塞等主线程结果。
fn build_window(label: &str, plugin: &str, key: &str, page: &str, url: String, w: f64, h: f64, px: f64, py: f64, transparent: bool, ticket: String) -> Result<(), String> {
    let app = host()?.clone();
    let app_build = app.clone();
    let (tx, rx) = mpsc::channel::<Result<(), String>>();
    let label2 = label.to_string();
    let plugin2 = plugin.to_string();
    let key2 = key.to_string();
    let page2 = page.to_string();
    let ticket2 = ticket.clone();
    app.run_on_main_thread(move || {
        let parsed = match tauri::Url::parse(&url) { Ok(u) => u, Err(e) => { let _ = tx.send(Err(format!("面板地址无效：{e}"))); return; } };
        let built = WebviewWindowBuilder::new(&app_build, &label2, WebviewUrl::External(parsed))
            .title("FISH Dock")
            .position(px, py)
            .inner_size(w, h)
            .decorations(false)
            .resizable(true)
            .maximizable(false)
            .minimizable(false)
            .skip_taskbar(true)
            .shadow(false)
            .always_on_top(true)
            .visible_on_all_workspaces(true)
            .focused(false)
            .transparent(transparent)
            .visible(true)
            .build();
        match built {
            Ok(win) => {
                let lbl = label2.clone();
                let pl = plugin2.clone();
                let ky = key2.clone();
                let tk = ticket2.clone();
                let _ = win.on_window_event(move |ev| {
                    if matches!(ev, tauri::WindowEvent::Destroyed) {
                        crate::plugin_page::close(&tk);
                        if let Ok(mut m) = wins().lock() { m.remove(&lbl); }
                        if let Ok(mut ix) = windex().lock() { ix.remove(&(pl.clone(), ky.clone())); }
                    }
                });
                if let Ok(mut m) = wins().lock() {
                    m.insert(label2.clone(), Win { label: label2.clone(), plugin: plugin2.clone(), key: key2.clone(), page: page2.clone(), ticket: ticket2.clone(), win });
                }
                if let Ok(mut ix) = windex().lock() { ix.insert((plugin2.clone(), key2.clone()), label2.clone()); }
                let _ = tx.send(Ok(()));
            }
            Err(e) => { let _ = tx.send(Err(format!("面板窗口创建失败：{e}"))); }
        }
    }).map_err(|e| format!("主线程调度失败：{e}"))?;
    rx.recv().map_err(|_| "面板窗口创建无响应".to_string())?
}

fn window_open(plugin: &str, caps: &[String], params: &Value) -> Result<Value, String> {
    need_cap(caps, "windows")?;
    let app = host()?;
    let page = params.get("page").and_then(Value::as_str).ok_or("windows.open 缺少 page")?.to_string();
    let key = params.get("key").and_then(Value::as_str).map(str::to_string).unwrap_or_else(|| page.clone());
    if key.is_empty() || key.len() > 64 { return Err("key 长度无效".into()); }
    let context = crate::storage_context::current().ok_or("存储尚未就绪")?;
    let manager = crate::plugin_install::Manager { context };
    let row = manager.list()?.into_iter().find(|r| r.manifest.id == plugin && r.enabled).ok_or("插件未启用")?;
    if !row.manifest.pages.iter().any(|p| p.id == page) { return Err("page 未在 manifest.pages 注册".into()); }
    // 同 key 复用：前置已开窗口。
    {
        let existing = windex().lock().map_err(|_| "窗口锁不可用")?.get(&(plugin.to_string(), key.clone())).cloned();
        if let Some(label) = existing {
            if let Some(w) = wins().lock().map_err(|_| "窗口锁不可用")?.get(&label) { let _ = w.win.show(); return Ok(json!({ "handle": label })); }
        }
    }
    // 限额：每插件 ≤4，全局 ≤16。
    {
        let m = wins().lock().map_err(|_| "窗口锁不可用")?;
        if m.values().filter(|x| x.plugin == plugin).count() >= 4 { return Err("该插件窗口已达上限 4".into()); }
        if m.len() >= 16 { return Err("插件窗口全局已达上限 16".into()); }
    }
    let width = clampf(as_f(params, "width", 320.0), 160.0, 2400.0);
    let height = clampf(as_f(params, "height", 240.0), 80.0, 1600.0);
    let anchor = params.get("anchor").and_then(Value::as_str).unwrap_or("dock-left");
    let ox = clampf(as_f(params, "offsetX", 0.0), -20000.0, 20000.0);
    let oy = clampf(as_f(params, "offsetY", 0.0), -20000.0, 20000.0);
    let transparent = params.get("transparent").and_then(Value::as_bool).unwrap_or(false);
    let grant = crate::plugin_page::open(&format!("{plugin}/{page}"))?;
    let ticket = grant.get("ticket").and_then(Value::as_str).ok_or("票据缺失")?.to_string();
    let base_url = grant.get("url").and_then(Value::as_str).ok_or("地址缺失")?.to_string();
    // 把窗口 key 作为 wkey 传给页面，页面据此可 self-close（FishDock.windowSelf）。
    let url = format!("{base_url}{}wkey={}", if base_url.contains('?') { '&' } else { '?' }, safe(&key));
    let (px, py) = anchor_pos(app, anchor, ox, oy, width, height);
    let base = format!("fish-pw-{}-{}", safe(plugin), safe(&key));
    let mut label = base.clone();
    let mut n = 1;
    while wins().lock().map_err(|_| "窗口锁不可用")?.contains_key(&label) { label = format!("{base}-{n}"); n += 1; }
    build_window(&label, plugin, &key, &page, url, width, height, px, py, transparent, ticket)?;
    // anchor 跟随：dock-* 锚点时，窗口随主岛移动/换屏/DPI 重排，并向页面发 anchorChanged。
    if anchor.starts_with("dock") {
        let app3 = app.clone();
        let lbl = label.clone();
        let ank = anchor.to_string();
        let (w2, h2, ox2, oy2) = (width, height, ox, oy);
        std::thread::spawn(move || {
            let mut last = (i32::MIN, i32::MIN);
            loop {
                let win = match wins().lock() { Ok(m) => m.get(&lbl).map(|x| x.win.clone()), Err(_) => break };
                let Some(win) = win else { break };
                if let Some(isl) = app3.get_webview_window("island") {
                    if let (Ok(p), Ok(s)) = (isl.outer_position(), isl.outer_size()) {
                        let (ix, iy, iw, ih) = (p.x as f64, p.y as f64, s.width as f64, s.height as f64);
                        let (nx, ny) = match ank.as_str() {
                            "dock-bottom" => (ix + (iw - w2) / 2.0 + ox2, iy + ih + 8.0 + oy2),
                            _ => (ix - w2 - 8.0 + ox2, iy + (ih - h2) / 2.0 + oy2),
                        };
                        let (nx, ny) = (nx.max(0.0) as i32, ny.max(0.0) as i32);
                        if (nx, ny) != last {
                            last = (nx, ny);
                            let _ = win.set_position(PhysicalPosition::new(nx, ny));
                            let _ = win.emit("plugin.windowEvent", json!({ "event": "anchorChanged", "anchor": ank, "x": nx, "y": ny }));
                        }
                    }
                }
                std::thread::sleep(std::time::Duration::from_millis(80));
            }
        });
    }
    Ok(json!({ "handle": label }))
}

fn window_update(plugin: &str, caps: &[String], params: &Value) -> Result<Value, String> {
    need_cap(caps, "windows")?;
    let key = params.get("key").and_then(Value::as_str).ok_or("update 缺少 key")?;
    let label = windex().lock().map_err(|_| "窗口锁不可用")?.get(&(plugin.to_string(), key.to_string())).cloned().ok_or("窗口不存在")?;
    let win = wins().lock().map_err(|_| "窗口锁不可用")?.get(&label).map(|x| x.win.clone()).ok_or("窗口不存在")?;
    if let Some(x) = params.get("x").and_then(Value::as_i64) {
        let y = params.get("y").and_then(Value::as_i64).unwrap_or_else(|| win.outer_position().map(|p| p.y as i64).unwrap_or(0));
        let _ = win.set_position(PhysicalPosition::new(x as i32, y as i32));
    }
    if let Some(wd) = params.get("width").and_then(Value::as_f64) {
        let hd = params.get("height").and_then(Value::as_f64).unwrap_or_else(|| win.outer_size().map(|s| s.height as f64).unwrap_or(240.0));
        let _ = win.set_size(PhysicalSize::new(clampf(wd, 160.0, 2400.0) as u32, clampf(hd, 80.0, 1600.0) as u32));
    }
    Ok(json!({ "updated": true }))
}

fn window_close(plugin: &str, caps: &[String], params: &Value) -> Result<Value, String> {
    need_cap(caps, "windows")?;
    let key = params.get("key").and_then(Value::as_str).ok_or("close 缺少 key")?;
    let label = windex().lock().map_err(|_| "窗口锁不可用")?.get(&(plugin.to_string(), key.to_string())).cloned();
    if let Some(label) = label {
        if let Ok(mut m) = wins().lock() {
            if let Some(w) = m.remove(&label) { crate::plugin_page::close(&w.ticket); let _ = w.win.close(); }
        }
        if let Ok(mut ix) = windex().lock() { ix.remove(&(plugin.to_string(), key.to_string())); }
    }
    Ok(json!({ "closed": true }))
}

fn window_list(plugin: &str, caps: &[String]) -> Result<Value, String> {
    need_cap(caps, "windows")?;
    let m = wins().lock().map_err(|_| "窗口锁不可用")?;
    let out: Vec<Value> = m.values().filter(|x| x.plugin == plugin)
        .map(|x| json!({ "handle": x.label, "key": x.key, "page": x.page })).collect();
    Ok(Value::Array(out))
}

fn drop_subscribe(plugin: &str, caps: &[String], params: &Value) -> Result<Value, String> {
    need_cap(caps, "file-drop")?;
    let scope = params.get("scope").and_then(Value::as_str).unwrap_or("dock");
    if !["dock", "window"].contains(&scope) { return Err("scope 只能是 dock 或 window".into()); }
    let handle = params.get("handle").and_then(Value::as_str).unwrap_or("").to_string();
    let mut d = drops().lock().map_err(|_| "订阅锁不可用")?;
    // 主 Dock 拖入订阅独占（冲突报错）。
    if scope == "dock" {
        for (p, scopes) in d.iter() {
            if p != plugin && scopes.contains_key("dock") { return Err("主 Dock 拖入已被其它插件订阅".into()); }
        }
    }
    d.entry(plugin.to_string()).or_default().insert(scope.to_string(), handle);
    Ok(json!({ "subscribed": true }))
}

fn drop_unsubscribe(plugin: &str, params: &Value) -> Result<Value, String> {
    let mut d = drops().lock().map_err(|_| "订阅锁不可用")?;
    if let Some(scopes) = d.get_mut(plugin) {
        if let Some(scope) = params.get("scope").and_then(Value::as_str) { scopes.remove(scope); } else { scopes.clear(); }
    }
    Ok(json!({ "subscribed": false }))
}

/// 页面侧（bridge → dock.windows.* / dock.fileDrop.*）。
pub fn page_call(plugin: &str, caps: &[String], method: &str, params: &Value) -> Result<Value, String> {
    match method {
        "dock.windows.open" => window_open(plugin, caps, params),
        "dock.windows.update" => window_update(plugin, caps, params),
        "dock.windows.close" => window_close(plugin, caps, params),
        "dock.windows.list" => window_list(plugin, caps),
        "dock.fileDrop.subscribe" => drop_subscribe(plugin, caps, params),
        "dock.fileDrop.unsubscribe" => drop_unsubscribe(plugin, params),
        _ => Err("未知的窗口或拖放方法".into()),
    }
}

/// 后端（Node SDK → host.windows.* / host.fileDrop.*）。
pub fn backend_call(plugin: &str, caps: &[String], method: &str, params: &Value) -> Result<Value, String> {
    match method {
        "host.windows.open" => window_open(plugin, caps, params),
        "host.windows.update" => window_update(plugin, caps, params),
        "host.windows.close" => window_close(plugin, caps, params),
        "host.windows.list" => window_list(plugin, caps),
        "host.fileDrop.subscribe" => drop_subscribe(plugin, caps, params),
        "host.fileDrop.unsubscribe" => drop_unsubscribe(plugin, params),
        _ => Err("未知的宿主窗口方法".into()),
    }
}
