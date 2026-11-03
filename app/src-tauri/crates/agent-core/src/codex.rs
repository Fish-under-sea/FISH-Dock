// T-I：Codex 桌面版的**内部请求**不是人的会话。
//
// 判据来自 Codex 写进会话记录（hook 的 transcript_path，rollout）里的东西，不猜
// 提示词的空与不空：
//
//   - 会话头 session_meta 的 thread_source：人的会话是 "user"；Desktop 自己发起的
//     线程（子代理、内部请求）不是（实测本机 13 份记录：user 11、subagent 2）。
//   - 每一轮的人的那条 response_item（role=user）带着 internal_chat_message_
//     metadata_passthrough：provenance.type 是 "user" 才是人说的话；"harness" /
//     "tool" 是 Codex 自己塞进这一轮的内容（environments.environment_context、
//     generic.turn_aborted、multi_agent.subagent_notification 都是这一类）。
//
// 人只发图片、没有文字的那一轮仍然是人的：这里从不按提示词是否为空判断。
//
// 记录还没写出来（或读不到）时不猜：Origin 默认 User，绝不因为读不到就吞掉一个
// 会话。过滤状态按 session / turn 存在适配器（events_codex.rs）里。
use serde_json::Value;

use crate::event::Origin;

/// Codex 自己写进一轮里的内容，不是人的话；只在没有 provenance 时兜底。
pub const INTERNAL_KINDS: [&str; 3] = [
    "environments.environment_context",
    "generic.turn_aborted",
    "multi_agent.subagent_notification",
];

fn s<'a>(value: &'a Value, key: &str) -> &'a str {
    value.get(key).and_then(Value::as_str).unwrap_or("")
}

/// 会话头（session_meta）说的：这个线程是谁的。
/// 没有这个字段、或值是 "user"，就是人的；别的线程（subagent、app 自己的）不是。
pub fn session_origin(meta: &Value) -> Origin {
    let payload = if meta.get("payload").is_some() { &meta["payload"] } else { meta };
    match s(payload, "thread_source") {
        "" | "user" => Origin::User,
        _ => Origin::Internal,
    }
}

// 这一轮属于谁：一条 response_item 的记录，人说过话就是人的。
fn entry_origin(entry: &Value, turn: &str) -> Option<Origin> {
    let payload = entry.get("payload")?;
    if s(payload, "role") != "user" {
        return None;
    }
    let meta = payload.get("internal_chat_message_metadata_passthrough");
    let (record_turn, provenance) = match meta {
        Some(meta) => (s(meta, "turn_id"), meta.get("content_item_metadata")),
        // 老版本没有这段旁注：只按内容种类兜底。
        None => ("", None),
    };
    if !turn.is_empty() && !record_turn.is_empty() && record_turn != turn {
        return None;
    }
    let first_party = provenance.is_some_and(|items| {
        items
            .as_array()
            .is_some_and(|items| items.iter().any(|item| s(item.get("provenance").unwrap_or(&Value::Null), "type") == "user"))
    });
    if first_party {
        return Some(Origin::User);
    }
    if provenance.is_some() {
        // 这一条不是人的：Codex 自己写的（harness / tool）。
        return Some(Origin::Internal);
    }
    let internal = meta.is_some_and(|meta| {
        meta.get("content_item_kinds")
            .and_then(Value::as_array)
            .is_some_and(|kinds| kinds.iter().any(|kind| kind.as_str().is_some_and(|kind| INTERNAL_KINDS.contains(&kind))))
    });
    Some(if internal { Origin::Internal } else { Origin::User })
}

/// 这一轮是谁的：在记录的末尾里找这一轮人的那条 message。
///
/// 同一轮里既有人的话、又有 Codex 自己补的内容（环境、被中止、子代理通知）时，
/// 这一轮是人的。找不到这一轮（记录还没写出来）→ None，由调用方按 User 处理。
/// `turn` 为空时看最后一条 user 记录。
pub fn turn_origin<'a>(lines: impl IntoIterator<Item = &'a str>, turn: &str) -> Option<Origin> {
    let mut last = None;
    let mut talked = false;
    let mut written = false;
    for line in lines {
        let Ok(entry) = serde_json::from_str::<Value>(line) else { continue };
        if s(&entry, "type") != "response_item" {
            continue;
        }
        let Some(origin) = entry_origin(&entry, turn) else { continue };
        talked |= origin == Origin::User;
        written |= origin == Origin::Internal;
        last = Some(origin);
    }
    if turn.is_empty() {
        last
    } else if talked {
        Some(Origin::User)
    } else if written {
        Some(Origin::Internal)
    } else {
        None
    }
}

/// 分类结果，调用方不必再区分「读不到」与「是人的」。
pub fn turn_is_internal<'a>(lines: impl IntoIterator<Item = &'a str>, turn: &str) -> bool {
    turn_origin(lines, turn) == Some(Origin::Internal)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn meta(thread_source: &str) -> Value {
        json!({ "type": "session_meta", "payload": { "session_id": "s", "thread_source": thread_source, "originator": "Codex Desktop", "source": "vscode" } })
    }

    fn user_message(turn: &str, provenance: &str, kind: Option<&str>) -> String {
        let mut meta = json!({
            "turn_id": turn,
            "content_item_metadata": [ { "provenance": { "type": provenance } } ],
        });
        if let Some(kind) = kind {
            meta["content_item_kinds"] = json!([kind]);
        }
        json!({ "type": "response_item", "payload": { "type": "message", "role": "user", "content": [{ "type": "input_text", "text": "…" }],
            "internal_chat_message_metadata_passthrough": meta } })
        .to_string()
    }

    #[test]
    fn a_thread_the_person_started_is_theirs() {
        assert_eq!(session_origin(&meta("user")), Origin::User);
        // 老记录没有 thread_source：不猜，算人的。
        assert_eq!(session_origin(&json!({ "type": "session_meta", "payload": {} })), Origin::User);
        assert_eq!(session_origin(&json!({ "thread_source": "user" })), Origin::User);
    }

    #[test]
    fn the_desktop_s_own_threads_are_not_the_person_s() {
        for source in ["subagent", "app_generated", "review"] {
            assert_eq!(session_origin(&meta(source)), Origin::Internal, "{source}");
        }
    }

    // 纯内部轮：这一轮的人那条 message 是 Codex 自己写的（环境、被中止、子代理通知）。
    #[test]
    fn a_turn_only_the_desktop_wrote_is_internal() {
        let lines = [
            meta("user").to_string(),
            user_message("t1", "user", Some("user_input")),
            user_message("t2", "harness", Some("environments.environment_context")),
        ];
        let tail: Vec<&str> = lines.iter().map(String::as_str).collect();
        assert_eq!(turn_origin(tail.iter().copied(), "t2"), Some(Origin::Internal));
        assert!(turn_is_internal(tail.iter().copied(), "t2"));
        assert_eq!(turn_origin(tail.iter().copied(), "t1"), Some(Origin::User));
        // 记录还没写到这一轮：不知道就是不知道，不吞。
        assert_eq!(turn_origin(tail.iter().copied(), "t9"), None);
        assert!(!turn_is_internal(tail.iter().copied(), "t9"));
        // 空轮次：看最后一条人的记录。
        assert_eq!(turn_origin(tail.iter().copied(), ""), Some(Origin::Internal));
    }

    // 混合会话：人的一轮之后，Desktop 自己又来了一轮；前一轮不受影响。
    #[test]
    fn a_mixed_session_keeps_the_person_s_turn() {
        let lines = [
            user_message("t1", "user", Some("user_input")),
            user_message("t1", "harness", Some("environments.environment_context")),
            user_message("t2", "user", Some("user_input")),
        ];
        let tail: Vec<&str> = lines.iter().map(String::as_str).collect();
        // 同一轮里既有人的话又有环境：这一轮是人的。
        assert_eq!(turn_origin(tail.iter().copied(), "t1"), Some(Origin::User));
        assert_eq!(turn_origin(tail.iter().copied(), "t2"), Some(Origin::User));
    }

    // 只有图片、没有文字的一轮：记录里 role=user、provenance 是 user，仍然是人的。
    #[test]
    fn a_turn_of_images_alone_keeps_its_name() {
        let line = json!({ "type": "response_item", "payload": { "type": "message", "role": "user",
            "content": [{ "type": "input_image" }],
            "internal_chat_message_metadata_passthrough": { "turn_id": "t1", "content_item_metadata": [{ "provenance": { "type": "user" } }] } } })
        .to_string();
        assert_eq!(turn_origin([line.as_str()], "t1"), Some(Origin::User));
        // 纯文本、没有旁注的老记录：按内容种类判断，不在内部清单里就是人的。
        let old = json!({ "type": "response_item", "payload": { "type": "message", "role": "user", "content": [] } }).to_string();
        assert_eq!(turn_origin([old.as_str()], "t1"), Some(Origin::User));
    }

    #[test]
    fn lines_that_are_not_records_are_skipped() {
        let lines = ["", "not json", r#"{"type":"response_item"}"#, r#"{"type":"turn_context","payload":{"turn_id":"t1"}}"#];
        assert_eq!(turn_origin(lines, "t1"), None);
        assert_eq!(turn_origin(Vec::<&str>::new(), "t1"), None);
    }
}
