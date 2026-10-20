// DeepSeek Harness hook events → pet messages: Claude Code's (events.rs),
// its tools named as Claude Code's, plus agent: "dsh".
//
// They come to /hook?agent=dsh from her own plugin inside DSH
// (integrations/dsh/wakuwaku.js, connection.rs), in the words of Claude
// Code's hooks as DSH's Claude Code hook bridge
// (@deepseek-ai/dsh-hooks-claude-code) said them, which until 0.3.2 ran this
// program with connection::DSH_FLAG for each (main.rs dsh_hook; still read,
// for a DSH still running those old rows). What there is, as the bridge of
// the desktop app 0.2.0-rc.2 and its CLI sent it (2026-10-15): session_id,
// cwd, hook_event_name and an empty transcript_path; SessionStart with
// source, UserPromptSubmit with prompt, PreToolUse and PostToolUse with
// tool_name, tool_input, tool_use_id (and tool_response, as text: "Error: …"
// for one that failed, which gets no event of its own), Stop with nothing
// more. The plugin sends every tool's (the bridge, some 0.9 s a hook, only a
// few), and what only it sees: the turn's last words
// (last_assistant_message: the bridge's are read from the session log,
// zstd-compressed, last_reply), the session's name (session_title: else
// from DSH's projection cache, title_of), whose the session is (origin,
// parent_session, top_session: else from its log, whose), whether a tool
// failed (is_error), and a turn that ended without finishing (Stop with
// stop_reason "aborted" or "blocked": stopped, not done; StopFailure for
// one that failed). Its approvals and questions come as PermissionRequest
// (her plugin's version 2; never from the bridge): told as Claude Code's
// (waiting, what for), and held for her answer where the plugin can take one
// (answerable) and she is in sight (server.rs; for_ask makes them asks.rs's,
// side by side, each closed by its own PermissionReplied); else told only,
// answered in DSH (answered_in_dsh). PermissionReplied, what came of one,
// lets the session work on. Its tools have names of their own (read, write, edit,
// str_replace_editor, pwsh or bash, glob, grep, web_search, web_fetch,
// todo_write, ask_user_question), told in Claude Code's words here
// (claude_named). A subagent's session is told as any other's: it is no
// session of hers (whose, #58).
use std::collections::HashMap;
use std::io::{BufRead, Read};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde_json::{json, Map, Value};

fn s<'a>(e: &'a Value, key: &str) -> &'a str {
    e.get(key).and_then(Value::as_str).unwrap_or("")
}

// A DSH tool and its input as Claude Code names them, for events.rs.
fn claude_named(tool: &str, input: &Value) -> (String, Value) {
    let file = |key: &str| json!({ "file_path": s(input, key) });
    match tool {
        "read" | "read_image" => ("Read".into(), file("file_path")),
        "write" => ("Write".into(), file("file_path")),
        "edit" => ("Edit".into(), file("file_path")),
        "str_replace_editor" => match s(input, "command") {
            "view" => ("Read".into(), file("path")),
            "create" => ("Write".into(), file("path")),
            _ => ("Edit".into(), file("path")),
        },
        "pwsh" | "bash" => ("Bash".into(), json!({ "command": s(input, "command"), "description": s(input, "description") })),
        "glob" => ("Glob".into(), input.clone()),
        "grep" => ("Grep".into(), input.clone()),
        "web_search" => ("WebSearch".into(), input.clone()),
        "web_fetch" => ("WebFetch".into(), input.clone()),
        "todo_write" => {
            let todos: Vec<Value> = input.get("todos").and_then(Value::as_array).into_iter().flatten().map(|t| json!({ "content": s(t, "content"), "status": s(t, "status") })).collect();
            ("TodoWrite".into(), json!({ "todos": todos }))
        }
        "ask_user_question" => ("AskUserQuestion".into(), input.clone()),
        _ => (tool.to_string(), input.clone()),
    }
}

// A prompt of DSH's (her plugin's PermissionRequest: an approval, or
// questions) as asks.rs reads one: the tool and its input in Claude Code's
// words (the panel's summary: the command, the file), questions as
// AskUserQuestion's, each keeping its id (DSH takes its answers back by
// them): one with no options asks for a text, a plan's detail (its review,
// exit_plan_mode's) is its description.
pub fn for_ask(e: &Value) -> Value {
    let mut out = e.clone();
    let input = e.get("tool_input").unwrap_or(&Value::Null);
    if s(e, "tool_name") == "ask_user_question" {
        let questions: Vec<Value> = input
            .get("questions")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .map(|q| {
                let options: Vec<Value> = q.get("options").and_then(Value::as_array).into_iter().flatten().map(|o| json!({ "label": s(o, "label"), "description": s(o, "description") })).collect();
                let mut view = json!({ "id": s(q, "id"), "question": s(q, "question"), "header": s(q, "header"), "multiSelect": q.get("multiSelect") == Some(&Value::Bool(true)) });
                if !s(q, "detail").is_empty() {
                    view["description"] = json!(crate::events::clip(s(q, "detail"), 400));
                }
                if options.is_empty() {
                    view["kind"] = json!("text");
                } else {
                    view["options"] = Value::Array(options);
                }
                view
            })
            .collect();
        out["tool_name"] = json!("AskUserQuestion");
        out["tool_input"] = json!({ "questions": questions });
    } else {
        let (name, input) = claude_named(s(e, "tool_name"), input);
        // Its PowerShell named so on the panel (Claude Code's own name for it).
        out["tool_name"] = json!(if s(e, "tool_name") == "pwsh" { "PowerShell" } else { name.as_str() });
        out["tool_input"] = input;
    }
    out
}

// The message for a prompt of DSH's she does not hold (her plugin cannot
// take her answer to it, or she is out of sight): waiting, to be approved
// in DSH's own window.
pub fn answered_in_dsh(message: &mut Value) {
    if message["detail"]["key"] == "detail.approve" {
        let what = message["detail"]["vars"]["what"].clone();
        message["detail"] = json!({ "key": "detail.approveIn", "vars": { "app": "DeepSeek Harness", "what": what } });
    }
}

// The session's name as DSH gave it (its first-prompt title, or one its
// model made up), from its projection cache:
// <DSH home>/storages/session_projcache/sessions/<session id>.json, whose
// rows.title.val it is. Undocumented: none when it reads otherwise.
pub fn title_of(home: &std::path::Path, session: &str) -> Option<String> {
    if session.is_empty() || !session.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') {
        return None;
    }
    let file = home.join("storages").join("session_projcache").join("sessions").join(format!("{session}.json"));
    let cache: Value = serde_json::from_str(&std::fs::read_to_string(file).ok()?).ok()?;
    let title = cache.pointer("/record/rows/title/val")?.as_str()?.trim();
    (!title.is_empty()).then(|| crate::events::clip(title, 80))
}

// --- Its last reply, from its session log ------------------------------------------
//
// DSH keeps each session's events as JSONL, compressed in Zstandard frames,
// one appended for each batch it writes (dsh-session-persistence-jsonl
// 0.2.0-rc.2): <home>/sessions/<the cwd's key>/<the session id>/
// session.v4.jsonl.zstd. A turn's last words are an assistant/message, its
// data.message.content's text parts, in one of the last frames: written
// with the Stop hook's hook/invoked, so there as the hook runs. Read from
// the end: only the file's tail (more of it while a turn needs it, up to
// MAX_WINDOW), each frame found back from the end by its magic number and
// taken only when it decodes whole into lines of JSON and ends where the one
// after it begins (a frame still being written at the end is passed over),
// newest first, up to the turn's turn/start; all of it in DEADLINE at most.
// Undocumented: no reply when anything reads otherwise.

const ZSTD_MAGIC: [u8; 4] = [0x28, 0xb5, 0x2f, 0xfd];
const FIRST_WINDOW: usize = 256 * 1024;
const MAX_WINDOW: usize = 4 * 1024 * 1024;
// One batch's events at most (a frame decoding to more is passed over).
const MAX_FRAME_TEXT: usize = 4 * 1024 * 1024;
// The most a frame's header may ask the decoder to hold (its window): DSH's
// level writes a few MB at most; what looks like a frame inside another's
// bytes may ask for anything (ruzstd's own bound is 128 MB).
const MAX_ZSTD_WINDOW: u64 = 16 * 1024 * 1024;
const DEADLINE: Duration = Duration::from_millis(300);

// A name as DSH writes it into a path: [A-Za-z0-9._-] as they are, any other
// UTF-16 unit (~ too) as ~XXXX (encodeSegment).
fn escaped(c: char, out: &mut String) {
    if c != '~' && (c.is_ascii_alphanumeric() || "._-".contains(c)) {
        out.push(c);
    } else {
        let mut units = [0u16; 2];
        for unit in c.encode_utf16(&mut units) {
            out.push_str(&format!("~{unit:04X}"));
        }
    }
}

// The folder DSH keeps a project's sessions in, from its path (projectKey):
// each run of separators (/ \ :) one -, the rest escaped, leading -s gone,
// 251 at most, between --.
pub fn project_key(cwd: &str) -> String {
    let mut out = String::new();
    let mut in_run = false;
    for c in cwd.chars() {
        if matches!(c, '/' | '\\' | ':') {
            if !in_run {
                out.push('-');
            }
            in_run = true;
        } else {
            escaped(c, &mut out);
            in_run = false;
        }
    }
    let key: String = out.trim_start_matches('-').chars().take(251).collect();
    format!("--{}--", if key.is_empty() { "root" } else { &key })
}

// A session's log: the current generation's (v4), else the newest there is,
// compressed or not; a look into its own folder alone.
pub fn session_log(home: &Path, cwd: &str, session: &str) -> Option<PathBuf> {
    if cwd.is_empty() || session.is_empty() {
        return None;
    }
    let mut id = String::new();
    session.chars().for_each(|c| escaped(c, &mut id));
    let dir = home.join("sessions").join(project_key(cwd)).join(id);
    let current = dir.join("session.v4.jsonl.zstd");
    if current.is_file() {
        return Some(current);
    }
    let version = |name: &str| name.strip_prefix("session.v")?.split_once('.').filter(|(_, rest)| matches!(*rest, "jsonl" | "jsonl.zstd"))?.0.parse::<u32>().ok();
    std::fs::read_dir(&dir)
        .ok()?
        .filter_map(|e| e.ok()?.file_name().into_string().ok())
        .filter_map(|name| Some((version(&name)?, name)))
        .max()
        .map(|(_, name)| dir.join(name))
}

// What a line of the log says of the turn: its last words, or its start.
enum Seen {
    Reply(String),
    TurnStart,
}

fn seen(line: &str) -> Option<Seen> {
    if !line.contains("\"assistant/message\"") && !line.contains("\"turn/start\"") {
        return None;
    }
    let event: Value = serde_json::from_str(line).ok()?;
    match event["type"].as_str()? {
        "turn/start" => Some(Seen::TurnStart),
        "assistant/message" => {
            let parts = event.pointer("/data/message/content")?.as_array()?;
            let text: Vec<&str> = parts.iter().filter(|p| p["type"] == "text").filter_map(|p| p["text"].as_str()).collect();
            let text = text.join("\n");
            (!text.trim().is_empty()).then(|| Seen::Reply(text.trim().to_string()))
        }
        _ => None,
    }
}

// One whole frame from the start of these bytes: its text, and how many of
// the bytes it took. None when it does not decode, or not into lines of JSON.
fn decode_frame(bytes: &[u8]) -> Option<(String, usize)> {
    let mut source = bytes;
    let mut out = Vec::new();
    {
        let decoder = ruzstd::decoding::StreamingDecoder::new_with_max_window_size(&mut source, MAX_ZSTD_WINDOW).ok()?;
        decoder.take(MAX_FRAME_TEXT as u64 + 1).read_to_end(&mut out).ok()?;
    }
    if out.len() > MAX_FRAME_TEXT {
        return None;
    }
    let text = String::from_utf8(out).ok()?;
    let is_jsonl = text.lines().map(str::trim).filter(|l| !l.is_empty()).all(|l| l.starts_with('{') && l.ends_with('}'));
    is_jsonl.then(|| (text, bytes.len() - source.len()))
}

// What a tail of the log gives: the turn's last words, none (the turn began
// without any, or the whole log was read), or more of the log is needed.
enum Scan {
    Found(String),
    NoReply,
    More,
}

fn scan_frames(tail: &[u8], is_whole: bool, until: Instant) -> Scan {
    let mut starts = Vec::new();
    let mut at = 0;
    while let Some(i) = tail[at..].windows(4).position(|w| w == ZSTD_MAGIC) {
        starts.push(at + i);
        at += i + 1;
    }
    // The start of the frame taken last (the one after this, in the log).
    let mut next: Option<usize> = None;
    for &start in starts.iter().rev() {
        if Instant::now() > until {
            return Scan::NoReply;
        }
        if next.is_some_and(|n| start >= n) {
            continue;
        }
        let Some((text, used)) = decode_frame(&tail[start..]) else { continue };
        let end = start + used;
        let fits = match next {
            Some(n) => end == n,
            // The newest whole frame: the end, or one being written after it.
            None => end == tail.len() || tail[end..].starts_with(&ZSTD_MAGIC),
        };
        if !fits {
            continue;
        }
        next = Some(start);
        for line in text.lines().rev() {
            match seen(line) {
                Some(Seen::Reply(reply)) => return Scan::Found(reply),
                Some(Seen::TurnStart) => return Scan::NoReply,
                None => {}
            }
        }
    }
    if is_whole { Scan::NoReply } else { Scan::More }
}

// The same for a log written as plain JSONL.
fn scan_lines(tail: &[u8], is_whole: bool) -> Scan {
    let text = String::from_utf8_lossy(tail);
    // A line cut by the window's start, or one still being written, is no line.
    let mut lines: Vec<&str> = text.split('\n').collect();
    lines.pop();
    for line in lines.iter().skip(usize::from(!is_whole)).rev() {
        match seen(line) {
            Some(Seen::Reply(reply)) => return Scan::Found(reply),
            Some(Seen::TurnStart) => return Scan::NoReply,
            None => {}
        }
    }
    if is_whole { Scan::NoReply } else { Scan::More }
}

// The last words of the session's turn just ended, from its log.
pub fn last_reply(log: &Path) -> Option<String> {
    last_reply_from(log, FIRST_WINDOW)
}

fn last_reply_from(log: &Path, first_window: usize) -> Option<String> {
    use std::io::{Seek, SeekFrom};
    let until = Instant::now() + DEADLINE;
    let compressed = log.extension().is_some_and(|e| e == "zstd");
    let mut file = std::fs::File::open(log).ok()?;
    let len = file.metadata().ok()?.len();
    let mut window = first_window.max(1);
    loop {
        let take = len.min(window as u64);
        file.seek(SeekFrom::Start(len - take)).ok()?;
        let mut tail = Vec::with_capacity(take as usize);
        (&mut file).take(take).read_to_end(&mut tail).ok()?;
        let is_whole = take == len;
        let scan = if compressed { scan_frames(&tail, is_whole, until) } else { scan_lines(&tail, is_whole) };
        match scan {
            Scan::Found(reply) => return Some(reply),
            Scan::NoReply => return None,
            Scan::More if is_whole || window >= MAX_WINDOW || Instant::now() > until => return None,
            Scan::More => window = (window * 4).min(MAX_WINDOW),
        }
    }
}

// --- Whose a session is: the person's, or a subagent's ---------------------------------
//
// A subagent DSH starts (its subagent tool, or a plugin's subagents.start(),
// as dsh-mnemon's idle review does) is a session of its own: its own id and
// log, and the bridge tells its SessionStart, UserPromptSubmit, tools and
// Stop as any other session's. Its first prompt, the task it was handed, is
// written as the person's words (source kind user: dsh-subagent and its
// in-process driver, 0.2.0-rc.2), so DSH's title is made of it too (#58:
// "Bounded completed checkpoint (untrusted evidence; …"). What tells it
// apart is the header its log begins with (childSessionMeta): origin
// "subagent", parentSession the session that started it, delegationDepth.
// DSH follows a lineage through origin "subagent" only, as here: a session
// the person forked has a parentSession and no origin, and is the person's.
//
// So a subagent has no line of its own on the pet: its start, its turns and
// their end take away any line it has (session-end), and nothing of it is
// named or counted. Its tools are the session's it works for (the one at the
// top of its lineage), as Claude Code's subagents' are, with `within` (state.rs):
// only into a turn of that one's under way, never starting one nor waking one
// at rest (an idle review runs while it is at rest). Its to-do list is its
// own. The header never changes: read once a session (the log's first line,
// 64 KB of it at most), kept; a log not there yet tells nothing (the
// person's, for now, and read again with the next event). Her plugin reads
// the same header in DSH (its session's, live) and sends it with each event:
// then no log is read (whose_of).

const MAX_HEADER: u64 = 64 * 1024;
const MAX_LINEAGE: usize = 8;
const MAX_KNOWN: usize = 512;

#[derive(Clone, Debug, PartialEq)]
pub enum Whose {
    Own,
    // A subagent's, and the session it works for at the top ("" when not named).
    Sub(String),
}

// The first line of a log, as written (no more than MAX_HEADER of it).
fn first_line(log: &Path) -> Option<String> {
    let file = std::fs::File::open(log).ok()?;
    let mut line = Vec::new();
    if log.extension().is_some_and(|e| e == "zstd") {
        let decoder = ruzstd::decoding::StreamingDecoder::new_with_max_window_size(std::io::BufReader::new(file), MAX_ZSTD_WINDOW).ok()?;
        std::io::BufReader::new(decoder.take(MAX_HEADER)).read_until(b'\n', &mut line).ok()?;
    } else {
        std::io::BufReader::new(file.take(MAX_HEADER)).read_until(b'\n', &mut line).ok()?;
    }
    String::from_utf8(line).ok()
}

// What a session's header says of it: None when there is none to read,
// Some(None) for the person's, Some(Some(parent)) for a subagent's.
fn parent_of(home: &Path, cwd: &str, session: &str) -> Option<Option<String>> {
    let header: Value = serde_json::from_str(first_line(&session_log(home, cwd, session)?)?.trim()).ok()?;
    if header["type"] != "session" || header["id"] != session {
        return None;
    }
    Some((header["origin"] == "subagent").then(|| s(&header, "parentSession").to_string()))
}

// The top of a lineage, from a session up by the sessions' logs.
fn top_from(home: &Path, cwd: &str, from: String, session: &str) -> String {
    let mut top = from;
    for _ in 0..MAX_LINEAGE {
        match parent_of(home, cwd, &top) {
            Some(Some(up)) if !up.is_empty() && up != session => top = up,
            _ => break,
        }
    }
    top
}

// Whose an event's session is: as her plugin says it (origin "subagent" or
// none; top_session, the top of its lineage as far as DSH's live sessions
// go, else its parent followed up by their logs), else (the old bridge's
// events) from its log.
pub fn whose_of(e: &Value, home: &Path) -> Whose {
    match e.get("origin").and_then(Value::as_str) {
        None => whose(home, s(e, "cwd"), s(e, "session_id")),
        Some("subagent") => match (s(e, "top_session"), s(e, "parent_session")) {
            (top, _) if !top.is_empty() => Whose::Sub(top.to_string()),
            (_, "") => Whose::Sub(String::new()),
            (_, parent) => Whose::Sub(top_from(home, s(e, "cwd"), parent.to_string(), s(e, "session_id"))),
        },
        Some(_) => Whose::Own,
    }
}

// Whose the session is, its lineage followed up to the top (its subagents'
// logs are in its folder: they keep its cwd).
pub fn whose(home: &Path, cwd: &str, session: &str) -> Whose {
    static KNOWN: Mutex<Option<HashMap<String, Whose>>> = Mutex::new(None);
    if let Some(known) = KNOWN.lock().unwrap().as_ref().and_then(|m| m.get(session)) {
        return known.clone();
    }
    let whose = match parent_of(home, cwd, session) {
        None => return Whose::Own,
        Some(None) => Whose::Own,
        Some(Some(parent)) => Whose::Sub(top_from(home, cwd, parent, session)),
    };
    let mut known = KNOWN.lock().unwrap();
    let known = known.get_or_insert_with(HashMap::new);
    if known.len() >= MAX_KNOWN {
        known.clear();
    }
    known.insert(session.to_string(), whose.clone());
    whose
}

// A subagent's event: its own start, turns and end take its line away; a
// tool of its, but its to-do list, is the top session's, within a turn of
// that one's.
fn of_subagent(e: &Value, top: &str, home: &Path) -> Option<Value> {
    if matches!(s(e, "hook_event_name"), "SessionStart" | "UserPromptSubmit" | "Stop" | "StopFailure") {
        return Some(json!({ "mood": "idle", "event": "session-end", "agent": "dsh", "session": s(e, "session_id") }));
    }
    if top.is_empty() || s(e, "tool_name") == "todo_write" {
        return None;
    }
    let mut theirs = e.clone();
    theirs["session_id"] = json!(top);
    // Its own name (made of its task) is not the session's it works for.
    if let Some(fields) = theirs.as_object_mut() {
        fields.remove("session_title");
    }
    let mut message = message_of(&theirs, home)?;
    message["within"] = json!(true);
    Some(message)
}

// The message for an event, or None for one that changes nothing.
pub fn to_message(e: &Value, home: &std::path::Path) -> Option<Value> {
    if !e.is_object() || s(e, "hook_event_name") == LOADED {
        return None;
    }
    match whose_of(e, home) {
        Whose::Own => message_of(e, home),
        Whose::Sub(top) => of_subagent(e, &top, home),
    }
}

// The plugin's word as DSH loads it (her log says it: log_line).
const LOADED: &str = "WakuwakuLoaded";

// That word: not anything a session did, so not what tells the bridge's way
// working (server.rs leaves it out of when DSH was last heard).
pub fn is_loaded(e: &Value) -> bool {
    s(e, "hook_event_name") == LOADED
}

// The message for an event of a session of the person's.
fn message_of(e: &Value, home: &std::path::Path) -> Option<Value> {
    let fields: &Map<String, Value> = e.as_object()?;
    let mut claude = fields.clone();
    // A turn's end: its last words, as Claude Code gives them; read from its
    // log when they did not come with it (the old bridge's).
    if s(e, "hook_event_name") == "Stop" && e.get("last_assistant_message").is_none() {
        if let Some(reply) = session_log(home, s(e, "cwd"), s(e, "session_id")).and_then(|log| last_reply(&log)) {
            claude.insert("last_assistant_message".into(), json!(reply));
        }
    }
    // A prompt answered (in DSH or on her island) or withdrawn: the session
    // works on, its tool's end or its turn's to come; only into a turn
    // still under way (within), never waking one.
    if s(e, "hook_event_name") == "PermissionReplied" {
        let mut message = json!({ "mood": "working", "agent": "dsh", "within": true });
        if !s(e, "session_id").is_empty() {
            message["session"] = json!(s(e, "session_id"));
        }
        return Some(message);
    }
    // A turn stopped by the person, or refused before it began: not done,
    // nothing to say of it; she is there again.
    if s(e, "hook_event_name") == "Stop" && matches!(s(e, "stop_reason"), "aborted" | "blocked") {
        let mut message = json!({ "mood": "idle", "agent": "dsh" });
        if !s(e, "session_id").is_empty() {
            message["session"] = json!(s(e, "session_id"));
        }
        if !s(e, "cwd").is_empty() {
            message["project"] = json!(crate::events::project_of(s(e, "cwd")));
        }
        return Some(message);
    }
    // A tool that failed (an edit the person refused in DSH's own dialog
    // too) still ends in PostToolUse, its result an error's words: a
    // failure, which changed nothing.
    let failed = e.get("is_error").and_then(Value::as_bool) == Some(true) || s(e, "tool_response").trim_start().starts_with("Error");
    if s(e, "hook_event_name") == "PostToolUse" && failed {
        claude.insert("hook_event_name".into(), json!("PostToolUseFailure"));
    }
    if let Some(tool) = fields.get("tool_name").and_then(Value::as_str) {
        let (name, input) = claude_named(tool, fields.get("tool_input").unwrap_or(&Value::Null));
        claude.insert("tool_name".into(), json!(name));
        claude.insert("tool_input".into(), input);
    }
    let claude = Value::Object(claude);
    let mut message = crate::events::to_message(&claude)?;
    message["agent"] = json!("dsh");
    // The to-do list as its tool ends: the one hook it has (connection.rs).
    if s(e, "hook_event_name") == "PostToolUse" {
        if let Some(todo) = crate::events::todo_of(s(&claude, "tool_name"), claude.get("tool_input").unwrap_or(&Value::Null)) {
            message["todo"] = todo;
        }
    }
    // Its name, when it did not come with the event (events.rs took one that did).
    if matches!(s(e, "hook_event_name"), "SessionStart" | "UserPromptSubmit" | "Stop") && s(e, "session_title").is_empty() {
        if let Some(title) = title_of(home, s(e, "session_id")) {
            message["title"] = json!(title);
        }
    }
    Some(message)
}

// A session's id, short: its first 8 after "session-".
fn short(id: &str) -> String {
    id.strip_prefix("session-").unwrap_or(id).chars().take(8).collect()
}

// A line for her log as a session starts or a turn begins or ends: what came,
// and whether she found the app it runs in (server.rs chain_for), for
// telling why its window can't be reached; a subagent's, whose it is.
pub fn log_line(e: &Value, found: Option<u32>, home: &Path) -> Option<String> {
    let name = s(e, "hook_event_name");
    if name == LOADED {
        let profile = if s(e, "profile").is_empty() { String::new() } else { format!(", profile {}", s(e, "profile")) };
        return Some(format!("dsh: her plugin loaded (version {}{profile}, pid {})", e["version"].as_u64().unwrap_or(0), e["pid"].as_u64().unwrap_or(0)));
    }
    // A prompt: one she can only show, and what came of each.
    if name == "PermissionRequest" && e.get("answerable") != Some(&Value::Bool(true)) {
        return Some(format!("dsh {name} ({}): her plugin cannot take an answer to it, told only", short(s(e, "session_id"))));
    }
    if name == "PermissionReplied" {
        let by = if s(e, "by") == "her" { "on her island" } else { "in DSH" };
        return Some(format!("dsh {name} ({}): {}, {by}", short(s(e, "session_id")), s(e, "outcome")));
    }
    if !matches!(name, "SessionStart" | "UserPromptSubmit" | "Stop") {
        return None;
    }
    let id = short(s(e, "session_id"));
    if let Whose::Sub(top) = whose_of(e, home) {
        return Some(format!("dsh {name} ({id}): a subagent of {}, not shown", if top.is_empty() { "a session not named".to_string() } else { short(&top) }));
    }
    let app = found.map_or("no app found".to_string(), |pid| format!("its app found (pid {pid})"));
    Some(format!("dsh {name} ({id}){}", if name == "Stop" { String::new() } else { format!(", {app}") }))
}

#[cfg(test)]
mod tests {
    use super::*;

    // As the CLI 0.2.0-rc.2's bridge sends them (from the test VM), in a
    // folder that is no git repository.
    fn event(name: &str, more: Value) -> Value {
        let mut e = json!({ "session_id": "session-c961", "transcript_path": "", "cwd": "Z:\\nowhere\\dsh", "hook_event_name": name });
        for (k, v) in more.as_object().unwrap() {
            e[k] = v.clone();
        }
        e
    }

    fn nowhere() -> std::path::PathBuf {
        std::path::PathBuf::from("Z:\\nowhere\\.dsh")
    }

    #[test]
    fn dsh_events_become_messages() {
        let m = to_message(&event("SessionStart", json!({ "source": "startup" })), &nowhere()).unwrap();
        assert_eq!(m, json!({ "mood": "idle", "react": "wave", "say": { "key": "say.hello" }, "agent": "dsh", "session": "session-c961", "project": "dsh" }));
        let m = to_message(&event("UserPromptSubmit", json!({ "prompt": "Read note.txt, then create copy.txt" })), &nowhere()).unwrap();
        assert_eq!((m["event"].as_str(), m["task"].as_str()), (Some("turn-start"), Some("Read note.txt, then create copy.txt")));
        let write = json!({ "tool_name": "write", "tool_input": { "file_path": "Z:\\nowhere\\dsh\\copy.txt", "content": "hi\n" }, "tool_use_id": "call_00_CKTB", "tool_response": "<path>…</path>" });
        let m = to_message(&event("PostToolUse", write), &nowhere()).unwrap();
        assert_eq!((m["event"].as_str(), m["detail"].as_str(), m["file"].as_str()), (Some("tool-done"), Some("Write"), Some("Z:\\nowhere\\dsh\\copy.txt")));
        // Refused in DSH's own dialog: no edit.
        let refused = json!({ "tool_name": "write", "tool_input": { "file_path": "C:\\waku\\agents\\outside.txt" }, "tool_response": "Error: the user rejected escalating this operation to \"danger-full-access\"" });
        let m = to_message(&event("PostToolUse", refused), &nowhere()).unwrap();
        assert_eq!((m["event"].as_str(), m.get("file")), (Some("tool-failed"), None));
        let m = to_message(&event("PreToolUse", json!({ "tool_name": "pwsh", "tool_input": { "command": "Get-ChildItem", "description": "List files" } })), &nowhere()).unwrap();
        assert_eq!(m["step"], json!({ "key": "step.run", "vars": { "what": "List files" } }));
        let m = to_message(&event("PreToolUse", json!({ "tool_name": "str_replace_editor", "tool_input": { "command": "str_replace", "path": "Z:\\a\\b.rs" } })), &nowhere()).unwrap();
        assert_eq!(m["step"], json!({ "key": "step.edit", "vars": { "file": "b.rs" } }));
        let m = to_message(&event("PreToolUse", json!({ "tool_name": "ask_user_question", "tool_input": { "questions": [] } })), &nowhere()).unwrap();
        assert_eq!(m["mood"], "waiting");
        let todos = json!({ "todos": [{ "content": "Read note.txt", "status": "completed" }, { "content": "Write copy.txt", "status": "in_progress" }] });
        let m = to_message(&event("PostToolUse", json!({ "tool_name": "todo_write", "tool_input": todos })), &nowhere()).unwrap();
        assert_eq!(m["todo"]["set"][1], json!({ "text": "Write copy.txt", "active": "", "status": "in_progress" }));
        let m = to_message(&event("Stop", json!({ "stop_hook_active": false })), &nowhere()).unwrap();
        assert_eq!((m["mood"].as_str(), m.get("reply"), m["agent"].as_str()), (Some("done"), None, Some("dsh")));
        assert!(to_message(&event("SubagentStop", json!({ "agent_id": "a" })), &nowhere()).is_none());
        let mut start = event("UserPromptSubmit", json!({ "prompt": "x" }));
        start["session_id"] = json!("session-6aa5a96d-33fc");
        assert_eq!(log_line(&start, Some(3684), &nowhere()).as_deref(), Some("dsh UserPromptSubmit (6aa5a96d), its app found (pid 3684)"));
        start["hook_event_name"] = json!("Stop");
        assert_eq!(log_line(&start, None, &nowhere()).as_deref(), Some("dsh Stop (6aa5a96d)"));
        assert!(log_line(&event("PostToolUse", json!({})), None, &nowhere()).is_none());
        assert!(to_message(&json!("nope"), &nowhere()).is_none());
    }

    // Real session logs from the test VM (DSH 0.2.0-rc.2, test prompts only):
    // c96167f1 the CLI reading note.txt and writing copy.txt, one turn;
    // 6aa5a96d the desktop app, a question then a write it was refused, two
    // turns; b7df6078 a one-word answer.
    const C96: &[u8] = include_bytes!("../../test/fixtures/dsh/session-c96167f1.v4.jsonl.zstd");
    const A6A: &[u8] = include_bytes!("../../test/fixtures/dsh/session-6aa5a96d.v4.jsonl.zstd");
    const B7D: &[u8] = include_bytes!("../../test/fixtures/dsh/session-b7df6078.v4.jsonl.zstd");
    const C96_REPLY: &str = "Done — note.txt contained `hi`, and copy.txt now has the same text.";

    // A log written into a folder of the test's own, by this name.
    fn log_file(tag: &str, bytes: &[u8]) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("wakuwaku-dsh-log-{}-{tag}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("session.v4.jsonl.zstd");
        std::fs::write(&file, bytes).unwrap();
        file
    }

    #[test]
    fn where_dsh_keeps_a_session_s_log() {
        assert_eq!(project_key("C:\\Users\\waku\\Documents\\deepseek-harness\\default-workspace"), "--C-Users-waku-Documents-deepseek-harness-default-workspace--");
        assert_eq!(project_key("C:\\waku\\agents\\dsh"), "--C-waku-agents-dsh--");
        assert_eq!(project_key("/home/me/my proj~1"), "--home-me-my~0020proj~007E1--");
        assert_eq!(project_key("D:\\宠物\\a.b_c-d"), "--D-~5BA0~7269-a.b_c-d--");
        assert_eq!(project_key("/"), "--root--");
        assert_eq!(project_key(&"x".repeat(300)).len(), 255);

        let home = std::env::temp_dir().join(format!("wakuwaku-dsh-home-{}", std::process::id()));
        let dir = home.join("sessions").join("--C-waku-agents-dsh--").join("session-c96167f1-eb93");
        std::fs::create_dir_all(&dir).unwrap();
        assert_eq!(session_log(&home, "C:\\waku\\agents\\dsh", "session-c96167f1-eb93"), None);
        // A later generation alone, or plain JSONL: the newest; v4 when there.
        std::fs::write(dir.join("session.v3.jsonl"), b"").unwrap();
        std::fs::write(dir.join("session.v5.jsonl.zstd"), b"").unwrap();
        std::fs::write(dir.join("session.v5.jsonl.zstd.tmp"), b"").unwrap();
        assert_eq!(session_log(&home, "C:\\waku\\agents\\dsh", "session-c96167f1-eb93"), Some(dir.join("session.v5.jsonl.zstd")));
        std::fs::write(dir.join("session.v4.jsonl.zstd"), C96).unwrap();
        let log = session_log(&home, "C:\\waku\\agents\\dsh", "session-c96167f1-eb93").unwrap();
        assert_eq!(log, dir.join("session.v4.jsonl.zstd"));
        assert_eq!(session_log(&home, "", "session-c96167f1-eb93"), None);

        // A turn's end, its last words on the island as Claude Code's are.
        let stop = json!({ "session_id": "session-c96167f1-eb93", "transcript_path": "", "cwd": "C:\\waku\\agents\\dsh", "hook_event_name": "Stop", "stop_hook_active": false });
        let m = to_message(&stop, &home).unwrap();
        assert_eq!((m["mood"].as_str(), m["reply"].as_str()), (Some("done"), Some("Done — note.txt contained hi, and copy.txt now has the same text.")));
        // Another session's, or none: no words, nothing else changed.
        let mut other = stop.clone();
        other["session_id"] = json!("session-none");
        assert_eq!(to_message(&other, &home).unwrap(), json!({ "mood": "done", "agent": "dsh", "session": "session-none", "project": "dsh" }));
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn the_last_words_of_a_turn_from_its_log() {
        // A frame decodes whole and says where it ends (the session line, 156 bytes).
        let (text, used) = decode_frame(C96).unwrap();
        assert_eq!(used, 156);
        assert!(text.starts_with("{\"type\":\"session\""), "{text}");
        assert!(decode_frame(&C96[1..]).is_none());

        assert_eq!(last_reply(&log_file("c96", C96)).as_deref(), Some(C96_REPLY));
        assert_eq!(last_reply(&log_file("b7d", B7D)).as_deref(), Some("hello"));
        // Two turns: the second's, all its lines.
        let second = last_reply(&log_file("6aa", A6A)).unwrap();
        assert!(second.starts_with("I couldn't create the file — you declined the escalation request") && second.ends_with("with the content `hi`."), "{second}");
        // A window smaller than the frames: more of the log, as needed.
        assert_eq!(last_reply_from(&log_file("6aa-small", A6A), 600).as_deref(), Some(second.as_str()));
        assert_eq!(last_reply_from(&log_file("c96-small", C96), 100).as_deref(), Some(C96_REPLY));
    }

    #[test]
    fn a_log_still_being_written() {
        // Its last frame cut short (turn/end on its way): passed over.
        assert_eq!(last_reply(&log_file("cut-last", &C96[..14900])).as_deref(), Some(C96_REPLY));
        // The frame with the last words cut short: the turn's words before them.
        assert_eq!(last_reply(&log_file("cut-reply", &C96[..14500])).as_deref(), Some("I'll read note.txt first."));
        // The second turn begun, none of its words yet: none, not the first turn's.
        assert_eq!(last_reply(&log_file("cut-turn", &A6A[..16700])), None);
        // Nothing of a log, or not one.
        assert_eq!(last_reply(&log_file("empty", b"")), None);
        assert_eq!(last_reply(&log_file("junk", b"\x28\xb5\x2f\xfdjunk that is no frame")), None);
        assert_eq!(last_reply(Path::new("Z:\\nowhere\\session.v4.jsonl.zstd")), None);
    }

    #[test]
    fn a_log_in_plain_jsonl() {
        let dir = std::env::temp_dir().join(format!("wakuwaku-dsh-plain-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("session.v4.jsonl");
        let line = |v: Value| format!("{v}\n");
        let text = [
            line(json!({ "type": "turn/start", "seq": 1, "data": { "turn": 1 } })),
            line(json!({ "type": "assistant/message", "seq": 2, "data": { "turn": 1, "message": { "role": "assistant", "content": [{ "type": "text", "text": "All done." }, { "type": "tool-call" }] } } })),
            line(json!({ "type": "turn/end", "seq": 3, "data": { "turn": 1 } })),
        ]
        .concat();
        std::fs::write(&file, &text).unwrap();
        assert_eq!(last_reply(&file).as_deref(), Some("All done."));
        assert_eq!(last_reply_from(&file, 40).as_deref(), Some("All done."));
        std::fs::write(&file, format!("{text}{}", line(json!({ "type": "turn/start", "seq": 4 })))).unwrap();
        assert_eq!(last_reply(&file), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_session_s_name_from_dsh_s_cache() {
        let home = std::env::temp_dir().join(format!("wakuwaku-dsh-{}", std::process::id()));
        let dir = home.join("storages").join("session_projcache").join("sessions");
        std::fs::create_dir_all(&dir).unwrap();
        let cache = json!({ "version": 7, "record": { "rows": { "title": { "ver": 1, "seq": 44, "val": "Read note.txt and copy it" } } } });
        std::fs::write(dir.join("session-c961.json"), cache.to_string()).unwrap();
        assert_eq!(title_of(&home, "session-c961").as_deref(), Some("Read note.txt and copy it"));
        let m = to_message(&event("Stop", json!({})), &home).unwrap();
        assert_eq!(m["title"], "Read note.txt and copy it");
        assert_eq!(title_of(&home, "session-none"), None);
        assert_eq!(title_of(&home, "..\\..\\x"), None);
        let _ = std::fs::remove_dir_all(&home);
    }

    // A log as DSH writes it, a Zstandard frame for each batch of lines.
    fn frames(batches: &[Vec<Value>]) -> Vec<u8> {
        let mut out = Vec::new();
        for batch in batches {
            let text: String = batch.iter().map(|v| format!("{v}\n")).collect();
            out.extend(ruzstd::encoding::compress_to_vec(text.as_bytes(), ruzstd::encoding::CompressionLevel::Fastest));
        }
        out
    }

    // A session's header as the log's first line, as DSH 0.2.0-rc.2 writes
    // it (SessionStore.prepare), with more fields of its own.
    fn header(id: &str, more: Value) -> Value {
        let mut h = json!({ "type": "session", "version": 4, "id": id, "createdAt": 1792100300000u64, "cwd": WORKSPACE });
        for (k, v) in more.as_object().unwrap() {
            h[k] = v.clone();
        }
        h
    }

    const WORKSPACE: &str = "C:\\Users\\waku\\Documents\\deepseek-harness\\default-workspace";
    // The desktop app's session in the test VM (the A6A log): the person's.
    const PARENT: &str = "session-6aa5a96d-33fc-4973-b085-bf5c8f30a6b7";

    #[test]
    fn a_subagent_is_no_session_of_its_own() {
        let home = std::env::temp_dir().join(format!("wakuwaku-dsh-sub-{}", std::process::id()));
        let folder = home.join("sessions").join(project_key(WORKSPACE));
        let put = |id: &str, file: &str, bytes: &[u8]| {
            std::fs::create_dir_all(folder.join(id)).unwrap();
            std::fs::write(folder.join(id).join(file), bytes).unwrap();
        };
        put(PARENT, "session.v4.jsonl.zstd", A6A);
        // A subagent it started, as dsh-subagent's in-process driver makes
        // one (childSessionMeta; its id a bare UUID): its header in a frame
        // of its own, its first turn after it (the task it was handed, as the
        // person's words), as a plugin's idle review starts one (#58).
        let child = "9d1f4c2a-5b7e-4f0a-8c3d-2e6b1a7f9c40";
        let task = "Bounded completed checkpoint (untrusted evidence; some context may be omitted):\n[user/message; checkpoint event 11]";
        let child_log = frames(&[
            vec![header(child, json!({ "parentSession": PARENT, "isSeeded": false, "origin": "subagent", "delegationDepth": 1, "agentPreset": "standard" }))],
            vec![
                json!({ "type": "approval/policy", "seq": 0, "time": 1792100300001u64, "data": { "policy": "never", "source": "delegation" } }),
                json!({ "type": "turn/start", "seq": 1, "time": 1792100300002u64, "data": { "turn": 1 } }),
                json!({ "type": "user/message", "seq": 2, "time": 1792100300003u64, "data": { "content": [{ "type": "text", "text": task }], "source": { "kind": "user" }, "role": "user" } }),
                json!({ "type": "subagent/descriptor", "seq": 3, "time": 1792100300004u64, "data": { "version": 3, "mode": "one-shot", "provider": "spawn" } }),
            ],
        ]);
        put(child, "session.v4.jsonl.zstd", &child_log);
        // Its own subagent, in plain JSONL: the top is still the person's session.
        let grandchild = "0e4b7d21-6c3a-4e9f-b1d8-5a2c9f7e3b16";
        let line = header(grandchild, json!({ "parentSession": child, "isSeeded": false, "origin": "subagent", "delegationDepth": 2 }));
        put(grandchild, "session.v4.jsonl", format!("{line}\n").as_bytes());
        // A session the person forked: a parentSession, no origin: the person's.
        let fork = "session-3c8e2a91-7d4f-4b6a-9e1c-8f2d5b7a0c63";
        put(fork, "session.v4.jsonl.zstd", &frames(&[vec![header(fork, json!({ "parentSession": PARENT, "isSeeded": true, "delegationDepth": 0 }))]]));
        // A subagent whose parent is not named.
        let orphan = "7a2c5e8b-1d4f-4c9a-8b3e-6f0d2a9c5e71";
        put(orphan, "session.v4.jsonl.zstd", &frames(&[vec![header(orphan, json!({ "isSeeded": false, "origin": "subagent", "delegationDepth": 1 }))]]));
        // A header of another session's in this one's folder: none read.
        let stray = "session-5e1a9c7b-2f4d-4a8e-b6c3-9d0f1e7a2b58";
        put(stray, "session.v4.jsonl.zstd", &frames(&[vec![header(child, json!({ "parentSession": PARENT, "origin": "subagent" }))]]));
        // DSH's projection cache names both, the subagent by its task.
        let cache = home.join("storages").join("session_projcache").join("sessions");
        std::fs::create_dir_all(&cache).unwrap();
        let record = |title: &str, subagent: Value| json!({ "version": 7, "record": { "identity": { "formatVersion": 4, "createdAt": 1792100300000u64, "cwd": WORKSPACE, "isSeeded": false, "inheritedEventCount": 0 }, "rows": { "title": { "ver": 1, "seq": 48, "val": title }, "subagent": { "ver": 2, "seq": 3, "val": subagent } } } });
        std::fs::write(cache.join(format!("{PARENT}.json")), record("Create a file", json!({})).to_string()).unwrap();
        std::fs::write(cache.join(format!("{child}.json")), record("Bounded completed checkpoint (untrusted", json!({ "identity": { "mode": "one-shot", "seq": 3 } })).to_string()).unwrap();

        assert_eq!(whose(&home, WORKSPACE, PARENT), Whose::Own);
        assert_eq!(whose(&home, WORKSPACE, child), Whose::Sub(PARENT.into()));
        assert_eq!(whose(&home, WORKSPACE, grandchild), Whose::Sub(PARENT.into()));
        assert_eq!(whose(&home, WORKSPACE, fork), Whose::Own);
        assert_eq!(whose(&home, WORKSPACE, orphan), Whose::Sub(String::new()));
        assert_eq!(whose(&home, WORKSPACE, stray), Whose::Own);
        assert_eq!(whose(&home, WORKSPACE, "session-not-there-yet"), Whose::Own);
        assert_eq!(whose(&home, "", child), Whose::Sub(PARENT.into()), "known once read");

        let of = |id: &str, name: &str, more: Value| {
            let mut e = json!({ "session_id": id, "transcript_path": "", "cwd": WORKSPACE, "hook_event_name": name });
            for (k, v) in more.as_object().unwrap() {
                e[k] = v.clone();
            }
            to_message(&e, &home)
        };
        // Its start, its turn and its end: no line of its own, no name, no
        // task, no reply, no wave; any line it had goes.
        let gone = json!({ "mood": "idle", "event": "session-end", "agent": "dsh", "session": child });
        assert_eq!(of(child, "SessionStart", json!({ "source": "startup" })), Some(gone.clone()));
        assert_eq!(of(child, "UserPromptSubmit", json!({ "prompt": task })), Some(gone.clone()));
        assert_eq!(of(child, "Stop", json!({ "stop_hook_active": false })), Some(gone));
        // Its edits are its session's, within a turn of that one's.
        let write = json!({ "tool_name": "write", "tool_input": { "file_path": "C:\\w\\notes.md" }, "tool_use_id": "call_01", "tool_response": "ok" });
        let m = of(child, "PostToolUse", write.clone()).unwrap();
        assert_eq!(m, json!({ "mood": "working", "detail": "Write", "event": "tool-done", "file": "C:\\w\\notes.md", "toolId": "call_01", "agent": "dsh", "session": PARENT, "project": "default-workspace", "within": true }));
        assert_eq!(of(grandchild, "PostToolUse", write.clone()).unwrap()["session"], PARENT);
        let ask = of(child, "PreToolUse", json!({ "tool_name": "ask_user_question", "tool_input": { "questions": [] } })).unwrap();
        assert_eq!((ask["mood"].as_str(), ask["session"].as_str(), ask["within"].as_bool()), (Some("waiting"), Some(PARENT), Some(true)));
        // Its to-do list is its own; one whose parent is not named, nothing.
        assert_eq!(of(child, "PostToolUse", json!({ "tool_name": "todo_write", "tool_input": { "todos": [] } })), None);
        assert_eq!(of(orphan, "PostToolUse", write), None);
        assert_eq!(of(orphan, "UserPromptSubmit", json!({ "prompt": "x" })).unwrap()["event"], "session-end");
        // The person's sessions as they were: a forked one too.
        let m = of(PARENT, "UserPromptSubmit", json!({ "prompt": "Create a file named a.txt" })).unwrap();
        assert_eq!((m["event"].as_str(), m["task"].as_str(), m["title"].as_str(), m.get("within")), (Some("turn-start"), Some("Create a file named a.txt"), Some("Create a file"), None));
        assert_eq!(of(fork, "SessionStart", json!({ "source": "startup" })).unwrap()["react"], "wave");
        // Her log says whose.
        let e = json!({ "session_id": grandchild, "cwd": WORKSPACE, "hook_event_name": "UserPromptSubmit" });
        assert_eq!(log_line(&e, None, &home).as_deref(), Some("dsh UserPromptSubmit (0e4b7d21): a subagent of 6aa5a96d, not shown"));
        let e = json!({ "session_id": orphan, "cwd": WORKSPACE, "hook_event_name": "Stop" });
        assert_eq!(log_line(&e, None, &home).as_deref(), Some("dsh Stop (7a2c5e8b): a subagent of a session not named, not shown"));
        let _ = std::fs::remove_dir_all(&home);
    }

    // As her plugin sends them (integrations/dsh/wakuwaku.js): what the
    // bridge sent, and what only it sees.
    fn plugin(name: &str, session: &str, more: Value) -> Value {
        let mut e = json!({ "session_id": session, "transcript_path": "", "cwd": WORKSPACE, "hook_event_name": name, "origin": "", "parent_session": "" });
        for (k, v) in more.as_object().unwrap() {
            e[k] = v.clone();
        }
        e
    }

    #[test]
    fn what_her_plugin_tells() {
        let home = std::env::temp_dir().join(format!("wakuwaku-dsh-plugin-ev-{}", std::process::id()));
        let folder = home.join("sessions").join(project_key(WORKSPACE));
        std::fs::create_dir_all(folder.join(PARENT)).unwrap();
        std::fs::write(folder.join(PARENT).join("session.v4.jsonl.zstd"), A6A).unwrap();
        // Its last words and its name come with it: neither the log nor the
        // projection cache is read (the log's last words differ).
        let stop = plugin("Stop", PARENT, json!({ "stop_hook_active": false, "last_assistant_message": "All done.", "session_title": "Create a file" }));
        let m = to_message(&stop, &home).unwrap();
        assert_eq!((m["mood"].as_str(), m["reply"].as_str(), m["title"].as_str()), (Some("done"), Some("All done."), Some("Create a file")));
        // A turn with no words: none, not the log's.
        let m = to_message(&plugin("Stop", PARENT, json!({ "last_assistant_message": "" })), &home).unwrap();
        assert_eq!(m.get("reply"), None);
        // Stopped by the person: there again, not done; a turn that failed: an error.
        let m = to_message(&plugin("Stop", PARENT, json!({ "stop_reason": "aborted", "last_assistant_message": "" })), &home).unwrap();
        assert_eq!(m, json!({ "mood": "idle", "agent": "dsh", "session": PARENT, "project": "default-workspace" }));
        let m = to_message(&plugin("StopFailure", PARENT, json!({ "error_details": "401 Unauthorized: invalid api key" })), &home).unwrap();
        assert_eq!((m["mood"].as_str(), m["error"].as_str()), (Some("error"), Some("401 Unauthorized: invalid api key")));
        // A tool that failed, said so.
        let m = to_message(&plugin("PostToolUse", PARENT, json!({ "tool_name": "pwsh", "tool_input": { "command": "exit 1" }, "tool_use_id": "c1", "tool_response": "exit code 1", "is_error": true })), &home).unwrap();
        assert_eq!(m["event"], "tool-failed");
        // Every tool's step, not only the few the bridge had.
        let m = to_message(&plugin("PreToolUse", PARENT, json!({ "tool_name": "grep", "tool_input": { "pattern": "TODO" }, "tool_use_id": "c2" })), &home).unwrap();
        assert_eq!((m["event"].as_str(), m["detail"].as_str()), (Some("tool-start"), Some("Grep")));

        // Whose, as it says: a subagent's, the top of its lineage given; the
        // person's (origin ""), with no log read at all.
        let sub = plugin("PostToolUse", "9d1f-child", json!({ "origin": "subagent", "parent_session": "a1-mid", "top_session": PARENT, "tool_name": "write", "tool_input": { "file_path": "C:\\w\\n.md" }, "tool_use_id": "c3", "tool_response": "ok", "session_title": "List all files in the" }));
        assert_eq!(whose_of(&sub, &home), Whose::Sub(PARENT.into()));
        let m = to_message(&sub, &home).unwrap();
        assert_eq!((m["session"].as_str(), m["within"].as_bool(), m["file"].as_str()), (Some(PARENT), Some(true), Some("C:\\w\\n.md")));
        // Its own name (of its task) does not rename the session it works for
        // (seen in the test VM, 0.2.0-rc.2: the line read "List all files in the").
        assert_eq!(m.get("title"), None);
        let mut start = sub.clone();
        start["hook_event_name"] = json!("UserPromptSubmit");
        assert_eq!(to_message(&start, &home).unwrap()["event"], "session-end");
        let mut failed = sub.clone();
        failed["hook_event_name"] = json!("StopFailure");
        assert_eq!(to_message(&failed, &home).unwrap()["event"], "session-end");
        // Its parent alone (not live in DSH): followed up by the logs (PARENT's own: the person's).
        let mut parent_only = sub.clone();
        parent_only["top_session"] = json!("");
        parent_only["parent_session"] = json!(PARENT);
        assert_eq!(whose_of(&parent_only, &home), Whose::Sub(PARENT.into()));
        parent_only["parent_session"] = json!("");
        assert_eq!(whose_of(&parent_only, &home), Whose::Sub(String::new()));
        assert_eq!(whose_of(&plugin("Stop", "session-not-there", json!({})), &home), Whose::Own);

        // A prompt: waiting, what for (as Claude Code's), answered on her
        // island when she holds it, else in DSH; what came of it lets the
        // session work on, only within a turn under way.
        let asked = plugin("PermissionRequest", PARENT, json!({ "tool_name": "pwsh", "tool_input": { "command": "Get-Content -Path C:\\Windows\\win.ini", "description": "" }, "tool_use_id": "c4", "permission_id": "9-1", "answerable": true }));
        let mut m = to_message(&asked, &home).unwrap();
        assert_eq!((m["mood"].as_str(), m["detail"]["key"].as_str(), m["detail"]["vars"]["what"]["vars"]["what"].as_str()), (Some("waiting"), Some("detail.approve"), Some("Get-Content -Path C:\\Windows\\win.ini")));
        answered_in_dsh(&mut m);
        assert_eq!((m["detail"]["key"].as_str(), m["detail"]["vars"]["app"].as_str()), (Some("detail.approveIn"), Some("DeepSeek Harness")));
        assert_eq!(m["detail"]["vars"]["what"]["key"], "step.command");
        let write = plugin("PermissionRequest", PARENT, json!({ "tool_name": "write", "tool_input": { "file_path": "C:\\waku\\note.txt" }, "answerable": false }));
        assert_eq!(to_message(&write, &home).unwrap()["detail"]["vars"]["what"], json!({ "key": "step.write", "vars": { "file": "note.txt" } }));
        assert_eq!(log_line(&write, None, &home).as_deref(), Some("dsh PermissionRequest (6aa5a96d): her plugin cannot take an answer to it, told only"));
        // A question: waiting with it, said so whoever answers.
        let question = plugin("PermissionRequest", PARENT, json!({ "tool_name": "ask_user_question", "tool_input": { "questions": [] }, "answerable": true }));
        let mut m = to_message(&question, &home).unwrap();
        answered_in_dsh(&mut m);
        assert_eq!(m["detail"], json!({ "key": "detail.asksQuestion" }));
        let replied = plugin("PermissionReplied", PARENT, json!({ "permission_id": "9-1", "tool_use_id": "c4", "outcome": "allowed-once", "by": "her" }));
        assert_eq!(to_message(&replied, &home).unwrap(), json!({ "mood": "working", "agent": "dsh", "within": true, "session": PARENT }));
        assert_eq!(log_line(&replied, None, &home).as_deref(), Some("dsh PermissionReplied (6aa5a96d): allowed-once, on her island"));

        // Its word as DSH loads it: nothing for the pet, a line for her log.
        let loaded = json!({ "session_id": "", "hook_event_name": "WakuwakuLoaded", "version": 1, "pid": 4321, "profile": "desktop" });
        assert!(to_message(&loaded, &home).is_none());
        assert_eq!(log_line(&loaded, None, &home).as_deref(), Some("dsh: her plugin loaded (version 1, profile desktop, pid 4321)"));
        assert!(is_loaded(&loaded) && !is_loaded(&json!({ "hook_event_name": "Stop" })));
        let _ = std::fs::remove_dir_all(&home);
    }
}
