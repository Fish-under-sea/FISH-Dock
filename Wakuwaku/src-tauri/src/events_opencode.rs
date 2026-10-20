// OpenCode's events → pet messages, the same messages as events.rs makes
// (state.rs has the format), plus agent: "opencode". The words for a step,
// a task, a reply and the project are events.rs's.
//
// OpenCode 2 has no command hooks: her plugin (integrations/opencode/
// wakuwaku.js, installed by connection.rs) runs in OpenCode's background
// service and POSTs its events to /hook?agent=opencode as OpenCode gives
// them, { type, sessionID, ... }, with what only it knows put in: the
// folder (directory), the session's title, the person's words for a turn
// (prompt), a call's tool, the turn's last text. As OpenCode 2.0.29 sends
// them (2026-10-15):
//   session.created              a session (sub-agents' are not sent)
//   session.execution.started    a turn begins (prompt: the person's words)
//   session.tool.called          a tool starts: id, tool, input
//   session.tool.success|failed  it ends (error: { type, message })
//   permission.asked             it asks the person: id, action ("shell",
//                                "edit", …), resources, save (what "always"
//                                allows), source.id (the call); agentID for
//                                a sub-agent's
//   permission.replied           answered (requestID, reply), anywhere
//   form.created|replied|cancelled  its question tool waits on the person
//   session.execution.succeeded  the turn is done (text: its last words)
//   session.execution.failed     it failed (error)
//   session.execution.interrupted  stopped (reason: user, shutdown, …)
//   session.deleted
// normalize gives each the names Claude Code's hooks have (hook_event_name,
// session_id, cwd, tool_name, tool_input, …): the prompts (asks.rs) and the
// server's look for where a session runs (jump.rs) read them so. A tool by
// Claude Code's name for it (shell is Bash, edit Edit, patch apply_patch),
// for the words of its step and the edits a turn made.
use serde_json::{json, Map, Value};

use crate::events::{base, clip, project_of, reply_of, step, step_of, task_of};
use crate::events_codex::patched;

fn s<'a>(e: &'a Value, key: &str) -> &'a str {
    e.get(key).and_then(Value::as_str).unwrap_or("")
}

// A tool as Claude Code names it, with its input as Claude Code's has it.
fn as_claude(tool: &str, input: &Value) -> (String, Value) {
    let i = |key: &str| json!(s(input, key));
    let (name, input) = match tool {
        "shell" => ("Bash", json!({ "command": i("command"), "description": i("description") })),
        "read" => ("Read", json!({ "file_path": i("path") })),
        "edit" => ("Edit", json!({ "file_path": i("path") })),
        "write" => ("Write", json!({ "file_path": i("path") })),
        "patch" => ("apply_patch", json!({ "command": i("patchText") })),
        "glob" => ("Glob", json!({ "pattern": i("pattern") })),
        "grep" => ("Grep", json!({ "pattern": i("pattern") })),
        "webfetch" => ("WebFetch", json!({ "url": i("url") })),
        "websearch" => ("WebSearch", json!({ "query": i("query") })),
        "subagent" => ("Agent", json!({ "description": i("description"), "subagent_type": i("agent") })),
        "skill" => ("Skill", json!({ "skill": i("id") })),
        _ => return (tool.to_string(), input.clone()),
    };
    (name.to_string(), input)
}

// What a tool is doing, in plain words: a patch by the files it edits.
fn step_of_opencode(tool: &str, input: &Value) -> Value {
    if tool != "apply_patch" {
        return step_of(tool, input);
    }
    match patched(s(input, "command")).as_slice() {
        [] => step("step.tool", json!({ "tool": "patch" })),
        [one] => step("step.edit", json!({ "file": base(one) })),
        [first, rest @ ..] => step("step.edit", json!({ "file": format!("{} +{}", base(first), rest.len()) })),
    }
}

// The file a finished edit changed.
fn edited(tool: &str, input: &Value) -> Option<String> {
    match tool {
        "Edit" | "Write" => Some(s(input, "file_path").to_string()),
        "apply_patch" => patched(s(input, "command")).first().map(|f| f.to_string()),
        _ => None,
    }
    .filter(|f| !f.is_empty())
}

// OpenCode's event under the name Claude Code's hook has for it, with the
// fields its hooks have (the plugin's kept too), or Null for one that is
// nothing to her.
pub fn normalize(e: &Value) -> Value {
    let error = e.get("error").unwrap_or(&Value::Null);
    let name = match s(e, "type") {
        "session.created" => "SessionStart",
        "session.execution.started" => "UserPromptSubmit",
        "session.tool.called" => "PreToolUse",
        "session.tool.success" => "PostToolUse",
        // Turned down by the person ("The user declined this tool call",
        // aborted, as 2.0.29 says it), or stopped: no failure of the tool's.
        "session.tool.failed" if matches!(s(error, "type"), "aborted" | "permission.rejected") => "PostToolUse",
        "session.tool.failed" => "PostToolUseFailure",
        "permission.asked" => "PermissionRequest",
        "permission.replied" => "PermissionReplied",
        "form.created" => "Question",
        "form.replied" | "form.cancelled" => "Answered",
        "session.execution.succeeded" => "Stop",
        // A turn ended by a permission turned down, or cut short, is stopped, not failed.
        "session.execution.failed" if matches!(s(error, "type"), "aborted" | "permission.rejected") => "Interrupt",
        "session.execution.failed" => "StopFailure",
        // Another turn took its place: that one goes on.
        "session.execution.interrupted" if s(e, "reason") == "superseded" => return Value::Null,
        "session.execution.interrupted" => "Interrupt",
        "session.deleted" => "SessionEnd",
        _ => return Value::Null,
    };
    let mut out: Map<String, Value> = e.as_object().cloned().unwrap_or_default();
    out.insert("hook_event_name".into(), json!(name));
    out.insert("session_id".into(), json!(s(e, "sessionID")));
    out.insert("cwd".into(), json!(s(e, "directory")));
    out.insert("agent_id".into(), json!(s(e, "agentID")));
    if !s(e, "title").is_empty() {
        out.insert("session_title".into(), json!(s(e, "title")));
    }
    let input = e.get("input").cloned().unwrap_or_else(|| json!({}));
    match name {
        "PreToolUse" | "PostToolUse" | "PostToolUseFailure" => {
            let (tool, input) = as_claude(s(e, "tool"), &input);
            out.insert("tool_name".into(), json!(tool));
            out.insert("tool_input".into(), input);
            out.insert("tool_use_id".into(), json!(s(e, "id")));
        }
        // The permission asked, by OpenCode's own name for it (shell, edit,
        // external_directory, …), with the call's input as Claude Code's has it.
        "PermissionRequest" => {
            let action = if s(e, "action").is_empty() { s(e, "tool") } else { s(e, "action") };
            let (tool, input) = as_claude(s(e, "tool"), &input);
            out.insert("tool_name".into(), json!(action));
            out.insert("tool_use_id".into(), json!(s(e.get("source").unwrap_or(&Value::Null), "id")));
            out.insert("permission_id".into(), json!(s(e, "id")));
            // What the call it asks for does, in plain words.
            if !s(e, "tool").is_empty() {
                out.insert("call_step".into(), step_of_opencode(&tool, &input));
            }
            out.insert("tool_input".into(), input);
        }
        "PermissionReplied" => {
            out.insert("permission_id".into(), json!(s(e, "requestID")));
        }
        "Stop" => {
            out.insert("last_assistant_message".into(), json!(s(e, "text")));
        }
        _ => {}
    }
    Value::Object(out)
}

// Why a turn failed: one of her error kinds, or OpenCode's own words.
fn error_of(error: &Value) -> Value {
    let key = match s(error, "type") {
        "provider.auth" => "authentication_failed",
        "provider.rate-limit" => "rate_limit",
        "provider.quota" => "billing_error",
        "provider.internal" => "server_error",
        "provider.invalid-request" => "invalid_request",
        "provider.unknown" | "provider.no-route" => "model_not_found",
        _ => return json!(clip(s(error, "message").lines().next().unwrap_or(""), 80)),
    };
    json!({ "key": format!("error.{key}"), "vars": {} })
}

// The mood part of the message for a normalized event, or None for one that changes nothing.
fn mood_of(e: &Value) -> Option<Value> {
    let tool = s(e, "tool_name");
    let input = e.get("tool_input").unwrap_or(&Value::Null);
    let mut m = match s(e, "hook_event_name") {
        "SessionStart" => json!({ "mood": "idle", "react": "wave", "say": { "key": "say.hello" } }),
        "SessionEnd" => json!({ "mood": "idle", "event": "session-end" }),
        "UserPromptSubmit" => {
            let mut m = json!({ "mood": "working", "event": "turn-start" });
            if let Some(task) = task_of(s(e, "prompt")) {
                m["task"] = json!(task);
            }
            m
        }
        // OpenCode's question to the person waits on them like a prompt.
        "PreToolUse" if tool == "question" => json!({ "mood": "waiting", "detail": { "key": "detail.asksQuestion" } }),
        "PreToolUse" => json!({ "mood": "working", "detail": tool, "event": "tool-start", "step": step_of_opencode(tool, input) }),
        "PostToolUse" => {
            let mut m = json!({ "mood": "working", "detail": tool, "event": "tool-done" });
            if let Some(file) = edited(tool, input) {
                m["file"] = json!(file);
            }
            m
        }
        "PostToolUseFailure" => json!({
            "mood": "working", "detail": tool, "event": "tool-failed", "react": "failed",
            "say": { "key": "say.toolFailed", "vars": { "tool": s(e, "tool") } },
        }),
        "PermissionRequest" => {
            let what = e.get("call_step").cloned().unwrap_or_else(|| step("step.tool", json!({ "tool": tool })));
            json!({ "mood": "waiting", "detail": { "key": "detail.approve", "vars": { "what": what } } })
        }
        "Question" => json!({ "mood": "waiting", "detail": { "key": "detail.asksQuestion" } }),
        // Answered, here or there: back at work.
        "PermissionReplied" | "Answered" => json!({ "mood": "working" }),
        "Stop" => json!({ "mood": "done", "event": "turn-end" }),
        "StopFailure" => json!({ "mood": "error", "event": "turn-end", "error": error_of(e.get("error").unwrap_or(&Value::Null)) }),
        // Stopped by the person: nothing to come and see.
        "Interrupt" => json!({ "mood": "idle", "event": "turn-end" }),
        _ => return None,
    };
    if matches!(s(e, "hook_event_name"), "PreToolUse" | "PostToolUse" | "PostToolUseFailure") && !s(e, "tool_use_id").is_empty() {
        m["toolId"] = json!(s(e, "tool_use_id"));
    }
    if let Some(reply) = reply_of(s(e, "last_assistant_message")) {
        m["reply"] = json!(reply);
    }
    Some(m)
}

// The message for a normalized event, or None for one that changes nothing.
pub fn to_message(e: &Value) -> Option<Value> {
    if !e.is_object() {
        return None;
    }
    let mut message = mood_of(e)?;
    message["agent"] = json!("opencode");
    if !s(e, "session_id").is_empty() {
        message["session"] = json!(s(e, "session_id"));
    }
    if !s(e, "cwd").is_empty() {
        message["project"] = json!(project_of(s(e, "cwd")));
    }
    if !s(e, "session_title").is_empty() {
        message["title"] = json!(clip(s(e, "session_title"), 80));
    }
    Some(message)
}

#[cfg(test)]
mod tests {
    use super::*;

    // As her plugin sends them from OpenCode 2.0.29 (trimmed); the folder is no git repository.
    fn event(kind: &str, more: Value) -> Value {
        let mut e = json!({ "type": kind, "sessionID": "ses_1", "directory": "Z:\\nowhere\\oc3", "title": "Shell echo command" });
        for (k, v) in more.as_object().unwrap() {
            e[k] = v.clone();
        }
        e
    }

    fn message(kind: &str, more: Value) -> Option<Value> {
        to_message(&normalize(&event(kind, more)))
    }

    #[test]
    fn opencode_events_become_messages() {
        let m = message("session.created", json!({})).unwrap();
        assert_eq!(
            m,
            json!({ "mood": "idle", "react": "wave", "say": { "key": "say.hello" }, "agent": "opencode", "session": "ses_1", "project": "oc3", "title": "Shell echo command" })
        );
        let m = message("session.execution.started", json!({ "prompt": "Run the shell command: echo hi" })).unwrap();
        assert_eq!((m["event"].as_str(), m["task"].as_str()), (Some("turn-start"), Some("Run the shell command: echo hi")));
        let call = json!({ "id": "call_00_E9yQ", "tool": "shell", "input": { "command": "cd x && echo hi" } });
        let m = message("session.tool.called", call.clone()).unwrap();
        assert_eq!(
            (m["detail"].as_str(), m["event"].as_str(), m["toolId"].as_str(), &m["step"]),
            (Some("Bash"), Some("tool-start"), Some("call_00_E9yQ"), &json!({ "key": "step.command", "vars": { "what": "echo hi" } }))
        );
        let m = message("session.tool.success", call.clone()).unwrap();
        assert_eq!((m["event"].as_str(), m["toolId"].as_str()), (Some("tool-done"), Some("call_00_E9yQ")));
        let m = message("session.tool.failed", json!({ "tool": "shell", "input": {}, "error": { "type": "tool.execution", "message": "exit 1" } })).unwrap();
        assert_eq!((m["event"].as_str(), m["react"].as_str(), &m["say"]["vars"]["tool"]), (Some("tool-failed"), Some("failed"), &json!("shell")));
        let m = message("session.execution.succeeded", json!({ "text": "Done — the command output `hi`." })).unwrap();
        assert_eq!((m["mood"].as_str(), m["reply"].as_str()), (Some("done"), Some("Done — the command output hi.")));
        let m = message("session.execution.failed", json!({ "error": { "type": "provider.rate-limit", "message": "429" } })).unwrap();
        assert_eq!((m["mood"].as_str(), &m["error"]), (Some("error"), &json!({ "key": "error.rate_limit", "vars": {} })));
        let m = message("session.execution.failed", json!({ "error": { "type": "unknown", "message": "Something broke\nmore" } })).unwrap();
        assert_eq!(m["error"], "Something broke");
        let m = message("session.execution.interrupted", json!({ "reason": "user" })).unwrap();
        assert_eq!((m["mood"].as_str(), m["event"].as_str()), (Some("idle"), Some("turn-end")));
        assert_eq!(message("session.deleted", json!({})).unwrap()["event"], "session-end");
        assert_eq!(message("form.created", json!({})).unwrap()["mood"], "waiting");
        assert_eq!(message("form.replied", json!({})).unwrap()["mood"], "working");
    }

    #[test]
    fn edits_name_their_files() {
        let m = message("session.tool.success", json!({ "tool": "edit", "input": { "path": "Z:\\p\\src\\a.rs", "oldString": "a", "newString": "b" } })).unwrap();
        assert_eq!((m["detail"].as_str(), m["file"].as_str()), (Some("Edit"), Some("Z:\\p\\src\\a.rs")));
        let patch = "*** Begin Patch\n*** Add File: hello.txt\n+hi\n*** Update File: b.txt\n@@\n-a\n+b\n*** End Patch";
        let m = message("session.tool.called", json!({ "tool": "patch", "input": { "patchText": patch } })).unwrap();
        assert_eq!(m["step"], json!({ "key": "step.edit", "vars": { "file": "hello.txt +1" } }));
        let m = message("session.tool.success", json!({ "tool": "patch", "input": { "patchText": patch } })).unwrap();
        assert_eq!((m["detail"].as_str(), m["file"].as_str()), (Some("apply_patch"), Some("hello.txt")));
        let m = message("session.tool.called", json!({ "tool": "read", "input": { "path": "/x/notes.md" } })).unwrap();
        assert_eq!(m["step"], json!({ "key": "step.read", "vars": { "file": "notes.md" } }));
        // A tool of its own (an MCP server's): by its name.
        let m = message("session.tool.called", json!({ "tool": "github_create_issue", "input": {} })).unwrap();
        assert_eq!(m["step"], json!({ "key": "step.tool", "vars": { "tool": "github_create_issue" } }));
    }

    #[test]
    fn a_permission_and_its_reply() {
        let asked = normalize(&event(
            "permission.asked",
            json!({ "id": "per_1", "action": "shell", "resources": ["echo hi"], "save": ["echo *"], "source": { "type": "tool", "id": "call_1" }, "tool": "shell", "input": { "command": "echo hi" } }),
        ));
        assert_eq!(
            (asked["hook_event_name"].as_str(), asked["tool_name"].as_str(), asked["permission_id"].as_str(), asked["tool_use_id"].as_str()),
            (Some("PermissionRequest"), Some("shell"), Some("per_1"), Some("call_1"))
        );
        assert_eq!(asked["tool_input"], json!({ "command": "echo hi", "description": "" }));
        let m = to_message(&asked).unwrap();
        assert_eq!(m["detail"], json!({ "key": "detail.approve", "vars": { "what": { "key": "step.command", "vars": { "what": "echo hi" } } } }));
        // One not of a call (a folder outside the project): by its action.
        let m = message("permission.asked", json!({ "id": "per_2", "action": "external_directory", "resources": ["C:\\elsewhere\\*"] })).unwrap();
        assert_eq!(m["detail"]["vars"]["what"], json!({ "key": "step.tool", "vars": { "tool": "external_directory" } }));
        let replied = normalize(&event("permission.replied", json!({ "requestID": "per_1", "reply": "once" })));
        assert_eq!((replied["hook_event_name"].as_str(), replied["permission_id"].as_str()), (Some("PermissionReplied"), Some("per_1")));
        assert_eq!(to_message(&replied).unwrap()["mood"], "working");
        // Turned down: no failure of the tool's, and the turn it ends is stopped.
        let m = message("session.tool.failed", json!({ "tool": "shell", "error": { "type": "aborted", "message": "The user declined this tool call" } })).unwrap();
        assert_eq!((m["event"].as_str(), m.get("react")), (Some("tool-done"), None));
        let m = message("session.tool.failed", json!({ "tool": "shell", "error": { "type": "permission.rejected", "message": "no" } })).unwrap();
        assert_eq!((m["event"].as_str(), m.get("react")), (Some("tool-done"), None));
        let m = message("session.execution.failed", json!({ "error": { "type": "permission.rejected", "message": "no" } })).unwrap();
        assert_eq!((m["mood"].as_str(), m["event"].as_str()), (Some("idle"), Some("turn-end")));
    }

    #[test]
    fn opencode_events_that_change_nothing() {
        for kind in ["session.renamed", "session.step.started", "message.updated", "", "session.execution.interrupted"] {
            let reason = if kind == "session.execution.interrupted" { json!({ "reason": "superseded" }) } else { json!({}) };
            assert!(normalize(&event(kind, reason)).is_null(), "{kind}");
        }
        assert!(to_message(&Value::Null).is_none());
    }

    // Each event as the server takes it, applied at a time (ms): the turns it
    // began and the time it counted.
    #[test]
    fn an_opencode_round_counts_its_turn_and_its_time() {
        let mut pet = crate::state::Pet::default();
        let mut feed = |kind: &str, more: Value, at: u64| {
            message(kind, more).and_then(|m| pet.apply(&m, at, crate::state::Hold::Seen)).map_or((0, 0), |out| (out.turns, out.worked_ms))
        };
        assert_eq!(feed("session.created", json!({}), 0), (0, 0));
        assert_eq!(feed("session.execution.started", json!({ "prompt": "Run the shell command: echo hi" }), 1_000), (1, 0));
        assert_eq!(feed("session.tool.called", json!({ "id": "c1", "tool": "shell", "input": { "command": "echo hi" } }), 2_000), (0, 0));
        assert_eq!(feed("permission.asked", json!({ "id": "per_1", "action": "shell", "resources": ["echo hi"], "tool": "shell" }), 2_100), (0, 0));
        assert_eq!(feed("permission.replied", json!({ "requestID": "per_1", "reply": "once" }), 9_000), (0, 0));
        assert_eq!(feed("session.tool.success", json!({ "id": "c1", "tool": "shell", "input": { "command": "echo hi" } }), 9_300), (0, 0));
        assert_eq!(feed("session.execution.succeeded", json!({ "text": "Done." }), 10_000), (0, 9_000));
    }
}
