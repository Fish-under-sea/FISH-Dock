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
        "PermissionRequest" => {
            let mut m = crate::events::to_message(e)?;
            m.as_object_mut().map(|o| {
                o.remove("agent");
                Value::Object(o.clone())
            })?
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

fn first_str<'a>(args: &'a Value, keys: &[&str]) -> &'a str {
    keys.iter().find_map(|k| args.get(*k).and_then(Value::as_str)).unwrap_or("")
}

pub fn is_question(e: &Value) -> bool {
    e.get("toolCall").and_then(|c| c.get("name")).and_then(Value::as_str) == Some("ask_question")
}

// Map an Antigravity toolCall to Claude Code's (tool_name, tool_input) and the
// permission category ("command" | "write_file" | "read_file" | "read_url" | "mcp"),
// or None when the tool is internal and never requires approval.
fn classify_tool(raw_tool: &str, raw_args: &Value) -> (String, Value, Option<(&'static str, String)>) {
    match raw_tool {
        "run_command" | "pwsh" | "bash" => {
            let cmd = first_str(raw_args, &["CommandLine", "command"]);
            let desc = first_str(raw_args, &["toolSummary", "toolAction", "description"]);
            let mut input = json!({ "command": cmd });
            if !desc.is_empty() {
                input["description"] = json!(desc);
            }
            ("Bash".into(), input, Some(("command", cmd.trim().to_string())))
        }
        "send_command_input" => {
            let cmd = first_str(raw_args, &["Input", "CommandId"]);
            let mut input = json!({ "command": cmd });
            let desc = first_str(raw_args, &["toolSummary", "toolAction"]);
            if !desc.is_empty() {
                input["description"] = json!(desc);
            }
            ("Bash".into(), input, None)
        }
        "write_file" | "write_to_file" => {
            let path = first_str(raw_args, &["Path", "TargetFile", "file_path"]);
            let content = first_str(raw_args, &["Content", "CodeContent", "content"]);
            ("Write".into(), json!({ "file_path": path, "content": content }), Some(("write_file", path.into())))
        }
        "edit_file" | "replace_file_content" | "multi_replace_file_content" | "sed_file" => {
            let path = first_str(raw_args, &["Path", "TargetFile", "file_path"]);
            let mut old = first_str(raw_args, &["Target", "TargetContent", "old_string"]).to_string();
            let mut new = first_str(raw_args, &["Replacement", "ReplacementContent", "new_string"]).to_string();
            if old.is_empty() && new.is_empty() {
                if let Some(chunk) = raw_args.get("ReplacementChunks").and_then(Value::as_array).and_then(|a| a.first()) {
                    old = first_str(chunk, &["TargetContent"]).to_string();
                    new = first_str(chunk, &["ReplacementContent"]).to_string();
                }
            }
            ("Edit".into(), json!({ "file_path": path, "old_string": old, "new_string": new }), Some(("write_file", path.into())))
        }
        "notebook_edit" => {
            let path = first_str(raw_args, &["NotebookPath", "Path", "TargetFile", "notebook_path"]);
            let src = first_str(raw_args, &["NewSource", "Content", "new_source"]);
            ("NotebookEdit".into(), json!({ "notebook_path": path, "new_source": src }), Some(("write_file", path.into())))
        }
        "read_file" | "view_file" | "view_code_item" => {
            let path = first_str(raw_args, &["Path", "AbsolutePath", "File", "file_path"]);
            ("Read".into(), json!({ "file_path": path }), Some(("read_file", path.into())))
        }
        "read_url_content" => {
            let url = first_str(raw_args, &["Url", "url"]);
            ("WebFetch".into(), json!({ "url": url }), Some(("read_url", url.into())))
        }
        "ask_question" => {
            let questions: Vec<Value> = raw_args
                .get("questions")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .map(|q| {
                    let options: Vec<Value> = q
                        .get("options")
                        .and_then(Value::as_array)
                        .into_iter()
                        .flatten()
                        .filter_map(Value::as_str)
                        .map(|label| json!({ "label": label, "description": "" }))
                        .collect();
                    json!({
                        "question": s(q, "question"),
                        "header": "",
                        "multiSelect": q.get("is_multi_select") == Some(&Value::Bool(true)),
                        "options": options,
                    })
                })
                .collect();
            ("AskUserQuestion".into(), json!({ "questions": questions }), None)
        }
        other if other.starts_with("mcp_") => {
            let suffix = other.strip_prefix("mcp_").unwrap_or(other);
            let target = suffix.split_once('_').map_or_else(|| suffix.to_string(), |(srv, tool)| format!("{srv}/{tool}"));
            (other.to_string(), raw_args.clone(), Some(("mcp", target)))
        }
        other => (other.to_string(), raw_args.clone(), None),
    }
}

fn decode_percent(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(hex) = std::str::from_utf8(&bytes[i + 1..i + 3]) {
                if let Ok(val) = u8::from_str_radix(hex, 16) {
                    out.push(val);
                    i += 3;
                    continue;
                }
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn norm_path(p: &str) -> String {
    let decoded = decode_percent(p.trim());
    let stripped = decoded.strip_prefix("file:///").unwrap_or(&decoded);
    let s = stripped.replace('\\', "/").trim_end_matches('/').to_string();
    if cfg!(windows) || s.as_bytes().get(1) == Some(&b':') {
        s.to_ascii_lowercase()
    } else {
        s
    }
}

fn path_in_roots(target: &str, roots: &[String]) -> bool {
    if target.is_empty() || target.contains("..") {
        return false;
    }
    let norm = norm_path(target);
    let is_abs = norm.starts_with('/') || norm.as_bytes().get(1) == Some(&b':');
    if !is_abs {
        return true;
    }
    roots.iter().map(|r| norm_path(r)).any(|r| !r.is_empty() && (norm == r || norm.starts_with(&format!("{r}/"))))
}

fn is_inside_ws_or_artifacts(e: &Value, target: &str) -> bool {
    let mut roots: Vec<String> = e
        .get("workspacePaths")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(String::from)
        .collect();
    for key in ["cwd", "artifactDirectoryPath"] {
        let v = s(e, key);
        if !v.is_empty() {
            roots.push(v.to_string());
        }
    }
    path_in_roots(target, &roots)
}

fn parse_grant(g: &str) -> Option<(&str, &str)> {
    let g = g.trim();
    if let Some(open) = g.find('(') {
        let close = g.strip_suffix(')').map(|_| g.len() - 1)?;
        Some((g[..open].trim(), g[open + 1..close].trim()))
    } else {
        Some((g, "*"))
    }
}

fn grant_matches(grant: &str, action: &str, target: &str) -> bool {
    let Some((g_action, g_target)) = parse_grant(grant) else { return false };
    if g_action != action {
        return false;
    }
    if g_target == "*" {
        return true;
    }
    match action {
        "command" => {
            let t = target.trim();
            if let Some(prefix) = g_target.strip_suffix('*') {
                t.starts_with(prefix.trim_end())
            } else {
                t == g_target
            }
        }
        "write_file" | "read_file" => norm_path(target) == norm_path(g_target) || path_in_roots(target, &[g_target.to_string()]),
        "read_url" => target.contains(g_target),
        _ => target == g_target,
    }
}

fn rules_of<'a>(grants: &'a Value, key: &str) -> Vec<&'a str> {
    grants.get(key).and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_str).collect()
}

// Under Antigravity 2.0 default mode ("Agent Decides"), normal safe workspace
// commands auto-run; only destructive/risky commands or explicit review-mode
// policies require approval on the island. Matches command verbs per segment
// rather than raw substring checks so words like "model" or "--format" do not
// false-positive on "del" or "format".
fn is_risky_command(cmd: &str) -> bool {
    let lower = cmd.to_ascii_lowercase();
    let multi_word = ["git push --force", "git push -f", "git reset --hard", "git clean -f"];
    if multi_word.iter().any(|p| lower.contains(p)) {
        return true;
    }
    let risky_verbs = [
        "rm", "rmdir", "del", "erase", "remove-item", "format",
        "stop-process", "taskkill", "shutdown", "restart-computer", "reg",
        "set-executionpolicy", "invoke-webrequest", "iwr", "curl", "wget",
        "chmod", "chown", "takeown", "icacls",
    ];
    lower.split([';', '|', '&', '\n']).any(|seg| {
        let mut words = seg.split_whitespace();
        let Some(first) = words.next() else { return false };
        let verb = first.rsplit(['/', '\\']).next().unwrap_or(first).trim_end_matches(".exe");
        risky_verbs.contains(&verb)
    })
}

fn config_dir() -> std::path::PathBuf {
    let home = std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME")).unwrap_or_default();
    std::path::PathBuf::from(home).join(".gemini").join("config")
}

fn read_json_file(path: &std::path::Path) -> Value {
    std::fs::read(path).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or(Value::Null)
}

fn load_project_config(dir: &std::path::Path, e: &Value) -> Value {
    let projects_dir = dir.join("projects");
    let Ok(entries) = std::fs::read_dir(projects_dir) else { return Value::Null };
    let mut targets: Vec<String> = e
        .get("workspacePaths")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(norm_path)
        .collect();
    if !s(e, "cwd").is_empty() {
        targets.push(norm_path(s(e, "cwd")));
    }
    for entry in entries.flatten() {
        let p = entry.path();
        if p.extension().and_then(|x| x.to_str()) != Some("json") {
            continue;
        }
        let val = read_json_file(&p);
        let matched = val["projectResources"]["resources"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|r| r["folderUri"].as_str())
            .map(norm_path)
            .any(|uri| targets.iter().any(|t| !uri.is_empty() && t == &uri));
        if matched {
            return val;
        }
    }
    Value::Null
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PreToolDecision {
    /// 交还 Antigravity 原生权限机制处理（极速模式下不拦截、遵循 AGY 全放行）。
    Passthrough,
    /// 效率模式自动放行：输出 auto_approve 跳过客户端与 Dock 弹窗。
    AutoApprove,
    /// 需要上 FISH Dock 人工批准。
    AskDock,
}

pub fn pre_tool_stdout(decision: PreToolDecision) -> Option<&'static str> {
    match decision {
        PreToolDecision::Passthrough => Some(r#"{"decision":"allow"}"#),
        PreToolDecision::AutoApprove => Some(r#"{"decision":"auto_approve"}"#),
        PreToolDecision::AskDock => None,
    }
}

pub fn read_host_efficiency_mode() -> bool {
    let mut candidates: Vec<std::path::PathBuf> = Vec::new();
    if let Some(home) = crate::config::unified_root_env() {
        candidates.push(home.join("host").join("config").join("config.json"));
    }
    if let Some(ud) = crate::config::user_data_override() {
        candidates.push(ud.join("config.json"));
    }
    if let Ok(profile) = crate::storage::profile_directory() {
        candidates.push(profile.join(".fish-dock").join("host").join("config").join("config.json"));
    } else if let Some(home) = std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME")).filter(|v| !v.is_empty()) {
        candidates.push(std::path::PathBuf::from(home).join(".fish-dock").join("host").join("config").join("config.json"));
    }
    for path in candidates {
        let val = read_json_file(&path);
        if let Some(enabled) = val.get("agyEfficiencyMode").and_then(Value::as_bool) {
            return enabled;
        }
        if val.is_object() {
            return true;
        }
    }
    true
}

pub fn evaluate_pre_tool_use(e: &Value, efficiency_mode: bool) -> PreToolDecision {
    let dir = config_dir();
    let cfg = read_json_file(&dir.join("config.json"));
    let proj = load_project_config(&dir, e);
    evaluate_pre_tool_use_with(e, &cfg, &proj, efficiency_mode)
}

pub fn evaluate_pre_tool_use_with(e: &Value, config: &Value, project: &Value, efficiency_mode: bool) -> PreToolDecision {
    if is_question(e) {
        return PreToolDecision::AskDock;
    }
    let call = e.get("toolCall").unwrap_or(&Value::Null);
    let (_, _, Some((action, target))) = classify_tool(s(call, "name"), call.get("args").unwrap_or(&Value::Null)) else {
        return PreToolDecision::Passthrough;
    };
    let global_settings = config.get("userSettings").unwrap_or(&Value::Null);
    let proj_settings = project.get("settings").unwrap_or(&Value::Null);
    let preset = proj_settings.get("permissionPreset").or_else(|| global_settings.get("permissionPreset")).and_then(Value::as_str).unwrap_or("");
    let policy = proj_settings
        .get("autoExecutionPolicy")
        .or_else(|| global_settings.get("autoExecutionPolicy"))
        .and_then(Value::as_str)
        .unwrap_or("");
    // 极速模式（TURBO / EAGER / always-proceed）最高优先级：hook 不拦截，全部透传交由 AGY 全放行。
    if preset == "AGENT_PERMISSION_PRESET_TURBO" || matches!(policy, "CASCADE_COMMANDS_AUTO_EXECUTION_EAGER" | "always-proceed") {
        return PreToolDecision::Passthrough;
    }
    let global_grants = global_settings.get("globalPermissionGrants").unwrap_or(&Value::Null);
    let proj_grants = project.get("permissionGrants").and_then(|g| g.get("permissionGrants").or(Some(g))).unwrap_or(&Value::Null);
    let has_rule = |key: &str| {
        rules_of(proj_grants, key)
            .into_iter()
            .chain(rules_of(global_grants, key))
            .any(|g| grant_matches(g, action, &target))
    };
    if has_rule("deny") {
        return PreToolDecision::Passthrough;
    }
    if has_rule("ask") {
        return PreToolDecision::AskDock;
    }
    if has_rule("allow") {
        return PreToolDecision::Passthrough;
    }
    match action {
        "read_file" | "write_file" => {
            if !is_inside_ws_or_artifacts(e, &target) {
                PreToolDecision::AskDock
            } else if efficiency_mode {
                PreToolDecision::AutoApprove
            } else {
                PreToolDecision::Passthrough
            }
        }
        "read_url" | "mcp" => PreToolDecision::AskDock,
        "command" => {
            if efficiency_mode {
                if is_risky_command(&target) {
                    PreToolDecision::AskDock
                } else {
                    PreToolDecision::AutoApprove
                }
            } else if matches!(
                policy,
                "CASCADE_COMMANDS_AUTO_EXECUTION_OFF"
                    | "CASCADE_COMMANDS_AUTO_EXECUTION_REQUEST_REVIEW"
                    | "request-review"
                    | "strict"
            ) || is_risky_command(&target)
            {
                PreToolDecision::AskDock
            } else {
                PreToolDecision::Passthrough
            }
        }
        _ => PreToolDecision::Passthrough,
    }
}

pub fn needs_approval(e: &Value) -> bool {
    !is_question(e) && evaluate_pre_tool_use(e, read_host_efficiency_mode()) == PreToolDecision::AskDock
}

pub fn needs_approval_with(e: &Value, config: &Value, project: &Value) -> bool {
    !is_question(e) && evaluate_pre_tool_use_with(e, config, project, false) == PreToolDecision::AskDock
}

// Convert an Antigravity PreToolUse event into Claude Code's PermissionRequest
// shape for asks.rs. Intentionally omits `permission_suggestions` so the Dock
// presents only Yes (Allow) and No (Deny).
pub fn for_ask(e: &Value) -> Value {
    let call = e.get("toolCall").unwrap_or(&Value::Null);
    let (tool, input, _) = classify_tool(s(call, "name"), call.get("args").unwrap_or(&Value::Null));
    let session = s(e, "conversationId");
    let cwd = e
        .get("workspacePaths")
        .and_then(Value::as_array)
        .and_then(|a| a.first())
        .and_then(Value::as_str)
        .filter(|p| !p.is_empty())
        .unwrap_or_else(|| s(e, "cwd"));
    let step = e.get("stepIdx").and_then(Value::as_i64).map(|v| v.to_string()).unwrap_or_default();
    json!({
        "hook_event_name": "PermissionRequest",
        "session_id": session,
        "conversationId": session,
        "cwd": cwd,
        "agent": "agy",
        "tool_name": tool,
        "tool_input": input,
        "tool_use_id": format!("{session}-{step}"),
    })
}

pub fn to_output(body: &str, is_q: bool) -> String {
    let Ok(val) = serde_json::from_str::<Value>(body) else {
        return r#"{"decision":"allow"}"#.to_string();
    };
    let Some(decision) = val.get("hookSpecificOutput").and_then(|h| h.get("decision")) else {
        return r#"{"decision":"allow"}"#.to_string();
    };
    match decision.get("behavior").and_then(Value::as_str) {
        Some("allow") if is_q => {
            let lines: Vec<String> = decision["updatedInput"]["answers"]
                .as_object()
                .into_iter()
                .flatten()
                .map(|(q, a)| format!("- {q}: {}", a.as_str().unwrap_or("")))
                .collect();
            if lines.is_empty() {
                return r#"{"decision":"allow"}"#.to_string();
            }
            let reason = format!(
                "用户已在 FISH Dock 中选好了方案（请直接按以下选择继续执行，不要重复调用 ask_question）：\n{}",
                lines.join("\n")
            );
            json!({ "decision": "deny", "reason": reason }).to_string()
        }
        Some("allow") => r#"{"decision":"auto_approve"}"#.to_string(),
        Some("deny") => {
            let msg = decision.get("reason").and_then(Value::as_str).or_else(|| decision.get("message").and_then(Value::as_str)).unwrap_or("The user denied this action on FISH Dock.");
            json!({ "decision": "deny", "reason": msg }).to_string()
        }
        _ => r#"{"decision":"allow"}"#.to_string(),
    }
}

pub fn with_seen(e: &mut Value) {
    let session = s(e, "conversationId").to_string();
    if session.is_empty() {
        return;
    }
    let is_post = s(e, "hook_event_name") == "PostToolUse";
    let step = e.get("stepIdx").and_then(Value::as_i64).map(|v| v.to_string());
    let mapped = is_post.then(|| {
        let call = e.get("toolCall").unwrap_or(&Value::Null);
        classify_tool(s(call, "name"), call.get("args").unwrap_or(&Value::Null))
    });
    if let Some(fields) = e.as_object_mut() {
        fields.entry("session_id".to_string()).or_insert_with(|| json!(session));
        if let Some((tool, input, _)) = mapped {
            fields.entry("tool_name".to_string()).or_insert_with(|| json!(tool));
            fields.entry("tool_input".to_string()).or_insert(input);
            if let Some(step) = step {
                fields.entry("tool_use_id".to_string()).or_insert_with(|| json!(format!("{session}-{step}")));
            }
        }
    }
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
        assert_eq!(pet.get(13_900, true)["mood"], "working");
        assert_eq!(feed(&mut pet, "PostToolUse", at(json!({ "stepIdx": 2, "error": "", "toolCall": { "name": "run_command", "args": { "toolAction": "Running ping" } } })), 20_300), (0, 0));
        write(&[json!({ "step_index": 4, "source": "SYSTEM", "type": "SYSTEM_MESSAGE", "status": "DONE", "content": "the task is done" })]);
        assert_eq!(feed(&mut pet, "PreInvocation", at(json!({ "invocationNum": 0, "initialNumSteps": 4 })), 20_500), (0, 0));
        write(&[json!({ "step_index": 5, "source": "MODEL", "type": "PLANNER_RESPONSE", "status": "DONE", "content": "It finished." })]);
        assert_eq!(feed(&mut pet, "Stop", at(json!({ "executionNum": 0, "fullyIdle": true, "terminationReason": "NO_TOOL_CALL", "error": "" })), 23_600), (0, 23_500));
        assert_eq!((pet.get(23_700, true)["mood"].as_str(), pet.get(23_700, true)["reply"].as_str()), (Some("done"), Some("It finished.")));

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

    #[test]
    fn needs_approval_respects_risk_workspace_and_policies() {
        let ws = ["Z:/nowhere/agyws"];
        let pre = |tool: &str, args: Value| json!({
            "hook_event_name": "PreToolUse",
            "conversationId": "5064aa2e",
            "workspacePaths": ws,
            "cwd": "Z:\\nowhere\\agyws",
            "artifactDirectoryPath": "Z:/nowhere/brain/5064aa2e",
            "stepIdx": 5,
            "toolCall": { "name": tool, "args": args }
        });
        let empty = Value::Null;
        // Safe workspace read/write & safe commands auto-run under default mode.
        assert!(!needs_approval_with(&pre("read_file", json!({ "Path": r"Z:\nowhere\agyws\a.txt" })), &empty, &empty));
        assert!(!needs_approval_with(&pre("write_file", json!({ "Path": r"Z:\nowhere\agyws\b.txt" })), &empty, &empty));
        assert!(!needs_approval_with(&pre("run_command", json!({ "CommandLine": "git status" })), &empty, &empty));
        assert!(!needs_approval_with(&pre("run_command", json!({ "CommandLine": "cargo test -j 1" })), &empty, &empty));
        assert!(!needs_approval_with(&pre("run_command", json!({ "CommandLine": "echo model confirm --format json" })), &empty, &empty));
        // Outside workspace & risky commands & web & mcp require approval.
        assert!(needs_approval_with(&pre("read_file", json!({ "Path": r"C:\outside\secret.txt" })), &empty, &empty));
        assert!(needs_approval_with(&pre("write_file", json!({ "Path": r"C:\outside\evil.txt" })), &empty, &empty));
        assert!(needs_approval_with(&pre("run_command", json!({ "CommandLine": "rm -rf foo" })), &empty, &empty));
        assert!(needs_approval_with(&pre("run_command", json!({ "CommandLine": "echo ok && rm -rf foo" })), &empty, &empty));
        assert!(needs_approval_with(&pre("run_command", json!({ "CommandLine": "Remove-Item -Recurse C:\\tmp" })), &empty, &empty));
        assert!(needs_approval_with(&pre("read_url_content", json!({ "Url": "https://example.com" })), &empty, &empty));
        assert!(needs_approval_with(&pre("mcp_github_get_issue", json!({ "issue_number": 1 })), &empty, &empty));
        // ask_question is routed via is_question, not needs_approval.
        let aq = pre("ask_question", json!({ "questions": [{ "question": "Pick", "options": ["A", "B"], "is_multi_select": false }] }));
        assert!(is_question(&aq));
        assert!(!needs_approval_with(&aq, &empty, &empty));
        // Request review policy forces safe commands to ask; project config overrides global.
        let review_cfg = json!({ "userSettings": { "autoExecutionPolicy": "CASCADE_COMMANDS_AUTO_EXECUTION_REQUEST_REVIEW" } });
        assert!(needs_approval_with(&pre("run_command", json!({ "CommandLine": "git status" })), &review_cfg, &empty));
        let turbo_proj = json!({ "settings": { "permissionPreset": "AGENT_PERMISSION_PRESET_TURBO" } });
        assert!(!needs_approval_with(&pre("run_command", json!({ "CommandLine": "rm -rf foo" })), &review_cfg, &turbo_proj));
    }

    #[test]
    fn for_ask_and_to_output_handle_yes_no_only_and_ask_question() {
        let cmd = event("PreToolUse", json!({
            "stepIdx": 7,
            "workspacePaths": ["Z:/nowhere/agyws"],
            "toolCall": { "name": "run_command", "args": { "CommandLine": "rm -rf tmp", "toolSummary": "Remove tmp" } }
        }));
        let req = for_ask(&cmd);
        assert_eq!(req["hook_event_name"], "PermissionRequest");
        assert_eq!(req["agent"], "agy");
        assert_eq!(req["tool_name"], "Bash");
        assert_eq!(req["tool_input"]["command"], "rm -rf tmp");
        assert!(req.get("permission_suggestions").is_none());

        assert_eq!(to_output(&json!({ "hookSpecificOutput": { "decision": { "behavior": "allow" } } }).to_string(), false), r#"{"decision":"auto_approve"}"#);
        assert_eq!(to_output(&json!({ "hookSpecificOutput": { "decision": { "behavior": "deny", "reason": "no" } } }).to_string(), false), r#"{"decision":"deny","reason":"no"}"#);
        assert_eq!(to_output("", false), r#"{"decision":"allow"}"#);

        let aq = event("PreToolUse", json!({
            "stepIdx": 8,
            "workspacePaths": ["Z:/nowhere/agyws"],
            "toolCall": { "name": "ask_question", "args": { "questions": [{ "question": "选哪个？", "options": ["A", "B"], "is_multi_select": false }] } }
        }));
        let aq_req = for_ask(&aq);
        assert_eq!(aq_req["tool_name"], "AskUserQuestion");
        assert_eq!(aq_req["tool_input"]["questions"][0]["options"][0]["label"], "A");
        assert_eq!(aq_req["tool_input"]["questions"][0]["multiSelect"], false);

        let answered = json!({ "hookSpecificOutput": { "decision": { "behavior": "allow", "updatedInput": { "answers": { "选哪个？": "B" } } } } });
        let out: Value = serde_json::from_str(&to_output(&answered.to_string(), true)).unwrap();
        assert_eq!(out["decision"], "deny");
        assert!(out["reason"].as_str().unwrap().contains("- 选哪个？: B"));
        assert_eq!(to_output("", true), r#"{"decision":"allow"}"#);
    }

    #[test]
    fn with_seen_populates_session_and_tool_use_id() {
        let mut post = event("PostToolUse", json!({ "stepIdx": 7, "toolCall": { "name": "run_command", "args": { "CommandLine": "rm -rf tmp" } } }));
        with_seen(&mut post);
        assert_eq!(post["session_id"], "5064aa2e");
        assert_eq!(post["tool_name"], "Bash");
        assert_eq!(post["tool_use_id"], "5064aa2e-7");
    }

    #[test]
    fn permission_request_becomes_waiting_message_and_uri_spaces_decode() {
        let cmd = event("PreToolUse", json!({
            "stepIdx": 9,
            "workspacePaths": ["H:/FISH Dock"],
            "toolCall": { "name": "run_command", "args": { "CommandLine": "rm -rf tmp" } }
        }));
        let msg = to_message(&for_ask(&cmd)).expect("PermissionRequest should become waiting message");
        assert_eq!(msg["mood"], "waiting");
        assert_eq!(msg["agent"], "agy");
        assert_eq!(norm_path("file:///h%3A/FISH%20Dock"), norm_path(r"H:\FISH Dock"));
    }

    #[test]
    fn efficiency_mode_auto_approves_safe_tools_and_turbo_always_passes_through() {
        let ws = ["Z:/nowhere/agyws"];
        let pre = |tool: &str, args: Value| json!({
            "hook_event_name": "PreToolUse",
            "conversationId": "5064aa2e",
            "workspacePaths": ws,
            "cwd": "Z:\\nowhere\\agyws",
            "artifactDirectoryPath": "Z:/nowhere/brain/5064aa2e",
            "stepIdx": 5,
            "toolCall": { "name": tool, "args": args }
        });
        let empty = Value::Null;
        let review_cfg = json!({ "userSettings": { "autoExecutionPolicy": "CASCADE_COMMANDS_AUTO_EXECUTION_REQUEST_REVIEW" } });
        let turbo_cfg = json!({ "userSettings": { "permissionPreset": "AGENT_PERMISSION_PRESET_TURBO" } });
        let eager_cfg = json!({ "userSettings": { "autoExecutionPolicy": "CASCADE_COMMANDS_AUTO_EXECUTION_EAGER" } });

        // 效率模式开启：请求人工审查 / 默认模式下非高危命令与工作区内读写自动 auto_approve。
        for cfg in [&empty, &review_cfg] {
            let dec_cmd = evaluate_pre_tool_use_with(&pre("run_command", json!({ "CommandLine": "git commit -m test" })), cfg, &empty, true);
            assert_eq!(dec_cmd, PreToolDecision::AutoApprove);
            assert_eq!(pre_tool_stdout(dec_cmd), Some(r#"{"decision":"auto_approve"}"#));
            assert_eq!(evaluate_pre_tool_use_with(&pre("read_file", json!({ "Path": r"Z:\nowhere\agyws\a.txt" })), cfg, &empty, true), PreToolDecision::AutoApprove);
            assert_eq!(evaluate_pre_tool_use_with(&pre("write_file", json!({ "Path": r"Z:\nowhere\agyws\b.txt" })), cfg, &empty, true), PreToolDecision::AutoApprove);
            // 高危命令、跨工作区、网络、MCP 仍走 Dock 批准。
            assert_eq!(evaluate_pre_tool_use_with(&pre("run_command", json!({ "CommandLine": "rm -rf foo" })), cfg, &empty, true), PreToolDecision::AskDock);
            assert_eq!(evaluate_pre_tool_use_with(&pre("run_command", json!({ "CommandLine": "icacls foo" })), cfg, &empty, true), PreToolDecision::AskDock);
            assert_eq!(evaluate_pre_tool_use_with(&pre("read_file", json!({ "Path": r"D:\outside\secret.txt" })), cfg, &empty, true), PreToolDecision::AskDock);
            assert_eq!(evaluate_pre_tool_use_with(&pre("read_url_content", json!({ "Url": "https://example.com" })), cfg, &empty, true), PreToolDecision::AskDock);
            assert_eq!(evaluate_pre_tool_use_with(&pre("mcp_github_get_issue", json!({ "issue_number": 1 })), cfg, &empty, true), PreToolDecision::AskDock);
        }

        // 极速模式（TURBO / EAGER）：无论效率模式是否开启，一律透传放行，交由 AGY 全放行。
        for cfg in [&turbo_cfg, &eager_cfg] {
            for eff in [true, false] {
                for call in [
                    pre("run_command", json!({ "CommandLine": "rm -rf foo" })),
                    pre("run_command", json!({ "CommandLine": "icacls C:\\secret" })),
                    pre("read_file", json!({ "Path": r"D:\outside\secret.txt" })),
                    pre("write_file", json!({ "Path": r"D:\outside\evil.txt" })),
                    pre("read_url_content", json!({ "Url": "https://example.com" })),
                    pre("mcp_github_get_issue", json!({ "issue_number": 1 })),
                ] {
                    let dec = evaluate_pre_tool_use_with(&call, cfg, &empty, eff);
                    assert_eq!(dec, PreToolDecision::Passthrough);
                    assert_eq!(pre_tool_stdout(dec), Some(r#"{"decision":"allow"}"#));
                }
            }
        }

        // 关闭效率模式：REQUEST_REVIEW 下普通命令交由 Dock 询问，空配置则退回 Passthrough。
        assert_eq!(evaluate_pre_tool_use_with(&pre("run_command", json!({ "CommandLine": "git status" })), &review_cfg, &empty, false), PreToolDecision::AskDock);
        assert_eq!(evaluate_pre_tool_use_with(&pre("run_command", json!({ "CommandLine": "git status" })), &empty, &empty, false), PreToolDecision::Passthrough);
    }
}
