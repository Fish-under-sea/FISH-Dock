// A dedicated, plain (non-WebView2) window that alone receives file drops.
//
// Why it exists: her home's window is a *transparent* WebView2 window. During a
// drag the Windows hit test lands on WebView2's own render surface
// (Chrome_RenderWidgetHostHWND), which belongs to the WebView2 browser process
// and carries no OLE drop target, so neither wry's drop handler nor the page's
// `tauri://drag-*` ever hear of the drop (measured 2026-11-06: the island window
// becomes hittable while the button is held on the pill, and WindowFromPoint then
// returns that render surface — no drop target there). A plain popup window we
// own has no such child, so WM_DROPFILES reaches it.
//
// 2.2.0: it is a **generic receiver**, not a stash catcher. It only exists while
// a drag is over a place some plugin has asked for:
//   · the main Dock's pill, and only while that plugin holds the (exclusive)
//     Dock subscription — no subscriber, no capture at all;
//   · a plugin's own external window, and only for that window's owner and only
//     when the owner subscribed for it.
// Paths go to the owner alone: never broadcast, never into the log (counts and
// owners only), and only a real drop carries them (enter/leave do not).
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Condvar, Mutex};

static WAKE: Condvar = Condvar::new();
static WAKE_LOCK: Mutex<()> = Mutex::new(());

/// 订阅变化时唤醒接收线程；无订阅时线程阻塞，不轮询鼠标或预建 HWND。
pub fn changed() {
    let _guard = WAKE_LOCK.lock().unwrap_or_else(|p| p.into_inner());
    WAKE.notify_all();
}

use tauri::AppHandle;

/// The pill's physical rect on screen, while a drag is over it; None otherwise.
/// (island.rs fills this from the page's own hit testing, so the zone is the real
/// pill, not a made-up rectangle.)
pub static DRAG_RECT: Mutex<Option<(i32, i32, i32, i32)>> = Mutex::new(None);
static STARTED: AtomicBool = AtomicBool::new(false);

pub fn note_drag(rect: Option<(i32, i32, i32, i32)>) {
    *DRAG_RECT.lock().unwrap_or_else(|poison| poison.into_inner()) = rect;
}

/// Where a drop goes: the Dock's subscriber, or one plugin window's owner.
#[derive(Clone, Debug, PartialEq)]
pub enum Target {
    Dock(String),
    Window(String),
}

/// The drop zones that are live right now, in screen pixels. Pure so the routing
/// can be tested without Win32: order matters (the pill first, then windows).
pub fn zones(dock: Option<(&str, &(i32, i32, i32, i32))>, windows: &[(String, (i32, i32, i32, i32))]) -> Vec<(Target, (i32, i32, i32, i32))> {
    let mut zones = Vec::new();
    if let Some((handle, rect)) = dock {
        zones.push((Target::Dock(handle.into()), *rect));
    }
    for (handle, rect) in windows {
        zones.push((Target::Window(handle.clone()), *rect));
    }
    zones
}

/// The target under a point, topmost first: a plugin window over the pill wins,
/// because that is the window the user is dropping onto.
pub fn target_at(point: (i32, i32), zones: &[(Target, (i32, i32, i32, i32))]) -> Option<Target> {
    zones
        .iter()
        .rev()
        .find(|(_, rect)| point.0 >= rect.0 && point.1 >= rect.1 && point.0 < rect.0 + rect.2 && point.1 < rect.1 + rect.3)
        .map(|(target, _)| target.clone())
}

// 原生窗口栈从前到后；None 表示其他应用遮住落点。
fn target_in_stack(point: (i32,i32), zones: &[(Target,(i32,i32,i32,i32))], stack: &[(Option<Target>,(i32,i32,i32,i32))]) -> Option<Target> {
    let contains = |rect: &(i32,i32,i32,i32)| point.0>=rect.0 && point.1>=rect.1 && point.0<rect.0+rect.2 && point.1<rect.1+rect.3;
    let target = stack.iter().find(|(_,rect)|contains(rect))?.0.as_ref()?;
    zones.iter().find(|(candidate,rect)|candidate==target && contains(rect)).map(|(target,_)|target.clone())
}

/// The zones to watch now: the pill only while a plugin holds the Dock
/// subscription, plus every window its owner subscribed for.
fn live_zones() -> Vec<(Target, (i32, i32, i32, i32))> {
    let dock = crate::plugin_window::dock_subscription().and_then(|handle| {
        DRAG_RECT.lock().unwrap_or_else(|p| p.into_inner()).map(|rect| (handle, rect))
    });
    let windows = crate::plugin_window::drop_rects();
    zones(dock.as_ref().map(|(handle, rect)| (handle.as_str(), rect)), &windows)
}

pub fn start(app: AppHandle) {
    if STARTED.swap(true, Ordering::SeqCst) {
        return;
    }
    #[cfg(windows)]
    imp::spawn(app);
    #[cfg(not(windows))]
    let _ = app;
}

#[cfg(windows)]
mod imp {
    use std::ffi::c_void;
    use std::sync::OnceLock;

    use tauri::{AppHandle, Manager};

    use super::{live_zones, target_in_stack, Target};

    // 与仓库内其他 Win32 模块（backplate.rs、systray.rs、desk.rs、inputs.rs 等）
    // 使用同一个 `Hwnd` 定义：窗口句柄是不透明指针，这里也按它们的写法声明，
    // 同一 crate 内的 user32 符号便不会触发 clashing_extern_declarations 警告。
    type Hwnd = *mut c_void;

    static APP: OnceLock<AppHandle> = OnceLock::new();
    static ACTIVE: super::Mutex<Option<Target>> = super::Mutex::new(None);

    fn log(msg: &str) {
        if let Some(app) = APP.get() {
            crate::shared(app).log(msg);
        }
    }

    #[repr(C)]
    struct WndClassW {
        style: u32,
        lpfn_wnd_proc: Option<unsafe extern "system" fn(Hwnd, u32, usize, isize) -> isize>,
        cb_cls_extra: i32,
        cb_wnd_extra: i32,
        h_instance: isize,
        h_icon: isize,
        h_cursor: isize,
        hbr_background: isize,
        lpsz_menu_name: *const u16,
        lpsz_class_name: *const u16,
    }

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct Point {
        x: i32,
        y: i32,
    }

    #[repr(C)]
    struct Msg {
        hwnd: Hwnd,
        message: u32,
        w_param: usize,
        l_param: isize,
        time: u32,
        pt: Point,
        l_private: u32,
    }

    #[repr(C)]
    struct Rect { left:i32, top:i32, right:i32, bottom:i32 }

    extern "system" {
        fn GetTopWindow(hwnd:Hwnd)->Hwnd;
        fn GetWindow(hwnd:Hwnd, command:u32)->Hwnd;
        fn IsWindowVisible(hwnd:Hwnd)->i32;
        fn IsIconic(hwnd:Hwnd)->i32;
        fn GetWindowRect(hwnd:Hwnd, rect:*mut Rect)->i32;
        // 签名与仓库内其他 Win32 模块保持一致：同一个 user32 符号只有一种声明，
        // 不会互相冲突。窗口句柄用 `Hwnd`（即 *mut c_void），模块/菜单/实例
        // 句柄等按仓库惯例仍用 `isize`。
        fn GetModuleHandleW(name: *const u16) -> isize;
        fn RegisterClassW(class: *const WndClassW) -> u16;
        #[allow(clippy::too_many_arguments)]
        fn CreateWindowExW(
            ex_style: u32, class: *const u16, name: *const u16, style: u32,
            x: i32, y: i32, w: i32, h: i32,
            parent: Hwnd, menu: isize, instance: isize, param: *mut c_void,
        ) -> Hwnd;
        fn DefWindowProcW(hwnd: Hwnd, msg: u32, w: usize, l: isize) -> isize;
        fn DestroyWindow(hwnd: Hwnd) -> i32;
        fn SetLayeredWindowAttributes(hwnd: Hwnd, key: u32, alpha: u8, flags: u32) -> i32;
        fn DragAcceptFiles(hwnd: Hwnd, accept: i32);
        fn DragQueryFileW(hdrop: Hwnd, index: u32, buf: *mut u16, cch: u32) -> u32;
        fn DragFinish(hdrop: Hwnd);
        fn ShowWindow(hwnd: Hwnd, cmd: i32) -> i32;
        fn MoveWindow(hwnd: Hwnd, x: i32, y: i32, w: i32, h: i32, repaint: i32) -> i32;
        fn SetWindowPos(hwnd: Hwnd, after: isize, x: i32, y: i32, cx: i32, cy: i32, flags: u32) -> i32;
        fn PeekMessageW(msg: *mut Msg, hwnd: Hwnd, min: u32, max: u32, remove: u32) -> i32;
        fn TranslateMessage(msg: *const Msg) -> i32;
        fn DispatchMessageW(msg: *const Msg) -> isize;
        fn GetAsyncKeyState(vk: i32) -> i16;
        fn GetCursorPos(point: *mut Point) -> i32;
    }

    pub(super) fn spawn(app: AppHandle) {
        let _ = APP.set(app);
        std::thread::spawn(|| unsafe { run() });
    }

    /// What the receiver is currently over; keeps enter/leave events honest.
    fn hovering(now: Option<Target>, before: Option<Target>) {
        if now == before {
            return;
        }
        // 进入/离开不带路径：真正的 drop 才交付路径（规格 §6）。
        match before {
            Some(Target::Dock(handle)) => crate::plugin_window::leave_dock(&handle),
            Some(Target::Window(handle)) => crate::plugin_window::leave_window(&handle),
            None => {}
        }
        match now {
            Some(Target::Dock(handle)) => crate::plugin_window::enter_dock(&handle),
            Some(Target::Window(handle)) => crate::plugin_window::enter_window(&handle),
            None => {}
        }
    }

    unsafe fn native_target(point:(i32,i32), zones:&[(Target,(i32,i32,i32,i32))], receiver:Hwnd)->Option<Target> {
        if super::target_at(point,zones).is_none() { return None; }
        let app = APP.get()?;
        let mut owners = Vec::new();
        if let Some(token) = crate::plugin_window::dock_subscription() {
            if let Some(win) = app.get_webview_window("island") {
                if let Ok(hwnd) = win.hwnd() { owners.push((hwnd.0 as Hwnd,Target::Dock(token))); }
            }
        }
        for (token,label) in crate::plugin_window::drop_labels() {
            if let Some(win) = app.get_webview_window(&label) {
                if let Ok(hwnd) = win.hwnd() { owners.push((hwnd.0 as Hwnd,Target::Window(token))); }
            }
        }
        // 跳过接收器自己，按 Windows 的顶层 Z 序核对遮挡，而非 BOOK 的句柄顺序。
        let mut stack = Vec::new();
        let mut hwnd = GetTopWindow(std::ptr::null_mut());
        for _ in 0..4096 {
            if hwnd.is_null() { break; }
            if hwnd!=receiver && IsWindowVisible(hwnd)!=0 && IsIconic(hwnd)==0 {
                let mut rect = Rect { left:0,top:0,right:0,bottom:0 };
                if GetWindowRect(hwnd,&mut rect)!=0 {
                    let owner=owners.iter().find(|(candidate,_)|*candidate==hwnd).map(|(_,target)|target.clone());
                    stack.push((owner,(rect.left,rect.top,rect.right-rect.left,rect.bottom-rect.top)));
                }
            }
            hwnd=GetWindow(hwnd,2); // GW_HWNDNEXT
        }
        target_in_stack(point,zones,&stack)
    }

    unsafe fn run() {
        let class: Vec<u16> = "FISH_DOCK_DROP\0".encode_utf16().collect();
        let title: Vec<u16> = "FISH_DOCK_DROP\0".encode_utf16().collect();
        let instance = GetModuleHandleW(std::ptr::null());
        let wc = WndClassW {
            style: 0,
            lpfn_wnd_proc: Some(wndproc),
            cb_cls_extra: 0,
            cb_wnd_extra: 0,
            h_instance: instance,
            h_icon: 0,
            h_cursor: 0,
            hbr_background: 0,
            lpsz_menu_name: std::ptr::null(),
            lpsz_class_name: class.as_ptr(),
        };
        let _ = RegisterClassW(&wc);
        const WS_POPUP: u32 = 0x8000_0000;
        // layered | topmost | no-activate | toolwindow
        const EX: u32 = 0x0008_0000 | 0x0000_0008 | 0x0800_0000 | 0x0000_0080;
        let mut hwnd: Hwnd = std::ptr::null_mut();
        let mut shown = false;
        let mut over: Option<Target> = None;
        loop {
            if !crate::plugin_window::has_drop_subscribers() {
                if !hwnd.is_null() { DestroyWindow(hwnd); hwnd = std::ptr::null_mut(); }
                shown = false;
                hovering(None, over.take());
                *ACTIVE.lock().unwrap_or_else(|p|p.into_inner()) = None;
                let mut guard = super::WAKE_LOCK.lock().unwrap_or_else(|p|p.into_inner());
                while !crate::plugin_window::has_drop_subscribers() {
                    guard = super::WAKE.wait(guard).unwrap_or_else(|p|p.into_inner());
                }
            }
            if hwnd.is_null() {
                hwnd = CreateWindowExW(
            EX, class.as_ptr(), title.as_ptr(), WS_POPUP,
            0, 0, 10, 10, std::ptr::null_mut(), 0, instance, std::ptr::null_mut(),
        );
        if hwnd.is_null() {
            log("dropwin: could not create its window");
            return;
        }
        log("dropwin: window ready");
        // Nearly invisible, but a non-zero alpha keeps it hit-testable.
        SetLayeredWindowAttributes(hwnd, 0, 1, 0x2);
        DragAcceptFiles(hwnd, 1);
            }
            let mut msg: Msg = std::mem::zeroed();
            while PeekMessageW(&mut msg, std::ptr::null_mut(), 0, 0, 0x1) != 0 {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
            let down = (GetAsyncKeyState(0x01) as u16 & 0x8000) != 0;
            let mut cursor = Point { x: 0, y: 0 };
            let cursor = if GetCursorPos(&mut cursor) != 0 { Some((cursor.x, cursor.y)) } else { None };
            // 只有「按住不放 + 指针正落在某个已订阅的落点上」才启用接收窗口。
            let zones = if down { live_zones() } else { Vec::new() };
            let hit = cursor.and_then(|point| native_target(point, &zones, hwnd));
            match hit {
                Some(target) => {
                    let Some((x, y, w, hgt)) = zones.iter().find(|(candidate, _)| *candidate == target).map(|(_, rect)| *rect) else {
                        continue;
                    };
                    let pad = 0;
                    MoveWindow(hwnd, x - pad, y - pad, w + 2 * pad, hgt + 2 * pad, 0);
                    // Stay the topmost window so the hit test lands here rather
                    // than on the (also topmost) transparent island window.
                    // HWND_TOPMOST=-1, SWP_NOSIZE|NOMOVE|NOACTIVATE|SHOWWINDOW.
                    SetWindowPos(hwnd, -1, 0, 0, 0, 0, 0x0001 | 0x0002 | 0x0010 | 0x0040);
                    if !shown {
                        ShowWindow(hwnd, 4); // SW_SHOWNOACTIVATE
                        shown = true;
                    }
                    hovering(Some(target.clone()), over.clone());
                    over = Some(target.clone());
                    *ACTIVE.lock().unwrap_or_else(|p|p.into_inner()) = Some(target);
                }
                None => {
                    // 离开落点或松开左键：命中区立刻撤掉（在途拖入随之作废）。
                    if shown {
                        ShowWindow(hwnd, 0); // SW_HIDE
                        shown = false;
                    }
                    if over.is_some() {
                        hovering(None, over.clone());
                        over = None;
                        *ACTIVE.lock().unwrap_or_else(|p|p.into_inner()) = None;
                    }
                }
            }
            std::thread::sleep(std::time::Duration::from_millis(40));
        }
    }

    pub(super) unsafe extern "system" fn wndproc(hwnd: Hwnd, msg: u32, w: usize, l: isize) -> isize {
        const WM_DROPFILES: u32 = 0x0233;
        if msg == WM_DROPFILES {
            let hdrop = w as Hwnd;
            let count = DragQueryFileW(hdrop, 0xFFFF_FFFF, std::ptr::null_mut(), 0);
            let mut paths: Vec<String> = Vec::new();
            let mut used = 0;
            let mut rejected = count as usize > crate::plugin_window::MAX_DROP_FILES;
            if !rejected {
                for i in 0..count {
                    let len = DragQueryFileW(hdrop, i, std::ptr::null_mut(), 0);
                    if len==0 { rejected=true; break; }
                    match crate::plugin_window::drop_budget(used,len as usize) {
                        Ok(total)=>used=total,
                        Err(_)=>{rejected=true;break;}
                    }
                    let mut buf = vec![0u16; len as usize + 1];
                    let got = DragQueryFileW(hdrop, i, buf.as_mut_ptr(), len + 1);
                    if got!=len { rejected=true;break; }
                    paths.push(String::from_utf16_lossy(&buf[..got as usize]));
                }
            }
            DragFinish(hdrop);
            if rejected {
                ACTIVE.lock().unwrap_or_else(|p|p.into_inner()).take();
                log("dropwin: file batch exceeds protocol limit or could not be read; not delivered");
                ShowWindow(hwnd,0);
                return 0;
            }
            // 只按落点路由给拥有者：主 Dock 的独占订阅者，或这个窗口的所有者。
            // 没有匹配的落点（窗口刚关、订阅刚撤）就丢弃，绝不转给别人。
            let mut cursor = Point { x: 0, y: 0 };
            let point = if GetCursorPos(&mut cursor) != 0 { Some((cursor.x, cursor.y)) } else { None };
            let active = ACTIVE.lock().unwrap_or_else(|p|p.into_inner()).take();
            let target = point.and_then(|point| native_target(point, &live_zones(), hwnd)).filter(|target| Some(target) == active.as_ref());
            match target {
                Some(Target::Dock(handle)) => {
                    if let Err(error) = crate::plugin_window::deliver_dock(&handle, &paths) {
                        log(&format!("dropwin: {} path(s) dropped: {error}", paths.len()));
                    }
                }
                Some(Target::Window(handle)) => {
                    if let Err(error) = crate::plugin_window::deliver_window(&handle, &paths) {
                        log(&format!("dropwin: {} path(s) dropped: {error}", paths.len()));
                    }
                }
                None => log(&format!("dropwin: {} path(s) with no live drop target", paths.len())),
            }
            ShowWindow(hwnd, 0);
            return 0;
        }
        DefWindowProcW(hwnd, msg, w, l)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // 落点路由：只有指针真正在矩形里才算，且插件窗口盖在主岛上面时归窗口。
    #[test]
    fn a_point_routes_to_the_topmost_live_zone() {
        let dock = (100, 0, 300, 40);
        let windows = vec![("handle-a".to_string(), (200, 10, 200, 200)), ("handle-b".to_string(), (900, 300, 100, 100))];
        let zones = zones(Some(("dock-sub", &dock)), &windows);
        assert_eq!(target_at((150, 10), &zones), Some(Target::Dock("dock-sub".into())), "胶囊上的点归主 Dock 订阅者");
        assert_eq!(target_at((250, 30), &zones), Some(Target::Window("handle-a".into())), "插件窗口盖在胶囊上时归那个窗口");
        assert_eq!(target_at((950, 350), &zones), Some(Target::Window("handle-b".into())));
        assert_eq!(target_at((0, 0), &zones), None, "落点之外不捕获");
    }

    #[test]
    fn native_stack_chooses_visible_owner_and_blocks_occluded_zones() {
        let rect = (0,0,100,100);
        let zones = zones(None, &[("a".into(),rect),("b".into(),rect)]);
        let stack = vec![(Some(Target::Window("a".into())),rect),(Some(Target::Window("b".into())),rect)];
        assert_eq!(target_in_stack((10,10), &zones, &stack), Some(Target::Window("a".into())), "按真实层级，不按句柄排序");
        let occluded = vec![(None,rect),(Some(Target::Window("a".into())),rect)];
        assert_eq!(target_in_stack((10,10), &zones, &occluded), None, "其他应用遮住落点时不得截获");
    }

    // 没有订阅者时 dropwin 一个落点都没有：不捕获、不出现「看不见的接收窗口」。
    #[test]
    fn no_subscriber_means_no_zone_at_all() {
        assert!(zones(None, &[]).is_empty());
        assert_eq!(target_at((150, 10), &zones(None, &[])), None);
        // 主岛有区域、但没有任何插件订阅：传进来的 dock 就是 None（live_zones 的判据）。
        let only_windows = zones(None, &[("h".to_string(), (0, 0, 10, 10))]);
        assert_eq!(target_at((150, 10), &only_windows), None);
    }

    // 落点边界是半开区间：右下角那一像素已经不算，避免「差一像素」把文件交给隔壁。
    #[test]
    fn zone_edges_are_half_open() {
        let zones = zones(Some(("dock-sub", &(10, 10, 100, 40))), &[]);
        assert_eq!(target_at((10, 10), &zones), Some(Target::Dock("dock-sub".into())));
        assert_eq!(target_at((109, 49), &zones), Some(Target::Dock("dock-sub".into())));
        assert_eq!(target_at((110, 49), &zones), None);
        assert_eq!(target_at((109, 50), &zones), None);
    }
}
