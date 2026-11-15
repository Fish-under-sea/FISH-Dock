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
