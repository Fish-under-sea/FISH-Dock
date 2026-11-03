// 一个会话当前是什么状态：机器可读的那一半。
//
// mood / seen / 停留多久 / 说什么话都是宿主的展示层（state.rs, i18n）；这里的
// Doing 只说「开始了、在跑工具、在等人、在休息、结束、出错」。
use serde_json::{json, Value};

/// 八类 agent，与 server.rs 的 ?agent= 和设置页的表格一致。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Agent {
    Claude,
    Codex,
    Agy,
    Opencode,
    Omp,
    Zcode,
    Reasonix,
    Dsh,
}

impl Agent {
    pub const ALL: [Agent; 8] = [
        Agent::Claude,
        Agent::Codex,
        Agent::Agy,
        Agent::Opencode,
        Agent::Omp,
        Agent::Zcode,
        Agent::Reasonix,
        Agent::Dsh,
    ];

    /// 事件的 `agent` 字段（server.rs 的 agent_of）；空串是 Claude Code。
    pub fn of_id(id: &str) -> Option<Agent> {
        Some(match id {
            "" | "claude" => Agent::Claude,
            "codex" => Agent::Codex,
            "agy" => Agent::Agy,
            "opencode" => Agent::Opencode,
            "omp" => Agent::Omp,
            "zcode" => Agent::Zcode,
            "reasonix" => Agent::Reasonix,
            "dsh" => Agent::Dsh,
            _ => return None,
        })
    }

    /// state.rs 与设置页记的名字。
    pub fn id(self) -> &'static str {
        match self {
            Agent::Claude => "claude",
            Agent::Codex => "codex",
            Agent::Agy => "agy",
            Agent::Opencode => "opencode",
            Agent::Omp => "omp",
            Agent::Zcode => "zcode",
            Agent::Reasonix => "reasonix",
            Agent::Dsh => "dsh",
        }
    }
}

/// 会话是什么状态。带内容的只有 Waiting（等的是什么，宿主给的原话或键）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Doing {
    Start,
    End,
    ToolStart,
    ToolEnd,
    ToolFailed,
    Waiting(String),
    Working,
    Resting,
    Stopped,
    Done,
    Failed,
    TaskDone,
}

impl Doing {
    pub fn id(&self) -> &'static str {
        match self {
            Doing::Start => "start",
            Doing::End => "end",
            Doing::ToolStart => "toolStart",
            Doing::ToolEnd => "toolEnd",
            Doing::ToolFailed => "toolFailed",
            Doing::Waiting(_) => "waiting",
            Doing::Working => "working",
            Doing::Resting => "resting",
            Doing::Stopped => "stopped",
            Doing::Done => "done",
            Doing::Failed => "failed",
            Doing::TaskDone => "taskDone",
        }
    }

    /// 一轮结束了：宿主据此把「做完 / 改好了 / 出错」算作已收尾。
    pub fn is_ending(&self) -> bool {
        matches!(self, Doing::Done | Doing::Failed)
    }

    /// 给 `/v1` 的 JSON 形状：`{ "id": "...", "detail"?: "..." }`。
    pub fn to_json(&self) -> Value {        match self {
            Doing::Waiting(detail) => json!({ "id": "waiting", "detail": detail }),
            other => json!({ "id": other.id() }),
        }
    }
}

/// T-I：这一轮/这个会话是不是 agent 自己发起的（桌面版的内部请求、内部轮、
/// 子代理的会话），而不是人。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Origin {
    User,
    Internal,
}

/// 一条中性事件。字段是共同形状；各 agent 的适配器只填它知道的部分。
/// `turn` 是 agent 自己的轮次 id（Codex 是 UUID，Antigravity 是它自己的编号），
/// 原样保存——不压成数字，也不丢（方案 §2.2 的「不丢 tool/turn 关联」）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AgentEvent {
    pub agent: Agent,
    pub session: String,
    pub subagent: bool,
    pub cwd: String,
    pub turn: String,
    pub name: String,
    pub last: String,
    pub doing: Doing,
    pub origin: Origin,
    pub at: u64,
}

/// 一次转换的结果：一条事件，或「这个事件不改变任何会话」（Ignore）。
///
/// 控制类事件（Codex 的 compact 开始、PermissionRequest 是 Codex 自己批的、
/// 子代理的收尾）本来就是「什么都不做」，不当作错误；形状坏了才报 EventError。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome {
    Event(AgentEvent),
    Ignore,
}

/// 转换失败：带上是哪一类 agent、为什么——宿主需要上报的失败态，不用 None 糊掉。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EventError {
    pub agent: Agent,
    pub why: String,
}

impl EventError {
    pub fn new(agent: Agent, why: impl Into<String>) -> Self {
        EventError { agent, why: why.into() }
    }
}

fn s<'a>(value: &'a Value, key: &str) -> &'a str {
    value.get(key).and_then(Value::as_str).unwrap_or("")
}

// 一句能放进 Waiting 的话：detail 是字符串就用它，是 { key, vars } 就用键。
fn detail_text(detail: &Value) -> String {
    match detail {
        Value::String(text) => text.clone(),
        Value::Object(fields) => fields.get("key").and_then(Value::as_str).unwrap_or("").to_string(),
        _ => String::new(),
    }
}

impl AgentEvent {
    /// 从宿主适配器已经做出来的消息里取出中性事件（八类 agent 共用同一形状）。
    ///
    /// 适配器自己的说法（mood、say、title、todo、文件……）不动，state.rs 照旧消费；
    /// 这里只是把「会话发生了什么」摊平成 /v1 事件与 T-I 用的字段。
    /// 没有任何状态变化的消息（只有 react 的、没有 mood 的）→ None。
    pub fn of_message(agent: Agent, msg: &Value, at: u64) -> Option<AgentEvent> {
        if !msg.is_object() {
            return None;
        }
        let event = s(msg, "event");
        let mood = s(msg, "mood");
        let react = s(msg, "react");
        let doing = if mood.is_empty() {
            // 只有一次反应：任务做完了的那一次（state.rs 的 react/say）。
            match react {
                "jump" => Doing::TaskDone,
                _ => return None,
            }
        } else {
            match event {
                "turn-start" => Doing::Start,
                "tool-start" => Doing::ToolStart,
                "tool-done" => Doing::ToolEnd,
                "tool-failed" => Doing::ToolFailed,
                "session-end" => Doing::End,
                // 一轮结束：做完、出错、还是被人停下（state.rs 的 turn-end）。
                "turn-end" => match mood {
                    "done" | "review" => Doing::Done,
                    "error" => Doing::Failed,
                    _ => Doing::Stopped,
                },
                _ => match mood {
                    "working" => Doing::Working,
                    "waiting" => Doing::Waiting(detail_text(msg.get("detail").unwrap_or(&Value::Null))),
                    "idle" => Doing::Resting,
                    "done" | "review" => Doing::Done,
                    "error" => Doing::Failed,
                    _ => return None,
                },
            }
        };
        let name = {
            let title = s(msg, "title");
            if title.is_empty() {
                s(msg, "task").to_string()
            } else {
                title.to_string()
            }
        };
        Some(AgentEvent {
            agent,
            session: s(msg, "session").to_string(),
            // DSH 的子代理工具算在父会话头上（events_dsh.rs whose）：不是它自己的会话。
            subagent: msg.get("within").and_then(Value::as_bool).unwrap_or(false),
            cwd: s(msg, "workspace").to_string(),
            turn: s(msg, "turn").to_string(),
            name,
            last: s(msg, "reply").to_string(),
            doing,
            // 适配器过滤掉内部轮之后才交出来；到这里就是人的（T-I 的状态在适配器）。
            origin: Origin::User,
            at,
        })
    }

    /// 隐私开关关掉时交给 `/v1/events` 的事件：不带人的话、会话名与本机路径。
    /// 会话、状态、agent、轮次还在，客户端仍能据此刷新快照。
    pub fn redacted(&self) -> AgentEvent {
        let mut event = self.clone();
        event.name.clear();
        event.last.clear();
        event.cwd.clear();
        event
    }

    /// `/v1/events` 里一帧的 data。
    pub fn to_json(&self) -> Value {
        json!({
            "agent": self.agent.id(),
            "session": self.session,
            "subagent": self.subagent,
            "cwd": self.cwd,
            "turn": self.turn,
            "name": self.name,
            "last": self.last,
            "doing": self.doing.to_json(),
            "origin": match self.origin { Origin::User => "user", Origin::Internal => "internal" },
            "at": self.at,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn msg(more: Value) -> Value {
        let mut base = json!({ "session": "s1", "agent": "codex" });
        for (k, v) in more.as_object().unwrap() {
            base[k] = v.clone();
        }
        base
    }

    #[test]
    fn every_message_becomes_one_neutral_event() {
        let cases = [
            (json!({ "mood": "working", "event": "turn-start" }), Doing::Start),
            (json!({ "mood": "working", "event": "tool-start", "detail": "Bash" }), Doing::ToolStart),
            (json!({ "mood": "working", "event": "tool-done", "detail": "Edit" }), Doing::ToolEnd),
            (json!({ "mood": "working", "event": "tool-failed", "detail": "Bash" }), Doing::ToolFailed),
            (json!({ "mood": "done", "event": "turn-end" }), Doing::Done),
            (json!({ "mood": "idle", "event": "turn-end" }), Doing::Stopped),
            (json!({ "mood": "idle", "event": "session-end" }), Doing::End),
            (json!({ "mood": "working" }), Doing::Working),
            (json!({ "mood": "idle" }), Doing::Resting),
            (json!({ "mood": "done" }), Doing::Done),
            (json!({ "mood": "review" }), Doing::Done),
            (json!({ "mood": "error" }), Doing::Failed),
        ];
        for (more, want) in cases {
            let event = AgentEvent::of_message(Agent::Codex, &msg(more.clone()), 7).expect("an event");
            assert_eq!(event.doing, want, "{more}");
            assert_eq!(event.at, 7);
            assert_eq!(event.origin, Origin::User);
        }
    }

    #[test]
    fn waiting_keeps_what_it_waits_for_and_a_reaction_alone_is_a_task_done() {
        let asked = msg(json!({ "mood": "waiting", "detail": { "key": "detail.approve" } }));
        assert_eq!(AgentEvent::of_message(Agent::Claude, &asked, 1).unwrap().doing, Doing::Waiting("detail.approve".into()));
        let said = msg(json!({ "react": "jump", "say": { "key": "say.taskDone" } }));
        assert_eq!(AgentEvent::of_message(Agent::Claude, &said, 2).unwrap().doing, Doing::TaskDone);
        // 一次失败的反应带着 mood，就还是它那个工具失败。
        let failed = msg(json!({ "mood": "working", "event": "tool-failed", "react": "failed" }));
        assert_eq!(AgentEvent::of_message(Agent::Claude, &failed, 3).unwrap().doing, Doing::ToolFailed);
    }

    #[test]
    fn a_message_that_changes_nothing_is_no_event() {
        for nothing in [json!({}), json!({ "react": "failed" }), json!({ "mood": "sleepy" }), json!("nope"), json!([1])] {
            assert!(AgentEvent::of_message(Agent::Codex, &nothing, 0).is_none(), "{nothing}");
        }
        assert_eq!(Doing::Done.is_ending(), true);
        assert_eq!(Doing::Stopped.is_ending(), false);
    }

    #[test]
    fn the_shape_carries_turn_subagent_place_and_words() {
        let m = msg(json!({
            "mood": "working", "event": "tool-start", "detail": "Edit", "turn": "t-42",
            "workspace": "D:\\w\\pet", "title": "岛的设置", "reply": "改好了。",
            "todo": { "set": [] }, "within": true,
        }));
        let event = AgentEvent::of_message(Agent::Dsh, &m, 11).unwrap();
        assert_eq!(
            (event.agent, event.session.as_str(), event.turn.as_str(), event.subagent),
            (Agent::Dsh, "s1", "t-42", true)
        );
        assert_eq!((event.cwd.as_str(), event.name.as_str(), event.last.as_str()), ("D:\\w\\pet", "岛的设置", "改好了。"));
        // 没有标题时用人的话当名字。
        let task = msg(json!({ "mood": "working", "event": "turn-start", "task": "修复岛" }));
        assert_eq!(AgentEvent::of_message(Agent::Claude, &task, 1).unwrap().name, "修复岛");
    }

    #[test]
    fn each_agent_has_its_id_and_the_table_is_whole() {
        for agent in Agent::ALL {
            assert_eq!(Agent::of_id(agent.id()), Some(agent), "{agent:?}");
        }
        assert_eq!(Agent::of_id(""), Some(Agent::Claude));
        assert_eq!(Agent::of_id("codexx"), None);
        let mut ids: Vec<&str> = Agent::ALL.iter().map(|a| a.id()).collect();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), 8);
    }

    #[test]
    fn doing_ids_are_stable() {
        let all = [
            Doing::Start, Doing::End, Doing::ToolStart, Doing::ToolEnd, Doing::ToolFailed,
            Doing::Waiting("x".into()), Doing::Working, Doing::Resting, Doing::Stopped,
            Doing::Done, Doing::Failed, Doing::TaskDone,
        ];
        let ids: Vec<&str> = all.iter().map(|d| d.id()).collect();
        assert_eq!(ids, ["start", "end", "toolStart", "toolEnd", "toolFailed", "waiting", "working", "resting", "stopped", "done", "failed", "taskDone"]);
    }

    #[test]
    fn a_redacted_event_keeps_the_state_but_not_the_words_or_paths() {
        let m = msg(json!({ "mood": "working", "event": "turn-start", "task": "修复岛", "workspace": "D:\\w\\pet", "reply": "改好了。" }));
        let event = AgentEvent::of_message(Agent::Codex, &m, 3).unwrap();
        let hidden = event.redacted();
        assert_eq!((hidden.name.as_str(), hidden.last.as_str(), hidden.cwd.as_str()), ("", "", ""));
        assert_eq!((hidden.agent, hidden.session.as_str(), hidden.doing.id(), hidden.at), (Agent::Codex, "s1", "start", 3));
        // 原事件不被改。
        assert_eq!(event.name, "修复岛");
    }
}