// Codex hook events → pet messages, the same messages as events.rs makes
// (state.rs has the format), plus agent: "codex" and the turn they belong to.
// The words for a step, a task, a reply and the project are events.rs's.
//
// Codex runs its hooks as commands: this program with connection::CODEX_FLAG
// hands each event to /hook?agent=codex (main.rs). What differs from Claude
// Code: the shell tool is Bash and every edit is apply_patch (the patch is
// tool_input.command); PostToolUse comes for a failed command too, with only
// what it printed, so its exit code is read from the session's rollout
// (command_failed); a failed request gets no Stop; Interrupt is the person
// stopping a turn. Each step's hooks run in the background (connection.rs),
// so one can arrive after its turn is over: the turn id tells it apart
// (state.rs).
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};

use serde_json::{json, Value};

use crate::events::{base, project_of, reply_of, step, step_of, task_of};

fn s<'a>(e: &'a Value, key: &str) -> &'a str {
    e.get(key).and_then(Value::as_str).unwrap_or("")
}

// So much of a rollout's end is read for a command's ending: its line, with
// all the command printed (a few hundred KB at most seen), is written before
// its PostToolUse runs, and only a few lines after it by the time a hook in
// the background reads it.
const ROLLOUT_TAIL: u64 = 1024 * 1024;

fn tail(path: &str, bytes: u64) -> Option<String> {
    if path.is_empty() {
        return None;
    }
    let mut file = File::open(path).ok()?;
    let len = file.metadata().ok()?.len();
    file.seek(SeekFrom::Start(len.saturating_sub(bytes))).ok()?;
    let mut read = Vec::new();
    file.read_to_end(&mut read).ok()?;
    Some(String::from_utf8_lossy(&read).into_owned())
}

// Whether the shell command `id` failed. Its PostToolUse says only what it
// printed, a non-zero exit as a zero one (CLI 0.162); the session's rollout
// (the hook's transcript_path) has its ending by then, as Codex writes it:
// an event_msg item_completed whose item is a CommandExecution of the hook's
// tool_use_id, with its exit_code. Undocumented: not failed when it reads
// otherwise, nor with no rollout (codex exec --ephemeral); a patch or an MCP
// tool is never judged here (the hook asks only about Bash).
pub fn command_failed(rollout: &str, id: &str) -> bool {
    if id.is_empty() {
        return false;
    }
    let Some(lines) = tail(rollout, ROLLOUT_TAIL) else { return false };
    let quoted = format!("\"{id}\"");
    lines
        .lines()
        .rev()
        .filter(|line| line.contains("\"item_completed\"") && line.contains(&quoted))
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .find(|entry| s(&entry["payload"], "type") == "item_completed" && s(&entry["payload"]["item"], "type") == "CommandExecution" && s(&entry["payload"]["item"], "id") == id)
        .and_then(|entry| entry["payload"]["item"]["exit_code"].as_i64())
        .is_some_and(|code| code != 0)
}

// Whether Codex approves the session's requests itself: the person let it
// (the desktop app's Approve for me; approvals_reviewer = "auto_review",
// "guardian_subagent" before). Codex runs the PermissionRequest hooks first,
// and asks its own reviewer, never the person, once none answered: one held
// for the person to answer only kept them from it (zhoupengjie/Wakuwaku#90).
// The hook does not say (its permission_mode is "default" either way); the
// session's rollout does, in the settings Codex writes as each turn the
// person begins starts, before any of its steps (turn_context): the latest is
// this turn's. Its last MiB read, all of it where that has none (a turn that
// printed much). Undocumented, as Codex writes it now (CLI 0.162): not when
// it reads otherwise, nor with no rollout.
pub fn auto_reviewed(rollout: &str) -> bool {
    let latest = |lines: &str| {
        lines
            .lines()
            .rev()
            .filter(|line| line.contains("\"turn_context\""))
            .filter_map(|line| serde_json::from_str::<Value>(line).ok())
            .find(|entry| s(entry, "type") == "turn_context")
            .map(|entry| s(&entry["payload"], "approvals_reviewer").to_string())
    };
    let Some(end) = tail(rollout, ROLLOUT_TAIL) else { return false };
    let reviewer = latest(&end).or_else(|| tail(rollout, u64::MAX).and_then(|all| latest(&all)));
    matches!(reviewer.as_deref(), Some("auto_review" | "guardian_subagent"))
}

// The files a patch adds, changes or deletes.
pub(crate) fn patched(patch: &str) -> Vec<&str> {
    patch
        .lines()
        .filter_map(|line| ["*** Add File: ", "*** Update File: ", "*** Delete File: "].iter().find_map(|p| line.strip_prefix(p)))
        .map(str::trim)
        .collect()
}

// What a tool is doing, in plain words: a patch by the file it edits.
fn step_of_codex(tool: &str, input: &Value) -> Value {
    if tool != "apply_patch" {
        return step_of(tool, input);
    }
    match patched(s(input, "command")).as_slice() {
        [] => step("step.tool", json!({ "tool": tool })),
        [one] => step("step.edit", json!({ "file": base(one) })),
        [first, rest @ ..] => step("step.edit", json!({ "file": format!("{} +{}", base(first), rest.len()) })),
    }
}

// The mood part of the message for an event, or None for one that changes nothing.
fn mood_of(e: &Value) -> Option<Value> {
    let tool = s(e, "tool_name");
    let input = e.get("tool_input").unwrap_or(&Value::Null);
    let mut m = match s(e, "hook_event_name") {
        // A compaction starts a new context mid-work: nothing changed for the pet.
        "SessionStart" if s(e, "source") == "compact" => return None,
        "SessionStart" => json!({ "mood": "idle", "react": "wave", "say": { "key": "say.hello" } }),
        "SessionEnd" => json!({ "mood": "idle", "event": "session-end" }),
        "UserPromptSubmit" => {
            let mut m = json!({ "mood": "working", "event": "turn-start" });
            if let Some(task) = task_of(s(e, "prompt")) {
                m["task"] = json!(task);
            }
            m
        }
        // Codex's question to the person waits on them like a prompt.
        "PreToolUse" if tool == "request_user_input" => json!({ "mood": "waiting", "detail": { "key": "detail.asksQuestion" } }),
        "PreToolUse" => json!({ "mood": "working", "detail": tool, "event": "tool-start", "step": step_of_codex(tool, input) }),
        "PostToolUse" if tool == "Bash" && command_failed(s(e, "transcript_path"), s(e, "tool_use_id")) => json!({
            "mood": "working", "detail": tool, "event": "tool-failed", "react": "failed",
            "say": { "key": "say.toolFailed", "vars": { "tool": tool } }
        }),
        "PostToolUse" => {
            let mut m = json!({ "mood": "working", "detail": tool, "event": "tool-done" });
            if let Some(file) = patched(s(input, "command")).first().filter(|_| tool == "apply_patch") {
                m["file"] = json!(file);
            }
            m
        }
        // Codex's own to approve, not the person's: nothing waits on them.
        "PermissionRequest" if auto_reviewed(s(e, "transcript_path")) => return None,
        "PermissionRequest" => json!({ "mood": "waiting", "detail": { "key": "detail.approve", "vars": { "what": step_of_codex(tool, input) } } }),
        "Stop" => json!({ "mood": "done", "event": "turn-end" }),
        // Stopped by the person: nothing to come and see.
        "Interrupt" => json!({ "mood": "idle", "event": "turn-end" }),
        _ => return None,
    };
    if matches!(s(e, "hook_event_name"), "PreToolUse" | "PostToolUse") && !s(e, "tool_use_id").is_empty() {
        m["toolId"] = json!(s(e, "tool_use_id"));
    }
    if let Some(reply) = reply_of(s(e, "last_assistant_message")) {
        m["reply"] = json!(reply);
    }
    Some(m)
}

// The message for an event, or None for one that changes nothing.
pub fn to_message(e: &Value) -> Option<Value> {
    if !e.is_object() {
        return None;
    }
    let mut message = mood_of(e)?;
    message["agent"] = json!("codex");
    for (from, to) in [("session_id", "session"), ("turn_id", "turn")] {
        if !s(e, from).is_empty() {
            message[to] = json!(s(e, from));
        }
    }
    let cwd = s(e, "cwd");
    if !cwd.is_empty() {
        message["project"] = json!(project_of(cwd));
        crate::events::place(&mut message, cwd);
    }
    Some(message)
}

#[cfg(test)]
mod tests {
    use super::*;

    // As Codex 0.146 sends them (trimmed); the folder is no git repository.
    fn event(name: &str, more: Value) -> Value {
        let mut e = json!({ "session_id": "s", "turn_id": "t1", "cwd": "Z:\\nowhere\\pet", "hook_event_name": name, "model": "gpt-5.6-luna" });
        for (k, v) in more.as_object().unwrap() {
            e[k] = v.clone();
        }
        e
    }

    #[test]
    fn codex_events_become_messages() {
        let m = to_message(&event("PreToolUse", json!({ "tool_name": "Bash", "tool_input": { "command": "cd x && cargo test" }, "tool_use_id": "u1" }))).unwrap();
        assert_eq!(
            m,
            json!({ "mood": "working", "detail": "Bash", "event": "tool-start", "step": { "key": "step.command", "vars": { "what": "cargo test" } },
                    "toolId": "u1", "agent": "codex", "session": "s", "turn": "t1", "project": "pet", "workspace": "Z:\\nowhere\\pet" })
        );
        let patch = "*** Begin Patch\n*** Add File: D:\\w\\note.txt\n+x\n*** End Patch";
        let m = to_message(&event("PostToolUse", json!({ "tool_name": "apply_patch", "tool_input": { "command": patch } }))).unwrap();
        assert_eq!((m["event"].as_str(), m["file"].as_str()), (Some("tool-done"), Some("D:\\w\\note.txt")));
        let m = to_message(&event("PreToolUse", json!({ "tool_name": "apply_patch", "tool_input": { "command": patch } }))).unwrap();
        assert_eq!(m["step"], json!({ "key": "step.edit", "vars": { "file": "note.txt" } }));
        let m = to_message(&event("UserPromptSubmit", json!({ "prompt": "fix the login page" }))).unwrap();
        assert_eq!((m["event"].as_str(), m["task"].as_str()), (Some("turn-start"), Some("fix the login page")));
        let m = to_message(&event("Stop", json!({ "stop_hook_active": false, "last_assistant_message": "All set." }))).unwrap();
        assert_eq!((m["mood"].as_str(), m["event"].as_str(), m["reply"].as_str()), (Some("done"), Some("turn-end"), Some("All set.")));
        let m = to_message(&event("Interrupt", json!({}))).unwrap();
        assert_eq!((m["mood"].as_str(), m["event"].as_str()), (Some("idle"), Some("turn-end")));
        let m = to_message(&event("PermissionRequest", json!({ "tool_name": "Bash", "tool_input": { "command": "rm -rf build" } }))).unwrap();
        assert_eq!(m["detail"], json!({ "key": "detail.approve", "vars": { "what": { "key": "step.command", "vars": { "what": "rm -rf build" } } } }));
        assert_eq!(to_message(&event("PreToolUse", json!({ "tool_name": "request_user_input" }))).unwrap()["mood"], "waiting");
        assert_eq!(to_message(&event("SessionStart", json!({ "source": "startup" }))).unwrap()["react"], "wave");
        assert_eq!(to_message(&event("SessionEnd", json!({ "reason": "other" }))).unwrap()["event"], "session-end");
    }

    #[test]
    fn codex_events_that_change_nothing() {
        assert!(to_message(&event("SessionStart", json!({ "source": "compact" }))).is_none());
        for name in ["PreCompact", "PostCompact", "SubagentStart", "SubagentStop", "Nope"] {
            assert!(to_message(&event(name, json!({}))).is_none(), "{name}");
        }
        assert!(to_message(&json!("nope")).is_none());
    }

    // A command that exits non-zero: failed, by the rollout's record of it
    // (CLI 0.162.1, as measured); one that exits 0, one not in it, one with
    // no rollout, and a patch are done as before.
    #[test]
    fn a_command_failed_by_its_exit_code() {
        let dir = std::env::temp_dir().join(format!("fish-dock-codex-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let rollout = dir.join("rollout.jsonl");
        let ended = |id: &str, code: i64, status: &str| {
            json!({ "timestamp": "2026-10-25T17:33:20.000Z", "type": "event_msg", "payload": { "type": "item_completed", "thread_id": "th", "turn_id": "t1", "item": { "type": "CommandExecution", "id": id, "command": ["pwsh.exe", "-Command", "x"], "status": status, "aggregated_output": format!("\"{id}\" printed"), "exit_code": code } } }).to_string()
        };
        let lines = [
            r#"utput":"…","exit_code":1}}}"#.to_string(),
            ended("exec-1", 1, "failed"),
            ended("exec-2", 0, "completed"),
            json!({ "type": "response_item", "payload": { "type": "message", "role": "assistant", "content": "exec-3 next" } }).to_string(),
        ];
        std::fs::write(&rollout, lines.join("\n")).unwrap();
        let path = rollout.to_string_lossy().to_string();
        assert_eq!((command_failed(&path, "exec-1"), command_failed(&path, "exec-2"), command_failed(&path, "exec-3"), command_failed(&path, "")), (true, false, false, false));
        assert!(!command_failed("Z:\\no\\such\\rollout.jsonl", "exec-1"));
        let done = |id: &str, tool: &str, rollout: &str| {
            to_message(&event("PostToolUse", json!({ "tool_name": tool, "tool_use_id": id, "tool_input": { "command": "Get-Item nope" }, "tool_response": "Get-Item: not found", "transcript_path": rollout })))
        };
        let failed = done("exec-1", "Bash", &path).unwrap();
        assert_eq!((failed["event"].as_str(), failed["react"].as_str(), &failed["say"]["vars"]["tool"]), (Some("tool-failed"), Some("failed"), &json!("Bash")));
        assert_eq!(done("exec-2", "Bash", &path).unwrap()["event"], "tool-done");
        // Only a shell command's; none with no rollout (--ephemeral), nor a patch.
        assert_eq!(done("exec-1", "apply_patch", &path).unwrap()["event"], "tool-done");
        assert_eq!(done("exec-1", "Bash", "").unwrap()["event"], "tool-done");
        let _ = std::fs::remove_dir_all(&dir);
    }

    // Approve for me (approvals_reviewer auto_review, as the turn's settings
    // in the rollout say): a request to approve is Codex's, nothing waits on
    // the person; the person's to answer waits as ever, as with no rollout.
    #[test]
    fn a_request_codex_approves_itself() {
        let dir = std::env::temp_dir().join(format!("fish-dock-codex-review-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let turn = |id: &str, reviewer: Option<&str>| {
            let mut payload = json!({ "turn_id": id, "cwd": "D:\\w", "approval_policy": "on-request", "sandbox_policy": { "type": "workspace-write" }, "model": "gpt-5.6" });
            if let Some(r) = reviewer {
                payload["approvals_reviewer"] = json!(r);
            }
            json!({ "timestamp": "2026-10-25T10:00:00.000Z", "type": "turn_context", "payload": payload }).to_string()
        };
        let said = json!({ "type": "response_item", "payload": { "type": "message", "role": "user", "content": "say \"turn_context\" once" } }).to_string();
        let write = |name: &str, lines: &[String]| {
            let path = dir.join(name);
            std::fs::write(&path, lines.join("\n")).unwrap();
            path.to_string_lossy().to_string()
        };
        let auto = write("auto.jsonl", &[turn("t1", Some("user")), said.clone(), turn("t2", Some("auto_review")), said.clone()]);
        let back = write("back.jsonl", &[turn("t1", Some("auto_review")), turn("t2", Some("user"))]);
        let older = write("older.jsonl", &[turn("t1", Some("guardian_subagent"))]);
        let none = write("none.jsonl", &[turn("t1", None)]);
        assert_eq!((auto_reviewed(&auto), auto_reviewed(&back), auto_reviewed(&older), auto_reviewed(&none), auto_reviewed("")), (true, false, true, false, false));
        let ask = |rollout: &str| to_message(&event("PermissionRequest", json!({ "tool_name": "Bash", "tool_use_id": "exec-9", "tool_input": { "command": "npm install" }, "transcript_path": rollout })));
        assert!(ask(&auto).is_none());
        assert_eq!(ask(&back).unwrap()["mood"], "waiting");
        assert_eq!(ask("").unwrap()["mood"], "waiting");
        // Its settings further back than the end read: found all the same.
        let long = write("long.jsonl", &[turn("t1", Some("auto_review")), "x".repeat(ROLLOUT_TAIL as usize + 10)]);
        assert!(auto_reviewed(&long));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
