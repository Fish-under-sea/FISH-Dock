// Permission prompts and questions waiting on the person, answered on the
// island or above her: what to show for a PermissionRequest hook event, the
// hook output for each choice, and the prompts waiting.
//
// Each ask holds the hook's HTTP request open: finishing it sends the hook
// output. Claude Code shows its own dialog meanwhile and keeps the first
// answer, so an ask also goes away (answered {}, no decision) once the
// session shows the dialog is gone: the same tool call finishing, the turn
// ending, or a newer prompt from the same agent. And before Claude Code's own
// timeout.
//
// Codex's prompts come the same way (events_codex.rs), but Codex shows its
// own dialog only once the hook has answered with no decision, and takes
// only allow or deny: no rules to add, so never "always". ZCode's too
// (events_zcode.rs): its dialog shows at once, as Claude Code's does, and
// it suggests no rules either.
//
// OpenCode's too, from her plugin (events_opencode.rs): OpenCode asks the
// person in its own window all the while, and the plugin gives her answer
// to OpenCode as its reply, { decision: once | always | reject }; "always"
// allows what the permission's save patterns say, in this project. Answered
// in OpenCode first, PermissionReplied with its id closes hers.
//
// DeepSeek Harness's too, from her plugin (events_dsh.rs for_ask, in
// Claude Code's words): its approvals, allow or deny ({ decision: allow |
// deny }: DSH has only one-shot grants, so never "always"), with DSH's own
// reason for asking (reason: its words in both languages, or its audit
// text); and its questions, answered as DSH takes them, one { id, selected,
// custom? } for each question by its id. DSH shows its own dialog all the
// while; answered there first, PermissionReplied with its id closes hers,
// and hers first withdraws DSH's (the plugin does).
use std::path::Path;

use serde_json::{json, Map, Value};

use crate::i18n;

const MAX_SUMMARY: usize = 400;
const MAX_ANSWER: usize = 2000;
// The longest a prompt can wait: under the 300 s timeout the PermissionRequest
// hook is installed with. The settings can make it shorter.
pub const ASK_TIMEOUT_MS: u64 = 290 * 1000;

fn clip(text: &str, max: usize) -> String {
    if text.chars().count() > max {
        format!("{}…", text.chars().take(max).collect::<String>())
    } else {
        text.to_string()
    }
}

fn s<'a>(v: &'a Value, key: &str) -> &'a str {
    v.get(key).and_then(Value::as_str).unwrap_or("")
}

// A patch (Codex's apply_patch) as the files it changes, one a line.
fn patch_files(patch: &str) -> Option<String> {
    let files: Vec<&str> = patch
        .lines()
        .filter_map(|line| line.strip_prefix("*** "))
        .filter(|line| ["Add File: ", "Update File: ", "Delete File: "].iter().any(|p| line.starts_with(p)))
        .collect();
    (!files.is_empty()).then(|| files.join("\n"))
}

// What a tool call does, in one short text.
pub fn summarize(tool: &str, input: &Value) -> String {
    if tool == "apply_patch" {
        if let Some(files) = input.get("command").and_then(Value::as_str).and_then(patch_files) {
            return clip(&files, MAX_SUMMARY);
        }
    }
    let field = match tool {
        "Bash" | "PowerShell" | "apply_patch" => Some("command"),
        "Edit" | "MultiEdit" | "Write" | "Read" | "NotebookEdit" => {
            Some(if input.get("file_path").is_some() { "file_path" } else { "notebook_path" })
        }
        "WebFetch" => Some("url"),
        "WebSearch" => Some("query"),
        "Glob" | "Grep" => Some("pattern"),
        _ => None,
    };
    let text = match field.and_then(|f| input.get(f)).and_then(Value::as_str) {
        Some(text) => text.to_string(),
        None => if input.is_object() { input.to_string() } else { "{}".into() },
    };
    clip(&text, MAX_SUMMARY)
}

// Only suggestions that add an allowance; never one that removes or denies.
fn allow_suggestions(list: Option<&Value>) -> Vec<Value> {
    let Some(Value::Array(list)) = list else { return Vec::new() };
    list.iter()
        .filter(|sug| {
            let kind = s(sug, "type");
            (kind == "addRules" && s(sug, "behavior") == "allow") || kind == "addDirectories" || (kind == "setMode" && s(sug, "mode") == "acceptEdits")
        })
        .cloned()
        .collect()
}

// What an "always allow" would add, as data the page words in its language.
fn describe_always(list: &[Value]) -> Value {
    let (mut rules, mut dirs, mut modes, mut wheres) = (Vec::new(), Vec::new(), Vec::new(), Vec::<String>::new());
    for sug in list {
        match s(sug, "type") {
            "addRules" => {
                for r in sug.get("rules").and_then(Value::as_array).into_iter().flatten() {
                    let tool = s(r, "toolName");
                    let content = s(r, "ruleContent");
                    rules.push(if content.is_empty() { tool.to_string() } else { format!("{tool}({content})") });
                }
            }
            "addDirectories" => dirs.extend(sug.get("directories").and_then(Value::as_array).into_iter().flatten().cloned()),
            "setMode" => modes.push(json!(s(sug, "mode"))),
            _ => {}
        }
        let dest = s(sug, "destination");
        if !dest.is_empty() && !wheres.iter().any(|w| w == dest) {
            wheres.push(dest.to_string());
        }
    }
    json!({ "rules": rules, "dirs": dirs, "modes": modes, "where": wheres })
}

// How a question is answered: pick options (choice, the default), type a text, or a number.
fn kind_of(q: &Value) -> &'static str {
    match s(q, "kind") {
        "text" => "text",
        "number" => "number",
        _ => "choice",
    }
}

fn is_answerable(q: &Value) -> bool {
    match kind_of(q) {
        "choice" => q.get("options").and_then(Value::as_array).is_some_and(|o| !o.is_empty()),
        "number" => match (q.get("min").and_then(Value::as_f64), q.get("max").and_then(Value::as_f64)) {
            (Some(min), Some(max)) => min <= max,
            _ => false,
        },
        _ => true,
    }
}

fn question_view(q: &Value) -> Value {
    let kind = kind_of(q);
    let options: Vec<Value> = if kind == "choice" {
        q.get("options")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .map(|o| json!({ "label": s(o, "label"), "description": s(o, "description") }))
            .collect()
    } else {
        Vec::new()
    };
    let mut view = json!({
        "kind": kind,
        "header": s(q, "header"),
        "question": s(q, "question"),
        "description": s(q, "description"),
        "multiSelect": kind == "choice" && q.get("multiSelect") == Some(&Value::Bool(true)),
        "options": options,
    });
    if kind == "text" {
        view["placeholder"] = json!(s(q, "placeholder"));
    }
    if kind == "number" {
        let min = q.get("min").cloned().unwrap_or(Value::Null);
        view["min"] = min.clone();
        view["max"] = q.get("max").cloned().unwrap_or(Value::Null);
        view["step"] = q.get("step").cloned().unwrap_or(json!(1));
        view["defaultValue"] = q.get("defaultValue").cloned().unwrap_or(min);
        view["unit"] = json!(s(q, "unit"));
    }
    view
}

fn input_of(e: &Value) -> Value {
    match e.get("tool_input") {
        Some(v @ Value::Object(_)) => v.clone(),
        _ => json!({}),
    }
}

// The view of a PermissionRequest event, or None when it is not one.
pub fn view_of(e: &Value) -> Option<Value> {
    if s(e, "hook_event_name") != "PermissionRequest" {
        return None;
    }
    let tool = e.get("tool_name")?.as_str()?;
    let input = input_of(e);
    let cwd = s(e, "cwd");
    let project = if cwd.is_empty() { String::new() } else { Path::new(cwd).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default() };
    // agent: the subagent asking (Claude Code, OpenCode); from: "codex",
    // "opencode", or "" for Claude Code.
    let mut view = json!({ "session": s(e, "session_id"), "agent": s(e, "agent_id"), "from": s(e, "agent"), "project": project, "tool": tool });

    if tool == "AskUserQuestion" {
        let questions: Vec<Value> = input.get("questions").and_then(Value::as_array).cloned().unwrap_or_default();
        view["kind"] = json!("question");
        view["title"] = json!(s(&input, "title"));
        view["canAnswer"] = json!(!questions.is_empty() && questions.iter().all(is_answerable));
        view["questions"] = Value::Array(questions.iter().map(question_view).collect());
    } else if tool == "ExitPlanMode" {
        view["kind"] = json!("plan");
        view["summary"] = json!(clip(s(&input, "plan"), MAX_SUMMARY));
    } else if is_opencode(e) {
        // What OpenCode asks about (the commands, the files), and what
        // "always" would allow from now on in this project.
        let resources: Vec<&str> = e.get("resources").and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_str).collect();
        let save = opencode_save(e);
        view["kind"] = json!("permission");
        view["summary"] = json!(if resources.is_empty() { summarize(tool, &input) } else { clip(&resources.join("\n"), MAX_SUMMARY) });
        view["always"] = if save.is_empty() {
            Value::Null
        } else {
            json!({ "rules": save.iter().map(|p| format!("{tool}({p})")).collect::<Vec<_>>(), "dirs": [], "modes": [], "where": ["projectSettings"] })
        };
    } else {
        let always = allow_suggestions(e.get("permission_suggestions"));
        view["kind"] = json!("permission");
        view["summary"] = json!(summarize(tool, &input));
        view["always"] = if always.is_empty() { Value::Null } else { describe_always(&always) };
        // Why DSH asks (a sandbox to widen, and the model's words for it):
        // in both languages, else its audit text.
        if is_dsh(e) {
            let shown = e.get("display_reason").filter(|r| r.is_object()).map(|r| json!({ "en": clip(s(r, "en"), MAX_SUMMARY), "zh": clip(s(r, "zh"), MAX_SUMMARY) }));
            view["reason"] = shown.unwrap_or_else(|| if s(e, "reason").is_empty() { Value::Null } else { json!(clip(s(e, "reason"), MAX_SUMMARY)) });
        }
    }
    Some(view)
}

fn is_opencode(e: &Value) -> bool {
    s(e, "agent") == "opencode"
}

fn is_dsh(e: &Value) -> bool {
    s(e, "agent") == "dsh"
}

// Prompts that wait side by side, each open till answered or closed by its
// own id: OpenCode's, DSH's (they ask for calls running together).
fn is_side_by_side(e: &Value) -> bool {
    is_opencode(e) || is_dsh(e)
}

// DSH's answer to one question, as it takes it back: the labels picked, or
// a text typed in their place.
fn dsh_answer(view: &Value, asked: &Value, picked: &Value) -> Option<Value> {
    answer_text(view, picked)?;
    let id = s(asked, "id");
    Some(match picked.as_array() {
        Some(labels) => json!({ "id": id, "selected": labels }),
        None => json!({ "id": id, "selected": [], "custom": answer_text(view, picked)? }),
    })
}

// The patterns an OpenCode permission's "always" saves.
fn opencode_save(e: &Value) -> Vec<&str> {
    e.get("save").and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_str).filter(|p| !p.is_empty()).collect()
}

fn output(decision: Value) -> Value {
    json!({ "hookSpecificOutput": { "hookEventName": "PermissionRequest", "decision": decision } })
}

// One question's answer as Claude Code takes it, or None when it does not fit.
//   choice   ['label', ...] picked options, or { text } typed instead ("Other")
//   text     { text }
//   number   { number }
fn answer_text(q: &Value, picked: &Value) -> Option<String> {
    let kind = s(q, "kind");
    if let Some(text) = picked.as_object().and_then(|o| o.get("text")) {
        if kind == "number" {
            return None;
        }
        let text = match text {
            Value::String(t) => t.trim().to_string(),
            Value::Null => String::new(),
            other => other.to_string(),
        };
        return (!text.is_empty() && text.chars().count() <= MAX_ANSWER).then_some(text);
    }
    if kind == "number" {
        let n = picked.get("number").and_then(|n| n.as_f64().or_else(|| n.as_str().and_then(|t| t.parse().ok())))?;
        let (min, max) = (q.get("min")?.as_f64()?, q.get("max")?.as_f64()?);
        return (n.is_finite() && n >= min && n <= max).then(|| if n.fract() == 0.0 { format!("{}", n as i64) } else { n.to_string() });
    }
    let picked = picked.as_array().filter(|p| !p.is_empty())?;
    let labels: Vec<&str> = q.get("options")?.as_array()?.iter().map(|o| s(o, "label")).collect();
    let picked: Vec<&str> = picked.iter().map(|p| p.as_str()).collect::<Option<_>>()?;
    if kind != "choice" || !picked.iter().all(|p| labels.contains(p)) {
        return None;
    }
    if q.get("multiSelect") != Some(&Value::Bool(true)) && picked.len() != 1 {
        return None;
    }
    Some(picked.join(", "))
}

// The hook output for a choice, or None when the choice does not fit the event.
//   { action: 'allow' }                  allow once
//   { action: 'always' }                 allow, adding the suggested rules
//   { action: 'deny' }                   deny
//   { action: 'answer', answers: [...] } AskUserQuestion, one answer per question
pub fn reply_for(e: &Value, choice: &Value, lang: &str) -> Option<Value> {
    let view = view_of(e)?;
    let input = input_of(e);
    let kind = s(&view, "kind");
    // OpenCode's reply, which her plugin hands it.
    if is_opencode(e) {
        let decision = match s(choice, "action") {
            "allow" => "once",
            "always" if !opencode_save(e).is_empty() => "always",
            "deny" => "reject",
            _ => return None,
        };
        return Some(json!({ "decision": decision }));
    }
    // DSH's, which her plugin hands it: allow once or deny; its questions'
    // answers by their ids.
    if is_dsh(e) {
        return match s(choice, "action") {
            "allow" if kind == "permission" => Some(json!({ "decision": "allow" })),
            "deny" if kind == "permission" => Some(json!({ "decision": "deny" })),
            "answer" if kind == "question" && view["canAnswer"] == Value::Bool(true) => {
                let given = choice.get("answers")?.as_array()?;
                let views = view["questions"].as_array()?;
                let asked = input.get("questions")?.as_array()?;
                if given.len() != views.len() || asked.len() != views.len() {
                    return None;
                }
                let answers = views.iter().zip(asked).zip(given).map(|((v, q), picked)| dsh_answer(v, q, picked)).collect::<Option<Vec<_>>>()?;
                Some(json!({ "answers": answers }))
            }
            _ => None,
        };
    }
    match s(choice, "action") {
        "deny" => Some(output(json!({ "behavior": "deny", "message": i18n::t(lang, "deny.message") }))),
        "allow" if kind == "question" => None,
        // A tool that needs the person (ExitPlanMode) only takes an allow that
        // carries its input back.
        "allow" if kind == "plan" => Some(output(json!({ "behavior": "allow", "updatedInput": input }))),
        "allow" => Some(output(json!({ "behavior": "allow" }))),
        "always" => {
            let rules = allow_suggestions(e.get("permission_suggestions"));
            (kind == "permission" && !rules.is_empty()).then(|| output(json!({ "behavior": "allow", "updatedPermissions": rules })))
        }
        "answer" => {
            if kind != "question" || view["canAnswer"] != Value::Bool(true) {
                return None;
            }
            let given = choice.get("answers")?.as_array()?;
            let questions = view["questions"].as_array()?;
            if given.len() != questions.len() {
                return None;
            }
            let mut answers = Map::new();
            for (q, picked) in questions.iter().zip(given) {
                answers.insert(s(q, "question").to_string(), json!(answer_text(q, picked)?));
            }
            let mut input = input;
            input["answers"] = Value::Object(answers);
            Some(output(json!({ "behavior": "allow", "updatedInput": input })))
        }
        _ => None,
    }
}

// --- The prompts waiting --------------------------------------------------------------

pub struct Ask<R> {
    pub id: u64,
    event: Value,
    view: Value,
    // Answers the hook: Some(output), or None to just drop it (Claude Code gave up).
    respond: R,
    deadline: u64,
}

// R: whatever answers the hook (the held HTTP request).
pub struct Asks<R: FnOnce(Option<Value>)> {
    next_id: u64,
    list: Vec<Ask<R>>,
    // Messages for the sessions whose prompt went back to the app's own
    // window, till taken (take_handed).
    handed: Vec<Value>,
}

impl<R: FnOnce(Option<Value>)> Default for Asks<R> {
    fn default() -> Self {
        Asks { next_id: 1, list: Vec::new(), handed: Vec::new() }
    }
}

// A prompt of DSH's handed back to its own window (her time up, "去
// DeepSeek Harness 处理", out of sight): its session still waits, and says
// where to answer now ("在 DeepSeek Harness 里批准：写 note.txt", "在 DeepSeek
// Harness 里回答：…"), only within its turn still under way. None for
// another agent's (Claude Code's keeps its words).
fn handed_back<R>(ask: &Ask<R>) -> Option<Value> {
    if !is_dsh(&ask.event) {
        return None;
    }
    let app = "DeepSeek Harness";
    let detail = if s(&ask.view, "kind") == "question" {
        let question = ask.view["questions"][0]["question"].as_str().unwrap_or("");
        json!({ "key": "detail.answerIn", "vars": { "app": app, "what": crate::events::clip(question, 40) } })
    } else {
        json!({ "key": "detail.approveIn", "vars": { "app": app, "what": crate::events::step_of(s(&ask.event, "tool_name"), &input_of(&ask.event)) } })
    };
    Some(json!({ "session": ask.view["session"], "agent": "dsh", "mood": "waiting", "detail": detail, "within": true }))
}

// Events that mean the session has moved past any dialog it showed (Interrupt: Codex's).
const MOVED_ON: [&str; 5] = ["UserPromptSubmit", "Stop", "StopFailure", "SessionEnd", "Interrupt"];

fn same_input(a: Option<&Value>, b: Option<&Value>) -> bool {
    let strip = |v: Option<&Value>| match v {
        Some(Value::Object(o)) => {
            let mut o = o.clone();
            o.remove("answers");
            Value::Object(o)
        }
        Some(v) => v.clone(),
        None => Value::Null,
    };
    strip(a) == strip(b)
}

impl<R: FnOnce(Option<Value>)> Asks<R> {
    fn finish(&mut self, index: usize, reply: Option<Value>) {
        let ask = self.list.remove(index);
        (ask.respond)(reply);
    }

    // A new ask, waiting wait_ms at most; Err(respond) when the event is not
    // one that can be shown (answer it yourself). A newer prompt from the
    // same agent means the last one closed (but OpenCode's and DSH's: they
    // ask for calls side by side, each open till answered).
    pub fn add(&mut self, event: Value, respond: R, now: u64, wait_ms: u64) -> Result<u64, R> {
        let Some(view) = view_of(&event) else { return Err(respond) };
        while let Some(i) = self.list.iter().position(|a| !is_side_by_side(&event) && a.view["session"] == view["session"] && a.view["agent"] == view["agent"]) {
            self.finish(i, Some(json!({})));
        }
        let id = self.next_id;
        self.next_id += 1;
        self.list.push(Ask { id, event, view, respond, deadline: now + wait_ms.min(ASK_TIMEOUT_MS) });
        Ok(id)
    }

    // The person's choice; false when there is no such ask or the choice does not fit it.
    pub fn answer(&mut self, id: u64, choice: &Value, lang: &str) -> bool {
        let Some(i) = self.list.iter().position(|a| a.id == id) else { return false };
        let Some(reply) = reply_for(&self.list[i].event, choice, lang) else { return false };
        self.finish(i, Some(reply));
        true
    }

    // Give it back to the terminal or the app's own window, the session told
    // so where that changes its words (handed_back).
    fn hand_back(&mut self, index: usize) {
        if let Some(message) = handed_back(&self.list[index]) {
            self.handed.push(message);
        }
        self.finish(index, Some(json!({})));
    }

    // Leave it to the terminal.
    pub fn dismiss(&mut self, id: u64) -> bool {
        let Some(i) = self.list.iter().position(|a| a.id == id) else { return false };
        self.hand_back(i);
        true
    }

    // Hand every prompt to the terminal (do not disturb, out of sight).
    // How many there were.
    pub fn dismiss_all(&mut self) -> usize {
        let n = self.list.len();
        while !self.list.is_empty() {
            self.hand_back(0);
        }
        n
    }

    // The messages for the sessions whose prompt was handed back, since last taken.
    pub fn take_handed(&mut self) -> Vec<Value> {
        std::mem::take(&mut self.handed)
    }

    // Any other hook event: drop the asks it shows were settled in the
    // terminal. Which ones (for the log).
    pub fn seen(&mut self, e: &Value) -> Vec<u64> {
        let session = s(e, "session_id");
        if session.is_empty() {
            return Vec::new();
        }
        let name = s(e, "hook_event_name");
        let mut settled = Vec::new();
        while let Some(i) = self.list.iter().position(|a| {
            a.view["session"] == session && {
                let is_done = name == "PostToolUse" || name == "PostToolUseFailure";
                let is_same_call = is_done
                    && s(e, "agent_id") == s(&a.view, "agent")
                    && s(e, "tool_name") == s(&a.view, "tool")
                    && same_input(e.get("tool_input"), a.event.get("tool_input"));
                // OpenCode's and DSH's: its call by id, and the permission
                // answered by id (DSH's question too).
                let is_same_id = |key: &str| !s(e, key).is_empty() && s(e, key) == s(&a.event, key);
                let is_answered = name == "PermissionReplied" && is_same_id("permission_id");
                is_same_call || (is_done && is_same_id("tool_use_id")) || is_answered || MOVED_ON.contains(&name)
            }
        }) {
            settled.push(self.list[i].id);
            self.finish(i, Some(json!({})));
        }
        settled
    }

    // Those whose time is up go to the terminal. Which ones (for the log).
    pub fn expire(&mut self, now: u64) -> Vec<u64> {
        let mut gone = Vec::new();
        while let Some(i) = self.list.iter().position(|a| a.deadline <= now) {
            gone.push(self.list[i].id);
            self.hand_back(i);
        }
        gone
    }

    pub fn views(&self) -> Value {
        Value::Array(
            self.list
                .iter()
                .map(|a| {
                    let mut v = a.view.clone();
                    v["id"] = json!(a.id);
                    v
                })
                .collect(),
        )
    }

    pub fn is_empty(&self) -> bool {
        self.list.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::rc::Rc;

    fn permission() -> Value {
        json!({
            "hook_event_name": "PermissionRequest", "session_id": "s", "cwd": "D:\\work\\pet", "tool_name": "Bash",
            "tool_input": { "command": "npm test" },
            "permission_suggestions": [
                { "type": "addRules", "behavior": "allow", "rules": [{ "toolName": "Bash", "ruleContent": "npm test:*" }], "destination": "localSettings" },
                { "type": "addRules", "behavior": "deny", "rules": [{ "toolName": "Bash" }], "destination": "localSettings" }
            ]
        })
    }

    #[test]
    fn a_permission_prompt_and_its_answers() {
        let view = view_of(&permission()).unwrap();
        assert_eq!(view["kind"], "permission");
        // The project is the last part of the cwd, D:\work\pet: a Windows path
        // (elsewhere a backslash is no separator).
        #[cfg(windows)]
        assert_eq!(view["project"], "pet");
        assert_eq!(view["summary"], "npm test");
        assert_eq!(view["always"]["rules"], json!(["Bash(npm test:*)"]));
        assert_eq!(view["always"]["where"], json!(["localSettings"]));

        let allow = reply_for(&permission(), &json!({ "action": "allow" }), "en").unwrap();
        assert_eq!(allow["hookSpecificOutput"]["decision"], json!({ "behavior": "allow" }));
        let always = reply_for(&permission(), &json!({ "action": "always" }), "en").unwrap();
        assert_eq!(always["hookSpecificOutput"]["decision"]["updatedPermissions"].as_array().unwrap().len(), 1);
        let deny = reply_for(&permission(), &json!({ "action": "deny" }), "zh").unwrap();
        assert_eq!(deny["hookSpecificOutput"]["decision"]["behavior"], "deny");
        assert!(reply_for(&permission(), &json!({ "action": "answer", "answers": [] }), "en").is_none());
    }

    #[test]
    fn questions_are_answered_by_their_text() {
        let e = json!({
            "hook_event_name": "PermissionRequest", "tool_name": "AskUserQuestion",
            "tool_input": { "questions": [
                { "question": "Which?", "options": [{ "label": "A" }, { "label": "B" }] },
                { "question": "How many?", "kind": "number", "min": 1, "max": 5 },
                { "question": "Why?", "kind": "text" }
            ] }
        });
        let view = view_of(&e).unwrap();
        assert_eq!(view["canAnswer"], true);
        let reply = reply_for(&e, &json!({ "action": "answer", "answers": [["B"], { "number": 3 }, { "text": " because " }] }), "en").unwrap();
        assert_eq!(
            reply["hookSpecificOutput"]["decision"]["updatedInput"]["answers"],
            json!({ "Which?": "B", "How many?": "3", "Why?": "because" })
        );
        assert!(reply_for(&e, &json!({ "action": "answer", "answers": [["C"], { "number": 3 }, { "text": "x" }] }), "en").is_none());
        assert!(reply_for(&e, &json!({ "action": "answer", "answers": [["A"], { "number": 9 }, { "text": "x" }] }), "en").is_none());
    }

    #[test]
    fn asks_close_when_the_terminal_moves_on() {
        let got = Rc::new(RefCell::new(Vec::new()));
        let mut asks: Asks<Box<dyn FnOnce(Option<Value>)>> = Asks::default();
        let reply = |got: &Rc<RefCell<Vec<Option<Value>>>>| {
            let got = got.clone();
            Box::new(move |v: Option<Value>| got.borrow_mut().push(v)) as Box<dyn FnOnce(Option<Value>)>
        };
        let id = asks.add(permission(), reply(&got), 0, 1000).ok().unwrap();
        assert_eq!(asks.views()[0]["id"], id);
        // The same call finishing: answered in the terminal.
        assert_eq!(asks.seen(&json!({ "hook_event_name": "PostToolUse", "session_id": "s", "tool_name": "Bash", "tool_input": { "command": "npm test" } })), vec![id]);
        assert_eq!(got.borrow().as_slice(), &[Some(json!({}))]);

        let id = asks.add(permission(), reply(&got), 0, 1000).ok().unwrap();
        assert!(asks.expire(999).is_empty());
        // Its time up: which one went, for the log.
        assert_eq!(asks.expire(1000), vec![id]);
        assert!(asks.is_empty());
        assert!(asks.add(json!({ "hook_event_name": "Stop" }), reply(&got), 0, 1000).is_err());
    }

    // As Codex sends it, tagged by the server.
    fn codex_permission(tool: &str, command: &str) -> Value {
        json!({
            "hook_event_name": "PermissionRequest", "session_id": "c", "turn_id": "t", "cwd": "D:\\work\\pet", "agent": "codex",
            "tool_name": tool, "tool_input": { "command": command, "description": null }
        })
    }

    #[test]
    fn a_codex_prompt_is_allow_or_deny() {
        let e = codex_permission("Bash", "cargo test");
        let view = view_of(&e).unwrap();
        assert_eq!((view["kind"].as_str(), view["from"].as_str(), view["summary"].as_str()), (Some("permission"), Some("codex"), Some("cargo test")));
        assert_eq!(view["always"], Value::Null);
        // The same output Codex reads: hookSpecificOutput.decision.behavior.
        let allow = reply_for(&e, &json!({ "action": "allow" }), "en").unwrap();
        assert_eq!(allow, json!({ "hookSpecificOutput": { "hookEventName": "PermissionRequest", "decision": { "behavior": "allow" } } }));
        assert_eq!(reply_for(&e, &json!({ "action": "deny" }), "en").unwrap()["hookSpecificOutput"]["decision"]["behavior"], "deny");
        assert!(reply_for(&e, &json!({ "action": "always" }), "en").is_none());

        let patch = "*** Begin Patch\n*** Add File: note.txt\n+x\n*** Update File: src/a.rs\n@@\n-a\n+b\n*** End Patch";
        assert_eq!(view_of(&codex_permission("apply_patch", patch)).unwrap()["summary"], "Add File: note.txt\nUpdate File: src/a.rs");
    }

    // As events_opencode.rs makes it of her plugin's, tagged by the server.
    fn opencode_permission(id: &str, save: Value) -> Value {
        let raw = json!({
            "type": "permission.asked", "id": id, "sessionID": "ses_1", "directory": "D:\\work\\oc3", "action": "shell",
            "resources": ["echo hi"], "save": save, "source": { "type": "tool", "id": format!("call_{id}") }, "tool": "shell", "input": { "command": "echo hi" }
        });
        let mut e = crate::events_opencode::normalize(&raw);
        e["agent"] = json!("opencode");
        e
    }

    #[test]
    fn an_opencode_permission_is_once_always_or_reject() {
        let e = opencode_permission("per_1", json!(["echo *"]));
        let view = view_of(&e).unwrap();
        assert_eq!((view["kind"].as_str(), view["from"].as_str(), view["tool"].as_str(), view["summary"].as_str()), (Some("permission"), Some("opencode"), Some("shell"), Some("echo hi")));
        assert_eq!(view["always"], json!({ "rules": ["shell(echo *)"], "dirs": [], "modes": [], "where": ["projectSettings"] }));
        assert_eq!(reply_for(&e, &json!({ "action": "allow" }), "en"), Some(json!({ "decision": "once" })));
        assert_eq!(reply_for(&e, &json!({ "action": "always" }), "en"), Some(json!({ "decision": "always" })));
        assert_eq!(reply_for(&e, &json!({ "action": "deny" }), "zh"), Some(json!({ "decision": "reject" })));
        assert!(reply_for(&e, &json!({ "action": "answer", "answers": [] }), "en").is_none());
        // Nothing to save: no "always".
        let e = opencode_permission("per_2", json!([]));
        assert_eq!(view_of(&e).unwrap()["always"], Value::Null);
        assert!(reply_for(&e, &json!({ "action": "always" }), "en").is_none());
    }

    #[test]
    fn opencode_permissions_wait_side_by_side_till_each_is_answered() {
        let got = Rc::new(RefCell::new(Vec::new()));
        let mut asks: Asks<Box<dyn FnOnce(Option<Value>)>> = Asks::default();
        let reply = |got: &Rc<RefCell<Vec<Option<Value>>>>| {
            let got = got.clone();
            Box::new(move |v: Option<Value>| got.borrow_mut().push(v)) as Box<dyn FnOnce(Option<Value>)>
        };
        let one = asks.add(opencode_permission("per_1", json!(["echo *"])), reply(&got), 0, 1000).ok().unwrap();
        let two = asks.add(opencode_permission("per_2", json!(["echo *"])), reply(&got), 0, 1000).ok().unwrap();
        assert_eq!(asks.views().as_array().unwrap().len(), 2);
        // The second answered in OpenCode: that one goes, the first stays.
        let replied = |id: &str| {
            let mut e = crate::events_opencode::normalize(&json!({ "type": "permission.replied", "sessionID": "ses_1", "requestID": id, "reply": "once" }));
            e["agent"] = json!("opencode");
            e
        };
        assert_eq!(asks.seen(&replied("per_2")), vec![two]);
        assert_eq!(asks.seen(&replied("per_9")), Vec::<u64>::new());
        // Its call done (by its id): the first goes too.
        let mut done = crate::events_opencode::normalize(&json!({ "type": "session.tool.success", "sessionID": "ses_1", "id": "call_per_1", "tool": "shell", "input": { "command": "echo hi" } }));
        done["agent"] = json!("opencode");
        assert_eq!(asks.seen(&done), vec![one]);
        assert_eq!(got.borrow().as_slice(), &[Some(json!({})), Some(json!({}))]);
    }

    // As her DSH plugin sends them (integrations/dsh/wakuwaku.js), tagged by
    // the server and put in Claude Code's words (events_dsh.rs for_ask): a
    // sandbox escalation of DSH 0.2.1-rc.2's, as the test machine had it.
    fn dsh_permission(id: &str, call: &str) -> Value {
        crate::events_dsh::for_ask(&json!({
            "hook_event_name": "PermissionRequest", "session_id": "session-ed6a", "cwd": "C:\\w\\default-workspace", "agent": "dsh",
            "tool_name": "write", "tool_input": { "file_path": "C:\\waku\\agents\\probe1.txt", "content": "hi", "sandbox_permissions": "danger-full-access", "justification": "outside the workspace" },
            "tool_use_id": call, "permission_id": id, "answerable": true,
            "reason": "escalate sandbox to danger-full-access: outside the workspace",
            "display_reason": { "en": "Allow this operation with danger-full-access permissions: outside the workspace", "zh": "允许本次操作使用 danger-full-access 权限：outside the workspace" }
        }))
    }

    fn dsh_question() -> Value {
        crate::events_dsh::for_ask(&json!({
            "hook_event_name": "PermissionRequest", "session_id": "session-q", "cwd": "C:\\w\\default-workspace", "agent": "dsh",
            "tool_name": "ask_user_question", "tool_use_id": "call_q", "permission_id": "9-2", "answerable": true,
            "tool_input": { "questions": [
                { "id": "name", "header": "File Name", "question": "test1 or test2?", "options": [{ "label": "test1", "description": "one" }, { "label": "test2" }] },
                { "id": "both", "question": "Which parts?", "multiSelect": true, "options": [{ "label": "a" }, { "label": "b" }] },
                { "id": "why", "question": "Anything else?" }
            ] }
        }))
    }

    #[test]
    fn a_dsh_approval_is_allow_or_deny_with_its_reason() {
        let e = dsh_permission("9-1", "call_1");
        let view = view_of(&e).unwrap();
        assert_eq!((view["kind"].as_str(), view["from"].as_str(), view["tool"].as_str(), view["summary"].as_str()), (Some("permission"), Some("dsh"), Some("Write"), Some("C:\\waku\\agents\\probe1.txt")));
        assert_eq!(view["always"], Value::Null);
        assert_eq!(view["reason"]["zh"], "允许本次操作使用 danger-full-access 权限：outside the workspace");
        assert_eq!(reply_for(&e, &json!({ "action": "allow" }), "en"), Some(json!({ "decision": "allow" })));
        assert_eq!(reply_for(&e, &json!({ "action": "deny" }), "zh"), Some(json!({ "decision": "deny" })));
        assert!(reply_for(&e, &json!({ "action": "always" }), "en").is_none());
        assert!(reply_for(&e, &json!({ "action": "answer", "answers": [] }), "en").is_none());
        // Its audit text when it gave no words to show; its PowerShell's
        // command as the summary, named PowerShell.
        let cmd = crate::events_dsh::for_ask(&json!({
            "hook_event_name": "PermissionRequest", "session_id": "session-ed6a", "agent": "dsh", "answerable": true,
            "tool_name": "pwsh", "tool_input": { "command": "Set-Content -Path C:\\waku\\agents\\cmd.txt -Value hi", "description": "Retry write with escalated permissions" },
            "reason": "escalate sandbox to danger-full-access: outside the workspace"
        }));
        let view = view_of(&cmd).unwrap();
        assert_eq!(
            (view["tool"].as_str(), view["summary"].as_str(), view["reason"].as_str()),
            (Some("PowerShell"), Some("Set-Content -Path C:\\waku\\agents\\cmd.txt -Value hi"), Some("escalate sandbox to danger-full-access: outside the workspace"))
        );
    }

    #[test]
    fn dsh_questions_are_answered_by_their_ids() {
        let e = dsh_question();
        let view = view_of(&e).unwrap();
        assert_eq!((view["kind"].as_str(), view["canAnswer"].as_bool()), (Some("question"), Some(true)));
        assert_eq!(view["questions"][0]["header"], "File Name");
        assert_eq!(view["questions"][1]["multiSelect"], true);
        // One with no options asks for a text.
        assert_eq!(view["questions"][2]["kind"], "text");
        let reply = reply_for(&e, &json!({ "action": "answer", "answers": [["test2"], ["a", "b"], { "text": " none " }] }), "en").unwrap();
        assert_eq!(
            reply,
            json!({ "answers": [
                { "id": "name", "selected": ["test2"] },
                { "id": "both", "selected": ["a", "b"] },
                { "id": "why", "selected": [], "custom": "none" }
            ] })
        );
        // Typed in place of an option: its text, nothing picked.
        let typed = reply_for(&e, &json!({ "action": "answer", "answers": [{ "text": "test3" }, ["a"], { "text": "x" }] }), "en").unwrap();
        assert_eq!(typed["answers"][0], json!({ "id": "name", "selected": [], "custom": "test3" }));
        // Not one of its options, two for a single choice, too few: none.
        assert!(reply_for(&e, &json!({ "action": "answer", "answers": [["test9"], ["a"], { "text": "x" }] }), "en").is_none());
        assert!(reply_for(&e, &json!({ "action": "answer", "answers": [["test1", "test2"], ["a"], { "text": "x" }] }), "en").is_none());
        assert!(reply_for(&e, &json!({ "action": "answer", "answers": [["test1"]] }), "en").is_none());
        assert!(reply_for(&e, &json!({ "action": "allow" }), "en").is_none());
    }

    #[test]
    fn dsh_prompts_wait_side_by_side_and_close_by_their_ids() {
        let got = Rc::new(RefCell::new(Vec::new()));
        let mut asks: Asks<Box<dyn FnOnce(Option<Value>)>> = Asks::default();
        let reply = |got: &Rc<RefCell<Vec<Option<Value>>>>| {
            let got = got.clone();
            Box::new(move |v: Option<Value>| got.borrow_mut().push(v)) as Box<dyn FnOnce(Option<Value>)>
        };
        let one = asks.add(dsh_permission("9-1", "call_1"), reply(&got), 0, 1000).ok().unwrap();
        let two = asks.add(dsh_permission("9-2", "call_2"), reply(&got), 0, 1000).ok().unwrap();
        assert_eq!(asks.views().as_array().unwrap().len(), 2);
        // The second answered in DSH: that one goes (no decision of hers).
        let replied = json!({ "hook_event_name": "PermissionReplied", "session_id": "session-ed6a", "agent": "dsh", "permission_id": "9-2", "tool_use_id": "call_2", "outcome": "rejected", "by": "dsh" });
        assert_eq!(asks.seen(&replied), vec![two]);
        // The first answered on her island.
        assert!(asks.answer(one, &json!({ "action": "allow" }), "en"));
        assert_eq!(got.borrow().as_slice(), &[Some(json!({})), Some(json!({ "decision": "allow" }))]);
        // Its call done (DSH's own name for it): closed by its id.
        let three = asks.add(dsh_permission("9-3", "call_3"), reply(&got), 0, 1000).ok().unwrap();
        let done = json!({ "hook_event_name": "PostToolUse", "session_id": "session-ed6a", "agent": "dsh", "tool_name": "write", "tool_input": {}, "tool_use_id": "call_3" });
        assert_eq!(asks.seen(&done), vec![three]);
    }

    #[test]
    fn a_dsh_prompt_handed_back_says_where_to_answer() {
        let mut asks: Asks<Box<dyn FnOnce(Option<Value>)>> = Asks::default();
        let none = || Box::new(|_: Option<Value>| {}) as Box<dyn FnOnce(Option<Value>)>;
        // Its time up: the session waits on DSH's own window for it.
        asks.add(dsh_permission("9-1", "call_1"), none(), 0, 1000).ok().unwrap();
        assert_eq!(asks.expire(1000).len(), 1);
        assert_eq!(
            asks.take_handed(),
            vec![json!({ "session": "session-ed6a", "agent": "dsh", "mood": "waiting", "within": true, "detail": { "key": "detail.approveIn", "vars": { "app": "DeepSeek Harness", "what": { "key": "step.write", "vars": { "file": "probe1.txt" } } } } })]
        );
        assert!(asks.take_handed().is_empty(), "taken once");
        // A question handed back ("去 DeepSeek Harness 处理"): to be answered there.
        let id = asks.add(dsh_question(), none(), 0, 1000).ok().unwrap();
        assert!(asks.dismiss(id));
        let handed = asks.take_handed();
        assert_eq!(handed[0]["detail"], json!({ "key": "detail.answerIn", "vars": { "app": "DeepSeek Harness", "what": "test1 or test2?" } }));
        // Out of sight: each of DSH's; Claude Code's keeps its words.
        asks.add(dsh_permission("9-2", "call_2"), none(), 0, 1000).ok().unwrap();
        asks.add(permission(), none(), 0, 1000).ok().unwrap();
        assert_eq!(asks.dismiss_all(), 2);
        assert_eq!(asks.take_handed().len(), 1);
        // Answered on her, or settled in DSH: nothing to say.
        let id = asks.add(dsh_permission("9-3", "call_3"), none(), 0, 1000).ok().unwrap();
        assert!(asks.answer(id, &json!({ "action": "allow" }), "en"));
        asks.add(dsh_permission("9-4", "call_4"), none(), 0, 1000).ok().unwrap();
        asks.seen(&json!({ "hook_event_name": "PermissionReplied", "session_id": "session-ed6a", "agent": "dsh", "permission_id": "9-4" }));
        assert!(asks.take_handed().is_empty());
    }

    #[test]
    fn an_interrupt_closes_a_codex_prompt() {
        let got = Rc::new(RefCell::new(Vec::new()));
        let mut asks: Asks<Box<dyn FnOnce(Option<Value>)>> = Asks::default();
        let sink = got.clone();
        asks.add(codex_permission("Bash", "rm x"), Box::new(move |v| sink.borrow_mut().push(v)), 0, 1000).ok().unwrap();
        assert!(asks.seen(&json!({ "hook_event_name": "PreToolUse", "session_id": "c", "tool_name": "Bash" })).is_empty());
        assert_eq!(asks.seen(&json!({ "hook_event_name": "Interrupt", "session_id": "c", "agent": "codex" })).len(), 1);
        assert_eq!(got.borrow().as_slice(), &[Some(json!({}))]);
    }
}
