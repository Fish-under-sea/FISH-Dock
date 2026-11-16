// DEC-5（上游 v0.4.2 的等价实现，安全默认）：日历与「正在播放」的**纯逻辑核心**。
//
// 宿主内建日历与正在播放两项能力，不要求插件仓另做。日历不联网、不写入外部日历账户；
// 正在播放只读系统媒体会话、无媒体时隐藏而非报错。本模块只放可测的纯部分：
//   - 日历：从本地日程数据取「今天的日程」，按时区与「全天事件」排序；不联网。
//   - 正在播放：媒体状态的展示模型（有/无、标题、艺人、播放/暂停），无媒体时给「隐藏」。
// 平台绑定（读 Windows 通知区的日历/媒体的系统 API）与页面渲染由后续接；这里先钉住语义，
// 避免双实现与网络/权限面。

use serde_json::{json, Value};

/// 一条日程（本地）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Event {
    /// 开始（自 1970 起的毫秒，本地墙钟已折算成 UTC 毫秒）。
    pub start_ms: i64,
    /// 结束（毫秒）。
    pub end_ms: i64,
    /// 标题。
    pub title: String,
    /// 全天事件（无具体时间）。
    pub all_day: bool,
}

/// 今天的日程：给定「现在」（毫秒）、本地日界（当天 00:00 与次日 00:00 的 UTC 毫秒），
/// 只留与这一天相交的，全天事件排在最前，其余按开始时间升序。空则空数组。
pub fn agenda(events: &[Event], day_start_ms: i64, day_end_ms: i64) -> Vec<Event> {
    let mut today: Vec<Event> = events.iter().filter(|e| e.end_ms > day_start_ms && e.start_ms < day_end_ms).cloned().collect();
    today.sort_by(|a, b| {
        // 全天在前；再按开始时间；再按标题。
        b.all_day.cmp(&a.all_day).then(a.start_ms.cmp(&b.start_ms)).then(a.title.cmp(&b.title))
    });
    today
}

/// 到下一场日程的毫秒数（用于「还有 N 分钟」）。已经在进行中的算 0；没有则 None。
pub fn until_next(events: &[Event], now_ms: i64) -> Option<i64> {
    events.iter().filter(|e| !e.all_day && e.start_ms > now_ms).map(|e| e.start_ms - now_ms).min()
}

/// 正在播放的展示模型。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NowPlaying {
    pub playing: bool,
    pub title: String,
    pub artist: String,
}

/// 有没有可展示的媒体：没有（title 与 artist 都空）就隐藏，不报错。
pub fn now_playing_view(np: Option<&NowPlaying>) -> Value {
    match np {
        Some(n) if !n.title.is_empty() || !n.artist.is_empty() => json!({
            "visible": true,
            "playing": n.playing,
            "title": n.title,
            "artist": n.artist,
        }),
        // 无媒体：隐藏（而非 null/错误）。
        _ => json!({ "visible": false }),
    }
}

/// 日历视图（给页面）：今天的日程 + 到下一场的分钟。
pub fn calendar_view(events: &[Event], now_ms: i64, day_start_ms: i64, day_end_ms: i64) -> Value {
    let today = agenda(events, day_start_ms, day_end_ms);
    let next = until_next(&today, now_ms);
    json!({
        "today": today.iter().map(|e| json!({
            "title": e.title,
            "start": e.start_ms,
            "end": e.end_ms,
            "allDay": e.all_day,
        })).collect::<Vec<_>>(),
        "nextInMs": next,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ev(start: i64, end: i64, title: &str, all_day: bool) -> Event {
        Event { start_ms: start, end_ms: end, title: title.to_string(), all_day }
    }

    #[test]
    fn today_only_and_ordered() {
        let day = (1_000, 2_000);
        let events = vec![
            ev(1_500, 1_600, "b", false),
            ev(1_100, 1_200, "a", false),
            ev(900, 1_050, "prev-day-but-overlaps-morning", false),
            ev(2_000, 2_100, "tomorrow", false),
            ev(1_000, 2_000, "all-day holiday", true),
        ];
        let a = agenda(&events, day.0, day.1);
        // 全天在最前；其余相交的按开始升序；明天的排除。
        assert_eq!(a.first().unwrap().title, "all-day holiday");
        let titles: Vec<_> = a.iter().map(|e| e.title.as_str()).collect();
        assert!(titles.contains(&"prev-day-but-overlaps-morning"), "跨日但今天有交集要保留");
        assert!(!titles.contains(&"tomorrow"));
        // 非全天部分升序
        let times: Vec<_> = a.iter().filter(|e| !e.all_day).map(|e| e.start_ms).collect();
        let mut sorted = times.clone();
        sorted.sort();
        assert_eq!(times, sorted);
    }

    #[test]
    fn until_next_only_future_timed_events() {
        let events = vec![ev(1_000, 1_100, "past", false), ev(5_000, 5_100, "next", false), ev(9_000, 9_100, "later", false)];
        assert_eq!(until_next(&events, 2_000), Some(3_000));
        assert_eq!(until_next(&events, 10_000), None);
    }

    #[test]
    fn no_media_hides_not_errors() {
        assert_eq!(now_playing_view(None), json!({ "visible": false }));
        let empty = NowPlaying { playing: false, title: String::new(), artist: String::new() };
        assert_eq!(now_playing_view(Some(&empty))["visible"], false);
        let playing = NowPlaying { playing: true, title: "Song".into(), artist: "Artist".into() };
        let v = now_playing_view(Some(&playing));
        assert_eq!(v["visible"], true);
        assert_eq!(v["title"], "Song");
        assert_eq!(v["playing"], true);
    }

    #[test]
    fn calendar_view_carries_next_in() {
        let (s, e) = (0, 100_000);
        let events = vec![ev(50_000, 50_001, "meeting", false)];
        let v = calendar_view(&events, 20_000, s, e);
        assert_eq!(v["nextInMs"], 30_000);
        assert_eq!(v["today"].as_array().unwrap().len(), 1);
    }
}
