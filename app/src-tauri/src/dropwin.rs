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
// It is shown only while the left button is held over the pill (the page's
// pointer polling fills DRAG_RECT), sits under the cursor, accepts the drop and
// hands the paths to the page as `pet:drag-drop` (island.js adds them to the
// stash dot). Alpha 1 keeps it invisible but still hit-testable.
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

use tauri::AppHandle;

// The pill's physical rect on screen, while a drag is over it; None otherwise.
pub static DRAG_RECT: Mutex<Option<(i32, i32, i32, i32)>> = Mutex::new(None);
static STARTED: AtomicBool = AtomicBool::new(false);

pub fn note_drag(rect: Option<(i32, i32, i32, i32)>) {
    *DRAG_RECT.lock().unwrap() = rect;
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

    use serde_json::json;
    use tauri::{AppHandle, Emitter};

    use super::DRAG_RECT;

    // 与仓库内其他 Win32 模块（backplate.rs、systray.rs、desk.rs、inputs.rs 等）
    // 使用同一个 `Hwnd` 定义：窗口句柄是不透明指针，这里也按它们的写法声明，
    // 同一 crate 内的 user32 符号便不会触发 clashing_extern_declarations 警告。
    type Hwnd = *mut c_void;

    static APP: OnceLock<AppHandle> = OnceLock::new();

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

    extern "system" {
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
    }

    pub(super) fn spawn(app: AppHandle) {
        let _ = APP.set(app);
        std::thread::spawn(|| unsafe { run() });
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
        let hwnd = CreateWindowExW(
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
        let mut shown = false;
        loop {
            let mut msg: Msg = std::mem::zeroed();
            while PeekMessageW(&mut msg, std::ptr::null_mut(), 0, 0, 0x1) != 0 {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
            let down = (GetAsyncKeyState(0x01) as u16 & 0x8000) != 0;
            let rect = if down { *DRAG_RECT.lock().unwrap() } else { None };
            match (rect, shown) {
                (Some((x, y, w, hgt)), _) => {
                    let pad = 6;
                    MoveWindow(hwnd, x - pad, y - pad, w + 2 * pad, hgt + 2 * pad, 0);
                    // Stay the topmost window so the hit test lands here rather
                    // than on the (also topmost) transparent island window.
                    // HWND_TOPMOST=-1, SWP_NOSIZE|NOMOVE|NOACTIVATE|SHOWWINDOW.
                    SetWindowPos(hwnd, -1, 0, 0, 0, 0, 0x0001 | 0x0002 | 0x0010 | 0x0040);
                    if !shown {
                        ShowWindow(hwnd, 4); // SW_SHOWNOACTIVATE
                        shown = true;
                        log(&format!("dropwin: shown over pill {x},{y} {w}x{hgt}"));
                    }
                }
                (None, true) => {
                    ShowWindow(hwnd, 0); // SW_HIDE
                    shown = false;
                    log("dropwin: hidden");
                }
                (None, false) => {}
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
            for i in 0..count {
                let len = DragQueryFileW(hdrop, i, std::ptr::null_mut(), 0);
                if len == 0 {
                    continue;
                }
                let mut buf = vec![0u16; (len + 1) as usize];
                let got = DragQueryFileW(hdrop, i, buf.as_mut_ptr(), len + 1);
                if got > 0 {
                    paths.push(String::from_utf16_lossy(&buf[..got as usize]));
                }
            }
            DragFinish(hdrop);
            log(&format!("dropwin: WM_DROPFILES got {} path(s): {:?}", paths.len(), paths));
            if let Some(app) = APP.get() {
                let _ = app.emit_to("island", "pet:drag-drop", json!({ "paths": paths }));
                crate::island_panel::forward_drop_all(&paths);
            }
            ShowWindow(hwnd, 0);
            return 0;
        }
        DefWindowProcW(hwnd, msg, w, l)
    }
}
