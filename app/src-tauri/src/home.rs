// DEC-2 / DEC-3 / DEC-4：统一的「家」与「显示器」抽象。
//
// 本仓此前「家」只有两种：island（灵动岛）与 taskbar（她的任务栏），由 main.rs 的
// `home()` 从设置 `display` 读字符串判断；显示器只在多处各自 `screen::read`。DEC-2 要加
// 第三种「顶栏 + dock」，DEC-3 要能把岛嵌入 Windows 任务栏，DEC-4 要让「家」放任意显示器。
// 为避免三套各自接管任务栏、各自处理显示变更（契约与 captain 都明确禁止），这里抽出：
//
//   - `Home`：家的三种形态（Island / Taskbar / Topbar）的唯一枚举与解析/切换/标签；
//   - `HomeChoice`：家 + 选定显示器（保存的 mon），是持久化在设置里的一份（display + homeMonitor）；
//   - `resolve`：给定保存的选择与「当前有哪些显示器」，解出实际该用哪个显示器，保存屏不在
//     回退主屏、无主屏回退第一个（DEC-4 的核心，纯函数、可测）。
//
// 显示变更事件本身仍在 main.rs 的 ticker 里（每 2 秒 `screen::read` 比较），这里只把「这份
// 变化怎么落实到家」收成一处 `on_displays_changed` 的纯决策：该重启哪条条带、岛/顶栏是否
// 需要重定位，返回一个动作集合，由 main.rs 统一执行——不再让 island/taskbar/topbar 各自听。
//
// 只依赖 std / serde_json，便于单测。

use serde_json::{json, Value};

/// 家的形态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Home {
    /// 灵动岛（屏幕顶部胶囊）。
    Island,
    /// 她的任务栏（接管底部/侧边 Windows 任务栏）。
    Taskbar,
    /// 顶栏 + dock（DEC-2：屏幕顶一条细顶栏 + 底部正中 dock）。
    Topbar,
}

impl Home {
    /// 从设置 `display` 的字符串解析；未知/空给 Island（与旧行为一致：非 "taskbar" 一律岛）。
    pub fn parse(s: Option<&str>) -> Home {
        match s {
            Some("taskbar") => Home::Taskbar,
            Some("topbar") => Home::Topbar,
            _ => Home::Island,
        }
    }

    /// 写回设置 `display` 的字符串。
    pub fn as_setting(self) -> &'static str {
        match self {
            Home::Island => "island",
            Home::Taskbar => "taskbar",
            Home::Topbar => "topbar",
        }
    }

    /// 三种家按这个顺序切换（外观页的「显示方式」三个选项）。
    pub fn next(self) -> Home {
        match self {
            Home::Island => Home::Taskbar,
            Home::Taskbar => Home::Topbar,
            Home::Topbar => Home::Island,
        }
    }

    pub fn all() -> [Home; 3] {
        [Home::Island, Home::Taskbar, Home::Topbar]
    }

    /// 是否占住 Windows 任务栏的位置（需要 shell 的接管/还回）。三种家里只有 Taskbar 与
    /// 嵌入模式会；Topbar 不接管任务栏（它只占屏幕顶，Windows 任务栏照旧或由她放走）。
    pub fn takes_taskbar(self) -> bool {
        matches!(self, Home::Taskbar)
    }
}

/// 一份「家」的选择：家形态 + 选定显示器（Windows 的 mon 矩形 (x,y,w,h)），持久化。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HomeChoice {
    pub home: Home,
    /// 选定显示器的 mon 矩形；None = 跟随主屏。
    pub monitor: Option<(i32, i32, i32, i32)>,
}

impl HomeChoice {
    pub fn from_settings(display: Option<&str>, monitor: Option<(i32, i32, i32, i32)>) -> HomeChoice {
        HomeChoice { home: Home::parse(display), monitor }
    }

    /// 序列化进设置的两项（display 与 homeMonitor）。
    pub fn to_settings(self) -> Value {
        json!({
            "display": self.home.as_setting(),
            "homeMonitor": self.monitor.map(|(x, y, w, h)| json!([x, y, w, h])).unwrap_or(Value::Null),
        })
    }
}

/// 从设置里读 homeMonitor（[x,y,w,h] 或空）。
pub fn parse_monitor(v: &Value) -> Option<(i32, i32, i32, i32)> {
    let a = v.as_array()?;
    if a.len() != 4 {
        return None;
    }
    let n = |i: usize| a[i].as_i64().map(|v| v as i32);
    Some((n(0)?, n(1)?, n(2)?, n(3)?))
}

/// DEC-4 核心：给定保存的选择和当前显示器的 mon 列表 + 主屏 mon，解出实际用的那个 mon。
///   1) 保存的 mon 仍在 → 用它（任意显示器都能当「家」）；
///   2) 保存的不在（拔掉/换了）→ 回退主屏；
///   3) 没有主屏（罕见）→ 回退第一个；
///   4) 一个都没有 → None。
pub fn resolve(choice: &HomeChoice, monitors: &[(i32, i32, i32, i32)], primary: Option<(i32, i32, i32, i32)>) -> Option<(i32, i32, i32, i32)> {
    if let Some(saved) = choice.monitor {
        if monitors.contains(&saved) {
            return Some(saved);
        }
    }
    if let Some(p) = primary {
        if monitors.is_empty() || monitors.contains(&p) {
            return Some(p);
        }
    }
    monitors.first().copied()
}

/// 显示变更时要落到「家」上的动作（main.rs 据此统一执行，不让各家自己听）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DisplayActions {
    /// 家所在的显示器（解出的 mon）；None = 无显示器（不动作）。
    pub home_monitor: Option<(i32, i32, i32, i32)>,
    /// 家的显示器是否变了（需要重定位岛/顶栏/dock）。
    pub home_moved: bool,
    /// 是否要重建副屏条带（bars.rs）。
    pub refresh_bars: bool,
    /// 是否要把主岛/浮窗拉回屏内。
    pub keep_on_screen: bool,
}

/// 由（变化的）显示器列表与保存的选择，算出要做什么。纯函数。
pub fn on_displays_changed(before: &[(i32, i32, i32, i32)], after: &[(i32, i32, i32, i32)], primary: Option<(i32, i32, i32, i32)>, choice: &HomeChoice) -> DisplayActions {
    let home_monitor = resolve(choice, after, primary);
    // 「家」的显示器是否变了：解出的 mon 与上次解出的不同（用 before 也解一次比较）。
    let prev = resolve(choice, before, primary);
    let home_moved = home_monitor != prev && home_monitor.is_some();
    // 条带：显示器集合变了就要副屏条带重算（bars.rs 听这一个信号）。
    let refresh_bars = before != after;
    DisplayActions { home_monitor, home_moved, refresh_bars, keep_on_screen: true }
}

#[cfg(test)]
mod tests {
    use super::*;

    const A: (i32, i32, i32, i32) = (0, 0, 1920, 1080);
    const B: (i32, i32, i32, i32) = (-2560, 0, 2560, 1440);
    const C: (i32, i32, i32, i32) = (1920, 0, 2560, 1440);

    #[test]
    fn parse_and_cycle_round_trip() {
        assert_eq!(Home::parse(None), Home::Island);
        assert_eq!(Home::parse(Some("taskbar")), Home::Taskbar);
        assert_eq!(Home::parse(Some("topbar")), Home::Topbar);
        assert_eq!(Home::parse(Some("weird")), Home::Island, "未知回岛，与旧行为一致");
        for h in Home::all() {
            assert_eq!(Home::parse(Some(h.as_setting())), h, "写回再读一致");
            assert_eq!(h.next().next().next(), h, "三态循环一圈回到自己");
        }
        assert!(Home::Taskbar.takes_taskbar());
        assert!(!Home::Topbar.takes_taskbar(), "顶栏不接管任务栏");
        assert!(!Home::Island.takes_taskbar());
    }

    #[test]
    fn dec4_any_display_kept_or_fallback_primary() {
        let monitors = [A, B, C];
        // 保存的屏仍在：用它（任意显示器都能当「家」）。
        let on_b = HomeChoice { home: Home::Topbar, monitor: Some(B) };
        assert_eq!(resolve(&on_b, &monitors, Some(A)), Some(B));
        // 保存的屏拔掉：回退主屏。
        let gone = HomeChoice { home: Home::Island, monitor: Some((9, 9, 9, 9)) };
        assert_eq!(resolve(&gone, &monitors, Some(A)), Some(A), "拔掉回退主屏");
        // 跟随主屏（monitor=None）。
        let follow = HomeChoice { home: Home::Island, monitor: None };
        assert_eq!(resolve(&follow, &monitors, Some(C)), Some(C));
        // 没有主屏也给一个（第一个）。
        assert_eq!(resolve(&gone, &monitors, None), Some(A));
        // 一个显示器都没有：None。
        assert_eq!(resolve(&gone, &[], None), None);
    }

    #[test]
    fn displays_changed_moves_home_and_refreshes_bars() {
        let choice = HomeChoice { home: Home::Topbar, monitor: Some(B) };
        // B 在 → A 才在：家的显示器变了。
        let acts = on_displays_changed(&[A, B], &[A], Some(A), &choice);
        assert_eq!(acts.home_monitor, Some(A), "B 没了回退主屏 A");
        assert!(acts.home_moved, "显示器集合变了，家要重定位");
        assert!(acts.refresh_bars);
        // 没变化：家不动（同一条显示器列表）。
        let same = on_displays_changed(&[A], &[A], Some(A), &choice);
        assert!(!same.home_moved);
        assert!(!same.refresh_bars);
    }

    #[test]
    fn settings_round_trip() {
        let c = HomeChoice { home: Home::Taskbar, monitor: Some(B) };
        let v = c.to_settings();
        assert_eq!(v["display"], "taskbar");
        assert_eq!(parse_monitor(&v["homeMonitor"]), Some(B));
        let back = HomeChoice::from_settings(v["display"].as_str(), parse_monitor(&v["homeMonitor"]));
        assert_eq!(back, c);
        // 跟随主屏：homeMonitor 是 null。
        let follow = HomeChoice { home: Home::Island, monitor: None };
        assert_eq!(follow.to_settings()["homeMonitor"], Value::Null);
        assert_eq!(parse_monitor(&Value::Null), None);
    }
}
