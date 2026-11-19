// DEC-2：顶栏 + dock（第三种「家」）的布局几何（纯函数，可测）。
//
// 顶栏是屏幕顶上一条细条（高度 TB_H）：正中是她的胶囊/会话，左上角当前程序，右侧小组件、
// 系统监控、托盘、输入法、快速设置、时钟，右上角显示桌面。dock 在屏幕底部正中：开始与
// 程序、最小化窗口的缩略图、回收站，背后是亚克力。两者都在「家」所在的显示器上（home.rs
// 的 resolve），与 island 的锚点/anchorBase 兼容：顶栏正中胶囊的锚点即顶栏的中点向下展开。
//
// 本模块只算几何（给定显示器 mon、缩放 sf、窗口尺寸），供顶栏/dock 窗口摆放与命中面用；
// 渲染与交互由页面后续接。这样顶栏不是「另起一套接管任务栏」，而是同一显示变更路径下
// （main.rs ticker → home::on_displays_changed）的一种家。

/// 顶栏高度（逻辑像素）。
pub const TB_H: f64 = 34.0;
/// dock 的图标格与高度（逻辑像素）。
pub const DOCK_ICON: f64 = 44.0;
pub const DOCK_GAP: f64 = 8.0;
pub const DOCK_PAD: f64 = 10.0;

/// 一个物理像素矩形 (x, y, w, h)。
pub type Rect = (i32, i32, i32, i32);

/// 顶栏窗口矩形：铺满显示器宽、贴顶，高 = TB_H * sf（物理）。
pub fn topbar_rect(mon: (i32, i32, i32, i32), sf: f64) -> Rect {
    let h = (TB_H * sf).round() as i32;
    (mon.0, mon.1, mon.2, h)
}

/// dock 窗口矩形：居中在显示器底部，宽度按图标数算（至少一个图标宽），高 = dock 高。
pub fn dock_rect(mon: (i32, i32, i32, i32), sf: f64, icons: usize) -> Rect {
    let icon = (DOCK_ICON * sf).round() as i32;
    let gap = (DOCK_GAP * sf).round() as i32;
    let pad = (DOCK_PAD * sf).round() as i32;
    let h = icon + 2 * pad;
    let n = icons.max(1) as i32;
    let w = (n * icon + (n - 1) * gap + 2 * pad).min(mon.2.max(icon + 2 * pad));
    let x = mon.0 + (mon.2 - w) / 2;
    let y = mon.1 + mon.3 - h;
    (x, y, w, h)
}

/// 顶栏正中胶囊的锚点（anchorBase）：顶栏水平中点、底边，向下展开（与 island 顶部的
/// anchor 语义一致）。
pub fn center_anchor(mon: (i32, i32, i32, i32), sf: f64) -> (i32, i32) {
    let h = (TB_H * sf).round() as i32;
    (mon.0 + mon.2 / 2, mon.1 + h)
}

/// 一个点是否落在顶栏上（顶栏带命中面：拖上来固定、点开）。
pub fn in_topbar(mon: (i32, i32, i32, i32), sf: f64, x: i32, y: i32) -> bool {
    let (bx, by, bw, bh) = topbar_rect(mon, sf);
    x >= bx && x < bx + bw && y >= by && y < by + bh
}

/// 一个点是否落在 dock 上（dock 的命中面，用于「碰到 dock 就滑下去让开」的判断）。
pub fn in_dock(mon: (i32, i32, i32, i32), sf: f64, icons: usize, x: i32, y: i32) -> bool {
    let (bx, by, bw, bh) = dock_rect(mon, sf, icons);
    x >= bx && x < bx + bw && y >= by && y < by + bh
}

#[cfg(test)]
mod tests {
    use super::*;

    const MON: (i32, i32, i32, i32) = (0, 0, 1920, 1080);

    #[test]
    fn topbar_spans_the_top() {
        let (x, y, w, h) = topbar_rect(MON, 1.0);
        assert_eq!((x, y, w), (0, 0, 1920));
        assert_eq!(h, 34);
        // 缩放 150%：物理高按 sf 放大。
        assert_eq!(topbar_rect(MON, 1.5).3, 51);
    }

    #[test]
    fn dock_is_centered_at_bottom() {
        let (x, y, w, h) = dock_rect(MON, 1.0, 5);
        // 居中：左右留白相等。
        assert_eq!(x, MON.0 + (MON.2 - w) / 2);
        // 贴底：底边在显示器底。
        assert_eq!(y + h, MON.1 + MON.3);
        assert!(w <= MON.2);
    }

    #[test]
    fn center_anchor_is_top_middle() {
        assert_eq!(center_anchor(MON, 1.0), (960, 34));
    }

    #[test]
    fn hits() {
        assert!(in_topbar(MON, 1.0, 960, 10), "顶栏中点命中");
        assert!(!in_topbar(MON, 1.0, 960, 100), "顶栏下方不命中");
        let (dx, dy, dw, dh) = dock_rect(MON, 1.0, 3);
        assert!(in_dock(MON, 1.0, 3, dx + dw / 2, dy + dh / 2), "dock 中点命中");
        assert!(!in_dock(MON, 1.0, 3, 0, 0), "左上不命中 dock");
    }
}

// ---------------------------------------------------------------------------------
// 真正的顶栏 / dock 窗口（DEC-2）：home=topbar 时在「家」的显示器上创建并显示两个窗口：
//   label "topbar"：顶上细条（topbar_rect）；label "dock"：底部图标条（dock_rect）。
// island / taskbar 两种家：这两个窗口销毁，一行不改既有行为。显示变更 / DPI 变化按新几何
// 重摆（main.rs ticker 的 home::on_displays_changed 之后调 sync）。窗口用与 island 同一套
// builder（透明 / 无边框 / 置顶 / 不进任务栏 / 不抢焦点）。
use std::sync::Mutex;
use std::sync::Arc;
use tauri::{Emitter, Manager};
use crate::Shared;

struct Managed {
    label: &'static str,
    hwnd: isize,
}

static MANAGED: Mutex<Vec<Managed>> = Mutex::new(Vec::new());

fn has(label: &str) -> bool {
    MANAGED.lock().unwrap().iter().any(|m| m.label == label)
}

fn record(label: &'static str, hwnd: isize) {
    let mut m = MANAGED.lock().unwrap();
    if let Some(e) = m.iter_mut().find(|e| e.label == label) {
        e.hwnd = hwnd;
    } else {
        m.push(Managed { label, hwnd });
    }
}

/// 该不该显示顶栏 / dock：只有 home 是 topbar。
pub fn wanted(sh: &Shared) -> bool {
    sh.home() == "topbar"
}

/// dock 悬浮矩形：居中摆放在工作区底边之上（避开 Windows 原生任务栏），留 8px 呼吸缝隙。
fn dock_floating_rect(work: (i32, i32, i32, i32), sf: f64, icons: usize) -> Rect {
    let lift = (8.0 * sf).round() as i32;
    let (x, y, w, h) = dock_rect(work, sf, icons);
    (x, (y - lift).max(work.1), w, h)
}

/// 显示变更 / 家切换 / 启动后调：按当前的「家」创建或销毁顶栏与 dock 窗口，并按当前显示器
/// 与缩放重摆。幂等。
pub fn sync(sh: &Arc<Shared>) {
    if !wanted(sh) {
        close_one(sh, "topbar");
        close_one(sh, "dock");
        return;
    }
    let Some(area) = crate::island::home_area(sh) else { return };
    let sf = area.sf;
    let top = topbar_rect(area.mon, sf);
    let items = items_vec(sf);
    let icons = items.len().max(1);
    let work = (area.x, area.y, area.w, area.h);
    let dock = dock_floating_rect(work, sf, icons);
    ensure(sh, "topbar", "pet/topbar.html?mode=topbar", top, sf);
    ensure(sh, "dock", "pet/topbar.html?mode=dock", dock, sf);
    push_state_with(sh, items);
}

/// 顶栏 / dock 要展示的窗口（运行中的窗口），直接走 tasks::list 实时获取，不依赖接管任务栏。
fn items_vec(sf: f64) -> Vec<serde_json::Value> {
    let live = crate::taskbar::topbar_windows(sf);
    if !live.is_empty() {
        return live;
    }
    let w = crate::taskbar::windows_now();
    let mut items = Vec::new();
    if let Some(groups) = w.as_array() {
        for g in groups {
            if let Some(ws) = g.get("windows").and_then(|w| w.as_array()) {
                for win in ws {
                    let hwnd = win.get("id").and_then(|v| v.as_i64()).unwrap_or(0);
                    if hwnd == 0 {
                        continue;
                    }
                    items.push(serde_json::json!({
                        "hwnd": hwnd,
                        "title": win.get("title").and_then(|v| v.as_str()).unwrap_or(""),
                        "minimized": win.get("min").and_then(|v| v.as_bool()).unwrap_or(false),
                        "icon": win.get("png").and_then(|v| v.as_str()).unwrap_or(""),
                    }));
                }
            }
        }
    }
    items
}

fn push_state_with(sh: &Arc<Shared>, items: Vec<serde_json::Value>) {
    if let (Some(area), Some(dock_win)) = (crate::island::home_area(sh), sh.app.get_webview_window("dock")) {
        let work = (area.x, area.y, area.w, area.h);
        lay(&dock_win, dock_floating_rect(work, area.sf, items.len().max(1)));
    }
    let state = serde_json::json!({
        "windows": items,
        "clock": "",
    });
    for label in ["topbar", "dock"] {
        let _ = sh.app.emit_to(label, "topbar:state", state.clone());
    }
}

/// 给顶栏/dock 两个窗口推当前状态（窗口列表 + 时钟）。时钟留空由页面按 i18n 格式化。
pub fn push_state(sh: &Arc<Shared>) {
    let sf = crate::island::home_area(sh).map(|a| a.sf).unwrap_or(1.0);
    push_state_with(sh, items_vec(sf));
}

/// 创建一个窗口（不存在时），按 rect 摆放；存在则只重摆。
fn ensure(sh: &Arc<Shared>, label: &'static str, url: &str, rect: (i32, i32, i32, i32), sf: f64) {
    if let Some(win) = sh.app.get_webview_window(label) {
        lay(&win, rect);
        let _ = win.show();
        return;
    }
    if has(label) {
        return; // 正在创建
    }
    record(label, 0);
    let sh = sh.clone();
    let url = url.to_string();
    std::thread::spawn(move || {
        use tauri::{Manager, WebviewUrl, WebviewWindowBuilder};
        let built = WebviewWindowBuilder::new(&sh.app, label, WebviewUrl::App(url.into()))
            .title("FISH Dock top bar")
            .inner_size(rect.2 as f64 / sf, rect.3 as f64 / sf)
            .additional_browser_args(crate::browser_args(&sh))
            .data_directory(crate::webview_dir(&sh))
            .transparent(true)
            .decorations(false)
            .resizable(false)
            .maximizable(false)
            .minimizable(false)
            .skip_taskbar(true)
            .shadow(false)
            .always_on_top(true)
            .visible_on_all_workspaces(true)
            .focusable(false)
            .focused(false)
            .visible(false)
            .build();
        match built {
            Ok(win) => {
                lay(&win, rect);
                #[cfg(windows)]
                if let Ok(hwnd) = win.hwnd() {
                    record(label, hwnd.0 as isize);
                    crate::tasks::no_transitions(hwnd.0 as isize);
                }
                sh.own_window(&win);
                crate::power::webview_rest(&win, false);
                let _ = win.show();
                sh.log(&format!("topbar: {label} made at {rect:?}"));
                push_state(&sh);
            }
            Err(err) => sh.log(&format!("topbar: {label} not made: {err}")),
        }
    });
}

fn lay(win: &tauri::WebviewWindow, rect: (i32, i32, i32, i32)) {
    use tauri::{PhysicalPosition, PhysicalSize};
    let _ = win.set_position(PhysicalPosition::new(rect.0, rect.1));
    let _ = win.set_size(PhysicalSize::new(rect.2.max(1) as u32, rect.3.max(1) as u32));
    let _ = win.set_position(PhysicalPosition::new(rect.0, rect.1));
}

fn close_one(sh: &Shared, label: &'static str) {
    if let Some(win) = sh.app.get_webview_window(label) {
        let _ = win.destroy();
    }
    MANAGED.lock().unwrap().retain(|m| m.label != label);
}

// --- 命令（给顶栏/dock 页面调用）---------------------------------------------

// 点击顶栏缩略项 / dock 图标：把该窗口带到前台（Restore 先还原最小化）。复用 taskbar 的
// bring-to-front（SetForegroundWindow），这是 DEC-2 的点击决策落地。
#[tauri::command]
pub async fn topbar_focus(hwnd: i64, restore: bool) -> bool {
    let _ = restore;
    crate::taskbar::taskbar_window(hwnd, "front".to_string())
}

// 页面刚起来：推一次当前状态（窗口列表）。
#[tauri::command]
pub fn topbar_ready(app: tauri::AppHandle) {
    let sh = crate::shared(&app);
    push_state(&sh);
}

// 顶栏右侧入口：打开设置抽屉。
#[tauri::command]
pub fn topbar_settings(app: tauri::AppHandle) {
    let sh = crate::shared(&app);
    crate::island::open_settings(&sh, None);
}


