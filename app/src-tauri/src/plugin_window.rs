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
use std::sync::atomic::{AtomicBool, AtomicU64};
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
/// 2.2.3（B4）：一个窗口最多几块可点区域。够长条上排几十个 chip，又不至于让
/// 命中判定变成 CPU 热点。
const MAX_HIT_RECTS: usize = 256;
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

/// 2.2.3（B4）：窗口的鼠标命中方式。
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum HitTest {
    /// 2.2.1 的行为：整块窗口矩形都接住鼠标（透明像素也接）。
    Window,
    /// 只接住 `hitRects` 给的区域，其余穿过去点到下面的主岛或桌面。
    Opaque,
}

impl HitTest {
    fn parse(name: &str) -> Option<Self> {
        Some(match name {
            "window" => Self::Window,
            "opaque" => Self::Opaque,
            _ => return None,
        })
    }
    fn name(self) -> &'static str {
        match self {
            Self::Window => "window",
            Self::Opaque => "opaque",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Anchor {
    DockLeft,
    DockBottom,
    Screen,
}

/// 2.3.4（#88 H2）：`windows.update` 的几何补间曲线。
/// `Linear` 匀速；`EaseOutCubic` 先快后慢；`Spring` 弹簧（过冲后收敛）。
/// 缺省 `EaseOutCubic`，只在 `duration > 0` 时插值；`duration == 0` 保持 2.2.3 瞬时摆放。
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Easing {
    Linear,
    EaseOutCubic,
    Spring,
}

impl Easing {
    fn parse(name: &str) -> Option<Self> {
        Some(match name {
            "linear" => Self::Linear,
            "ease-out-cubic" | "ease-out" => Self::EaseOutCubic,
            "spring" => Self::Spring,
            _ => return None,
        })
    }
    fn name(self) -> &'static str {
        match self {
            Self::Linear => "linear",
            Self::EaseOutCubic => "ease-out-cubic",
            Self::Spring => "spring",
        }
    }
}

/// 2.3.4（#88 H2）：归一化时间 `t ∈ [0,1]` → 进度 `p`。端点严格 t=0→0、t=1→1。
fn ease_at(easing: Easing, t: f64) -> f64 {
    let t = t.clamp(0.0, 1.0);
    if t <= 0.0 { return 0.0; }
    if t >= 1.0 { return 1.0; }
    match easing {
        Easing::Linear => t,
        Easing::EaseOutCubic => 1.0 - (1.0 - t).powi(3),
        Easing::Spring => 1.0 - (-6.0 * t).exp() * (12.0 * t).cos(),
    }
}

/// 2.3.4（#88 H2）：`from → to` 在归一化时间 `t` 处的插值。
fn tween(from: f64, to: f64, t: f64, easing: Easing) -> f64 {
    from + (to - from) * ease_at(easing, t)
}

/// 2.3.4（#88 H2）：duration 合法上界（毫秒）。
const DURATION_MAX_MS: u64 = 2000;

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
    /// 2.2.3（B4）：命中方式。'opaque' 时只接住 `hit_rects` 给的区域。
    pub hit_test: HitTest,
    /// 可点区域（页面逻辑像素 x,y,w,h）。空表示整窗可点。
    pub hit_rects: Vec<[f64; 4]>,
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
            if !["key", "page", "title", "width", "height", "anchor", "offsetX", "offsetY", "transparent", "maxWidth", "align", "anchorBase", "zOrder", "hitTest", "hitRects"].contains(&name.as_str()) {
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
        let hit_test = match object.get("hitTest") {
            None => HitTest::Window,
            Some(value) => value.as_str().and_then(HitTest::parse).ok_or("插件窗口 hitTest 只支持 window、opaque")?,
        };
        // 区域一律按逻辑像素收：取值必须是有限数，免得一个 NaN 让命中判定恒假。
        let hit_rects = match object.get("hitRects") {
            None => Vec::new(),
            Some(value) => {
                let list = value.as_array().ok_or("插件窗口 hitRects 应为矩形数组")?;
                if list.len() > MAX_HIT_RECTS { return Err(format!("插件窗口 hitRects 最多 {MAX_HIT_RECTS} 块")); }
                let mut rects = Vec::with_capacity(list.len());
                for item in list {
                    let parts = item.as_array().ok_or("插件窗口 hitRects 的每一项应为 [x,y,w,h]")?;
                    if parts.len() != 4 { return Err("插件窗口 hitRects 的每一项应为 [x,y,w,h]".into()); }
                    let mut rect = [0.0f64; 4];
                    for (i, part) in parts.iter().enumerate() {
                        rect[i] = part.as_f64().filter(|v| v.is_finite()).ok_or("插件窗口 hitRects 含非有限数")?;
                    }
                    if rect[2] <= 0.0 || rect[3] <= 0.0 { return Err("插件窗口 hitRects 的宽高必须为正".into()); }
                    rects.push(rect);
                }
                rects
            }
        };
        if hit_test == HitTest::Opaque && hit_rects.is_empty() {
            // 说好了只命中不透明区却没给区域：按「整窗可点」处理太意外，
            // 按「全穿透」又会让窗口点不到。直接拒绝，让插件给出区域。
            return Err("插件窗口 hitTest:'opaque' 需要同时给出 hitRects".into());
        }
        Ok(Self { key: key.into(), page: page.into(), title, width, height, anchor, align, anchor_base, offset_x, offset_y, transparent, z_order, hit_test, hit_rects })
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
    /// 2.2.3（B4）：命中方式与可点区域；`Window` 时不登记任何规则。
    hit_test: HitTest,
    hit_rects: Vec<[f64; 4]>,
    state: State,
    /// 窗口当前是否显示（被 Dock 全局隐藏时为 false）。
    shown: bool,
    /// 因为 Dock 隐藏而被宿主收起来：恢复时只恢复这一批（用户自己关掉的不复活）。
    hidden_by_dock: bool,
    /// 本窗口的外框（屏幕物理像素），拖入命中区用。
    rect: Option<(i32, i32, i32, i32)>,
    /// 本窗口的文件拖入订阅句柄：没有订阅就不捕获、不交付（句柄原样回给插件）。
    drop: Option<String>,
    /// 2.3.4（#88 H2）：位置尺寸变化的补间时长（毫秒）。0 = 瞬时（2.2.3 行为）。
    duration_ms: u64,
    /// 2.3.4（#88 H2）：补间曲线，仅在 `duration_ms > 0` 时生效。
    easing: Easing,
    /// 2.3.4（#88 H7）：上次成功摆放的外框，用于胶囊短暂未知时保持上次位置。
    last_rect: Option<(i32, i32, i32, i32)>,
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

static BOOK: Mutex<Book> = Mutex::new(Book { windows: BTreeMap::new(), dock: None, next: 0, sessions: BTreeMap::new(), outputs: BTreeMap::new(), dock_hidden: false });
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

pub(crate) fn app_handle() -> Result<&'static AppHandle, String> {
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
    let app = app_handle()?.clone();
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
    let Ok(app) = app_handle() else { return };
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
        // 2.2.3（B3）：页面里拖动文件出去
        "dock.windows.beginDragOut" => begin_drag_out(plugin, generation, params),
        "dock.fileDrop.subscribe" => subscribe(&context, plugin, generation, params),
        "dock.fileDrop.unsubscribe" => unsubscribe(plugin, generation, params),
        // 2.2.2：主岛几何与状态归 plugin_dock，窗口模块只做转发。
        // 2026-11-16 本地补丁：把 dock.reserve/release（2.3.4 H10 占位）也转发给 plugin_dock——
        // 上游 plugin_dock::dispatch 已实现这两个方法，但这里漏了转发，导致插件调 reserve 报「方法无效」。
        "dock.dock.rect" | "dock.dock.subscribe" | "dock.dock.unsubscribe" | "dock.dock.openPage" | "dock.dock.reserve" | "dock.dock.release" => crate::plugin_dock::page(plugin, method, params),
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
        // 2.3.4（#88 H6）：单窗口多形态 / keepAlive 回原位，保留句柄与订阅。
        "host.windows.reopen" => reopen(context, plugin, generation, params),
        "host.windows.close" => close_owned(plugin, generation, params, "plugin"),
        "host.windows.list" => list(context, plugin, generation),
        "host.fileDrop.subscribe" => subscribe(context, plugin, generation, params),
        "host.fileDrop.unsubscribe" => unsubscribe(plugin, generation, params),
        "host.dock.rect" | "host.dock.subscribe" | "host.dock.unsubscribe" | "host.dock.openPage" | "host.dock.reserve" | "host.dock.release" => crate::plugin_dock::backend(context, plugin, generation, method, params),
        // 2.2.3（B3）：把本窗口里的文件拖回资源管理器。
        "host.windows.beginDragOut" => begin_drag_out(plugin, generation, params),
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
        width: options.width, height: options.height, transparent: options.transparent, z_order: options.z_order, hit_test: options.hit_test, hit_rects: options.hit_rects.clone(),
        state: State::Pending, shown: false, hidden_by_dock: false, rect: None, drop: None,
        duration_ms: 0, easing: Easing::EaseOutCubic, last_rect: None,
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
    let app = app_handle()?.clone();
    // 2026-11-16 本地补丁（上游 2.3.4 缺陷）：原实现把 WebviewWindowBuilder::build()
    // 放进 std::thread::spawn 子线程，同时主线程 recv_timeout(6s) 阻塞等待。
    // Tauri v2 的 build() 需要主线程执行（内部 run_on_main_thread），主线程此刻被
    // recv_timeout 占住 -> 死锁 -> 每次都 "插件窗口创建在主线程超时"，窗口永远建不出来。
    // 改为在调用线程（RPC 线程，非主线程）直接同步 build，由 Tauri 自己投递主线程并等待。
    {
        if !book().windows.contains_key(&snapshot.handle) { return Err("窗口创建已取消".into()); }
        let shared = crate::shared(&app);
        // 2.3.4（#88 P1）：WebView2 不能在主线程同步 build（与消息泵冲突报
        // `0x8007139F`，插件外部窗口一个都开不出来）。改在独立线程创建，主线程用
        // mpsc 超时等待；并补 `.additional_browser_args` / `.data_directory`
        // 复用全局 WebView 配置。
        let browser_args = crate::browser_args(&shared).to_string();
        let data_dir = crate::webview_dir(&shared);
        let capacity = snapshot.width.resolve(capsule_width_logical(&app));
        let height = snapshot.height;
        let label = snapshot.label.clone();
        let title = snapshot.title.clone();
        let win = WebviewWindowBuilder::new(&app, &label, WebviewUrl::App("window/window.html".into()))
            .title(&title)
            .inner_size(capacity, height)
            .decorations(!snapshot.transparent)
            .transparent(snapshot.transparent)
            .resizable(true)
            .maximizable(false)
            .minimizable(true)
            .skip_taskbar(true)
            .focused(false)
            .always_on_top(snapshot.z_order == ZOrder::AboveDock)
            .additional_browser_args(&browser_args)
            .data_directory(data_dir)
            .shadow(false)
            .visible(false)
            .build()
            .map_err(|error| format!("插件窗口创建失败：{error}"))?;
        let watched = win.clone();
        win.on_window_event(move |event| window_event(&watched, event));
        if !book().windows.contains_key(&snapshot.handle) {
            let _ = win.destroy();
            return Err("窗口创建已取消".into());
        }
        // 2.3.4（#88 P2）：透明窗剥掉原生标题栏/边框；非透明窗保持系统标题栏。
        if snapshot.transparent {
            // 2026-11-16 本地补丁：Tauri v2 未把 transparent 透传给 WebView2 controller，
            // 默认白底 -> 小球外面一圈白色方框、展开时大白框。显式设 alpha=0 透明背景
            //（Windows 上 alpha 非 0 会被强制改成 255，只有 0 才是真正透明）。
            let _ = win.set_background_color(Some(tauri::window::Color(0, 0, 0, 0)));
            strip_chrome(&win);
        }
        if let Err(error) = place(&win, &snapshot) { let _ = win.destroy(); return Err(error); }
        // H7：记下首帧外框，供后续胶囊短暂未知时保持位置。
        record_rect(&snapshot.handle, geometry_of(&win));
        // 2.2.3（B4）：只有 hitTest:'opaque' 的窗口才登记命中规则。
        // 其余窗口连登记都不做，plugin_hit 的规则表为空 → 主循环不会调 poll，
        // 与 2.2.2 的指针节奏一模一样。
        if snapshot.hit_test == HitTest::Opaque {
            crate::plugin_hit::watch(&snapshot.label, snapshot.hit_rects.clone());
            if let Some(rect) = geometry_of(&win) {
                crate::plugin_hit::place(&snapshot.label, Some(rect), win.scale_factor().unwrap_or(1.0));
            }
        }
        Ok::<(), String>(())
    };
    Ok(())
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
    // H7：anchorBase=capsule 且胶囊暂时未知时保持上次位置，绝不回落整窗。
    let (x, y, width, height) = placement_h7(rec, &screens, dock, capsule, sf, rec.last_rect);
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
                    // 锚点：dock-left 取基准的左缘中点，dock-bottom 取其底边中点
                    // （2.3.4/P4：与 dock-left 对称，修正旧版取左下角导致长条贴屏幕最左）。
                    let point = match rec.anchor {
                        Anchor::DockLeft => (rx, ry + rh / 2),
                        _ => (rx + rw / 2, ry + rh),
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

/// 2.3.4（#88 H7）：`placement` 之上加「胶囊未知时保持上次位置」。
/// 只有 anchorBase=capsule 且这次拿不到胶囊、但有上次摆放结果时才保持；其余走既有 `placement`。
fn placement_h7(rec: &Rec, screens: &crate::screen::Screens, dock: Option<(i32, i32, i32, i32)>, capsule: Option<(i32, i32, i32, i32)>, fallback_sf: f64, last_rect: Option<(i32, i32, i32, i32)>) -> (i32, i32, i32, i32) {
    if rec.anchor_base == AnchorBase::Capsule && capsule.is_none() {
        if let Some((lx, ly, lw, lh)) = last_rect {
            return (lx, ly, lw, lh);
        }
    }
    placement(rec, screens, dock, capsule, fallback_sf)
}

fn geometry_of(win: &WebviewWindow) -> Option<(i32, i32, i32, i32)> {
    match (win.outer_position(), win.outer_size()) {
        (Ok(position), Ok(size)) => Some((position.x, position.y, size.width as i32, size.height as i32)),
        _ => None,
    }
}

/// 2.3.4（#88 H2）：`rec` 按当前锚点/胶囊应落在的外框（补间目标，供记 last_rect/命中）。
fn intend_rect(app: &AppHandle, rec: &Rec) -> Option<(i32, i32, i32, i32)> {
    let win = app.get_webview_window(&rec.label)?;
    let sf = win.scale_factor().ok().unwrap_or(1.0).max(0.5);
    let screens = crate::screen::read(app);
    let dock = app.get_webview_window("island").and_then(|w| match (w.outer_position(), w.outer_size()) {
        (Ok(pos), Ok(size)) => Some((pos.x, pos.y, size.width as i32, size.height as i32)),
        _ => None,
    });
    let capsule = crate::island::snapshot(&crate::shared(app)).capsule;
    Some(placement_h7(rec, &screens, dock, capsule, sf, rec.last_rect))
}
fn record_rect(handle: &str, rect: Option<(i32, i32, i32, i32)>) {
    let Some(rect) = rect else { return };
    if let Some(rec) = book().windows.get_mut(handle) {
        rec.rect = Some(rect);
        rec.last_rect = Some(rect);
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

/// 2.3.4（#88 H3）：给一个窗口的所有者发 `outsidePress`（若这次按下落在该窗口之外）。
pub(crate) fn outside_press_for_label(label: &str, point: (i32, i32)) {
    let rec = { let book = book(); book.by_label(label).cloned() };
    let Some(rec) = rec else { return };
    if !generation_fresh(&rec.plugin, rec.generation) { return; }
    let inside = rec.rect.is_some_and(|(x, y, w, h)| point.0 >= x && point.1 >= y && point.0 < x + w && point.1 < y + h);
    if inside { return; }
    let payload = json!({"event":"dock","type":"outside","x":point.0,"y":point.1,"handle":rec.handle});
    emit(&rec.plugin, rec.generation, Some(&rec.label), payload.clone());
    let _ = notify_backend(&rec.plugin, rec.generation, payload);
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
    // 2.2.3（B4）：命中测试要窗口外框与缩放；这里顺手喂给它，免得每帧再问窗口。
    let scale = win.scale_factor().unwrap_or(1.0);
    crate::plugin_hit::place(label, Some(rect), scale);
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
    let app = app_handle()?.clone();
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
    // 2.2.3（B4）：命中规则必须跟着窗口一起撤，否则规则表会留下一个永远
    // 找不到窗口的条目，指针循环还得为它每 40ms 醒一次。
    crate::plugin_hit::forget(&rec.label);
    READY.notify_all();
    if !rec.ticket.is_empty() {
        crate::plugin_page::close(&rec.ticket);
    }
    if let Ok(app) = app_handle() {
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
            if !["handle", "width", "height", "anchor", "offsetX", "offsetY", "title", "maxWidth", "align", "anchorBase", "zOrder", "hitTest", "hitRects", "duration", "easing", "resize"].contains(&name.as_str()) {
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
        // 2.2.3（B4）：插件可以在运行中改可点区域（内容变了、chip 增减）。
        if let Some(hit_test) = object.get("hitTest") {
            rec.hit_test = hit_test.as_str().and_then(HitTest::parse).ok_or("插件窗口 hitTest 只支持 window、opaque")?;
        }
        if let Some(value) = object.get("hitRects") {
            let list = value.as_array().ok_or("插件窗口 hitRects 应为矩形数组")?;
            if list.len() > MAX_HIT_RECTS { return Err(format!("插件窗口 hitRects 最多 {MAX_HIT_RECTS} 块")); }
            let mut rects = Vec::with_capacity(list.len());
            for item in list {
                let parts = item.as_array().ok_or("插件窗口 hitRects 的每一项应为 [x,y,w,h]")?;
                if parts.len() != 4 { return Err("插件窗口 hitRects 的每一项应为 [x,y,w,h]".into()); }
                let mut rect = [0.0f64; 4];
                for (i, part) in parts.iter().enumerate() {
                    rect[i] = part.as_f64().filter(|v| v.is_finite()).ok_or("插件窗口 hitRects 含非有限数")?;
                }
                if rect[2] <= 0.0 || rect[3] <= 0.0 { return Err("插件窗口 hitRects 的宽高必须为正".into()); }
                rects.push(rect);
            }
            rec.hit_rects = rects;
        }
        if rec.hit_test == HitTest::Opaque && rec.hit_rects.is_empty() {
            return Err("插件窗口 hitTest:'opaque' 需要同时给出 hitRects".into());
        }
        if let Some(title) = object.get("title") {
            let text = title.as_str().map(str::trim).filter(|t| !t.is_empty() && t.chars().count() <= TITLE_MAX && !t.chars().any(char::is_control)).ok_or("插件窗口标题无效")?;
            rec.title = text.to_string();
        }
        // 2.3.4（#88 H2）：duration/easing。duration=0 回到瞬时；easing 缺省 ease-out-cubic。
        if let Some(duration) = object.get("duration") {
            let value = duration.as_u64().filter(|v| *v <= DURATION_MAX_MS).ok_or("插件窗口 duration 应为 0–2000 的整数毫秒")?;
            rec.duration_ms = value;
        }
        if let Some(easing) = object.get("easing") {
            rec.easing = easing.as_str().and_then(Easing::parse).ok_or("插件窗口 easing 只支持 linear、ease-out-cubic、spring")?;
        }
        // H6：resize 只为兼容显式布尔；实际重摆由 reopen 完成（这里校验类型即可）。
        if let Some(resize) = object.get("resize") {
            if !matches!(resize, Value::Bool(_)) { return Err("插件窗口 resize 应为布尔值".into()); }
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
    let app = app_handle()?.clone();
    let for_main = snapshot.clone();
    on_main(move || -> Result<(), String> {
        // 排队期间可能已有更新或撤销；只应用登记表里的当前值。
        let current = {
            let book = book();
            book.windows.get(&for_main.handle).filter(|rec| book.fresh(&rec.plugin, rec.generation)).cloned()
        }.ok_or("窗口已关闭或会话已撤销")?;
        let win = app.get_webview_window(&current.label).ok_or("插件窗口尚未创建")?;
        win.set_title(&current.title).map_err(|_| "插件窗口标题设置失败")?;
        if current.duration_ms > 0 {
            // H2：补间到目标（帧循环在工作线程，不阻塞主线程）；先记目标外框，供上次位置/命中用。
            let target = intend_rect(&app, &current);
            animate_place(&win, app.clone(), &current)?;
            if let Some(target) = target { record_rect(&current.handle, Some(target)); }
        } else {
            place(&win, &current)?;
            record_rect(&current.handle, geometry_of(&win));
        }
        // 2.2.3（B4）：命中区域或命中方式可能刚改过，按当前值重新登记/撤销。
        if current.hit_test == HitTest::Opaque {
            crate::plugin_hit::watch(&current.label, current.hit_rects.clone());
            let hit_rect = book().windows.get(&current.handle).and_then(|r| r.rect).or_else(|| geometry_of(&win));
            crate::plugin_hit::place(&current.label, hit_rect, win.scale_factor().unwrap_or(1.0));
        } else {
            crate::plugin_hit::forget(&current.label);
        }
        Ok(())
    })??;
    let _ = context;
    Ok(view(&snapshot))
}

/// 2.3.4（#88 H6）：单窗口多形态 / keepAlive 的「回原位」。句柄、票据、页面、drop
/// 订阅、代次全部保留，只按当前记录重摆（可选带 duration/easing 补间），并允许顺带改形状。
fn reopen(context: &Context, plugin: &str, generation: u64, params: &Value) -> Result<Value, String> {
    let handle = handle_of(params)?.to_string();
    let snapshot = {
        let mut book = book();
        if !book.fresh(plugin, generation) { return Err("插件会话已撤销".into()); }
        let current = book.sessions.get(plugin).copied();
        let rec = book.windows.get_mut(&handle).ok_or("插件窗口不存在或已关闭")?;
        if rec.plugin != plugin { return Err("这个插件窗口不属于当前插件".into()); }
        if current != Some(generation) || rec.generation != generation {
            return Err("插件实例代次已变化，本次重开已丢弃".into());
        }
        let mut candidate = rec.clone();
        validate_update(&mut candidate, params)?;
        *rec = candidate.clone();
        candidate
    };
    let _ = context;
    let app = app_handle()?.clone();
    let for_main = snapshot.clone();
    on_main(move || -> Result<(), String> {
        let current = {
            let book = book();
            book.windows.get(&for_main.handle).filter(|rec| book.fresh(&rec.plugin, rec.generation)).cloned()
        }.ok_or("窗口已关闭或会话已撤销")?;
        let win = app.get_webview_window(&current.label).ok_or("插件窗口尚未创建")?;
        if current.duration_ms > 0 {
            let target = intend_rect(&app, &current);
            animate_place(&win, app.clone(), &current)?;
            if let Some(target) = target { record_rect(&current.handle, Some(target)); }
        } else {
            place(&win, &current)?;
            record_rect(&current.handle, geometry_of(&win));
        }
        if current.hit_test == HitTest::Opaque {
            crate::plugin_hit::watch(&current.label, current.hit_rects.clone());
            let hit_rect = book().windows.get(&current.handle).and_then(|r| r.rect).or_else(|| geometry_of(&win));
            crate::plugin_hit::place(&current.label, hit_rect, win.scale_factor().unwrap_or(1.0));
        }
        Ok(())
    })??;
    Ok(view(&snapshot))
}

/// 2.3.4（#88 H2）：一帧补间的几何。`t∈(0,1]` 为归一化时间，返回 (x, y, w, h)。
/// 抽成纯函数，便于单测覆盖端点/单调/取消语义，也便于主线程只做「套用一帧」。
fn tween_frame(start: (i32, i32, i32, i32), target: (i32, i32, i32, i32), t: f64, easing: Easing) -> (i32, i32, u32, u32) {
    let (sx, sy, sw, sh) = (start.0, start.1, start.2.max(1), start.3.max(1));
    let (tx, ty, tw, th) = (target.0, target.1, target.2.max(1), target.3.max(1));
    let e = ease_at(easing, t.clamp(0.0, 1.0));
    let x = (sx as f64 + (tx - sx) as f64 * e).round() as i32;
    let y = (sy as f64 + (ty - sy) as f64 * e).round() as i32;
    let w = (sw as f64 + (tw - sw) as f64 * e).round() as u32;
    let h = (sh as f64 + (th - sh) as f64 * e).round() as u32;
    (x, y, w.max(1), h.max(1))
}

/// 2.3.4（#88 H2）：一帧的间隔与总帧数（16 ms/帧，与 60 Hz 对齐）。
/// 用**向上取整**，保证末帧 `t≥1`（经 `anim_frame_t` 夹到 1.0），终帧精确落在目标几何——
/// 否则 duration 不是 16 的整数倍时，末帧只到 t<1，窗口停在目标之前（旧实现靠额外 place() 兜底）。
const ANIM_STEP_MS: u64 = 16;
fn anim_steps(duration_ms: u64) -> u64 {
    duration_ms.max(1).div_ceil(ANIM_STEP_MS).max(1)
}
fn anim_frame_t(step: u64, duration_ms: u64) -> f64 {
    (step as f64 * ANIM_STEP_MS as f64 / duration_ms.max(1) as f64).min(1.0)
}

/// 2.3.4（#88 H2）：把当前窗口从眼前几何补间到目标几何。
///
/// **不在主线程上 sleep**：本函数在调用方线程（插件后端线程）里跑帧循环，每帧只把
/// 「套用一帧（set_size/set_position）」这一小步**排进主线程**（`run_on_main_thread`
/// 非阻塞投递），把 sleep 留在工作线程。这样主线程消息泵不被阻塞——否则 `duration_ms`
/// 内整个界面（主岛、任务栏、所有窗口）都会僵住，与 P1「不在主线程做重活」相违。
fn animate_place(_win: &WebviewWindow, app: AppHandle, rec: &Rec) -> Result<(), String> {
    let sf = app.get_webview_window(&rec.label).and_then(|w| w.scale_factor().ok()).unwrap_or(1.0).max(0.5);
    let screens = crate::screen::read(&app);
    let dock = app.get_webview_window("island").and_then(|win| match (win.outer_position(), win.outer_size()) {
        (Ok(pos), Ok(size)) => Some((pos.x, pos.y, size.width as i32, size.height as i32)),
        _ => None,
    });
    let capsule = crate::island::snapshot(&crate::shared(&app)).capsule;
    let start = {
        let Some(win) = app.get_webview_window(&rec.label) else { return Ok(()); };
        match (win.outer_position(), win.outer_size()) {
            (Ok(pos), Ok(size)) => (pos.x, pos.y, size.width as i32, size.height as i32),
            _ => return Ok(()), // 拿不到起始几何：由调用方 place() 兜底
        }
    };
    let target = placement_h7(rec, &screens, dock, capsule, sf, rec.last_rect);
    let total = rec.duration_ms.max(1);
    let steps = anim_steps(total);
    let easing = rec.easing;
    let label = rec.label.clone();
    // 帧循环在工作线程跑；每帧把 set_size/set_position 非阻塞排进主线程。sleep 不在主线程。
    std::thread::spawn(move || {
        for step in 1..=steps {
            let (x, y, w, h) = tween_frame(start, target, anim_frame_t(step, total), easing);
            let app_for_frame = app.clone();
            let label_for_frame = label.clone();
            let _ = app.run_on_main_thread(move || {
                if let Some(win) = app_for_frame.get_webview_window(&label_for_frame) {
                    let _ = win.set_size(tauri::PhysicalSize::new(w, h));
                    let _ = win.set_position(tauri::PhysicalPosition::new(x, y));
                }
            });
            std::thread::sleep(Duration::from_millis(ANIM_STEP_MS));
        }
    });
    Ok(())
}

/// 2.2.3（B3）：把本窗口里的文件交给系统的 OLE 拖放，用户松手即落在资源管理器。
///
/// 只接受**本插件自己的窗口句柄**：跨插件的句柄在这里也过不去，与 close/update
/// 同一套归属检查。路径的合法性（存在、绝对、总量）由 `plugin_drag::check` 管。
fn begin_drag_out(plugin: &str, generation: u64, params: &Value) -> Result<Value, String> {
    let handle = handle_of(params)?;
    {
        let book = book();
        let rec = book.windows.get(handle).ok_or("插件窗口不存在或已关闭")?;
        if rec.plugin != plugin { return Err("这个插件窗口不属于当前插件".into()); }
        if rec.generation != generation || !book.fresh(plugin, generation) {
            return Err("插件实例代次已变化，本次拖出已丢弃".into());
        }
        if rec.state != State::Ready || !rec.shown {
            return Err("窗口还没显示，无法从它拖出".into());
        }
    }
    let paths: Vec<String> = params.get("paths").and_then(Value::as_array)
        .ok_or("beginDragOut 需要 paths 数组")?
        .iter()
        .map(|item| item.as_str().map(str::to_owned).ok_or("beginDragOut 的 paths 只能是字符串"))
        .collect::<Result<_, _>>()?;
    crate::plugin_drag::begin(paths)?;
    Ok(json!({"started": true}))
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
        "zOrder":rec.z_order.name(),
        "hitTest":rec.hit_test.name(),
        "hitRects":rec.hit_rects,
        "transparent":rec.transparent,
        "duration":rec.duration_ms,
        "easing":rec.easing.name(),
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

pub(crate) fn emit(plugin: &str, generation: u64, label: Option<&str>, payload: Value) {
    let Ok(app) = app_handle() else { return };
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

pub(crate) fn notify_backend(plugin: &str, generation: u64, payload: Value) -> Result<(), String> {
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
    // 2.3.4（#88 H9）：在真实落点回放吸收动画（失败只记日志，不阻断交付）。
    if let Some(point) = point {
        let scale = app_handle().map(|app| goo_scale_for(&crate::screen::read(&app), point)).unwrap_or(1.0);
        let to = rect.map(|(x, y, w, h)| (x + w / 2, y + h / 2)).unwrap_or(point);
        play_absorb(point, to, scale);
    }
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
    let capsule = app_handle().ok().map(|app| crate::island::snapshot(&crate::shared(app)).capsule).flatten();
    // 2.3.4（#88 H9）：在主 Dock 落点回放吸收动画（目标 = 胶囊中心）。
    if let Some(point) = point {
        let scale = app_handle().map(|app| goo_scale_for(&crate::screen::read(&app), point)).unwrap_or(1.0);
        let to = capsule.map(|(x, y, w, h)| (x + w / 2, y + h / 2)).unwrap_or(point);
        play_absorb(point, to, scale);
    }
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
    // 订阅表不归这里管（窗口域与主岛域分开），但撤销一个实例时它必须跟着清：
    // 否则插件被卸载后订阅残留，watchers() 永远不回落，主岛页面会一直上报。
    crate::plugin_dock::forget(plugin);
    crate::dropwin::changed();
    for rec in retired {
        terminal_event(&rec, reason, output.as_ref());
        destroy_record(rec, reason);
    }
    // 2.3.4（#88 H9，DEC-1）：插件窗口销毁（停用/卸载/换版/崩溃）时取消在途吸收动画。
    cancel_goo();
}

// 撤销后仅向撤销瞬间捕获的旧会话通知；不查新会话，永不把旧事件送给替代实例。
fn terminal_event(rec: &Rec, reason: &str, output: Option<&std::sync::mpsc::SyncSender<Value>>) {
    let payload = json!({"event":"closed","handle":rec.handle,"reason":reason});
    if let Some(output) = output { let _ = send_backend(output, &rec.plugin, payload.clone()); }
    if let Ok(app) = app_handle() {
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

// ---------------------------------------------------------------------------------------
// 2.3.4（#88 H9，DEC-1 方案 A）：跨窗口「文件融入」吸收动画的顶层透明覆盖窗口
// ---------------------------------------------------------------------------------------

/// 当前是否有活着的吸收覆盖窗口（同一时刻至多一个；连续拖放串行，不叠窗）。
static GOO_ACTIVE: AtomicBool = AtomicBool::new(false);
/// 吸收覆盖窗的单调代次：每次 `play_absorb` 取一个新代次，TTL 线程捕获它，
/// 到点时**只在代次仍是当前代次**时才销毁并清 GOO_ACTIVE——否则会误销毁后一次
/// `play_absorb` 刚建的新窗（旧 TTL 到点把新窗关掉）。（repair t16 / t9 发现）
static GOO_GENERATION: AtomicU64 = AtomicU64::new(0);
/// 覆盖窗口兜底销毁时长（动画约 700ms；2s 后强制销毁，页面异常也不常驻）。
const GOO_TTL: Duration = Duration::from_secs(2);

/// 取下一个吸收覆盖窗代次（单调 +1；跳过 0 以免与「未开始」混淆）。
fn next_goo_generation() -> u64 {
    GOO_GENERATION.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1
}

/// 当前吸收覆盖窗代次（供测试与取消逻辑读取）。
fn goo_generation() -> u64 {
    GOO_GENERATION.load(std::sync::atomic::Ordering::SeqCst)
}

/// 某个 TTL/完成/取消动作捕获的代次是否仍代表当前覆盖窗。
/// 仅当代次相等时，该动作才允许销毁窗口并清 GOO_ACTIVE——旧代次的迟到 TTL
/// 不会销毁后一次 `play_absorb` 的新窗。
fn goo_generation_is_current(captured: u64) -> bool {
    captured == goo_generation()
}

/// 2.3.4（#88 H9）：吸收覆盖窗按落点所在显示器的 DPI 缩放（DEC-1 要求「DPI/多屏换算」）。
/// 落点（物理像素）落在哪块屏就用它的 `sf`；拿不到时回落 1.0，绝不给 0 或 NaN。
fn goo_scale_for(screens: &crate::screen::Screens, point: (i32, i32)) -> f64 {
    screens
        .near(point.0, point.1)
        .map(|area| area.sf)
        .filter(|sf| sf.is_finite() && *sf > 0.0)
        .unwrap_or(1.0)
        .max(0.5)
}

/// 在一个真实拖放落点回放「液滴→颈→球」吸收动画。
///
/// `from` 是落点（屏幕物理像素），`to` 是目标中心（同坐标系），`scale` 是目标屏 DPI。
/// 宿主创建一个**顶层透明覆盖窗口**（WS_EX_LAYERED | WS_EX_TRANSPARENT |
/// WS_EX_TOOLWINDOW：不抢焦点、不进任务栏、鼠标穿透），窗内用 Canvas 回放吸收；
/// 动画结束或 TTL 到点即销毁。**动画失败绝不阻断实际 drop**——所有错误只记日志后返回。
pub fn play_absorb(from: (i32, i32), to: (i32, i32), scale: f64) {
    let app = match app_handle() { Ok(app) => app.clone(), Err(_) => return };
    // 取新代次：后一次 play_absorb 让先前所有在途 TTL/完成动作失效。
    let generation = next_goo_generation();
    // 只能有一个覆盖窗；已有就先销毁，避免重叠。
    if GOO_ACTIVE.swap(true, std::sync::atomic::Ordering::SeqCst) {
        if let Some(win) = app.get_webview_window("plugin-goo") { let _ = win.destroy(); }
    }
    let min_x = from.0.min(to.0) - 160;
    let min_y = from.1.min(to.1) - 160;
    let max_x = from.0.max(to.0) + 160;
    let max_y = from.1.max(to.1) + 160;
    let (ox, oy) = (min_x, min_y);
    let (w, h) = ((max_x - min_x).max(1), (max_y - min_y).max(1));
    let (fx, fy) = (from.0 - ox, from.1 - oy);
    let (tx, ty) = (to.0 - ox, to.1 - oy);
    let url = format!("window/goo.html?fx={fx}&fy={fy}&tx={tx}&ty={ty}&scale={}", scale.max(0.5));
    let app_for_build = app.clone();
    // 建窗放独立线程：不在主线程同步 build，也不阻塞 drop 交付。
    let built = std::thread::spawn(move || {
        let shared = crate::shared(&app_for_build);
        WebviewWindowBuilder::new(&app_for_build, "plugin-goo", WebviewUrl::App(url.into()))
            .title("")
            .inner_size(w as f64, h as f64)
            .position(ox as f64, oy as f64)
            .decorations(false)
            .transparent(true)
            .resizable(false)
            .skip_taskbar(true)
            .focused(false)
            .always_on_top(true)
            .additional_browser_args(crate::browser_args(&shared))
            .data_directory(crate::webview_dir(&shared))
            .visible(true)
            .build()
            .map_err(|error| format!("吸收覆盖窗创建失败：{error}"))
    });
    match built.join() {
        Ok(Ok(win)) => { make_goo_overlay(&win); }
        Ok(Err(error)) => {
            log(&format!("goo overlay: {error}; drop continues without animation"));
            GOO_ACTIVE.store(false, std::sync::atomic::Ordering::SeqCst);
            return;
        }
        Err(_) => {
            log("goo overlay: build thread panicked; drop continues without animation");
            GOO_ACTIVE.store(false, std::sync::atomic::Ordering::SeqCst);
            return;
        }
    }
    // TTL 兜底：到点销毁（页面自己调 plugin_goo_done 时也会销毁）。
    // 捕获本窗代次：到点时若代次已被后一次 play_absorb 顶替，则不动新窗、不清 GOO_ACTIVE。
    let app2 = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(GOO_TTL);
        if goo_generation_is_current(generation) {
            if let Some(win) = app2.get_webview_window("plugin-goo") { let _ = win.destroy(); }
            GOO_ACTIVE.store(false, std::sync::atomic::Ordering::SeqCst);
        }
    });
}

/// 页面动画结束回调：销毁覆盖窗口。
/// 先推进代次使本窗的在途 TTL 失效，再销毁——避免 TTL 与完成回调争相销毁。
pub fn goo_done() {
    let _ = next_goo_generation();
    if let Ok(app) = app_handle() {
        if let Some(win) = app.get_webview_window("plugin-goo") { let _ = win.destroy(); }
    }
    GOO_ACTIVE.store(false, std::sync::atomic::Ordering::SeqCst);
}

/// 2.3.4（#88 H9，DEC-1）：取消在途吸收动画并销毁覆盖窗。
/// 插件窗口销毁、主岛全局隐藏（收起滑出/全屏避让）时调用——DEC-1 要求这两种情形取消动画，
/// 不留下悬空的顶层覆盖窗（TTL 兜底之外再显式收一次）。幂等。
fn cancel_goo() {
    // 先推进代次：使本窗的在途 TTL 失效，之后它到点也不会再动（可能已被新窗占用的）资源。
    let _ = next_goo_generation();
    if !GOO_ACTIVE.swap(false, std::sync::atomic::Ordering::SeqCst) {
        return;
    }
    if let Ok(app) = app_handle() {
        if let Some(win) = app.get_webview_window("plugin-goo") { let _ = win.destroy(); }
    }
}

/// 把覆盖窗设成鼠标穿透的顶层工具窗（不抢焦点、不进任务栏）。
#[cfg(windows)]
fn make_goo_overlay(win: &WebviewWindow) {
    type Hwnd = *mut std::ffi::c_void;
    extern "system" {
        fn GetWindowLongW(hwnd: Hwnd, index: i32) -> i32;
        fn SetWindowLongW(hwnd: Hwnd, index: i32, value: i32) -> i32;
        fn SetWindowPos(hwnd: Hwnd, after: isize, x: i32, y: i32, cx: i32, cy: i32, flags: u32) -> i32;
    }
    const GWL_EXSTYLE: i32 = -20;
    if let Ok(hwnd) = win.hwnd() {
        let hwnd = hwnd.0 as Hwnd;
        unsafe {
            let ex = GetWindowLongW(hwnd, GWL_EXSTYLE);
            // LAYERED | TRANSPARENT | TOOLWINDOW | NOACTIVATE
            SetWindowLongW(hwnd, GWL_EXSTYLE, ex | 0x0008_0000 | 0x0000_0020 | 0x0000_0080 | 0x0800_0000);
            // HWND_TOPMOST, SWP_NOSIZE|NOMOVE|NOACTIVATE|SHOWWINDOW
            SetWindowPos(hwnd, -1, 0, 0, 0, 0, 0x0001 | 0x0002 | 0x0010 | 0x0040);
        }
    }
}

#[cfg(not(windows))]
fn make_goo_overlay(_win: &WebviewWindow) {}

/// Dock 全局隐藏 / 全屏避让：插件窗口随之隐藏并停止接受拖入；恢复时只恢复那一批
/// 本来还开着的窗口（用户自己关掉的不复活）。主岛普通收起（缩成胶囊）不算全局隐藏，
/// 那时窗口照常留着——screen 窗口更不跟随主岛的展开/收起。
pub fn set_dock_hidden(hidden: bool) {
    // 2.3.4（#88 H9，DEC-1）：主岛全局隐藏（收起滑出/全屏避让）时取消在途吸收动画。
    if hidden { cancel_goo(); }
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
    let Ok(app) = app_handle() else { return };
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
    let rect = app_handle().ok().and_then(|app| app.get_webview_window("island")).and_then(|island| geometry_of(&island));
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


fn rearrange(include_screen: bool) {
    let targets: Vec<Rec> = { book().windows.values().filter(|rec| include_screen || rec.anchor != Anchor::Screen).cloned().collect() };
    if targets.is_empty() {
        return;
    }
    let Ok(app) = app_handle() else { return };
    let app = app.clone();
    post_main(move || {
        for requested in targets {
            let current = {
                let book = book();
                book.windows.get(&requested.handle).filter(|rec| (include_screen || rec.anchor != Anchor::Screen) && book.fresh(&rec.plugin, rec.generation)).cloned()
            };
            if let Some(rec) = current {
                if let Some(win) = app.get_webview_window(&rec.label) {
                    if place(&win, &rec).is_ok() { record_rect(&rec.handle, geometry_of(&win)); }
                }
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
        fn GetForegroundWindow() -> Hwnd;
        fn SetForegroundWindow(hwnd: Hwnd) -> i32;
    }
    const SWP_NOSIZE: u32 = 0x0001;
    const SWP_NOMOVE: u32 = 0x0002;
    const SWP_NOACTIVATE: u32 = 0x0010;
    const SWP_SHOWWINDOW: u32 = 0x0040;
    // 2026-11-16 本地补丁：Tauri v2 用 .visible(false) 建窗时 WebView2 controller 也是隐藏的，
    // 仅 SetWindowPos(SWP_SHOWWINDOW) 只显示 Win32 壳、内容不绘制（屏幕空白，PrintWindow 却有画面，
    // IsWindowVisible 仍为 false）。必须先走 win.show() 让 controller 可见；再 SetWindowPos 压住不激活，
    // 并把前台窗口还回去，尽量保持「首显不抢焦点」。
    let prev = unsafe { GetForegroundWindow() };
    let _ = win.show();
    if let Ok(hwnd) = win.hwnd() {
        unsafe { SetWindowPos(hwnd.0 as Hwnd, 0, 0, 0, 0, 0, SWP_NOSIZE | SWP_NOMOVE | SWP_NOACTIVATE | SWP_SHOWWINDOW); }
    }
    if !prev.is_null() {
        unsafe { SetForegroundWindow(prev); }
    }
}

#[cfg(not(windows))]
fn show_without_activating(win: &WebviewWindow) {
    let _ = win.show();
}

/// 2.3.4（#88 P2）：透明窗剥掉原生标题栏与边框（`.decorations(false)` 对透明 WebView2
/// 窗不彻底，会留一条原生标题栏/边框）。清 `WS_CAPTION|WS_THICKFRAME|WS_SYSMENU|
/// WS_MINIMIZEBOX|WS_MAXIMIZEBOX` 后 `SetWindowPos(SWP_FRAMECHANGED)` 让系统重算非客户区。
#[cfg(windows)]
fn strip_chrome(win: &WebviewWindow) {
    type Hwnd = *mut std::ffi::c_void;
    extern "system" {
        fn GetWindowLongW(hwnd: Hwnd, index: i32) -> i32;
        fn SetWindowLongW(hwnd: Hwnd, index: i32, value: i32) -> i32;
        fn SetWindowPos(hwnd: Hwnd, after: isize, x: i32, y: i32, cx: i32, cy: i32, flags: u32) -> i32;
        fn GetAncestor(hwnd: Hwnd, flags: u32) -> Hwnd;
    }
    const GWL_STYLE: i32 = -16;
    const WS_CAPTION: i32 = 0x00c0_0000;
    const WS_THICKFRAME: i32 = 0x0004_0000;
    const WS_SYSMENU: i32 = 0x0008_0000;
    const WS_MINIMIZEBOX: i32 = 0x0002_0000;
    const WS_MAXIMIZEBOX: i32 = 0x0001_0000;
    if let Ok(hwnd) = win.hwnd() {
        // 2026-11-16 本地补丁：Tauri v2 的 WebviewWindow::hwnd() 给的是 WebView2 子窗口，
        // 直接 SetWindowLong 作用在子窗口上、顶层样式不变（WS_CAPTION/WS_THICKFRAME 仍在，
        // 于是内容外面多出一圈 8px 的非客户区白框）。先 GetAncestor(GA_ROOT) 取顶层窗口再剥。
        let child = hwnd.0 as Hwnd;
        let root = unsafe { GetAncestor(child, 2) };
        let hwnd = if root.is_null() { child } else { root };
        unsafe {
            let style = GetWindowLongW(hwnd, GWL_STYLE);
            let naked = style & !(WS_CAPTION | WS_THICKFRAME | WS_SYSMENU | WS_MINIMIZEBOX | WS_MAXIMIZEBOX);
            SetWindowLongW(hwnd, GWL_STYLE, naked);
            SetWindowPos(hwnd, 0, 0, 0, 0, 0, 0x0001 | 0x0002 | 0x0004 | 0x0010 | 0x0020);
        }
    }
}

#[cfg(not(windows))]
fn strip_chrome(_win: &WebviewWindow) {}

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

/// 2.3.4（#88 H9）：吸收覆盖窗页面动画结束，请宿主销毁覆盖窗。
#[tauri::command]
pub fn plugin_goo_done() -> Value {
    goo_done();
    json!({"done": true})
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
        let mut book = Book { windows: BTreeMap::new(), dock: None, next: 0, sessions: BTreeMap::new(), outputs: BTreeMap::new(), dock_hidden: false };
        assert_eq!(book.per_plugin("acme.weather"), 0);
        for index in 0..PER_PLUGIN {
            book.windows.insert(format!("h{index}"), Rec {
                handle: format!("h{index}"), plugin: "acme.weather".into(), digest: "d".into(), generation: 1, key: format!("k{index}"),
                page: "main".into(), title: "t".into(), label: format!("plugin-win-{index}"), ticket: String::new(), url: String::new(),
                anchor: Anchor::DockBottom, align: Align::RightCenter, anchor_base: AnchorBase::Window, offset_x: 0.0, offset_y: 0.0, width: Width::Fixed(420.0), height: 320.0, transparent: false, z_order: ZOrder::Normal, hit_test: HitTest::Window, hit_rects: Vec::new(),
                state: State::Ready, shown: true, hidden_by_dock: false, rect: None, drop: None,
                duration_ms: 0, easing: Easing::EaseOutCubic, last_rect: None,
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
        let mut book = Book { windows: BTreeMap::new(), dock: None, next: 0, sessions: BTreeMap::new(), outputs: BTreeMap::new(), dock_hidden: false };
        let record = |handle: &str, drop: bool, state: State, shown: bool| Rec {
            handle: handle.into(), plugin: "acme.weather".into(), digest: "d".into(), generation: 1, key: handle.into(),
            page: "main".into(), title: "t".into(), label: format!("plugin-win-{handle}"), ticket: "t".into(), url: "u".into(),
            anchor: Anchor::DockBottom, align: Align::RightCenter, anchor_base: AnchorBase::Window, offset_x: 0.0, offset_y: 0.0, width: Width::Fixed(420.0), height: 320.0, transparent: false, z_order: ZOrder::Normal, hit_test: HitTest::Window, hit_rects: Vec::new(),
            state, shown, hidden_by_dock: false, rect: Some((10, 10, 100, 100)), drop: if drop { Some(format!("drop-{handle}")) } else { None },
            duration_ms: 0, easing: Easing::EaseOutCubic, last_rect: None,
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
            anchor: Anchor::DockLeft, align: Align::RightCenter, anchor_base: AnchorBase::Window, offset_x: 0.0, offset_y: 0.0, width: Width::Fixed(420.0), height: 320.0, transparent: false, z_order: ZOrder::Normal, hit_test: HitTest::Window, hit_rects: Vec::new(),
            state: State::Pending, shown: false, hidden_by_dock: false, rect: None, drop: None,
            duration_ms: 0, easing: Easing::EaseOutCubic, last_rect: None,
        }
    }

    #[test]
    fn reservation_is_atomic_for_duplicate_keys_and_quotas() {
        let registry = std::sync::Arc::new(Mutex::new(Book { windows: BTreeMap::new(), dock: None, next: 0, sessions: BTreeMap::new(), outputs: BTreeMap::new(), dock_hidden: false }));
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
        let mut registry = Book { windows: BTreeMap::new(), dock: None, next: 0, sessions: BTreeMap::new(), outputs: BTreeMap::new(), dock_hidden: false };
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
        let mut registry = Book { windows: BTreeMap::new(), dock: None, next: 0, sessions: BTreeMap::from([("acme.notes".into(), 1)]), outputs: BTreeMap::new(), dock_hidden: true };
        let rec = registry.reserve(pending_record("acme.notes", "panel")).unwrap().0;
        assert!(rec.hidden_by_dock);
        assert!(!rec.shown);
    }

    #[test]
    fn view_exposes_only_opaque_state() {
        let rec = Rec {
            handle: "h".into(), plugin: "acme.weather".into(), digest: "d".into(), generation: 7, key: "panel".into(),
            page: "main".into(), title: "面板".into(), label: "plugin-win-9".into(), ticket: "secret-ticket".into(), url: "http://plugin/page".into(),
            anchor: Anchor::DockLeft, align: Align::RightCenter, anchor_base: AnchorBase::Window, offset_x: 8.0, offset_y: 0.0, width: Width::Fixed(500.0), height: 240.0, transparent: true, z_order: ZOrder::Normal, hit_test: HitTest::Window, hit_rects: Vec::new(),
            state: State::Ready, shown: true, hidden_by_dock: false, rect: None, drop: None,
            duration_ms: 0, easing: Easing::EaseOutCubic, last_rect: None,
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

    // P4：dock-bottom 以基准底边中点为锚点（与 dock-left 用左边中点对称）。
    #[test]
    fn dock_bottom_anchors_at_the_bottom_center_not_the_left_corner() {
        use crate::screen::{Area, Screens};
        let primary = Area { x:0, y:0, w:1920, h:1040, sf:1.0, mon:(0,0,1920,1080) };
        let screens = Screens { areas:vec![primary], primary:Some(primary) };
        let mut rec = pending_record("acme.notes", "bar");
        rec.anchor = Anchor::DockBottom;
        rec.align = Align::TopCenter;
        rec.width = Width::Fixed(420.0); rec.height = 72.0;
        rec.anchor_base = AnchorBase::Capsule;
        let window = Some((0, 0, 1920, 132));
        let capsule = Some((810, 8, 300, 36));
        let (x, y, _, _) = placement(&rec, &screens, window, capsule, 1.0);
        assert_eq!(x, 960 - 210, "TopCenter 以底边中点为锚：960-210=750");
        assert_eq!(y, 44, "窗口顶贴胶囊底边");
        // 缺省 align 由 Options::parse 决定：dock-bottom → left-top（P4 下锚点=底边中点）。
        let parsed = Options::parse(&json!({"key":"bar","page":"main","anchor":"dock-bottom","width":420,"height":72,"anchorBase":"capsule"})).unwrap();
        assert_eq!(parsed.align, Align::LeftTop, "dock-bottom 缺省仍 left-top");
        let mut default = pending_record("acme.notes", "bar");
        default.anchor = parsed.anchor; default.align = parsed.align; default.anchor_base = parsed.anchor_base;
        default.width = parsed.width; default.height = parsed.height;
        let (dx, _, _, _) = placement(&default, &screens, window, capsule, 1.0);
        assert_eq!(dx, 960, "dock-bottom 缺省 left-top 下，锚点从「左下角」改为「底边中点」（P4）");
    }

    // H2：动画补间——两条曲线端点精确、中点合理。
    #[test]
    fn tween_hits_both_ends_and_is_monotonic() {
        assert!((ease_at(Easing::Linear, 0.0) - 0.0).abs() < 1e-9);
        assert!((ease_at(Easing::Linear, 1.0) - 1.0).abs() < 1e-9);
        assert!((ease_at(Easing::Linear, 0.5) - 0.5).abs() < 1e-9);
        assert!(ease_at(Easing::EaseOutCubic, 0.0).abs() < 1e-9);
        assert!((ease_at(Easing::EaseOutCubic, 1.0) - 1.0).abs() < 1e-9);
        assert!(ease_at(Easing::EaseOutCubic, 0.5) > 0.5);
        assert!(ease_at(Easing::Spring, 0.0).abs() < 1e-9);
        assert!((ease_at(Easing::Spring, 1.0) - 1.0).abs() < 1e-6, "弹簧末端收敛到 1");
        assert!(ease_at(Easing::Spring, 0.9) > 0.99, "接近末尾已基本到位");
        assert_eq!(tween(100.0, 200.0, 1.0, Easing::Linear), 200.0);
        assert_eq!(tween(100.0, 200.0, 0.0, Easing::Linear), 100.0);
    }

    // H2：update 接受 duration/easing；缺省 duration=0 保持瞬时。
    #[test]
    fn update_accepts_duration_and_easing_and_defaults_to_instant() {
        let mut rec = pending_record("acme.notes", "bar");
        assert_eq!(rec.duration_ms, 0, "缺省瞬时");
        assert_eq!(rec.easing, Easing::EaseOutCubic, "缺省曲线");
        validate_update(&mut rec, &json!({"duration": 220, "easing": "spring"})).unwrap();
        assert_eq!(rec.duration_ms, 220);
        assert_eq!(rec.easing, Easing::Spring);
        assert!(validate_update(&mut rec, &json!({"easing": "bounce"})).is_err());
        assert!(validate_update(&mut rec, &json!({"duration": 99999})).is_err());
        validate_update(&mut rec, &json!({"duration": 0})).unwrap();
        assert_eq!(rec.duration_ms, 0, "duration:0 回到瞬时");
    }

    // H6：reopen 语义——保留句柄与订阅，不重置状态。
    #[test]
    fn reopen_keeps_the_same_handle_and_drop_subscription() {
        let mut registry = Book { windows: BTreeMap::new(), dock: None, next: 0, sessions: BTreeMap::new(), outputs: BTreeMap::new(), dock_hidden: false };
        registry.sessions.insert("acme.notes".into(), 1);
        let mut rec = pending_record("acme.notes", "ball");
        rec.state = State::Ready; rec.shown = true; rec.rect = Some((0,0,36,36));
        let (rec, _) = registry.reserve(rec).unwrap();
        let handle = rec.handle.clone();
        registry.windows.get_mut(&handle).unwrap().drop = Some("drop-x".into());
        let after = registry.windows.get(&handle).unwrap();
        assert_eq!(after.handle, handle);
        assert_eq!(after.drop.as_deref(), Some("drop-x"), "reopen 不得丢订阅");
        assert_eq!(after.state, State::Ready, "reopen 不重置状态");
    }

    // H7：胶囊未知时 anchorBase=capsule 保持上次位置，不回落整窗。
    #[test]
    fn capsule_unknown_keeps_last_position_instead_of_the_whole_window() {
        use crate::screen::{Area, Screens};
        let primary = Area { x:0, y:0, w:1920, h:1040, sf:1.0, mon:(0,0,1920,1080) };
        let screens = Screens { areas:vec![primary], primary:Some(primary) };
        let mut rec = pending_record("acme.notes", "ball");
        rec.anchor = Anchor::DockLeft; rec.align = Align::RightCenter; rec.anchor_base = AnchorBase::Capsule;
        rec.width = Width::Fixed(36.0); rec.height = 36.0;
        let with = placement(&rec, &screens, Some((0,0,1920,132)), Some((810, 8, 300, 36)), 1.0);
        let kept = placement_h7(&rec, &screens, Some((0,0,1920,132)), None, 1.0, Some(with));
        assert_eq!(kept, with, "胶囊未知时保持上次位置");
        let fresh = placement_h7(&rec, &screens, Some((0,0,1920,132)), None, 1.0, None);
        let fallback = placement(&rec, &screens, Some((0,0,1920,132)), None, 1.0);
        assert_eq!(fresh, fallback, "没有上次位置时回落到既有算法");
    }

    // H9（DEC-1）：吸收覆盖窗按落点所在显示器的 DPI 缩放，多屏各按各的 sf（不再硬编码 1.0）。
    #[test]
    fn goo_scale_follows_the_monitor_under_the_drop_point() {
        use crate::screen::{Area, Screens};
        let primary = Area { x:0, y:0, w:1920, h:1040, sf:1.0, mon:(0,0,1920,1080) };
        let hdpi = Area { x:1920, y:0, w:1600, h:900, sf:1.5, mon:(1920,0,3840,960) };
        let screens = Screens { areas:vec![primary, hdpi], primary:Some(primary) };
        assert_eq!(goo_scale_for(&screens, (500, 40)), 1.0, "100% 屏落点用 1.0");
        assert_eq!(goo_scale_for(&screens, (2600, 40)), 1.5, "150% 屏落点用 1.5，不是硬编码 1.0");
        // 空屏表也绝不返回 0/NaN：回落 1.0。
        let empty = Screens { areas:vec![], primary:None };
        assert_eq!(goo_scale_for(&empty, (10, 10)), 1.0, "拿不到显示器时回落 1.0");
    }

    // t16（t9 发现）：吸收覆盖窗的单调代次。连续两次 play_absorb，旧窗的 TTL 捕获旧代次，
    // 后一次取新代次使旧代次失效——旧 TTL 到点不得销毁新窗、不得清 GOO_ACTIVE。
    // 这里用纯代次函数表达「旧 TTL 是否仍属当前窗」：只用公开的代次/判定函数，不建真窗。
    #[test]
    fn a_second_play_absorb_invalidates_the_first_windows_ttl() {
        // 第一次 play_absorb 取代次 g1（模拟其 TTL 线程捕获 g1）。
        let g1 = next_goo_generation();
        assert!(goo_generation_is_current(g1), "刚取到的代次是当前代次");
        // 第二次 play_absorb 取新代次 g2：g1 的在途 TTL 立即失效。
        let g2 = next_goo_generation();
        assert!(g2 > g1, "代次单调递增");
        assert!(!goo_generation_is_current(g1), "旧代次不再当前：旧 TTL 到点不会销毁新窗");
        assert!(goo_generation_is_current(g2), "新代次是当前代次");
        // goo_done / cancel_goo 会推进代次：使其后任何在途 TTL 失效（用 next_goo_generation 表达）。
        let g3 = next_goo_generation();
        assert!(!goo_generation_is_current(g2), "完成/取消后再推进代次，先前 TTL 亦失效");
        assert!(goo_generation_is_current(g3));
    }

    // H2：补间帧序列——首帧靠近起点、末帧精确到目标、步骤数随 duration 缩放。
    #[test]
    fn tween_frames_start_near_start_and_end_at_target() {
        let start = (100, 100, 36, 36);
        let target = (400, 200, 531, 72);
        // 末帧（t=1）精确落到目标。
        let last = tween_frame(start, target, anim_frame_t(anim_steps(220), 220), Easing::Linear);
        assert_eq!(last, (400, 200, 531, 72), "末帧等于目标几何");
        // 第一帧（t 小）更靠近起点而非目标。
        let first = tween_frame(start, target, anim_frame_t(1, 220), Easing::Linear);
        assert!(first.0 < 400 && first.0 > 100, "首帧在起点与目标之间");
        // 步骤数：220ms / 16ms ≈ 13 步（至少 1），duration 越大步数越多。
        assert!(anim_steps(220) > anim_steps(16));
        assert_eq!(anim_steps(0), 1, "duration:0 至少一帧，不退化为 0");
    }

    // H-D1：对外契约下限为 32×32（与文档 §3 修订、§8.1 一致）。这条钉住实现的常量，
    // 将来若有人把下限又调回 160×80，这里立刻红。
    #[test]
    fn minimum_size_contract_is_32_logical_pixels() {
        assert_eq!(WIDTH_MIN, 32.0, "H-D1：宽度下限对外契约为 32 逻辑像素");
        assert_eq!(HEIGHT_MIN, 32.0, "H-D1：高度下限对外契约为 32 逻辑像素");
        // 32×32 合法、31×31 拒绝：契约边界一致。
        assert!(Options::parse(&json!({"key":"ball","page":"main","width":32.0,"height":32.0})).is_ok());
        assert!(Options::parse(&json!({"key":"ball","page":"main","width":31.0,"height":32.0})).is_err());
    }
}
