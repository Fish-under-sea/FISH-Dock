// ZCode hook events → pet messages: Claude Code's (events.rs), plus agent:
// "zcode". ZCode (Z.ai's agent, its desktop app and CLI) runs its hooks as
// this program with connection::ZCODE_FLAG and the event's name, which hands
// each to /hook?agent=zcode (main.rs zcode_hook). Each comes with Claude
// Code's fields (session_id, hook_event_name, tool_name, tool_input,
// tool_use_id, prompt, last_assistant_message, permission_mode) beside its
// own camelCase ones (sessionId, turnId, toolName, responseText...), and its
// tools by Claude Code's names (Bash, Read, Write, Edit, Agent or Task), so
// events.rs reads them as they are. What differs, as the desktop app 3.15.3
// sends them (2026-10-15): 7 events, no SessionEnd or StopFailure; a
// failed tool's PostToolUseFailure carries error as its words; a call the
// person refused in ZCode's own dialog gets no PostToolUse(Failure), and its
// turn goes on to Stop; transcript_path is a file of its own made for each
// hook (and gone after it) with no session name in it; no permission
// suggestions, so her prompt for it is allow or deny (asks.rs).
use serde_json::{json, Value};

pub fn to_message(e: &Value) -> Option<Value> {
    let mut message = crate::events::to_message(e)?;
    message["agent"] = json!("zcode");
    Some(message)
}

#[cfg(test)]
mod tests {
    use super::*;

    // As the desktop app 3.15.3 sends them (trimmed), in a folder that is no
    // git repository.
    fn event(name: &str, more: Value) -> Value {
        let mut e = json!({
            "cwd": "Z:\\nowhere\\zc", "hookEventName": name, "mode": "build", "sessionId": "sess_fc09",
            "timestamp": "2026-10-15T20:33:22.513Z", "turnId": "turn_22f3",
            "hook_event_name": name, "permission_mode": "build", "session_id": "sess_fc09",
            "transcript_path": "Z:\\nowhere\\zcode-hook-BIXdU1\\transcript.jsonl",
        });
        for (k, v) in more.as_object().unwrap() {
            e[k] = v.clone();
        }
        e
    }

    #[test]
    fn zcode_events_become_messages() {
        let m = to_message(&event("SessionStart", json!({ "model": "deepseek/deepseek-flash", "source": "startup" }))).unwrap();
        assert_eq!(m, json!({ "mood": "idle", "react": "wave", "say": { "key": "say.hello" }, "agent": "zcode", "session": "sess_fc09", "project": "zc" }));
        let m = to_message(&event("UserPromptSubmit", json!({ "prompt": "Create a file named note.txt containing hi" }))).unwrap();
        assert_eq!((m["event"].as_str(), m["task"].as_str()), (Some("turn-start"), Some("Create a file named note.txt containing hi")));
        let write = json!({ "file_path": "Z:\\nowhere\\zc\\note.txt", "content": "hi\n" });
        let call = json!({ "toolCallId": "call_00_UlU5", "toolInput": write, "toolName": "Write", "tool_name": "Write", "tool_input": write, "tool_use_id": "call_00_UlU5", "riskLevel": "medium" });
        let m = to_message(&event("PreToolUse", call.clone())).unwrap();
        assert_eq!((m["step"].clone(), m["toolId"].as_str()), (json!({ "key": "step.write", "vars": { "file": "note.txt" } }), Some("call_00_UlU5")));
        let mut ask = call.clone();
        ask["reason"] = json!("Tool has side effects and requires approval");
        ask["requestId"] = json!("perm_20d3");
        let m = to_message(&event("PermissionRequest", ask)).unwrap();
        assert_eq!((m["mood"].as_str(), m["detail"]["key"].as_str()), (Some("waiting"), Some("detail.approve")));
        let m = to_message(&event("PostToolUse", call.clone())).unwrap();
        assert_eq!((m["event"].as_str(), m["file"].as_str()), (Some("tool-done"), Some("Z:\\nowhere\\zc\\note.txt")));
        let mut failed = call.clone();
        failed["error"] = json!("EACCES");
        failed["is_interrupt"] = json!(false);
        assert_eq!(to_message(&event("PostToolUseFailure", failed)).unwrap()["event"], "tool-failed");
        let m = to_message(&event("Stop", json!({ "responseText": "hello", "last_assistant_message": "hello", "stop_hook_active": false, "toolCallCount": 0 }))).unwrap();
        assert_eq!((m["mood"].as_str(), m["reply"].as_str(), m["agent"].as_str()), (Some("done"), Some("hello"), Some("zcode")));
        // Its subagents' tool, by either name.
        for tool in ["Agent", "Task"] {
            let m = to_message(&event("PreToolUse", json!({ "tool_name": tool, "tool_input": { "description": "Explore the code" } }))).unwrap();
            assert_eq!(m["step"], json!({ "key": "step.agent", "vars": { "what": "Explore the code" } }));
        }
    }

    #[test]
    fn zcode_events_that_change_nothing() {
        let mut stopped = event("PostToolUseFailure", json!({ "tool_name": "Bash", "error": "cancelled" }));
        stopped["is_interrupt"] = json!(true);
        assert!(to_message(&stopped).is_none());
        assert!(to_message(&event("Nope", json!({}))).is_none());
        assert!(to_message(&json!("nope")).is_none());
    }
}
