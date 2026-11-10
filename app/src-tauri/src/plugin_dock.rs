//! 主岛几何与状态的订阅（2.2.2，issue #87 的 C1/C2/C5）。
//!
//! 这一层只回答三件事，不碰窗口生命周期：
//!
//!   · 主岛现在在哪、多大、什么形态（`rect`）；
//!   · 谁在听主岛的变化（订阅表）；
//!   · 变化时把快照推给谁（`notify_changed` / `notify_pointer`）。
//!
//! 它与 `plugin_window` 的分工是硬边界：窗口的开、关、位置、拖入都在
//! `plugin_window`；主岛快照的读与分发在这里。后者单向依赖前者的**事件投递
//! 原语**（`emit` / `notify_backend`），反过来不成立。
//!
//! **按需启用**：订阅表为空时主岛页面不上报任何东西（`island:watch-dock`），
//! 因此不装插件、装了不启用、或启用但没订阅的用户，这一层的代码一行都不执行。

use std::collections::BTreeMap;
use std::sync::{Mutex, MutexGuard};

use serde_json::{json, Value};

/// 订阅了主岛事件的插件 → 代次。空表示没人听。
static SUBSCRIBERS: Mutex<BTreeMap<String, u64>> = Mutex::new(BTreeMap::new());
/// 上一次推出去（或读到时）的快照，用来去重：只在真的变了才发事件。
static LAST: Mutex<Option<crate::island::DockSnapshot>> = Mutex::new(None);

fn subscribers() -> MutexGuard<'static, BTreeMap<String, u64>> {
    SUBSCRIBERS.lock().unwrap_or_else(|poison| poison.into_inner())
}
fn last() -> MutexGuard<'static, Option<crate::island::DockSnapshot>> {
    LAST.lock().unwrap_or_else(|poison| poison.into_inner())
}

/// 有多少插件在听。这是「按需启用」的唯一判据。
pub fn watchers() -> usize {
    subscribers().len()
}

/// 读一次主岛快照。宿主窗口还没建时返回 None。
fn snapshot() -> Option<crate::island::DockSnapshot> {
    let app = crate::plugin_window::app_handle().ok()?;
    let shared = crate::shared(app);
    Some(crate::island::snapshot(&shared))
}

/// 把快照整理成插件看到的形状。`window` 与 `capsule` 都是物理像素；
/// 胶囊尺寸未知时 `capsule` 为 null，不给假矩形。
pub fn rect_value(snapshot: &crate::island::DockSnapshot) -> Value {
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

/// `dock.rect()`：读一次实时快照。
pub fn rect() -> Result<Value, String> {
    snapshot().map(|snapshot| rect_value(&snapshot)).ok_or_else(|| "主岛尚未创建".to_string())
}

/// 当前所有仍在有效代次内的订阅者。已停用/换版的插件不在此列。
///
/// 先把表复制出来再查代次：`current_generation` 要拿窗口域的锁，若还持着订阅表
/// 的锁去拿，就成了「SUBSCRIBERS → BOOK」的嵌套。目前没有反方向的路径，但把
/// 它写成不嵌套的形式，后来的改动就不会无意中造出环。
pub fn live_subscribers() -> Vec<(String, u64)> {
    let table: Vec<(String, u64)> = {
        let table = subscribers();
        table.iter().map(|(plugin, generation)| (plugin.clone(), *generation)).collect()
    };
    table
        .into_iter()
        .filter(|(plugin, generation)| crate::plugin_window::current_generation(plugin) == *generation)
        .collect()
}

/// 把一条 payload 推给每个有效订阅者：页面与前端的 `plugin:windowEvent` 桥都收。
/// 返回实际投递到后端的条数，供测试与日志核对，不对外暴露细节。
fn broadcast(payload: &Value) -> usize {
    let targets = live_subscribers();
    let mut delivered = 0;
    for (plugin, generation) in targets {
        crate::plugin_window::emit(&plugin, generation, None, payload.clone());
        if crate::plugin_window::notify_backend(&plugin, generation, payload.clone()).is_ok() {
            delivered += 1;
        }
    }
    delivered
}

/// 主岛几何或状态变了。`island.rs` 在胶囊尺寸、视图形态、stow、全屏避让变化后调用。
///
/// 先去重：与上次相同就直接返回，不产生任何重排或 IPC。这条是滑动动画期间
/// 不刷爆队列的关键——页面每帧都报，但只有真正的变化才继续往下走。
pub fn notify_changed() {
    let Some(snapshot) = snapshot() else { return };
    {
        let mut last = last();
        if last.as_ref() == Some(&snapshot) {
            return;
        }
        *last = Some(snapshot.clone());
    }
    broadcast(&json!({"event":"dock","type":"geometry","rect":rect_value(&snapshot)}));
}

/// 指针进入/离开主岛胶囊。由 `island.rs` 的轮询在跨过边界时调用。
pub fn notify_pointer(on: bool) {
    broadcast(&json!({
        "event": "dock",
        "type": "pointer",
        "action": if on { "enter" } else { "leave" },
    }));
}

/// 订阅。同一个插件重复订阅是幂等的：不重复计数，也不会在退订时被提前清掉。
/// 从 0 变 1 时通知主岛页面开始上报。
pub fn subscribe(plugin: &str, generation: u64) -> Result<Value, String> {
    if crate::plugin_window::current_generation(plugin) != generation {
        return Err("插件会话已撤销".into());
    }
    let before = {
        let mut table = subscribers();
        let before = table.len();
        table.insert(plugin.to_string(), generation);
        before
    };
    if before == 0 {
        tell_island(true);
    }
    Ok(json!({"subscribed": true}))
}

/// 退订。回到 0 时通知主岛页面停止上报，并清掉去重快照——
/// 下一个订阅者要拿到的是现状，不是上一次的缓存。
pub fn unsubscribe(plugin: &str) -> Result<Value, String> {
    let after = {
        let mut table = subscribers();
        table.remove(plugin);
        table.len()
    };
    if after == 0 {
        *last() = None;
        tell_island(false);
    }
    Ok(json!({"subscribed": false}))
}

/// 插件被停用/卸载时清掉它的订阅（不依赖它自己记得退订）。
pub fn forget(plugin: &str) {
    let after = {
        let mut table = subscribers();
        table.remove(plugin);
        table.len()
    };
    if after == 0 {
        *last() = None;
        tell_island(false);
    }
}

/// 告诉主岛页面开/关按需上报。页面未创建时静默失败——它起来后会自己问一次。
fn tell_island(on: bool) {
    let Ok(app) = crate::plugin_window::app_handle() else { return };
    use tauri::Emitter as _;
    let _ = app.emit_to("island", "island:watch-dock", json!({"on": on}));
}

/// 主岛页面就绪时按当前订阅数补发一次开关。
///
/// 页面可能在插件订阅之后才建起来（Dock 刚启动、主岛窗口被重建），
/// 那样它从没收到过 `island:watch-dock`，永远不知道要上报。
pub fn sync_island_watch() {
    tell_island(watchers() > 0);
}

/// `dock` 能力校验：主岛几何与 `windows` 分开授权，只要几何的插件不必开窗。
pub fn require_capability(context: &crate::storage_context::Context, plugin: &str) -> Result<(), String> {
    let row = crate::plugin_install::Manager { context }
        .list()?
        .into_iter()
        .find(|row| row.manifest.id == plugin)
        .ok_or_else(|| "插件未安装".to_string())?;
    if row.manifest.capabilities.iter().any(|c| c == "dock") {
        Ok(())
    } else {
        Err("插件未声明 dock 能力".into())
    }
}

/// 页面桥的 `dock.dock.*`。身份与代次已由 `plugin_window::page` 验过，
/// 这里只按方法名分发；能力由本模块再校验一次（页面票据证明不了能力）。
pub fn page(plugin: &str, method: &str, params: &Value) -> Result<Value, String> {
    let context = crate::storage_context::current().ok_or("存储尚未就绪")?;
    require_capability(&context, plugin)?;
    let generation = crate::plugin_window::current_generation(plugin);
    dispatch(plugin, generation, method, params)
}

/// 后端桥的 `host.dock.*`。代次来自拉起这个进程时的那个实例。
pub fn backend(context: &crate::storage_context::Context, plugin: &str, generation: u64, method: &str, params: &Value) -> Result<Value, String> {
    if generation == 0 || crate::plugin_window::current_generation(plugin) != generation {
        return Err("插件实例代次已变化，本次主岛请求已丢弃".into());
    }
    require_capability(context, plugin)?;
    dispatch(plugin, generation, method, params)
}

fn dispatch(plugin: &str, generation: u64, method: &str, params: &Value) -> Result<Value, String> {
    match method {
        "dock.dock.rect" | "host.dock.rect" => rect(),
        "dock.dock.subscribe" | "host.dock.subscribe" => subscribe(plugin, generation),
        "dock.dock.unsubscribe" | "host.dock.unsubscribe" => unsubscribe(plugin),
        "dock.dock.openPage" | "host.dock.openPage" => open_page(plugin, params),
        _ => Err("主岛方法无效".into()),
    }
}

/// 2.2.3（D2）：在本插件的设置面板里打开它的内部页。
///
/// 外部窗口里的「+」按钮要的是**设置面板里的那一页**，不是再开一个外部窗口。
/// 身份已由页面票据/后端会话验过，这里只核「这个页确实是它登记过的」，
/// 然后把设置打开到 `pluginId/pageId`（与 `ExtensionPanel.pages()` 的 key 同形）。
pub fn open_page(plugin: &str, params: &Value) -> Result<Value, String> {
    let page = params.get("page").and_then(Value::as_str).filter(|p| !p.is_empty() && p.len() <= 64)
        .ok_or("openPage 需要一个页面 ID")?;
    let context = crate::storage_context::current().ok_or("存储尚未就绪")?;
    let row = crate::plugin_install::Manager { context }
        .list()?
        .into_iter()
        .find(|row| row.manifest.id == plugin)
        .ok_or_else(|| "插件未安装".to_string())?;    if !row.manifest.pages.iter().any(|p| p.id == page) {
        return Err("插件没有登记这个页面".into());
    }
    // 停用状态的插件不给开页：设置面板里那一页依赖它的运行时，
    // 打开了也只是「插件未运行」。与 windows.open 的判据保持一致。
    if !row.enabled {
        return Err("插件已停用，请先启用再打开它的页面".into());
    }
    let app = crate::plugin_window::app_handle()?.clone();
    let shared = crate::shared(&app);
    // 设置不在此刻可见时 open_settings 会把主岛临时升起来，用户关了也不丢这次请求。
    crate::island::open_settings(&shared, Some(&format!("{plugin}/{page}")));
    Ok(json!({"opened": true, "page": page}))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::island::DockSnapshot;

    fn snapshot_of(window: Option<(i32, i32, i32, i32)>, capsule: Option<(i32, i32, i32, i32)>, view: &'static str) -> DockSnapshot {
        DockSnapshot { window, capsule, view, stowed: false, hidden: false, fullscreen: false, scale: 1.0 }
    }

    // C1：两个矩形必须分开给。窗口铺满工作区、胶囊只有可见宽度，只给一个都不够用。
    #[test]
    fn rect_value_keeps_window_and_capsule_apart() {
        let value = rect_value(&snapshot_of(Some((0, 0, 1920, 132)), Some((810, 8, 300, 36)), "expanded"));
        assert_eq!(value["window"]["width"], 1920);
        assert_eq!(value["capsule"]["width"], 300);
        assert_eq!(value["capsule"]["x"], 810);
        assert_eq!(value["state"]["view"], "expanded");
        assert_eq!(value["state"]["stowed"], false);
    }

    // 胶囊尺寸未知时给 null：2.2.0 起就不再兜底 500×36 的假矩形。
    #[test]
    fn rect_value_reports_a_null_capsule_rather_than_a_fake_one() {
        let value = rect_value(&snapshot_of(Some((0, 0, 1920, 132)), None, "compact"));
        assert!(value["capsule"].is_null());
        assert_eq!(value["window"]["width"], 1920);
    }

    // 状态枚举覆盖 E3 要求的全部取值。
    #[test]
    fn rect_value_carries_the_full_state_enumeration() {
        let mut snapshot = snapshot_of(Some((0, 0, 1920, 132)), Some((810, 8, 300, 36)), "ask");
        snapshot.stowed = true;
        snapshot.hidden = true;
        snapshot.fullscreen = true;
        let value = rect_value(&snapshot);
        for key in ["view", "stowed", "hidden", "fullscreen"] {
            assert!(value["state"].get(key).is_some(), "state 缺 {key}");
        }
        assert_eq!(value["state"]["view"], "ask");
        assert_eq!(value["state"]["stowed"], true);
        assert_eq!(value["state"]["hidden"], true);
        assert_eq!(value["state"]["fullscreen"], true);
    }

    // 没有活会话的插件订阅必须被拒：否则订阅表会留下永远清不掉的条目。
    #[test]
    fn subscribe_requires_a_live_session() {
        let plugin = "acme.dock-no-session";
        {
            let mut table = subscribers();
            table.clear();
        }
        assert!(subscribe(plugin, 1).is_err(), "无会话的插件不得订阅");
        assert_eq!(watchers(), 0, "被拒的订阅不得进表");
    }

    // 撤销实例时订阅必须跟着清（revoke_generation 调 forget）。
    // 否则插件卸载后订阅残留，watchers() 永不回落，主岛页面会一直上报。
    #[test]
    fn forget_removes_the_subscriber_and_releases_the_watch() {
        {
            let mut table = subscribers();
            table.clear();
            // 直接种一个条目：这里只验清理逻辑，不依赖真实的会话登记。
            table.insert("acme.dock-forget".into(), 7);
        }
        assert_eq!(watchers(), 1, "先确认表里确实有一个");
        forget("acme.dock-forget");
        assert_eq!(watchers(), 0, "撤销后必须回落，按需上报才会关掉");
        // 重复撤销是幂等的，不 panic。
        forget("acme.dock-forget");
        assert_eq!(watchers(), 0);
    }

    // 按需启用的核心：没有任何订阅者时 watchers() 必须是 0，
    // 这样主岛页面收到的开关永远是 false，新增代码一行都不执行。
    #[test]
    fn no_subscribers_means_no_work() {
        {
            let mut table = subscribers();
            table.clear();
        }
        assert_eq!(watchers(), 0);
        assert!(live_subscribers().is_empty());
        // 没有订阅者时广播是空操作，不会 panic、也不会触碰任何通道。
        assert_eq!(broadcast(&json!({"event":"dock","type":"pointer","action":"enter"})), 0);
    }

    // 已停用的插件留在表里也不该被推送：live_subscribers 按当前代次过滤。
    #[test]
    fn retired_generations_are_not_pushed_to() {
        {
            let mut table = subscribers();
            table.clear();
            // 代次 999 不可能是任何真实会话，等价于「已停用的旧代次」。
            table.insert("acme.dock-retired".into(), 999);
        }
        assert_eq!(watchers(), 1, "表里有条目");
        assert!(live_subscribers().is_empty(), "但代次已失效，不该被推");
        forget("acme.dock-retired");
    }
}
