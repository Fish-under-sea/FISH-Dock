// Antigravity hook events → pet messages, the same messages as events.rs makes
// (state.rs has the format), plus agent: "agy". The words for a task, a reply
// and the project are events.rs's.
//
// Antigravity (agy, and its 2.0 app) runs its hooks as commands: this program
// with connection::AGY_FLAG and the event's name, which its payload does not
// carry, hands each to /hook?agent=agy (main.rs agy_hook). What there is, as
// agy 1.3.4 sends it (2026-10-13): camelCase; conversationId the session,
// workspacePaths the folders it works in, transcriptPath its transcript.
// SessionStart; PreInvocation before each call of the model, the first of a
// turn invocationNum 0; PostToolUse with toolCall { name, args }, the args
// saying what was done in words of their own (toolAction: "Creating
// hello.txt"); Stop with fullyIdle, terminationReason and error. No prompt
// waiting on the person, no tool starting (PreToolUse would have to answer
// for every tool, and an answer missing denies it): she follows the work and
// its end. The person's words and the last reply come from the transcript
// (with_transcript), as the payload has neither.
//
// A command it leaves running in the background (run_command's
// WaitMsBeforeAsync), as agy 1.3.4 does it (2026-10-14): the model answers,
// Stop with fullyIdle false (still at work, for her); the command ends, its
// PostToolUse comes, a SYSTEM_MESSAGE step says so and the model is called
// again from invocationNum 0, then Stop, fully idle. That second start is the
// same turn: a turn is known by the person's USER_INPUT step it answers
// (turn), else she counted it twice and its time from the second start.
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};

use serde_json::{json, Value};

use crate::events::{clip, project_of, reply_of, step, task_of};

fn s<'a>(e: &'a Value, key: &str) -> &'a str {
    e.get(key).and_then(Value::as_str).unwrap_or("")
}

// What a tool did, in plain words: its own, else its name.
fn step_of_agy(tool: &str, args: &Value) -> Value {
    match [s(args, "toolAction"), s(args, "toolSummary")].into_iter().find(|w| !w.trim().is_empty()) {
        Some(words) => step("step.run", json!({ "what": clip(words.trim(), 60) })),
        None => step("step.tool", json!({ "tool": tool })),
    }
}

// The mood part of the message for an event, or None for one that changes nothing.
fn mood_of(e: &Value) -> Option<Value> {
    let call = e.get("toolCall").unwrap_or(&Value::Null);
    let tool = s(call, "name");
    let args = call.get("args").unwrap_or(&Value::Null);
    let m = match s(e, "hook_event_name") {
        "SessionStart" => json!({ "mood": "idle", "react": "wave", "say": { "key": "say.hello" } }),
        // The turn's first call of the model: a turn begins.
        "PreInvocation" if e.get("invocationNum").and_then(Value::as_u64) == Some(0) => {
            let mut m = json!({ "mood": "working", "event": "turn-start" });
            if let Some(task) = task_of(s(e, "prompt")) {
                m["task"] = json!(task);
            }
            if let Some(step) = e.get("userStep").and_then(Value::as_u64) {
                m["turn"] = json!(step.to_string());
            }
            m
        }
        "PreInvocation" => json!({ "mood": "working" }),
        "PostToolUse" if !s(e, "error").is_empty() => json!({
            "mood": "working", "detail": tool, "event": "tool-failed", "react": "failed",
            "say": { "key": "say.toolFailed", "vars": { "tool": tool } },
        }),
        "PostToolUse" => {
            let mut m = json!({ "mood": "working", "detail": tool, "event": "tool-done", "step": step_of_agy(tool, args) });
            // A file it wrote (write_to_file's, the edits' TargetFile): what
            // the turn changed. (A file read is view_file's AbsolutePath.)
            if !s(args, "TargetFile").is_empty() {
                m["file"] = json!(s(args, "TargetFile"));
            }
            m
        }
        "Stop" if !s(e, "error").is_empty() => json!({ "mood": "error", "error": clip(s(e, "error").lines().next().unwrap_or(""), 80) }),
        // Stopped by the person: nothing to come and see.
        "Stop" if ["CANCEL", "INTERRUPT", "ABORT"].iter().any(|w| s(e, "terminationReason").to_uppercase().contains(w)) => json!({ "mood": "idle", "event": "turn-end" }),
        // Not done yet (something of it still runs).
        "Stop" if e.get("fullyIdle").and_then(Value::as_bool) == Some(false) => return None,
        "Stop" => {
            let mut m = json!({ "mood": "done", "event": "turn-end" });
            if let Some(reply) = reply_of(s(e, "last_assistant_message")) {
                m["reply"] = json!(reply);
            }
            m
        }
        _ => return None,
    };
    Some(m)
}

// The message for an event, or None for one that changes nothing.
pub fn to_message(e: &Value) -> Option<Value> {
    if !e.is_object() {
        return None;
    }
    let mut message = mood_of(e)?;
    message["agent"] = json!("agy");
    if !s(e, "conversationId").is_empty() {
        message["session"] = json!(s(e, "conversationId"));
    }
    if let Some(folder) = e.get("workspacePaths").and_then(Value::as_array).and_then(|w| w.first()).and_then(Value::as_str).filter(|w| !w.is_empty()) {
        message["project"] = json!(project_of(folder));
        crate::events::place(&mut message, folder);
    }
    Some(message)
}

// So much of the transcript's end is read: a turn's lines are there.
const TAIL: u64 = 256 * 1024;

// The person's words for a turn starting (prompt) and the step they are
// (userStep: the turn), and the last reply for one ending
// (last_assistant_message), from the transcript's last lines, as agy 1.3.4
// writes it: one step a line, the person's a USER_INPUT whose content holds
// them in <USER_REQUEST>, the model's answer a PLANNER_RESPONSE with content.
// Undocumented: nothing when it reads otherwise.
pub fn with_transcript(e: &mut Value) {
    let event = s(e, "hook_event_name");
    let wants = match event {
        "PreInvocation" if e.get("invocationNum").and_then(Value::as_u64) == Some(0) => "USER_INPUT",
        "Stop" => "PLANNER_RESPONSE",
        _ => return,
    };
    let Some(lines) = tail(s(e, "transcriptPath")) else { return };
    let mut steps = lines.lines().rev().filter_map(|line| serde_json::from_str::<Value>(line).ok());
    if wants == "USER_INPUT" {
        // The person's last words, with or without any (an image alone).
        let Some(step) = steps.find(|step| s(step, "type") == wants) else { return };
        if let Some(at) = step.get("step_index").and_then(Value::as_u64) {
            e["userStep"] = json!(at);
        }
        let content = s(&step, "content");
        let said = content.split_once("<USER_REQUEST>").and_then(|(_, rest)| rest.split_once("</USER_REQUEST>")).map_or(content, |(said, _)| said);
        if !said.trim().is_empty() {
            e["prompt"] = json!(said.trim());
        }
    } else if let Some(step) = steps.find(|step| s(step, "type") == wants && !s(step, "content").trim().is_empty()) {
        e["last_assistant_message"] = json!(s(&step, "content").trim());
    }
}

// A line for her log as a turn begins or ends: what Antigravity said, to
// tell a turn that never ended from one shown short.
pub fn log_line(e: &Value) -> Option<String> {
    let name = s(e, "hook_event_name");
    let what = match name {
        "SessionStart" => String::new(),
        "PreInvocation" if e.get("invocationNum").and_then(Value::as_u64) == Some(0) => format!(" 0, the person's step {}", e["userStep"]),
        "Stop" => format!(", fullyIdle {}, {}{}", e["fullyIdle"], s(e, "terminationReason"), if s(e, "error").is_empty() { "" } else { ", an error" }),
        _ => return None,
    };
    Some(format!("agy {name}{what} ({})", s(e, "conversationId").chars().take(8).collect::<String>()))
}

fn tail(path: &str) -> Option<String> {
    if path.is_empty() {
        return None;
    }
    let mut file = File::open(path).ok()?;
    let len = file.metadata().ok()?.len();
    file.seek(SeekFrom::Start(len.saturating_sub(TAIL))).ok()?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes).ok()?;
    Some(String::from_utf8_lossy(&bytes).into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    // As agy 1.3.4 sends them (trimmed), the event's name added by main.rs.
    fn event(name: &str, more: Value) -> Value {
        let mut e = json!({
            "hook_event_name": name,
            "conversationId": "5064aa2e",
            "modelName": "gemini-3.8-flash-high",
            "workspacePaths": ["Z:/nowhere/agyws"],
            "transcriptPath": "",
        });
        for (k, v) in more.as_object().unwrap() {
            e[k] = v.clone();
        }
        e
    }

    #[test]
    fn agy_events_become_messages() {
        let m = to_message(&event("SessionStart", json!({}))).unwrap();
        assert_eq!(m, json!({ "mood": "idle", "react": "wave", "say": { "key": "say.hello" }, "agent": "agy", "session": "5064aa2e", "project": "agyws", "workspace": "Z:/nowhere/agyws" }));
        let m = to_message(&event("PreInvocation", json!({ "invocationNum": 0, "initialNumSteps": 1, "prompt": "Create a file named hello.txt" }))).unwrap();
        assert_eq!((m["event"].as_str(), m["task"].as_str()), (Some("turn-start"), Some("Create a file named hello.txt")));
        let m = to_message(&event("PreInvocation", json!({ "invocationNum": 1, "initialNumSteps": 3 }))).unwrap();
        assert_eq!((m["mood"].as_str(), m.get("event")), (Some("working"), None));
        let call = json!({ "name": "write_to_file", "args": { "CodeContent": "hi", "TargetFile": "Z:\\nowhere\\agyws\\hello.txt", "toolAction": "Creating hello.txt", "toolSummary": "Create hello.txt" } });
        let m = to_message(&event("PostToolUse", json!({ "stepIdx": 2, "error": "", "toolCall": call }))).unwrap();
        assert_eq!(m["step"], json!({ "key": "step.run", "vars": { "what": "Creating hello.txt" } }));
        assert_eq!((m["event"].as_str(), m["file"].as_str()), (Some("tool-done"), Some("Z:\\nowhere\\agyws\\hello.txt")));
        let m = to_message(&event("PostToolUse", json!({ "error": "exit status 1", "toolCall": { "name": "run_command", "args": {} } }))).unwrap();
        assert_eq!((m["event"].as_str(), m["react"].as_str()), (Some("tool-failed"), Some("failed")));
        let m = to_message(&event("Stop", json!({ "executionNum": 0, "fullyIdle": true, "terminationReason": "NO_TOOL_CALL", "error": "", "last_assistant_message": "done" }))).unwrap();
        assert_eq!((m["mood"].as_str(), m["event"].as_str(), m["reply"].as_str()), (Some("done"), Some("turn-end"), Some("done")));
        let m = to_message(&event("Stop", json!({ "fullyIdle": true, "error": "quota exceeded" }))).unwrap();
        assert_eq!((m["mood"].as_str(), m["error"].as_str()), (Some("error"), Some("quota exceeded")));
        let m = to_message(&event("Stop", json!({ "fullyIdle": true, "terminationReason": "USER_CANCELLED", "error": "" }))).unwrap();
        assert_eq!((m["mood"].as_str(), m["event"].as_str()), (Some("idle"), Some("turn-end")));
    }

    #[test]
    fn agy_events_that_change_nothing() {
        assert!(to_message(&event("Stop", json!({ "fullyIdle": false, "error": "" }))).is_none());
        for name in ["PostInvocation", "PreToolUse", "Nope", ""] {
            assert!(to_message(&event(name, json!({}))).is_none(), "{name}");
        }
        assert!(to_message(&json!("nope")).is_none());
    }

    #[test]
    fn the_person_s_words_and_the_reply_from_the_transcript() {
        let file = std::env::temp_dir().join(format!("wakuwaku-agy-{}.jsonl", std::process::id()));
        let lines = [
            json!({ "step_index": 0, "source": "USER_EXPLICIT", "type": "USER_INPUT", "status": "DONE", "content": "<USER_REQUEST>\nCreate a file named hello.txt\n</USER_REQUEST>\n<ADDITIONAL_METADATA>\nThe current local time is 09:47.\n</ADDITIONAL_METADATA>" }),
            json!({ "step_index": 1, "source": "MODEL", "type": "PLANNER_RESPONSE", "status": "DONE", "thinking": "write it", "tool_calls": [] }),
            json!({ "step_index": 3, "source": "MODEL", "type": "PLANNER_RESPONSE", "status": "DONE", "content": "done" }),
        ];
        std::fs::write(&file, lines.iter().map(Value::to_string).collect::<Vec<_>>().join("\n")).unwrap();
        let path = file.to_string_lossy().into_owned();
        let mut e = event("PreInvocation", json!({ "invocationNum": 0, "transcriptPath": path }));
        with_transcript(&mut e);
        assert_eq!((e["prompt"].as_str(), e["userStep"].as_u64()), (Some("Create a file named hello.txt"), Some(0)));
        assert_eq!(to_message(&e).unwrap()["turn"], "0");
        let mut e = event("Stop", json!({ "fullyIdle": true, "transcriptPath": path }));
        with_transcript(&mut e);
        assert_eq!(e["last_assistant_message"], "done");
        // A later call of the model reads nothing.
        let mut e = event("PreInvocation", json!({ "invocationNum": 1, "transcriptPath": path }));
        with_transcript(&mut e);
        assert!(e.get("prompt").is_none());
        // The person's next words, an image alone: a turn of its own, without words.
        let mut f = std::fs::OpenOptions::new().append(true).open(&file).unwrap();
        std::io::Write::write_all(&mut f, format!("\n{}", json!({ "step_index": 4, "type": "USER_INPUT", "content": "" })).as_bytes()).unwrap();
        let mut e = event("PreInvocation", json!({ "invocationNum": 0, "transcriptPath": path }));
        with_transcript(&mut e);
        assert_eq!((e.get("prompt"), e["userStep"].as_u64()), (None, Some(4)));
        let _ = std::fs::remove_file(&file);
    }

    // Each event as the server takes it, applied at a time (ms): the turns it
    // began and the time it counted.
    fn feed(pet: &mut crate::state::Pet, name: &str, more: Value, at: u64) -> (u64, u64) {
        let mut e = event(name, more);
        with_transcript(&mut e);
        to_message(&e).and_then(|m| pet.apply(&m, at, crate::state::Hold::Seen)).map_or((0, 0), |out| (out.turns, out.worked_ms))
    }

    #[test]
    fn an_agy_round_counts_its_turn_and_its_time() {
        // The plainest: the model called, a tool, the end.
        let mut pet = crate::state::Pet::default();
        assert_eq!(feed(&mut pet, "SessionStart", json!({}), 0), (0, 0));
        assert_eq!(feed(&mut pet, "PreInvocation", json!({ "invocationNum": 0 }), 1_000), (1, 0));
        let call = json!({ "name": "write_to_file", "args": { "TargetFile": "Z:\\nowhere\\agyws\\a.txt", "toolAction": "Creating a.txt" } });
        assert_eq!(feed(&mut pet, "PostToolUse", json!({ "error": "", "toolCall": call }), 4_000), (0, 0));
        assert_eq!(feed(&mut pet, "Stop", json!({ "fullyIdle": true, "terminationReason": "NO_TOOL_CALL", "error": "" }), 6_500), (0, 5_500));

        // A command left running in the background, as agy 1.3.4 ran one: the
        // model answers (not fully idle), the command ends, the model is
        // called again from 0, then the end. One turn, all of its time.
        let file = std::env::temp_dir().join(format!("wakuwaku-agy-round-{}.jsonl", std::process::id()));
        let path = file.to_string_lossy().into_owned();
        let _ = std::fs::remove_file(&file);
        let write = |steps: &[Value]| {
            let mut f = std::fs::OpenOptions::new().create(true).append(true).open(&file).unwrap();
            let lines: String = steps.iter().map(|s| format!("{s}\n")).collect();
            std::io::Write::write_all(&mut f, lines.as_bytes()).unwrap();
        };
        let at = |more: Value| {
            let mut more = more;
            more["transcriptPath"] = json!(path);
            more
        };
        let mut pet = crate::state::Pet::default();
        write(&[json!({ "step_index": 0, "source": "USER_EXPLICIT", "type": "USER_INPUT", "status": "DONE", "content": "<USER_REQUEST>\nping in the background\n</USER_REQUEST>" })]);
        assert_eq!(feed(&mut pet, "PreInvocation", at(json!({ "invocationNum": 0, "initialNumSteps": 1 })), 100), (1, 0));
        write(&[
            json!({ "step_index": 1, "source": "MODEL", "type": "PLANNER_RESPONSE", "status": "DONE", "tool_calls": [{ "name": "run_command" }] }),
            json!({ "step_index": 2, "source": "MODEL", "type": "GENERIC", "status": "RUNNING", "content": "Tool is running as a background task" }),
        ]);
        assert_eq!(feed(&mut pet, "PreInvocation", at(json!({ "invocationNum": 1, "initialNumSteps": 3 })), 9_000), (0, 0));
        write(&[json!({ "step_index": 3, "source": "MODEL", "type": "PLANNER_RESPONSE", "status": "DONE", "content": "started" })]);
        assert_eq!(feed(&mut pet, "Stop", at(json!({ "executionNum": 0, "fullyIdle": false, "terminationReason": "NO_TOOL_CALL", "error": "" })), 13_800), (0, 0));
        assert_eq!(pet.get(13_900)["mood"], "working");
        assert_eq!(feed(&mut pet, "PostToolUse", at(json!({ "stepIdx": 2, "error": "", "toolCall": { "name": "run_command", "args": { "toolAction": "Running ping" } } })), 20_300), (0, 0));
        write(&[json!({ "step_index": 4, "source": "SYSTEM", "type": "SYSTEM_MESSAGE", "status": "DONE", "content": "the task is done" })]);
        assert_eq!(feed(&mut pet, "PreInvocation", at(json!({ "invocationNum": 0, "initialNumSteps": 4 })), 20_500), (0, 0));
        write(&[json!({ "step_index": 5, "source": "MODEL", "type": "PLANNER_RESPONSE", "status": "DONE", "content": "It finished." })]);
        assert_eq!(feed(&mut pet, "Stop", at(json!({ "executionNum": 0, "fullyIdle": true, "terminationReason": "NO_TOOL_CALL", "error": "" })), 23_600), (0, 23_500));
        assert_eq!((pet.get(23_700)["mood"].as_str(), pet.get(23_700)["reply"].as_str()), (Some("done"), Some("It finished.")));

        // The person's next words, stopped by them: a turn, and its time.
        write(&[json!({ "step_index": 6, "source": "USER_EXPLICIT", "type": "USER_INPUT", "status": "DONE", "content": "<USER_REQUEST>\nnext\n</USER_REQUEST>" })]);
        assert_eq!(feed(&mut pet, "PreInvocation", at(json!({ "invocationNum": 0, "initialNumSteps": 7 })), 60_000), (1, 0));
        assert_eq!(feed(&mut pet, "Stop", at(json!({ "fullyIdle": true, "terminationReason": "USER_CANCELED", "error": "" })), 70_000), (0, 10_000));
        let _ = std::fs::remove_file(&file);
    }

    #[test]
    fn turns_begin_and_end_in_her_log() {
        let line = |name: &str, more: Value| log_line(&event(name, more));
        assert_eq!(line("SessionStart", json!({})).as_deref(), Some("agy SessionStart (5064aa2e)"));
        assert_eq!(line("PreInvocation", json!({ "invocationNum": 0, "userStep": 6 })).as_deref(), Some("agy PreInvocation 0, the person's step 6 (5064aa2e)"));
        assert_eq!(line("Stop", json!({ "fullyIdle": false, "terminationReason": "NO_TOOL_CALL", "error": "" })).as_deref(), Some("agy Stop, fullyIdle false, NO_TOOL_CALL (5064aa2e)"));
        assert!(line("PreInvocation", json!({ "invocationNum": 1 })).is_none() && line("PostToolUse", json!({})).is_none());
    }
}
