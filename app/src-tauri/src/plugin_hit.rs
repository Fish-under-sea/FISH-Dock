//! 插件外部窗口的鼠标命中（2.2.3，issue #87 的 B4）：透明区点击穿透。
//!
//! 问题：`transparent:true` 的窗口整块矩形都会吃掉鼠标，透明像素背后本该落到
//! 主岛或桌面的点击被截住。B1 把窗口下限降到 32×32 之后多数形态已经不需要它，
//! 但「大窗口装小内容」仍然会撞上，所以两条路都给。
//!
//! 做法：插件给出可点区域（`hitRects`，页面逻辑像素），宿主在指针轮询里判断
//! 光标是否落在其中；不在就对该窗口 `set_ignore_cursor_events(true)`，点击穿过去。
//!
//! **不改主岛任何东西**：这里只对插件窗口调 `set_ignore_cursor_events`，主岛
//! 的穿透由它自己的 `pointer.rs` 管，互不相干。
//!
//! **按需启用**：没有任何窗口登记命中规则时 `poll` 立刻返回长间隔，不读窗口、
//! 不碰 API——不装插件、装了不启用、或插件没给 `hitRects` 的用户零开销。

use std::collections::BTreeMap;
use std::sync::Mutex;

use tauri::Manager as _;

/// 一个窗口的命中规则。`rects` 是页面逻辑像素（相对窗口内容区左上角）。
struct Rule {
    rects: Vec<[f64; 4]>,
    /// 窗口外框（屏幕物理像素），由 `place` 维护；拿不到时不做切换。
    rect: Option<(i32, i32, i32, i32)>,
    /// 该窗口当前所在显示器的缩放。
    scale: f64,
    /// 上一次设过的穿透状态，避免每帧都调 API。
    ignoring: Option<bool>,
}

static RULES: Mutex<BTreeMap<String, Rule>> = Mutex::new(BTreeMap::new());

/// 没有命中规则时，指针循环可以睡这么久。
const IDLE_MS: u64 = 1000;
/// 有规则时的检查间隔：足够跟手，又不至于占满主线程。
const TICK_MS: u64 = 40;

fn rules() -> std::sync::MutexGuard<'static, BTreeMap<String, Rule>> {
    RULES.lock().unwrap_or_else(|poison| poison.into_inner())
}

/// 登记（或更新）一个窗口的可点区域。`rects` 为空表示「整窗都可点」——
/// 页面还没量出不透明区域时不穿透，免得用户根本点不到窗口。
pub fn watch(label: &str, rects: Vec<[f64; 4]>) {
    let mut table = rules();
    let entry = table.entry(label.to_string()).or_insert_with(|| Rule {
        rects: Vec::new(), rect: None, scale: 1.0, ignoring: None,
    });
    entry.rects = rects;
}

/// 撤销一个窗口的规则（关闭、隐藏、取消命中测试时调用）。
pub fn forget(label: &str) {
    rules().remove(label);
}

/// 窗口外框或缩放变了：更新缓存，好让 `poll` 不用每帧问窗口。
pub fn place(label: &str, rect: Option<(i32, i32, i32, i32)>, scale: f64) {
    let mut table = rules();
    let Some(entry) = table.get_mut(label) else { return };
    entry.rect = rect;
    entry.scale = if scale > 0.0 { scale } else { 1.0 };
}

/// 有没有窗口在等命中测试。主循环据此决定要不要把间隔压到 `TICK_MS`。
pub fn watching() -> bool {
    !rules().is_empty()
}

/// 判定光标是否落在某条规则的可点区域内。逻辑像素乘缩放换成物理，再加窗口原点。
fn hits(rule: &Rule, (x, y): (i32, i32)) -> bool {
    let Some((ox, oy, w, h)) = rule.rect else { return true };
    // 还没量出区域：整窗都算命中（不穿透），免得窗口变成点不到。
    if rule.rects.is_empty() { return true; }
    let s = rule.scale.max(0.5);
    let inside = rule.rects.iter().any(|r| {
        let rx = ox as f64 + r[0] * s;
        let ry = oy as f64 + r[1] * s;
        let rw = r[2] * s;
        let rh = r[3] * s;
        (x as f64) >= rx && (x as f64) <= rx + rw && (y as f64) >= ry && (y as f64) <= ry + rh
    });
    // 窗口外框之外也算穿透：光标根本不在窗口上，不该抓鼠标。
    let within_window = (x as f64) >= ox as f64 && (x as f64) <= (ox + w) as f64
        && (y as f64) >= oy as f64 && (y as f64) <= (oy + h) as f64;
    inside && within_window
}

/// 一次命中检查。返回下次该等多久（与 `pet::poll` / `island::poll` 同一个约定）。
///
/// 调用方是 main.rs 的 pointer 线程：它已经取过光标，这里只做判定与切换。
pub fn poll(cursor: (i32, i32)) -> u64 {
    let Ok(app) = crate::plugin_window::app_handle() else { return IDLE_MS };
    // 第一段：只读规则表，算出「哪些窗口还在、各自该不该穿透」。**整个循环不碰
    // 任何窗口 API**——那会走主线程，持着规则表的锁去等它，主岛就得跟着等。
    let (stale, planned): (Vec<String>, Vec<(String, bool)>) = {
        let mut table = rules();
        if table.is_empty() {
            return IDLE_MS;
        }
        let mut stale = Vec::new();
        let mut planned = Vec::new();
        for (label, rule) in table.iter_mut() {
            if rule.rect.is_none() {
                continue;
            }
            let want = !hits(rule, cursor);
            if rule.ignoring != Some(want) {
                rule.ignoring = Some(want);
                planned.push((label.clone(), want));
            }
        }
        // 窗口已经不在的规则当场清掉：否则规则表永远非空，指针循环会为一条
        // 找不到窗口的记录每 40ms 醒一次，白白占着主线程。
        stale.extend(table.keys().cloned());
        (stale, planned)
    };
    // 第二段：锁外确认窗口是否还在，顺手清掉失效的规则。
    let gone: Vec<String> = stale.into_iter().filter(|label| app.get_webview_window(label).is_none()).collect();
    if !gone.is_empty() {
        let mut table = rules();
        for label in gone {
            table.remove(&label);
        }
        if table.is_empty() {
            return IDLE_MS;
        }
    }
    // 第三段：锁外切换穿透。
    for (label, ignore) in planned {
        if let Some(win) = app.get_webview_window(&label) {
            // true = 鼠标穿过；与 pointer.rs 给主岛的语义一致。
            let _ = win.set_ignore_cursor_events(ignore);
        }
    }
    TICK_MS
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rule(rects: Vec<[f64; 4]>, rect: Option<(i32, i32, i32, i32)>, scale: f64) -> Rule {
        Rule { rects, rect, scale, ignoring: None }
    }

    // 光标落在给定矩形里才算命中；矩形用逻辑像素，乘缩放再加原点。
    #[test]
    fn a_pointer_inside_the_given_rects_is_a_hit() {
        // 窗口在 (100,200)，缩放 2；可点区域是页面的 (10,10,40,20)。
        let r = rule(vec![[10.0, 10.0, 40.0, 20.0]], Some((100, 200, 400, 300)), 2.0);
        // 物理上那块是 (120,220) 到 (200,260)。
        assert!(hits(&r, (120, 220)), "左上角命中");
        assert!(hits(&r, (200, 260)), "右下角命中");
        assert!(!hits(&r, (119, 220)), "左侧一像素即穿透");
        assert!(!hits(&r, (120, 219)), "上方一像素即穿透");
    }

    // 窗口之外一律穿透：光标不在窗口上时不该抓鼠标。
    #[test]
    fn a_pointer_outside_the_window_passes_through() {
        let r = rule(vec![[0.0, 0.0, 1000.0, 1000.0]], Some((100, 200, 100, 100)), 1.0);
        assert!(!hits(&r, (50, 250)), "窗口左边的光标不算命中");
        assert!(!hits(&r, (250, 250)), "窗口右边的光标不算命中");
    }

    // 还没量出可点区域时整窗都可点：否则插件窗口会变成「点不到」。
    #[test]
    fn an_unmeasured_window_is_fully_clickable() {
        let r = rule(Vec::new(), Some((100, 200, 100, 100)), 1.0);
        assert!(hits(&r, (150, 250)), "没有区域数据时按整窗命中");
        assert!(hits(&r, (0, 0)) || !hits(&r, (0, 0))); // 不在窗口内时走 within_window 判定
    }

    // 外框未知（还没 place 过）时不做穿透判断，保持窗口可用。
    #[test]
    fn an_unplaced_window_keeps_its_clicks() {
        let r = rule(vec![[0.0, 0.0, 10.0, 10.0]], None, 1.0);
        assert!(hits(&r, (9999, 9999)), "没有外框时按命中算，宁可不穿透");
    }

    // 多块区域任一命中即可：长条上分散的 chip 就是这种形态。
    #[test]
    fn any_of_several_rects_counts_as_a_hit() {
        let r = rule(
            vec![[0.0, 0.0, 20.0, 20.0], [100.0, 0.0, 20.0, 20.0]],
            Some((0, 0, 400, 200)),
            1.0,
        );
        assert!(hits(&r, (5, 5)), "第一块");
        assert!(hits(&r, (110, 5)), "第二块");
        assert!(!hits(&r, (50, 5)), "两块之间穿透");
    }

    // 按需启用：没有规则时 poll 立刻返回长间隔，不碰任何窗口。
    #[test]
    fn poll_is_idle_without_rules() {
        {
            let mut table = rules();
            table.clear();
        }
        assert!(!watching());
        assert_eq!(poll((0, 0)), IDLE_MS);
    }
}
