// oh-my-pi (omp) events → pet messages, the same messages as events.rs makes
// (state.rs has the format), plus agent: "omp".
//
// omp has no hooks of its own to run: her extension inside it
// (integrations/omp/wakuwaku.ts, written by connection.rs) hands each event
// to /hook?agent=omp in Claude Code's shape, so most are read as events.rs
// reads Claude Code's. What differs: omp's tools are named in lower case,
// their file argument `path` (events.rs reads Claude Code's names, as
// step_of, the edits that make a done a review, the files a turn changed),
// and each tool call comes with its intent, the model's words for what it
// does ("Reading a.txt"), for the tools that have no words of ours. And a
// few of its own: Interrupt (the person stopped the turn), ApprovalRequested
// and ApprovalResolved (omp asks for a tool in its own window, which she
// cannot answer: waiting there, and back), and a plan review answered
// (PostToolUse for ExitPlanMode with the choice: a turn follows, or none).
// As omp 18.11.1 sends them (2026-10-15).
use serde_json::{json, Value};

use crate::events::{self, clip, project_of, step, step_of};

fn s<'a>(e: &'a Value, key: &str) -> &'a str {
    e.get(key).and_then(Value::as_str).unwrap_or("")
}

// The first of these arguments given.
fn first<'a>(input: &'a Value, keys: &[&str]) -> &'a str {
    keys.iter().map(|k| s(input, k)).find(|v| !v.trim().is_empty()).unwrap_or("")
}

// An omp tool by Claude Code's name, with the arguments step_of reads, or
// None for one of its own (lsp, python, browser, eval, …).
fn as_claude(tool: &str, input: &Value, intent: &str) -> Option<(&'static str, Value)> {
    let path = || first(input, &["path", "file_path"]);
    Some(match tool {
        "bash" => ("Bash", json!({ "command": s(input, "command"), "description": intent })),
        "read" => ("Read", json!({ "file_path": path() })),
        "write" => ("Write", json!({ "file_path": path() })),
        "edit" | "ast_edit" => ("Edit", json!({ "file_path": path() })),
        "notebook" => ("NotebookEdit", json!({ "notebook_path": path() })),
        "grep" | "ast_grep" => ("Grep", json!({ "pattern": first(input, &["pattern", "query"]) })),
        // glob takes its pattern as `path` ("*.txt").
        "glob" | "find" => ("Glob", json!({ "pattern": first(input, &["pattern", "path", "query"]) })),
        "task" => {
            let job = input.get("tasks").and_then(Value::as_array).and_then(|t| t.first()).unwrap_or(&Value::Null);
            let what = if intent.is_empty() { first(job, &["name", "description"]) } else { intent };
            ("Task", json!({ "description": what, "subagent_type": s(job, "agent") }))
        }
        "ask" => ("AskUserQuestion", json!({})),
        "todo" => ("TodoWrite", json!({})),
        "web_search" => ("WebSearch", json!({ "query": s(input, "query") })),
        "fetch" => {
            let item = input.get("items").and_then(Value::as_array).and_then(|i| i.first()).unwrap_or(&Value::Null);
            ("WebFetch", json!({ "url": if s(input, "url").is_empty() { s(item, "url") } else { s(input, "url") } }))
        }
        _ => return None,
    })
}

// What a tool is doing, in plain words: ours for the tools Claude Code has
// too, else the model's intent, else its name.
fn step_of_omp(tool: &str, input: &Value, intent: &str) -> Value {
    let said = || step("step.run", json!({ "what": clip(intent, 60) }));
    match as_claude(tool, input, intent) {
        // A tool of ours with nothing to name (no file, no pattern): its intent.
        Some((name, args)) => {
            let blank = args.as_object().is_some_and(|a| !a.is_empty() && a.values().all(|v| v.as_str().is_some_and(|t| t.trim().is_empty())));
            if blank && !intent.is_empty() { said() } else { step_of(name, &args) }
        }
        None if tool.starts_with("mcp__") || intent.is_empty() => step_of(tool, input),
        None => said(),
    }
}

// The event with omp's tool as Claude Code's (its own kept as it is).
fn translated(e: &Value) -> Value {
    let mut e = e.clone();
    let (tool, intent) = (s(&e, "tool_name").to_string(), s(&e, "intent").to_string());
    let input = e.get("tool_input").cloned().unwrap_or(Value::Null);
    if let Some((name, args)) = as_claude(&tool, &input, &intent) {
        e["tool_name"] = json!(name);
        e["tool_input"] = args;
    }
    e
}

// What a plan review answered leads to: a turn (executed, refined), or none
// (saved, put away).
fn after_review(choice: &str) -> Value {
    if matches!(choice, "execute" | "compact" | "keep" | "refine") {
        json!({ "mood": "working" })
    } else {
        json!({ "mood": "idle" })
    }
}

// The message for an event, or None for one that changes nothing.
pub fn to_message(e: &Value) -> Option<Value> {
    if !e.is_object() {
        return None;
    }
    let (name, tool) = (s(e, "hook_event_name"), s(e, "tool_name"));
    let mut message = match name {
        "Interrupt" => json!({ "mood": "idle", "event": "turn-end" }),
        "ApprovalRequested" => {
            let what = step_of_omp(tool, e.get("tool_input").unwrap_or(&Value::Null), s(e, "intent"));
            json!({ "mood": "waiting", "detail": { "key": "detail.approveInTerminal", "vars": { "what": what } } })
        }
        "ApprovalResolved" => json!({ "mood": "working" }),
        "PostToolUse" if tool == "ExitPlanMode" => after_review(s(e, "choice")),
        _ => {
            let mut m = events::to_message(&translated(e))?;
            let input = e.get("tool_input").unwrap_or(&Value::Null);
            if name == "PreToolUse" && m.get("step").is_some() {
                m["step"] = step_of_omp(tool, input, s(e, "intent"));
            }
            // The file an edit changed, as omp resolved it (a whole path).
            if m.get("file").is_some() && !s(e, "file").is_empty() {
                m["file"] = json!(s(e, "file"));
            }
            m
        }
    };
    message["agent"] = json!("omp");
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

    // As the extension sends them from omp 18.11.1 (trimmed); the folder is no git repository.
    fn event(name: &str, more: Value) -> Value {
        let mut e = json!({ "session_id": "01a14146", "cwd": "Z:\\nowhere\\proj", "hook_event_name": name });
        for (k, v) in more.as_object().unwrap() {
            e[k] = v.clone();
        }
        e
    }

    fn tool(name: &str, args: Value, intent: &str) -> Value {
        to_message(&event("PreToolUse", json!({ "tool_name": name, "tool_input": args, "tool_use_id": "call_1", "intent": intent }))).unwrap()
    }

    #[test]
    fn omp_events_become_messages() {
        let m = to_message(&event("SessionStart", json!({ "source": "startup" }))).unwrap();
        assert_eq!(m, json!({ "mood": "idle", "react": "wave", "say": { "key": "say.hello" }, "session": "01a14146", "project": "proj", "agent": "omp", "workspace": "Z:\\nowhere\\proj" }));
        let m = to_message(&event("UserPromptSubmit", json!({ "prompt": "Run the shell command echo hi" }))).unwrap();
        assert_eq!((m["mood"].as_str(), m["event"].as_str(), m["task"].as_str()), (Some("working"), Some("turn-start"), Some("Run the shell command echo hi")));
        let m = tool("bash", json!({ "command": "echo hi", "timeout": 30, "cwd": "C:/waku" }), "Running echo hi");
        assert_eq!(
            m,
            json!({ "mood": "working", "detail": "Bash", "event": "tool-start", "step": { "key": "step.run", "vars": { "what": "Running echo hi" } },
                    "toolId": "call_1", "session": "01a14146", "project": "proj", "agent": "omp", "workspace": "Z:\\nowhere\\proj" })
        );
        let m = to_message(&event("Stop", json!({ "last_assistant_message": "Done.\nMore." }))).unwrap();
        assert_eq!((m["mood"].as_str(), m["reply"].as_str(), m["agent"].as_str()), (Some("done"), Some("Done. More."), Some("omp")));
        let m = to_message(&event("StopFailure", json!({ "error": "", "error_details": "Connection error.\nretry later" }))).unwrap();
        assert_eq!((m["mood"].as_str(), m["error"].as_str()), (Some("error"), Some("Connection error.")));
        let m = to_message(&event("Interrupt", json!({}))).unwrap();
        assert_eq!((m["mood"].as_str(), m["event"].as_str()), (Some("idle"), Some("turn-end")));
        assert_eq!(to_message(&event("SessionEnd", json!({ "reason": "exit" }))).unwrap()["event"], "session-end");
        let m = to_message(&event("Stop", json!({ "session_title": "Echo a greeting" }))).unwrap();
        assert_eq!(m["title"], "Echo a greeting");
    }

    #[test]
    fn omp_tools_say_what_they_do() {
        assert_eq!(tool("read", json!({ "path": "src/a.txt" }), "Reading a.txt")["step"], json!({ "key": "step.read", "vars": { "file": "a.txt" } }));
        assert_eq!(tool("write", json!({ "path": "a.txt", "content": "hi\n" }), "")["step"], json!({ "key": "step.write", "vars": { "file": "a.txt" } }));
        let m = tool("edit", json!({ "path": "a.txt", "old_string": "hi", "new_string": "bye" }), "Changing hi to bye");
        assert_eq!((m["detail"].as_str(), &m["step"]), (Some("Edit"), &json!({ "key": "step.edit", "vars": { "file": "a.txt" } })));
        assert_eq!(tool("grep", json!({ "pattern": "bye", "path": "." }), "")["step"], json!({ "key": "step.search", "vars": { "what": "bye" } }));
        assert_eq!(tool("glob", json!({ "path": "*.txt" }), "Listing *.txt files")["step"], json!({ "key": "step.find", "vars": { "what": "*.txt" } }));
        assert_eq!(tool("bash", json!({ "command": "cd x && cargo test" }), "")["step"], json!({ "key": "step.command", "vars": { "what": "cargo test" } }));
        let task = json!({ "context": "…", "tasks": [{ "name": "SayHi", "agent": "sonic", "task": "Reply hi" }] });
        assert_eq!(tool("task", task.clone(), "Starting one subagent to reply hi")["step"], json!({ "key": "step.agent", "vars": { "what": "Starting one subagent to reply hi" } }));
        assert_eq!(tool("task", task, "")["step"]["vars"]["what"], "SayHi");
        assert_eq!(tool("web_search", json!({ "query": "omp extensions" }), "")["step"], json!({ "key": "step.webSearch", "vars": { "what": "omp extensions" } }));
        assert_eq!(tool("todo", json!({ "ops": [] }), "")["step"]["key"], "step.plan");
        // Its own tools: the model's words for them, else the name.
        assert_eq!(tool("lsp", json!({ "action": "hover" }), "Checking the type")["step"], json!({ "key": "step.run", "vars": { "what": "Checking the type" } }));
        assert_eq!(tool("lsp", json!({}), "")["step"], json!({ "key": "step.tool", "vars": { "tool": "lsp" } }));
        assert_eq!(tool("mcp__github__get_issue", json!({}), "Reading the issue")["step"], json!({ "key": "step.mcp", "vars": { "server": "github", "tool": "get_issue" } }));
        // One of ours with nothing to name: its intent.
        assert_eq!(tool("edit", json!({ "ops": [] }), "Fixing the import")["step"], json!({ "key": "step.run", "vars": { "what": "Fixing the import" } }));
        // A question waits on the person.
        assert_eq!(tool("ask", json!({ "questions": [] }), "")["mood"], "waiting");
    }

    #[test]
    fn an_edit_brings_its_file_and_a_failure_says_so() {
        let done = json!({ "tool_name": "edit", "tool_input": { "path": "a.txt" }, "tool_use_id": "c2", "file": "C:\\waku\\omp\\proj\\a.txt" });
        let m = to_message(&event("PostToolUse", done)).unwrap();
        assert_eq!((m["detail"].as_str(), m["event"].as_str(), m["file"].as_str(), m["toolId"].as_str()), (Some("Edit"), Some("tool-done"), Some("C:\\waku\\omp\\proj\\a.txt"), Some("c2")));
        // A read's path is no edit.
        let m = to_message(&event("PostToolUse", json!({ "tool_name": "read", "tool_input": { "path": "a.txt" }, "file": "C:\\a.txt" }))).unwrap();
        assert!(m.get("file").is_none());
        let m = to_message(&event("PostToolUseFailure", json!({ "tool_name": "bash", "tool_input": { "command": "false" }, "error": "exit 1" }))).unwrap();
        assert_eq!((m["event"].as_str(), m["react"].as_str(), &m["say"]["vars"]["tool"]), (Some("tool-failed"), Some("failed"), &json!("Bash")));
        // The edit made the turn's done a review.
        let mut pet = crate::state::Pet::default();
        for (e, at) in [(event("UserPromptSubmit", json!({ "prompt": "fix it" })), 1), (event("PostToolUse", json!({ "tool_name": "write", "tool_input": { "path": "b.txt" } })), 2), (event("Stop", json!({})), 3)] {
            pet.apply(&to_message(&e).unwrap(), at, crate::state::Hold::Seen);
        }
        assert_eq!((pet.get(4)["mood"].as_str(), pet.get(4)["agent"].as_str()), (Some("review"), Some("omp")));
    }

    #[test]
    fn approvals_wait_in_omp_and_plans_on_her() {
        let m = to_message(&event("ApprovalRequested", json!({ "tool_name": "bash", "tool_input": { "command": "rm -rf build" }, "intent": "" }))).unwrap();
        assert_eq!(m["mood"], "waiting");
        assert_eq!(m["detail"], json!({ "key": "detail.approveInTerminal", "vars": { "what": { "key": "step.command", "vars": { "what": "rm -rf build" } } } }));
        assert_eq!(to_message(&event("ApprovalResolved", json!({ "approved": true }))).unwrap()["mood"], "working");
        // A plan review: waiting on her, as Claude Code's ExitPlanMode.
        let plan = json!({ "tool_name": "ExitPlanMode", "tool_input": { "plan": "1. Write a.txt" }, "tool_use_id": "r1" });
        let m = to_message(&event("PermissionRequest", plan.clone())).unwrap();
        assert_eq!((m["mood"].as_str(), &m["detail"]), (Some("waiting"), &json!({ "key": "detail.planToConfirm" })));
        let mut answered = plan.clone();
        answered["choice"] = json!("execute");
        assert_eq!(to_message(&event("PostToolUse", answered.clone())).unwrap()["mood"], "working");
        answered["choice"] = json!("save");
        assert_eq!(to_message(&event("PostToolUse", answered.clone())).unwrap()["mood"], "idle");
        answered["choice"] = json!("");
        assert_eq!(to_message(&event("PostToolUse", answered)).unwrap()["mood"], "idle");
    }

    #[test]
    fn omp_events_that_change_nothing() {
        for name in ["Nope", ""] {
            assert!(to_message(&event(name, json!({}))).is_none(), "{name}");
        }
        assert!(to_message(&json!("nope")).is_none());
    }
}
