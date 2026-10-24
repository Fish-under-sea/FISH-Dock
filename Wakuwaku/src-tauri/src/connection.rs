// How Claude Code and Codex reach her: the plugin, or hooks written into
// Claude Code's settings.json; hooks written into Codex's hooks.json (and
// Antigravity's; OpenCode gets a plugin file of ours, omp an extension in
// its agent folder). Read on demand (the person may change any of them at
// any time), and written only when they ask, from the settings.
//
// Every Claude Code event is an HTTP hook: Claude Code POSTs it to her port,
// no process at all. SessionStart also gets a command, run in the background
// (async), that starts her when she is not up, which no HTTP hook can do.
// That command is this program with ENSURE_FLAG, given as an argument list
// (no shell). Codex runs only commands: see "Codex" below.
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde_json::{json, Map, Value};

pub const ENSURE_FLAG: &str = "--wakuwaku-ensure-running";
pub const CODEX_FLAG: &str = "--wakuwaku-codex-hook";
pub const AGY_FLAG: &str = "--wakuwaku-agy-hook";
const URL_MARK: &str = "from=wakuwaku";
// What the app wrote before it was called Wakuwaku (claude-pets).
pub const LEGACY_ENSURE_FLAG: &str = "--claude-pets-ensure-running";
const LEGACY_URL_MARK: &str = "from=claude-pets";
pub const PLUGIN_ID: &str = "wakuwaku@wakuwaku";
pub const PLUGIN_COMMANDS: [&str; 2] = ["/plugin marketplace add zhoupengjie/wakuwaku", "/plugin install wakuwaku@wakuwaku"];

// Each event Claude Code reports, and whether it takes a matcher; the prompt
// waits for the person (up to 300 s) instead of the default 2.
const EVENTS: [(&str, bool, u64); 11] = [
    ("SessionStart", false, 2),
    ("SessionEnd", false, 2),
    ("UserPromptSubmit", false, 2),
    ("PreToolUse", true, 2),
    ("PostToolUse", true, 2),
    ("PostToolUseFailure", true, 2),
    ("PermissionRequest", true, 300),
    ("Elicitation", false, 2),
    ("TaskCompleted", false, 2),
    ("Stop", false, 2),
    ("StopFailure", false, 2),
];

pub fn claude_settings_file() -> PathBuf {
    let dir = std::env::var_os("CLAUDE_CONFIG_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME")).unwrap_or_default()).join(".claude"));
    dir.join("settings.json")
}

// Whether Claude Code is on this computer, as far as can be told: its
// folder there (~/.claude, or CLAUDE_CONFIG_DIR).
pub fn claude_here() -> bool {
    claude_settings_file().parent().is_some_and(Path::is_dir)
}

// The copy write_json keeps of a settings file (made once, beside it).
pub fn backup_of(file: &Path) -> PathBuf {
    PathBuf::from(format!("{}.wakuwaku.bak", file.display()))
}

// A path as the person reads it: ~ for the home folder when it is under it.
pub fn shown_path(file: &Path) -> String {
    let home = std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME")).map(PathBuf::from);
    match home.as_deref().filter(|h| !h.as_os_str().is_empty()).and_then(|h| file.strip_prefix(h).ok()) {
        Some(rest) => format!("~{}{}", std::path::MAIN_SEPARATOR, rest.display()),
        None => file.display().to_string(),
    }
}

// What kept a settings file from being written, for what to do about it:
// locked (another program has it open), invalid (not JSON), flowList (DSH's
// patch file with its list as [ … ]), path (agy can run no program from
// where she is), pathChars (nor Reasonix's cmd, for a character in that
// path), old (a Reasonix 1.x), other. From
// the error's text: io's ends "(os error 32)" (or 33) whatever the
// language, serde_json's says where, "at line 3 column 5".
pub fn error_kind(error: &str) -> &'static str {
    if error.contains(AGY_NO_NAME) {
        "path"
    } else if error.contains(REASONIX_NO_NAME) {
        "pathChars"
    } else if error.contains(REASONIX_OLD) {
        "old"
    } else if error.contains("(os error 32)") || error.contains("(os error 33)") {
        "locked"
    } else if error.contains(DSH_FLAT) {
        "flowList"
    } else if let Some(rest) = error.strip_prefix(DSH_BRIDGE_FAILED) {
        // The bridge's packages: why, as dsh_bridge.rs tells it.
        match rest.trim_start().strip_prefix('[').and_then(|r| r.split(']').next()).unwrap_or("") {
            "registry" => "bridgeRegistry",
            "version" => "bridgeVersion",
            "notFound" => "bridgeNotFound",
            "releaseAge" => "bridgeReleaseAge",
            "timeout" => "bridgeTimeout",
            "runtime" => "bridgeRuntime",
            "busy" => "bridgeBusy",
            "disk" => "bridgeDisk",
            "permission" => "bridgePermission",
            _ => "bridgeOther",
        }
    } else if error.contains(" at line ") && error.contains(" column ") {
        "invalid"
    } else {
        "other"
    }
}

// A JSON settings file: {} when there is none, Err when unreadable.
// A file as last read, kept while its size and time stay (a look at the
// file's metadata, not its contents): the settings' snapshot asks for
// Claude Code's settings.json three times and Codex's hooks.json twice,
// up to four times a second while the settings are open (2026-10-13).
type Stamp = Option<(u64, std::time::SystemTime)>;
static READ: Mutex<Option<std::collections::HashMap<PathBuf, (Stamp, Result<Value, String>)>>> = Mutex::new(None);

fn read_json(file: &Path) -> Result<Value, String> {
    let stamp: Stamp = fs::metadata(file).ok().map(|m| (m.len(), m.modified().unwrap_or(std::time::UNIX_EPOCH)));
    let mut read = READ.lock().unwrap_or_else(|e| e.into_inner());
    let read = read.get_or_insert_with(std::collections::HashMap::new);
    if let Some((at, value)) = read.get(file) {
        if *at == stamp {
            return value.clone();
        }
    }
    let value = read_json_now(file);
    read.insert(file.to_path_buf(), (stamp, value.clone()));
    value
}

fn read_json_now(file: &Path) -> Result<Value, String> {
    match fs::read_to_string(file) {
        Ok(text) => serde_json::from_str(&text).map_err(|e| e.to_string()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(json!({})),
        Err(e) => Err(e.to_string()),
    }
}

// Claude Code's settings.
fn read_settings() -> Result<Value, String> {
    read_json(&claude_settings_file())
}

// Write a settings file, backed up once beside itself.
fn write_json(file: &Path, value: &Value) -> Result<(), String> {
    let backup = backup_of(file);
    if file.exists() && !backup.exists() {
        fs::copy(file, &backup).map_err(|e| e.to_string())?;
    }
    if let Some(dir) = file.parent() {
        fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let text = serde_json::to_string_pretty(value).map_err(|e| e.to_string())?;
    let written = fs::write(file, format!("{text}\n")).map_err(|e| e.to_string());
    forget_read(file);
    written
}

// As last read no longer: read again next time, whatever its stamp.
fn forget_read(file: &Path) {
    if let Some(read) = READ.lock().unwrap_or_else(|e| e.into_inner()).as_mut() {
        read.remove(file);
    }
}

// An agent's file with our part taken out: written back with no backup made
// (what is there is ours alone; a backup then held our hooks, seen
// 2026-10-16), or, when she made the file (no backup beside it: there was
// nothing of the person's to keep when she first wrote it) and nothing is
// left in it, taken away, as it was before.
fn write_back(file: &Path, value: &Value) -> Result<(), String> {
    if value.as_object().is_some_and(|o| o.is_empty()) && !backup_of(file).exists() {
        let gone = fs::remove_file(file).map_err(|e| e.to_string());
        forget_read(file);
        return gone;
    }
    let text = serde_json::to_string_pretty(value).map_err(|e| e.to_string())?;
    let written = fs::write(file, format!("{text}\n")).map_err(|e| e.to_string());
    forget_read(file);
    written
}

// This program, as the hook command starts it.
pub fn launch_command() -> String {
    crate::data::exe().map(|p| p.to_string_lossy().into_owned()).unwrap_or_default()
}

fn hook_url(port: u16) -> String {
    format!("http://127.0.0.1:{port}/hook?{URL_MARK}")
}

// A path as Windows compares them: one slash, any case.
fn same_path(p: &str) -> String {
    if cfg!(windows) { p.replace('\\', "/").to_lowercase() } else { p.to_string() }
}

// Ours: what this version installs, and what older ones did.
fn is_ours(hook: &Value) -> bool {
    let mut text = [hook.get("url"), hook.get("command")].iter().flatten().map(|v| v.to_string()).collect::<Vec<_>>().join(" ");
    for arg in hook.get("args").and_then(Value::as_array).into_iter().flatten() {
        text.push(' ');
        text.push_str(&arg.to_string());
    }
    [URL_MARK, ENSURE_FLAG, CODEX_FLAG, ZCODE_FLAG, REASONIX_FLAG, LEGACY_URL_MARK, LEGACY_ENSURE_FLAG, "claude-hook.js"].iter().any(|m| text.contains(m))
}

// Our hooks, event by event.
fn ours_by_event(settings: &Value) -> Map<String, Value> {
    let mut found = Map::new();
    for (event, groups) in settings.get("hooks").and_then(Value::as_object).into_iter().flatten() {
        let ours: Vec<Value> = groups
            .as_array()
            .into_iter()
            .flatten()
            .flat_map(|g| g.get("hooks").and_then(Value::as_array).cloned().unwrap_or_default())
            .filter(is_ours)
            .collect();
        if !ours.is_empty() {
            found.insert(event.clone(), Value::Array(ours));
        }
    }
    found
}

// Our entries dropped from every event, and events left empty.
fn strip(hooks: Option<&Value>) -> Map<String, Value> {
    let mut out = Map::new();
    let Some(Value::Object(hooks)) = hooks else { return out };
    for (event, groups) in hooks {
        let Value::Array(groups) = groups else {
            out.insert(event.clone(), groups.clone());
            continue;
        };
        let kept: Vec<Value> = groups
            .iter()
            .filter_map(|group| {
                let mut group = group.clone();
                let hooks: Vec<Value> = group.get("hooks").and_then(Value::as_array).into_iter().flatten().filter(|h| !is_ours(h)).cloned().collect();
                if hooks.is_empty() {
                    return None;
                }
                group["hooks"] = Value::Array(hooks);
                Some(group)
            })
            .collect();
        if !kept.is_empty() {
            out.insert(event.clone(), Value::Array(kept));
        }
    }
    out
}

// What we add, event by event. http_only: no command, nothing starts her.
fn entries(port: u16, http_only: bool) -> Map<String, Value> {
    let mut out = Map::new();
    for (event, matcher, timeout) in EVENTS {
        // Claude Code runs no HTTP hook for SessionStart: the starter command
        // passes that one on.
        if event == "SessionStart" {
            continue;
        }
        let hooks = json!([{ "type": "http", "url": hook_url(port), "timeout": timeout }]);
        let group = if matcher { json!({ "matcher": "*", "hooks": hooks }) } else { json!({ "hooks": hooks }) };
        out.insert(event.into(), json!([group]));
    }
    if !http_only {
        out.insert(
            "SessionStart".into(),
            json!([{ "hooks": [{ "type": "command", "command": launch_command(), "args": [ENSURE_FLAG], "async": true, "timeout": 15 }] }]),
        );
    }
    out
}

pub fn install(settings: &Value, port: u16, http_only: bool) -> Value {
    let mut hooks = strip(settings.get("hooks"));
    for (event, groups) in entries(port, http_only) {
        let mut all = hooks.get(&event).and_then(Value::as_array).cloned().unwrap_or_default();
        all.extend(groups.as_array().cloned().unwrap_or_default());
        hooks.insert(event, Value::Array(all));
    }
    let mut out = settings.as_object().cloned().unwrap_or_default();
    out.insert("hooks".into(), Value::Object(hooks));
    Value::Object(out)
}

pub fn uninstall(settings: &Value) -> Value {
    let mut out = settings.as_object().cloned().unwrap_or_default();
    let hooks = strip(settings.get("hooks"));
    if hooks.is_empty() {
        out.remove("hooks");
    } else {
        out.insert("hooks".into(), Value::Object(hooks));
    }
    Value::Object(out)
}

// How our entries stand against what this copy would install:
//   missing / ok / httpOnly (nothing starts her) / stale (another port or
//   copy, or an old version) / partial (some events lack them).
pub fn status_of(settings: &Value, port: u16) -> &'static str {
    let found = ours_by_event(settings);
    if !EVENTS.iter().any(|(name, ..)| found.contains_key(*name)) {
        return "missing";
    }
    let list = |name: &str| found.get(name).and_then(Value::as_array).cloned().unwrap_or_default();
    let has_flag = |h: &Value| h.get("args").and_then(Value::as_array).is_some_and(|a| a.iter().any(|x| x == ENSURE_FLAG));
    let is_old = EVENTS.iter().any(|(name, ..)| list(name).iter().any(|h| h["type"] != "http" && !has_flag(h)))
        || list("SessionStart").iter().any(|h| h["type"] == "http");
    let want_url = hook_url(port);
    let is_other_port = EVENTS.iter().any(|(name, ..)| list(name).iter().any(|h| h["type"] == "http" && h["url"] != want_url.as_str()));
    let starters: Vec<Value> = list("SessionStart").into_iter().filter(|h| h["type"] == "command").collect();
    let want = vec![same_path(&launch_command()), same_path(ENSURE_FLAG)];
    let is_other_copy = starters.iter().any(|h| {
        let mut got = vec![same_path(h["command"].as_str().unwrap_or(""))];
        got.extend(h.get("args").and_then(Value::as_array).into_iter().flatten().map(|a| same_path(a.as_str().unwrap_or(""))));
        got != want
    });
    if is_old || is_other_port || is_other_copy {
        return "stale";
    }
    if EVENTS.iter().any(|(name, ..)| *name != "SessionStart" && !found.contains_key(*name)) {
        return "partial";
    }
    if starters.is_empty() { "httpOnly" } else { "ok" }
}

pub fn hooks_status(port: u16) -> &'static str {
    match read_settings() {
        Ok(settings) => status_of(&settings, port),
        Err(_) => "unreadable",
    }
}

pub fn is_plugin_enabled() -> bool {
    read_settings().is_ok_and(|s| s.get("enabledPlugins").and_then(|p| p.get(PLUGIN_ID)) == Some(&Value::Bool(true)))
}

// 'plugin', 'hooks' (written into settings.json), 'both' (every event twice), or 'none'.
pub fn connection(port: u16) -> &'static str {
    let plugin = is_plugin_enabled();
    let hooks = !matches!(hooks_status(port), "missing" | "unreadable");
    match (plugin, hooks) {
        (true, true) => "both",
        (true, false) => "plugin",
        (false, true) => "hooks",
        (false, false) => "none",
    }
}

// Change Claude Code's settings.json, backed up once beside itself.
// action: install, install-http, remove.
pub fn write_hooks(action: &str, port: u16) -> Result<(), String> {
    // Read from the disk, not as last read: nothing of someone else's is written back.
    let before = read_json_now(&claude_settings_file())?;
    let after = match action {
        "remove" => uninstall(&before),
        "install" => install(&before, port, false),
        "install-http" => install(&before, port, true),
        _ => return Err(format!("unknown action {action}")),
    };
    write_json(&claude_settings_file(), &after)
}


// --- Codex: hooks in ~/.codex/hooks.json ---------------------------------------------
//
// Codex runs hooks only as commands, through the session's shell (on Windows
// `pwsh -NoProfile -Command`, about 0.3 s to start): this program with
// CODEX_FLAG, which hands the event to her port and prints her answer to a
// prompt (main.rs). So few hooks make Codex wait: the session starting and
// ending, a prompt, the turn ending (Codex runs no Stop or Interrupt hook in
// the background: none ran in 0.162.1, a slow SessionEnd keeping it up). The
// turn starting and each tool call come from hooks run in the background
// (async), which Codex has from 0.148 on; older ones skip async hooks with a
// warning, so they get those few in the foreground instead: the turn
// starting, an edit (apply_patch), a question.
//
// Codex runs a hook only once the person trusts it (/hooks in Codex), and
// again after it changes, such as this program moving: we never trust them
// on their behalf.

pub const CODEX_PROMPT_TIMEOUT: u64 = 300;
// A version: major, minor, patch.
pub type Version = (u64, u64, u64);
// The first Codex that runs async hooks.
const CODEX_ASYNC_SINCE: Version = (0, 148, 0);

// One hook we add: its event, the tools it is for (a regex, None for all),
// how long it may run (s), whether in the background (only for a Codex that
// runs those), and whether only for one that does not.
struct CodexHook {
    event: &'static str,
    matcher: Option<&'static str>,
    timeout: u64,
    is_async: bool,
    is_older: bool,
}

// A prompt waits for the person; Codex gives Interrupt and SessionEnd 3 s at most.
const CODEX_HOOKS: [CodexHook; 11] = [
    CodexHook { event: "SessionStart", matcher: None, timeout: 10, is_async: false, is_older: false },
    CodexHook { event: "PermissionRequest", matcher: None, timeout: CODEX_PROMPT_TIMEOUT, is_async: false, is_older: false },
    CodexHook { event: "Stop", matcher: None, timeout: 10, is_async: false, is_older: false },
    CodexHook { event: "Interrupt", matcher: None, timeout: 3, is_async: false, is_older: false },
    CodexHook { event: "SessionEnd", matcher: None, timeout: 3, is_async: false, is_older: false },
    // The turn starting and every step, in the background.
    CodexHook { event: "UserPromptSubmit", matcher: None, timeout: 10, is_async: true, is_older: false },
    CodexHook { event: "PreToolUse", matcher: None, timeout: 10, is_async: true, is_older: false },
    CodexHook { event: "PostToolUse", matcher: None, timeout: 10, is_async: true, is_older: false },
    // Or, for a Codex without those, the few steps that matter most, waited on.
    CodexHook { event: "UserPromptSubmit", matcher: None, timeout: 10, is_async: false, is_older: true },
    CodexHook { event: "PreToolUse", matcher: Some("^request_user_input$"), timeout: 10, is_async: false, is_older: true },
    CodexHook { event: "PostToolUse", matcher: Some("^(apply_patch|request_user_input)$"), timeout: 10, is_async: false, is_older: true },
];

fn codex_hooks_for(with_async: bool) -> impl Iterator<Item = &'static CodexHook> {
    CODEX_HOOKS.iter().filter(move |h| if with_async { !h.is_older } else { !h.is_async })
}

pub fn codex_home() -> PathBuf {
    std::env::var_os("CODEX_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME")).unwrap_or_default()).join(".codex"))
}

pub fn codex_hooks_file() -> PathBuf {
    codex_home().join("hooks.json")
}

// "codex-cli 0.146.1" (or 0.149.0-alpha.1) → (0, 146, 1).
fn parse_version(text: &str) -> Option<Version> {
    text.split_whitespace().find_map(|word| {
        let mut parts = word.split('.').map(|p| p.chars().take_while(char::is_ascii_digit).collect::<String>().parse::<u64>().ok());
        Some((parts.next()??, parts.next()??, parts.next()??))
    })
}

fn ask_codex_version() -> Option<Version> {
    let mut command = std::process::Command::new("codex");
    command.arg("--version").stdin(std::process::Stdio::null()).stderr(std::process::Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // CREATE_NO_WINDOW
        command.creation_flags(0x0800_0000);
    }
    parse_version(&String::from_utf8_lossy(&command.output().ok()?.stdout))
}

// Codex's version, None when there is no `codex` to ask (the desktop app
// alone). Asked at most once a minute.
pub fn codex_version() -> Option<Version> {
    static KNOWN: Mutex<Option<(Instant, Option<Version>)>> = Mutex::new(None);
    let mut known = KNOWN.lock().unwrap();
    match *known {
        Some((at, version)) if at.elapsed() < Duration::from_secs(60) => version,
        _ => {
            let version = ask_codex_version();
            *known = Some((Instant::now(), version));
            version
        }
    }
}

// Whether Codex runs async hooks; one we cannot ask is taken as a current one.
pub fn codex_runs_async() -> bool {
    codex_version().is_none_or(|v| v >= CODEX_ASYNC_SINCE)
}

// The command line Codex's shell runs: the bare path when it needs no
// quoting (any shell takes it), else quoted for PowerShell (& '...') on
// Windows or for a POSIX shell.
pub fn codex_command(exe: &str) -> String {
    let is_plain = !exe.is_empty() && exe.chars().all(|c| c.is_ascii_alphanumeric() || "\\/:._-".contains(c));
    if is_plain {
        return format!("{exe} {CODEX_FLAG}");
    }
    if cfg!(windows) {
        format!("& '{}' {CODEX_FLAG}", exe.replace('\'', "''"))
    } else {
        format!("'{}' {CODEX_FLAG}", exe.replace('\'', r"'\''"))
    }
}

// What we add, event by event; `waiting` is what Codex shows while a prompt waits on her.
fn codex_entries(command: &str, waiting: &str, with_async: bool) -> Map<String, Value> {
    let mut out = Map::new();
    for want in codex_hooks_for(with_async) {
        let mut hook = json!({ "type": "command", "command": command, "timeout": want.timeout });
        if want.is_async {
            hook["async"] = json!(true);
        }
        if want.event == "PermissionRequest" {
            hook["statusMessage"] = json!(waiting);
        }
        let mut group = json!({ "hooks": [hook] });
        if let Some(matcher) = want.matcher {
            group["matcher"] = json!(matcher);
        }
        let groups = out.entry(want.event).or_insert_with(|| json!([]));
        if let Value::Array(groups) = groups {
            groups.push(group);
        }
    }
    out
}

pub fn codex_install(file: &Value, command: &str, waiting: &str, with_async: bool) -> Value {
    let mut hooks = strip(file.get("hooks"));
    for (event, groups) in codex_entries(command, waiting, with_async) {
        let mut all = hooks.get(&event).and_then(Value::as_array).cloned().unwrap_or_default();
        all.extend(groups.as_array().cloned().unwrap_or_default());
        hooks.insert(event, Value::Array(all));
    }
    let mut out = file.as_object().cloned().unwrap_or_default();
    out.insert("hooks".into(), Value::Object(hooks));
    Value::Object(out)
}

// Our hooks in a hooks.json: (event, matcher, hook).
fn codex_ours(file: &Value) -> Vec<(String, Option<String>, Value)> {
    let mut ours = Vec::new();
    for (event, groups) in file.get("hooks").and_then(Value::as_object).into_iter().flatten() {
        for group in groups.as_array().into_iter().flatten() {
            let matcher = group.get("matcher").and_then(Value::as_str).map(str::to_string);
            for hook in group.get("hooks").and_then(Value::as_array).into_iter().flatten().filter(|h| is_ours(h)) {
                ours.push((event.clone(), matcher.clone(), hook.clone()));
            }
        }
    }
    ours
}

fn is_codex_hook(found: &(String, Option<String>, Value), want: &CodexHook, command: &str) -> bool {
    let (event, matcher, hook) = found;
    event == want.event
        && matcher.as_deref() == want.matcher
        && hook["type"] == "command"
        && hook["command"] == command
        && hook["timeout"].as_u64() == Some(want.timeout)
        && hook.get("async").and_then(Value::as_bool).unwrap_or(false) == want.is_async
}

// How our entries stand against what this copy would install: missing / ok /
// stale (another copy of the program, or another version's hooks) /
// upgradable (only background ones lack: Codex was updated since, or put in
// before the turn starting moved there) /
// partial (others lack).
pub fn codex_status_of(file: &Value, command: &str, with_async: bool) -> &'static str {
    let ours = codex_ours(file);
    if ours.is_empty() {
        return "missing";
    }
    if !ours.iter().all(|found| CODEX_HOOKS.iter().any(|want| is_codex_hook(found, want, command))) {
        return "stale";
    }
    let lacking: Vec<&CodexHook> = codex_hooks_for(with_async).filter(|want| !ours.iter().any(|found| is_codex_hook(found, want, command))).collect();
    if lacking.is_empty() {
        "ok"
    } else if lacking.iter().all(|want| want.is_async) {
        "upgradable"
    } else {
        "partial"
    }
}

// absent (no Codex here), unreadable, or how our entries stand.
pub fn codex_hooks_status() -> &'static str {
    if !codex_home().is_dir() {
        return "absent";
    }
    match read_json(&codex_hooks_file()) {
        Ok(file) => codex_status_of(&file, &codex_command(&launch_command()), codex_runs_async()),
        Err(_) => "unreadable",
    }
}

// Whether Codex trusts our hooks as they are in hooks.json now
// (zhoupengjie/Wakuwaku#66), read from its config.toml and never written:
// it keeps each hook it trusted as [hooks.state.'<hooks.json>:<event>:<group>:<index>']
// with trusted_hash, the hash of the hook as it was then (enabled = false
// turns one off). The hash is Codex's own (codex-rs hooks discovery.rs
// hook_hash, config fingerprint.rs version_for_toml): "sha256:" and the hex
// of the hook's identity as JSON, keys sorted, nothing between:
// { event_name, hooks: [the hook], matcher } with the timeout as Codex runs
// it and no empty field. Some(true): every one of ours trusted and on;
// Some(false): one is not (new, changed, moved, turned off); None: none of
// ours, or Codex's settings not to be read.
pub fn codex_trusted() -> Option<bool> {
    let hooks = read_json(&codex_hooks_file()).ok()?;
    let config = std::fs::read_to_string(codex_home().join("config.toml")).ok()?;
    codex_trust_of(&codex_hooks_file().to_string_lossy(), &hooks, &config)
}

fn codex_trust_of(hooks_path: &str, hooks: &Value, config: &str) -> Option<bool> {
    let states = codex_hook_states(config);
    let mut any = false;
    for (event, groups) in hooks.get("hooks").and_then(Value::as_object).into_iter().flatten() {
        let label = snake(event);
        for (g, group) in groups.as_array().into_iter().flatten().enumerate() {
            for (h, hook) in group.get("hooks").and_then(Value::as_array).into_iter().flatten().enumerate() {
                if !is_ours(hook) {
                    continue;
                }
                any = true;
                let key = same_key(&format!("{hooks_path}:{label}:{g}:{h}"));
                let trusted = states.get(&key).is_some_and(|(hash, enabled)| *enabled && *hash == codex_hook_hash(&label, group, hook));
                if !trusted {
                    return Some(false);
                }
            }
        }
    }
    any.then_some(true)
}

// SessionStart → session_start.
fn snake(name: &str) -> String {
    let mut out = String::new();
    for (i, c) in name.chars().enumerate() {
        if c.is_ascii_uppercase() && i > 0 {
            out.push('_');
        }
        out.push(c.to_ascii_lowercase());
    }
    out
}

// A key as Codex's and ours both name it: Windows' paths in any case.
fn same_key(key: &str) -> String {
    if cfg!(windows) {
        key.replace('/', "\\").to_lowercase()
    } else {
        key.to_string()
    }
}

// Codex's hash of one hook of a group (codex_trusted).
fn codex_hook_hash(event: &str, group: &Value, hook: &Value) -> String {
    use sha2::Digest;
    let timeout = hook.get("timeout").and_then(Value::as_u64);
    let timeout = if matches!(event, "session_end" | "interrupt") { timeout.unwrap_or(1).clamp(1, 3) } else { timeout.unwrap_or(600).max(1) };
    let mut handler = json!({
        "type": hook.get("type").cloned().unwrap_or(json!("command")),
        "command": hook.get("command").cloned().unwrap_or(json!("")),
        "timeout": timeout,
        "async": hook.get("async").and_then(Value::as_bool).unwrap_or(false),
    });
    for (from, to) in [("commandWindows", "commandWindows"), ("command_windows", "commandWindows"), ("statusMessage", "statusMessage"), ("additionalContextLimit", "additionalContextLimit")] {
        if let Some(v) = hook.get(from).filter(|v| !v.is_null()) {
            handler[to] = v.clone();
        }
    }
    let mut identity = json!({ "event_name": event, "hooks": [handler] });
    if let Some(m) = group.get("matcher").filter(|m| !m.is_null()) {
        identity["matcher"] = m.clone();
    }
    let digest = sha2::Sha256::digest(sorted_json(&identity).as_bytes());
    format!("sha256:{}", digest.iter().map(|b| format!("{b:02x}")).collect::<String>())
}

// JSON with every object's keys sorted, nothing between (serde_json's
// sort_all_objects and to_vec, as Codex hashes it).
fn sorted_json(v: &Value) -> String {
    match v {
        Value::Object(map) => {
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort();
            let parts: Vec<String> = keys.iter().map(|k| format!("{}:{}", Value::String((*k).clone()), sorted_json(&map[*k]))).collect();
            format!("{{{}}}", parts.join(","))
        }
        Value::Array(items) => format!("[{}]", items.iter().map(sorted_json).collect::<Vec<_>>().join(",")),
        other => other.to_string(),
    }
}

// The hooks Codex trusted, by key (same_key): their hash, and whether on.
// Read from its config.toml as Codex writes it: a [hooks.state.'…'] table
// for each, trusted_hash and enabled within.
fn codex_hook_states(config: &str) -> std::collections::HashMap<String, (String, bool)> {
    let mut states = std::collections::HashMap::new();
    let mut at: Option<String> = None;
    for line in config.lines().map(str::trim) {
        if line.starts_with('[') {
            at = line.strip_prefix("[hooks.state.").and_then(|rest| rest.strip_suffix(']')).and_then(toml_key).map(|k| same_key(&k));
            if let Some(key) = &at {
                states.insert(key.clone(), (String::new(), true));
            }
            continue;
        }
        let (Some(key), Some((name, value))) = (&at, line.split_once('=')) else { continue };
        let Some(state) = states.get_mut(key) else { continue };
        match name.trim() {
            "trusted_hash" => state.0 = toml_key(value.trim()).unwrap_or_default(),
            "enabled" => state.1 = value.trim() != "false",
            _ => {}
        }
    }
    states
}

// A quoted TOML key or string: 'literal', or "basic" with its escapes.
fn toml_key(text: &str) -> Option<String> {
    if let Some(rest) = text.strip_prefix('\'') {
        return rest.strip_suffix('\'').map(str::to_string);
    }
    let rest = text.strip_prefix('"')?.strip_suffix('"')?;
    let mut out = String::new();
    let mut chars = rest.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next()? {
            'n' => out.push('\n'),
            't' => out.push('\t'),
            other => out.push(other),
        }
    }
    Some(out)
}

pub fn is_codex_connected() -> bool {
    matches!(codex_hooks_status(), "ok" | "stale" | "upgradable" | "partial")
}

// Whether any of them is connected, as last looked (at start, as the
// settings read it, after connecting) or heard (any event from them): what
// the island asks for while it is not (island.js), without reading their
// settings files on every redraw. Connected until looked, so a first draw
// never asks wrongly.
static CONNECTED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(true);

pub fn note_connected(connected: bool) -> bool {
    CONNECTED.swap(connected, std::sync::atomic::Ordering::SeqCst) != connected
}

// Whether any of them is connected, read fresh from their settings files
// and the plugin's word. One list for both who look (the start, the
// settings snapshot): they must name the same agents.
pub fn any_connected(port: u16) -> bool {
    connection(port) != "none" || is_codex_connected() || is_agy_connected() || is_opencode_connected(port) || is_omp_connected(port) || is_zcode_connected() || is_dsh_connected(port) || is_reasonix_connected()
}

pub fn look_connected(port: u16) -> bool {
    let connected = any_connected(port);
    note_connected(connected);
    connected
}

pub fn seems_connected() -> bool {
    CONNECTED.load(std::sync::atomic::Ordering::SeqCst)
}

// Change Codex's hooks.json, backed up once beside itself. action: install, remove.
pub fn write_codex_hooks(action: &str, waiting: &str) -> Result<(), String> {
    let file = codex_hooks_file();
    // Read from the disk, not as last read: nothing of someone else's is written back.
    let before = read_json_now(&file)?;
    let after = match action {
        "remove" if !file.exists() => return Ok(()),
        "remove" => return write_back(&file, &uninstall(&before)),
        "install" => codex_install(&before, &codex_command(&launch_command()), waiting, codex_runs_async()),
        _ => return Err(format!("unknown action {action}")),
    };
    write_json(&file, &after)
}

// --- Antigravity: hooks in ~/.gemini/config/hooks.json --------------------------------
//
// Antigravity (Google's agy, and its 2.0 app, which reads the same file) runs
// hooks only as commands, each tool's in a block of its own by name: ours is
// "wakuwaku". Its events: SessionStart; PreInvocation, each call of the model
// (the first of a turn, invocationNum 0); PostToolUse; Stop. None for a prompt
// waiting on the person: she follows its work and its end, and asks nothing
// (events_agy.rs). Its standalone IDE runs none, as reported.
//
// Tried with agy 1.3.4 (2026-10-13): it runs a hook as `cmd /c "<command>"`,
// the command's own double quotes escaped as Go escapes them (\"), which cmd
// does not read, so a path with a space can not be quoted: its 8.3 name is
// used for one, and with none it is not installed. And a hook that fails is
// told back in cmd's code page, not UTF-8: agy's turn failed on it (invalid
// UTF-8). Ours answers at once whatever comes (main.rs agy_hook).

const AGY_BLOCK: &str = "wakuwaku";
const AGY_TIMEOUT: u64 = 10;

pub fn gemini_dir() -> PathBuf {
    PathBuf::from(std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME")).unwrap_or_default()).join(".gemini")
}

pub fn agy_hooks_file() -> PathBuf {
    gemini_dir().join("config").join("hooks.json")
}

// Whether Antigravity is on this computer: agy's folder, or the 2.0 app's
// (the standalone IDE's alone does not count: it runs no hooks).
pub fn agy_here() -> bool {
    let gemini = gemini_dir();
    gemini.join("antigravity-cli").is_dir() || gemini.join("antigravity").is_dir()
}

// A path cmd reads as one word with no quotes around it.
fn is_bare(path: &str) -> bool {
    !path.is_empty() && !path.chars().any(|c| c.is_whitespace() || "&|<>^()%!\"',;=`".contains(c))
}

#[cfg(windows)]
fn short_path(path: &str) -> Option<String> {
    #[link(name = "kernel32")]
    extern "system" {
        fn GetShortPathNameW(long: *const u16, short: *mut u16, size: u32) -> u32;
    }
    let wide: Vec<u16> = path.encode_utf16().chain(std::iter::once(0)).collect();
    let mut out = vec![0u16; 1024];
    // SAFETY: both buffers ours, the second as long as said.
    let n = unsafe { GetShortPathNameW(wide.as_ptr(), out.as_mut_ptr(), out.len() as u32) } as usize;
    (n > 0 && n < out.len()).then(|| String::from_utf16_lossy(&out[..n]))
}

#[cfg(not(windows))]
fn short_path(_path: &str) -> Option<String> {
    None
}

// This program as agy's cmd reads it: as it is, or by its 8.3 name; None
// when neither will do (a space in it, and short names turned off there).
pub fn agy_program(exe: &str) -> Option<String> {
    if is_bare(exe) {
        return Some(exe.to_string());
    }
    short_path(exe).filter(|s| is_bare(s))
}

// Our block: the event's name after the flag (the payload has none).
fn agy_block(program: &str) -> Value {
    let hook = |event: &str| json!({ "type": "command", "command": format!("{program} {AGY_FLAG} {event}"), "timeout": AGY_TIMEOUT });
    json!({
        "SessionStart": [hook("SessionStart")],
        "PreInvocation": [hook("PreInvocation")],
        "PostToolUse": [{ "matcher": "", "hooks": [hook("PostToolUse")] }],
        "Stop": [hook("Stop")],
    })
}

pub fn agy_install(file: &Value, program: &str) -> Value {
    let mut out = file.as_object().cloned().unwrap_or_default();
    out.insert(AGY_BLOCK.into(), agy_block(program));
    Value::Object(out)
}

pub fn agy_uninstall(file: &Value) -> Value {
    let mut out = file.as_object().cloned().unwrap_or_default();
    out.remove(AGY_BLOCK);
    Value::Object(out)
}

// How our block stands against what this copy would write: missing / ok /
// stale (another copy of the program, or another version's) / off (turned
// off in the file: enabled false).
pub fn agy_status_of(file: &Value, program: Option<&str>) -> &'static str {
    let Some(ours) = file.get(AGY_BLOCK) else { return "missing" };
    if ours.get("enabled").and_then(Value::as_bool) == Some(false) {
        return "off";
    }
    let mut ours = ours.clone();
    if let Some(m) = ours.as_object_mut() {
        m.remove("enabled");
    }
    match program {
        Some(p) if ours == agy_block(p) => "ok",
        _ => "stale",
    }
}

// absent (no Antigravity here), unreadable, or how our block stands.
pub fn agy_hooks_status() -> &'static str {
    if !agy_here() {
        return "absent";
    }
    match read_json(&agy_hooks_file()) {
        Ok(file) => agy_status_of(&file, agy_program(&launch_command()).as_deref()),
        Err(_) => "unreadable",
    }
}

// The tools on this computer by name, in their order (the island names
// them while none is connected): Claude Code by its folder, Codex,
// Antigravity, OpenCode, omp, ZCode, DeepSeek Harness and Reasonix by
// theirs; none found, all but omp, ZCode, DeepSeek Harness and Reasonix
// (it can not be told).
pub fn tools_here() -> Vec<&'static str> {
    let here: Vec<&str> = [("Claude Code", claude_here()), ("Codex", codex_home().is_dir()), ("Antigravity", agy_here()), ("OpenCode", opencode_here()), ("omp", omp_here()), ("ZCode", zcode_here()), ("DeepSeek Harness", dsh_here()), ("Reasonix", reasonix_here())]
        .into_iter()
        .filter(|(_, is)| *is)
        .map(|(name, _)| name)
        .collect();
    if here.is_empty() {
        vec!["Claude Code", "Codex", "Antigravity", "OpenCode"]
    } else {
        here
    }
}

pub fn is_agy_connected() -> bool {
    matches!(agy_hooks_status(), "ok" | "stale")
}

// What keeps this program from agy's hooks, as write_agy_hooks says it
// (error_kind: "path").
const AGY_NO_NAME: &str = "no name for this program without quotes";

// Change Antigravity's hooks.json, backed up once beside itself. action: install, remove.
pub fn write_agy_hooks(action: &str) -> Result<(), String> {
    let file = agy_hooks_file();
    // Read from the disk, not as last read: nothing of someone else's is written back.
    let before = read_json_now(&file)?;
    let after = match action {
        "remove" if !file.exists() => return Ok(()),
        "remove" => return write_back(&file, &agy_uninstall(&before)),
        "install" => {
            let exe = launch_command();
            let Some(program) = agy_program(&exe) else { return Err(format!("{exe}: {AGY_NO_NAME} (a space in its path, and no short name for it)")) };
            agy_install(&before, &program)
        }
        _ => return Err(format!("unknown action {action}")),
    };
    write_json(&file, &after)
}

// --- OpenCode: our plugin in its plugin folder ----------------------------------------
//
// OpenCode 2 runs no hook commands: it loads JavaScript plugins, every .js in
// <its config folder>/plugins/ by itself (and again as one changes), into its
// background service. Ours is integrations/opencode/wakuwaku.js, written
// there with her port in it: it tells her port of its sessions and asks the
// person there too (events_opencode.rs); no program of hers runs. Its config
// folder, as OpenCode 2.0.29 finds it: OPENCODE_CONFIG_DIR, else
// $XDG_CONFIG_HOME/opencode, else ~/.config/opencode (on Windows too).

const OPENCODE_PLUGIN: &str = include_str!("../../integrations/opencode/wakuwaku.js");
// The template's first line: ours, whatever port it was written with.
const OPENCODE_MARK: &str = "// wakuwaku-opencode-plugin";
const OPENCODE_PORT_LINE: &str = "const PORT = 47213";

fn home_dir() -> PathBuf {
    PathBuf::from(std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME")).unwrap_or_default())
}

fn xdg(var: &str, under_home: &[&str]) -> PathBuf {
    std::env::var_os(var).filter(|v| !v.is_empty()).map(PathBuf::from).unwrap_or_else(|| under_home.iter().fold(home_dir(), |p, part| p.join(part)))
}

pub fn opencode_dir() -> PathBuf {
    std::env::var_os("OPENCODE_CONFIG_DIR").filter(|v| !v.is_empty()).map(PathBuf::from).unwrap_or_else(|| xdg("XDG_CONFIG_HOME", &[".config"]).join("opencode"))
}

pub fn opencode_plugin_file() -> PathBuf {
    opencode_dir().join("plugins").join("wakuwaku.js")
}

// Whether OpenCode is on this computer: its config folder, or its data
// folder (~/.local/share/opencode), which it makes as it first runs.
pub fn opencode_here() -> bool {
    opencode_dir().is_dir() || xdg("XDG_DATA_HOME", &[".local", "share"]).join("opencode").is_dir()
}

// The plugin as this copy writes it: the template, her port in it.
pub fn opencode_plugin(port: u16) -> String {
    OPENCODE_PLUGIN.replacen(OPENCODE_PORT_LINE, &format!("const PORT = {port}"), 1)
}

fn is_our_plugin(text: &str) -> bool {
    text.trim_start_matches('\u{feff}').starts_with(OPENCODE_MARK)
}

// How the file stands against what this copy would write: missing (none,
// or someone else's of that name) / ok / stale (another port, another
// version of ours). Line ends aside.
pub fn opencode_status_of(text: Option<&str>, port: u16) -> &'static str {
    match text {
        Some(text) if is_our_plugin(text) => {
            if text.replace("\r\n", "\n") == opencode_plugin(port).replace("\r\n", "\n") {
                "ok"
            } else {
                "stale"
            }
        }
        _ => "missing",
    }
}

// absent (no OpenCode here), unreadable, or how our plugin stands.
pub fn opencode_status(port: u16) -> &'static str {
    let file = opencode_plugin_file();
    match fs::read_to_string(&file) {
        Ok(text) => opencode_status_of(Some(&text), port),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            if opencode_here() {
                "missing"
            } else {
                "absent"
            }
        }
        Err(_) => "unreadable",
    }
}

pub fn is_opencode_connected(port: u16) -> bool {
    matches!(opencode_status(port), "ok" | "stale")
}

// Write or take away our plugin. action: install, remove. Someone else's
// file of that name is kept (once) beside it before ours goes in its place,
// and never removed.
pub fn write_opencode_plugin(action: &str, port: u16) -> Result<(), String> {
    let file = opencode_plugin_file();
    let theirs = fs::read_to_string(&file).ok().filter(|t| !is_our_plugin(t));
    match action {
        "install" => {
            let backup = backup_of(&file);
            if theirs.is_some() && !backup.exists() {
                fs::copy(&file, &backup).map_err(|e| e.to_string())?;
            }
            if let Some(dir) = file.parent() {
                fs::create_dir_all(dir).map_err(|e| e.to_string())?;
            }
            fs::write(&file, opencode_plugin(port)).map_err(|e| e.to_string())
        }
        "remove" if theirs.is_some() || !file.exists() => Ok(()),
        "remove" => {
            fs::remove_file(&file).map_err(|e| e.to_string())?;
            // Its plugins folder too, once nothing else is in it (as omp's).
            if let Some(dir) = file.parent() {
                let _ = fs::remove_dir(dir);
            }
            Ok(())
        }
        _ => Err(format!("unknown action {action}")),
    }
}

// --- oh-my-pi (omp): an extension in its agent folder ---------------------------------
//
// omp runs no hooks: what extends it is a module it loads (in its own Bun,
// inside omp.exe) from <agent dir>/extensions/ as it starts. Ours,
// integrations/omp/wakuwaku.ts, is written there with this copy's port, exe
// and data folder filled in (an omp started after that loads it), and taken
// out again; nothing else of omp's is touched. It reports to her port from
// inside omp (events_omp.rs). The agent folder as omp 18.11.1 finds it (its
// utils/src/dirs.ts): ~/.omp/agent, PI_CONFIG_DIR in place of .omp,
// PI_CODING_AGENT_DIR in place of the whole; and a profile (omp --profile
// <name>, or OMP_PROFILE) has one of its own, ~/.omp/profiles/<name>/agent,
// which she cannot tell from here: the extension goes into each one there is.

pub const OMP_FILE: &str = "wakuwaku.ts";
const OMP_SOURCE: &str = include_str!("../../integrations/omp/wakuwaku.ts");

fn env_dir(name: &str) -> Option<PathBuf> {
    std::env::var_os(name).filter(|v| !v.is_empty()).map(PathBuf::from)
}

// omp's own folder: ~/.omp, or what PI_CONFIG_DIR names under the home folder.
pub fn omp_root() -> PathBuf {
    home_dir().join(env_dir("PI_CONFIG_DIR").unwrap_or_else(|| PathBuf::from(".omp")))
}

// Its agent folders: the one it uses with no profile, then each profile's.
pub fn omp_agent_dirs() -> Vec<PathBuf> {
    agent_dirs_in(&omp_root(), env_dir("PI_CODING_AGENT_DIR"))
}

fn agent_dirs_in(root: &Path, agent_dir: Option<PathBuf>) -> Vec<PathBuf> {
    let mut dirs = vec![agent_dir.unwrap_or_else(|| root.join("agent"))];
    let mut profiles: Vec<PathBuf> = fs::read_dir(root.join("profiles"))
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| entry.path().join("agent"))
        .filter(|agent| agent.is_dir())
        .collect();
    profiles.sort();
    for dir in profiles {
        if !dirs.iter().any(|d| same_path(&d.to_string_lossy()) == same_path(&dir.to_string_lossy())) {
            dirs.push(dir);
        }
    }
    dirs
}

pub fn omp_files() -> Vec<PathBuf> {
    omp_agent_dirs().into_iter().map(|dir| dir.join("extensions").join(OMP_FILE)).collect()
}

// Whether omp is on this computer: its folder (made as it first runs), or
// its exe where its installer puts it.
pub fn omp_here() -> bool {
    let installed = env_dir("LOCALAPPDATA").is_some_and(|dir| dir.join("omp").join("omp.exe").is_file());
    omp_root().is_dir() || omp_agent_dirs()[0].is_dir() || installed
}

// The extension as a copy of her on this port, this exe and this data
// folder ("" for her own) writes it.
pub fn omp_extension(port: u16, exe: &str, data: &str) -> String {
    OMP_SOURCE
        .replace("\"__WAKUWAKU_PORT__\"", &format!("\"{port}\""))
        .replace("\"__WAKUWAKU_EXE__\"", &json!(exe).to_string())
        .replace("\"__WAKUWAKU_DATA__\"", &json!(data).to_string())
}

// What this copy writes: her data folder only when she was given one
// (WAKUWAKU_USER_DATA, a test's copy), for the one the extension starts.
fn omp_wanted(port: u16) -> String {
    omp_extension(port, &launch_command(), &std::env::var("WAKUWAKU_USER_DATA").unwrap_or_default())
}

// How the files stand against what this copy writes (None: no file in that
// folder): missing (in none) / ok / stale (another port or copy, an older
// version) / partial (a profile made since lacks it).
pub fn omp_status_of(found: &[Option<String>], want: &str) -> &'static str {
    if found.iter().all(Option::is_none) {
        "missing"
    } else if found.iter().flatten().any(|text| text != want) {
        "stale"
    } else if found.iter().all(Option::is_some) {
        "ok"
    } else {
        "partial"
    }
}

// absent (no omp here), unreadable, or how the extension stands.
pub fn omp_hooks_status(port: u16) -> &'static str {
    if !omp_here() {
        return "absent";
    }
    let mut found = Vec::new();
    for file in omp_files() {
        match fs::read_to_string(&file) {
            Ok(text) => found.push(Some(text)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => found.push(None),
            Err(_) => return "unreadable",
        }
    }
    omp_status_of(&found, &omp_wanted(port))
}

pub fn is_omp_connected(port: u16) -> bool {
    matches!(omp_hooks_status(port), "ok" | "stale" | "partial")
}

// Write the extension into every agent folder, or take it out of each (its
// extensions folder too, once empty). action: install, remove.
pub fn write_omp_hooks(action: &str, port: u16) -> Result<(), String> {
    write_omp_files(&omp_files(), action, &omp_wanted(port))
}

fn write_omp_files(files: &[PathBuf], action: &str, want: &str) -> Result<(), String> {
    match action {
        "install" => {
            for file in files {
                if let Some(dir) = file.parent() {
                    fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
                }
                fs::write(file, want).map_err(|e| format!("{}: {e}", file.display()))?;
            }
            Ok(())
        }
        "remove" => {
            for file in files {
                match fs::remove_file(file) {
                    Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(format!("{}: {e}", file.display())),
                    _ => {}
                }
                if let Some(dir) = file.parent() {
                    let _ = fs::remove_dir(dir);
                }
            }
            Ok(())
        }
        _ => Err(format!("unknown action {action}")),
    }
}

// --- ZCode: hooks in ~/.zcode/cli/config.json -----------------------------------------
//
// ZCode (Z.ai's agent: its desktop app, whose engine is its CLI bundled,
// and the CLI) runs the hooks in the user's config.json, under hooks.events,
// once hooks.enabled is true: no trust asked for (a workspace's own
// .zcode/config.json is, again after each change, so ours go in the user's).
// Its 7 events, each with Claude Code's fields beside its own camelCase ones
// (events_zcode.rs). A hook is a command for a shell or, ours, a program and
// its arguments (process: no shell, no quoting); none over HTTP. So this
// program with ZCODE_FLAG and the event's name hands each to her port
// (main.rs zcode_hook), and prints her answer to a PermissionRequest in
// Claude Code's words. ZCode shows its own dialog at the same time and
// takes the first answer: hers, or the person's there (which ends our hook).
// The others answer at once; ZCode waits on each (no background for a
// process hook), some 0.1 s. A hook may be turned off one by one in ZCode's
// settings (enabled: false), which is off here too.
//
// Tried with the desktop app 3.15.3 (2026-10-15): it reads the file for each
// new session, no restart needed.

pub const ZCODE_FLAG: &str = "--wakuwaku-zcode-hook";
// ZCode's 7 events, and how long each may run (ms): the prompt waits for the
// person, past her own wait (asks.rs ASK_TIMEOUT_MS); ZCode's default is 60 s.
pub const ZCODE_PROMPT_TIMEOUT_MS: u64 = 600_000;
const ZCODE_TIMEOUT_MS: u64 = 10_000;
const ZCODE_EVENTS: [&str; 7] = ["SessionStart", "UserPromptSubmit", "PreToolUse", "PermissionRequest", "PostToolUse", "PostToolUseFailure", "Stop"];

fn zcode_timeout(event: &str) -> u64 {
    if event == "PermissionRequest" { ZCODE_PROMPT_TIMEOUT_MS } else { ZCODE_TIMEOUT_MS }
}

pub fn zcode_dir() -> PathBuf {
    PathBuf::from(std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME")).unwrap_or_default()).join(".zcode")
}

pub fn zcode_config_file() -> PathBuf {
    zcode_dir().join("cli").join("config.json")
}

// Whether ZCode is on this computer: its folder (made as it first runs).
pub fn zcode_here() -> bool {
    zcode_dir().is_dir()
}

fn zcode_hook(exe: &str, event: &str) -> Value {
    json!({ "type": "process", "command": exe, "args": [ZCODE_FLAG, event], "timeoutMs": zcode_timeout(event) })
}

pub fn zcode_install(file: &Value, exe: &str) -> Value {
    let root = file.get("hooks").cloned().unwrap_or_else(|| json!({}));
    let mut events = strip(root.get("events"));
    for event in ZCODE_EVENTS {
        let mut all = events.get(event).and_then(Value::as_array).cloned().unwrap_or_default();
        all.push(json!({ "matcher": "*", "hooks": [zcode_hook(exe, event)] }));
        events.insert(event.into(), Value::Array(all));
    }
    let mut root = root.as_object().cloned().unwrap_or_default();
    root.insert("enabled".into(), json!(true));
    root.insert("events".into(), Value::Object(events));
    let mut out = file.as_object().cloned().unwrap_or_default();
    out.insert("hooks".into(), Value::Object(root));
    Value::Object(out)
}

// Ours taken out, and hooks.enabled as it was before we turned it on
// (enabled_before: what it was, null for none; None when we did not, or
// cannot tell: then it stays, unless no hook is left to run). An events
// left empty goes, and the hooks with it when nothing else is there.
pub fn zcode_uninstall(file: &Value, enabled_before: Option<&Value>) -> Value {
    let mut out = file.as_object().cloned().unwrap_or_default();
    let Some(Value::Object(root)) = file.get("hooks") else { return Value::Object(out) };
    let mut root = root.clone();
    let events = strip(root.get("events"));
    match enabled_before {
        Some(Value::Null) => drop(root.remove("enabled")),
        Some(was) => drop(root.insert("enabled".into(), was.clone())),
        None if events.is_empty() => drop(root.remove("enabled")),
        None => {}
    }
    if events.is_empty() {
        root.remove("events");
    } else {
        root.insert("events".into(), Value::Object(events));
    }
    if root.is_empty() {
        out.remove("hooks");
    } else {
        out.insert("hooks".into(), Value::Object(root));
    }
    Value::Object(out)
}

// Where she keeps what hooks.enabled was before she turned it on, for
// putting it back as she is removed: in her data folder (ZCode's file takes
// no field it does not know).
fn zcode_note() -> PathBuf {
    crate::data::folder().join("zcode-hooks.json")
}

// How our hooks stand against what this copy would write: missing / ok /
// stale (another copy of the program, or another version's) / partial (some
// events lack them) / off (hooks turned off, all of them or one of ours).
pub fn zcode_status_of(file: &Value, exe: &str) -> &'static str {
    let root = file.get("hooks").unwrap_or(&Value::Null);
    let mut ours: Vec<(String, Value)> = Vec::new();
    for (event, groups) in root.get("events").and_then(Value::as_object).into_iter().flatten() {
        for group in groups.as_array().into_iter().flatten() {
            for hook in group.get("hooks").and_then(Value::as_array).into_iter().flatten().filter(|h| is_ours(h)) {
                ours.push((event.clone(), hook.clone()));
            }
        }
    }
    if ours.is_empty() {
        return "missing";
    }
    if root.get("enabled").and_then(Value::as_bool) != Some(true) || ours.iter().any(|(_, h)| h.get("enabled").and_then(Value::as_bool) == Some(false)) {
        return "off";
    }
    let is_want = |event: &str, hook: &Value| {
        let mut hook = hook.clone();
        if let Some(h) = hook.as_object_mut() {
            h.remove("enabled");
            if let Some(command) = h.get("command").and_then(Value::as_str).map(same_path) {
                h.insert("command".into(), json!(command));
            }
        }
        hook == zcode_hook(&same_path(exe), event)
    };
    if !ours.iter().all(|(event, hook)| is_want(event, hook)) {
        return "stale";
    }
    if ZCODE_EVENTS.iter().all(|want| ours.iter().any(|(event, _)| event == want)) {
        "ok"
    } else {
        "partial"
    }
}

// absent (no ZCode here), unreadable, or how our hooks stand.
pub fn zcode_hooks_status() -> &'static str {
    if !zcode_here() {
        return "absent";
    }
    match read_json(&zcode_config_file()) {
        Ok(file) => zcode_status_of(&file, &launch_command()),
        Err(_) => "unreadable",
    }
}

pub fn is_zcode_connected() -> bool {
    matches!(zcode_hooks_status(), "ok" | "stale" | "partial")
}

// Change ZCode's config.json, backed up once beside itself. action: install, remove.
pub fn write_zcode_hooks(action: &str) -> Result<(), String> {
    let file = zcode_config_file();
    // Read from the disk, not as last read: nothing of someone else's is written back.
    let before = read_json_now(&file)?;
    let note = zcode_note();
    match action {
        "remove" if !file.exists() => Ok(()),
        "remove" => {
            let was = read_json_now(&note).ok().and_then(|n| n.get("enabledBefore").cloned());
            write_back(&file, &zcode_uninstall(&before, was.as_ref()))?;
            let _ = fs::remove_file(&note);
            Ok(())
        }
        "install" => {
            // Turned on by us: what it was, kept from the first install on.
            let enabled = before.pointer("/hooks/enabled").cloned().unwrap_or(Value::Null);
            if enabled != json!(true) && !note.exists() {
                fs::write(&note, json!({ "enabledBefore": enabled }).to_string()).map_err(|e| e.to_string())?;
            }
            write_json(&file, &zcode_install(&before, &launch_command()))
        }
        _ => Err(format!("unknown action {action}")),
    }
}

// --- DeepSeek Harness: her own plugin, inserted by its path ----------------------------
//
// DeepSeek Harness (dsh: its desktop app and its command line) has no hooks
// file of its own: everything in it is a Cordis plugin, mounted by the rows
// of a profile's patch layers. Ours is integrations/dsh/wakuwaku.js, a small
// plugin with no dependencies that runs in DSH's own process and tells her
// port of its sessions (events_dsh.rs reads them). Install writes it, with a
// package.json of its own (an ES module asking nothing of DSH's versions),
// into a folder in DSH's home, <DSH home>/wakuwaku/, and inserts a row for
// it into the home-level patch layer ($DSH_HOME/cordis.patch.yml, ~/.dsh by
// default) by its path beside that file (./wakuwaku/wakuwaku.js), her port
// its config. DSH turns such a path into a file URL and imports it as it is
// (app-boot's anchorInsertedPluginNames, 0.2.0-rc.2 and 0.2.1-rc.2 alike):
// no package name to resolve, nothing in a profile's node_modules, so no
// pnpm run of DSH's plugin manager can take it away, and nothing is
// downloaded. The home layer goes over every profile's own (the desktop
// app's "desktop", the command line's default, any other) and outranks
// them, and DSH never rewrites it (its self-repair renames a profile's own
// patch file only). Our rows sit between two comment lines of ours, so they
// are found and taken out again without a YAML parser.
//
// Until 0.3.2 the row was DSH's own Claude Code hook bridge
// (@deepseek-ai/dsh-hooks-claude-code), which ran this program with DSH_FLAG
// through PowerShell for each event (some 0.9 s each, DSH waiting), its hook
// list in her data folder. The desktop app 0.2.0-rc.2 carried that bridge;
// 0.2.1 no longer does (its experimental packages left the installation), so
// the import failed, DSH only warned, and no event ever came while her
// Connect page said connected (zhoupengjie/Wakuwaku#61). Those rows read as
// stale here (why: bridge): Repair puts the plugin in their place and takes
// the hook list away. The flag still works, for a DSH still running them.
// With the plugin her program's own path no longer matters (it talks HTTP
// to her port): an exe of hers renamed, moved or carrying its version in its
// name breaks nothing.
//
// Written is not loaded. The plugin says it was, itself, as DSH loads it: a
// file beside it (loaded-<profile>.json: the process, when, which version),
// taken away again as it is let go. From those and DSH's running processes
// her Connect page tells loaded, waiting for DSH to start, waiting for a
// restart (DSH was running before, or runs an older version of it), or not
// loaded although DSH has been up for a while since (dsh_load_of): what is
// so, not "connected" because a file was written. DSH watches its patch
// layers (dsh-hmr), so a row added while it runs may load at once.

pub const DSH_FLAG: &str = "--wakuwaku-dsh-hook";
const DSH_PLUGIN: &str = include_str!("../../integrations/dsh/wakuwaku.js");
const DSH_PACKAGE: &str = include_str!("../../integrations/dsh/package.json");
// Its folder in DSH's home, its file there, and the row's name for it: its
// path beside the patch file.
const DSH_DIR: &str = "wakuwaku";
const DSH_PLUGIN_FILE: &str = "wakuwaku.js";
const DSH_ROW_NAME: &str = "./wakuwaku/wakuwaku.js";
// The other way's rows insert DSH's own Claude Code hook bridge (and 0.3.2's did).
const DSH_BRIDGE: &str = crate::dsh_bridge::HOOKS_PACKAGE;
// The two ways' rows by ids of their own (the bridge's as 0.3.2 wrote it):
// DSH's loader, as a running row of the same id changes its name, keeps the
// plugin it has and only gives it the new config, so a switch while DSH runs
// went on with the way left (seen on 0.2.1-rc.2); a row gone and another
// come, it lets the one go and loads the other.
const DSH_PLUGIN_ID: &str = "wakuwaku-plugin";
const DSH_BRIDGE_ID: &str = "wakuwaku";
// Our rows in the patch file, between these lines.
const DSH_BEGIN: &str = "# wakuwaku begin";
const DSH_END: &str = "# wakuwaku end";
// How long DSH may take from starting to loading its plugins before ours,
// not loaded, is said to be so.
const DSH_LOAD_GRACE_MS: u64 = 60_000;
// DSH's desktop app, as its processes are named (its Host, which loads the
// plugins, is one of them).
const DSH_APP: &str = "DeepSeek Harness.exe";

pub fn dsh_home() -> PathBuf {
    match std::env::var_os("DSH_HOME").filter(|h| !h.is_empty()) {
        Some(home) => PathBuf::from(home),
        None => PathBuf::from(std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME")).unwrap_or_default()).join(".dsh"),
    }
}

// Whether DeepSeek Harness is on this computer: its folder (made as it first runs).
pub fn dsh_here() -> bool {
    dsh_home().is_dir()
}

pub fn dsh_patch_file() -> PathBuf {
    dsh_home().join("cordis.patch.yml")
}

pub fn dsh_plugin_dir() -> PathBuf {
    dsh_home().join(DSH_DIR)
}

pub fn dsh_plugin_file() -> PathBuf {
    dsh_plugin_dir().join(DSH_PLUGIN_FILE)
}

// The old bridge's hook list in her data folder: no longer written, taken
// away as the plugin is installed or removed.
pub fn dsh_hooks_file() -> PathBuf {
    crate::data::folder().join("dsh-hooks.json")
}

// The plugin's version, as its source says it (export const VERSION = n).
pub fn dsh_plugin_version() -> u32 {
    DSH_PLUGIN.lines().find_map(|l| l.trim().strip_prefix("export const VERSION =")).and_then(|v| v.trim().trim_end_matches(';').parse().ok()).unwrap_or(0)
}

// Our rows: the plugin inserted by its path, her port its config.
fn dsh_block(port: u16) -> String {
    format!(
        "{DSH_BEGIN}: Wakuwaku follows DeepSeek Harness through her own plugin in ./wakuwaku (remove it from her settings)\n\
         - insert:\n    - id: {DSH_PLUGIN_ID}\n      name: '{DSH_ROW_NAME}'\n      config:\n        port: {port}\n{DSH_END}\n"
    )
}

// What our rows in a patch file are: none, the bridge's (the hook list it
// was pointed at), or the plugin's (the name it was inserted by, and the
// port in its config).
#[derive(Clone, Debug, PartialEq)]
pub enum DshRows {
    None,
    Bridge { config_path: String },
    Plugin { name: String, port: Option<u16> },
}

// A YAML scalar as written plainly or in quotes.
fn unquote(value: &str) -> String {
    let value = value.trim();
    if let Some(inner) = value.strip_prefix('\'').and_then(|v| v.strip_suffix('\'')) {
        return inner.replace("''", "'");
    }
    if let Some(inner) = value.strip_prefix('"').and_then(|v| v.strip_suffix('"')) {
        return inner.to_string();
    }
    value.to_string()
}

// The text without our rows, and what they were.
fn dsh_strip(text: &str) -> (String, DshRows) {
    let mut out = String::new();
    let (mut inside, mut rows) = (false, DshRows::None);
    for line in text.split_inclusive('\n') {
        let bare = line.trim();
        if !inside && bare.starts_with(DSH_BEGIN) {
            inside = true;
            rows = DshRows::Plugin { name: String::new(), port: None };
            continue;
        }
        if inside {
            if bare.starts_with(DSH_END) {
                inside = false;
            } else if let Some(value) = bare.strip_prefix("configPath:") {
                rows = DshRows::Bridge { config_path: unquote(value) };
            } else if let Some(value) = bare.strip_prefix("name:") {
                let given = unquote(value);
                rows = match rows {
                    DshRows::Bridge { config_path } => DshRows::Bridge { config_path },
                    _ if given == DSH_BRIDGE => DshRows::Bridge { config_path: String::new() },
                    DshRows::Plugin { port, .. } => DshRows::Plugin { name: given, port },
                    other => other,
                };
            } else if let Some(value) = bare.strip_prefix("port:") {
                if let DshRows::Plugin { port, .. } = &mut rows {
                    *port = value.trim().parse().ok();
                }
            }
            continue;
        }
        out.push_str(line);
    }
    (out, rows)
}

// The rows of a YAML list a patch file has, past its comments and blank lines.
fn dsh_content(text: &str) -> Vec<&str> {
    text.lines().map(str::trim).filter(|l| !l.is_empty() && !l.starts_with('#')).collect()
}

// The text without a row that only turns ours off (`id:` either way's and
// `disabled: true`, nothing else): Repair puts it back to work.
fn without_off_rows(text: &str) -> String {
    let mut out = String::new();
    let mut item = String::new();
    let flush = |item: &mut String, out: &mut String| {
        let fields: Vec<&str> = item.lines().map(|l| l.trim().trim_start_matches("- ").trim()).filter(|l| !l.is_empty() && !l.starts_with('#')).collect();
        let only_off = fields.len() == 2 && (row_disabled(item, DSH_PLUGIN_ID) || row_disabled(item, DSH_BRIDGE_ID));
        if !only_off {
            out.push_str(item);
        }
        item.clear();
    };
    for line in text.split_inclusive('\n') {
        let top = line.starts_with("- ");
        let under = line.starts_with(' ') && !item.is_empty();
        if top {
            flush(&mut item, &mut out);
            item.push_str(line);
        } else if under {
            item.push_str(line);
        } else {
            flush(&mut item, &mut out);
            out.push_str(line);
        }
    }
    flush(&mut item, &mut out);
    out
}

// The patch file with the plugin's rows.
pub fn dsh_install(text: Option<&str>, port: u16) -> Result<String, String> {
    dsh_insert(text, &dsh_block(port))
}

// The patch file with the bridge's rows, pointed at a hook list.
pub fn dsh_install_bridge(text: Option<&str>, config_path: &str) -> Result<String, String> {
    dsh_insert(text, &dsh_bridge_block(config_path))
}

// The patch file with these rows of ours (in place of ours before, the
// other way's too, and of a row that turned ours off): added after the
// list's last row, or in place of an empty list ([]); the list as a YAML
// flow sequence of rows ([ ... ]) is not one to add to.
fn dsh_insert(text: Option<&str>, block: &str) -> Result<String, String> {
    let rest = without_off_rows(&dsh_strip(text.unwrap_or("")).0);
    let block = block.to_string();
    let content = dsh_content(&rest);
    if content.is_empty() {
        return Ok(if rest.trim().is_empty() { block } else { format!("{}{block}", with_newline(&rest)) });
    }
    if content == ["[]"] {
        let mut out = String::new();
        for line in rest.split_inclusive('\n') {
            out.push_str(if line.trim() == "[]" { &block } else { line });
        }
        return Ok(out);
    }
    if content[0].starts_with('[') {
        return Err(DSH_FLAT.to_string());
    }
    Ok(format!("{}{block}", with_newline(&rest)))
}

fn with_newline(text: &str) -> String {
    if text.is_empty() || text.ends_with('\n') { text.to_string() } else { format!("{text}\n") }
}

// The patch file without our rows: None when nothing else was in it (the
// file goes); an empty list ([]) when only comments are left, which DSH
// would refuse to start with.
pub fn dsh_uninstall(text: &str) -> Option<String> {
    let (rest, _) = dsh_strip(text);
    if rest.trim().is_empty() {
        return None;
    }
    if dsh_content(&rest).is_empty() {
        return Some(format!("{}[]\n", with_newline(&rest)));
    }
    Some(rest)
}

// How our rows and the plugin's files stand against what this copy writes:
// missing / ok / stale, and why stale: "bridge" (the bridge's rows: DSH's
// Claude Code hook bridge, 0.3.2's or the other way's), "row" (inserted
// otherwise, or for another port: another copy of hers), "files" (the
// plugin's files gone, or not as this version writes them).
pub fn dsh_status_of(text: &str, files_ok: bool, port: u16) -> (&'static str, &'static str) {
    match dsh_strip(text).1 {
        DshRows::None => ("missing", ""),
        DshRows::Bridge { .. } => ("stale", "bridge"),
        DshRows::Plugin { name, port: given } if name != DSH_ROW_NAME || given != Some(port) => ("stale", "row"),
        DshRows::Plugin { .. } if !files_ok => ("stale", "files"),
        DshRows::Plugin { .. } => ("ok", ""),
    }
}

// The plugin's files as this version writes them (line ends aside).
fn dsh_files() -> [(&'static str, &'static str); 2] {
    [("package.json", DSH_PACKAGE), (DSH_PLUGIN_FILE, DSH_PLUGIN)]
}

fn same_text(file: &Path, want: &str) -> bool {
    fs::read_to_string(file).is_ok_and(|text| text.replace("\r\n", "\n") == want.replace("\r\n", "\n"))
}

fn dsh_files_ok(dir: &Path) -> bool {
    dsh_files().iter().all(|(name, want)| same_text(&dir.join(name), want))
}

// --- The other way: DSH's own Claude Code hook bridge (dshVia "bridge") ---------------
//
// The person may pick it on the Agent page (the setting dshVia, the plugin
// by default): her rows insert DSH's Claude Code hook bridge
// (@deepseek-ai/dsh-hooks-claude-code) pointed at a hooks.json in her data
// folder (dsh-hooks.json), as 0.3.2 wrote them, whose command hooks run
// this program with DSH_FLAG and the event's name (main.rs dsh_hook). The
// bridge needs its packages in each profile from DSH 0.2.1 on
// (dsh_bridge.rs puts them there, through DSH's own pnpm, and takes those it
// added away again). It runs each hook as a command through DSH's shell,
// which on Windows is PowerShell (pwsh 7, else Windows PowerShell), in
// DSH's sandbox (a write-restricted, low-integrity token: the hook may read
// and open a socket, but not write; so it never starts her, who would run
// so too). A hook costs some 0.9 s there (desktop app 0.2.0-rc.2 in the
// test VM, Windows PowerShell 5.1), and DSH waits on each but
// SessionStart's: so only a turn's start and end, and the few tools worth it
// (an edit, the to-do list, a question to the person, PostToolUse; the
// question's start, PreToolUse) have one, by name. No approval comes this
// way (the bridge has no PermissionRequest). The rows and the hook list
// name this program's path: as she starts, they are written again for
// where she is now (dsh_at_start).

const DSH_TIMEOUT: u64 = 15;
// The tools whose end (and the question's start) is told: DSH's own names.
pub const DSH_POST_TOOLS: &str = "write|edit|str_replace_editor|todo_write|ask_user_question";
const DSH_PRE_TOOLS: &str = "ask_user_question";

// The command DSH's PowerShell runs for an event: the bare path when it
// needs no quoting, else as PowerShell calls a quoted program; its output
// piped away, so that PowerShell waits for it (it does not for a program
// with a window of its own, as this one is, and was gone before her server
// could see where the hook ran: server.rs chain_for).
pub fn dsh_command(exe: &str, event: &str) -> String {
    let is_plain = !exe.is_empty() && exe.chars().all(|c| c.is_ascii_alphanumeric() || "\\/:._-".contains(c));
    if is_plain {
        format!("{exe} {DSH_FLAG} {event} | Out-Null")
    } else {
        format!("& '{}' {DSH_FLAG} {event} | Out-Null", exe.replace('\'', "''"))
    }
}

// The hooks.json the bridge runs.
pub fn dsh_hooks(exe: &str) -> Value {
    let hook = |event: &str| json!([{ "type": "command", "command": dsh_command(exe, event), "timeout": DSH_TIMEOUT }]);
    json!({ "hooks": {
        "SessionStart": [{ "hooks": hook("SessionStart") }],
        "UserPromptSubmit": [{ "hooks": hook("UserPromptSubmit") }],
        "PreToolUse": [{ "matcher": DSH_PRE_TOOLS, "hooks": hook("PreToolUse") }],
        "PostToolUse": [{ "matcher": DSH_POST_TOOLS, "hooks": hook("PostToolUse") }],
        "Stop": [{ "hooks": hook("Stop") }],
    } })
}

// The bridge's rows, pointed at the hooks.json (0.3.2's, word for word).
fn dsh_bridge_block(config_path: &str) -> String {
    format!(
        "{DSH_BEGIN}: Wakuwaku follows DeepSeek Harness through its Claude Code hook bridge (remove it from her settings)\n\
         - insert:\n    - id: {DSH_BRIDGE_ID}\n      name: '{DSH_BRIDGE}'\n      config:\n        configPath: '{}'\n{DSH_END}\n",
        config_path.replace('\'', "''")
    )
}

// The way chosen: dshVia when set; else as the rows have it (the bridge's
// rows: the bridge, as 0.3.2 left them while they work; anything else the plugin).
pub fn dsh_way(setting: &Value, rows: &DshRows) -> &'static str {
    match setting.as_str() {
        Some("bridge") => "bridge",
        Some("plugin") => "plugin",
        _ if matches!(rows, DshRows::Bridge { .. }) => "bridge",
        _ => "plugin",
    }
}

// How the bridge's rows, hook list and packages stand against what this
// copy writes: missing / ok / stale, why: "way" (the plugin's rows there
// instead), "hooks" (the rows or the hook list not as this copy writes
// them: her exe or data folder elsewhere), "packages" (not in a profile),
// "version" (another DSH version's).
pub fn dsh_bridge_status_of(rows: &DshRows, config_path: &str, hooks: Option<&Value>, want: &Value, packages: &str) -> (&'static str, &'static str) {
    match rows {
        DshRows::None => ("missing", ""),
        DshRows::Plugin { .. } => ("stale", "way"),
        DshRows::Bridge { config_path: given } if same_path(given) != same_path(config_path) || hooks != Some(want) => ("stale", "hooks"),
        DshRows::Bridge { .. } if packages == "missing" => ("stale", "packages"),
        DshRows::Bridge { .. } if packages == "version" => ("stale", "version"),
        DshRows::Bridge { .. } => ("ok", ""),
    }
}

// The DSH version the bridge's packages must have: its desktop app's.
pub fn dsh_desktop_version() -> Option<String> {
    versions().get("dsh").and_then(Value::as_str).map(str::to_string)
}

// How the bridge's packages stand: ok (or carried by the app itself, as
// 0.2.0-rc.2 does), missing, version (dsh_bridge.rs packages_state).
pub fn dsh_packages() -> &'static str {
    if crate::dsh_bridge::app_dir().is_some_and(|app| crate::dsh_bridge::bundled(&app)) {
        return "ok";
    }
    let found = crate::dsh_bridge::found_in(&crate::dsh_bridge::profiles(&dsh_home()));
    match dsh_desktop_version() {
        Some(version) => crate::dsh_bridge::packages_state(&found, &version),
        None if !found.is_empty() && found.iter().flatten().all(Option::is_some) => "ok",
        None => "missing",
    }
}

// The bridge written (its hook list's time: `written`) and DSH: waiting (DSH
// not running), effective (a hook of it came since DSH started and since
// the bridge was put in: what her plugin told before a switch is not the
// bridge's), firstEvent (DSH up, none yet). DSH's warning when it can't
// import the bridge goes to its Host's stderr only (the desktop app keeps no
// log file of it), so this is what can be told.
pub fn dsh_bridge_load(apps: &[u64], seen_at: u64, written: u64) -> &'static str {
    match apps.iter().min() {
        None => "waiting",
        Some(&first) if seen_at >= first.max(written) => "effective",
        Some(_) => "firstEvent",
    }
}

// The way chosen (dsh_way), and how its rows and files stand: absent (no
// DeepSeek Harness here), unreadable, or missing / ok / stale and why
// (dsh_status_of, dsh_bridge_status_of), DSH's loading aside.
pub fn dsh_rows_status(port: u16, setting: &Value) -> (&'static str, &'static str, &'static str) {
    if !dsh_here() {
        return (dsh_way(setting, &DshRows::None), "absent", "");
    }
    let text = match fs::read_to_string(dsh_patch_file()) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(_) => return (dsh_way(setting, &DshRows::None), "unreadable", ""),
    };
    let rows = dsh_strip(&text).1;
    let way = dsh_way(setting, &rows);
    let (status, why) = if way == "bridge" {
        let hooks = read_json(&dsh_hooks_file()).ok();
        let packages = if matches!(rows, DshRows::Bridge { .. }) { dsh_packages() } else { "ok" };
        dsh_bridge_status_of(&rows, &dsh_hooks_file().to_string_lossy(), hooks.as_ref(), &dsh_hooks(&launch_command()), packages)
    } else {
        dsh_status_of(&text, dsh_files_ok(&dsh_plugin_dir()), port)
    };
    (way, status, why)
}

pub fn is_dsh_connected(port: u16) -> bool {
    matches!(dsh_rows_status(port, &Value::Null).1, "ok" | "stale")
}

// The plugin's word that DSH loaded it (loaded-<profile>.json beside it):
// which process, when (ms since 1970), which version, which code (its
// file's digest as DSH imported it: dsh_plugin_sha), in which profile.
#[derive(Clone, Debug, PartialEq)]
pub struct DshLoaded {
    pub pid: u32,
    pub at: u64,
    pub version: u32,
    pub sha: String,
    pub profile: String,
}

// The plugin's code as this version writes it, as the plugin tells its own
// (SHA-256 of the file's text with CRLF made LF, as a checkout may have
// either, its first 16 hex digits): DSH keeps a module it imported until it
// restarts, so the same name written again is not what runs until then.
pub fn dsh_plugin_sha() -> String {
    sha_of_text(DSH_PLUGIN)
}

fn sha_of_text(text: &str) -> String {
    use sha2::{Digest, Sha256};
    Sha256::digest(text.replace("\r\n", "\n").as_bytes()).iter().take(8).map(|b| format!("{b:02x}")).collect()
}

pub fn dsh_markers(dir: &Path) -> Vec<DshLoaded> {
    let mut out = Vec::new();
    for entry in fs::read_dir(dir).into_iter().flatten().flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if !(name.starts_with("loaded-") && name.ends_with(".json")) {
            continue;
        }
        let Some(word) = fs::read_to_string(entry.path()).ok().and_then(|t| serde_json::from_str::<Value>(&t).ok()) else { continue };
        let (Some(pid), Some(at)) = (word["pid"].as_u64(), word["at"].as_u64()) else { continue };
        let text = |key: &str| word[key].as_str().unwrap_or("").to_string();
        out.push(DshLoaded { pid: pid as u32, at, version: word["version"].as_u64().unwrap_or(0) as u32, sha: text("sha"), profile: text("profile") });
    }
    out
}

// How DSH stands with the plugin written.
#[derive(Clone, Debug, PartialEq)]
pub enum DshLoad {
    // Loaded by a process still running (the latest such).
    Loaded(DshLoaded),
    // Loaded, but an older version than the one written: a restart takes the new one.
    Older(DshLoaded),
    // DSH's app running since before the plugin was written: a restart loads it.
    Restart,
    // DSH's app up a while since it was written, and it did not load it.
    NotLoaded,
    // DSH not running, or only starting: it loads it as it starts.
    Waiting,
}

impl DshLoad {
    pub fn word(&self) -> &'static str {
        match self {
            DshLoad::Loaded(_) => "loaded",
            DshLoad::Older(_) => "older",
            DshLoad::Restart => "restart",
            DshLoad::NotLoaded => "notLoaded",
            DshLoad::Waiting => "waiting",
        }
    }
}

// From the markers, when each marker's process started (None: gone; ms
// since 1970), when DSH's app's processes started, when the plugin was
// written, now, and the code written (its digest; a word without one, by
// its version).
pub fn dsh_load_of(markers: &[DshLoaded], started: impl Fn(u32) -> Option<u64>, apps: &[u64], written: u64, now: u64, sha: &str, version: u32) -> DshLoad {
    // Of a process still running that was there before its word (its number not reused).
    let live: Vec<&DshLoaded> = markers.iter().filter(|m| started(m.pid).is_some_and(|at| at <= m.at + 1_000)).collect();
    let current = |m: &&&DshLoaded| if m.sha.is_empty() { m.version == version } else { m.sha == sha };
    if let Some(m) = live.iter().filter(current).max_by_key(|m| m.at) {
        return DshLoad::Loaded((*m).clone());
    }
    if let Some(m) = live.iter().max_by_key(|m| m.at) {
        return DshLoad::Older((*m).clone());
    }
    match apps.iter().min() {
        None => DshLoad::Waiting,
        Some(&first) if first < written => DshLoad::Restart,
        Some(&first) if now.saturating_sub(first) >= DSH_LOAD_GRACE_MS => DshLoad::NotLoaded,
        Some(_) => DshLoad::Waiting,
    }
}

// Whether a patch file turns a row of this id off: a row of its list (not
// one inside another's insert) with `id: <id>` and `disabled: true`, as
// DSH's plugin settings write one.
fn row_disabled(text: &str, id: &str) -> bool {
    let (mut is_row, mut off) = (false, false);
    for line in text.lines() {
        let bare = line.trim();
        if bare.is_empty() || bare.starts_with('#') {
            continue;
        }
        // A row's own fields: on its "- " line, or under it by two.
        let field = match line.len() - line.trim_start_matches(' ').len() {
            0 => {
                if is_row && off {
                    return true;
                }
                (is_row, off) = (false, false);
                match bare.strip_prefix("- ") {
                    Some(rest) => rest.trim(),
                    None => continue,
                }
            }
            2 => bare,
            _ => continue,
        };
        if let Some((key, value)) = field.split_once(':') {
            match key.trim() {
                "id" => is_row = unquote(value) == id,
                "disabled" => off = value.trim() == "true",
                _ => {}
            }
        }
    }
    is_row && off
}

// Whether the home-level patch file turns our row off, past our own rows:
// only a patch applied after our insert finds it (a profile's own patch
// file is applied before the home one, and a row of it naming ours finds
// none yet).
fn dsh_turned_off() -> bool {
    fs::read_to_string(dsh_patch_file()).is_ok_and(|text| row_disabled(&dsh_strip(&text).0, DSH_PLUGIN_ID))
}

fn ms_of(time: std::time::SystemTime) -> u64 {
    time.duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis() as u64)
}

// The Connect page's block for DSH, past its files (settings.rs): the way
// (plugin or bridge, and the setting as it is), hooks absent / unreadable /
// missing / ok / stale / notLoaded, why (stale: bridge, row, files; the
// bridge's: way, hooks, packages, version; notLoaded: off, or "" for no
// reason known), and how DSH stands with it (load: the plugin's loaded,
// older, restart, notLoaded, waiting, with when, which version, which
// profile once loaded; the bridge's waiting, effective, firstEvent).
// seen_at: when a DSH event last came (ms since 1970, 0 for none).
pub fn dsh_view(port: u16, setting: &Value, seen_at: u64) -> Value {
    let (way, status, why) = dsh_rows_status(port, setting);
    let mut view = json!({ "way": way, "via": setting, "hooks": status, "why": why, "dshVersion": dsh_desktop_version() });
    if status != "ok" {
        return view;
    }
    if way == "bridge" {
        let written = fs::metadata(dsh_hooks_file()).and_then(|m| m.modified()).map_or(0, ms_of);
        view["load"] = json!({ "state": dsh_bridge_load(&crate::jump::started_named(DSH_APP), seen_at, written) });
        return view;
    }
    let dir = dsh_plugin_dir();
    let written = fs::metadata(dir.join(DSH_PLUGIN_FILE)).and_then(|m| m.modified()).map_or(0, ms_of);
    let load = dsh_load_of(&dsh_markers(&dir), crate::jump::started_ms, &crate::jump::started_named(DSH_APP), written, ms_of(std::time::SystemTime::now()), &dsh_plugin_sha(), dsh_plugin_version());
    view["load"] = json!({ "state": load.word() });
    if let DshLoad::Loaded(m) | DshLoad::Older(m) = &load {
        view["load"] = json!({ "state": load.word(), "at": m.at, "version": m.version, "profile": m.profile, "pid": m.pid });
    }
    if load == DshLoad::NotLoaded {
        view["hooks"] = json!("notLoaded");
        view["why"] = json!(if dsh_turned_off() { "off" } else { "" });
    }
    view
}

// What keeps our rows out of a patch file, as write_dsh_hooks says it (error_kind: "flowList").
const DSH_FLAT: &str = "its list is written as [ … ], not a row a line: our rows cannot be added to it";

// Write the plugin's files where they are not as this version writes them,
// or all of them (`all`: the row changed, so their time says when it was
// written: dsh_load_of).
fn write_dsh_files(dir: &Path, all: bool) -> Result<(), String> {
    fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    for (name, want) in dsh_files() {
        let file = dir.join(name);
        if all || !same_text(&file, want) {
            fs::write(&file, want).map_err(|e| format!("{}: {e}", file.display()))?;
        }
    }
    Ok(())
}

// Take the plugin's files away (its markers too), and its folder once
// nothing else is in it.
fn remove_dsh_files(dir: &Path) -> Result<(), String> {
    let mut names: Vec<String> = dsh_files().iter().map(|(name, _)| name.to_string()).collect();
    for entry in fs::read_dir(dir).into_iter().flatten().flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with("loaded-") && name.ends_with(".json") {
            names.push(name);
        }
    }
    for name in names {
        match fs::remove_file(dir.join(&name)) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(format!("{}: {e}", dir.join(&name).display())),
            _ => {}
        }
    }
    let _ = fs::remove_dir(dir);
    Ok(())
}

// What keeps the bridge's packages out of a profile, as write_dsh_hooks says
// it: this, the kind in brackets (dsh_bridge.rs classify, runtime, busy),
// then what pnpm said (error_kind: "bridge" and the kind).
pub const DSH_BRIDGE_FAILED: &str = "the hook bridge's packages were not installed";

// Her note of the bridge's packages she added to each profile's
// package.json (in her data folder): those she takes away again with it.
fn dsh_bridge_note() -> PathBuf {
    crate::data::folder().join("dsh-bridge.json")
}

fn noted_packages() -> Vec<(PathBuf, Vec<String>)> {
    let Ok(note) = read_json_now(&dsh_bridge_note()) else { return Vec::new() };
    note.get("added")
        .and_then(Value::as_object)
        .into_iter()
        .flatten()
        .map(|(profile, names)| (PathBuf::from(profile), names.as_array().into_iter().flatten().filter_map(Value::as_str).map(str::to_string).collect()))
        .collect()
}

// Added to the note (as JSON values: no map type of its own for it).
fn note_packages(added: &[(PathBuf, Vec<String>)]) {
    let mut all = read_json_now(&dsh_bridge_note()).ok().and_then(|n| n.get("added").cloned()).filter(Value::is_object).unwrap_or_else(|| json!({}));
    for (profile, names) in added.iter().filter(|(_, names)| !names.is_empty()) {
        let key = profile.to_string_lossy();
        let had = &mut all[key.as_ref()];
        if !had.is_array() {
            *had = json!([]);
        }
        for name in names {
            let list = had.as_array_mut().expect("an array");
            if !list.iter().any(|n| n == name.as_str()) {
                list.push(json!(name));
            }
        }
    }
    if all.as_object().is_none_or(|m| m.is_empty()) {
        let _ = fs::remove_file(dsh_bridge_note());
    } else {
        let _ = write_json(&dsh_bridge_note(), &json!({ "added": all }));
    }
}

// Take the bridge's leftovers away: its hook list, and the packages she
// added (best effort: one left behind does no harm, and says why in her log).
fn remove_bridge_leftovers(log: &mut Vec<String>) {
    let _ = fs::remove_file(dsh_hooks_file());
    let noted = noted_packages();
    if noted.is_empty() {
        return;
    }
    let runtime = crate::dsh_bridge::app_dir().and_then(|app| crate::dsh_bridge::runtime_in(&app));
    let mut kept = Vec::new();
    for (profile, names) in noted {
        let gone = match &runtime {
            Some(runtime) => crate::dsh_bridge::remove(runtime, &profile, &names),
            None => Err(crate::dsh_bridge::Failure { kind: "runtime", detail: String::new() }),
        };
        match gone {
            Ok(()) => log.push(format!("dsh: the hook bridge's packages taken out of {}", profile.display())),
            Err(why) => {
                log.push(format!("dsh: the hook bridge's packages left in {} ({}: {})", profile.display(), why.kind, why.detail));
                kept.push((profile, names));
            }
        }
    }
    let _ = fs::remove_file(dsh_bridge_note());
    note_packages(&kept);
}

fn read_patch(file: &Path) -> Result<Option<String>, String> {
    match fs::read_to_string(file) {
        Ok(text) => Ok(Some(text)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.to_string()),
    }
}

// Change DSH's home-level patch file (backed up once beside itself), the
// plugin's folder in DSH's home, and the bridge's hook list in her data
// folder and its packages in DSH's profiles, for a way (plugin, bridge).
// action: install (that way, the other taken away once it is in), remove
// (either). registry: the one the bridge's packages come from, when the
// person picked one (NPMMIRROR); else pnpm's own. Lines for her log in `log`.
pub fn write_dsh_hooks(action: &str, way: &str, port: u16, registry: Option<&str>, log: &mut Vec<String>) -> Result<(), String> {
    let file = dsh_patch_file();
    let text = read_patch(&file)?;
    // A copy of the person's own: none of a file of ours alone.
    let backup = backup_of(&file);
    if text.as_deref().is_some_and(|t| !dsh_strip(t).0.trim().is_empty()) && !backup.exists() {
        fs::copy(&file, &backup).map_err(|e| e.to_string())?;
    }
    match (action, way) {
        ("remove", _) => {
            // The row first: DSH lets the plugin go as it sees it gone.
            if let Some(text) = text.filter(|t| dsh_strip(t).1 != DshRows::None) {
                match dsh_uninstall(&text) {
                    Some(rest) => fs::write(&file, rest).map_err(|e| e.to_string())?,
                    None => fs::remove_file(&file).map_err(|e| e.to_string())?,
                }
            }
            remove_bridge_leftovers(log);
            remove_dsh_files(&dsh_plugin_dir())
        }
        ("install", "bridge") => {
            // The packages first: should they fail, nothing else has changed.
            let app = crate::dsh_bridge::app_dir();
            if !app.as_deref().is_some_and(crate::dsh_bridge::bundled) {
                let failed = |kind: &str, detail: &str| format!("{DSH_BRIDGE_FAILED} [{kind}]: {detail}");
                let runtime = app.as_deref().and_then(crate::dsh_bridge::runtime_in).ok_or_else(|| failed("runtime", "DeepSeek Harness's own Node and pnpm were not found"))?;
                let version = dsh_desktop_version().ok_or_else(|| failed("runtime", "DeepSeek Harness's version could not be read"))?;
                let mut added = Vec::new();
                for profile in crate::dsh_bridge::profiles(&dsh_home()) {
                    match crate::dsh_bridge::install(&runtime, &profile, &version, registry) {
                        Ok(names) => {
                            log.push(format!("dsh: the hook bridge's packages {version} in {}{}", profile.display(), if names.is_empty() { " (there already)" } else { "" }));
                            added.push((profile, names));
                        }
                        Err(why) => {
                            note_packages(&added);
                            return Err(failed(why.kind, &why.detail));
                        }
                    }
                }
                note_packages(&added);
            }
            let hooks = dsh_hooks_file();
            let list = serde_json::to_string_pretty(&dsh_hooks(&launch_command())).map_err(|e| e.to_string())?;
            fs::write(&hooks, format!("{list}\n")).map_err(|e| e.to_string())?;
            forget_read(&hooks);
            let after = dsh_install_bridge(text.as_deref(), &hooks.to_string_lossy())?;
            if text.as_deref() != Some(after.as_str()) {
                fs::write(&file, after).map_err(|e| e.to_string())?;
            }
            remove_dsh_files(&dsh_plugin_dir())
        }
        ("install", _) => {
            let after = dsh_install(text.as_deref(), port)?;
            let changed = text.as_deref() != Some(after.as_str());
            // The files first: DSH may load the plugin as soon as its row is there.
            write_dsh_files(&dsh_plugin_dir(), changed)?;
            if changed {
                fs::write(&file, after).map_err(|e| e.to_string())?;
            }
            remove_bridge_leftovers(log);
            Ok(())
        }
        _ => Err(format!("unknown action {action}")),
    }
}

// What a start sees to (on her own port, with her own data folder: a test's
// copy changes nothing of DSH's), lines for her log:
// - 0.3.2's bridge rows whose packages are missing (DSH 0.2.1 and on), the
//   way not chosen: her plugin in their place (the patch file kept beside
//   itself first): the default the user chose for those upgrading
//   (zhoupengjie/Wakuwaku#61);
// - the bridge as the way (chosen, or 0.3.2's rows that still work): its
//   rows and hook list written again for where this program is now (an exe
//   renamed or moved, one carrying its version in its name); its packages,
//   which need the network, are left to the person's Repair;
// - the plugin as the way: its files written again where an update of hers
//   changed them (DSH takes the new one as it restarts).
// What a start does (dsh_at_start), from the rows, the setting, and lazily
// how the bridge's packages stand: migrate (to the plugin), bridge (its
// rows and hook list again), plugin (its files again, when not as written),
// none.
pub fn dsh_start_plan(rows: &DshRows, setting: &Value, packages: impl FnOnce() -> &'static str, port: u16, plugin_files_ok: bool) -> &'static str {
    match (rows, dsh_way(setting, rows)) {
        (DshRows::Bridge { .. }, "bridge") if setting.is_null() && packages() != "ok" => "migrate",
        (DshRows::Bridge { .. }, "bridge") => "bridge",
        (DshRows::Plugin { name, port: given }, "plugin") if name == DSH_ROW_NAME && *given == Some(port) && !plugin_files_ok => "plugin",
        _ => "none",
    }
}

pub fn dsh_at_start(port: u16, setting: &Value) -> Vec<String> {
    let mut log = Vec::new();
    if !dsh_here() {
        return log;
    }
    let file = dsh_patch_file();
    let Ok(Some(text)) = read_patch(&file) else { return log };
    let rows = dsh_strip(&text).1;
    match (dsh_start_plan(&rows, setting, dsh_packages, port, dsh_files_ok(&dsh_plugin_dir())), &rows) {
        ("migrate", _) => {
            let backup = backup_of(&file);
            if !backup.exists() {
                let _ = fs::copy(&file, &backup);
            }
            match write_dsh_hooks("install", "plugin", port, None, &mut log) {
                Ok(()) => log.push(format!("dsh: 0.3.2's hook bridge rows, its packages not there: her plugin in their place (backup beside it: {})", backup.display())),
                Err(e) => log.push(format!("dsh: 0.3.2's hook bridge rows not replaced: {e}")),
            }
        }
        ("bridge", DshRows::Bridge { config_path }) => {
            let hooks = dsh_hooks_file();
            let want = dsh_hooks(&launch_command());
            if read_json_now(&hooks).ok().as_ref() != Some(&want) {
                match serde_json::to_string_pretty(&want).map_err(|e| e.to_string()).and_then(|list| fs::write(&hooks, format!("{list}\n")).map_err(|e| e.to_string())) {
                    Ok(()) => log.push(format!("dsh: the hook bridge's hook list written again for {}", launch_command())),
                    Err(e) => log.push(format!("dsh: the hook bridge's hook list not written again: {e}")),
                }
                forget_read(&hooks);
            }
            if same_path(config_path) != same_path(&hooks.to_string_lossy()) {
                if let Ok(after) = dsh_install_bridge(Some(&text), &hooks.to_string_lossy()) {
                    match fs::write(&file, after) {
                        Ok(()) => log.push(format!("dsh: the hook bridge's rows pointed at {}", hooks.display())),
                        Err(e) => log.push(format!("dsh: the hook bridge's rows not written again: {e}")),
                    }
                }
            }
        }
        ("plugin", _) => match write_dsh_files(&dsh_plugin_dir(), false) {
            Ok(()) => log.push("dsh: her plugin's files written again for this version (DSH takes it as it restarts)".to_string()),
            Err(e) => log.push(format!("dsh: her plugin's files not written again: {e}")),
        },
        _ => {}
    }
    log
}

// --- Reasonix: hooks in its global settings.json ----------------------------------------
//
// Reasonix (DeepSeek-Reasonix: Reasonix Studio, its Electron desktop app,
// whose kernel reasonix-studio-host.exe is its Go engine, and the 2.x
// command line; zhoupengjie/Wakuwaku#60) runs the hooks in <its home>\
// settings.json for every session of either, with no trust asked for (a
// workspace's own .reasonix\settings.json is, as a program approved by its
// fingerprint, again after each change: ours go in the global one). Its
// schema is its own, not Claude Code's: { "hooks": { "<Event>": [{ "match",
// "command", "timeout" (ms), "description", "cwd", "env" }] } }, a hook a
// row, one written Claude Code's way (matcher, nested hooks) skipped
// without a word, and a file that does not parse (a comment, a timeout as
// text) taken as no hooks at all. Read as a session's runtime is built (the
// command line started, a new Studio session or pane, /reload; not /new):
// nothing to restart, but sessions open before it do not have ours.
//
// Each hook is a shell command, run and waited for (no background for the
// settings' hooks), its payload on stdin: on Windows `cmd.exe /d /s /c
// "<command>"` with no window (reasonix_command), so this program with
// REASONIX_FLAG and the event's name hands each to her port and is gone
// (main.rs reasonix_hook), some 0.1 s. cmd waits for it, a program with a
// window of its own as it is; one that holds the hook's output open after it
// (a child that inherited it) is a failure, which on its gating events
// (PreToolUse, UserPromptSubmit), as a timeout is, blocks the call or the
// prompt: she starts with no handle of the hook's (detach.rs). A hook that
// cannot run (this program gone, its hooks left) fails in cmd with 1: a
// warning in Reasonix, no block. What a hook prints is Reasonix's: put in the
// model's context at SessionStart, so ours prints nothing; and a
// PermissionRequest of the settings' hooks can only be told, not answered
// (a Claude Code-shaped hook of a plugin package can, before its dialog:
// zhoupengjie/Wakuwaku#60's phase 2).
//
// Tried against its source, the studio branch at 2.43 (2026-10-17); its Go
// 1.x reads the same file the same way (no SubagentStart), its Node 0.x only
// ~/.reasonix/settings.json, 4 of these events and no session id
// (reasonix_line).

pub const REASONIX_FLAG: &str = "--wakuwaku-reasonix-hook";
// The events she follows, each with a hook of ours; how long each may run
// (ms): Reasonix waits on every one of them, and one of its gating events
// (PreToolUse, UserPromptSubmit) timing out blocks the call or the prompt,
// so no less than its own default for them (5 s). Ours is gone in some
// 0.1 s.
pub const REASONIX_EVENTS: [&str; 10] = ["SessionStart", "SessionEnd", "UserPromptSubmit", "PreToolUse", "PostToolUse", "PostToolUseFailure", "PermissionRequest", "Notification", "Stop", "StopFailure"];
const REASONIX_TIMEOUT_MS: u64 = 5_000;
// What `/hooks` in Reasonix names ours by.
const REASONIX_DESCRIPTION: &str = "Wakuwaku: tells her how this session is doing";

// Its home, as its config package's ReasonixHomeDir finds it: REASONIX_HOME,
// else %APPDATA%\reasonix on Windows (~\AppData\Roaming\reasonix without
// %APPDATA%), else ~/.reasonix.
pub fn reasonix_home() -> PathBuf {
    if let Some(home) = env_dir("REASONIX_HOME") {
        return home;
    }
    if cfg!(windows) {
        env_dir("APPDATA").unwrap_or_else(|| home_dir().join("AppData").join("Roaming")).join("reasonix")
    } else {
        home_dir().join(".reasonix")
    }
}

// The older home's settings.json (~/.reasonix), which Reasonix reads in
// place of its home's while that one is not there; none while REASONIX_HOME
// isolates it, or where the two are the same.
fn reasonix_legacy_file() -> Option<PathBuf> {
    if env_dir("REASONIX_HOME").is_some() {
        return None;
    }
    let legacy = home_dir().join(".reasonix");
    (same_path(&legacy.to_string_lossy()) != same_path(&reasonix_home().to_string_lossy())).then(|| legacy.join("settings.json"))
}

// The settings.json Reasonix reads its global hooks from: its home's, or
// the older one while that is missing and the older one is there (hook.go
// Load). Ours are always written into its home's, which then is the one
// read: made from the older one's (the person's hooks there carried over,
// as Reasonix would no longer read them), and taken away again on a remove
// that leaves it as that one is.
pub fn reasonix_settings_file() -> PathBuf {
    reasonix_file_of(reasonix_home().join("settings.json"), reasonix_legacy_file())
}

fn reasonix_file_of(home_file: PathBuf, legacy: Option<PathBuf>) -> PathBuf {
    match legacy {
        Some(legacy) if !home_file.exists() && legacy.is_file() => legacy,
        _ => home_file,
    }
}

// Whether Reasonix is on this computer: its home (made as it first runs),
// or a Reasonix 0.x (whose home is ~/.reasonix: reasonix_line).
pub fn reasonix_here() -> bool {
    reasonix_home().is_dir() || reasonix_line() == Some(0)
}

// Its line, the major version of the Reasonix found here (versions): 2 for
// Reasonix Studio and the 2.x command line, 1 for the Go 1.x (its desktop
// app and command line), 0 for the Node 0.x. 1.x and 2.x run the same
// hooks from the same settings.json, with the same payload (1.x has no
// SubagentStart, which she does not hook); 0.x reads only
// ~/.reasonix/settings.json, runs 4 of the events and gives no session id:
// not one she writes to. None where no file says.
pub fn reasonix_line() -> Option<u64> {
    versions()["reasonix"].as_str().and_then(|v| v.split('.').next()?.parse().ok())
}

// A Reasonix 0.x alone: here, and nothing of hers written to it.
fn reasonix_unsupported() -> bool {
    !reasonix_home().is_dir() && reasonix_line() == Some(0)
}

// The command line Reasonix runs for an event. Its settings' hooks are
// shell commands: on Windows, one like ours goes to `cmd.exe /d /s /c
// "<command>"` as it is written (its hook package's windowsCmdCommandLine),
// so the program is in double quotes (a space in its path is fine); single
// quotes, $ and ` would send it to Git Bash instead (requiresPOSIXShell),
// and % or ! are cmd's own: a path with any of those, or a quote, gets
// none. Elsewhere it goes to sh -c, in single quotes.
pub fn reasonix_command(exe: &str, event: &str) -> Option<String> {
    if exe.is_empty() {
        return None;
    }
    if cfg!(windows) {
        (!exe.chars().any(|c| "\"'$`%!".contains(c) || c.is_control())).then(|| format!("\"{exe}\" {REASONIX_FLAG} {event}"))
    } else {
        Some(format!("'{}' {REASONIX_FLAG} {event}", exe.replace('\'', r"'\''")))
    }
}

fn reasonix_hook(command: String) -> Value {
    json!({ "command": command, "timeout": REASONIX_TIMEOUT_MS, "description": REASONIX_DESCRIPTION })
}

// Its hooks without ours: each event's list (Reasonix's is flat, a hook a
// row, its tools by `match`), events left empty taken out.
fn reasonix_strip(hooks: Option<&Value>) -> Map<String, Value> {
    let mut out = Map::new();
    let Some(Value::Object(hooks)) = hooks else { return out };
    for (event, list) in hooks {
        let Value::Array(list) = list else {
            out.insert(event.clone(), list.clone());
            continue;
        };
        let kept: Vec<Value> = list.iter().filter(|h| !is_ours(h)).cloned().collect();
        if !kept.is_empty() {
            out.insert(event.clone(), Value::Array(kept));
        }
    }
    out
}

// Its settings.json with ours (in place of ours before), after the person's
// own of each event; everything else in it kept. None: no command for this
// program (reasonix_command).
pub fn reasonix_install(file: &Value, exe: &str) -> Option<Value> {
    let mut hooks = reasonix_strip(file.get("hooks"));
    for event in REASONIX_EVENTS {
        let mut list = hooks.get(event).and_then(Value::as_array).cloned().unwrap_or_default();
        list.push(reasonix_hook(reasonix_command(exe, event)?));
        hooks.insert(event.into(), Value::Array(list));
    }
    let mut out = file.as_object().cloned().unwrap_or_default();
    out.insert("hooks".into(), Value::Object(hooks));
    Some(Value::Object(out))
}

// Ours taken out; the hooks too when nothing else is left in them.
pub fn reasonix_uninstall(file: &Value) -> Value {
    let mut out = file.as_object().cloned().unwrap_or_default();
    let hooks = reasonix_strip(file.get("hooks"));
    if hooks.is_empty() {
        out.remove("hooks");
    } else {
        out.insert("hooks".into(), Value::Object(hooks));
    }
    Value::Object(out)
}

// How our hooks stand against what this copy would write: missing / ok /
// stale (another copy of the program, or another version's) / partial (some
// events lack them).
pub fn reasonix_status_of(file: &Value, exe: &str) -> &'static str {
    let mut ours: Vec<(String, Value)> = Vec::new();
    for (event, list) in file.get("hooks").and_then(Value::as_object).into_iter().flatten() {
        for hook in list.as_array().into_iter().flatten().filter(|h| is_ours(h)) {
            ours.push((event.clone(), hook.clone()));
        }
    }
    if ours.is_empty() {
        return "missing";
    }
    let is_want = |event: &str, hook: &Value| {
        let Some(command) = reasonix_command(exe, event) else { return false };
        let mut hook = hook.clone();
        if let Some(h) = hook.as_object_mut() {
            if let Some(found) = h.get("command").and_then(Value::as_str).map(same_path) {
                h.insert("command".into(), json!(found));
            }
        }
        hook == reasonix_hook(same_path(&command))
    };
    if !ours.iter().all(|(event, hook)| REASONIX_EVENTS.contains(&event.as_str()) && is_want(event, hook)) {
        return "stale";
    }
    if REASONIX_EVENTS.iter().all(|want| ours.iter().any(|(event, _)| event == want)) {
        "ok"
    } else {
        "partial"
    }
}

// A JSON file as Reasonix reads it: a byte order mark first is no error.
fn read_json_bom(file: &Path) -> Result<Value, String> {
    match fs::read_to_string(file) {
        Ok(text) => serde_json::from_str(text.trim_start_matches('\u{feff}')).map_err(|e| e.to_string()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(json!({})),
        Err(e) => Err(e.to_string()),
    }
}

// absent (no Reasonix here), unsupported (a Reasonix 0.x alone:
// reasonix_line), unreadable, or how our hooks stand in the settings.json it
// reads.
pub fn reasonix_hooks_status() -> &'static str {
    if !reasonix_here() {
        return "absent";
    }
    if reasonix_unsupported() {
        return "unsupported";
    }
    match read_json_bom(&reasonix_settings_file()) {
        Ok(found) => reasonix_status_of(&found, &launch_command()),
        Err(_) => "unreadable",
    }
}

pub fn is_reasonix_connected() -> bool {
    matches!(reasonix_hooks_status(), "ok" | "stale" | "partial")
}

// Change Reasonix's global settings.json, backed up once beside itself.
// action: install (into its home's, made from the older one's while it has
// none: reasonix_settings_file), remove (from the one it reads; its home's
// taken away again when what is left is the older one's as it is, and
// nothing of the person's was there before). Never into a Reasonix 0.x.
pub fn write_reasonix_hooks(action: &str) -> Result<(), String> {
    if action == "install" && reasonix_unsupported() {
        return Err(format!("{}: {REASONIX_OLD}", reasonix_legacy_file().unwrap_or_default().display()));
    }
    write_reasonix_files(action, &reasonix_home().join("settings.json"), reasonix_legacy_file(), &launch_command())
}

fn is_empty_object(value: &Value) -> bool {
    value.as_object().is_some_and(|o| o.is_empty())
}

// The same between its home's file and the older one (None: none read).
fn write_reasonix_files(action: &str, home_file: &Path, legacy: Option<PathBuf>, exe: &str) -> Result<(), String> {
    let legacy = legacy.filter(|l| l.is_file());
    match action {
        "remove" => {
            let file = reasonix_file_of(home_file.to_path_buf(), legacy.clone());
            if !file.exists() {
                return Ok(());
            }
            // Read from the disk, not as last read: nothing of someone else's is written back.
            let after = reasonix_uninstall(&read_json_bom(&file)?);
            let backup = backup_of(&file);
            // A copy that held nothing but hers (an older version's write
            // over a file of hers alone): no copy of the person's, gone too.
            if backup.exists() && read_json_bom(&backup).is_ok_and(|kept| is_empty_object(&reasonix_uninstall(&kept))) {
                fs::remove_file(&backup).map_err(|e| e.to_string())?;
            }
            let as_older = legacy.as_ref().filter(|_| file == home_file).and_then(|l| read_json_bom(l).ok()).is_some_and(|older| older == after);
            if as_older && !backup.exists() {
                let gone = fs::remove_file(&file).map_err(|e| e.to_string());
                forget_read(&file);
                return gone;
            }
            write_back(&file, &after)
        }
        "install" => {
            let before = match (home_file.exists(), &legacy) {
                (false, Some(older)) => read_json_bom(older)?,
                _ => read_json_bom(home_file)?,
            };
            let Some(after) = reasonix_install(&before, exe) else { return Err(format!("{exe}: {REASONIX_NO_NAME}")) };
            // Its home's holding nothing but ours (a repair, ours moved): no
            // copy of it kept, as none of it is the person's (a copy would
            // then keep a remove from giving the file back as it was).
            if home_file.exists() && is_empty_object(&reasonix_uninstall(&before)) {
                return write_back(home_file, &after);
            }
            write_json(home_file, &after)
        }
        _ => Err(format!("unknown action {action}")),
    }
}

// Reasonix's version as its files say it (for versions): Reasonix Studio's
// app (2.x; Electron's, per user, where its installer puts it by default),
// else the 1.x desktop app's (Reasonix, its exe in app\ or beside), else its
// npm package's (2.x's launcher of its Go CLI, 1.x's, or the Node 0.x).
// The 2.x command line from its zip carries none (a Go exe, anywhere).
fn reasonix_version(in_local: &dyn Fn(&[&str]) -> Option<PathBuf>, in_npm: &dyn Fn(&[&str]) -> Option<PathBuf>) -> Option<String> {
    let app = |rest: &[&str]| in_local(rest).and_then(|f| file_version(&f, "Reasonix"));
    app(&["Programs", "Reasonix Studio", "Reasonix Studio.exe"])
        .or_else(|| app(&["Programs", "Reasonix", "app", "Reasonix.exe"]))
        .or_else(|| app(&["Programs", "Reasonix", "Reasonix.exe"]))
        .or_else(|| in_npm(&["reasonix"]).and_then(|f| package_version(&f)))
}

const REASONIX_NO_NAME: &str = "no command line for this program that cmd and Reasonix both read as it is (a quote, $, `, % or ! in its path)";
const REASONIX_OLD: &str = "a Reasonix 0.x, which runs hooks of another kind from here: nothing written";

// --- The agents' versions, as their files say them -------------------------------------
//
// For the Agent page (src/pet/agents.js), where one is cheap to know: the
// version a program file carries (Claude Code's own claude.exe, ZCode's and
// DeepSeek Harness's desktop apps), taken only when the file names that
// product (omp.exe, built by Bun, says Bun's own, 1.4.3, for omp 18.11), or
// its npm package's package.json (Claude Code's, OpenCode's), each where it
// installs by default. Never by running it (Codex's comes from `codex
// --version`, asked already: codex_version). None where no file says;
// Antigravity's agy and omp's carry none of their own. Read at most once a
// minute.

// Its version as its file says it ("0.2.0-rc.2", "2.1.296"): the
// FileVersion text's first word (Windows' own add a build's name), four
// parts cut to three (ZCode's 3.15.3.8237 is its 3.15.3).
pub fn tidy_version(text: &str) -> Option<String> {
    let text = text.trim_end_matches('\0').split_whitespace().next().unwrap_or("");
    if text.is_empty() || !text.starts_with(|c: char| c.is_ascii_digit()) || text.len() > 40 {
        return None;
    }
    let parts: Vec<&str> = text.split('.').collect();
    Some(if parts.len() == 4 && parts.iter().all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit())) { parts[..3].join(".") } else { text.to_string() })
}

// A program file's version, when its ProductName holds product.
#[cfg(windows)]
fn file_version(path: &Path, product: &str) -> Option<String> {
    use std::ffi::c_void;
    use std::os::windows::ffi::OsStrExt;
    #[link(name = "version")]
    extern "system" {
        fn GetFileVersionInfoSizeW(name: *const u16, handle: *mut u32) -> u32;
        fn GetFileVersionInfoW(name: *const u16, handle: u32, len: u32, data: *mut c_void) -> i32;
        fn VerQueryValueW(block: *const c_void, sub: *const u16, buffer: *mut *mut c_void, len: *mut u32) -> i32;
    }
    let wide = |s: &std::ffi::OsStr| s.encode_wide().chain(std::iter::once(0)).collect::<Vec<u16>>();
    let file = wide(path.as_os_str());
    // SAFETY: a buffer of the size the call asks for; the values read point
    // into it and are copied before it goes.
    unsafe {
        let size = GetFileVersionInfoSizeW(file.as_ptr(), std::ptr::null_mut());
        if size == 0 {
            return None;
        }
        let mut data = vec![0u8; size as usize];
        if GetFileVersionInfoW(file.as_ptr(), 0, size, data.as_mut_ptr() as *mut c_void) == 0 {
            return None;
        }
        let (mut at, mut len): (*mut c_void, u32) = (std::ptr::null_mut(), 0);
        let translation = wide(std::ffi::OsStr::new("\\VarFileInfo\\Translation"));
        if VerQueryValueW(data.as_ptr() as *const c_void, translation.as_ptr(), &mut at, &mut len) == 0 || len < 4 {
            return None;
        }
        let (lang, page) = (*(at as *const u16), *(at as *const u16).add(1));
        let text_of = |name: &str| {
            let key = wide(std::ffi::OsStr::new(&format!("\\StringFileInfo\\{lang:04x}{page:04x}\\{name}")));
            let (mut at, mut len): (*mut c_void, u32) = (std::ptr::null_mut(), 0);
            (VerQueryValueW(data.as_ptr() as *const c_void, key.as_ptr(), &mut at, &mut len) != 0 && len > 1)
                .then(|| String::from_utf16_lossy(std::slice::from_raw_parts(at as *const u16, len as usize)))
        };
        if !text_of("ProductName").is_some_and(|name| name.to_lowercase().contains(&product.to_lowercase())) {
            return None;
        }
        tidy_version(&text_of("FileVersion")?)
    }
}

#[cfg(not(windows))]
fn file_version(_path: &Path, _product: &str) -> Option<String> {
    None
}

// An npm package's version, from its package.json.
fn package_version(file: &Path) -> Option<String> {
    let text = fs::read_to_string(file).ok()?;
    let v: Value = serde_json::from_str(&text).ok()?;
    tidy_version(v.get("version")?.as_str()?)
}

pub fn versions() -> Value {
    static KNOWN: Mutex<Option<(Instant, Value)>> = Mutex::new(None);
    let mut known = KNOWN.lock().unwrap_or_else(|e| e.into_inner());
    if let Some((_, v)) = known.as_ref().filter(|(at, _)| at.elapsed() < Duration::from_secs(60)) {
        return v.clone();
    }
    let local = env_dir("LOCALAPPDATA");
    let npm = env_dir("APPDATA").map(|a| a.join("npm").join("node_modules"));
    let in_local = |rest: &[&str]| local.as_ref().map(|l| rest.iter().fold(l.clone(), |p, part| p.join(part)));
    let in_npm = |rest: &[&str]| npm.as_ref().map(|n| rest.iter().fold(n.clone(), |p, part| p.join(part)).join("package.json"));
    let claude = file_version(&home_dir().join(".local").join("bin").join(if cfg!(windows) { "claude.exe" } else { "claude" }), "Claude Code")
        .or_else(|| in_npm(&["@anthropic-ai", "claude-code"]).and_then(|f| package_version(&f)));
    let v = json!({
        "claude": claude,
        // OpenCode 2 is @opencode/cli (2.0.29 in the test VM); before it, opencode-ai.
        "opencode": in_npm(&["@opencode", "cli"]).and_then(|f| package_version(&f)).or_else(|| in_npm(&["opencode-ai"]).and_then(|f| package_version(&f))),
        "zcode": in_local(&["Programs", "ZCode", "ZCode.exe"]).and_then(|f| file_version(&f, "ZCode")),
        "dsh": in_local(&["Programs", "DeepSeek Harness", "DeepSeek Harness.exe"]).and_then(|f| file_version(&f, "DeepSeek Harness")),
        "reasonix": reasonix_version(&in_local, &in_npm),
    });
    *known = Some((Instant::now(), v.clone()));
    v
}

// --- Start at login: this program, from where it is now -------------------------------
//
// The switch is the Run value (HKCU ...\CurrentVersion\Run, Wakuwaku): this
// program with AT_LOGIN_FLAG and "run". Task Manager's Startup apps list it
// and may turn it off there (StartupApproved\Run, by the same name), which
// is off here too. With it a logon task (logon.rs, with "task"), whatever
// her home: Windows starts the Run items one by one once Explorer is up,
// late and held back (three minutes after a logon on the user's laptop,
// zhoupengjie/Wakuwaku#44), while the task starts her as the user logs on.
// The Run value stays for when the task does not run; whichever of the two
// comes second finds her up and leaves without a word (any other start
// brings her back into sight).
//
// Kept to where she is: as she starts on her own port (not a test's copy on
// another, nor one with a data folder of its own), a Run value naming
// another exe (gone: renamed or moved by hand, a download kept under its
// own name; or another copy of her) is written anew for this one, and the
// task made, repaired or removed to match. Never turned on where it was off.
// Run from the source tree (cargo, its target folder), she writes no build's
// path over another exe's; her own value she keeps up.

pub const AT_LOGIN_FLAG: &str = "--wakuwaku-at-login";
pub const BY_TASK: &str = "task";
const BY_RUN: &str = "run";

// Uninstalling (scripts/installer.mjs): her logon task, when it is this
// installation's own, taken away before the uninstaller deletes her program.
// The task's name holds the user's SID, which the uninstaller cannot work
// out, so it asks her.
pub const UNINSTALL_CLEANUP_FLAG: &str = "--wakuwaku-uninstall-cleanup";

// Started at login: by her logon task (task) or the Run value (run); none
// when started any other way.
pub fn started_by(args: &[String]) -> Option<&'static str> {
    let at = args.iter().position(|a| a == AT_LOGIN_FLAG)?;
    Some(if args.get(at + 1).map(String::as_str) == Some(BY_TASK) { BY_TASK } else { BY_RUN })
}

// That, for her log: ", by her logon task", ", by the Run value", or nothing.
pub fn said_by(by: Option<&str>) -> &'static str {
    match by {
        Some(BY_TASK) => ", by her logon task",
        Some(_) => ", by the Run value",
        None => "",
    }
}

// Start at login's decisions, plain (tested below; acted on, on Windows alone).
#[cfg_attr(not(windows), allow(dead_code))]
mod rules {
    use super::{same_path, AT_LOGIN_FLAG, BY_RUN, BY_TASK};
    use crate::logon::TaskSeen;

    // The Run value for this program, and what its logon task gives it.
    pub fn run_command(exe: &str) -> String {
        format!("\"{exe}\" {AT_LOGIN_FLAG} {BY_RUN}")
    }

    pub fn task_args() -> String {
        format!("{AT_LOGIN_FLAG} {BY_TASK}")
    }

    // The program a command line starts: its first word, in quotes or not
    // (a path with spaces and no quotes, up to its .exe).
    pub fn program_of(command: &str) -> &str {
        let command = command.trim();
        if let Some(rest) = command.strip_prefix('"') {
            return rest.split('"').next().unwrap_or("");
        }
        match command.to_ascii_lowercase().find(".exe") {
            Some(at) => &command[..at + 4],
            None => command.split_whitespace().next().unwrap_or(""),
        }
    }

    // Start at login as it stands: off (no Run value, or turned off in Task
    // Manager), on for this program, or on for another exe.
    #[derive(Debug, PartialEq, Clone, Copy)]
    pub enum Login {
        Off,
        Mine,
        Theirs,
    }

    pub fn login_of(value: Option<&str>, turned_off: bool, exe: &str) -> Login {
        match value {
            None => Login::Off,
            Some(_) if turned_off => Login::Off,
            Some(v) if same_path(program_of(v)) == same_path(exe) => Login::Mine,
            Some(_) => Login::Theirs,
        }
    }

    // What her start does with the Run value: keeps it, or writes this
    // program's in its place, and why (the exe it named is gone, it is
    // another copy of her, or it is this one as an older version wrote it).
    // No value stays none: start at login turned off stays off. Run from
    // the source tree, she takes no other exe's.
    #[derive(Debug, PartialEq)]
    pub enum RunFix {
        Keep,
        Rewrite(&'static str),
    }

    pub fn run_fix(value: Option<&str>, exe: &str, from_source: bool, exists: impl Fn(&str) -> bool) -> RunFix {
        let Some(value) = value else { return RunFix::Keep };
        let was = program_of(value);
        if same_path(was) == same_path(exe) {
            return if same_path(value) == same_path(&run_command(exe)) { RunFix::Keep } else { RunFix::Rewrite("as an older version wrote it") };
        }
        if from_source {
            return RunFix::Keep;
        }
        RunFix::Rewrite(if exists(was) { "another copy of her" } else { "gone" })
    }

    // What becomes of her logon task: there while start at login is hers,
    // whatever her home, running this program; gone otherwise. It was the
    // taskbar's alone at first, the island left to the Run value; but
    // Explorer starts the Run values one by one after its own, each waited
    // on, some half a minute in: on the user's PC hers came 96 and 110 s
    // after the logon, and a session shut down 52 s in never got to it
    // (2026-10-13, 10-15).
    #[derive(Debug, PartialEq)]
    pub enum TaskFix {
        Keep,
        Make,
        Remove,
    }

    pub fn task_fix(login: Login, task: Option<&TaskSeen>, exe: &str) -> TaskFix {
        match (login, task) {
            // Another exe's start at login: not this one's to change.
            (Login::Theirs, _) => TaskFix::Keep,
            (Login::Off, Some(_)) => TaskFix::Remove,
            (Login::Mine, None) => TaskFix::Make,
            // Turned off in the Task Scheduler: left as the person left it.
            (Login::Mine, Some(t)) if !t.enabled => TaskFix::Keep,
            (Login::Mine, Some(t)) if same_path(&t.program) != same_path(exe) || t.args != task_args() => TaskFix::Make,
            _ => TaskFix::Keep,
        }
    }

    // Her logon task, as she uninstalls: hers to take away only when its name
    // is the one her start at login gives it ("<name> at logon-<SID>") and it
    // runs this very program. Another installation's task (another folder),
    // another program's, or a name that is not the one she writes: left alone,
    // so uninstalling one copy never touches another person's start at login.
    #[derive(Debug, PartialEq)]
    pub enum Own {
        // No such task: nothing to take away.
        Missing,
        // A task that is not this installation's own.
        Theirs,
        // This installation's: hers to take away.
        Mine,
    }

    pub fn own_task(name: &str, task: &str, seen: Option<&TaskSeen>, exe: &str) -> Own {
        // No program to compare with (a run from the source tree): never a
        // reason to delete anyone's task.
        if exe.is_empty() {
            return Own::Theirs;
        }
        let Some(seen) = seen else { return Own::Missing };
        let ours = task
            .strip_prefix(&format!("{name} at logon-"))
            .is_some_and(|sid| !sid.trim().is_empty());
        if !ours || same_path(&seen.program) != same_path(exe) {
            return Own::Theirs;
        }
        Own::Mine
    }

    #[cfg(test)]
    mod own_task_tests {
        use super::{own_task, Own};
        use crate::logon::TaskSeen;

        fn seen(program: &str, args: &str, enabled: bool) -> TaskSeen {
            TaskSeen { program: program.to_string(), args: args.to_string(), enabled }
        }

        const TASK: &str = "Wakuwaku at logon-S-1-5-21-1-2-3-1001";
        const EXE: &str = r"C:\Users\me\AppData\Local\Programs\DSH-FISH\DSH-FISH.exe";

        #[test] fn this_installations_own_task_is_taken_away() {
            assert_eq!(own_task("Wakuwaku", TASK, Some(&seen(EXE, "--wakuwaku-at-login task", true)), EXE), Own::Mine);
        }

        // Another installation (another folder) or another program: never hers
        // to delete, uninstalling one copy must not touch the other's start.
        #[test] fn another_installations_task_is_left_alone() {
            let other = r"D:\Tools\DSH-FISH\DSH-FISH.exe";
            assert_eq!(own_task("Wakuwaku", TASK, Some(&seen(other, "--wakuwaku-at-login task", true)), EXE), Own::Theirs);
            assert_eq!(own_task("Wakuwaku", TASK, Some(&seen(r"C:\Windows\notepad.exe", "", true)), EXE), Own::Theirs);
        }

        // The same program, written another way (case, slashes): still hers.
        #[test] fn the_same_program_written_another_way_is_hers() {
            let written = r"c:/users/ME/appdata/local/programs/dsh-fish/dsh-fish.EXE";
            assert_eq!(own_task("Wakuwaku", TASK, Some(&seen(written, "", false)), EXE), Own::Mine);
        }

        // A name this start at login never writes is not hers to judge.
        #[test] fn a_task_named_otherwise_is_not_hers() {
            let other = "WakuwakuTest at logon-S-1-5-21-1-2-3-1001";
            assert_eq!(own_task("Wakuwaku", other, Some(&seen(EXE, "", true)), EXE), Own::Theirs);
            assert_eq!(own_task("Wakuwaku", "Wakuwaku", Some(&seen(EXE, "", true)), EXE), Own::Theirs);
            assert_eq!(own_task("Wakuwaku", "Wakuwaku at logon-", Some(&seen(EXE, "", true)), EXE), Own::Theirs);
        }

        // Nothing there: nothing to take away, and no error.
        #[test] fn no_task_at_all_is_nothing_to_do() {
            assert_eq!(own_task("Wakuwaku", TASK, None, EXE), Own::Missing);
        }

        // Without a program of her own (run from the source tree) no task is
        // hers to delete: an empty path would match anything Windows calls the
        // same, and that is everyone's.
        #[test] fn without_a_program_of_her_own_nothing_is_deleted() {
            assert_eq!(own_task("Wakuwaku", TASK, Some(&seen("", "", true)), ""), Own::Theirs);
        }
    }
}

#[cfg(windows)]
mod login {
    use super::launch_command;
    use super::rules::{login_of, own_task, run_command, run_fix, task_args, task_fix, Login, Own, RunFix, TaskFix};
    use crate::logon;
    use winreg::enums::{HKEY_CURRENT_USER, KEY_READ, KEY_WRITE};
    use winreg::RegKey;

    const RUN: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
    // Task Manager's Startup apps: those turned off there, by name, their
    // first byte odd (03; turned on again, 02).
    const APPROVED: &str = r"Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run";
    const NAME: &str = "Wakuwaku";

    fn value(name: &str) -> Option<String> {
        RegKey::predef(HKEY_CURRENT_USER).open_subkey_with_flags(RUN, KEY_READ).and_then(|k| k.get_value::<String, _>(name)).ok()
    }

    fn turned_off(name: &str) -> bool {
        RegKey::predef(HKEY_CURRENT_USER)
            .open_subkey_with_flags(APPROVED, KEY_READ)
            .and_then(|k| k.get_raw_value(name))
            .is_ok_and(|v| v.bytes.first().is_some_and(|b| b & 1 == 1))
    }

    fn write(name: &str, command: &str) -> Result<(), String> {
        let (key, _) = RegKey::predef(HKEY_CURRENT_USER).create_subkey_with_flags(RUN, KEY_READ | KEY_WRITE).map_err(|e| e.to_string())?;
        key.set_value(name, &command.to_string()).map_err(|e| e.to_string())
    }

    fn delete(path: &str, name: &str) -> Result<(), String> {
        let Ok(key) = RegKey::predef(HKEY_CURRENT_USER).open_subkey_with_flags(path, KEY_READ | KEY_WRITE) else { return Ok(()) };
        match key.delete_value(name) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e.to_string()),
            _ => Ok(()),
        }
    }

    fn login(name: &str, exe: &str) -> Login {
        login_of(value(name).as_deref(), turned_off(name), exe)
    }

    // On for this program: a Run value naming it (written by an older
    // version or moved here as she started, too), not turned off in Task
    // Manager.
    pub fn is_on() -> bool {
        login(NAME, &launch_command()) == Login::Mine
    }

    // This user's logon task: the Run value's name, "at logon", their SID;
    // and the SID.
    fn task_of(name: &str) -> Result<(String, String), String> {
        let sid = logon::user_sid().ok_or("no SID for this user")?;
        Ok((format!("{name} at logon-{sid}"), sid))
    }

    // The logon task made, repaired or removed to match start at login;
    // what was done, if anything.
    fn sync_task(name: &str, exe: &str) -> Result<Option<String>, String> {
        let login = login(name, exe);
        if login == Login::Theirs {
            return Ok(None);
        }
        let (task, sid) = task_of(name)?;
        let seen = logon::xml(&task)?.map(|xml| logon::seen(&xml));
        match task_fix(login, seen.as_ref(), exe) {
            TaskFix::Keep => Ok(None),
            TaskFix::Make => {
                logon::register(&task, &logon::task_xml(&sid, exe, &task_args()))?;
                Ok(Some(match seen {
                    Some(was) => format!("her logon task pointed at {} {}; now {exe} {}", was.program, was.args, task_args()),
                    None => format!("her logon task made: {task}"),
                }))
            }
            TaskFix::Remove => {
                logon::remove(&task)?;
                Ok(Some("her logon task removed (start at login is off)".to_string()))
            }
        }
    }

    fn set_named(name: &str, exe: &str, on: bool) -> Result<(), String> {
        if on {
            write(name, &run_command(exe))?;
            // Turned off in Task Manager before: on there too, as asked here.
            delete(APPROVED, name)?;
        } else {
            delete(RUN, name)?;
        }
        sync_task(name, exe).map(drop).map_err(|e| format!("her logon task: {e}"))
    }

    // The switch: the Run value and the logon task, or neither.
    pub fn set(on: bool) -> Result<(), String> {
        set_named(NAME, &launch_command(), on)
    }

    fn at_start_named(name: &str, exe: &str, from_source: bool) -> Vec<String> {
        let mut said = Vec::new();
        let was = value(name);
        if let RunFix::Rewrite(why) = run_fix(was.as_deref(), exe, from_source, |p| std::path::Path::new(p).is_file()) {
            let now = run_command(exe);
            let was = was.unwrap_or_default();
            said.push(match write(name, &now) {
                Ok(()) => format!("start at login pointed at {was} ({why}); now {now}"),
                Err(e) => format!("start at login pointed at {was} ({why}); not written anew: {e}"),
            });
        }
        match sync_task(name, exe) {
            Ok(done) => said.extend(done),
            Err(e) => said.push(format!("her logon task: {e}")),
        }
        said
    }

    // As she starts on her own port: the Run value moved to this program,
    // the task to match; what was done, a line each.
    pub fn at_start() -> Vec<String> {
        at_start_named(NAME, &launch_command(), crate::data::from_source())
    }

    // Started by her logon task while start at login is off (the Run value
    // taken away, or turned off in Task Manager): the task goes, and true
    // (she goes too).
    pub fn task_unwanted() -> bool {
        if login(NAME, &launch_command()) != Login::Off {
            return false;
        }
        if let Ok((task, _)) = task_of(NAME) {
            let _ = logon::remove(&task);
        }
        true
    }

    // Uninstalling: her own logon task taken away, and what was done.
    // Only when it runs this very program: another copy of her in another
    // folder, or another program's task, is left as it is. A task that is not
    // there is nothing to do; a task read but not removable is an error, and
    // the uninstaller goes on (it says so and deletes her files anyway: once
    // the program is gone the task starts nothing).
    pub fn uninstall_cleanup() -> Result<Option<String>, String> {
        let exe = launch_command();
        let (task, _) = task_of(NAME)?;
        let seen = logon::xml(&task)?.map(|xml| logon::seen(&xml));
        match own_task(NAME, &task, seen.as_ref(), &exe) {
            Own::Missing => Ok(None),
            Own::Theirs => Ok(Some(format!("her logon task is not this program's; left as it is: {task}"))),
            Own::Mine => {
                logon::remove(&task)?;
                Ok(Some(format!("her logon task removed: {task}")))
            }
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        // Against this computer's own registry and Task Scheduler, under a
        // test's name (WakuwakuTest, its task WakuwakuTest at logon-<SID>),
        // all of it taken away at the end: cargo test -- --ignored
        // start_at_login_kept_on_this_computer --nocapture.
        #[test]
        #[ignore]
        fn start_at_login_kept_on_this_computer() {
            const TEST: &str = "WakuwakuTest";
            let exe = launch_command();
            let (task, _) = task_of(TEST).unwrap();
            let was = r#""C:\nowhere\wakuwaku-v0.2.5-windows-x64.exe""#;
            let windows_view = || {
                let out = std::process::Command::new("schtasks").args(["/Query", "/TN", &task, "/V", "/FO", "LIST"]).output().unwrap();
                println!("{}", String::from_utf8_lossy(&out.stdout));
            };
            let clean = || {
                let _ = delete(RUN, TEST);
                let _ = delete(APPROVED, TEST);
                let _ = logon::remove(&task);
            };
            clean();

            // Off: nothing made at the start.
            assert!(at_start_named(TEST, &exe, false).is_empty());
            assert_eq!(value(TEST), None);
            assert_eq!(logon::xml(&task).unwrap(), None);

            // A value for an exe gone: this one's, and the task made for it.
            write(TEST, was).unwrap();
            let said = at_start_named(TEST, &exe, false);
            println!("{said:#?}");
            assert!(said[0].contains("(gone)"), "{said:?}");
            assert_eq!(value(TEST), Some(run_command(&exe)));
            assert_eq!(login(TEST, &exe), Login::Mine);
            let seen = logon::seen(&logon::xml(&task).unwrap().unwrap());
            assert_eq!((seen.program.as_str(), seen.args.as_str(), seen.enabled), (exe.as_str(), task_args().as_str(), true));
            windows_view();
            // Again: nothing to do.
            assert!(at_start_named(TEST, &exe, false).is_empty());

            // From the source tree another exe's value stays as it is.
            write(TEST, was).unwrap();
            assert!(at_start_named(TEST, &exe, true).is_empty());
            assert_eq!(value(TEST).as_deref(), Some(was));
            write(TEST, &run_command(&exe)).unwrap();

            // The task taken away in the Task Scheduler: made again.
            logon::remove(&task).unwrap();
            assert_eq!(sync_task(TEST, &exe).unwrap(), Some(format!("her logon task made: {task}")));

            // Turned off in Task Manager: off here, the task gone; on again from the switch.
            let off = winreg::RegValue { bytes: vec![3, 0, 0, 0, 1, 2, 3, 4, 5, 6, 7, 8].into(), vtype: winreg::enums::RegType::REG_BINARY };
            RegKey::predef(HKEY_CURRENT_USER).create_subkey(APPROVED).unwrap().0.set_raw_value(TEST, &off).unwrap();
            assert_eq!(login(TEST, &exe), Login::Off);
            assert_eq!(at_start_named(TEST, &exe, false), vec!["her logon task removed (start at login is off)".to_string()]);
            set_named(TEST, &exe, true).unwrap();
            assert!(!turned_off(TEST));
            assert_eq!(login(TEST, &exe), Login::Mine);
            assert!(logon::xml(&task).unwrap().is_some());

            // Off from the switch: both gone.
            set_named(TEST, &exe, false).unwrap();
            assert_eq!(value(TEST), None);
            assert_eq!(logon::xml(&task).unwrap(), None);
            clean();
        }
    }
}

#[cfg(not(windows))]
mod login {
    pub fn is_on() -> bool {
        false
    }

    pub fn set(_on: bool) -> Result<(), String> {
        Err("not on this system".into())
    }

    pub fn at_start() -> Vec<String> {
        Vec::new()
    }

    pub fn task_unwanted() -> bool {
        false
    }

    pub fn uninstall_cleanup() -> Result<Option<String>, String> {
        Ok(None)
    }
}

pub use login::{at_start as login_at_start, is_on as is_open_at_login, set as set_open_at_login, task_unwanted as login_task_unwanted, uninstall_cleanup as login_uninstall_cleanup};

#[cfg(test)]
mod tests {
    #[test]
    fn an_agents_version_as_its_file_says_it() {
        assert_eq!(tidy_version("2.1.296.0").as_deref(), Some("2.1.296"));
        assert_eq!(tidy_version("3.15.3.8237").as_deref(), Some("3.15.3"));
        assert_eq!(tidy_version("0.2.0-rc.2\0").as_deref(), Some("0.2.0-rc.2"));
        assert_eq!(tidy_version(" 2.0.29 ").as_deref(), Some("2.0.29"));
        // Windows' own: the build's name after it.
        assert_eq!(tidy_version("10.0.26100.1 (WinBuild.160101.0800)").as_deref(), Some("10.0.26100"));
        assert_eq!(tidy_version("1.2.3.beta").as_deref(), Some("1.2.3.beta"));
        assert_eq!(tidy_version(""), None);
        assert_eq!(tidy_version("unknown"), None);
        // An npm package's, from its package.json; none without one.
        let dir = std::env::temp_dir().join(format!("wakuwaku-version-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("package.json"), r#"{ "name": "opencode-ai", "version": "2.0.29" }"#).unwrap();
        assert_eq!(package_version(&dir.join("package.json")).as_deref(), Some("2.0.29"));
        assert_eq!(package_version(&dir.join("none.json")), None);
        fs::remove_dir_all(&dir).unwrap();
        // A program file's, read from it (every Windows has this one).
        #[cfg(windows)]
        {
            let kernel = PathBuf::from(std::env::var_os("SystemRoot").unwrap()).join("System32").join("kernel32.dll");
            assert!(file_version(&kernel, "Windows").is_some_and(|v| v.starts_with("10.") || v.starts_with("6.")), "{:?}", file_version(&kernel, "Windows"));
            // Another product's (as omp.exe says Bun): none.
            assert_eq!(file_version(&kernel, "Claude Code"), None);
        }
        assert_eq!(file_version(&std::env::temp_dir().join("no-such-program.exe"), "x"), None);
    }

    #[test]
    fn agy_hooks_install_status_and_uninstall() {
        let program = r"D:\pet\wakuwaku.exe";
        let theirs = json!({ "my-linter-hook": { "PostToolUse": [{ "matcher": "run_command", "hooks": [{ "command": "./lint.sh" }] }] } });
        assert_eq!(agy_status_of(&theirs, Some(program)), "missing");
        let with = agy_install(&theirs, program);
        assert_eq!(agy_status_of(&with, Some(program)), "ok");
        assert_eq!(with["wakuwaku"]["Stop"][0]["command"], r"D:\pet\wakuwaku.exe --wakuwaku-agy-hook Stop");
        assert_eq!(with["wakuwaku"]["PostToolUse"][0]["matcher"], "");
        // Theirs kept, ours alone taken out.
        assert_eq!(with["my-linter-hook"], theirs["my-linter-hook"]);
        assert_eq!(agy_uninstall(&with), theirs);
        assert_eq!(agy_status_of(&with, Some(r"E:\elsewhere\wakuwaku.exe")), "stale");
        assert_eq!(agy_status_of(&with, None), "stale");
        let mut off = with.clone();
        off["wakuwaku"]["enabled"] = json!(false);
        assert_eq!(agy_status_of(&off, Some(program)), "off");
        off["wakuwaku"]["enabled"] = json!(true);
        assert_eq!(agy_status_of(&off, Some(program)), "ok");
    }

    #[test]
    fn the_opencode_plugin_carries_her_port() {
        // The template has its port once, on a line of its own.
        assert_eq!(OPENCODE_PLUGIN.matches(OPENCODE_PORT_LINE).count(), 1);
        let ours = opencode_plugin(47311);
        assert!(ours.contains("const PORT = 47311\n") || ours.contains("const PORT = 47311\r\n"));
        assert!(!ours.contains(OPENCODE_PORT_LINE));
        assert_eq!(opencode_status_of(Some(&ours), 47311), "ok");
        assert_eq!(opencode_status_of(Some(&ours.replace('\n', "\r\n").replace("\r\r", "\r")), 47311), "ok");
        // Another port, or another version of ours: a repair.
        assert_eq!(opencode_status_of(Some(&ours), 47213), "stale");
        assert_eq!(opencode_status_of(Some(&format!("{OPENCODE_MARK}\n// an older one\n")), 47311), "stale");
        // Someone else's file of that name, or none: not ours.
        assert_eq!(opencode_status_of(Some("export default { id: 'mine', setup() {} }"), 47311), "missing");
        assert_eq!(opencode_status_of(None, 47311), "missing");
    }

    #[test]
    fn opencode_s_folder_as_it_finds_it() {
        // Changing the process's environment: only here, and put back.
        let was = (std::env::var_os("OPENCODE_CONFIG_DIR"), std::env::var_os("XDG_CONFIG_HOME"));
        std::env::set_var("OPENCODE_CONFIG_DIR", "Z:\\oc-config");
        assert_eq!(opencode_plugin_file(), PathBuf::from("Z:\\oc-config").join("plugins").join("wakuwaku.js"));
        std::env::remove_var("OPENCODE_CONFIG_DIR");
        std::env::set_var("XDG_CONFIG_HOME", "Z:\\xdg");
        assert_eq!(opencode_dir(), PathBuf::from("Z:\\xdg").join("opencode"));
        std::env::remove_var("XDG_CONFIG_HOME");
        assert_eq!(opencode_dir(), home_dir().join(".config").join("opencode"));
        for (key, value) in [("OPENCODE_CONFIG_DIR", was.0), ("XDG_CONFIG_HOME", was.1)] {
            if let Some(value) = value {
                std::env::set_var(key, value);
            }
        }
    }

    #[test]
    fn the_omp_extension_is_written_for_this_copy() {
        let exe = r#"C:\Program Files\Wakuwaku\it's "her".exe"#;
        let text = omp_extension(47312, exe, r"C:\waku\omp\data");
        assert!(text.contains(r#"const PORT = Number("47312") || 47213"#), "the port filled in");
        assert!(text.contains(&format!("const EXE = {}", json!(exe))), "the exe as a JS string");
        assert!(text.contains(r#"const DATA = "C:\\waku\\omp\\data""#));
        assert!(!text.contains("\"__WAKUWAKU_"), "nothing left to fill in");
        assert!(omp_extension(47213, "", "").contains(r#"const DATA = """#));
        // Every agent folder has it as this copy writes it, or not.
        let (ours, old) = (Some(text.clone()), Some(omp_extension(47213, exe, "")));
        assert_eq!(omp_status_of(&[None, None], &text), "missing");
        assert_eq!(omp_status_of(&[ours.clone()], &text), "ok");
        assert_eq!(omp_status_of(&[ours.clone(), ours.clone()], &text), "ok");
        assert_eq!(omp_status_of(&[ours.clone(), None], &text), "partial");
        assert_eq!(omp_status_of(&[ours.clone(), old.clone()], &text), "stale");
        assert_eq!(omp_status_of(&[None, old], &text), "stale");
    }

    #[test]
    fn the_omp_extension_goes_into_each_agent_folder_and_out() {
        let root = std::env::temp_dir().join(format!("wakuwaku-omp-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        // No profiles: the agent folder alone, made or not; PI_CODING_AGENT_DIR in its place.
        assert_eq!(agent_dirs_in(&root, None), vec![root.join("agent")]);
        assert_eq!(agent_dirs_in(&root, Some(PathBuf::from(r"D:\omp-agent"))), vec![PathBuf::from(r"D:\omp-agent")]);
        // Each profile's (a folder with no agent folder yet is none).
        for p in ["work", "home"] {
            fs::create_dir_all(root.join("profiles").join(p).join("agent")).unwrap();
        }
        fs::create_dir_all(root.join("profiles").join("empty")).unwrap();
        let dirs = agent_dirs_in(&root, None);
        assert_eq!(dirs, vec![root.join("agent"), root.join("profiles").join("home").join("agent"), root.join("profiles").join("work").join("agent")]);

        let files: Vec<PathBuf> = dirs.iter().map(|d| d.join("extensions").join(OMP_FILE)).collect();
        let theirs = root.join("agent").join("extensions").join("theirs.ts");
        fs::create_dir_all(theirs.parent().unwrap()).unwrap();
        fs::write(&theirs, "export default () => {}").unwrap();
        write_omp_files(&files, "install", "ours").unwrap();
        assert!(files.iter().all(|f| fs::read_to_string(f).unwrap() == "ours"));
        write_omp_files(&files, "install", "ours again").unwrap();
        assert_eq!(fs::read_to_string(&files[1]).unwrap(), "ours again");
        // Out: ours alone, and an extensions folder left empty.
        write_omp_files(&files, "remove", "").unwrap();
        assert!(files.iter().all(|f| !f.exists()));
        assert!(theirs.exists(), "another extension stays");
        assert!(!root.join("profiles").join("work").join("agent").join("extensions").exists());
        assert!(root.join("profiles").join("work").join("agent").is_dir());
        write_omp_files(&files, "remove", "").unwrap();
        assert!(write_omp_files(&files, "frob", "").is_err());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn zcode_hooks_install_status_and_uninstall() {
        let exe = r"C:\Program Files\Wakuwaku\wakuwaku.exe";
        // As the person may have it: a model, their own hook, a field we do not know.
        let theirs = json!({
            "model": "deepseek/deepseek-flash",
            "hooks": { "timeoutMs": 30000, "events": { "Stop": [{ "matcher": "*", "hooks": [{ "type": "command", "command": "notify-me" }] }] } },
            "future": { "x": 1 }
        });
        assert_eq!(zcode_status_of(&theirs, exe), "missing");
        let with = zcode_install(&theirs, exe);
        assert_eq!(zcode_status_of(&with, exe), "ok");
        assert_eq!((&with["hooks"]["enabled"], &with["hooks"]["timeoutMs"], &with["model"], &with["future"]), (&json!(true), &json!(30000), &theirs["model"], &theirs["future"]));
        // Theirs first, then ours; a path with a space needs no quotes (no shell).
        assert_eq!(with["hooks"]["events"]["Stop"][0], theirs["hooks"]["events"]["Stop"][0]);
        assert_eq!(
            with["hooks"]["events"]["Stop"][1],
            json!({ "matcher": "*", "hooks": [{ "type": "process", "command": exe, "args": ["--wakuwaku-zcode-hook", "Stop"], "timeoutMs": 10000 }] })
        );
        assert_eq!(with["hooks"]["events"]["PermissionRequest"][0]["hooks"][0]["timeoutMs"], 600_000);
        assert_eq!(with["hooks"]["events"].as_object().unwrap().len(), 7);
        // Installing twice replaces, never doubles.
        let again = zcode_install(&with, exe);
        assert_eq!(again["hooks"]["events"]["Stop"].as_array().unwrap().len(), 2);
        // Removed: theirs as it was, hooks.enabled too (none before: null).
        assert_eq!(zcode_uninstall(&with, Some(&Value::Null)), theirs);
        let mut was_off = theirs.clone();
        was_off["hooks"]["enabled"] = json!(false);
        assert_eq!(zcode_uninstall(&zcode_install(&was_off, exe), Some(&json!(false))), was_off);
        // On before (no note): it stays on while a hook of theirs is left.
        let mut was_on = theirs.clone();
        was_on["hooks"]["enabled"] = json!(true);
        assert_eq!(zcode_uninstall(&zcode_install(&was_on, exe), None), was_on);
        assert_eq!(zcode_uninstall(&zcode_install(&json!({}), exe), None), json!({}));
        assert_eq!(zcode_uninstall(&zcode_install(&json!({ "x": 1 }), exe), Some(&Value::Null)), json!({ "x": 1 }));

        assert_eq!(zcode_status_of(&with, r"D:\elsewhere\wakuwaku.exe"), "stale");
        if cfg!(windows) {
            assert_eq!(zcode_status_of(&with, &exe.to_lowercase()), "ok");
        }
        let mut part = with.clone();
        part["hooks"]["events"].as_object_mut().unwrap().remove("PostToolUseFailure");
        assert_eq!(zcode_status_of(&part, exe), "partial");
        // Turned off in ZCode: all hooks, or one of ours; on again, as it was.
        let mut off = with.clone();
        off["hooks"]["enabled"] = json!(false);
        assert_eq!(zcode_status_of(&off, exe), "off");
        let mut one = with.clone();
        one["hooks"]["events"]["Stop"][1]["hooks"][0]["enabled"] = json!(false);
        assert_eq!(zcode_status_of(&one, exe), "off");
        one["hooks"]["events"]["Stop"][1]["hooks"][0]["enabled"] = json!(true);
        assert_eq!(zcode_status_of(&one, exe), "ok");
    }

    #[test]
    fn reasonix_hooks_install_status_and_uninstall() {
        let exe = r"C:\Program Files\Wakuwaku\wakuwaku.exe";
        // As the person may have it: a hook of their own (for one tool), another
        // event's, a key we do not know.
        let theirs = json!({
            "hooks": {
                "PreToolUse": [{ "match": "bash", "command": "node guard.js", "timeout": 3000 }],
                "PreCompact": [{ "command": "echo keep the plan" }]
            },
            "future": { "x": 1 }
        });
        assert_eq!(reasonix_status_of(&theirs, exe), "missing");
        let with = reasonix_install(&theirs, exe).unwrap();
        assert_eq!(reasonix_status_of(&with, exe), "ok");
        assert_eq!(with["future"], theirs["future"]);
        // Theirs first and as it was, then ours: a flat list a hook a row, every tool.
        assert_eq!(with["hooks"]["PreToolUse"][0], theirs["hooks"]["PreToolUse"][0]);
        assert_eq!(
            with["hooks"]["PreToolUse"][1],
            json!({ "command": r#""C:\Program Files\Wakuwaku\wakuwaku.exe" --wakuwaku-reasonix-hook PreToolUse"#, "timeout": 5000, "description": REASONIX_DESCRIPTION })
        );
        assert_eq!(with["hooks"]["PreCompact"], theirs["hooks"]["PreCompact"]);
        assert_eq!(with["hooks"].as_object().unwrap().len(), REASONIX_EVENTS.len() + 1);
        for event in REASONIX_EVENTS {
            assert!(with["hooks"][event].as_array().unwrap().iter().any(|h| h["command"].as_str().unwrap().ends_with(&format!("--wakuwaku-reasonix-hook {event}"))), "{event}");
        }
        // Installing twice replaces, never doubles; removing gives theirs back.
        let again = reasonix_install(&with, exe).unwrap();
        assert_eq!(again["hooks"]["PreToolUse"].as_array().unwrap().len(), 2);
        assert_eq!(reasonix_uninstall(&again), theirs);
        // Ours alone: no hooks left at all.
        assert_eq!(reasonix_uninstall(&reasonix_install(&json!({}), exe).unwrap()), json!({}));
        assert_eq!(reasonix_uninstall(&reasonix_install(&json!({ "hooks": {} }), exe).unwrap()), json!({}));

        // Another copy, another version's, an event of ours missing.
        assert_eq!(reasonix_status_of(&with, r"D:\elsewhere\wakuwaku.exe"), "stale");
        if cfg!(windows) {
            assert_eq!(reasonix_status_of(&with, &exe.to_lowercase()), "ok");
        }
        let mut old = with.clone();
        old["hooks"]["Stop"][0]["timeout"] = json!(30000);
        assert_eq!(reasonix_status_of(&old, exe), "stale");
        let mut part = with.clone();
        part["hooks"].as_object_mut().unwrap().remove("Notification");
        assert_eq!(reasonix_status_of(&part, exe), "partial");
        // One of ours under an event we no longer hook: stale (a repair takes it out).
        let mut extra = with.clone();
        extra["hooks"]["PreCompact"].as_array_mut().unwrap().push(json!({ "command": format!("\"{exe}\" {REASONIX_FLAG} PreCompact"), "timeout": 5000, "description": REASONIX_DESCRIPTION }));
        assert_eq!(reasonix_status_of(&extra, exe), "stale");
        assert_eq!(reasonix_uninstall(&extra), theirs);
    }

    #[test]
    fn reasonix_commands_for_cmd() {
        if cfg!(windows) {
            // In double quotes, as cmd /d /s /c "<command>" takes it whole.
            assert_eq!(reasonix_command(r"D:\pet\wakuwaku.exe", "Stop").as_deref(), Some(r#""D:\pet\wakuwaku.exe" --wakuwaku-reasonix-hook Stop"#));
            assert_eq!(reasonix_command(r"C:\Program Files (x86)\W\wakuwaku.exe", "Stop").as_deref(), Some(r#""C:\Program Files (x86)\W\wakuwaku.exe" --wakuwaku-reasonix-hook Stop"#));
            // What would send it to Git Bash, or that cmd reads as its own: none.
            for odd in [r"C:\Users\O'Brien\w.exe", r"C:\$w\w.exe", r"C:\a`b\w.exe", r"C:\100%\w.exe", r"C:\hi!\w.exe", r#"C:\"\w.exe"#] {
                assert_eq!(reasonix_command(odd, "Stop"), None, "{odd}");
                assert_eq!(reasonix_install(&json!({}), odd), None, "{odd}");
            }
            assert_eq!(super::error_kind(&format!(r"C:\100%\w.exe: {REASONIX_NO_NAME}")), "pathChars");
        } else {
            assert_eq!(reasonix_command("/opt/it's/wakuwaku", "Stop").as_deref(), Some(r"'/opt/it'\''s/wakuwaku' --wakuwaku-reasonix-hook Stop"));
        }
        assert_eq!(reasonix_command("", "Stop"), None);
        assert_eq!(super::error_kind(&format!("x: {REASONIX_OLD}")), "old");
    }

    #[test]
    fn reasonix_reads_its_older_settings_while_its_home_has_none() {
        let dir = std::env::temp_dir().join(format!("wakuwaku-reasonix-{}", std::process::id()));
        let (home, legacy) = (dir.join("roaming").join("reasonix").join("settings.json"), dir.join(".reasonix").join("settings.json"));
        // Neither there: its home's, made there.
        assert_eq!(reasonix_file_of(home.clone(), Some(legacy.clone())), home);
        // Only the older one: that one, as Reasonix reads it.
        fs::create_dir_all(legacy.parent().unwrap()).unwrap();
        fs::write(&legacy, "{}").unwrap();
        assert_eq!(reasonix_file_of(home.clone(), Some(legacy.clone())), legacy);
        // Its home's there too: its home's, the older one no longer read.
        fs::create_dir_all(home.parent().unwrap()).unwrap();
        fs::write(&home, "\u{feff}{ \"hooks\": {} }").unwrap();
        assert_eq!(reasonix_file_of(home.clone(), Some(legacy.clone())), home);
        assert_eq!(reasonix_file_of(home.clone(), None), home);
        // A byte order mark first is no error; a file that is no JSON is.
        assert_eq!(read_json_bom(&home).unwrap(), json!({ "hooks": {} }));
        assert_eq!(read_json_bom(&dir.join("none.json")).unwrap(), json!({}));
        fs::write(&home, "{ hooks: ").unwrap();
        assert_eq!(super::error_kind(&read_json_bom(&home).unwrap_err()), "invalid");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn reasonix_settings_written_and_given_back() {
        let exe = r"D:\pet\wakuwaku.exe";
        let dir = std::env::temp_dir().join(format!("wakuwaku-reasonix-write-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let (home, legacy) = (dir.join("roaming").join("reasonix").join("settings.json"), dir.join(".reasonix").join("settings.json"));
        let theirs = json!({ "hooks": { "Stop": [{ "command": "notify-me" }] } });
        let read = |f: &Path| read_json_bom(f).unwrap();

        // Only the older file, the person's hooks in it: ours go into its
        // home's, made from it; the older one is left as it was, no copy needed.
        fs::create_dir_all(legacy.parent().unwrap()).unwrap();
        fs::write(&legacy, theirs.to_string()).unwrap();
        write_reasonix_files("install", &home, Some(legacy.clone()), exe).unwrap();
        assert_eq!(reasonix_status_of(&read(&home), exe), "ok");
        assert_eq!(read(&home)["hooks"]["Stop"][0], theirs["hooks"]["Stop"][0]);
        assert_eq!(read(&legacy), theirs);
        assert!(!backup_of(&home).exists());
        // Removed: its home's goes again (what is left is the older one's), and Reasonix reads that one.
        write_reasonix_files("remove", &home, Some(legacy.clone()), exe).unwrap();
        assert!(!home.exists());
        assert_eq!(read(&legacy), theirs);

        // Its home's the person's own: backed up once, ours taken out again, theirs kept.
        let mine = json!({ "hooks": { "PreCompact": [{ "command": "echo plan" }] }, "x": 1 });
        fs::create_dir_all(home.parent().unwrap()).unwrap();
        fs::write(&home, mine.to_string()).unwrap();
        write_reasonix_files("install", &home, Some(legacy.clone()), exe).unwrap();
        assert_eq!(read(&backup_of(&home)), mine);
        assert_eq!(read(&home)["hooks"]["PreCompact"], mine["hooks"]["PreCompact"]);
        write_reasonix_files("remove", &home, Some(legacy.clone()), exe).unwrap();
        assert_eq!(read(&home), mine);
        assert_eq!(read(&legacy), theirs);
        fs::remove_file(backup_of(&home)).unwrap();
        fs::remove_file(&home).unwrap();

        // Neither there: made, and taken away again.
        fs::remove_file(&legacy).unwrap();
        write_reasonix_files("install", &home, Some(legacy.clone()), exe).unwrap();
        assert_eq!(reasonix_status_of(&read(&home), exe), "ok");
        write_reasonix_files("remove", &home, None, exe).unwrap();
        assert!(!home.exists());
        // Nothing to remove; an action it does not know.
        write_reasonix_files("remove", &home, None, exe).unwrap();
        assert!(write_reasonix_files("frob", &home, None, exe).is_err());

        // Ours moved (another copy, stale), repaired: no copy kept of a file
        // of ours alone, and a remove gives no file back, as there was none
        // (seen in the test VM: a repair kept one, and the remove left {}).
        write_reasonix_files("install", &home, None, r"D:\old\wakuwaku.exe").unwrap();
        assert_eq!(reasonix_status_of(&read(&home), exe), "stale");
        write_reasonix_files("install", &home, None, exe).unwrap();
        assert_eq!(reasonix_status_of(&read(&home), exe), "ok");
        assert!(!backup_of(&home).exists());
        // A copy an older version kept of ours alone goes with the remove.
        fs::write(backup_of(&home), reasonix_install(&json!({}), r"D:\old\wakuwaku.exe").unwrap().to_string()).unwrap();
        write_reasonix_files("remove", &home, None, exe).unwrap();
        assert!(!home.exists() && !backup_of(&home).exists());
        // One of the person's own stays, and so does their file.
        fs::write(&home, mine.to_string()).unwrap();
        fs::write(backup_of(&home), mine.to_string()).unwrap();
        write_reasonix_files("install", &home, None, exe).unwrap();
        write_reasonix_files("remove", &home, None, exe).unwrap();
        assert_eq!((read(&home), read(&backup_of(&home))), (mine.clone(), mine));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn reasonix_s_version_and_line() {
        let dir = std::env::temp_dir().join(format!("wakuwaku-reasonix-version-{}", std::process::id()));
        fs::create_dir_all(dir.join("reasonix")).unwrap();
        fs::write(dir.join("reasonix").join("package.json"), r#"{ "name": "reasonix", "version": "0.52.0" }"#).unwrap();
        let none = |_: &[&str]| -> Option<PathBuf> { None };
        let npm = |rest: &[&str]| Some(rest.iter().fold(dir.clone(), |p, part| p.join(part)).join("package.json"));
        // No app found: its npm package's, the Node 0.x here.
        assert_eq!(reasonix_version(&none, &npm).as_deref(), Some("0.52.0"));
        assert_eq!(reasonix_version(&none, &none), None);
        // An app's file that is not Reasonix's says nothing.
        #[cfg(windows)]
        {
            let system = |_: &[&str]| Some(PathBuf::from(std::env::var_os("SystemRoot").unwrap()).join("System32").join("kernel32.dll"));
            assert_eq!(reasonix_version(&system, &none), None);
        }
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn dsh_plugin_rows_in_and_out_of_its_patch_file() {
        // As DSH's template writes a profile's patch file, rows of the person's own.
        let theirs = "# Your patch layer\n- id: ui-chat\n  name: \"@deepseek-ai/dsh-client-ui-chat\"\n  config:\n    transcriptView: standard\n";
        let with = dsh_install(Some(theirs), 47213).unwrap();
        assert!(with.starts_with(theirs), "{with}");
        assert!(with.contains("\n- insert:\n    - id: wakuwaku-plugin\n      name: './wakuwaku/wakuwaku.js'\n      config:\n        port: 47213\n# wakuwaku end\n"), "{with}");
        assert_eq!(dsh_strip(&with).1, DshRows::Plugin { name: DSH_ROW_NAME.into(), port: Some(47213) });
        // Installing twice replaces, never doubles; removing gives theirs back.
        let again = dsh_install(Some(&with), 47299).unwrap();
        assert_eq!(again.matches(DSH_BEGIN).count(), 1);
        assert!(again.contains("port: 47299") && !again.contains("port: 47213"));
        assert_eq!(dsh_uninstall(&again).as_deref(), Some(theirs));
        // No file, or an empty list: ours alone, then gone, or the list again.
        let alone = dsh_install(None, 47213).unwrap();
        assert!(alone.starts_with(DSH_BEGIN));
        assert_eq!(dsh_uninstall(&alone), None);
        let empty = "# comments\n[]\n";
        let filled = dsh_install(Some(empty), 47213).unwrap();
        assert!(filled.starts_with("# comments\n# wakuwaku begin") && !filled.contains("[]"), "{filled}");
        assert_eq!(dsh_uninstall(&filled).as_deref(), Some(empty));
        // A list written as [ … ] is not added to.
        assert_eq!(super::error_kind(&dsh_install(Some("[ { id: x, disabled: true } ]\n"), 47213).unwrap_err()), "flowList");

        // The old rows, as 0.3.2 wrote them (zhoupengjie/Wakuwaku#61): the
        // bridge, read as such; installing puts the plugin in their place.
        let old = "# wakuwaku begin: Wakuwaku follows DeepSeek Harness through its Claude Code hook bridge (remove it from her settings)\n\
                   - insert:\n    - id: wakuwaku\n      name: '@deepseek-ai/dsh-hooks-claude-code'\n      config:\n        configPath: 'D:\\wakuaku\\wakuwaku-data\\dsh-hooks.json'\n# wakuwaku end\n";
        assert_eq!(dsh_strip(old).1, DshRows::Bridge { config_path: r"D:\wakuaku\wakuwaku-data\dsh-hooks.json".into() });
        assert_eq!(dsh_status_of(&format!("{theirs}{old}"), true, 47213), ("stale", "bridge"));
        let moved = dsh_install(Some(&format!("{theirs}{old}")), 47213).unwrap();
        assert_eq!(moved, with);
        assert_eq!(dsh_uninstall(old), None);

        // How it stands: missing, ok, stale and why.
        assert_eq!(dsh_status_of(theirs, true, 47213), ("missing", ""));
        assert_eq!(dsh_status_of(&with, true, 47213), ("ok", ""));
        assert_eq!(dsh_status_of(&with, true, 47299), ("stale", "row"));
        assert_eq!(dsh_status_of(&with.replace("./wakuwaku/wakuwaku.js", "D:/elsewhere/wakuwaku.js"), true, 47213), ("stale", "row"));
        assert_eq!(dsh_status_of(&with, false, 47213), ("stale", "files"));
        // Quoted otherwise, or not at all: the same.
        assert_eq!(dsh_status_of(&with.replace("'./wakuwaku/wakuwaku.js'", "\"./wakuwaku/wakuwaku.js\""), true, 47213), ("ok", ""));
        assert_eq!(dsh_status_of(&with.replace("'./wakuwaku/wakuwaku.js'", "./wakuwaku/wakuwaku.js"), true, 47213), ("ok", ""));
    }

    #[test]
    fn dsh_bridge_rows_and_hooks_the_other_way() {
        let hooks = r"C:\Users\Me\wakuwaku-data\dsh-hooks.json";
        let theirs = "# Your patch layer\n- id: ui-chat\n  name: \"@deepseek-ai/dsh-client-ui-chat\"\n";
        // The rows as 0.3.2 wrote them, word for word (the issue's file).
        let with = dsh_install_bridge(Some(theirs), hooks).unwrap();
        assert_eq!(
            with,
            format!("{theirs}# wakuwaku begin: Wakuwaku follows DeepSeek Harness through its Claude Code hook bridge (remove it from her settings)\n- insert:\n    - id: wakuwaku\n      name: '@deepseek-ai/dsh-hooks-claude-code'\n      config:\n        configPath: '{hooks}'\n# wakuwaku end\n")
        );
        assert_eq!(dsh_strip(&with).1, DshRows::Bridge { config_path: hooks.into() });
        // Switching: the plugin's rows in place of the bridge's, and back; theirs kept.
        // Each way's row an id of its own, so DSH running lets the one go and loads the other.
        let plugin = dsh_install(Some(&with), 47213).unwrap();
        assert!(plugin.contains("- id: wakuwaku-plugin\n") && !plugin.contains("- id: wakuwaku\n"), "{plugin}");
        assert_eq!(dsh_install_bridge(Some(&plugin), hooks).unwrap(), with);
        assert_eq!(dsh_uninstall(&with).as_deref(), Some(theirs));
        // A quote in the path.
        let quoted = dsh_install_bridge(None, r"D:\it's\dsh-hooks.json").unwrap();
        assert!(quoted.contains(r"configPath: 'D:\it''s\dsh-hooks.json'"));
        assert_eq!(dsh_strip(&quoted).1, DshRows::Bridge { config_path: r"D:\it's\dsh-hooks.json".into() });

        // The hooks: commands for its PowerShell, `| Out-Null` kept (it waits
        // for a windowed program only so), only the tools worth a wait.
        let exe = r"C:\Program Files\Wakuwaku\wakuwaku.exe";
        let want = dsh_hooks(exe);
        let stop = &want["hooks"]["Stop"][0]["hooks"][0];
        assert_eq!(stop["command"], r"& 'C:\Program Files\Wakuwaku\wakuwaku.exe' --wakuwaku-dsh-hook Stop | Out-Null");
        assert_eq!((stop["type"].as_str(), stop["timeout"].as_u64()), (Some("command"), Some(DSH_TIMEOUT)));
        assert_eq!(dsh_command(r"D:\pet\wakuwaku.exe", "PostToolUse"), r"D:\pet\wakuwaku.exe --wakuwaku-dsh-hook PostToolUse | Out-Null");
        assert_eq!(want["hooks"]["PostToolUse"][0]["matcher"], DSH_POST_TOOLS);
        assert_eq!(want["hooks"]["PreToolUse"][0]["matcher"], "ask_user_question");
        assert_eq!(want["hooks"].as_object().unwrap().len(), 5);

        // How it stands.
        let rows = dsh_strip(&with).1;
        assert_eq!(dsh_bridge_status_of(&rows, hooks, Some(&want), &want, "ok"), ("ok", ""));
        assert_eq!(dsh_bridge_status_of(&DshRows::None, hooks, None, &want, "ok"), ("missing", ""));
        assert_eq!(dsh_bridge_status_of(&dsh_strip(&plugin).1, hooks, None, &want, "ok"), ("stale", "way"));
        // Her exe renamed (a version in its name): the hook list no longer this copy's.
        assert_eq!(dsh_bridge_status_of(&rows, hooks, Some(&dsh_hooks(r"D:\w\wakuwaku-v0.3.1-windows-x64.exe")), &want, "ok"), ("stale", "hooks"));
        assert_eq!(dsh_bridge_status_of(&rows, r"D:\elsewhere\dsh-hooks.json", Some(&want), &want, "ok"), ("stale", "hooks"));
        assert_eq!(dsh_bridge_status_of(&rows, hooks, Some(&want), &want, "missing"), ("stale", "packages"));
        assert_eq!(dsh_bridge_status_of(&rows, hooks, Some(&want), &want, "version"), ("stale", "version"));
        // The plugin chosen with the bridge's rows there: to repair.
        assert_eq!(dsh_status_of(&with, true, 47213), ("stale", "bridge"));

        // The way: as chosen; unset, as the rows have it.
        assert_eq!(dsh_way(&json!("bridge"), &DshRows::None), "bridge");
        assert_eq!(dsh_way(&json!("plugin"), &rows), "plugin");
        assert_eq!(dsh_way(&Value::Null, &rows), "bridge");
        assert_eq!(dsh_way(&Value::Null, &DshRows::None), "plugin");
        assert_eq!(dsh_way(&Value::Null, &dsh_strip(&plugin).1), "plugin");

        // DSH and the bridge: an event since it started, or not yet; not running.
        assert_eq!(dsh_bridge_load(&[], 5, 0), "waiting");
        assert_eq!(dsh_bridge_load(&[1_000, 900], 950, 0), "effective");
        assert_eq!(dsh_bridge_load(&[1_000], 500, 0), "firstEvent");
        assert_eq!(dsh_bridge_load(&[1_000], 0, 0), "firstEvent");
        // Switched to while DSH ran: what came before (her plugin's) is not the bridge's.
        assert_eq!(dsh_bridge_load(&[900], 950, 2_000), "firstEvent");
        assert_eq!(dsh_bridge_load(&[900], 2_100, 2_000), "effective");

        // As she starts: 0.3.2's rows whose packages are missing, the way
        // not chosen, become the plugin; chosen, or working, the bridge's
        // rows and hook list are written again; the plugin's files when not
        // as this version writes them.
        let never = || -> &'static str { panic!("not asked") };
        assert_eq!(dsh_start_plan(&rows, &Value::Null, || "missing", 47213, true), "migrate");
        assert_eq!(dsh_start_plan(&rows, &Value::Null, || "version", 47213, true), "migrate");
        assert_eq!(dsh_start_plan(&rows, &Value::Null, || "ok", 47213, true), "bridge");
        assert_eq!(dsh_start_plan(&rows, &json!("bridge"), never, 47213, true), "bridge");
        assert_eq!(dsh_start_plan(&rows, &json!("plugin"), never, 47213, true), "none");
        let plugin_rows = dsh_strip(&plugin).1;
        assert_eq!(dsh_start_plan(&plugin_rows, &Value::Null, never, 47213, false), "plugin");
        assert_eq!(dsh_start_plan(&plugin_rows, &Value::Null, never, 47213, true), "none");
        assert_eq!(dsh_start_plan(&plugin_rows, &Value::Null, never, 47299, false), "none");
        assert_eq!(dsh_start_plan(&DshRows::None, &json!("bridge"), never, 47213, false), "none");

        // Why its packages were not installed, as the page reads it.
        for (kind, want) in [("registry", "bridgeRegistry"), ("version", "bridgeVersion"), ("releaseAge", "bridgeReleaseAge"), ("timeout", "bridgeTimeout"), ("runtime", "bridgeRuntime"), ("busy", "bridgeBusy"), ("x", "bridgeOther")] {
            assert_eq!(super::error_kind(&format!("{DSH_BRIDGE_FAILED} [{kind}]: what pnpm said")), want);
        }
    }

    #[test]
    fn dsh_plugin_files_as_shipped() {
        // An ES module asking nothing of DSH's versions: DSH's compatibility
        // check reads engines.dsh or its DSH peers (none: no constraint; an
        // engines that is no object refuses the package, 0.2.1).
        let package: Value = serde_json::from_str(DSH_PACKAGE).unwrap();
        assert_eq!((package["type"].as_str(), package["main"].as_str()), (Some("module"), Some(DSH_PLUGIN_FILE)));
        assert!(package.get("engines").is_none() && package.get("peerDependencies").is_none() && package.get("dependencies").is_none());
        assert!(DSH_PLUGIN.starts_with("// wakuwaku-dsh-plugin"));
        assert!(dsh_plugin_version() >= 1);
        // Node's own modules alone.
        for line in DSH_PLUGIN.lines().filter(|l| l.starts_with("import ")) {
            assert!(line.contains("'node:"), "{line}");
        }
        assert!(DSH_ROW_NAME.ends_with(&format!("{DSH_DIR}/{DSH_PLUGIN_FILE}")));
    }

    #[test]
    fn dsh_plugin_files_written_and_taken_away() {
        let dir = std::env::temp_dir().join(format!("wakuwaku-dsh-plugin-{}", std::process::id())).join(DSH_DIR);
        let _ = fs::remove_dir_all(&dir);
        assert!(!dsh_files_ok(&dir));
        write_dsh_files(&dir, false).unwrap();
        assert!(dsh_files_ok(&dir));
        // One changed by hand is written again; the others are left as they are.
        fs::write(dir.join(DSH_PLUGIN_FILE), "// not ours\n").unwrap();
        assert!(!dsh_files_ok(&dir));
        write_dsh_files(&dir, false).unwrap();
        assert!(dsh_files_ok(&dir));
        // Its words that DSH loaded it, a broken one passed over.
        fs::write(dir.join("loaded-desktop.json"), r#"{"pid":4321,"at":1792200000000,"version":1,"sha":"0123456789abcdef","profile":"desktop","port":47213}"#).unwrap();
        fs::write(dir.join("loaded-default.json"), "{ half").unwrap();
        assert_eq!(dsh_markers(&dir), vec![DshLoaded { pid: 4321, at: 1792200000000, version: 1, sha: "0123456789abcdef".into(), profile: "desktop".into() }]);
        // Taken away: ours and the words; someone else's file and the folder kept.
        fs::write(dir.join("notes.txt"), "mine").unwrap();
        remove_dsh_files(&dir).unwrap();
        let left: Vec<String> = fs::read_dir(&dir).unwrap().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
        assert_eq!(left, vec!["notes.txt".to_string()]);
        fs::remove_file(dir.join("notes.txt")).unwrap();
        remove_dsh_files(&dir).unwrap();
        assert!(!dir.exists());
        // Nothing there: nothing to do.
        remove_dsh_files(&dir).unwrap();
        let _ = fs::remove_dir_all(dir.parent().unwrap());
    }

    #[test]
    fn whether_dsh_loaded_the_plugin() {
        let at = 1_792_200_000_000u64;
        let word = |pid: u32, when: u64, sha: &str| DshLoaded { pid, at: when, version: 1, sha: sha.into(), profile: "desktop".into() };
        // pid 10 runs since at-5s; 11 since at+60s (a number reused); 12 is gone.
        let started = |pid: u32| match pid {
            10 => Some(at - 5_000),
            11 => Some(at + 60_000),
            _ => None,
        };
        let written = at - 3_600_000;
        let now = at + 120_000;
        let (new, old) = ("0123456789abcdef", "fedcba9876543210");
        assert_eq!(dsh_load_of(&[word(10, at, new)], started, &[at - 6_000], written, now, new, 1), DshLoad::Loaded(word(10, at, new)));
        // Other code loaded (an older plugin, or the file written again since
        // under the same name: DSH keeps what it imported): a restart takes the new.
        assert_eq!(dsh_load_of(&[word(10, at, old)], started, &[at - 6_000], written, now, new, 1), DshLoad::Older(word(10, at, old)));
        // A word with no digest (none written by that one): by its version.
        assert_eq!(dsh_load_of(&[word(10, at, "")], started, &[at - 6_000], written, now, new, 1), DshLoad::Loaded(word(10, at, "")));
        assert_eq!(dsh_load_of(&[word(10, at, "")], started, &[at - 6_000], written, now, new, 2), DshLoad::Older(word(10, at, "")));
        // Its process gone, or the number another's: not loaded by anything running.
        assert_eq!(dsh_load_of(&[word(12, at, new), word(11, at, new)], started, &[], written, now, new, 1), DshLoad::Waiting);
        // DSH up since before the plugin was written: a restart loads it.
        assert_eq!(dsh_load_of(&[], started, &[written - 1_000, at], written, now, new, 1), DshLoad::Restart);
        // Up a while since, and no word: not loaded; only starting: wait.
        assert_eq!(dsh_load_of(&[], started, &[now - 61_000], written, now, new, 1), DshLoad::NotLoaded);
        assert_eq!(dsh_load_of(&[], started, &[now - 10_000], written, now, new, 1), DshLoad::Waiting);
        assert_eq!(DshLoad::NotLoaded.word(), "notLoaded");
        // The digest as the plugin takes its own: 16 hex digits of the file's SHA-256,
        // the same whether this checkout has its lines ending CRLF or LF.
        assert!(dsh_plugin_sha().len() == 16 && dsh_plugin_sha().chars().all(|c| c.is_ascii_hexdigit()));
        let lf = DSH_PLUGIN.replace("\r\n", "\n");
        assert_eq!(sha_of_text(&lf), dsh_plugin_sha());
        assert_eq!(sha_of_text(&lf.replace('\n', "\r\n")), dsh_plugin_sha());
        // As node's crypto has it (test/dsh.test.js takes the plugin's own the same way).
        assert_eq!(sha_of_text("a\r\nb\r\n"), "911169ddaaf146af");
    }

    #[test]
    fn a_dsh_row_turned_off() {
        // As DSH's plugin settings turn a row off, in a profile's own patch file.
        let off = "# Your patch layer\n- id: ui-chat\n  name: \"@deepseek-ai/dsh-client-ui-chat\"\n- id: wakuwaku\n  disabled: true\n";
        assert!(row_disabled(off, "wakuwaku"));
        assert!(row_disabled("- disabled: true\n  id: 'wakuwaku'\n- id: other\n", "wakuwaku"));
        assert!(!row_disabled("- id: wakuwaku\n  disabled: false\n", "wakuwaku"));
        assert!(!row_disabled("- id: other\n  disabled: true\n- id: wakuwaku\n", "wakuwaku"));
        // One inside another's insert is not a row of this list.
        assert!(!row_disabled("- insert:\n    - id: wakuwaku\n      disabled: true\n", "wakuwaku"));
        // In the home patch file, after our rows (dsh_turned_off reads it past them).
        let ours = dsh_install(None, 47213).unwrap();
        let home = format!("{ours}- id: wakuwaku-plugin\n  disabled: true\n");
        assert!(row_disabled(&dsh_strip(&home).0, DSH_PLUGIN_ID));
        assert!(!row_disabled(&dsh_strip(&ours).0, DSH_PLUGIN_ID));
        // Repair takes a row that only turns ours off away (either way's); the person's rows stay.
        let theirs = "# mine\n- id: ui-chat\n  config:\n    transcriptView: standard\n";
        assert_eq!(dsh_install(Some(&home), 47213).unwrap(), ours);
        assert_eq!(dsh_install(Some(&format!("{theirs}{home}")), 47213).unwrap(), format!("{theirs}{ours}"));
        assert_eq!(dsh_install(Some(&format!("{ours}- id: wakuwaku\n  disabled: true\n")), 47213).unwrap(), ours);
        // One that says more is the person's: kept.
        let more = "- id: wakuwaku-plugin\n  disabled: true\n  config:\n    port: 1\n";
        assert!(dsh_install(Some(&format!("{theirs}{more}")), 47213).unwrap().contains(more));
    }

    // A file she made, left empty by a remove, goes again, with no backup
    // made of what was ours; one the person had (its backup beside it) is
    // written back, its backup kept as it was.
    #[test]
    fn a_remove_leaves_no_file_of_ours_and_no_backup_of_ours() {
        let dir = std::env::temp_dir().join(format!("wakuwaku-writeback-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let made = dir.join("hooks.json");
        write_json(&made, &json!({ "hooks": { "Stop": [] } })).unwrap();
        assert!(!backup_of(&made).exists(), "nothing of the person's to back up");
        write_back(&made, &json!({})).unwrap();
        assert!(!made.exists() && !backup_of(&made).exists());
        // Theirs: backed up as she first wrote it, written back on a remove.
        let theirs = dir.join("config.json");
        fs::write(&theirs, "{ \"model\": \"m\" }").unwrap();
        write_json(&theirs, &json!({ "model": "m", "hooks": {} })).unwrap();
        write_back(&theirs, &json!({ "model": "m" })).unwrap();
        assert_eq!(read_json_now(&theirs).unwrap(), json!({ "model": "m" }));
        assert_eq!(fs::read_to_string(backup_of(&theirs)).unwrap(), "{ \"model\": \"m\" }");
        // Emptied, but the person's once: kept, as {}.
        write_back(&theirs, &json!({})).unwrap();
        assert_eq!(read_json_now(&theirs).unwrap(), json!({}));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_path_cmd_reads_without_quotes() {
        assert!(is_bare(r"D:\pet\wakuwaku.exe"));
        assert!(is_bare(r"C:\Users\张三\wakuwaku.exe"));
        assert!(!is_bare(r"C:\Program Files\Wakuwaku\wakuwaku.exe"));
        assert!(!is_bare(r"C:\Program Files (x86)\w.exe"));
        assert!(!is_bare(r"C:\a&b\w.exe"));
        assert_eq!(agy_program(r"D:\pet\wakuwaku.exe").as_deref(), Some(r"D:\pet\wakuwaku.exe"));
        assert_eq!(super::error_kind(&format!("x: {AGY_NO_NAME} (a space)")), "path");
    }

    #[test]
    fn a_write_that_failed_says_why() {
        assert_eq!(super::error_kind("另一个程序正在使用此文件，进程无法访问。 (os error 32)"), "locked");
        assert_eq!(super::error_kind("expected `,` or `}` at line 3 column 5"), "invalid");
        assert_eq!(super::error_kind("拒绝访问。 (os error 5)"), "other");
    }

    use super::*;

    #[test]
    fn install_status_and_uninstall() {
        let mine = json!({ "hooks": { "Stop": [{ "hooks": [{ "type": "command", "command": "echo hi" }] }] }, "model": "x" });
        assert_eq!(status_of(&mine, 47213), "missing");

        let with = install(&mine, 47213, false);
        assert_eq!(status_of(&with, 47213), "ok");
        assert_eq!(status_of(&with, 47299), "stale");
        assert_eq!(with["hooks"]["Stop"].as_array().unwrap().len(), 2);
        assert_eq!(with["hooks"]["PermissionRequest"][0]["hooks"][0]["timeout"], 300);
        assert_eq!(with["hooks"]["SessionStart"][0]["hooks"][0]["args"], json!([ENSURE_FLAG]));
        assert_eq!(status_of(&install(&mine, 47213, true), 47213), "httpOnly");

        // Installing twice replaces, never doubles.
        let again = install(&with, 47213, false);
        assert_eq!(again["hooks"]["Stop"].as_array().unwrap().len(), 2);

        let without = uninstall(&with);
        assert_eq!(without, mine);
        assert_eq!(uninstall(&install(&json!({}), 47213, false)), json!({}));
    }

    #[test]
    fn some_events_missing_is_partial() {
        let mut with = install(&json!({}), 47213, false);
        with["hooks"].as_object_mut().unwrap().remove("Elicitation");
        assert_eq!(status_of(&with, 47213), "partial");
    }

    #[test]
    fn codex_hooks_install_status_and_uninstall() {
        let cmd = codex_command(r"D:\pet\wakuwaku.exe");
        assert_eq!(cmd, r"D:\pet\wakuwaku.exe --wakuwaku-codex-hook");
        let mine = json!({ "hooks": { "Stop": [{ "hooks": [{ "type": "command", "command": "notify-me" }] }] }, "other": 1 });
        assert_eq!(codex_status_of(&mine, &cmd, true), "missing");

        let with = codex_install(&mine, &cmd, "waiting", true);
        assert_eq!(codex_status_of(&with, &cmd, true), "ok");
        assert_eq!(with["hooks"]["Stop"].as_array().unwrap().len(), 2);
        let ask = &with["hooks"]["PermissionRequest"][0]["hooks"][0];
        assert_eq!((ask["timeout"].as_u64(), ask.get("async"), ask["statusMessage"].as_str()), (Some(300), None, Some("waiting")));
        // The turn starting and every step in the background, nothing else
        // for them; the turn ending waited on (Codex runs no Stop in the background).
        for event in ["UserPromptSubmit", "PreToolUse", "PostToolUse"] {
            let groups = with["hooks"][event].as_array().unwrap();
            assert_eq!(groups.len(), 1, "{event}");
            assert_eq!((groups[0].get("matcher"), &groups[0]["hooks"][0]["async"]), (None, &json!(true)), "{event}");
        }
        assert_eq!(with["hooks"]["Stop"][1]["hooks"][0].get("async"), None);
        // Another copy of the program: a repair.
        assert_eq!(codex_status_of(&with, &codex_command(r"E:\elsewhere\wakuwaku.exe"), true), "stale");

        let again = codex_install(&with, &cmd, "waiting", true);
        assert_eq!(again["hooks"]["Stop"].as_array().unwrap().len(), 2);
        assert_eq!(again["hooks"]["PreToolUse"].as_array().unwrap().len(), 1);
        let mut part = again.clone();
        part["hooks"].as_object_mut().unwrap().remove("Interrupt");
        assert_eq!(codex_status_of(&part, &cmd, true), "partial");

        assert_eq!(uninstall(&with), mine);
        assert_eq!(uninstall(&codex_install(&json!({}), &cmd, "w", true)), json!({}));
    }

    #[test]
    fn an_older_codex_gets_no_background_hooks() {
        let cmd = codex_command(r"D:\pet\wakuwaku.exe");
        let old = codex_install(&json!({}), &cmd, "w", false);
        let all: Vec<&Value> = old["hooks"].as_object().unwrap().values().flat_map(|g| g.as_array().unwrap()).collect();
        assert_eq!(all.len(), 8);
        assert!(all.iter().all(|g| g["hooks"][0].get("async").is_none()));
        assert_eq!(old["hooks"]["PreToolUse"][0]["matcher"], "^request_user_input$");
        assert_eq!(old["hooks"]["PostToolUse"][0]["matcher"], "^(apply_patch|request_user_input)$");
        assert_eq!(codex_status_of(&old, &cmd, false), "ok");
        // Codex updated since: only the background ones lack.
        assert_eq!(codex_status_of(&old, &cmd, true), "upgradable");
        let mut part = old.clone();
        part["hooks"].as_object_mut().unwrap().remove("Stop");
        assert_eq!(codex_status_of(&part, &cmd, true), "partial");
        // Installed for a newer Codex, read for an older one: still ours,
        // nothing stale, but the turn starting it would not run.
        assert_eq!(codex_status_of(&codex_install(&json!({}), &cmd, "w", true), &cmd, false), "partial");
    }

    // As put in before the turn starting moved to the background (0.3.3):
    // still working, the background one to be had by a repair, which leaves
    // no foreground one for it or the steps.
    #[test]
    fn hooks_from_before_the_turn_moved_are_upgradable() {
        let cmd = codex_command(r"D:\pet\wakuwaku.exe");
        let hook = |is_async: bool, timeout: u64| {
            let mut h = json!({ "type": "command", "command": cmd, "timeout": timeout });
            if is_async {
                h["async"] = json!(true);
            }
            h
        };
        let group = |matcher: Option<&str>, h: Value| match matcher {
            Some(m) => json!({ "matcher": m, "hooks": [h] }),
            None => json!({ "hooks": [h] }),
        };
        let mut ask = hook(false, CODEX_PROMPT_TIMEOUT);
        ask["statusMessage"] = json!("w");
        let before = json!({ "hooks": {
            "SessionStart": [group(None, hook(false, 10))],
            "UserPromptSubmit": [group(None, hook(false, 10))],
            "PreToolUse": [group(Some("^request_user_input$"), hook(false, 10)), group(None, hook(true, 10))],
            "PostToolUse": [group(Some("^(apply_patch|request_user_input)$"), hook(false, 10)), group(None, hook(true, 10))],
            "PermissionRequest": [group(None, ask)],
            "Stop": [group(None, hook(false, 10))],
            "Interrupt": [group(None, hook(false, 3))],
            "SessionEnd": [group(None, hook(false, 3))],
        } });
        assert_eq!(codex_status_of(&before, &cmd, true), "upgradable");
        let after = codex_install(&before, &cmd, "w", true);
        assert_eq!(codex_status_of(&after, &cmd, true), "ok");
        assert_eq!(after, codex_install(&json!({}), &cmd, "w", true));
    }

    // Codex's own hashes of these hooks (the ones its trust keeps).
    #[test]
    fn codex_hashes_a_hook_as_codex_does() {
        let cmd = codex_command(r"D:\pet\wakuwaku.exe");
        let hook = |more: Value| {
            let mut h = json!({ "type": "command", "command": cmd, "timeout": 10 });
            for (k, v) in more.as_object().unwrap() {
                h[k] = v.clone();
            }
            h
        };
        let none = json!({});
        assert_eq!(codex_hook_hash("session_end", &none, &hook(json!({ "timeout": 3 }))), "sha256:16edcbf293c15c5ee4fb0676738d9120abacbe1847b436a5cb228fe4e7eab2a4");
        // As Codex runs it: SessionEnd's timeout 3 at most.
        assert_eq!(codex_hook_hash("session_end", &none, &hook(json!({ "timeout": 10 }))), codex_hook_hash("session_end", &none, &hook(json!({ "timeout": 3 }))));
        let ask = hook(json!({ "timeout": 300, "statusMessage": "等她" }));
        assert_eq!(codex_hook_hash("permission_request", &none, &ask), "sha256:6be114b27c0c0f23cf1c67e5ee6df5f203500bd1922bee8bf07ed42e87689358");
        assert_eq!(codex_hook_hash("pre_tool_use", &none, &hook(json!({ "async": true }))), "sha256:c663281847ce4e2d721144353a7c412486f17c9c56ca6146c6d75dba1c3d4178");
        let question = json!({ "matcher": "^request_user_input$" });
        assert_eq!(codex_hook_hash("pre_tool_use", &question, &hook(json!({}))), "sha256:9924d21717040e6d0e1d416b7b63f52481e509233655651f512ff85297a788e3");
    }

    // Trusted as they are, or not (new, changed, moved, off): never the
    // events she has or has not heard since she started.
    #[test]
    fn codex_trust_is_read_from_its_config() {
        let cmd = codex_command(r"D:\pet\wakuwaku.exe");
        let path = r"C:\Users\someone\.codex\hooks.json";
        let hooks = codex_install(&json!({ "hooks": { "Stop": [{ "hooks": [{ "type": "command", "command": "notify-me" }] }] } }), &cmd, "等她", true);
        // What Codex writes on trusting them all (keys in another case too),
        // each key quoted as given.
        let config_with = |quote: &dyn Fn(&str) -> String| {
            let mut config = String::from("model = \"x\"\n\n[hooks.state]\n\n");
            for (event, groups) in hooks["hooks"].as_object().unwrap() {
                for (g, group) in groups.as_array().unwrap().iter().enumerate() {
                    let key = format!("{}:{}:{g}:0", if cfg!(windows) { path.to_uppercase() } else { path.to_string() }, snake(event));
                    config.push_str(&format!("[hooks.state.{}]\ntrusted_hash = \"{}\"\n\n", quote(&key), codex_hook_hash(&snake(event), group, &group["hooks"][0])));
                }
            }
            config
        };
        let config = config_with(&|k| format!("'{k}'"));
        assert_eq!(codex_trust_of(path, &hooks, &config), Some(true));
        // Only someone else's trusted, or nothing of ours there.
        assert_eq!(codex_trust_of(path, &hooks, "[hooks.state]\n"), Some(false));
        assert_eq!(codex_trust_of(path, &json!({ "hooks": {} }), &config), None);
        // One turned off in Codex.
        let off = config.replacen("trusted_hash", "enabled = false\ntrusted_hash", 1);
        assert_eq!(codex_trust_of(path, &hooks, &off), Some(false));
        // One changed since (another copy of the program): not trusted as it is.
        let mut changed = hooks.clone();
        changed["hooks"]["SessionEnd"][0]["hooks"][0]["command"] = json!(codex_command(r"E:\elsewhere\wakuwaku.exe"));
        assert_eq!(codex_trust_of(path, &changed, &config), Some(false));
        // Another hooks.json: none of these keys are its.
        assert_eq!(codex_trust_of(r"D:\other\hooks.json", &hooks, &config), Some(false));
        // A key written as a basic string, its backslashes escaped.
        let basic = config_with(&|k| format!("\"{}\"", k.replace('\\', "\\\\")));
        assert_eq!(codex_trust_of(path, &hooks, &basic), Some(true));
    }

    // Whether this computer's Codex trusts her hooks (cargo test -- --ignored --nocapture).
    #[test]
    #[ignore]
    fn codex_trust_here() {
        println!("codex trusted: {:?}", codex_trusted());
    }

    #[test]
    fn codex_versions() {
        assert_eq!(parse_version("codex-cli 0.146.1\n"), Some((0, 146, 1)));
        assert_eq!(parse_version("codex-cli 0.149.0-alpha.1"), Some((0, 149, 0)));
        assert_eq!(parse_version("nothing here"), None);
        assert!((0, 146, 1) < CODEX_ASYNC_SINCE && (0, 148, 0) >= CODEX_ASYNC_SINCE && (1, 0, 0) >= CODEX_ASYNC_SINCE);
    }

    #[test]
    fn codex_commands_quote_what_needs_it() {
        if cfg!(windows) {
            assert_eq!(codex_command(r"C:\Program Files\Waku\wakuwaku.exe"), r"& 'C:\Program Files\Waku\wakuwaku.exe' --wakuwaku-codex-hook");
            assert_eq!(codex_command(r"D:\宠物\it's.exe"), r"& 'D:\宠物\it''s.exe' --wakuwaku-codex-hook");
        } else {
            assert_eq!(codex_command("/opt/my pets/wakuwaku"), "'/opt/my pets/wakuwaku' --wakuwaku-codex-hook");
        }
    }

    #[test]
    fn start_at_login_moves_with_her() {
        use super::rules::*;
        use crate::logon::TaskSeen;
        let exe = r"D:\Program Files\Wakuwaku\wakuwaku.exe";
        let ours = run_command(exe);
        assert_eq!(ours, r#""D:\Program Files\Wakuwaku\wakuwaku.exe" --wakuwaku-at-login run"#);
        let old = r#""D:\Program Files\Wakuwaku\wakuwaku-v0.2.5-windows-x64.exe""#;
        let before = format!("\"{exe}\"");
        let (gone, there) = (|_: &str| false, |_: &str| true);

        // Off stays off, whatever she is.
        assert_eq!(run_fix(None, exe, false, gone), RunFix::Keep);
        // Renamed, moved, or another copy: this one's in its place; from a
        // source tree no other exe's.
        assert_eq!(run_fix(Some(old), exe, false, gone), RunFix::Rewrite("gone"));
        assert_eq!(run_fix(Some(old), exe, false, there), RunFix::Rewrite("another copy of her"));
        assert_eq!(run_fix(Some(old), exe, true, gone), RunFix::Keep);
        // Hers: as this version writes it, or as 0.3.0 did (no flag), which
        // even a source tree's run brings up to date.
        assert_eq!(run_fix(Some(&ours), exe, false, there), RunFix::Keep);
        assert_eq!(run_fix(Some(&before), exe, true, there), RunFix::Rewrite("as an older version wrote it"));
        if cfg!(windows) {
            assert_eq!(run_fix(Some(&ours.to_uppercase()), exe, false, there), RunFix::Keep);
        }

        // The program a value starts, quoted or not.
        assert_eq!(program_of(&ours), exe);
        assert_eq!(program_of(r"C:\Program Files\W\w.EXE --x"), r"C:\Program Files\W\w.EXE");
        assert_eq!(program_of(r"C:\w\w --x"), r"C:\w\w");
        assert_eq!(program_of(""), "");

        // On: a value naming this program, in either form, not turned off in Task Manager.
        assert_eq!(login_of(None, false, exe), Login::Off);
        assert_eq!(login_of(Some(&ours), true, exe), Login::Off);
        assert_eq!(login_of(Some(&ours), false, exe), Login::Mine);
        assert_eq!(login_of(Some(&before), false, exe), Login::Mine);
        assert_eq!(login_of(Some(old), false, exe), Login::Theirs);

        // The logon task: there whenever start at login is hers (the island
        // her home too), running this program; left alone when turned off
        // in the Task Scheduler or another exe's.
        let task = |program: &str, enabled| TaskSeen { program: program.into(), args: task_args(), enabled };
        let elsewhere = r"D:\old\wakuwaku.exe";
        assert_eq!(task_fix(Login::Mine, None, exe), TaskFix::Make);
        assert_eq!(task_fix(Login::Mine, Some(&task(exe, true)), exe), TaskFix::Keep);
        assert_eq!(task_fix(Login::Mine, Some(&task(elsewhere, true)), exe), TaskFix::Make);
        assert_eq!(task_fix(Login::Mine, Some(&TaskSeen { args: String::new(), ..task(exe, true) }), exe), TaskFix::Make);
        assert_eq!(task_fix(Login::Mine, Some(&task(elsewhere, false)), exe), TaskFix::Keep);
        assert_eq!(task_fix(Login::Off, Some(&task(exe, false)), exe), TaskFix::Remove);
        assert_eq!(task_fix(Login::Off, Some(&task(exe, true)), exe), TaskFix::Remove);
        assert_eq!(task_fix(Login::Off, None, exe), TaskFix::Keep);
        assert_eq!(task_fix(Login::Theirs, Some(&task(elsewhere, true)), exe), TaskFix::Keep);
        assert_eq!(task_fix(Login::Theirs, None, exe), TaskFix::Keep);

        // How she was started, as the two give it to her.
        let args = |line: &str| line.split(' ').map(String::from).collect::<Vec<_>>();
        assert_eq!(started_by(&args("wakuwaku.exe")), None);
        assert_eq!(started_by(&args(&format!("wakuwaku.exe {}", &ours[ours.find(" --").unwrap() + 1..]))), Some("run"));
        assert_eq!(started_by(&args(&format!("wakuwaku.exe {} --wakuwaku-out-of-job", task_args()))), Some("task"));
        // And what her log says of it.
        assert_eq!(said_by(started_by(&args(&format!("wakuwaku.exe {}", task_args())))), ", by her logon task");
        assert_eq!(said_by(Some(BY_RUN)), ", by the Run value");
        assert_eq!(said_by(None), "");
    }
}
