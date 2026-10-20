// Reasonix hook events → pet messages: read as Claude Code's (events.rs),
// plus agent: "reasonix" (zhoupengjie/Wakuwaku#60).
//
// Reasonix runs the hooks in its global settings.json (connection.rs), each
// as this program with connection::REASONIX_FLAG and the event's name,
// which hands it to /hook?agent=reasonix (main.rs reasonix_hook). Its
// payload is its own (its hook package's Payload, 2.x and the Go 1.x alike):
// event, sessionId, cwd (the workspace's root), toolName and toolArgs (its
// tools' own names and arguments: write_file with path), toolResult, prompt
// and turn, lastAssistantText, error, isInterrupt, source, reason, message
// and notificationType; read here as Claude Code's fields and tools (the
// table Reasonix itself uses for the Claude Code hooks it runs), so events.rs
// reads them as it reads Claude Code's. What differs, as its source has it
// (studio, 2.43, 2026-10-17):
//   - a failed turn fires StopFailure, then its settings' Stop hooks again
//     with the same payload (error set): that Stop is no ending of its own;
//     a failed tool, PostToolUseFailure and then PostToolUse the same way;
//     the person stopping a turn is a StopFailure with isInterrupt;
//   - SessionStart comes with the session's first turn, not as it opens;
//   - its subagents (a task, explore, ...), its planner and its guardian
//     fire the tool hooks under ids of their own, the session's with
//     ":subagent:<ref>", ":planner" or ":guardian" after it: told as the
//     session's steps while a turn of it is under way (within), never as a
//     session of their own;
//   - a PermissionRequest comes before its dialog, a Notification
//     (permission_prompt) as the dialog shows: the first says what waits,
//     the second only says it again (but for a plan's approval and Auto
//     Guard's, which have no PermissionRequest); its questions (the ask
//     tool) come as PreToolUse, then a Notification (question_prompt).
// Its approvals are answered in Reasonix (zhoupengjie/Wakuwaku#60, phase 1).
use std::collections::HashMap;
use std::sync::Mutex;

use serde_json::{json, Map, Value};

use crate::events::{self, clip};

fn s<'a>(e: &'a Value, key: &str) -> &'a str {
    e.get(key).and_then(Value::as_str).unwrap_or("")
}

// Its tools as Claude Code names them (Reasonix's own table for the hooks it
// imports from Claude Code: claudeToolNames in its hook package), each
// subagent's starter as Agent; one of its own (code_index, move_file, ...)
// keeps its name, as does an MCP tool (mcp__<server>__<tool> in both).
fn claude_name(tool: &str) -> &str {
    match tool {
        "bash" => "Bash",
        "read_file" => "Read",
        "write_file" => "Write",
        "edit_file" => "Edit",
        "multi_edit" => "MultiEdit",
        "glob" => "Glob",
        "grep" => "Grep",
        "web_fetch" => "WebFetch",
        "web_search" => "WebSearch",
        "ask" => "AskUserQuestion",
        "run_skill" | "read_only_skill" => "Skill",
        "todo_write" => "TodoWrite",
        "notebook_edit" => "NotebookEdit",
        "bash_output" | "wait" => "TaskOutput",
        "kill_shell" => "TaskStop",
        "task" | "read_only_task" | "parallel_tasks" | "explore" | "locate" | "research" | "review" | "security_review" => "Agent",
        _ => tool,
    }
}

// Its tool's arguments with Claude Code's names for them (the same table's
// claudeToolInputKeyRenames, for what events.rs reads: a file's path, a
// skill's name, a subagent's task); a file's path made whole from the
// session's folder, as its tools resolve one (the files a turn changed).
fn claude_input(tool: &str, args: &Value, cwd: &str) -> Value {
    let renames: &[(&str, &str)] = match tool {
        "read_file" | "write_file" | "edit_file" | "multi_edit" => &[("path", "file_path")],
        "notebook_edit" => &[("path", "notebook_path")],
        "run_skill" | "read_only_skill" => &[("name", "skill")],
        "explore" | "locate" | "research" | "review" | "security_review" => &[("task", "prompt")],
        _ => &[],
    };
    let Some(fields) = args.as_object() else { return args.clone() };
    let mut out: Map<String, Value> = fields.clone();
    for (from, to) in renames {
        if let Some(v) = out.remove(*from) {
            out.insert((*to).into(), v);
        }
    }
    for key in ["file_path", "notebook_path"] {
        let path = out.get(key).and_then(Value::as_str).unwrap_or("");
        if !path.is_empty() && !cwd.is_empty() && !std::path::Path::new(path).is_absolute() {
            let whole = std::path::Path::new(cwd).join(path).to_string_lossy().into_owned();
            out.insert(key.into(), json!(whole));
        }
    }
    Value::Object(out)
}

// The person's words in the prompt Reasonix hands its UserPromptSubmit hook:
// the input it composed for the model, which begins with blocks of its own
// (<current-date>…</current-date>, <available-skills>…</available-skills>,
// as 2.44 sends it, 2026-10-18) before them, each closed on a line of its
// own; those taken off. Words that begin with a tag of their own (closed on
// the same line) are left as they are.
fn person_words(prompt: &str) -> &str {
    let mut rest = prompt.trim_start();
    while let Some(after) = rest.strip_prefix('<') {
        let Some(name) = after.split_once('>').map(|(n, _)| n).filter(|n| !n.is_empty() && n.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')) else { break };
        let close = format!("\n</{name}>");
        let Some(end) = rest.find(&close) else { break };
        rest = rest[end + close.len()..].trim_start();
    }
    if rest.is_empty() { prompt } else { rest }
}

// The event in Claude Code's words, for events.rs: Reasonix's own payload
// (camelCase) with the fields renamed, its tool and arguments as Claude
// Code's.
pub fn as_claude(e: &Value) -> Value {
    let tool = s(e, "toolName");
    let args = match e.get("toolArgs") {
        // Its arguments as the model gave them: JSON, or a string of it.
        Some(Value::String(text)) => serde_json::from_str(text).unwrap_or(Value::Null),
        Some(v) => v.clone(),
        None => Value::Null,
    };
    let mut out = json!({
        "hook_event_name": s(e, "event"),
        "session_id": s(e, "sessionId"),
        "cwd": s(e, "cwd"),
        "tool_name": claude_name(tool),
        "tool_input": claude_input(tool, &args, s(e, "cwd")),
        "prompt": person_words(s(e, "prompt")),
        "last_assistant_message": s(e, "lastAssistantText"),
        "source": s(e, "source"),
        "reason": s(e, "reason"),
        "message": s(e, "message"),
        "notification_type": s(e, "notificationType"),
        "is_interrupt": e.get("isInterrupt").and_then(Value::as_bool).unwrap_or(false),
        // Why a turn failed, in its own words (events.rs error_of).
        "error_details": s(e, "error"),
    });
    if !s(e, "callId").is_empty() {
        out["tool_use_id"] = json!(s(e, "callId"));
    }
    out
}

// The session an id is, and whether it is one working for it (a subagent,
// its planner, its guardian: Reasonix's hook runner names theirs after the
// session's, ForRole): "<session>:subagent:<ref>", "<session>:planner",
// "<session>:guardian".
pub fn whose(id: &str) -> (&str, bool) {
    if let Some((session, _)) = id.split_once(":subagent:") {
        return (session, true);
    }
    for role in [":planner", ":guardian"] {
        if let Some(session) = id.strip_suffix(role) {
            return (session, true);
        }
    }
    (id, false)
}

// When each session last asked for an approval through PermissionRequest
// (ms), so that its Notification for the same dialog, just after, changes
// nothing: the first already said what waits, in her words.
static ASKED: Mutex<Option<HashMap<String, u64>>> = Mutex::new(None);
// How long after a PermissionRequest its Notification is taken as the same
// dialog's (it comes at once, in its own goroutine).
const SAME_ASK_MS: u64 = 30_000;

fn asked(session: &str, now: u64) {
    let mut asked = ASKED.lock().unwrap_or_else(|e| e.into_inner());
    let asked = asked.get_or_insert_with(HashMap::new);
    asked.retain(|_, at| now.saturating_sub(*at) < SAME_ASK_MS);
    asked.insert(session.to_string(), now);
}

fn was_asked(session: &str, now: u64) -> bool {
    let asked = ASKED.lock().unwrap_or_else(|e| e.into_inner());
    asked.as_ref().and_then(|a| a.get(session)).is_some_and(|at| now.saturating_sub(*at) < SAME_ASK_MS)
}

// The message for an event, or None for one that changes nothing.
pub fn to_message(e: &Value) -> Option<Value> {
    to_message_at(e, crate::now_ms())
}

fn to_message_at(e: &Value, now: u64) -> Option<Value> {
    if !e.is_object() {
        return None;
    }
    let (session, is_within) = whose(s(e, "sessionId"));
    let failed = !s(e, "error").is_empty();
    let is_interrupt = e.get("isInterrupt").and_then(Value::as_bool) == Some(true);
    let claude = as_claude(e);
    let mut message = match s(e, "event") {
        // A prompt for its approval: answered in Reasonix (phase 1: she shows
        // the session waiting on it, and what it waits for).
        "PermissionRequest" => {
            let mut m = events::to_message(&claude)?;
            if m["detail"]["key"] == "detail.approve" {
                m["detail"] = json!({ "key": "detail.approveIn", "vars": { "app": "Reasonix", "what": m["detail"]["vars"]["what"].clone() } });
            }
            asked(session, now);
            m
        }
        "Notification" => notification(e, session, now)?,
        // A failed turn's or tool's, again as its plain end: told already.
        "Stop" | "PostToolUse" if failed => return None,
        // The person stopped the turn: at rest, nothing to come and see.
        "StopFailure" if is_interrupt => json!({ "mood": "idle", "event": "turn-end" }),
        // Its subagents' bounds and model calls, compactions: nothing to her.
        "SubagentStart" | "SubagentStop" | "PostLLMCall" | "PreCompact" => return None,
        _ => events::to_message(&claude)?,
    };
    message["agent"] = json!("reasonix");
    if !session.is_empty() {
        message["session"] = json!(session);
    }
    if is_within {
        // A subagent's step, as its session's; never a session, a turn or an
        // ending of its own.
        if message["event"] == "session-end" || message["event"] == "turn-start" || matches!(message["mood"].as_str(), Some("done" | "error" | "idle")) {
            return None;
        }
        message["within"] = json!(true);
        if let Some(m) = message.as_object_mut() {
            m.remove("react");
            m.remove("say");
            m.remove("task");
            m.remove("reply");
        }
    }
    if !s(e, "cwd").is_empty() && message.get("project").is_none() {
        message["project"] = json!(events::project_of(s(e, "cwd")));
    }
    Some(message)
}

// What it wants the person's attention for: an approval (said already by its
// PermissionRequest, unless none came: a plan's, Auto Guard's), a question
// (said already by the ask tool's start, and said the same again), or
// something else, in its own words.
fn notification(e: &Value, session: &str, now: u64) -> Option<Value> {
    let said = clip(s(e, "message"), 80);
    match s(e, "notificationType") {
        "permission_prompt" if was_asked(session, now) => None,
        "permission_prompt" => Some(json!({ "mood": "waiting", "detail": { "key": "detail.approveIn", "vars": { "app": "Reasonix", "what": said } } })),
        "question_prompt" => Some(json!({ "mood": "waiting", "detail": { "key": "detail.asksQuestion" } })),
        _ if said.is_empty() => Some(json!({ "mood": "waiting", "detail": { "key": "detail.needsYou" } })),
        _ => Some(json!({ "mood": "waiting", "detail": said })),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // As Reasonix's hook package writes them (its Payload), in a folder that
    // is no git repository.
    fn event(name: &str, more: Value) -> Value {
        let mut e = json!({ "event": name, "sessionId": "rx-7f3a", "cwd": "Z:\\nowhere\\rx" });
        for (k, v) in more.as_object().unwrap() {
            e[k] = v.clone();
        }
        e
    }

    #[test]
    fn reasonix_events_become_messages() {
        let m = to_message(&event("SessionStart", json!({ "source": "startup" }))).unwrap();
        assert_eq!(m, json!({ "mood": "idle", "react": "wave", "say": { "key": "say.hello" }, "agent": "reasonix", "session": "rx-7f3a", "project": "rx" }));
        let m = to_message(&event("UserPromptSubmit", json!({ "prompt": "Create a file named note.txt containing hi", "turn": 1 }))).unwrap();
        assert_eq!((m["mood"].as_str(), m["event"].as_str(), m["task"].as_str()), (Some("working"), Some("turn-start"), Some("Create a file named note.txt containing hi")));
        // Its tools by Claude Code's names, their arguments too: the step says the file.
        let write = json!({ "path": "note.txt", "content": "hi\n" });
        let m = to_message(&event("PreToolUse", json!({ "toolName": "write_file", "toolArgs": write }))).unwrap();
        assert_eq!((m["mood"].as_str(), m["detail"].as_str()), (Some("working"), Some("Write")));
        assert_eq!(m["step"], json!({ "key": "step.write", "vars": { "file": "note.txt" } }));
        // Arguments as a string of JSON read the same.
        let m = to_message(&event("PreToolUse", json!({ "toolName": "edit_file", "toolArgs": "{\"path\":\"src\\\\a.rs\"}" }))).unwrap();
        assert_eq!(m["step"], json!({ "key": "step.edit", "vars": { "file": "a.rs" } }));
        let m = to_message(&event("PreToolUse", json!({ "toolName": "bash", "toolArgs": { "command": "cd x && cargo test" } }))).unwrap();
        assert_eq!(m["step"], json!({ "key": "step.command", "vars": { "what": "cargo test" } }));
        // The file it changed, made whole from the session's folder; one whole already as it is.
        let m = to_message(&event("PostToolUse", json!({ "toolName": "write_file", "toolArgs": write, "toolResult": "wrote 3 bytes" }))).unwrap();
        let whole = std::path::Path::new("Z:\\nowhere\\rx").join("note.txt").to_string_lossy().into_owned();
        assert_eq!((m["event"].as_str(), m["file"].as_str()), (Some("tool-done"), Some(whole.as_str())));
        if cfg!(windows) {
            let m = to_message(&event("PostToolUse", json!({ "toolName": "edit_file", "toolArgs": { "path": "D:\\w\\b.rs" } }))).unwrap();
            assert_eq!(m["file"], "D:\\w\\b.rs");
        }
        let m = to_message(&event("PostToolUseFailure", json!({ "toolName": "bash", "toolArgs": { "command": "false" }, "error": "exit status 1" }))).unwrap();
        assert_eq!((m["event"].as_str(), m["react"].as_str()), (Some("tool-failed"), Some("failed")));
        // A subagent's starter, by any of its names.
        for tool in ["task", "explore", "parallel_tasks"] {
            let m = to_message(&event("PreToolUse", json!({ "toolName": tool, "toolArgs": { "description": "Explore the code" } }))).unwrap();
            assert_eq!(m["step"], json!({ "key": "step.agent", "vars": { "what": "Explore the code" } }), "{tool}");
        }
        // Its question to the person, and its to-do list.
        assert_eq!(to_message(&event("PreToolUse", json!({ "toolName": "ask", "toolArgs": { "questions": [] } }))).unwrap()["mood"], "waiting");
        let todo = to_message(&event("PreToolUse", json!({ "toolName": "todo_write", "toolArgs": { "todos": [{ "content": "Run tests", "activeForm": "Running tests", "status": "in_progress" }] } }))).unwrap();
        assert_eq!(todo["todo"], json!({ "set": [{ "text": "Run tests", "active": "Running tests", "status": "in_progress" }] }));
        // A tool of its own, and an MCP one.
        assert_eq!(to_message(&event("PreToolUse", json!({ "toolName": "code_index" }))).unwrap()["step"], json!({ "key": "step.tool", "vars": { "tool": "code_index" } }));
        assert_eq!(to_message(&event("PreToolUse", json!({ "toolName": "mcp__github__create_issue" }))).unwrap()["step"]["key"], "step.mcp");
        // A prompt for its approval: waiting, answered in Reasonix.
        let m = to_message(&event("PermissionRequest", json!({ "toolName": "bash", "toolArgs": { "command": "git push" } }))).unwrap();
        assert_eq!(m["mood"], "waiting");
        assert_eq!(m["detail"], json!({ "key": "detail.approveIn", "vars": { "app": "Reasonix", "what": { "key": "step.command", "vars": { "what": "git push" } } } }));
        // Its end, with its last words; a failure, with why.
        let m = to_message(&event("Stop", json!({ "lastAssistantText": "## Done\nCreated **note.txt** with `hi`.", "turn": 1 }))).unwrap();
        assert_eq!((m["mood"].as_str(), m["reply"].as_str(), m["agent"].as_str()), (Some("done"), Some("Done: Created note.txt with hi."), Some("reasonix")));
        let m = to_message(&event("StopFailure", json!({ "error": "deepseek: 402 Insufficient Balance" }))).unwrap();
        assert_eq!((m["mood"].as_str(), m["error"].as_str()), (Some("error"), Some("deepseek: 402 Insufficient Balance")));
        let m = to_message(&event("SessionEnd", json!({ "reason": "exit" }))).unwrap();
        assert_eq!((m["mood"].as_str(), m["event"].as_str()), (Some("idle"), Some("session-end")));
    }

    #[test]
    fn the_person_s_words_past_reasonix_s_own_blocks() {
        // As Reasonix 2.44 composes it (trimmed): its blocks, then the person's words.
        let prompt = "<current-date>\nToday's date is 2026-10-18 (Sunday); the local time zone is UTC+02:00.\n</current-date>\n\n<available-skills>\n# Skills\n- explore [🧬 subagent] — Explore…\n</available-skills>\n\nList the files in this folder and reply in one short sentence.";
        assert_eq!(person_words(prompt), "List the files in this folder and reply in one short sentence.");
        let m = to_message(&event("UserPromptSubmit", json!({ "prompt": prompt, "turn": 1 }))).unwrap();
        assert_eq!(m["task"], "List the files in this folder and reply in one short sentence.");
        // Plain words, words beginning with a tag of their own, a block never closed: as they are.
        assert_eq!(person_words("fix the login page"), "fix the login page");
        assert_eq!(person_words("<b>bold</b> is a tag"), "<b>bold</b> is a tag");
        assert_eq!(person_words("<div class=x>keep</div>"), "<div class=x>keep</div>");
        assert_eq!(person_words("<current-date>\nno end"), "<current-date>\nno end");
        assert_eq!(person_words("<current-date>x</current-date>"), "<current-date>x</current-date>");
    }

    #[test]
    fn a_failure_is_told_once_and_a_stop_by_the_person_is_no_failure() {
        // A failed turn: StopFailure, then its Stop hooks again with the error.
        let m = to_message(&event("StopFailure", json!({ "error": "context deadline exceeded", "turn": 2 }))).unwrap();
        assert_eq!(m["mood"], "error");
        assert!(to_message(&event("Stop", json!({ "error": "context deadline exceeded", "turn": 2 }))).is_none());
        // A failed tool: PostToolUseFailure, then PostToolUse again with the error.
        let failed = json!({ "toolName": "edit_file", "toolArgs": { "path": "a.rs" }, "error": "old_string not found" });
        assert_eq!(to_message(&event("PostToolUseFailure", failed.clone())).unwrap()["event"], "tool-failed");
        assert!(to_message(&event("PostToolUse", failed)).is_none());
        // The person stopping the turn: at rest, no error; its Stop again changes nothing.
        let stopped = json!({ "error": "context canceled", "isInterrupt": true });
        let m = to_message(&event("StopFailure", stopped.clone())).unwrap();
        assert_eq!((m["mood"].as_str(), m["event"].as_str(), m.get("error")), (Some("idle"), Some("turn-end"), None));
        assert!(to_message(&event("Stop", stopped)).is_none());
    }

    #[test]
    fn subagents_work_for_their_session() {
        assert_eq!(whose("20261018-101500.123456789-deepseek-chat"), ("20261018-101500.123456789-deepseek-chat", false));
        assert_eq!(whose("s1:subagent:call_7"), ("s1", true));
        assert_eq!(whose("s1:subagent:a:subagent:b"), ("s1", true));
        assert_eq!(whose("s1:planner"), ("s1", true));
        assert_eq!(whose("s1:guardian"), ("s1", true));
        // A subagent's step: the session's, within a turn of it.
        let m = to_message(&event("PreToolUse", json!({ "sessionId": "rx-7f3a:subagent:call_7", "toolName": "grep", "toolArgs": { "pattern": "TODO" } }))).unwrap();
        assert_eq!((m["session"].as_str(), m["within"].as_bool(), m["mood"].as_str()), (Some("rx-7f3a"), Some(true), Some("working")));
        assert_eq!(m["step"], json!({ "key": "step.search", "vars": { "what": "TODO" } }));
        // Its edits count as the session's; a failure of its tool is said by no reaction of its own.
        let m = to_message(&event("PostToolUse", json!({ "sessionId": "rx-7f3a:subagent:call_7", "toolName": "write_file", "toolArgs": { "path": "C:\\w\\n.md" } }))).unwrap();
        assert_eq!((m["file"].as_str(), m["within"].as_bool()), (Some("C:\\w\\n.md"), Some(true)));
        let m = to_message(&event("PostToolUseFailure", json!({ "sessionId": "rx-7f3a:guardian", "toolName": "bash", "error": "exit 1" }))).unwrap();
        assert!(m.get("react").is_none() && m.get("say").is_none());
        // Never a turn, an ending or a session of its own.
        for (name, more) in [("UserPromptSubmit", json!({ "prompt": "Explore the code for TODOs" })), ("Stop", json!({ "lastAssistantText": "Found 3." })), ("SessionStart", json!({})), ("SessionEnd", json!({}))] {
            let mut more = more;
            more["sessionId"] = json!("rx-7f3a:subagent:call_7");
            assert!(to_message(&event(name, more)).is_none(), "{name}");
        }
    }

    #[test]
    fn an_approval_is_told_once_with_what_waits() {
        let ask = |name: &str, more: Value, now: u64| to_message_at(&event(name, more), now);
        let session = json!({ "sessionId": "rx-ask-1" });
        let mut pr = session.clone();
        pr["toolName"] = json!("bash");
        pr["toolArgs"] = json!({ "command": "git push" });
        pr["subject"] = json!("git push");
        assert_eq!(ask("PermissionRequest", pr, 1_000).unwrap()["detail"]["key"], "detail.approveIn");
        // Its Notification as the dialog shows: said already.
        let mut note = session.clone();
        note["notificationType"] = json!("permission_prompt");
        note["message"] = json!("Approval needed: bash (git push)");
        assert!(ask("Notification", note.clone(), 1_050).is_none());
        // One with no PermissionRequest before it (a plan's, Auto Guard's): in its words.
        let m = ask("Notification", note.clone(), 1_000 + SAME_ASK_MS + 1).unwrap();
        assert_eq!(m["detail"], json!({ "key": "detail.approveIn", "vars": { "app": "Reasonix", "what": "Approval needed: bash (git push)" } }));
        let mut other = note.clone();
        other["sessionId"] = json!("rx-ask-2");
        other["message"] = json!("Auto Guard: confirm the next action");
        assert_eq!(ask("Notification", other, 1_100).unwrap()["mood"], "waiting");
        // A question: as the ask tool's start said it.
        let mut question = session.clone();
        question["notificationType"] = json!("question_prompt");
        assert_eq!(ask("Notification", question, 2_000).unwrap()["detail"], json!({ "key": "detail.asksQuestion" }));
        // Anything else: in its words, or that it needs the person.
        let mut else_ = session;
        else_["notificationType"] = json!("idle");
        assert_eq!(ask("Notification", else_.clone(), 3_000).unwrap()["detail"], json!({ "key": "detail.needsYou" }));
        else_["message"] = json!("Waiting for your input");
        assert_eq!(ask("Notification", else_, 3_000).unwrap()["detail"], "Waiting for your input");
    }

    #[test]
    fn reasonix_events_that_change_nothing() {
        // The person stopping a tool is no failure of it.
        assert!(to_message(&event("PostToolUseFailure", json!({ "toolName": "bash", "isInterrupt": true }))).is_none());
        for name in ["SubagentStart", "SubagentStop", "PostLLMCall", "PreCompact", "Nope"] {
            assert!(to_message(&event(name, json!({ "callId": "c1" }))).is_none(), "{name}");
        }
        assert!(to_message(&json!("nope")).is_none());
    }
}
