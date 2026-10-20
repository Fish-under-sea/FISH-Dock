// Windows' side of crash reports: the unhandled-exception filter, a stack
// as module+offset, the box the guard asks in, WebView2's ProcessFailed,
// and what Windows says of itself (its version, WebView2's; its proxy is
// proxy.rs's).
use std::ffi::c_void;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, OnceLock};

use crate::Shared;

#[repr(C)]
struct ExceptionRecord {
    code: u32,
    flags: u32,
    record: *mut ExceptionRecord,
    address: *mut c_void,
    count: u32,
    info: [usize; 15],
}

#[repr(C)]
struct ExceptionPointers {
    record: *mut ExceptionRecord,
    context: *mut c_void,
}

type Filter = unsafe extern "system" fn(*mut ExceptionPointers) -> i32;

#[link(name = "kernel32")]
extern "system" {
    fn SetUnhandledExceptionFilter(filter: Option<Filter>) -> Option<Filter>;
    fn RtlCaptureStackBackTrace(skip: u32, count: u32, frames: *mut *mut c_void, hash: *mut u32) -> u16;
    fn GetModuleHandleExW(flags: u32, name: *const u16, module: *mut isize) -> i32;
    fn GetModuleFileNameW(module: isize, name: *mut u16, size: u32) -> u32;
    fn CreateFileW(name: *const u16, access: u32, share: u32, security: *mut c_void, disposition: u32, flags: u32, template: isize) -> isize;
    fn WriteFile(file: isize, buf: *const u8, len: u32, written: *mut u32, overlapped: *mut c_void) -> i32;
    fn CloseHandle(handle: isize) -> i32;
    fn GetCurrentThreadId() -> u32;
    fn GetCurrentProcess() -> isize;
    fn TerminateProcess(process: isize, code: u32) -> i32;
}

#[link(name = "user32")]
extern "system" {
    fn MessageBoxW(hwnd: isize, text: *const u16, caption: *const u16, kind: u32) -> i32;
}

const FROM_ADDRESS: u32 = 0x4;
const UNCHANGED_REFCOUNT: u32 = 0x2;
const GENERIC_WRITE: u32 = 0x4000_0000;
const CREATE_ALWAYS: u32 = 2;
const FILE_ATTRIBUTE_NORMAL: u32 = 0x80;
const INVALID_HANDLE: isize = -1;
const ACCESS_VIOLATION: u32 = 0xC000_0005;

// Where the filter writes, made before (it allocates nothing); when she started.
static NATIVE_FILE: OnceLock<Vec<u16>> = OnceLock::new();
static STARTED: AtomicU64 = AtomicU64::new(0);
static WRITING: AtomicBool = AtomicBool::new(false);

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

// Text put together without allocating: the heap may be what broke.
struct Buf {
    b: [u8; 3072],
    n: usize,
}

impl Buf {
    fn new() -> Self {
        Buf { b: [0; 3072], n: 0 }
    }

    fn text(&mut self, s: &str) {
        for &c in s.as_bytes() {
            if self.n < self.b.len() {
                self.b[self.n] = c;
                self.n += 1;
            }
        }
    }

    fn hex(&mut self, v: u64, min: usize) {
        let mut digits = [0u8; 16];
        let mut len = 0;
        let mut v = v;
        while v > 0 || len < min.max(1) {
            digits[len] = b"0123456789abcdef"[(v & 0xf) as usize];
            v >>= 4;
            len += 1;
            if len == 16 {
                break;
            }
        }
        for i in (0..len).rev() {
            self.text(std::str::from_utf8(&digits[i..i + 1]).unwrap_or("0"));
        }
    }

    fn dec(&mut self, v: u64) {
        let mut digits = [0u8; 20];
        let mut len = 0;
        let mut v = v;
        loop {
            digits[len] = b'0' + (v % 10) as u8;
            v /= 10;
            len += 1;
            if v == 0 || len == 20 {
                break;
            }
        }
        for i in (0..len).rev() {
            self.text(std::str::from_utf8(&digits[i..i + 1]).unwrap_or("0"));
        }
    }

    // An address as the module it is in and how far into it: ntdll.dll+0x1a2b.
    fn place(&mut self, addr: usize) {
        let mut module = 0isize;
        // SAFETY: asks which module holds an address; nothing is loaded or kept.
        let ok = unsafe { GetModuleHandleExW(FROM_ADDRESS | UNCHANGED_REFCOUNT, addr as *const u16, &mut module) } != 0;
        if !ok || module == 0 {
            self.text("unknown+0x");
            self.hex(addr as u64, 1);
            return;
        }
        let mut name = [0u16; 260];
        // SAFETY: our own buffer, its size passed along.
        let len = unsafe { GetModuleFileNameW(module, name.as_mut_ptr(), 260) } as usize;
        let name = &name[..len.min(260)];
        let start = name.iter().rposition(|&c| c == u16::from(b'\\')).map_or(0, |i| i + 1);
        let mut put = 0;
        for &c in &name[start..] {
            if put == 64 {
                break;
            }
            let c = u8::try_from(c).ok().filter(|c| c.is_ascii_alphanumeric() || b"_.-".contains(c)).unwrap_or(b'_');
            self.text(std::str::from_utf8(&[c]).unwrap_or("_"));
            put += 1;
        }
        if put == 0 {
            self.text("unknown");
        }
        self.text("+0x");
        self.hex((addr - module as usize) as u64, 1);
    }

    fn as_str(&self) -> &str {
        std::str::from_utf8(&self.b[..self.n]).unwrap_or("")
    }
}

// Her stack as module+offset, the newest first (at most 32; `skip` of them
// left out at the top).
pub fn frames(skip: u32) -> Vec<String> {
    let mut frames = [std::ptr::null_mut::<c_void>(); 32];
    // SAFETY: our own array, its length passed along.
    let n = unsafe { RtlCaptureStackBackTrace(skip, 32, frames.as_mut_ptr(), std::ptr::null_mut()) } as usize;
    frames[..n.min(32)]
        .iter()
        .map(|&f| {
            let mut b = Buf::new();
            b.place(f as usize);
            b.as_str().to_string()
        })
        .collect()
}

// Called by Windows for an exception nobody handled, on the thread it
// happened on, before she is ended: what and where, written down. Nothing
// allocated, nothing locked; once.
unsafe extern "system" fn on_crash(pointers: *mut ExceptionPointers) -> i32 {
    if WRITING.swap(true, Ordering::SeqCst) {
        return 0;
    }
    let Some(path) = NATIVE_FILE.get() else { return 0 };
    let mut out = Buf::new();
    // SAFETY: Windows hands the exception's pointers, alive for this call.
    let record = if pointers.is_null() { std::ptr::null_mut() } else { unsafe { (*pointers).record } };
    if !record.is_null() {
        // SAFETY: as above.
        let r = unsafe { &*record };
        out.text("code=");
        out.hex(u64::from(r.code), 8);
        out.text("\nwhere=");
        out.place(r.address as usize);
        out.text("\n");
        if r.code == ACCESS_VIOLATION && r.count >= 2 {
            out.text("access=");
            out.text(match r.info[0] {
                0 => "read",
                1 => "write",
                8 => "execute",
                _ => "other",
            });
            if r.info[1] < 0x10000 {
                out.text(" null+0x");
                out.hex(r.info[1] as u64, 1);
            }
            out.text("\n");
        }
    }
    out.text("started=");
    out.dec(STARTED.load(Ordering::SeqCst));
    out.text("\nthread=");
    // SAFETY: no arguments.
    out.dec(u64::from(unsafe { GetCurrentThreadId() }));
    out.text("\n");
    let mut frames = [std::ptr::null_mut::<c_void>(); 32];
    // SAFETY: our own array, its length passed along.
    let n = unsafe { RtlCaptureStackBackTrace(0, 32, frames.as_mut_ptr(), std::ptr::null_mut()) } as usize;
    for &f in &frames[..n.min(32)] {
        out.text("frame=");
        out.place(f as usize);
        out.text("\n");
    }
    // SAFETY: a path made before, null-terminated; our own buffer written.
    unsafe {
        let file = CreateFileW(path.as_ptr(), GENERIC_WRITE, 0, std::ptr::null_mut(), CREATE_ALWAYS, FILE_ATTRIBUTE_NORMAL, 0);
        if file != INVALID_HANDLE && file != 0 {
            let mut written = 0u32;
            WriteFile(file, out.b.as_ptr(), out.n as u32, &mut written, std::ptr::null_mut());
            CloseHandle(file);
        }
    }
    // Ended now, her exit code the exception's: left to Windows, its error
    // reporting kept her (and the taskbar she held, which the guard gives
    // back once she is gone) eight or nine seconds more for a dump nobody
    // reads (2026-10-11). No WER entry in the event log for it.
    if !record.is_null() {
        // SAFETY: as above; this process's own pseudo-handle.
        unsafe { TerminateProcess(GetCurrentProcess(), (*record).code) };
    }
    0
}

// The filter set, writing to `file`; `started`, her start (ms).
pub fn catch_native(file: &Path, started: u64) {
    STARTED.store(started, Ordering::SeqCst);
    if NATIVE_FILE.set(wide(&file.to_string_lossy())).is_ok() {
        // SAFETY: a function of ours that lives as long as the process.
        unsafe { SetUnhandledExceptionFilter(Some(on_crash)) };
    }
}

// Ended at once with `code`, nothing written (a crash test).
pub fn end_with(code: u32) {
    // SAFETY: this process's own pseudo-handle.
    unsafe { TerminateProcess(GetCurrentProcess(), code) };
}

// Windows' own Yes/No box, on top: Yes or not.
pub fn ask(title: &str, text: &str) -> bool {
    const MB_YESNO: u32 = 0x4;
    const MB_ICONWARNING: u32 = 0x30;
    const MB_SETFOREGROUND: u32 = 0x1_0000;
    const MB_TOPMOST: u32 = 0x4_0000;
    const IDYES: i32 = 6;
    // SAFETY: two strings of our own, null-terminated.
    unsafe { MessageBoxW(0, wide(text).as_ptr(), wide(title).as_ptr(), MB_YESNO | MB_ICONWARNING | MB_SETFOREGROUND | MB_TOPMOST) == IDYES }
}

// "Windows 11 25H2 (26200.6584) x64": its build, from the registry (its
// ProductName says 10 on 11 too).
pub fn os() -> String {
    use winreg::enums::HKEY_LOCAL_MACHINE;
    use winreg::RegKey;
    let arch = match std::env::consts::ARCH {
        "x86_64" => "x64",
        "aarch64" => "arm64",
        other => other,
    };
    let Ok(key) = RegKey::predef(HKEY_LOCAL_MACHINE).open_subkey(r"SOFTWARE\Microsoft\Windows NT\CurrentVersion") else {
        return format!("Windows {arch}");
    };
    let build: String = key.get_value("CurrentBuild").unwrap_or_default();
    let ubr: Option<u32> = key.get_value("UBR").ok();
    let display: String = key.get_value("DisplayVersion").unwrap_or_default();
    let major = if build.parse::<u32>().is_ok_and(|b| b >= 22000) { "11" } else { "10" };
    let build = match ubr {
        Some(ubr) => format!("{build}.{ubr}"),
        None => build,
    };
    let display = if display.is_empty() { String::new() } else { format!(" {display}") };
    clean(&format!("Windows {major}{display} ({build}) {arch}"))
}

fn clean(s: &str) -> String {
    s.chars().filter(|c| c.is_ascii_alphanumeric() || " .()-_".contains(*c)).take(80).collect()
}

// WebView2's version, as EdgeUpdate keeps it.
pub fn webview_version() -> Option<String> {
    use winreg::enums::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE};
    use winreg::RegKey;
    const CLIENT: &str = r"Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}";
    let places = [(HKEY_LOCAL_MACHINE, format!(r"SOFTWARE\WOW6432Node\{CLIENT}")), (HKEY_LOCAL_MACHINE, format!(r"SOFTWARE\{CLIENT}")), (HKEY_CURRENT_USER, format!(r"Software\{CLIENT}"))];
    places.iter().find_map(|(root, path)| {
        let v: String = RegKey::predef(*root).open_subkey(path).ok()?.get_value("pv").ok()?;
        (v.chars().all(|c| c.is_ascii_digit() || c == '.') && !v.is_empty() && v != "0.0.0.0").then(|| v.chars().take(24).collect())
    })
}

// One of WebView2's processes gone, for this window: told (crash.rs
// webview_failed), and its page loaded again when that says so.
pub fn watch(sh: &Arc<Shared>, window: &tauri::WebviewWindow) -> tauri::Result<()> {
    use webview2_com::Microsoft::Web::WebView2::Win32::{ICoreWebView2ProcessFailedEventArgs2, COREWEBVIEW2_PROCESS_FAILED_KIND, COREWEBVIEW2_PROCESS_FAILED_REASON};
    use webview2_com::ProcessFailedEventHandler;
    use windows_core::Interface;

    let label = window.label().to_string();
    let sh = sh.clone();
    window.with_webview(move |view| {
        // SAFETY: WebView2's own objects, on its thread, as it hands them.
        unsafe {
            let Ok(core) = view.controller().CoreWebView2() else { return };
            let handler = ProcessFailedEventHandler::create(Box::new(move |webview, args| {
                let Some(args) = args else { return Ok(()) };
                let mut kind = COREWEBVIEW2_PROCESS_FAILED_KIND::default();
                args.ProcessFailedKind(&mut kind)?;
                let mut reason = COREWEBVIEW2_PROCESS_FAILED_REASON::default();
                let mut code = 0i32;
                if let Ok(more) = args.cast::<ICoreWebView2ProcessFailedEventArgs2>() {
                    let _ = more.Reason(&mut reason);
                    let _ = more.ExitCode(&mut code);
                }
                if super::webview_failed(&sh, &label, kind.0, reason.0, code) {
                    if let Some(webview) = webview {
                        let _ = webview.Reload();
                    }
                }
                Ok(())
            }));
            let mut token = 0i64;
            let _ = core.add_ProcessFailed(&handler, &mut token);
        }
    })
}
