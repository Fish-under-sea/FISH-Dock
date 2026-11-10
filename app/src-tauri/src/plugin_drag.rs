//! 把插件窗口里的文件「拖出去」（2.2.3，issue #87 的 B3）。
//!
//! 旧产物靠自建 HTTP / 原生 drag-out 服务把暂存区的文件交回资源管理器，易碎且
//! 没有统一契约。这里给一条宿主级的通道：插件给出本窗口里的文件，宿主用 OLE
//! `DoDragDrop` 把它作为 `CF_HDROP` 交给系统，资源管理器接收后就是一次普通的
//! 文件拖放（可复制、可移动到别的文件夹）。
//!
//! **不改主岛任何东西**：这里只起一次系统拖放会话，不碰主岛的窗口、布局或绘制。
//!
//! 线程模型：`DoDragDrop` 会阻塞到用户松手，且要求 STA。所以它跑在**专用线程**
//! 里，调用方立即返回。拖拽期间插件窗口被关掉也不影响这次会话——系统会话持有
//! 自己的数据对象。

/// 允许一次拖出多少项。和拖入的批上限同一个量级，够长条整批交回。
const MAX_ITEMS: usize = 512;
/// 单条路径的长度上限（UTF-16 单元）。Windows 的路径上限是 32767，按最宽算。
const MAX_PATH_UNITS: usize = 32767;
/// 同时最多几个拖放会话。用户一次只能拖一批，1 就够；留一点余量给「松手瞬间
/// 又发起下一次」的竞态。
const MAX_SESSIONS: usize = 2;

/// 拖出前的合法性检查。**只做与安全相关的判断**：路径必须存在、必须是绝对路径、
/// 总量有上限。宿主不判断「这个插件有没有权限读它」——它已经拿到了这些路径。
pub fn check(paths: &[String]) -> Result<(), String> {
    if paths.is_empty() { return Err("没有要拖出的文件".into()); }
    if paths.len() > MAX_ITEMS { return Err(format!("一次最多拖出 {MAX_ITEMS} 个文件")); }
    let mut units = 0usize;
    for path in paths {
        if path.is_empty() || path.chars().any(|c| c.is_control()) { return Err("路径无效".into()); }
        units = units.saturating_add(path.encode_utf16().count() + 1);
        if units > MAX_PATH_UNITS { return Err("路径总量超过限制".into()); }
        let candidate = std::path::Path::new(path);
        if !candidate.is_absolute() { return Err("只能拖出绝对路径".into()); }
        if !candidate.exists() { return Err("要拖出的文件已不存在".into()); }
    }
    Ok(())
}

/// 起一次系统拖放。立即返回；`DoDragDrop` 在专用线程里跑到用户松手为止。
pub fn begin(paths: Vec<String>) -> Result<(), String> {
    check(&paths)?;
    start_session(paths);
    Ok(())
}

#[cfg(not(windows))]
fn start_session(_paths: Vec<String>) {
    // 只在 Windows 上有 OLE 拖放；其他平台没有资源管理器这个概念。
}

#[cfg(windows)]
fn start_session(paths: Vec<String>) {
    // 同时只允许一次拖放会话。OLE 拖放在用户松手前会一直阻塞，插件若在循环里
    // 反复调用就会一个窗口一个线程地堆起来。多出来的请求直接拒绝，插件那边
    // 拿到 started:false 而不是把宿主拖垮。
    static LIVE: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    if LIVE.fetch_add(1, std::sync::atomic::Ordering::SeqCst) >= MAX_SESSIONS {
        LIVE.fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
        eprintln!("FISH Dock: 已有拖出会话在进行，本次忽略");
        return;
    }
    std::thread::spawn(move || {
        if let Err(error) = session::run(&paths) {
            eprintln!("FISH Dock: 拖出会话未能开始：{error}");
        }
        LIVE.fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
    });
}

#[cfg(windows)]
mod session {
    use windows::core::{implement, BOOL, HRESULT, Ref, Result as WinResult};
    use windows::Win32::Foundation::{DATA_S_SAMEFORMATETC, DRAGDROP_S_CANCEL, DRAGDROP_S_DROP, DRAGDROP_S_USEDEFAULTCURSORS, E_NOTIMPL, HGLOBAL, S_OK, S_FALSE};
    use windows::Win32::System::Com::{
        IAdviseSink, IDataObject, IDataObject_Impl, IEnumFORMATETC, IEnumSTATDATA,
        FORMATETC, STGMEDIUM, STGMEDIUM_0, TYMED_HGLOBAL,
    };
    use windows::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE, GMEM_ZEROINIT};
    use windows::Win32::System::Ole::{
        DoDragDrop, IDropSource, IDropSource_Impl, OleInitialize, OleUninitialize,
        DROPEFFECT_COPY, DROPEFFECT_MOVE,
    };
        use windows::Win32::System::SystemServices::{MK_LBUTTON, MK_RBUTTON, MODIFIERKEYS_FLAGS};
    use windows::Win32::UI::Shell::DROPFILES;

    /// 剪贴板格式：一串文件路径（资源管理器认的就是这个）。
    const CF_HDROP_FORMAT: u16 = 15;

    /// 把路径列表打包成一个 `CF_HDROP` 的全局内存块。
    ///
    /// 布局是 `DROPFILES` 头 + 一串以 NUL 结尾的宽字符路径 + 末尾再一个 NUL。
    /// 每次 `GetData` 都重新造一份：`STGMEDIUM` 的所有权交给调用方，
    /// 它会 `ReleaseStgMedium` 释放这块内存，重复使用同一块就会二次释放。
    fn build_hdrop(paths: &[String]) -> WinResult<HGLOBAL> {
        let wide: Vec<Vec<u16>> = paths
            .iter()
            .map(|path| path.encode_utf16().chain(std::iter::once(0)).collect())
            .collect();
        let units: usize = wide.iter().map(|w| w.len()).sum::<usize>() + 1;
        let header = std::mem::size_of::<DROPFILES>();
        let bytes = header + units * std::mem::size_of::<u16>();
        unsafe {
            let handle = GlobalAlloc(GMEM_MOVEABLE | GMEM_ZEROINIT, bytes)?;
            let base = GlobalLock(handle) as *mut u8;
            if base.is_null() {
                return Err(windows::core::Error::from(E_NOTIMPL));
            }
            let header_ptr = base as *mut DROPFILES;
            (*header_ptr).pFiles = header as u32;
            (*header_ptr).fWide = BOOL(1);
            let mut cursor = base.add(header) as *mut u16;
            for item in &wide {
                std::ptr::copy_nonoverlapping(item.as_ptr(), cursor, item.len());
                cursor = cursor.add(item.len());
            }
            // 末尾已经由 GMEM_ZEROINIT 置零，这里显式写一次以示这是列表终止符。
            *cursor = 0;
            let _ = GlobalUnlock(handle);
            Ok(handle)
        }
    }

    /// 作为拖放数据源的文件列表。
    #[implement(IDataObject)]
    struct FileData {
        paths: Vec<String>,
    }

    impl IDataObject_Impl for FileData_Impl {
        fn GetData(&self, pformatetcin: *const FORMATETC) -> WinResult<STGMEDIUM> {
            let format = unsafe { (*pformatetcin).cfFormat };
            if format != CF_HDROP_FORMAT {
                return Err(E_NOTIMPL.into());
            }
            let hdrop = build_hdrop(&self.paths)?;
            Ok(STGMEDIUM {
                tymed: windows::Win32::System::Com::TYMED_HGLOBAL.0 as u32,
                u: STGMEDIUM_0 { hGlobal: hdrop },
                pUnkForRelease: std::mem::ManuallyDrop::new(None),
            })
        }

        fn GetDataHere(&self, _pformatetc: *const FORMATETC, _pmedium: *mut STGMEDIUM) -> WinResult<()> {
            Err(E_NOTIMPL.into())
        }

        fn QueryGetData(&self, pformatetc: *const FORMATETC) -> HRESULT {
            // 只有 CF_HDROP 有内容：别的格式一律 S_FALSE（不是错误，只是没有）。
            let format = unsafe { (*pformatetc).cfFormat };
            if format == CF_HDROP_FORMAT { S_OK } else { S_FALSE }
        }

        fn GetCanonicalFormatEtc(&self, _pformatectin: *const FORMATETC, pformatetcout: *mut FORMATETC) -> HRESULT {
            // 没有「等价格式」可言：按约定回 DATA_S_SAMEFORMATETC。
            unsafe { (*pformatetcout).ptd = std::ptr::null_mut() };
            DATA_S_SAMEFORMATETC
        }

        fn SetData(&self, _pformatetc: *const FORMATETC, _pmedium: *const STGMEDIUM, _frelease: BOOL) -> WinResult<()> {
            Err(E_NOTIMPL.into())
        }

        fn EnumFormatEtc(&self, _dwdirection: u32) -> WinResult<IEnumFORMATETC> {
            Err(E_NOTIMPL.into())
        }

        fn DAdvise(&self, _pformatetc: *const FORMATETC, _advf: u32, _padvsink: Ref<IAdviseSink>) -> WinResult<u32> {
            Err(E_NOTIMPL.into())
        }

        fn DUnadvise(&self, _dwconnection: u32) -> WinResult<()> {
            Err(E_NOTIMPL.into())
        }

        fn EnumDAdvise(&self) -> WinResult<IEnumSTATDATA> {
            Err(E_NOTIMPL.into())
        }
    }

    /// 拖放的「源」：决定什么时候算放下、什么时候算取消。
    #[implement(IDropSource)]
    struct Source;

    impl IDropSource_Impl for Source_Impl {
        fn QueryContinueDrag(&self, fescapepressed: BOOL, grfkeystate: MODIFIERKEYS_FLAGS) -> HRESULT {
            // Esc：取消；鼠标键松了：放下；否则继续。
            if fescapepressed.as_bool() { return DRAGDROP_S_CANCEL; }
            if (grfkeystate & (MK_LBUTTON | MK_RBUTTON)).0 == 0 { return DRAGDROP_S_DROP; }
            S_OK
        }

        fn GiveFeedback(&self, _dweffect: windows::Win32::System::Ole::DROPEFFECT) -> HRESULT {
            // 光标交给系统画（复制/移动的默认光标）。
            DRAGDROP_S_USEDEFAULTCURSORS
        }
    }

    /// 在一个专用线程里跑完整会话。调用方已经把它放进线程。
    pub fn run(paths: &[String]) -> Result<(), String> {
        unsafe {
            // 这个线程由我们起、只做这一次拖放：初始化成 STA 正合适。
            let _ = OleInitialize(None);
            let result = (|| -> Result<(), String> {
                let data: IDataObject = FileData { paths: paths.to_vec() }.into();
                let source: IDropSource = Source.into();
                let mut effect = DROPEFFECT_COPY | DROPEFFECT_MOVE;
                // 返回 S_OK / DRAGDROP_S_DROP / DRAGDROP_S_CANCEL 都算正常结束：
                // 用户拖到哪、放没放下不是宿主要判断的事。
                let _ = DoDragDrop(&data, &source, DROPEFFECT_COPY | DROPEFFECT_MOVE, &mut effect);
                Ok(())
            })();
            OleUninitialize();
            result
        }
    }

    // 引用 HGLOBAL 让编译器确认类型；实际释放由系统的 STGMEDIUM 流程负责。
    #[allow(dead_code)]
    fn _owns_hglobal(_handle: HGLOBAL) {}
}

#[cfg(test)]
mod tests {
    use super::*;

    // 空列表直接拒绝：没有可拖的东西就不要再起一次系统会话。
    #[test]
    fn an_empty_list_is_refused() {
        assert!(check(&[]).is_err());
    }

    // 相对路径拒绝：拖出的目标必须是真实存在的绝对路径，
    // 免得插件把「当前目录下的某个名字」交给系统去解析。
    #[test]
    fn relative_paths_are_refused() {
        assert!(check(&["README.md".to_string()]).is_err());
    }

    // 不存在的文件拒绝：拖一个已经不在了的路径只会让资源管理器报错。
    #[test]
    fn missing_files_are_refused() {
        assert!(check(&["C:/__fish_dock_nonexistent__/x.txt".to_string()]).is_err());
    }

    // 超过批量上限拒绝。
    #[test]
    fn an_oversized_batch_is_refused() {
        let many: Vec<String> = (0..=MAX_ITEMS).map(|i| format!("C:/tmp/{i}.txt")).collect();
        assert!(check(&many).is_err(), "超过 {MAX_ITEMS} 项必须拒绝");
    }

    // 控制字符拒绝：路径里不可能有，出现就是构造出来的。
    #[test]
    fn control_characters_are_refused() {
        assert!(check(&["C:/tmp/a\u{0}b.txt".to_string()]).is_err());
    }
}
