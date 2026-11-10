//! 插件外部原生窗口（2.2.0 §1–7）。
//!
//! 这里只做「通用容器」：一个独立的 Tauri/WebView2 顶层窗口 + 宿主自己的最小页面
//! （src/window/），页面里的 iframe 加载插件已授权的资源。宿主不画任何业务 UI，
//! 不认识任何插件 ID（旧的文件暂存子岛已删除），只回答三件事：
//!
//!   · 谁能开窗、开哪一个已登记页面（身份来自页面票据或后端会话，从不听插件自称）；
//!   · 窗口活到哪一刻（关闭/停用/卸载/换版/崩溃/退出/迟到创建都要收干净）；
//!   · 文件拖入交给谁（只按所有者定向，不广播路径，日志不打印路径）。
//!
//! 锁的边界（重要）：本模块的 BOOK 是唯一权威表。**绝不在持有 BOOK 时去取
//! plugin_runtime::CURRENT**（plugin_runtime 的停用路径是 CURRENT → plugin_window，
//! 反过来取就会成环）；票据与事件通过独立的 Hook 会话校验，不获取 CURRENT。
//! 等页面就绪用 Condvar 的 `wait_timeout`（等待时释放 BOOK），创建/关闭都由别的
//! 线程推进，不会出现「持锁等自己」。
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Condvar, Mutex, MutexGuard, OnceLock};
use std::thread::ThreadId;
use std::time::{Duration, Instant};

use serde_json::{json, Value};
use tauri::{AppHandle, Emitter, Manager as _, WebviewUrl, WebviewWindow, WebviewWindowBuilder, WindowEvent};

/// 页面就绪的等待上限：超时即回收名额与票据，绝不留空白窗口。
const READY_WAIT: Duration = Duration::from_secs(8);
/// 主线程窗口操作的等待上限（建窗、销毁、改尺寸）。
const MAIN_WAIT: Duration = Duration::from_secs(6);
/// 每插件与全局窗口上限（规格 §4.2）。
const PER_PLUGIN: usize = 4;
const TOTAL: usize = 16;
const KEY_MAX: usize = 64;
const TITLE_MAX: usize = 80;
const WIDTH_MIN: f64 = 32.0;
const WIDTH_MAX: f64 = 2400.0;
const HEIGHT_MIN: f64 = 32.0;
const HEIGHT_MAX: f64 = 1600.0;
const OFFSET_LIMIT: f64 = 20000.0;
const WIDTH_DEFAULT: f64 = 420.0;
const HEIGHT_DEFAULT: f64 = 320.0;

/// 2.2.2（B2）：窗口的哪条边/角贴住锚点。
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Align {
    RightTop, RightCenter, RightBottom,
    LeftTop, LeftCenter, LeftBottom,
    TopCenter, BottomCenter,
}

impl Align {
    fn parse(name: &str) -> Option<Self> {
        Some(match name {
            "right-top" => Self::RightTop,
            "right-center" => Self::RightCenter,
            "right-bottom" => Self::RightBottom,
            "left-top" => Self::LeftTop,
            "left-center" => Self::LeftCenter,
            "left-bottom" => Self::LeftBottom,
            "top-center" => Self::TopCenter,
            "bottom-center" => Self::BottomCenter,
            _ => return None,
        })
    }
    fn name(self) -> &'static str {
        match self {
            Self::RightTop => "right-top",
            Self::RightCenter => "right-center",
            Self::RightBottom => "right-bottom",
            Self::LeftTop => "left-top",
            Self::LeftCenter => "left-center",
            Self::LeftBottom => "left-bottom",
            Self::TopCenter => "top-center",
            Self::BottomCenter => "bottom-center",
        }
    }
    /// 按对齐方式把窗口左上角放到锚点 (px, py) 上。
    fn offset(self, (px, py): (i32, i32), (width, height): (i32, i32)) -> (i32, i32) {
        match self {
            Self::RightTop => (px - width, py),
            Self::RightCenter => (px - width, py - height / 2),
            Self::RightBottom => (px - width, py - height),
            Self::LeftTop => (px, py),
            Self::LeftCenter => (px, py - height / 2),
            Self::LeftBottom => (px, py - height),
            Self::TopCenter => (px - width / 2, py),
            Self::BottomCenter => (px - width / 2, py - height),
        }
    }
}

/// 2.2.2（B2）：锚点相对哪个矩形——主岛窗口还是可见胶囊。
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum AnchorBase {
    /// 2.2.1 的行为：相对主岛窗口（铺满工作区宽）。
    Window,
    /// 相对可见胶囊；胶囊尺寸未知时回落到 Window。
    Capsule,
}

impl AnchorBase {
    fn parse(name: &str) -> Option<Self> {
        Some(match name {
            "window" => Self::Window,
            "capsule" => Self::Capsule,
            _ => return None,
        })
    }
    fn name(self) -> &'static str {
        match self {
            Self::Window => "window",
            Self::Capsule => "capsule",
        }
    }
}

/// 2.2.2（E6）：窗口层级。
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ZOrder {
    /// 2.2.1 的行为：普通顶层窗口，不置顶。
    Normal,
    /// 置顶，且排在主岛窗口之上（主岛本身是 always_on_top）。
    AboveDock,
}

impl ZOrder {
    fn parse(name: &str) -> Option<Self> {
        Some(match name {
            "normal" => Self::Normal,
            "above-dock" => Self::AboveDock,
            _ => return None,
        })
    }
    fn name(self) -> &'static str {
        match self {
            Self::Normal => "normal",
            Self::AboveDock => "above-dock",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Anchor {
    DockLeft,
    DockBottom,
    Screen,
}

/// 2.2.2：窗口宽度。`Dock` 表示跟随可见胶囊当前宽度（C4）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Width {
    Fixed(f64),
    /// 跟随主岛胶囊宽度；`max` 是内容可增长到的上限（逻辑像素），None = 恒定等宽。
    Dock { max: Option<f64> },
}

impl Width {
    /// 算出实际逻辑宽度。`capsule` 是胶囊的逻辑宽度；拿不到时回落到默认值。
    fn resolve(self, capsule: Option<f64>) -> f64 {
        match self {
            Width::Fixed(value) => value,
            Width::Dock { max } => {
                // 胶囊宽度未知时用默认宽度，绝不让窗口塌成 0。
                let base = capsule.unwrap_or(WIDTH_DEFAULT).max(WIDTH_MIN).min(WIDTH_MAX);
                match max {
                    // max 小于胶囊宽时按胶囊宽算：语义是「可增长到」，不是「缩到」。
                    Some(limit) => base.min(limit.max(base)).min(WIDTH_MAX),
                    None => base,
                }
            }
        }
    }
}

impl Anchor {
    fn parse(name: &str) -> Option<Self> {
        match name {
            "dock-left" => Some(Self::DockLeft),
            "dock-bottom" => Some(Self::DockBottom),
            "screen" => Some(Self::Screen),
            _ => None,
        }
    }
    fn name(self) -> &'static str {
        match self {
            Self::DockLeft => "dock-left",
            Self::DockBottom => "dock-bottom",
            Self::Screen => "screen",
        }
    }
}

/// 一次 open 的规范化参数。任何一项不合格都在入口拒绝，绝不带病建窗。
#[derive(Clone, Debug)]
pub struct Options {
    pub key: String,
    pub page: String,
    pub title: Option<String>,
    pub width: Width,
    pub height: f64,
    pub anchor: Anchor,
    /// 2.2.2（B2）：窗口哪条边/角贴住锚点。
    pub align: Align,
    /// 2.2.2（B2）：锚点相对主岛窗口还是可见胶囊。
    pub anchor_base: AnchorBase,
    pub offset_x: f64,
    pub offset_y: f64,
    pub transparent: bool,
    /// 2.2.2（E6）：'normal' 或 'above-dock'（置顶，压在主岛之上）。
    pub z_order: ZOrder,
}

fn finite(value: &Value) -> Option<f64> {
    value.as_f64().filter(|v| v.is_finite())
}

fn name_ok(value: &str) -> bool {
    // 小写字母、数字、-、_：既可读又能原样进 IPC 日志，不含路径分隔符。
    !value.is_empty() && value.len() <= KEY_MAX && value.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'_')
}

impl Options {
    /// 纯函数：options 的校验与默认值只在这里定义一次（前端/后端/测试共用）。
    pub fn parse(params: &Value) -> Result<Self, String> {
        let object = params.as_object().ok_or("插件窗口参数应为对象")?;
        for name in object.keys() {
            if !["key", "page", "title", "width", "height", "anchor", "offsetX", "offsetY", "transparent", "maxWidth", "align", "anchorBase", "zOrder"].contains(&name.as_str()) {
                return Err("插件窗口参数含未知字段".into());
            }
        }
        let text = |name: &str| object.get(name).and_then(Value::as_str);
        let key = text("key").ok_or("插件窗口缺少 key")?;
        if !name_ok(key) {
            return Err("插件窗口 key 无效（小写字母、数字、- 或 _，最长 64）".into());
        }
        let page = text("page").ok_or("插件窗口缺少 page")?;
        if !name_ok(page) {
            return Err("插件窗口 page 无效（必须是插件已登记页面 ID）".into());
        }
        let title = match object.get("title") {
            None => None,
            Some(value) => {
                let value = value.as_str().ok_or("插件窗口标题应为字符串")?;
                let trimmed = value.trim();
                if trimmed.is_empty() || trimmed.chars().count() > TITLE_MAX || trimmed.chars().any(char::is_control) {
                    return Err("插件窗口标题无效（1–80 字，不含控制字符）".into());
                }
                Some(trimmed.to_string())
            }
        };
        let size = |name: &str, default: f64, min: f64, max: f64| -> Result<f64, String> {
            match object.get(name) {
                None => Ok(default),
                Some(value) => match finite(value) {
                    Some(number) if (min..=max).contains(&number) => Ok(number),
                    Some(_) => Err(format!("插件窗口 {name} 超出范围（{min}–{max} 逻辑像素）")),
                    None => Err(format!("插件窗口 {name} 必须是有限数")),
                },
            }
        };
        // 2.2.2（C4）：width 可以是数字，也可以是 'dock'（跟随胶囊宽度）。
        let width = match object.get("width") {
            None => Width::Fixed(WIDTH_DEFAULT),
            Some(Value::String(text)) if text == "dock" => Width::Dock { max: None },
            Some(value) => match finite(value) {
                Some(number) if (WIDTH_MIN..=WIDTH_MAX).contains(&number) => Width::Fixed(number),
                Some(_) => return Err(format!("插件窗口 width 超出范围（{WIDTH_MIN}–{WIDTH_MAX} 逻辑像素）或 'dock'")),
                None => return Err("插件窗口 width 必须是有限数或 'dock'".into()),
            },
        };
        // maxWidth 只在 width:'dock' 时有意义；其余情况忽略（不报错，避免插件
        // 为了兼容旧宿主而分支）。范围同 width。
        let max_width = match (object.get("maxWidth"), width) {
            (Some(value), Width::Dock { .. }) => match finite(value) {
                Some(number) if (WIDTH_MIN..=WIDTH_MAX).contains(&number) => Some(number),
                Some(_) => return Err(format!("插件窗口 maxWidth 超出范围（{WIDTH_MIN}–{WIDTH_MAX} 逻辑像素）")),
                None => return Err("插件窗口 maxWidth 必须是有限数".into()),
            },
            _ => None,
        };
        let width = match width {
            Width::Dock { .. } => Width::Dock { max: max_width },
            other => other,
        };
        let height = size("height", HEIGHT_DEFAULT, HEIGHT_MIN, HEIGHT_MAX)?;
        let anchor = match object.get("anchor") {
            None => Anchor::DockBottom,
            Some(value) => value.as_str().and_then(Anchor::parse).ok_or("插件窗口 anchor 只支持 dock-left、dock-bottom、screen")?,
        };
        // 2.2.2（B2）：align 缺省按 anchor 取旧版等价的对齐，保证不写这个字段时
        // 位置与 2.2.1 一模一样。
        let align = match object.get("align") {
            None => match anchor {
                Anchor::DockLeft => Align::RightCenter,
                _ => Align::LeftTop,
            },
            Some(value) => value.as_str().and_then(Align::parse).ok_or(
                "插件窗口 align 只支持 right-top、right-center、right-bottom、left-top、left-center、left-bottom、top-center、bottom-center",
            )?,
        };
        let anchor_base = match object.get("anchorBase") {
            None => AnchorBase::Window,
            Some(value) => value.as_str().and_then(AnchorBase::parse).ok_or("插件窗口 anchorBase 只支持 window、capsule")?,
        };
        let offset = |name: &str| -> Result<f64, String> {
            match object.get(name) {
                None => Ok(0.0),
                Some(value) => match finite(value) {
                    Some(number) if number.abs() <= OFFSET_LIMIT => Ok(number),
                    Some(_) => Err(format!("插件窗口 {name} 超出范围（±{OFFSET_LIMIT}）")),
                    None => Err(format!("插件窗口 {name} 必须是有限数")),
                },
            }
        };
        let offset_x = offset("offsetX")?;
        let offset_y = offset("offsetY")?;
        let transparent = match object.get("transparent") {
            None => false,
            Some(Value::Bool(value)) => *value,
            Some(_) => return Err("插件窗口 transparent 应为布尔值".into()),
        };
        let z_order = match object.get("zOrder") {
            None => ZOrder::Normal,
            Some(value) => value.as_str().and_then(ZOrder::parse).ok_or("插件窗口 zOrder 只支持 normal、above-dock")?,
        };
        Ok(Self { key: key.into(), page: page.into(), title, width, height, anchor, align, anchor_base, offset_x, offset_y, transparent, z_order })
    }
}

// ---------------------------------------------------------------------------------------
// 登记表
// ---------------------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Debug)]
enum State {
    /// 名额已占、票据已建，窗口还在建或页面还没报 ready。
    Pending,
    Ready,
}

#[derive(Clone, Debug)]
struct Rec {
    handle: String,
    plugin: String,
    digest: String,
    generation: u64,
    key: String,
    page: String,
    title: String,
    label: String,
    ticket: String,
    url: String,
    anchor: Anchor,
    /// 2.2.2（B2）：窗口哪条边/角贴住锚点。
    align: Align,
    /// 2.2.2（B2）：锚点相对主岛窗口还是可见胶囊。
    anchor_base: AnchorBase,
    offset_x: f64,
    offset_y: f64,
    /// 2.2.2（C4）：固定宽度，或跟随胶囊宽度。
    width: Width,
    height: f64,
    transparent: bool,
    /// 2.2.2（E6）：窗口层级。
    z_order: ZOrder,
    state: State,
    /// 窗口当前是否显示（被 Dock 全局隐藏时为 false）。
    shown: bool,
    /// 因为 Dock 隐藏而被宿主收起来：恢复时只恢复这一批（用户自己关掉的不复活）。
    hidden_by_dock: bool,
    /// 本窗口的外框（屏幕物理像素），拖入命中区用。
    rect: Option<(i32, i32, i32, i32)>,
    /// 本窗口的文件拖入订阅句柄：没有订阅就不捕获、不交付（句柄原样回给插件）。
    drop: Option<String>,
}

#[derive(Clone, Debug)]
struct DockSub {
    handle: String,
    plugin: String,
    generation: u64,
}

struct Book {
    windows: BTreeMap<String, Rec>,
    dock: Option<DockSub>,
    next: u64,
    sessions: BTreeMap<String, u64>,
    outputs: BTreeMap<String, std::sync::mpsc::SyncSender<Value>>,
    dock_hidden: bool,
    /// 2.2.2：订阅了主岛几何/状态事件的插件 → 代次。空表示没人听。
    dock_watchers: BTreeMap<String, u64>,
}

impl Book {
    fn per_plugin(&self, plugin: &str) -> usize {
        self.windows.values().filter(|rec| rec.plugin == plugin).count()
    }
    fn by_label(&self, label: &str) -> Option<&Rec> {
        self.windows.values().find(|rec| rec.label == label)
    }

    fn fresh(&self, plugin: &str, generation: u64) -> bool {
        generation != 0 && self.sessions.get(plugin) == Some(&generation)
    }
    fn retire(&mut self, plugin: &str, generation: Option<u64>) -> Vec<Rec> {
        if generation.is_some_and(|generation| !self.fresh(plugin, generation)) { return Vec::new(); }
        self.sessions.remove(plugin);
        self.outputs.remove(plugin);
        if self.dock.as_ref().is_some_and(|sub| sub.plugin == plugin) { self.dock = None; }
        let handles: Vec<_> = self.windows.values().filter(|rec| rec.plugin == plugin).map(|rec| rec.handle.clone()).collect();
        handles.into_iter().filter_map(|handle| self.windows.remove(&handle)).collect()
    }
    fn reserve(&mut self, mut candidate: Rec) -> Result<(Rec, bool), String> {
        if !self.fresh(&candidate.plugin, candidate.generation) { return Err("插件会话已撤销".into()); }
        if let Some(rec) = self.windows.values().find(|rec| rec.plugin == candidate.plugin && rec.key == candidate.key) {
            if rec.generation != candidate.generation || rec.digest != candidate.digest {
                return Err("插件实例或版本已变化，不能复用旧窗口".into());
            }
            return Ok((rec.clone(), true));
        }
        if self.per_plugin(&candidate.plugin) >= PER_PLUGIN {
            return Err(format!("每个插件最多同时打开 {PER_PLUGIN} 个窗口"));
        }
        if self.windows.len() >= TOTAL {
            return Err(format!("插件窗口总数已达上限（{TOTAL}），请先关闭部分窗口"));
        }
        self.next = self.next.wrapping_add(1);
        candidate.label = format!("plugin-win-{}", self.next);
        candidate.handle = token()?;
        candidate.hidden_by_dock = self.dock_hidden;
        self.windows.insert(candidate.handle.clone(), candidate.clone());
        Ok((candidate, false))
    }
}

static BOOK: Mutex<Book> = Mutex::new(Book { windows: BTreeMap::new(), dock: None, next: 0, sessions: BTreeMap::new(), outputs: BTreeMap::new(), dock_hidden: false, dock_watchers: BTreeMap::new() });
/// 上一次看到的 Dock 外框：变了才重排锚定窗口（主岛的移动/尺寸事件很密）。
static DOCK_RECT: Mutex<Option<(i32, i32, i32, i32)>> = Mutex::new(None);
static READY: Condvar = Condvar::new();
static APP: OnceLock<AppHandle> = OnceLock::new();
static MAIN: OnceLock<ThreadId> = OnceLock::new();
/// 只用来给 handle 上一点变化，真正的唯一性由 BOOK 的键决定。
static TICK: AtomicU64 = AtomicU64::new(1);

fn book() -> MutexGuard<'static, Book> {
    BOOK.lock().unwrap_or_else(|poison| poison.into_inner())
}

/// 宿主 setup 时调用一次：记住 AppHandle 与主线程身份（建窗必须在主线程）。
pub fn install(app: AppHandle) {
    let _ = APP.set(app);
    let _ = MAIN.set(std::thread::current().id());
}

fn app() -> Result<&'static AppHandle, String> {
    APP.get().ok_or_else(|| "宿主窗口服务尚未就绪".to_string())
}

fn log(text: &str) {
    if let Some(app) = APP.get() {
        crate::shared(app).log(&format!("plugin-window: {text}"));
    }
}

/// 一个不透明的窗口句柄：插件只能拿它回指自己的窗口，不从中读任何信息。
fn token() -> Result<String, String> {
    let mut bytes = [0u8; 16];
    #[cfg(windows)] {
        #[link(name = "bcrypt")]
        unsafe extern "system" {
            fn BCryptGenRandom(algorithm: isize, buffer: *mut u8, size: u32, flags: u32) -> i32;
        }
        if unsafe { BCryptGenRandom(0, bytes.as_mut_ptr(), 16, 2) } < 0 {
            return Err("窗口句柄生成失败".into());
        }
    }
    #[cfg(not(windows))] {
        use std::io::Read;
        std::fs::File::open("/dev/urandom").and_then(|mut file| file.read_exact(&mut bytes)).map_err(|_| "窗口句柄生成失败")?;
    }
    let tick = TICK.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    Ok(format!("{}{:04x}", bytes.iter().map(|b| format!("{b:02x}")).collect::<String>(), tick & 0xffff))
}

// ---------------------------------------------------------------------------------------
// 主线程
// ---------------------------------------------------------------------------------------

fn on_main<R, F>(work: F) -> Result<R, String>
where
    R: Send + 'static,
    F: FnOnce() -> R + Send + 'static,
{
    let app = app()?.clone();
    if MAIN.get() == Some(&std::thread::current().id()) {
        return Ok(work());
    }
    let (tell, told) = std::sync::mpsc::channel();
    app.run_on_main_thread(move || {
        let _ = tell.send(work());
    })
    .map_err(|_| "窗口操作无法排入主线程".to_string())?;
    told.recv_timeout(MAIN_WAIT).map_err(|_| "窗口操作在主线程超时".to_string())
}

/// 不等待主线程的版本：销毁/隐藏/重排这类动作只需「排进去」。停用与卸载路径可能
/// 正持着 plugin_runtime 的锁，绝不能在这里等主线程，否则锁序会成环。
fn post_main<F>(work: F)
where
    F: FnOnce() + Send + 'static,
{
    let Ok(app) = app() else { return };
    let app = app.clone();
    if MAIN.get() == Some(&std::thread::current().id()) {
        work();
        return;
    }
    let _ = app.run_on_main_thread(work);
}

// ---------------------------------------------------------------------------------------
// 身份：能力、已登记页面、代次
// ---------------------------------------------------------------------------------------

type Context = crate::storage_context::Context;

// Hook 会话在握手前登记，覆盖 activate 内的宿主回调。它与 CURRENT 分离，
// 因此持 BOOK 或 CURRENT 时撤销都不会反向获取运行时锁。
static NEXT_SESSION: AtomicU64 = AtomicU64::new(1);
pub struct Session { plugin: String, pub generation: u64 }
impl Session {
    pub fn revoke(&self) { revoke_generation(&self.plugin, Some(self.generation), "stopped"); }
}
impl Drop for Session { fn drop(&mut self) { self.revoke(); } }
pub fn begin_session(plugin: &str, output: std::sync::mpsc::SyncSender<Value>) -> Session {
    let generation = NEXT_SESSION.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let (retired, previous) = {
        let mut book = book();
        let previous = book.outputs.get(plugin).cloned();
        let retired = book.retire(plugin, None);
        book.sessions.insert(plugin.into(), generation);
        book.outputs.insert(plugin.into(), output);
        (retired, previous)
    };
    for rec in retired {
        terminal_event(&rec, "replaced", previous.as_ref());
        destroy_record(rec, "replaced");
    }
    Session { plugin: plugin.into(), generation }
}
pub fn has_session(plugin: &str) -> bool { current_generation(plugin) != 0 }

fn installed(context: &Context, id: &str) -> Result<crate::plugin_install::Installed, String> {
    crate::plugin_install::Manager { context }
        .list()?
        .into_iter()
        .find(|row| row.manifest.id == id)
        .ok_or_else(|| "插件未安装".to_string())
}

fn require_capability(row: &crate::plugin_install::Installed, name: &str, text: &str) -> Result<(), String> {
    if row.manifest.capabilities.iter().any(|c| c == name) {
        Ok(())
    } else {
        Err(text.into())
    }
}

/// 后端与页面共用的「当前代次」：停用/卸载/换版会 +1，旧代次一律不许再动窗口。
pub fn current_generation(plugin: &str) -> u64 {
    book().sessions.get(plugin).copied().unwrap_or(0)
}

fn generation_fresh(plugin: &str, generation: u64) -> bool {
    generation != 0 && current_generation(plugin) == generation
}

// ---------------------------------------------------------------------------------------
// 公开入口：页面与后端
// ---------------------------------------------------------------------------------------

/// 页面桥的 dock.windows.* / dock.fileDrop.*（插件页与外部窗口页共用）。
/// 身份、摘要、承载标签都由 plugin_page 的票据推出，这里不接受调用方自报的 owner。
pub fn page(plugin: &str, digest: &str, generation: u64, _host: &str, method: &str, params: &Value) -> Result<Value, String> {
    if !generation_fresh(plugin, generation) { return Err("页面会话已撤销".into()); }
    let context = crate::storage_context::current().ok_or("存储尚未就绪")?;
    match method {
        "dock.windows.open" => open(&context, plugin, digest, generation, params),
        "dock.windows.update" => update(&context, plugin, generation, params),
        "dock.windows.close" => close_owned(plugin, generation, params, "plugin"),
        "dock.windows.list" => list(&context, plugin, generation),
        "dock.fileDrop.subscribe" => subscribe(&context, plugin, generation, params),        "dock.fileDrop.unsubscribe" => unsubscribe(plugin, generation, params),
        // 2.2.2：主岛几何与状态（只读接口 + 事件订阅）
        "dock.dock.rect" => { let context = crate::storage_context::current().ok_or("存储尚未就绪")?; dock_rect_for(&context, plugin) },
        "dock.dock.subscribe" => { let context = crate::storage_context::current().ok_or("存储尚未就绪")?; subscribe_dock_for(&context, plugin, generation) },
        "dock.dock.unsubscribe" => unsubscribe_dock(plugin),
        _ => Err("插件窗口方法无效".into()),
    }
}

/// 后端桥的 host.windows.* / host.fileDrop.*。代次来自拉起这个进程时的那个实例，
/// 旧的进程（哪怕回调线程还没退）也不能操作新实例的窗口。
pub fn backend(context: &Context, plugin: &str, generation: u64, method: &str, params: &Value) -> Result<Value, String> {
    if generation == 0 || !generation_fresh(plugin, generation) {
        return Err("插件实例代次已变化，本次窗口请求已丢弃".into());
    }
    let row = installed(context, plugin)?;
    match method {
        "host.windows.open" => open(context, plugin, &row.digest.clone(), generation, params),
        "host.windows.update" => update(context, plugin, generation, params),
        "host.windows.close" => close_owned(plugin, generation, params, "plugin"),
        "host.windows.list" => list(context, plugin, generation),
        "host.fileDrop.subscribe" => subscribe(context, plugin, generation, params),        "host.fileDrop.unsubscribe" => unsubscribe(plugin, generation, params),
        "host.dock.rect" => dock_rect_for(context, plugin),
        "host.dock.subscribe" => subscribe_dock_for(context, plugin, generation),
        "host.dock.unsubscribe" => unsubscribe_dock(plugin),
        _ => Err("插件窗口方法无效".into()),
    }
}

/// 开窗：唯一实现。所有身份校验都在这里，两条入口只在「身份从哪来」上不同。
fn open(context: &Context, plugin: &str, digest: &str, generation: u64, params: &Value) -> Result<Value, String> {
    let options = Options::parse(params)?;
    if !generation_fresh(plugin, generation) {
        return Err("插件尚未运行或实例已变化，无法打开窗口".into());
    }
    let row = installed(context, plugin)?;
    if row.digest != digest {
        return Err("插件版本已变化，请重新打开页面后再试".into());
    }
    require_capability(&row, "windows", "插件未声明 windows 能力")?;
    let entry = row.manifest.pages.iter().find(|page| page.id == options.page).ok_or("插件没有登记这个页面")?;
    let title = options.title.clone().unwrap_or_else(|| entry.title.clone());

    // 名额、同 key 复用和插入在一次 BOOK 临界区完成，待就绪时再释放锁。
    let (reserved, reuse) = book().reserve(Rec {
        handle: String::new(), plugin: plugin.into(), digest: digest.into(), generation,
        key: options.key.clone(), page: options.page.clone(), title,
        label: String::new(), ticket: String::new(), url: String::new(),
        anchor: options.anchor, align: options.align, anchor_base: options.anchor_base, offset_x: options.offset_x, offset_y: options.offset_y,
        width: options.width, height: options.height, transparent: options.transparent, z_order: options.z_order,
        state: State::Pending, shown: false, hidden_by_dock: false, rect: None, drop: None,
    })?;
    let (handle, label) = (reserved.handle, reserved.label);
    if reuse {
        return wait_ready(&handle);
    }
    // 票据在 BOOK 之外建立，避免嵌套页面资源与窗口登记表锁。
    let page_key = format!("{plugin}/{}", options.page);
    let granted = match crate::plugin_page::open_in(&page_key, &label) {
        Ok(granted) => granted,
        Err(error) => {
            let _ = close_handle(&handle, "failed");
            return Err(error);
        }
    };
    let ticket = granted["ticket"].as_str().unwrap_or_default().to_string();
    let url = granted["url"].as_str().unwrap_or_default().to_string();
    let assigned = {
        let mut book = book();
        if let Some(rec) = book.windows.get_mut(&handle) {
            rec.ticket = ticket.clone();
            rec.url = url;
            true
        } else { false }
    };
    // 不在 BOOK 临界区获取页面票据锁。
    if !assigned {
        crate::plugin_page::close(&ticket);
        return Err("插件已停用，窗口请求已取消".into());
    }

    if let Err(error) = build(&handle) {
        let _ = close_handle(&handle, "failed");
        return Err(error);
    }
    match wait_ready(&handle) {
        Ok(value) => {
            log(&format!("opened {} for {plugin} key={} {}", short(&handle), options.key, label));
            Ok(value)
        }
        Err(error) => {
            // 创建中的请求取消/超时/构建失败：名额与票据当场释放，不留空白窗口。
            let _ = close_handle(&handle, "failed");
            Err(error)
        }
    }
}

fn wait_ready(handle: &str) -> Result<Value, String> {
    let deadline = Instant::now() + READY_WAIT;
    loop {
        let book = book();
        match book.windows.get(handle) {
            None => return Err("插件窗口在就绪前被回收".into()),
            Some(rec) if rec.state == State::Ready => return Ok(view(rec)),
            Some(_) => {}
        }
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return Err("插件窗口未能就绪，已回收".into());
        }
        let _ = READY.wait_timeout(book, left).unwrap_or_else(|poison| poison.into_inner());
    }
}

fn build(handle: &str) -> Result<(), String> {
    let snapshot = {
        let book = book();
        book.windows.get(handle).cloned().ok_or_else(|| "窗口记录已消失".to_string())?
    };
    let app = app()?.clone();
    on_main(move || -> Result<(), String> {
        if !book().windows.contains_key(&snapshot.handle) { return Err("窗口创建已取消".into()); }
        let win = WebviewWindowBuilder::new(&app, &snapshot.label, WebviewUrl::App("window/window.html".into()))
            .title(&snapshot.title)
            .inner_size(snapshot.width.resolve(capsule_width_logical(&app)), snapshot.height)
            // 透明浮窗允许插件自己画形状与按钮；此时没有原生标题栏，只剩宿主 Escape 与托盘出口。
            .decorations(!snapshot.transparent)
            .transparent(snapshot.transparent)
            .resizable(true)
            .maximizable(false)
            .minimizable(true)
            // 默认不占任务栏，也不抢当前焦点（显示走 SWP_NOACTIVATE）。
            .skip_taskbar(true)
            .focused(false)
            // 2.2.2（E6）：above-dock 要压在主岛之上，主岛本身是 always_on_top。
            .always_on_top(snapshot.z_order == ZOrder::AboveDock)
            .visible(false)
            .build()
            .map_err(|error| format!("插件窗口创建失败：{error}"))?;
        let watched = win.clone();
        win.on_window_event(move |event| window_event(&watched, event));
        if !book().windows.contains_key(&snapshot.handle) {
            let _ = win.destroy();
            return Err("窗口创建已取消".into());
        }
        if let Err(error) = place(&win, &snapshot) { let _ = win.destroy(); return Err(error); }
        Ok(())
    })?
}

/// 2.2.2（C4）：主岛胶囊的逻辑宽度，供 `width:'dock'` 用。
/// 物理像素 → 逻辑像素按主岛页面的 page_sf 换算；拿不到时返回 None，
/// 由 `Width::resolve` 回落到默认宽度。
fn capsule_width_logical(app: &AppHandle) -> Option<f64> {
    let shared = crate::shared(app);
    let snapshot = crate::island::snapshot(&shared);
    let (_, _, width, _) = snapshot.capsule?;
    let sf = if snapshot.scale > 0.0 { snapshot.scale } else { 1.0 };
    Some(width as f64 / sf)
}

fn place(win: &WebviewWindow, rec: &Rec) -> Result<(), String> {
    let app = win.app_handle();
    let sf = win.scale_factor().unwrap_or(1.0).max(0.5);
    let screens = crate::screen::read(app);
    let island = app.get_webview_window("island");
    let dock = island.as_ref().and_then(|win| match (win.outer_position(), win.outer_size()) {
        (Ok(pos), Ok(size)) => Some((pos.x, pos.y, size.width as i32, size.height as i32)),
        _ => None,
    });
    let capsule = crate::shared(app);
    let capsule = crate::island::snapshot(&capsule).capsule;
    let (x, y, width, height) = placement(rec, &screens, dock, capsule, sf);
    win.set_size(tauri::PhysicalSize::new(width as u32, height as u32)).map_err(|_| "插件窗口尺寸设置失败")?;
    win.set_position(tauri::PhysicalPosition::new(x, y)).map_err(|_| "插件窗口位置设置失败")?;
    // set_size 指客户区；原生标题栏和边框也必须留在工作区。
    if let (Some(area), Ok(inner), Ok(outer)) = (screens.near(x + width / 2, y + height / 2), win.inner_size(), win.outer_size()) {
        let (px, py, iw, ih) = fit_outer(area, (x,y), (inner.width,inner.height), (outer.width,outer.height));
        if iw != inner.width || ih != inner.height {
            win.set_size(tauri::PhysicalSize::new(iw,ih)).map_err(|_| "插件窗口工作区尺寸设置失败")?;
        }
        win.set_position(tauri::PhysicalPosition::new(px,py)).map_err(|_| "插件窗口工作区位置设置失败")?;
    }
    Ok(())
}
fn fit_outer(area: crate::screen::Area, (x,y): (i32,i32), (iw,ih): (u32,u32), (ow,oh): (u32,u32)) -> (i32,i32,u32,u32) {
    let border_w = ow.saturating_sub(iw);
    let border_h = oh.saturating_sub(ih);
    let iw = iw.min((area.w.max(1) as u32).saturating_sub(border_w).max(1));
    let ih = ih.min((area.h.max(1) as u32).saturating_sub(border_h).max(1));
    (x.clamp(area.x, area.x + (area.w - (iw + border_w) as i32).max(0)),
     y.clamp(area.y, area.y + (area.h - (ih + border_h) as i32).max(0)), iw, ih)
}

fn placement(rec: &Rec, screens: &crate::screen::Screens, dock: Option<(i32, i32, i32, i32)>, capsule: Option<(i32, i32, i32, i32)>, fallback_sf: f64) -> (i32, i32, i32, i32) {
    let base = screens.primary.or_else(|| screens.near(0, 0));
    let target = if rec.anchor == Anchor::Screen {
        let sf = base.map_or(fallback_sf, |area| area.sf);
        let (x,y) = base.map_or((0,0), |area| (area.x,area.y));
        screens.near(x + (rec.offset_x * sf).round() as i32, y + (rec.offset_y * sf).round() as i32)
    } else {
        dock.and_then(|(x,y,w,h)| screens.near(x+w/2,y+h/2)).or(base)
    };
    let sf = target.map_or(fallback_sf, |area| area.sf).max(0.5);
    // 2.2.2（C4）：width:'dock' 跟随胶囊当前宽度。胶囊是物理像素，先换成逻辑。
    let capsule_logical = capsule.map(|(_, _, w, _)| w as f64 / sf);
    let width = ((rec.width.resolve(capsule_logical) * sf).round() as i32).min(target.map_or(i32::MAX, |a| a.w.max(1)));
    let height = ((rec.height * sf).round() as i32).min(target.map_or(i32::MAX, |a| a.h.max(1)));
    let (x, y) = match rec.anchor {
        Anchor::Screen => {
            let origin = base.map(|a| (a.x, a.y)).unwrap_or((0, 0));
            let screen_sf = base.map_or(fallback_sf, |a| a.sf).max(0.5);
            (origin.0 + (rec.offset_x * screen_sf).round() as i32, origin.1 + (rec.offset_y * screen_sf).round() as i32)
        }
        Anchor::DockLeft | Anchor::DockBottom => {
            // 2.2.2（B2）：anchorBase 决定基准。缺省 window 保持 2.2.1 的行为；
            // capsule 让窗口贴可见胶囊；胶囊尺寸还没上报时回落到窗口。
            let reference = match rec.anchor_base {
                AnchorBase::Capsule => capsule.or(dock),
                AnchorBase::Window => dock,
            };
            match reference {
                Some((rx, ry, rw, rh)) => {
                    // 锚点：dock-left 取基准的左缘中点，dock-bottom 取基准的左下角。
                    // 这两个点与 2.2.1 的默认 align（right-center / left-top）配合，
                    // 算出来的位置与旧版完全一致。
                    let point = match rec.anchor {
                        Anchor::DockLeft => (rx, ry + rh / 2),
                        _ => (rx, ry + rh),
                    };
                    let point = (point.0 + (rec.offset_x * sf).round() as i32, point.1 + (rec.offset_y * sf).round() as i32);
                    rec.align.offset(point, (width, height))
                }
                // 没有 Dock 窗口（主岛还没建）时退到主显示器工作区中央，不让窗口跑到屏幕外。
                None => {
                    let area = screens.primary;
                    area.map(|a| (a.x + (a.w - width) / 2, a.y + (a.h - height) / 2)).unwrap_or((0, 0))
                }
            }
        }
    };
    let (x, y) = target.map_or((x,y), |area| (
        x.clamp(area.x, area.x + area.w.max(width) - width),
        y.clamp(area.y, area.y + area.h.max(height) - height),
    ));
    (x, y, width, height)
}

fn geometry_of(win: &WebviewWindow) -> Option<(i32, i32, i32, i32)> {
    match (win.outer_position(), win.outer_size()) {
        (Ok(position), Ok(size)) => Some((position.x, position.y, size.width as i32, size.height as i32)),
        _ => None,
    }
}

fn window_event(win: &WebviewWindow, event: &WindowEvent) {
    match event {
        WindowEvent::Moved(_) | WindowEvent::Resized(_) | WindowEvent::ScaleFactorChanged { .. } => geometry_changed(win),
        // 2.2.2（E2）：本窗口的焦点变化，向上发给插件。「点窗口外部」收不到，
        // 用这个近似——文档里写明。
        WindowEvent::Focused(on) => focus_changed(win, *on),
        WindowEvent::Destroyed => { let _ = close_label(win.label(), "destroyed"); }
        WindowEvent::CloseRequested { api, .. } => {
            // 点原生关闭按钮 = 用户关闭：真正销毁，不缩成小球、也不自动重开。
            api.prevent_close();
            let label = win.label().to_string();
            if let Err(error) = close_label(&label, "user") {
                log(&format!("close {label} failed: {error}"));
            }
        }
        _ => {}
    }
}

/// 2.2.2（E2）：窗口得到/失去焦点，发给插件。「点窗口外部」拿不到（独立原生
/// 窗口收不到窗口外的指针事件），插件只能用 blur 近似实现「点外面收起」。
fn focus_changed(win: &WebviewWindow, focused: bool) {
    let label = win.label();
    let target = {
        let book = book();
        book.windows.values().find(|rec| rec.label == label).map(|rec| (rec.plugin.clone(), rec.generation, rec.handle.clone()))
    };
    let Some((plugin, generation, handle)) = target else { return };
    let payload = json!({"event": if focused {"focus"} else {"blur"}, "handle": handle});
    emit(&plugin, generation, Some(label), payload.clone());
    let _ = notify_backend(&plugin, generation, payload);
}

fn geometry_changed(win: &WebviewWindow) {
    let Some(rect) = geometry_of(win) else { return };
    let label = win.label();
    let payload = {
        let mut book = book();
        match book.windows.values_mut().find(|rec| rec.label == label) {
            None => return,
            Some(rec) => {
                if rec.rect == Some(rect) {
                    return;
                }
                rec.rect = Some(rect);
                json!({"event":"anchorChanged","handle":rec.handle,"x":rect.0,"y":rect.1,"width":rect.2,"height":rect.3})
            }
        }
    };
    let plugin = {
        let book = book();
        book.windows.values().find(|rec| rec.label == label).map(|rec| (rec.plugin.clone(), rec.generation))
    };
    if let Some((plugin, generation)) = plugin {
        emit(&plugin, generation, Some(label), payload.clone());
        let _ = notify_backend(&plugin, generation, payload);
    }
}

// ---------------------------------------------------------------------------------------
// 宿主容器页面（src/window/）用的命令
// ---------------------------------------------------------------------------------------

/// 容器页面问「我这一页该加载什么」。只认登记过的容器标签，别的窗口问不到东西。
/// 这里再核一次「插件还在、还是同一版、还是同一代次」：创建与加载之间被停用/换版的
/// 窗口当场收掉（记录的摘要就是为此保存的），不把旧页面挂给新版本。
pub fn attach(label: &str) -> Result<Value, String> {
    let rec = {
        let book = book();
        book.by_label(label).cloned().ok_or("这个窗口没有对应的插件窗口")?
    };
    let context = crate::storage_context::current().ok_or("存储尚未就绪")?;
    let row = installed(&context, &rec.plugin)?;
    if row.digest != rec.digest || !generation_fresh(&rec.plugin, rec.generation) {
        let _ = close_handle(&rec.handle, "upgraded");
        return Err("插件已停用或换版，插件窗口已关闭".into());
    }
    if rec.ticket.is_empty() || rec.url.is_empty() {
        return Err("插件窗口页面尚未就绪".into());
    }
    Ok(json!({"handle":rec.handle,"ticket":rec.ticket,"url":rec.url,"generation":rec.generation,"title":rec.title,"transparent":rec.transparent}))
}

/// 容器页面报 ready：窗口这时才显示（页面 ready 之前一直隐藏）。
pub fn ready(label: &str) -> Result<Value, String> {
    let target = {
        let mut book = book();
        let rec = book.windows.values_mut().find(|rec| rec.label == label).ok_or("这个窗口没有对应的插件窗口")?;
        if rec.state == State::Ready {
            return Ok(view(rec));
        }
        rec.state = State::Ready;
        rec.shown = !rec.hidden_by_dock;
        (rec.handle.clone(), rec.plugin.clone(), rec.generation, rec.hidden_by_dock)
    };
    let (handle, plugin, generation, hidden) = target;
    READY.notify_all();
    let app = app()?.clone();
    if !hidden {
        let label = label.to_string();
        on_main(move || {
            let visible = book().by_label(&label).is_some_and(|rec| rec.state == State::Ready && rec.shown && !rec.hidden_by_dock);
            if visible {
                if let Some(win) = app.get_webview_window(&label) { show_without_activating(&win); }
            }
        })?;
    }
    emit(&plugin, generation, Some(label), json!({"event":"ready","handle":handle}));
    let _ = notify_backend(&plugin, generation, json!({"event":"ready","handle":handle}));
    log(&format!("ready {} {}", short(&handle), label));
    Ok(json!({"handle":handle}))
}

/// 容器页面的桥：插件页发来的 fish:call 全走这里。票据从登记表取，不信页面自报。
pub fn call_page(label: &str, method: &str, params: Value) -> Result<Value, String> {
    let ticket = {
        let book = book();
        book.by_label(label).map(|rec| rec.ticket.clone()).ok_or("这个窗口没有对应的插件窗口")?
    };
    if ticket.is_empty() {
        return Err("插件窗口页面尚未就绪".into());
    }
    crate::plugin_page::call(&ticket, method, params)
}

// ---------------------------------------------------------------------------------------
// 关闭、更新、列表
// ---------------------------------------------------------------------------------------

fn handle_of(params: &Value) -> Result<&str, String> {
    params.get("handle").and_then(Value::as_str).filter(|value| !value.is_empty()).ok_or_else(|| "缺少插件窗口句柄".to_string())
}

fn close_owned(plugin: &str, generation: u64, params: &Value, reason: &str) -> Result<Value, String> {
    let handle = handle_of(params)?.to_string();
    let rec = {
        let mut book = book();
        if !book.fresh(plugin, generation) { return Err("插件会话已撤销".into()); }
        match book.windows.get(&handle) {
            None => return Ok(json!({"closed":false,"handle":handle})),
            Some(rec) if rec.plugin != plugin || rec.generation != generation => return Err("这个插件窗口不属于当前会话".into()),
            Some(_) => book.windows.remove(&handle).unwrap(),
        }
    };
    destroy_record(rec, reason);
    Ok(json!({"closed":true,"handle":handle}))
}

/// 真正销毁一个窗口：名额、票据、拖入订阅、事件、WebView 一次性收干净。
pub fn close_handle(handle: &str, reason: &str) -> Result<(), String> {
    let rec = { book().windows.remove(handle) };
    let Some(rec) = rec else { return Ok(()) };
    destroy_record(rec, reason);
    Ok(())
}

fn destroy_record(rec: Rec, reason: &str) {
    crate::dropwin::changed();
    READY.notify_all();
    if !rec.ticket.is_empty() {
        crate::plugin_page::close(&rec.ticket);
    }
    if let Ok(app) = app() {
        let app = app.clone();
        let label = rec.label.clone();
        // 释放 WebView：调用 destroy 而不是 hide，绝不保留隐藏 renderer。停用/卸载
        // 路径可能正持着插件运行锁，所以这里只把动作排进主线程，不等它。
        post_main(move || {
            if let Some(win) = app.get_webview_window(&label) {
                let _ = win.destroy();
            }
        });
    }
    emit(&rec.plugin, rec.generation, Some(&rec.label), json!({"event":"closed","handle":rec.handle,"reason":reason}));
    let _ = notify_backend(&rec.plugin, rec.generation, json!({"event":"closed","handle":rec.handle,"reason":reason}));
    log(&format!("closed {} reason={reason}", short(&rec.handle)));
}

fn close_label(label: &str, reason: &str) -> Result<(), String> {
    let handle = book().by_label(label).map(|rec| rec.handle.clone());
    match handle {
        Some(handle) => close_handle(&handle, reason),
        None => Ok(()),
    }
}

fn apply_update(rec: &mut Rec, params: &Value) -> Result<(), String> {
        let mut candidate = rec.clone();
        validate_update(&mut candidate, params)?;
        *rec = candidate;
        Ok(())
}
fn validate_update(rec: &mut Rec, params: &Value) -> Result<(), String> {
        let object = params.as_object().ok_or("插件窗口更新参数应为对象")?;
        for name in object.keys() {
            if !["handle", "width", "height", "anchor", "offsetX", "offsetY", "title", "maxWidth", "align", "anchorBase", "zOrder"].contains(&name.as_str()) {
                return Err("插件窗口更新参数含未知字段".into());
            }
        }
        if let Some(width) = object.get("width") {
            // 2.2.2（C4）：update 也接受 'dock'，与 open 同一套语义。
            rec.width = match width {
                Value::String(text) if text == "dock" => Width::Dock { max: None },
                value => Width::Fixed(finite(value).filter(|v| (WIDTH_MIN..=WIDTH_MAX).contains(v)).ok_or("插件窗口宽度超出范围（或为 'dock'）")?),
            };
        }
        if let Some(limit) = object.get("maxWidth") {
            let value = finite(limit).filter(|v| (WIDTH_MIN..=WIDTH_MAX).contains(v)).ok_or("插件窗口 maxWidth 超出范围")?;
            rec.width = match rec.width {
                Width::Dock { .. } => Width::Dock { max: Some(value) },
                // 固定宽度下调 maxWidth：静默忽略，不让插件为此分支。
                other => other,
            };
        }
        if let Some(height) = object.get("height") {
            rec.height = finite(height).filter(|v| (HEIGHT_MIN..=HEIGHT_MAX).contains(v)).ok_or("插件窗口高度超出范围")?;
        }
        if let Some(anchor) = object.get("anchor") {
            rec.anchor = anchor.as_str().and_then(Anchor::parse).ok_or("插件窗口 anchor 只支持 dock-left、dock-bottom、screen")?;
        }
        if let Some(align) = object.get("align") {
            rec.align = align.as_str().and_then(Align::parse).ok_or("插件窗口 align 取值无效")?;
        }
        if let Some(base) = object.get("anchorBase") {
            rec.anchor_base = base.as_str().and_then(AnchorBase::parse).ok_or("插件窗口 anchorBase 只支持 window、capsule")?;
        }
        if let Some(offset) = object.get("offsetX") {
            rec.offset_x = finite(offset).filter(|v| v.abs() <= OFFSET_LIMIT).ok_or("插件窗口 offsetX 超出范围")?;
        }
        if let Some(offset) = object.get("offsetY") {
            rec.offset_y = finite(offset).filter(|v| v.abs() <= OFFSET_LIMIT).ok_or("插件窗口 offsetY 超出范围")?;
        }
        if let Some(z_order) = object.get("zOrder") {
            rec.z_order = z_order.as_str().and_then(ZOrder::parse).ok_or("插件窗口 zOrder 只支持 normal、above-dock")?;
        }
        if let Some(title) = object.get("title") {
            let text = title.as_str().map(str::trim).filter(|t| !t.is_empty() && t.chars().count() <= TITLE_MAX && !t.chars().any(char::is_control)).ok_or("插件窗口标题无效")?;
            rec.title = text.to_string();
        }
    Ok(())
}

fn update(context: &Context, plugin: &str, generation: u64, params: &Value) -> Result<Value, String> {
    let handle = handle_of(params)?.to_string();
    let snapshot = {
        let mut book = book();
        let current = book.sessions.get(plugin).copied();
        let rec = book.windows.get_mut(&handle).ok_or("插件窗口不存在或已关闭")?;
        if rec.plugin != plugin {
            return Err("这个插件窗口不属于当前插件".into());
        }
        if current != Some(generation) || rec.generation != generation {
            return Err("插件实例代次已变化，本次窗口更新已丢弃".into());
        }
        apply_update(rec, params)?;
        rec.clone()
    };
    // 资源所有者（plugin/digest/page/ticket）不在这里的写入范围里：update 只能动自己窗口的几何。
    let app = app()?.clone();
    let for_main = snapshot.clone();
    on_main(move || -> Result<(), String> {
        // 排队期间可能已有更新或撤销；只应用登记表里的当前值。
        let current = {
            let book = book();
            book.windows.get(&for_main.handle).filter(|rec| book.fresh(&rec.plugin, rec.generation)).cloned()
        }.ok_or("窗口已关闭或会话已撤销")?;
        let win = app.get_webview_window(&current.label).ok_or("插件窗口尚未创建")?;
        win.set_title(&current.title).map_err(|_| "插件窗口标题设置失败")?;
        place(&win, &current)?;
        Ok(())
    })??;
    let _ = context;
    Ok(view(&snapshot))
}

fn list(context: &Context, plugin: &str, generation: u64) -> Result<Value, String> {
    let _ = context;
    let book = book();
    if !book.fresh(plugin, generation) { return Err("插件会话已撤销".into()); }
    let rows: Vec<Value> = book.windows.values().filter(|rec| rec.plugin == plugin && rec.generation == generation).map(view).collect();
    Ok(json!({"windows":rows}))
}

fn view(rec: &Rec) -> Value {
    // 2.2.2（C4）：width:'dock' 对外报成 "dock"，固定宽度报数字——插件据此判断
    // 宿主有没有接管宽度，不必自己猜。
    let width = match rec.width {
        Width::Fixed(value) => json!(value),
        Width::Dock { .. } => json!("dock"),
    };
    json!({
        "handle":rec.handle,
        "key":rec.key,
        "page":rec.page,
        "title":rec.title,
        "anchor":rec.anchor.name(),
        "align":rec.align.name(),
        "anchorBase":rec.anchor_base.name(),
        "width":width,
        "height":rec.height,
        "maxWidth":match rec.width { Width::Dock { max } => json!(max), _ => Value::Null },
        "transparent":rec.transparent,
        "ready":rec.state == State::Ready,
        "visible":rec.shown && !rec.hidden_by_dock,
    })
}

fn short(handle: &str) -> String {
    handle.chars().take(8).collect()
}

// ---------------------------------------------------------------------------------------
// 文件拖入订阅
// ---------------------------------------------------------------------------------------

fn subscribe(context: &Context, plugin: &str, generation: u64, params: &Value) -> Result<Value, String> {
    let result = subscribe_inner(context, plugin, generation, params);
    crate::dropwin::changed();
    result
}
fn subscribe_inner(context: &Context, plugin: &str, generation: u64, params: &Value) -> Result<Value, String> {
    let row = installed(context, plugin)?;
    require_capability(&row, "file-drop", "插件未声明 file-drop 能力")?;
    let scope = params.get("scope").and_then(Value::as_str).unwrap_or("dock");
    match scope {
        "dock" => {
            let mut book = book();
            if !book.fresh(plugin, generation) { return Err("插件会话已撤销".into()); }
            // 主 Dock 独占：已经有一个**还在**的插件占着就明确报冲突，不广播、不改绑。
            if let Some(current) = book.dock.clone() {
                if current.plugin != plugin && book.fresh(&current.plugin, current.generation) {
                    return Err(format!("主 Dock 拖入已被插件 {} 占用，请先解除它的订阅", current.plugin));
                }
                // 同一个插件、同一代次：返回原句柄（重复订阅幂等）；换了代次（重拉过实例）
                // 就换成新的：旧代次的句柄交不回任何路径。
                if current.plugin == plugin && current.generation == generation {
                    return Ok(json!({"handle":current.handle,"scope":"dock"}));
                }
            }
            let handle = format!("drop-{}", token()?);
            book.dock = Some(DockSub { handle: handle.clone(), plugin: plugin.into(), generation });
            log(&format!("dock file drop subscribed by {plugin}"));
            Ok(json!({"handle":handle,"scope":"dock"}))
        }
        "window" => {
            let target = handle_of(params)?.to_string();
            let mut book = book();
            let current = book.sessions.get(plugin).copied();
            let rec = book.windows.get_mut(&target).ok_or("插件窗口不存在或已关闭")?;
            if rec.plugin != plugin {
                return Err("这个插件窗口不属于当前插件".into());
            }
            if current != Some(generation) || rec.generation != generation {
                return Err("插件实例代次已变化，本次订阅已丢弃".into());
            }
            if let Some(handle) = rec.drop.clone() { return Ok(json!({"handle":handle,"scope":"window","window":target})); }
            let handle = format!("drop-{}", token()?);
            rec.drop = Some(handle.clone());
            Ok(json!({"handle":handle,"scope":"window","window":target}))
        }
        _ => Err("文件拖入订阅范围只支持 dock 或 window".into()),
    }
}

fn unsubscribe(plugin: &str, generation: u64, params: &Value) -> Result<Value, String> {
    let result = unsubscribe_inner(plugin, generation, params);
    crate::dropwin::changed();
    result
}
fn unsubscribe_inner(plugin: &str, generation: u64, params: &Value) -> Result<Value, String> {
    let handle = handle_of(params)?.to_string();
    let mut book = book();
    if !book.fresh(plugin, generation) { return Err("插件会话已撤销".into()); }
    // 先按 Dock 订阅找：同一个句柄只属于一个订阅者。
    if let Some(current) = book.dock.clone() {
        if current.handle == handle {
            if current.plugin != plugin {
                return Err("这个拖入订阅不属于当前插件".into());
            }
            book.dock = None;
            log(&format!("dock file drop unsubscribed by {plugin}"));
            return Ok(json!({"unsubscribed":true}));
        }
    }
    let mut cleared = false;
    for rec in book.windows.values_mut() {
        if rec.plugin != plugin {
            continue;
        }
        // 句柄必须**逐字**相等：不接受前缀/子串匹配，免得一个句柄解除别人的订阅。
        if rec.drop.as_deref() == Some(handle.as_str()) {
            rec.drop = None;
            cleared = true;
        }
    }
    if cleared {
        return Ok(json!({"unsubscribed":true}));
    }
    // 已经解除过的句柄按幂等处理，不把重复调用变成错误。
    Ok(json!({"unsubscribed":false}))
}

// ---------------------------------------------------------------------------------------
// 定向交付与事件
// ---------------------------------------------------------------------------------------

fn emit(plugin: &str, generation: u64, label: Option<&str>, payload: Value) {
    let Ok(app) = app() else { return };
    if !generation_fresh(plugin, generation) { return; }
    let mut full = payload;
    if let Some(object) = full.as_object_mut() {
        object.insert("plugin".into(), Value::String(plugin.into()));
        object.insert("generation".into(), json!(generation));
    }
    // 本窗口自己的容器页先收（页面里的插件页通过它转发）；island 里的同一插件页面
    // 也收——extension-page.js 只转给这个插件的帧，别的插件收不到。
    if let Some(label) = label {
        let _ = app.emit_to(label, "plugin:window-event", full.clone());
    }
    let _ = app.emit_to("island", "plugin:window-event", full);
}

fn notify_backend(plugin: &str, generation: u64, payload: Value) -> Result<(), String> {
    let output = {
        let book = book();
        if !book.fresh(plugin, generation) { return Err("插件会话已撤销，事件未交付".into()); }
        book.outputs.get(plugin).cloned().ok_or("插件事件通道已关闭")?
    };
    send_backend(&output, plugin, payload)
}
fn send_backend(output: &std::sync::mpsc::SyncSender<Value>, plugin: &str, payload: Value) -> Result<(), String> {
    // 字符串编号独立于 Process 的整数 RPC 编号；try_send 不等待忙实例或停用锁。
    let sequence = TICK.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let frame = json!({"id":format!("window-event-{sequence}"),"method":"plugin.windowEvent","params":payload});
    if serde_json::to_vec(&frame).map_err(|_|"插件事件编码失败")?.len() as u64 >= crate::plugin_protocol::MAX_LINE {
        return Err("插件事件超过协议限制，未交付".into());
    }
    output.try_send(frame).map_err(|_| {
        log(&format!("backend event queue unavailable for {plugin}; event discarded"));
        "插件事件队列已满或关闭，未交付".into()
    })
}

pub fn has_drop_subscribers() -> bool {
    let book = book();
    book.dock.as_ref().is_some_and(|sub| book.fresh(&sub.plugin, sub.generation)) ||
        book.windows.values().any(|rec| rec.drop.is_some() && book.fresh(&rec.plugin, rec.generation))
}

/// 主 Dock 的拖入订阅者（没有就说明谁都没申请，dropwin 不启用捕获）。
pub fn dock_subscription() -> Option<String> {
    let book = book();
    let sub = book.dock.as_ref()?;
    if book.dock_hidden || !book.fresh(&sub.plugin, sub.generation) { return None; }
    Some(sub.handle.clone())
}
fn dock_subscriber(subscription: &str) -> Option<(String, u64)> {
    let book = book();
    let sub = book.dock.as_ref()?;
    if sub.handle != subscription || book.dock_hidden || !book.fresh(&sub.plugin, sub.generation) {
        return None;
    }
    Some((sub.plugin.clone(), sub.generation))
}

/// 现在真正接受拖入的窗口区域（屏幕物理像素）。没有订阅的窗口不在此列。
pub fn drop_rects() -> Vec<(String, (i32, i32, i32, i32))> {
    let book = book();
    book.windows
        .values()
        .filter(|rec| rec.drop.is_some() && rec.state == State::Ready && rec.shown && !rec.hidden_by_dock)
        .filter(|rec| book.fresh(&rec.plugin, rec.generation))
        .filter_map(|rec| rec.rect.map(|rect| (rec.drop.clone().unwrap(), rect)))
        .collect()
}

/// 接收线程获取标签快照后在锁外读 HWND，避免持 BOOK 等主线程。
#[cfg(windows)]
pub fn drop_labels() -> Vec<(String,String)> {
    let book = book();
    book.windows.values().filter(|rec| rec.state==State::Ready && rec.shown && !rec.hidden_by_dock && book.fresh(&rec.plugin,rec.generation))
        .filter_map(|rec|rec.drop.as_ref().map(|token|(token.clone(),rec.label.clone()))).collect()
}

pub fn enter_window(subscription: &str) { hover_window(subscription, "enter"); }
pub fn leave_window(subscription: &str) { hover_window(subscription, "leave"); }
fn hover_window(subscription: &str, phase: &str) {
    let target = {
        let book = book();
        book.windows.values().find(|rec| rec.drop.as_deref() == Some(subscription) && rec.state == State::Ready && rec.shown && !rec.hidden_by_dock && book.fresh(&rec.plugin, rec.generation)).cloned()
    };
    if let Some(rec) = target {
        let payload = json!({"event":"fileDrop","phase":phase,"handle":rec.handle,"scope":"window"});
        emit(&rec.plugin, rec.generation, Some(&rec.label), payload.clone());
        let _ = notify_backend(&rec.plugin, rec.generation, payload);
    }
}

pub fn enter_dock(subscription: &str) {
    let Some((plugin, generation)) = dock_subscriber(subscription) else { return };
    let payload = json!({"event":"fileDrop","phase":"enter","scope":"dock","handle":subscription});
    emit(&plugin, generation, None, payload.clone());
    let _ = notify_backend(&plugin, generation, payload);
}

pub fn leave_dock(subscription: &str) {
    let Some((plugin, generation)) = dock_subscriber(subscription) else { return };
    let payload = json!({"event":"fileDrop","phase":"leave","scope":"dock","handle":subscription});
    emit(&plugin, generation, None, payload.clone());
    let _ = notify_backend(&plugin, generation, payload);
}

/// 在分配原生路径缓冲区及构造事件前统一限额。每 UTF-16 单元最多产生 6 个 JSON 字节，
/// 另预留帧头空间；整批拒绝，不截断文件集合后冒充完整拖入。
pub const MAX_DROP_FILES: usize = 4096;
pub fn drop_budget(used: usize, units: usize) -> Result<usize, String> {
    used.checked_add(units).and_then(|n|n.checked_add(3))
        .filter(|n|*n <= (crate::plugin_protocol::MAX_LINE as usize - 65536) / 6)
        .ok_or_else(||"文件拖入超过协议限制，本次未交付".into())
}
fn validate_drop(paths: &[String]) -> Result<(), String> {
    if paths.len()>MAX_DROP_FILES { return Err("文件拖入超过协议限制，本次未交付".into()); }
    paths.iter().try_fold(0,|used,path|drop_budget(used,path.encode_utf16().count()))?;
    Ok(())
}

/// 交付窗口拖入：只给这个窗口的所有者。窗口已关/已隐藏/换版时丢弃，不转给别人。
pub fn deliver_window(handle: &str, paths: &[String]) -> Result<(), String> {
    deliver_window_at(handle, paths, None)
}

/// 2.2.2（E1）：带落点坐标的窗口拖入。`point` 是屏幕物理坐标（光标位置）。
pub fn deliver_window_at(handle: &str, paths: &[String], point: Option<(i32, i32)>) -> Result<(), String> {
    validate_drop(paths)?;
    let target = {
        let book = book();
        let rec = book.windows.values().find(|rec| rec.drop.as_deref() == Some(handle)).ok_or_else(|| "这个窗口没有拖入订阅".to_string())?;
        if !book.fresh(&rec.plugin, rec.generation) || rec.state != State::Ready || !rec.shown || rec.hidden_by_dock {
            return Err("窗口已关闭、隐藏或实例已变化，本次拖入已丢弃".into());
        }
        (rec.plugin.clone(), rec.label.clone(), rec.handle.clone(), rec.generation, rec.rect)
    };
    let (plugin, label, handle, generation, rect) = target;
    log(&format!("file drop -> {} path(s) for {plugin} window {}", paths.len(), short(&handle)));
    let payload = json!({"event":"fileDrop","phase":"drop","handle":handle,"scope":"window","paths":paths,
        "position":drop_position(point, rect)});
    emit(&plugin, generation, Some(&label), payload.clone());
    notify_backend(&plugin, generation, payload)
}

/// 2.2.2（E1）：落点坐标。`screen` 是物理像素；`client` 是相对这个窗口的物理像素
/// （减去窗口原点），插件可直接用于页面内定位。拿不到任一值就整体给 null。
fn drop_position(point: Option<(i32, i32)>, rect: Option<(i32, i32, i32, i32)>) -> Value {
    let Some((x, y)) = point else { return Value::Null };
    let client = rect.map(|(rx, ry, _, _)| json!({"x": x - rx, "y": y - ry}));
    json!({
        "screen": {"x": x, "y": y},
        "client": client.unwrap_or(Value::Null),
    })
}

/// 交付主 Dock 拖入：只给订阅者（独占）。没有订阅者时不上报，也不广播。
pub fn deliver_dock(subscription: &str, paths: &[String]) -> Result<(), String> {
    deliver_dock_at(subscription, paths, None)
}

/// 2.2.2（E1）：带落点坐标的主 Dock 拖入。
pub fn deliver_dock_at(subscription: &str, paths: &[String], point: Option<(i32, i32)>) -> Result<(), String> {
    validate_drop(paths)?;
    let (plugin, generation, handle) = {
        let book = book();
        let sub = book.dock.clone().ok_or_else(|| "主 Dock 拖入没有订阅者".to_string())?;
        if sub.handle != subscription || book.dock_hidden || !book.fresh(&sub.plugin, sub.generation) { return Err("拖入订阅已更换或隐藏，本次拖入已丢弃".into()); }
        (sub.plugin, sub.generation, sub.handle)
    };
    if !generation_fresh(&plugin, generation) {
        return Err("订阅者的实例已变化，本次拖入已丢弃".into());
    }
    log(&format!("file drop -> {} path(s) for {plugin} dock", paths.len()));
    // 主 Dock 订阅没有对应窗口，`client` 以胶囊矩形为参照。
    let capsule = app().ok().map(|app| crate::island::snapshot(&crate::shared(app)).capsule).flatten();
    let payload = json!({"event":"fileDrop","phase":"drop","handle":handle,"scope":"dock","paths":paths,
        "position":drop_position(point, capsule)});
    emit(&plugin, generation, None, payload.clone());
    notify_backend(&plugin, generation, payload)
}

// ---------------------------------------------------------------------------------------
// 生命周期：停用/卸载/换版/崩溃/退出/Dock 隐藏
// ---------------------------------------------------------------------------------------

/// 撤销一个插件的全部窗口与订阅（停用、卸载、升级、崩溃、上下文切换都走这里）。
/// 调用方通常正持着 plugin_runtime 的锁，所以这里只做内存操作 + 把销毁排到主线程。
pub fn revoke_plugin(plugin: &str, reason: &str) {
    revoke_generation(plugin, None, reason);
}
fn revoke_generation(plugin: &str, generation: Option<u64>, reason: &str) {
    let (retired, output) = {
        let mut book = book();
        if generation.is_some_and(|generation| !book.fresh(plugin, generation)) { return; }
        let output = book.outputs.get(plugin).cloned();
        (book.retire(plugin, generation), output)
    };
    crate::dropwin::changed();
    for rec in retired {
        terminal_event(&rec, reason, output.as_ref());
        destroy_record(rec, reason);
    }
}

// 撤销后仅向撤销瞬间捕获的旧会话通知；不查新会话，永不把旧事件送给替代实例。
fn terminal_event(rec: &Rec, reason: &str, output: Option<&std::sync::mpsc::SyncSender<Value>>) {
    let payload = json!({"event":"closed","handle":rec.handle,"reason":reason});
    if let Some(output) = output { let _ = send_backend(output, &rec.plugin, payload.clone()); }
    if let Ok(app) = app() {
        let mut full = payload;
        full["plugin"] = json!(rec.plugin);
        full["generation"] = json!(rec.generation);
        let _ = app.emit_to(&rec.label, "plugin:window-event", full.clone());
        let _ = app.emit_to("island", "plugin:window-event", full);
    }
}
/// 用户只关闭窗口，插件会话继续运行。会话仍有效，destroy_record 按当前会话
/// 正常通知一次；这里不再叠加 terminal_event（那是给已撤销旧会话的补发，#83）。
pub fn close_windows(reason: &str) {
    let retired = std::mem::take(&mut book().windows);
    crate::dropwin::changed();
    for (_, rec) in retired {
        destroy_record(rec, reason);
    }
}

/// 宿主退出：全部窗口与订阅一起收（不放任它们留到下一次启动）。
pub fn close_all(reason: &str) {
    let (retired, outputs) = {
        let mut book = book();
        book.sessions.clear();
        book.dock = None;
        (std::mem::take(&mut book.windows), std::mem::take(&mut book.outputs))
    };
    crate::dropwin::changed();
    for (_, rec) in retired {
        terminal_event(&rec, reason, outputs.get(&rec.plugin));
        destroy_record(rec, reason);
    }
}

pub fn count() -> usize {
    book().windows.len()
}

/// Dock 全局隐藏 / 全屏避让：插件窗口随之隐藏并停止接受拖入；恢复时只恢复那一批
/// 本来还开着的窗口（用户自己关掉的不复活）。主岛普通收起（缩成胶囊）不算全局隐藏，
/// 那时窗口照常留着——screen 窗口更不跟随主岛的展开/收起。
pub fn set_dock_hidden(hidden: bool) {
    let changes: Vec<(String, String)> = {
        let mut book = book();
        book.dock_hidden = hidden;
        let mut changes = Vec::new();
        for rec in book.windows.values_mut() {
            if !hidden {
                if rec.hidden_by_dock {
                    rec.hidden_by_dock = false;
                    rec.shown = rec.state == State::Ready;
                    if rec.shown { changes.push((rec.label.clone(), rec.handle.clone())); }
                }
            } else if !rec.hidden_by_dock {
                rec.hidden_by_dock = true;
                if rec.shown { changes.push((rec.label.clone(), rec.handle.clone())); }
            }
        }
        changes
    };
    if changes.is_empty() {
        return;
    }
    let Ok(app) = app() else { return };
    let app = app.clone();
    let labels: Vec<String> = changes.iter().map(|(label, _)| label.clone()).collect();
    post_main(move || {
        for label in &labels {
            let visible = {
                let book = book();
                let Some(rec) = book.by_label(label) else { continue; };
                rec.state == State::Ready && rec.shown && !rec.hidden_by_dock
            };
            if let Some(win) = app.get_webview_window(label) {
                if visible {
                    show_without_activating(&win);
                } else {
                    let _ = win.hide();
                }
            }
        }
    });
}

/// 主 Dock 移动/换屏/DPI 变化之后重排所有锚定窗口（不通过扩大主岛来容纳）。
/// 只在 Dock 的外框真的变了才做：主岛的移动/尺寸事件很密，重复重排既费主线程也没意义。
pub fn dock_changed() {
    let rect = app().ok().and_then(|app| app.get_webview_window("island")).and_then(|island| geometry_of(&island));
    let Some(rect) = rect else { return };
    {
        let mut last = DOCK_RECT.lock().unwrap_or_else(|poison| poison.into_inner());
        if *last == Some(rect) {
            return;
        }
        *last = Some(rect);
    }
    rearrange(false);
}

/// 显示器、DPI、主屏或工作区快照改变时，screen 锚点也必须重新限制在工作区。
/// 与 dock_changed 分开，避免主岛普通移动把用户拖动的 screen 窗口吸回去。
pub fn screens_changed() {
    rearrange(true);
}

// ---------------------------------------------------------------------------------------
// 2.2.2：主岛几何与状态订阅（dock.onEvent）
// ---------------------------------------------------------------------------------------

/// 订阅了 `dock.onEvent` 的插件数。**这是「按需启用」的唯一开关**：
/// 计数为 0 时主岛页面不上报任何东西（§1.3），因此不装插件、装了不启用、
/// 或启用但没订阅的用户，新增代码一行都不执行。
static DOCK_WATCHERS: AtomicUsize = AtomicUsize::new(0);
/// 上一次推给插件的快照，用来去重：只在真的变了才发事件。
static LAST_DOCK: Mutex<Option<crate::island::DockSnapshot>> = Mutex::new(None);

/// 有没有插件在听主岛事件。主岛页面据此决定要不要上报（`island:watch-dock`）。
pub fn dock_watchers() -> usize {
    DOCK_WATCHERS.load(Ordering::SeqCst)
}

/// 订阅/退订 `dock.onEvent`。从 0 变 1 时通知页面开始上报，回到 0 时停止。
fn watch_dock(add: bool) {
    let before = if add {
        DOCK_WATCHERS.fetch_add(1, Ordering::SeqCst)
    } else {
        let current = DOCK_WATCHERS.load(Ordering::SeqCst);
        if current == 0 {
            0
        } else {
            DOCK_WATCHERS.fetch_sub(1, Ordering::SeqCst) - 1
        }
    };
    let after = DOCK_WATCHERS.load(Ordering::SeqCst);
    // 只在跨过 0 边界时告诉页面：1 → 2 不必再说一次。
    if (before == 0) != (after == 0) {
        if after == 0 {
            *LAST_DOCK.lock().unwrap_or_else(|p| p.into_inner()) = None;
        }
        tell_page_watch(after > 0);
    }
}

/// 告诉主岛页面开/关按需上报。页面未创建时静默失败（它起来后会自己问一次）。
fn tell_page_watch(on: bool) {
    let Ok(app) = app() else { return };
    let _ = app.emit_to("island", "island:watch-dock", json!({"on": on}));
}

/// 主岛的几何或状态变了：重排锚定窗口，并把新快照推给订阅者。
/// 由 `island.rs` 在胶囊尺寸、视图形态、stow、全屏避让变化时调用。
pub fn dock_state_changed() {
    let Ok(app) = app() else { return };
    let shared = crate::shared(app);
    let snapshot = crate::island::snapshot(&shared);
    {
        let mut last = LAST_DOCK.lock().unwrap_or_else(|p| p.into_inner());
        if last.as_ref() == Some(&snapshot) {
            return;
        }
        *last = Some(snapshot.clone());
    }
    rearrange(false);
    broadcast_dock(&snapshot);
}

/// 把快照推给每个订阅了 dock 事件的插件（按所有者定向，不广播给没订阅的）。
fn broadcast_dock(snapshot: &crate::island::DockSnapshot) {
    let targets: Vec<(String, u64)> = {
        let book = book();
        book.dock_watchers.iter().filter(|(plugin, generation)| book.fresh(plugin, **generation)).map(|(plugin, generation)| (plugin.clone(), *generation)).collect()
    };
    if targets.is_empty() {
        return;
    }
    let payload = json!({"event":"dock","type":"geometry","rect":dock_rect_value(snapshot)});
    for (plugin, generation) in targets {
        emit(&plugin, generation, None, payload.clone());
        let _ = notify_backend(&plugin, generation, payload.clone());
    }
}

/// 2.2.2（C5）：指针进入/离开主岛胶囊。由 `island.rs` 的轮询在跨边界时调用。
pub fn dock_pointer_changed(on: bool) {
    let targets: Vec<(String, u64)> = {
        let book = book();
        book.dock_watchers.iter().filter(|(plugin, generation)| book.fresh(plugin, **generation)).map(|(plugin, generation)| (plugin.clone(), *generation)).collect()
    };
    if targets.is_empty() {
        return;
    }
    let payload = json!({"event":"dock","type":"pointer","action": if on {"enter"} else {"leave"}});
    for (plugin, generation) in targets {
        emit(&plugin, generation, None, payload.clone());
        let _ = notify_backend(&plugin, generation, payload.clone());
    }
}

/// 把快照整理成插件看到的形状。`window` 与 `capsule` 都是物理像素。
pub fn dock_rect_value(snapshot: &crate::island::DockSnapshot) -> Value {
    let rect = |value: Option<(i32, i32, i32, i32)>| match value {
        Some((x, y, width, height)) => json!({"x":x,"y":y,"width":width,"height":height}),
        None => Value::Null,
    };
    json!({
        "window": rect(snapshot.window),
        "capsule": rect(snapshot.capsule),
        "state": {
            "view": snapshot.view,
            "stowed": snapshot.stowed,
            "hidden": snapshot.hidden,
            "fullscreen": snapshot.fullscreen,
        },
        "scale": snapshot.scale,
    })
}

/// `dock.rect()`：读一次实时快照。页面与后端两条入口共用。
fn dock_rect() -> Result<Value, String> {
    let app = app()?;
    let shared = crate::shared(app);
    Ok(dock_rect_value(&crate::island::snapshot(&shared)))
}

/// 2.2.2：主岛几何/状态接口要求插件声明 `dock` 能力（只读，但与外部窗口
/// 分开授权——不需要开窗的插件也能只要几何）。
fn require_dock(context: &Context, plugin: &str) -> Result<(), String> {
    let row = installed(context, plugin)?;
    require_capability(&row, "dock", "插件未声明 dock 能力")
}

fn dock_rect_for(context: &Context, plugin: &str) -> Result<Value, String> {
    require_dock(context, plugin)?;
    dock_rect()
}

fn subscribe_dock_for(context: &Context, plugin: &str, generation: u64) -> Result<Value, String> {
    require_dock(context, plugin)?;
    subscribe_dock(plugin, generation)
}

/// `dock.onEvent(listener)` 的后端侧登记：订阅即计入 watcher。
fn subscribe_dock(plugin: &str, generation: u64) -> Result<Value, String> {
    {
        let mut book = book();
        if !book.fresh(plugin, generation) {
            return Err("插件会话已撤销".into());
        }
        if book.dock_watchers.insert(plugin.to_string(), generation).is_some() {
            // 同一个插件重复订阅：幂等，不再加计数。
            return Ok(json!({"subscribed":true}));
        }
    }
    watch_dock(true);
    Ok(json!({"subscribed":true}))
}

fn unsubscribe_dock(plugin: &str) -> Result<Value, String> {
    let removed = { book().dock_watchers.remove(plugin).is_some() };
    if removed {
        watch_dock(false);
    }
    Ok(json!({"subscribed":false}))
}

fn rearrange(include_screen: bool) {
    let targets: Vec<Rec> = { book().windows.values().filter(|rec| include_screen || rec.anchor != Anchor::Screen).cloned().collect() };
    if targets.is_empty() {
        return;
    }
    let Ok(app) = app() else { return };
    let app = app.clone();
    post_main(move || {
        for requested in targets {
            let current = {
                let book = book();
                book.windows.get(&requested.handle).filter(|rec| (include_screen || rec.anchor != Anchor::Screen) && book.fresh(&rec.plugin, rec.generation)).cloned()
            };
            if let Some(rec) = current {
                if let Some(win) = app.get_webview_window(&rec.label) { let _ = place(&win, &rec); }
            }
        }
    });
}

/// 托盘与容器页的 Escape：用户关闭。
pub fn close_label_as_user(label: &str) -> Result<(), String> {
    close_label(label, "user")
}

/// 回给插件后端的错误形状：码固定，文案只在可安全上行时原样给（长度与控制字符
/// 照 plugin_protocol::answer 的判据），否则退回一句通用说明——绝不夹带文件路径。
pub fn failure(error: &str) -> Value {
    if !error.is_empty() && error.len() <= 600 && !error.chars().any(char::is_control) {
        json!({"code":"WINDOW_OPERATION_FAILED","message":error})
    } else {
        json!({"code":"WINDOW_OPERATION_FAILED","message":"插件窗口操作失败"})
    }
}

// ---------------------------------------------------------------------------------------
// 平台：显示但不抢焦点
// ---------------------------------------------------------------------------------------

#[cfg(windows)]
fn show_without_activating(win: &WebviewWindow) {
    type Hwnd = *mut std::ffi::c_void;
    extern "system" {
        fn SetWindowPos(hwnd: Hwnd, after: isize, x: i32, y: i32, cx: i32, cy: i32, flags: u32) -> i32;
    }
    const SWP_NOSIZE: u32 = 0x0001;
    const SWP_NOMOVE: u32 = 0x0002;
    const SWP_NOACTIVATE: u32 = 0x0010;
    const SWP_SHOWWINDOW: u32 = 0x0040;
    match win.hwnd() {
        Ok(hwnd) => {
            // 显示但不激活：当前主界面的焦点不被抢走；用户点进来时仍能正常获焦。
            unsafe {
                SetWindowPos(hwnd.0 as Hwnd, 0, 0, 0, 0, 0, SWP_NOSIZE | SWP_NOMOVE | SWP_NOACTIVATE | SWP_SHOWWINDOW);
            }
        }
        Err(_) => {
            let _ = win.show();
        }
    }
}

#[cfg(not(windows))]
fn show_without_activating(win: &WebviewWindow) {
    let _ = win.show();
}

// ---------------------------------------------------------------------------------------
// Tauri 命令（容器页面用）
// ---------------------------------------------------------------------------------------

#[tauri::command]
pub fn plugin_window_attach(webview: tauri::Webview) -> Result<Value, String> {
    attach(webview.label())
}

#[tauri::command]
pub fn plugin_window_ready(webview: tauri::Webview) -> Result<Value, String> {
    ready(webview.label())
}

#[tauri::command]
pub async fn plugin_window_call(webview: tauri::Webview, method: String, params: Value) -> Result<Value, String> {
    // 只接受插件页会用的两类方法名：宿主保留的 dock.* 与业务方法（不允许 plugin./host.）。
    if method.is_empty() || method.len() > 100 || method.starts_with("host.") || method.starts_with("plugin.") {
        return Err("插件窗口请求的方法无效".into());
    }
    if !method.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'-' || b == b'_') {
        return Err("插件窗口请求的方法无效".into());
    }
    let label = webview.label().to_string();
    tauri::async_runtime::spawn_blocking(move || call_page(&label, &method, params))
        .await.map_err(|_| "插件窗口请求意外结束")?
}

/// 容器页面的 Escape / 关闭按钮 / 托盘出口：真正销毁窗口。
#[tauri::command]
pub fn plugin_window_close(webview: tauri::Webview) -> Result<Value, String> {
    close_label_as_user(webview.label())?;
    Ok(json!({"closed":true}))
}

// ---------------------------------------------------------------------------------------
// 单元测试（纯逻辑：参数、配额、隔离、拖入路由不碰真实窗口）
// ---------------------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn options(value: Value) -> Result<Options, String> {
        Options::parse(&value)
    }

    #[test]
    fn options_carry_defaults_and_only_known_fields() {
        let parsed = options(json!({"key":"panel","page":"main"})).unwrap();
        assert_eq!((parsed.width, parsed.height), (Width::Fixed(WIDTH_DEFAULT), HEIGHT_DEFAULT));
        assert_eq!(parsed.anchor, Anchor::DockBottom);
        assert_eq!((parsed.offset_x, parsed.offset_y), (0.0, 0.0));
        assert!(!parsed.transparent);
        // 未知字段直接拒绝：插件不能借「顺手多传一个」去改宿主不认识的语义。
        assert_eq!(options(json!({"key":"panel","page":"main","owner":"fish.other"})).unwrap_err(), "插件窗口参数含未知字段");
        // 身份不接受插件自报（§4.1）：多传 pluginId 也只会被当成未知字段。
        assert!(options(json!({"key":"panel","page":"main","pluginId":"fish.other"})).is_err());
    }

    #[test]
    fn options_reject_bad_names_sizes_anchors_and_floats() {
        for bad in [json!({"page":"main"}), json!({"key":"","page":"main"}), json!({"key":"../escape","page":"main"}), json!({"key":"Panel","page":"main"}), json!({"key":"panel"}), json!({"key":"panel","page":"main/../x"})] {
            assert!(options(bad.clone()).is_err(), "{bad}");
        }
        for bad in [json!({"width":10}), json!({"height":10}), json!({"width":99999}), json!({"anchor":"dock-right"}), json!({"offsetX":1e9}), json!({"transparent":"yes"}), json!({"title":""}), json!({"title":"x".repeat(81)}), json!({"title":42}), json!({"anchor":42})] {
            let mut value = json!({"key":"panel","page":"main"});
            value.as_object_mut().unwrap().extend(bad.as_object().unwrap().clone());
            assert!(options(value.clone()).is_err(), "{value}");
        }
        // 有限数：NaN/Infinity 在 JSON 里表达不了，用字符串与非数拒绝。
        assert!(options(json!({"key":"panel","page":"main","width":"420"})).is_err());
        let ok = options(json!({"key":"panel","page":"main","width":600.5,"height":200,"anchor":"screen","offsetX":-1200,"offsetY":40,"transparent":true,"title":"文件暂存"})).unwrap();
        assert_eq!(ok.anchor, Anchor::Screen);
        assert!(ok.transparent);
        assert_eq!(ok.offset_x, -1200.0);
    }

    // 配额与「同 key 复用」是两条独立判据：都必须在 BOOK 里成立。
    #[test]
    fn quota_is_per_plugin_and_global() {
        let mut book = Book { windows: BTreeMap::new(), dock: None, next: 0, sessions: BTreeMap::new(), outputs: BTreeMap::new(), dock_hidden: false, dock_watchers: BTreeMap::new() };
        assert_eq!(book.per_plugin("acme.weather"), 0);
        for index in 0..PER_PLUGIN {
            book.windows.insert(format!("h{index}"), Rec {
                handle: format!("h{index}"), plugin: "acme.weather".into(), digest: "d".into(), generation: 1, key: format!("k{index}"),
                page: "main".into(), title: "t".into(), label: format!("plugin-win-{index}"), ticket: String::new(), url: String::new(),
                anchor: Anchor::DockBottom, align: Align::RightCenter, anchor_base: AnchorBase::Window, offset_x: 0.0, offset_y: 0.0, width: Width::Fixed(420.0), height: 320.0, transparent: false, z_order: ZOrder::Normal,
                state: State::Ready, shown: true, hidden_by_dock: false, rect: None, drop: None,
            });
        }
        assert_eq!(book.per_plugin("acme.weather"), PER_PLUGIN);
        // 另一个插件不受这个插件已占 4 个的影响（配额按插件计）。
        assert_eq!(book.per_plugin("fish.other"), 0);
        assert!(book.windows.len() < TOTAL, "全局上限只在超过 16 时才挡住");
    }

    #[test]
    fn drop_routing_only_reaches_the_owner() {
        // 只有「订阅了 + 已就绪 + 显示着 + 代次还有效」的窗口才进命中区。
        let mut book = Book { windows: BTreeMap::new(), dock: None, next: 0, sessions: BTreeMap::new(), outputs: BTreeMap::new(), dock_hidden: false, dock_watchers: BTreeMap::new() };
        let record = |handle: &str, drop: bool, state: State, shown: bool| Rec {
            handle: handle.into(), plugin: "acme.weather".into(), digest: "d".into(), generation: 1, key: handle.into(),
            page: "main".into(), title: "t".into(), label: format!("plugin-win-{handle}"), ticket: "t".into(), url: "u".into(),
            anchor: Anchor::DockBottom, align: Align::RightCenter, anchor_base: AnchorBase::Window, offset_x: 0.0, offset_y: 0.0, width: Width::Fixed(420.0), height: 320.0, transparent: false, z_order: ZOrder::Normal,
            state, shown, hidden_by_dock: false, rect: Some((10, 10, 100, 100)), drop: if drop { Some(format!("drop-{handle}")) } else { None },
        };
        book.windows.insert("a".into(), record("a", true, State::Ready, true));
        book.windows.insert("b".into(), record("b", false, State::Ready, true));
        book.windows.insert("c".into(), record("c", true, State::Pending, true));
        book.windows.insert("d".into(), record("d", true, State::Ready, false));
        let live: Vec<String> = book.windows.values().filter(|rec| rec.drop.is_some() && rec.state == State::Ready && rec.shown).map(|rec| rec.handle.clone()).collect();
        assert_eq!(live, vec!["a".to_string()], "没订阅、没就绪、没显示的窗口都不捕获");
        // 主 Dock 订阅是独占的：一个 dock 槽位只记一个插件。
        let mut book = book;
        book.dock = Some(DockSub { handle: "drop-1".into(), plugin: "acme.weather".into(), generation: 1 });
        assert_eq!(book.dock.as_ref().map(|sub| sub.plugin.as_str()), Some("acme.weather"));
    }

    fn pending_record(plugin: &str, key: &str) -> Rec {
        Rec {
            handle: String::new(), plugin: plugin.into(), digest: "digest".into(), generation: 1, key: key.into(),
            page: "main".into(), title: "面板".into(), label: String::new(), ticket: String::new(), url: String::new(),
            anchor: Anchor::DockLeft, align: Align::RightCenter, anchor_base: AnchorBase::Window, offset_x: 0.0, offset_y: 0.0, width: Width::Fixed(420.0), height: 320.0, transparent: false, z_order: ZOrder::Normal,
            state: State::Pending, shown: false, hidden_by_dock: false, rect: None, drop: None,
        }
    }

    #[test]
    fn reservation_is_atomic_for_duplicate_keys_and_quotas() {
        let registry = std::sync::Arc::new(Mutex::new(Book { windows: BTreeMap::new(), dock: None, next: 0, sessions: BTreeMap::new(), outputs: BTreeMap::new(), dock_hidden: false, dock_watchers: BTreeMap::new() }));
        {
            let mut registry = registry.lock().unwrap();
            registry.sessions.insert("acme.notes".into(), 1);
            for index in 0..12 { registry.sessions.insert(format!("acme.other{index}"), 1); }
            registry.sessions.insert("acme.last".into(), 1);
        }
        let gate = std::sync::Arc::new(std::sync::Barrier::new(12));
        let workers: Vec<_> = (0..12).map(|_| {
            let registry = registry.clone();
            let gate = gate.clone();
            std::thread::spawn(move || {
                gate.wait();
                registry.lock().unwrap().reserve(pending_record("acme.notes", "panel")).unwrap().0.handle
            })
        }).collect();
        let handles: Vec<_> = workers.into_iter().map(|worker| worker.join().unwrap()).collect();
        assert!(handles.iter().all(|handle| handle == &handles[0]), "同 key 并发只预留一个窗口");
        let mut registry = registry.lock().unwrap();
        assert_eq!(registry.windows.len(), 1);
        for key in ["two", "three", "four"] { registry.reserve(pending_record("acme.notes", key)).unwrap(); }
        assert!(registry.reserve(pending_record("acme.notes", "five")).is_err());
        for index in 0..12 { registry.reserve(pending_record(&format!("acme.other{index}"), "main")).unwrap(); }
        assert_eq!(registry.windows.len(), TOTAL);
        assert!(registry.reserve(pending_record("acme.last", "main")).is_err());
        let mut old = pending_record("acme.notes", "panel");
        old.generation = 2;
        assert!(registry.reserve(old).is_err(), "新代次不能复用旧窗口");
    }

    #[test]
    fn revoked_session_cannot_reserve_or_retire_its_replacement() {
        let mut registry = Book { windows: BTreeMap::new(), dock: None, next: 0, sessions: BTreeMap::new(), outputs: BTreeMap::new(), dock_hidden: false, dock_watchers: BTreeMap::new() };
        registry.sessions.insert("acme.notes".into(), 1);
        let first = registry.reserve(pending_record("acme.notes", "panel")).unwrap().0;
        assert_eq!(registry.retire("acme.notes", Some(1)).len(), 1);
        assert!(registry.reserve(first).is_err(), "撤销后迟到创建不得复活");
        registry.sessions.insert("acme.notes".into(), 2);
        let mut second = pending_record("acme.notes", "panel"); second.generation = 2;
        registry.reserve(second).unwrap();
        assert!(registry.retire("acme.notes", Some(1)).is_empty());
        assert_eq!(registry.windows.len(), 1, "旧实例 Drop 不得关闭新窗口");
    }

    #[test]
    fn activation_session_is_usable_without_runtime_lock() {
        let (output, input) = std::sync::mpsc::sync_channel(4);
        let session = begin_session("acme.hook-session-test", output);
        let _runtime = crate::plugin_runtime::CURRENT.lock().unwrap();
        assert!(generation_fresh("acme.hook-session-test", session.generation));
        notify_backend("acme.hook-session-test", session.generation, json!({"event":"ready"})).unwrap();
        assert_eq!(input.try_recv().unwrap()["method"], "plugin.windowEvent");
        {
            let mut registry = book();
            let mut rec = pending_record("acme.hook-session-test", "panel");
            rec.generation = session.generation;
            rec.state = State::Ready; rec.shown = true; rec.rect = Some((0, 0, 100, 100)); rec.drop = Some("new-drop".into());
            registry.reserve(rec).unwrap();
        }
        assert_eq!(drop_rects()[0].0, "new-drop", "接收器绑定订阅句柄，而非可反复订阅的窗口句柄");
        assert!(deliver_window("old-drop", &["synthetic-old-path".into()]).is_err());
        enter_window("old-drop");
        assert!(input.try_recv().is_err(), "旧订阅不得产生进入事件");
        enter_window("new-drop");
        let entered = input.try_recv().expect("后端也应收到进入事件");
        assert_eq!(entered["params"]["phase"], "enter");
        let handle = book().windows.values().find(|rec| rec.plugin == "acme.hook-session-test").unwrap().handle.clone();
        assert_eq!(entered["params"]["handle"], handle, "事件公开窗口句柄，订阅 token 只用于路由");
        leave_window("new-drop");
        assert_eq!(input.try_recv().unwrap()["params"]["phase"], "leave");
        session.revoke();
        assert!(!has_session("acme.hook-session-test"));
        let closed = input.try_recv().expect("撤销也应通知旧后端关闭原因");
        assert_eq!(closed["params"]["event"], "closed");
        assert_eq!(closed["params"]["reason"], "stopped");
    }

    #[test]
    fn closing_windows_keeps_sessions_outputs_and_dock_subscription() {
        let (output, input) = std::sync::mpsc::sync_channel(4);
        let session = begin_session("synthetic-close-windows", output);
        let first = {
            let mut registry = book();
            registry.dock = Some(DockSub { handle: "synthetic-dock".into(), plugin: "synthetic-close-windows".into(), generation: session.generation });
            let mut rec = pending_record("synthetic-close-windows", "panel");
            rec.generation = session.generation;
            registry.reserve(rec).unwrap().0
        };
        close_windows("user");
        assert!(generation_fresh("synthetic-close-windows", session.generation), "关闭窗口不应撤销设置页与后端会话");
        assert_eq!(dock_subscription().as_deref(), Some("synthetic-dock"));
        assert_eq!(input.try_recv().unwrap()["params"]["reason"], "user");
        assert!(input.try_recv().is_err(), "一次关闭只应通知一次 closed（#83）");
        notify_backend("synthetic-close-windows", session.generation, json!({"event":"ready"})).unwrap();
        assert_eq!(input.try_recv().unwrap()["params"]["event"], "ready");
        let mut registry = book();
        assert!(!registry.windows.contains_key(&first.handle));
        let mut rec = pending_record("synthetic-close-windows", "panel");
        rec.generation = session.generation;
        assert!(registry.reserve(rec).is_ok(), "原实例应可再次开窗");
    }

    #[test]
    fn oversized_drop_is_rejected_before_destination_lookup() {
        let paths = vec!["x".repeat(crate::plugin_protocol::MAX_LINE as usize)];
        for result in [deliver_window("missing", &paths),deliver_dock("missing", &paths)] {
            assert!(result.unwrap_err().contains("超过协议限制"), "超大拖入须在查找目标、复制或发给页面之前拒绝");
        }
    }

    #[test]
    fn saturated_backend_queue_reports_delivery_failure() {
        let (output, _input) = std::sync::mpsc::sync_channel(1);
        let session = begin_session("acme.queue-test", output.clone());
        output.try_send(json!({"busy":true})).unwrap();
        assert!(notify_backend("acme.queue-test", session.generation, json!({"event":"fileDrop","phase":"drop"})).is_err(), "队列满不得报告交付成功");
    }

    #[test]
    fn retired_events_never_reach_replacement_backend() {
        let (old_output, _old_input) = std::sync::mpsc::sync_channel(4);
        let old = begin_session("acme.event-generation-test", old_output);
        let (new_output, new_input) = std::sync::mpsc::sync_channel(4);
        let new = begin_session("acme.event-generation-test", new_output);
        assert!(notify_backend("acme.event-generation-test", old.generation, json!({"event":"fileDrop","paths":["synthetic-old-path"]})).is_err());
        assert!(new_input.try_recv().is_err(), "旧会话路径不得交给新后端");
        drop(old);
        assert_eq!(current_generation("acme.event-generation-test"), new.generation);
        notify_backend("acme.event-generation-test", new.generation, json!({"event":"ready"})).unwrap();
        assert_eq!(new_input.try_recv().unwrap()["params"]["event"], "ready");
    }

    #[test]
    fn invalid_update_leaves_all_geometry_unchanged() {
        let mut rec = pending_record("acme.notes", "panel");
        let original = view(&rec);
        assert!(apply_update(&mut rec, &json!({"width":500,"height":-1})).is_err());
        assert_eq!(view(&rec), original, "后续字段无效时不得提前写入宽度");
        apply_update(&mut rec, &json!({"width":500,"height":260})).unwrap();
        assert_eq!(rec.width, Width::Fixed(500.0));
        assert_eq!(rec.height, 260.0);
    }

    #[test]
    fn placement_uses_target_dpi_and_contains_oversized_windows() {
        use crate::screen::{Area, Screens};
        let primary = Area { x:0, y:0, w:1920, h:1040, sf:1.0, mon:(0,0,1920,1080) };
        let second = Area { x:-1600, y:0, w:1600, h:900, sf:2.0, mon:(-1600,0,1600,960) };
        let screens = Screens { areas:vec![primary,second], primary:Some(primary) };
        let mut rec = pending_record("acme.notes", "panel");
        let (_, _, width, height) = placement(&rec, &screens, Some((-800,100,400,60)), None, 1.0);
        assert_eq!((width,height),(840,640), "目标屏幕 DPI 决定逻辑尺寸");
        let (x,y,iw,ih) = fit_outer(second, (-1000,800), (1600,900), (1616,939));
        assert_eq!((iw,ih),(1584,861), "工作区须扣除原生标题栏和边框");
        assert!(x>=second.x && y>=second.y && x+iw as i32+16<=second.x+second.w && y+ih as i32+39<=second.y+second.h);
        rec.width = Width::Fixed(2400.0); rec.height=1600.0;
        let (x,y,w,h) = placement(&rec, &screens, Some((-800,100,400,60)), None, 1.0);
        assert!(x>=second.x && y>=second.y && x+w<=second.x+second.w && y+h<=second.y+second.h, "超大窗口仍须包含在工作区: {:?}", (x,y,w,h));
    }

    #[test]
    fn screen_placement_reflows_after_monitor_removal_and_work_area_change() {
        use crate::screen::{Area, Screens};
        let primary = Area { x:0, y:0, w:1920, h:1040, sf:1.0, mon:(0,0,1920,1080) };
        let second = Area { x:-1600, y:0, w:1600, h:900, sf:2.0, mon:(-1600,0,1600,960) };
        let mut rec = pending_record("synthetic-screen", "panel");
        rec.anchor = Anchor::Screen; rec.offset_x = -800.0; rec.offset_y = 100.0;
        let before = Screens { areas:vec![primary,second], primary:Some(primary) };
        let (x,_,w,_) = placement(&rec,&before,None,None,1.0);
        assert!(x < 0); assert_eq!(w,840);
        let smaller = Area { x:40, y:20, w:900, h:600, sf:1.5, mon:(0,0,960,720) };
        let after = Screens { areas:vec![smaller], primary:Some(smaller) };
        let (x,y,w,h) = placement(&rec,&after,None,None,1.0);
        assert_eq!(w,630,"新屏幕 DPI 用于逻辑宽度");
        assert!(x>=smaller.x && y>=smaller.y && x+w<=smaller.x+smaller.w && y+h<=smaller.y+smaller.h);
    }

    #[test]
    fn reservation_inherits_dock_hidden_state() {
        let mut registry = Book { windows: BTreeMap::new(), dock: None, next: 0, sessions: BTreeMap::from([("acme.notes".into(), 1)]), outputs: BTreeMap::new(), dock_hidden: true, dock_watchers: BTreeMap::new() };
        let rec = registry.reserve(pending_record("acme.notes", "panel")).unwrap().0;
        assert!(rec.hidden_by_dock);
        assert!(!rec.shown);
    }

    #[test]
    fn view_exposes_only_opaque_state() {
        let rec = Rec {
            handle: "h".into(), plugin: "acme.weather".into(), digest: "d".into(), generation: 7, key: "panel".into(),
            page: "main".into(), title: "面板".into(), label: "plugin-win-9".into(), ticket: "secret-ticket".into(), url: "http://plugin/page".into(),
            anchor: Anchor::DockLeft, align: Align::RightCenter, anchor_base: AnchorBase::Window, offset_x: 8.0, offset_y: 0.0, width: Width::Fixed(500.0), height: 240.0, transparent: true, z_order: ZOrder::Normal,
            state: State::Ready, shown: true, hidden_by_dock: false, rect: None, drop: None,
        };
        let view = view(&rec);
        assert_eq!(view["handle"], "h");
        assert_eq!(view["anchor"], "dock-left");
        assert_eq!(view["ready"], true);
        // 票据与 URL 不出现在给插件看的状态里（它们只回给宿主容器页面）。
        assert!(view.get("ticket").is_none());
        assert!(view.get("url").is_none());
        assert!(view.get("digest").is_none());
    }

    // 日志与错误里不出现文件路径：交付只记数量与所有者（§6）。
    #[test]
    fn drop_delivery_messages_never_carry_paths() {
        let paths = vec!["C:/Users/someone/Secret/report.docx".to_string(), "D:/x/y.png".to_string()];
        // 交付函数自身的日志文案只有数量与所有者；这里钉住那条文案本身。
        let line = format!("file drop -> {} path(s) for {} window {}", paths.len(), short("abcdef0123"), "acme.weather");
        for path in &paths {
            assert!(!line.contains(path), "日志不许出现完整路径");
        }
    }

    // -----------------------------------------------------------------------------------
    // 2.2.2 新增能力的验收
    // -----------------------------------------------------------------------------------

    // B1：下限降到 32×32，36 px 的球终于能做成窗口；但仍拒绝 31 与 0。
    #[test]
    fn tiny_windows_are_accepted_down_to_32() {
        let ok = Options::parse(&json!({"key":"ball","page":"main","width":36.0,"height":36.0})).unwrap();
        assert_eq!((ok.width, ok.height), (Width::Fixed(36.0), 36.0), "36 px 球必须能建窗");
        for bad in [json!({"key":"ball","page":"main","width":31.0}), json!({"key":"ball","page":"main","height":0.0})] {
            assert!(Options::parse(&bad).is_err(), "小于 32 仍须拒绝: {bad}");
        }
    }

    // C4：width:'dock' 取胶囊宽度；maxWidth 只能放宽不能收紧。
    #[test]
    fn dock_width_follows_the_capsule_and_max_only_grows() {
        // 胶囊 300 逻辑像素：等宽就是 300（不走默认值）。
        assert_eq!(Width::Dock { max: None }.resolve(Some(300.0)), 300.0);
        // 给了上限 600：当前胶囊 300 仍按胶囊算，不会自己变宽。
        assert_eq!(Width::Dock { max: Some(600.0) }.resolve(Some(300.0)), 300.0);
        // 上限小于胶囊时必须按胶囊算——语义是「可增长到」，不能缩到比胶囊还窄。
        assert_eq!(Width::Dock { max: Some(100.0) }.resolve(Some(300.0)), 300.0);
        // 胶囊尺寸未知（页面还没上报）时回落到默认宽度，绝不塌成 0。
        assert_eq!(Width::Dock { max: None }.resolve(None), WIDTH_DEFAULT);
        // 固定宽度不受胶囊影响。
        assert_eq!(Width::Fixed(420.0).resolve(Some(300.0)), 420.0);
    }

    // C4：open 与 update 都接受 'dock'，view 对外报 "dock" 而不是数字。
    #[test]
    fn dock_width_round_trips_through_options_and_view() {
        let options = Options::parse(&json!({"key":"bar","page":"main","width":"dock","maxWidth":600,"height":240})).unwrap();
        assert_eq!(options.width, Width::Dock { max: Some(600.0) });
        let mut rec = pending_record("acme.notes", "bar");
        rec.width = options.width;
        assert_eq!(view(&rec)["width"], "dock", "插件要能看出宿主接管了宽度");
        assert_eq!(view(&rec)["maxWidth"], 600.0);
        // update 也能切回固定宽度。
        validate_update(&mut rec, &json!({"width":500})).unwrap();
        assert_eq!(rec.width, Width::Fixed(500.0));
        assert_eq!(view(&rec)["width"], 500.0);
    }

    // B2：align 的八种组合把窗口左上角放在锚点的正确方位上。
    #[test]
    fn align_places_the_window_on_the_right_corner() {
        let point = (100, 200);
        let size = (40, 20);
        assert_eq!(Align::RightTop.offset(point, size), (60, 200));
        assert_eq!(Align::RightCenter.offset(point, size), (60, 190));
        assert_eq!(Align::RightBottom.offset(point, size), (60, 180));
        assert_eq!(Align::LeftTop.offset(point, size), (100, 200));
        assert_eq!(Align::LeftCenter.offset(point, size), (100, 190));
        assert_eq!(Align::LeftBottom.offset(point, size), (100, 180));
        assert_eq!(Align::TopCenter.offset(point, size), (80, 200));
        assert_eq!(Align::BottomCenter.offset(point, size), (80, 180));
    }

    // B2：不写 align / anchorBase 时，位置必须与 2.2.1 完全一致（向后兼容）。
    #[test]
    fn default_align_reproduces_the_2_2_1_position() {
        let options = Options::parse(&json!({"key":"panel","page":"main","width":160,"height":80})).unwrap();
        assert_eq!(options.align, Align::LeftTop, "dock-bottom 缺省沿用 2.2.1 的左下角");
        assert_eq!(options.anchor_base, AnchorBase::Window, "缺省仍锚主岛窗口，不是胶囊");
        let left = Options::parse(&json!({"key":"panel","page":"main","width":160,"height":80,"anchor":"dock-left"})).unwrap();
        assert_eq!(left.align, Align::RightCenter, "dock-left 缺省沿用 2.2.1 的右缘居中");
    }

    // B2：anchorBase:'capsule' 时以胶囊为准，胶囊未知则回落到窗口。
    #[test]
    fn capsule_base_uses_the_capsule_and_falls_back_to_the_window() {
        use crate::screen::{Area, Screens};
        let primary = Area { x:0, y:0, w:1920, h:1040, sf:1.0, mon:(0,0,1920,1080) };
        let screens = Screens { areas:vec![primary], primary:Some(primary) };
        let mut rec = pending_record("acme.notes", "ball");
        rec.width = Width::Fixed(36.0); rec.height = 36.0;
        rec.anchor = Anchor::DockLeft; rec.align = Align::RightCenter; rec.anchor_base = AnchorBase::Capsule;
        // 主岛窗口铺满工作区；胶囊只有 300 宽、位于 810..1110，顶边 y=8。
        // dock-left 的锚点是基准左缘的中点：胶囊 (810, 8+36/2) = (810, 26)。
        // 窗口高 36 与胶囊等高时，RightCenter 恰好把球顶对齐胶囊顶：y = 26-18 = 8。
        let window = Some((0, 0, 1920, 132));
        let capsule = Some((810, 8, 300, 36));
        let (x, y, _, _) = placement(&rec, &screens, window, capsule, 1.0);
        assert_eq!((x, y), (774, 8), "球右缘贴胶囊左缘、球顶贴胶囊顶：810-36=774, 26-18=8");
        // 换成 RightTop 则贴的是锚点本身（胶囊左缘中点），y 就是 26。
        rec.align = Align::RightTop;
        assert_eq!(placement(&rec, &screens, window, capsule, 1.0).1, 26, "RightTop 贴锚点本身");
        // 胶囊尺寸还没上报：回落到主岛窗口，行为等于 2.2.1。窗口左缘在 0，
        // 窗口宽 36 时算出的 x = 0-36 = -36，随后被工作区约束夹回左缘 0。
        rec.align = Align::RightCenter;
        let (fallback_x, fallback_y, _, _) = placement(&rec, &screens, window, None, 1.0);
        assert_eq!(fallback_x, 0, "没有胶囊时按窗口左缘算，再被工作区夹回左缘");
        assert_eq!(fallback_y, 48, "垂直按窗口 132 高中点 66 再减半高 18");
    }

    // C1：快照把两个矩形分开给，且视图形态未上报时按 compact 算。
    #[test]
    fn dock_snapshot_reports_window_and_capsule_separately() {
        let snapshot = crate::island::DockSnapshot {
            window: Some((0, 0, 1920, 132)),
            capsule: Some((810, 8, 300, 36)),
            view: "expanded",
            stowed: false,
            hidden: false,
            fullscreen: false,
            scale: 1.0,
        };
        let value = dock_rect_value(&snapshot);
        assert_eq!(value["window"]["width"], 1920, "窗口矩形是工作区宽");
        assert_eq!(value["capsule"]["width"], 300, "胶囊矩形是可见宽度");
        assert_eq!(value["capsule"]["x"], 810);
        assert_eq!(value["state"]["view"], "expanded");
        assert_eq!(value["state"]["stowed"], false);
        // 胶囊未知时给 null，不给一个假矩形（2.2.0 起就不再兜底 500×36）。
        let unknown = crate::island::DockSnapshot { capsule: None, ..snapshot };
        assert!(dock_rect_value(&unknown)["capsule"].is_null());
    }
}
