// Today's tokens, from what Claude Code, Codex and Antigravity write down as
// they go:
//   Claude Code  <config>/projects/**/*.jsonl: each reply's usage, counted
//                once per message id (a reply is written once per part)
//   Codex        <codex home>/sessions/**/*.jsonl: token_count events, each
//                the session's total so far; today's share is its last total
//                today less its last one before today
//   Antigravity  ~/.gemini/<product>/brain/<conversation>/.system_generated/
//                logs/transcript.jsonl (agy's antigravity-cli, the 2.0 app's
//                antigravity): a step a line, each answer of the model
//                (PLANNER_RESPONSE) with its input_tokens, cache_read_tokens
//                and output_tokens (its thinking in them) and created_at;
//                counted once per step. Undocumented, as agy 1.3.4 writes it
//                (2026-10-14); the transcript_full.jsonl beside it, the one
//                the hooks name, has the same steps with all their words.
// Only files written to today are read, each from where the last reading
// stopped (and only whole lines). A new day starts from nothing.
use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::time::{Duration, UNIX_EPOCH};

use serde_json::{json, Value};

// Never more than this much of one file in one reading.
const MAX_READ: u64 = 64 * 1024 * 1024;

#[derive(Default, Clone, Copy, PartialEq, Debug)]
pub struct Count {
    pub total: u64,
    pub output: u64,
}

#[derive(Default)]
struct Codex {
    before: Count,
    today: Option<Count>,
}

#[derive(Default)]
pub struct Tokens {
    // Local midnight, in ms since 1970.
    day_start: u64,
    read_to: HashMap<PathBuf, u64>,
    claude_ids: HashSet<String>,
    claude: Count,
    codex: HashMap<PathBuf, Codex>,
    // Antigravity's steps counted, per transcript.
    agy_steps: HashMap<PathBuf, HashSet<u64>>,
    agy: Count,
    // ZCode's and OpenCode's own bills (their SQLite records, read whole on
    // each look: no offset to keep).
    zcode: Count,
    opencode: Count,
    // DeepSeek Harness's: the usage each answer of the model's carries in its
    // session log (a compressed stream: read whole, as the two above).
    dsh: Count,
}

// "2026-10-09T07:25:43.995Z" in ms since 1970.
pub fn parse_time(s: &str) -> Option<u64> {
    let b = s.as_bytes();
    if b.len() < 19 || b[4] != b'-' || b[10] != b'T' {
        return None;
    }
    let n = |r: std::ops::Range<usize>| s.get(r)?.parse::<i64>().ok();
    let (y, m, d) = (n(0..4)?, n(5..7)?, n(8..10)?);
    let (hh, mm, ss) = (n(11..13)?, n(14..16)?, n(17..19)?);
    let ms = if b.get(19) == Some(&b'.') { s.get(20..23).and_then(|f| f.parse::<i64>().ok()).unwrap_or(0) } else { 0 };
    // Days from 1970-01-01 to y-m-d (the civil calendar).
    let (y, m) = if m <= 2 { (y - 1, m + 12) } else { (y, m) };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * (m - 3) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    u64::try_from(((days * 24 + hh) * 60 + mm) * 60_000 + ss * 1000 + ms).ok()
}

// The .jsonl files under a folder written to since a moment.
fn written_since(dir: &Path, since: u64, depth: u32, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        // What the listing itself says of each entry (on Windows its kind
        // and times come with it): no further look at every file.
        let Ok(kind) = entry.file_type() else { continue };
        let path = entry.path();
        if kind.is_dir() {
            if depth > 0 {
                written_since(&path, since, depth - 1, out);
            }
        } else if path.extension().is_some_and(|e| e == "jsonl") {
            let modified = entry.metadata().ok().and_then(|m| m.modified().ok()).and_then(|t| t.duration_since(UNIX_EPOCH).ok()).map_or(0, |d| d.as_millis() as u64);
            if modified >= since {
                out.push(path);
            }
        }
    }
}

// Antigravity's transcripts written to since a moment: one a conversation,
// in each brain folder (the steps without their words, transcript.jsonl).
fn agy_written_since(brain: &Path, since: u64, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(brain) else { return };
    for entry in entries.flatten() {
        let path = entry.path().join(".system_generated").join("logs").join("transcript.jsonl");
        let modified = std::fs::metadata(&path).ok().and_then(|m| m.modified().ok()).and_then(|t| t.duration_since(UNIX_EPOCH).ok());
        if modified.is_some_and(|d| d.as_millis() as u64 >= since) {
            out.push(path);
        }
    }
}

// The whole lines added to a file since the last reading.
fn new_lines(path: &Path, from: u64) -> Option<(String, u64)> {
    let mut file = File::open(path).ok()?;
    let len = file.metadata().ok()?.len();
    let from = if from > len { 0 } else { from };
    file.seek(SeekFrom::Start(from)).ok()?;
    let mut bytes = Vec::new();
    file.take(MAX_READ).read_to_end(&mut bytes).ok()?;
    let whole = bytes.iter().rposition(|&c| c == b'\n').map_or(0, |i| i + 1);
    bytes.truncate(whole);
    Some((String::from_utf8_lossy(&bytes).into_owned(), from + whole as u64))
}

impl Tokens {
    // Read what is new. agy_brains: Antigravity's brain folders (agy_dirs).
    // day_start: local midnight (ms since 1970).
    pub fn update(&mut self, claude_dir: &Path, codex_dir: &Path, agy_brains: &[PathBuf], day_start: u64) {
        if day_start != self.day_start {
            *self = Tokens { day_start, ..Tokens::default() };
        }
        let mut files = Vec::new();
        written_since(claude_dir, day_start, 4, &mut files);
        let claude_count = files.len();
        written_since(codex_dir, day_start, 5, &mut files);
        let codex_count = files.len();
        for brain in agy_brains {
            agy_written_since(brain, day_start, &mut files);
        }
        for (i, path) in files.into_iter().enumerate() {
            let from = self.read_to.get(&path).copied().unwrap_or(0);
            let Some((text, to)) = new_lines(&path, from) else { continue };
            self.read_to.insert(path.clone(), to);
            for line in text.lines() {
                if i < claude_count {
                    self.claude_line(line);
                } else if i < codex_count {
                    self.codex_line(&path, line);
                } else {
                    self.agy_line(&path, line);
                }
            }
        }
        // Their own bills, read whole each time: their records are not written
        // in a shape that can be followed from where the last look stopped.
        // (The tests' own: no bill of this computer's is read under them.)
        if !cfg!(test) {
            if let Some(db) = zcode_db() {
                self.zcode = zcode_bill(&db, day_start).unwrap_or_default();
            }
            if let Some(db) = opencode_db() {
                self.opencode = opencode_bill(&db, day_start).unwrap_or_default();
            }
            self.dsh = dsh_bill(day_start);
        }
    }

    fn claude_line(&mut self, line: &str) {
        if !line.contains("\"usage\"") {
            return;
        }
        let Ok(e) = serde_json::from_str::<Value>(line) else { return };
        let at = e["timestamp"].as_str().and_then(parse_time).unwrap_or(0);
        let usage = &e["message"]["usage"];
        let id = e["message"]["id"].as_str().unwrap_or("");
        if e["type"] != "assistant" || at < self.day_start || !usage.is_object() || id.is_empty() || !self.claude_ids.insert(id.to_string()) {
            return;
        }
        let n = |k: &str| usage[k].as_u64().unwrap_or(0);
        let output = n("output_tokens");
        self.claude.total += n("input_tokens") + n("cache_creation_input_tokens") + n("cache_read_input_tokens") + output;
        self.claude.output += output;
    }

    fn codex_line(&mut self, path: &Path, line: &str) {
        if !line.contains("\"token_count\"") {
            return;
        }
        let Ok(e) = serde_json::from_str::<Value>(line) else { return };
        let usage = &e["payload"]["info"]["total_token_usage"];
        if e["payload"]["type"] != "token_count" || !usage.is_object() {
            return;
        }
        let at = e["timestamp"].as_str().and_then(parse_time).unwrap_or(0);
        let count = Count { total: usage["total_tokens"].as_u64().unwrap_or(0), output: usage["output_tokens"].as_u64().unwrap_or(0) };
        let session = self.codex.entry(path.to_path_buf()).or_default();
        if at < self.day_start {
            session.before = count;
        } else {
            session.today = Some(count);
        }
    }

    // A step of Antigravity's: an answer of the model's, with what it used.
    // Its input taken to leave out what was read from the cache, as Claude
    // Code's does, the names being the same (every step seen so far read
    // nothing from it).
    fn agy_line(&mut self, path: &Path, line: &str) {
        if !line.contains("\"output_tokens\"") {
            return;
        }
        let Ok(step) = serde_json::from_str::<Value>(line) else { return };
        let at = step["created_at"].as_str().and_then(parse_time).unwrap_or(0);
        let (Some(index), Some(output)) = (step["step_index"].as_u64(), step["output_tokens"].as_u64()) else { return };
        if at < self.day_start || !self.agy_steps.entry(path.to_path_buf()).or_default().insert(index) {
            return;
        }
        let n = |k: &str| step[k].as_u64().unwrap_or(0);
        self.agy.total += n("input_tokens") + n("cache_read_tokens") + output;
        self.agy.output += output;
    }

    pub fn count(&self) -> Count {
        let mut sum = self.claude;
        sum.total += self.agy.total;
        sum.output += self.agy.output;
        for s in self.codex.values() {
            if let Some(today) = s.today {
                sum.total += today.total.saturating_sub(s.before.total);
                sum.output += today.output.saturating_sub(s.before.output);
            }
        }
        sum.total += self.zcode.total + self.opencode.total + self.dsh.total;
        sum.output += self.zcode.output + self.opencode.output + self.dsh.output;
        sum
    }

    // Each agent's own bill today, by its id (widgets.rs brand names it): the
    // three read from their session records, and the two that keep their own
    // (ZCode, OpenCode). Only those with anything spent, the most first.
    pub fn by_agent(&self) -> Vec<(&'static str, Count)> {
        let mut codex = Count::default();
        for s in self.codex.values() {
            if let Some(today) = s.today {
                codex.total += today.total.saturating_sub(s.before.total);
                codex.output += today.output.saturating_sub(s.before.output);
            }
        }
        let mut all = vec![("claude", self.claude), ("codex", codex), ("agy", self.agy), ("zcode", self.zcode), ("opencode", self.opencode), ("dsh", self.dsh)];
        all.retain(|(_, c)| c.total > 0 || c.output > 0);
        all.sort_by(|a, b| b.1.total.cmp(&a.1.total).then(a.0.cmp(b.0)));
        all
    }
}

// 1234567 as 1.2M, 85000 as 85K.
pub fn short(n: u64) -> String {
    match n {
        0..=999 => n.to_string(),
        1_000..=999_999 => format!("{}K", trim(n as f64 / 1e3)),
        1_000_000..=999_999_999 => format!("{}M", trim(n as f64 / 1e6)),
        _ => format!("{}B", trim(n as f64 / 1e9)),
    }
}

fn trim(x: f64) -> String {
    if x >= 100.0 { format!("{x:.0}") } else { format!("{x:.1}").trim_end_matches(".0").to_string() }
}

pub fn words(t: &Tokens) -> Value {
    let c = t.count();
    // Beside the sum, each agent's own: the island's token widget swipes
    // between them (wcards.js). Beside vars, not in them: a var the pages
    // render in her words, and an agent is no words.
    let agents: Vec<Value> = t.by_agent().into_iter().map(|(id, c)| json!({ "id": id, "who": crate::widgets::brand(id).unwrap_or(id), "total": short(c.total), "output": short(c.output) })).collect();
    json!({ "key": "widget.tokensValue", "vars": { "total": short(c.total), "output": short(c.output) }, "agents": agents })
}

// ZCode's own record of what it spent: one row per model request, in its
// SQLite database (~/.zcode/cli/db/db.sqlite, table model_usage). Its agent
// column names the one that asked (zcode-agent, zcode-Explore, ...): all of
// them are ZCode's. status says whether the request ended ('completed'); the
// ones still running, cancelled or failed spent nothing that counts. Its
// input_tokens already take in what was read from the cache (unlike Claude
// Code's own count, nothing is added here for it).
// Its schema carries no version of its own: a field it renames ends the
// reading (no bill at all) rather than a wrong one.
fn zcode_bill(db: &Path, day_start: u64) -> Option<Count> {
    let db = rusqlite::Connection::open_with_flags(db, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY).ok()?;
    let mut stmt = db.prepare("SELECT SUM(input_tokens), SUM(output_tokens) FROM model_usage WHERE started_at >= ?1 AND status = 'completed'").ok()?;
    let (input, output): (i64, i64) = stmt.query_row([day_start as i64], |r| Ok((r.get(0)?, r.get(1)?))).ok()?;
    Some(Count { total: (input.max(0) + output.max(0)) as u64, output: output.max(0) as u64 })
}

// OpenCode's own record: each message's data carries its tokens (input,
// output, reasoning, the cache's) as the step's own use, in its SQLite
// database (~/.local/share/opencode/opencode.db, table message). Its input
// already takes in what was read from the cache.
fn opencode_bill(db: &Path, day_start: u64) -> Option<Count> {
    let db = rusqlite::Connection::open_with_flags(db, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY).ok()?;
    let mut stmt = db
        .prepare("SELECT SUM(COALESCE(json_extract(data, '$.tokens.input'), 0)), SUM(COALESCE(json_extract(data, '$.tokens.output'), 0)) FROM message WHERE time_created >= ?1")
        .ok()?;
    let (input, output): (i64, i64) = stmt.query_row([day_start as i64], |r| Ok((r.get(0)?, r.get(1)?))).ok()?;
    Some(Count { total: (input.max(0) + output.max(0)) as u64, output: output.max(0) as u64 })
}

// DeepSeek Harness's own record: each answer of its model's carries a usage
// (inputTokens, outputTokens, cacheReadTokens) in its session log, one line
// an event of its own (~/.dsh/sessions, or DSH_HOME; events_dsh.rs's own
// reading of the log). The log is a Zstandard stream (session.v4.jsonl.zstd),
// so it is read whole rather than followed from an offset, and only the logs
// written since midnight are opened at all.
// Its inputTokens already takes in what was read from the cache (unlike
// Claude Code's count, nothing is added for it here).
fn dsh_bill(day_start: u64) -> Count {
    let home = crate::connection::dsh_home();
    let mut files = Vec::new();
    // Its logs are two folders deep (sessions/<project>/<session>/), and one
    // more for a subagent's own folder.
    dsh_written_since(&home.join("sessions"), day_start, 3, &mut files);
    let mut sum = Count::default();
    for path in files {
        let Some(text) = dsh_text(&path) else { continue };
        for line in text.lines() {
            if !line.contains("\"usage\"") || !line.contains("\"assistant/message\"") {
                continue;
            }
            let Ok(e) = serde_json::from_str::<Value>(line) else { continue };
            if at_of(&e) < Some(day_start) {
                continue;
            }
            let usage = &e["data"]["usage"];
            let n = |k: &str| usage[k].as_u64().unwrap_or(0);
            let output = n("outputTokens");
            sum.total += n("inputTokens") + n("cacheReadTokens") + output;
            sum.output += output;
        }
    }
    sum
}

// When one of its events was written, in ms since 1970 (its `time` is that
// already).
fn at_of(e: &Value) -> Option<u64> {
    e["time"].as_u64()
}

// Its logs written to since a moment, in any depth of its sessions folders.
fn dsh_written_since(dir: &Path, since: u64, depth: u32, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let Ok(kind) = entry.file_type() else { continue };
        let path = entry.path();
        if kind.is_dir() {
            if depth > 0 {
                dsh_written_since(&path, since, depth - 1, out);
            }
        } else if path.file_name().is_some_and(|n| n.to_string_lossy().starts_with("session.v")) {
            let modified = entry.metadata().ok().and_then(|m| m.modified().ok()).and_then(|t| t.duration_since(UNIX_EPOCH).ok()).map_or(0, |d| d.as_millis() as u64);
            if modified >= since {
                out.push(path);
            }
        }
    }
}

// A whole session log as text, compressed or not. Its log is not one stream
// but frames appended one after another (events_dsh.rs's own reading),
// so each is decoded in turn, back to back.
fn dsh_text(path: &Path) -> Option<String> {
    let bytes = std::fs::read(path).ok()?;
    if !path.extension().is_some_and(|e| e == "zstd") {
        return Some(String::from_utf8_lossy(&bytes).into_owned());
    }
    let mut out = Vec::new();
    let mut source = &bytes[..];
    while !source.is_empty() {
        let before = source.len();
        {
            let mut decoder = ruzstd::decoding::StreamingDecoder::new_with_max_window_size(&mut source, 64 * 1024 * 1024).ok()?;
            if std::io::Read::read_to_end(&mut decoder, &mut out).is_err() {
                break;
            }
        }
        // A frame that took nothing (padding, or an unreadable tail) ends it.
        if source.len() == before || out.len() > MAX_READ as usize {
            break;
        }
    }
    Some(String::from_utf8_lossy(&out).into_owned())
}

pub fn zcode_db() -> Option<PathBuf> {
    let path = crate::connection::zcode_dir().join("cli").join("db").join("db.sqlite");
    path.is_file().then_some(path)
}

pub fn opencode_db() -> Option<PathBuf> {
    let path = std::env::var_os("XDG_DATA_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME")).map(PathBuf::from).unwrap_or_default().join(".local").join("share")
        })
        .join("opencode")
        .join("opencode.db");
    path.is_file().then_some(path)
}

// Where Claude Code and Codex keep their records.
pub fn claude_dir() -> PathBuf {
    let home = std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME")).map(PathBuf::from).unwrap_or_default();
    std::env::var_os("CLAUDE_CONFIG_DIR").map(PathBuf::from).unwrap_or_else(|| home.join(".claude")).join("projects")
}

pub fn codex_dir() -> PathBuf {
    let home = std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME")).map(PathBuf::from).unwrap_or_default();
    std::env::var_os("CODEX_HOME").map(PathBuf::from).unwrap_or_else(|| home.join(".codex")).join("sessions")
}

// Antigravity's, a brain folder for each of its products: agy's, the 2.0
// app's, and (by its docs, which name the folders) the IDE's.
pub fn agy_dirs() -> Vec<PathBuf> {
    let gemini = crate::connection::gemini_dir();
    ["antigravity-cli", "antigravity", "antigravity-ide"].iter().map(|product| gemini.join(product).join("brain")).collect()
}

pub const EVERY: Duration = Duration::from_secs(60);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn times_and_short_numbers() {
        assert_eq!(parse_time("1970-01-01T00:00:01.250Z"), Some(1250));
        assert_eq!(parse_time("2026-10-09T07:25:43.995Z"), Some(1_791_530_743_995));
        // Antigravity's, to the second.
        assert_eq!(parse_time("2026-10-09T07:25:43Z"), Some(1_791_530_743_000));
        assert_eq!(parse_time("nope"), None);
        assert_eq!((short(999), short(85_000), short(1_234_567), short(250_000_000), short(3_400_000_000)), ("999".into(), "85K".into(), "1.2M".into(), "250M".into(), "3.4B".into()));
    }

    #[test]
    fn today_is_counted_once_per_reply_and_from_where_it_stopped() {
        let dir = std::env::temp_dir().join(format!("wakuwaku-tokens-{}", std::process::id()));
        let claude = dir.join("claude").join("proj");
        let codex = dir.join("codex").join("2026").join("10").join("09");
        std::fs::create_dir_all(&claude).unwrap();
        std::fs::create_dir_all(&codex).unwrap();
        let day = parse_time("2026-10-09T00:00:00.000Z").unwrap();
        let reply = |id: &str, at: &str, out: u64| format!(r#"{{"type":"assistant","timestamp":"{at}","message":{{"id":"{id}","usage":{{"input_tokens":2,"cache_creation_input_tokens":10,"cache_read_input_tokens":100,"output_tokens":{out}}}}}}}"#);
        let lines = [
            reply("a", "2026-10-08T23:59:59.000Z", 50), // yesterday
            reply("b", "2026-10-09T08:00:00.000Z", 20),
            reply("b", "2026-10-09T08:00:00.000Z", 20), // the same reply, written again
            r#"{"type":"user","message":{"content":"hi"}}"#.to_string(),
        ];
        std::fs::write(claude.join("s.jsonl"), lines.join("\n") + "\n").unwrap();
        let token = |at: &str, total: u64, out: u64| format!(r#"{{"timestamp":"{at}","type":"event_msg","payload":{{"type":"token_count","info":{{"total_token_usage":{{"total_tokens":{total},"output_tokens":{out}}}}}}}}}"#);
        std::fs::write(codex.join("rollout.jsonl"), [token("2026-10-08T22:00:00.000Z", 1000, 100), token("2026-10-09T09:00:00.000Z", 1500, 130)].join("\n") + "\n").unwrap();
        // Antigravity's: a conversation's steps, the model's answers with
        // what they used; the same steps again beside them, not counted.
        let brains = [dir.join("agy").join("brain")];
        let logs = brains[0].join("5d94c80b").join(".system_generated").join("logs");
        std::fs::create_dir_all(logs.join("chunks").join("transcript")).unwrap();
        let answer = |i: u64, at: &str, input: u64, out: u64| format!(r#"{{"step_index":{i},"source":"MODEL","type":"PLANNER_RESPONSE","status":"DONE","created_at":"{at}","input_tokens":{input},"cache_read_tokens":200,"output_tokens":{out},"content":"ok"}}"#);
        let steps = [
            r#"{"step_index":0,"source":"USER_EXPLICIT","type":"USER_INPUT","status":"DONE","created_at":"2026-10-08T23:59:00Z","content":"say \"output_tokens\""}"#.to_string(),
            answer(1, "2026-10-08T23:59:30Z", 1000, 10), // yesterday
            answer(2, "2026-10-09T08:00:00Z", 1100, 20),
        ];
        for at in [logs.join("transcript.jsonl"), logs.join("transcript_full.jsonl"), logs.join("chunks").join("transcript").join("00000000.jsonl")] {
            std::fs::write(at, steps.join("\n") + "\n").unwrap();
        }

        let mut t = Tokens::default();
        t.update(&dir.join("claude"), &dir.join("codex"), &brains, day);
        assert_eq!(t.count(), Count { total: 132 + 500 + 1320, output: 20 + 30 + 20 });
        // More is written: only the new lines are read; a half-written one waits.
        let mut f = std::fs::OpenOptions::new().append(true).open(claude.join("s.jsonl")).unwrap();
        std::io::Write::write_all(&mut f, (reply("c", "2026-10-09T10:00:00.000Z", 5) + "\n{\"type\":\"assist").as_bytes()).unwrap();
        let mut f = std::fs::OpenOptions::new().append(true).open(logs.join("transcript.jsonl")).unwrap();
        std::io::Write::write_all(&mut f, (answer(3, "2026-10-09T08:01:00Z", 50, 5) + "\n").as_bytes()).unwrap();
        t.update(&dir.join("claude"), &dir.join("codex"), &brains, day);
        assert_eq!(t.count(), Count { total: 632 + 117 + 1320 + 255, output: 55 + 25 });
        // Written anew, shorter: read again from its start, each step still once.
        std::fs::write(logs.join("transcript.jsonl"), steps.join("\n") + "\n").unwrap();
        t.update(&dir.join("claude"), &dir.join("codex"), &brains, day);
        assert_eq!(t.count(), Count { total: 632 + 117 + 1320 + 255, output: 55 + 25 });
        // A new day: nothing yet.
        t.update(&dir.join("claude"), &dir.join("codex"), &brains, day + 86_400_000);
        assert_eq!(t.count(), Count::default());
        assert_eq!(words(&Tokens { claude: Count { total: 1_234_567, output: 85_000 }, ..Tokens::default() })["vars"], json!({ "total": "1.2M", "output": "85K" }));
        let _ = std::fs::remove_dir_all(&dir);
    }

    // ZCode's and OpenCode's bills: their own databases, read whole; the day
    // is a range of their times, and ZCode's unfinished requests spend nothing.
    #[test]
    fn their_own_bills_are_read_per_day_and_per_agent() {
        let dir = std::env::temp_dir().join(format!("wakuwaku-bills-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let day = parse_time("2026-10-09T00:00:00.000Z").unwrap();

        let zcode = dir.join("zcode.sqlite");
        let db = rusqlite::Connection::open(&zcode).unwrap();
        db.execute_batch("CREATE TABLE model_usage (agent TEXT, started_at INTEGER, status TEXT, input_tokens INTEGER, output_tokens INTEGER)").unwrap();
        let row = |agent: &str, at: u64, status: &str, input: u64, output: u64| {
            db.execute("INSERT INTO model_usage VALUES (?1, ?2, ?3, ?4, ?5)", rusqlite::params![agent, at as i64, status, input as i64, output as i64]).unwrap();
        };
        row("zcode-agent", day - 1, "completed", 9_000, 900); // yesterday
        row("zcode-agent", day, "completed", 1_000, 100);
        row("zcode-Explore", day, "completed", 2_000, 200);
        row("zcode-agent", day, "running", 5_000, 500); // not ended: nothing spent
        row("zcode-agent", day, "error", 4_000, 400);
        drop(db);
        assert_eq!(zcode_bill(&zcode, day), Some(Count { total: 3_300, output: 300 }));

        let opencode = dir.join("opencode.sqlite");        let db = rusqlite::Connection::open(&opencode).unwrap();
        db.execute_batch("CREATE TABLE message (time_created INTEGER, data TEXT)").unwrap();
        let msg = |at: u64, input: u64, output: u64| {
            db.execute("INSERT INTO message VALUES (?1, ?2)", rusqlite::params![at as i64, format!(r#"{{"tokens":{{"input":{input},"output":{output}}}}}"#)]).unwrap();
        };
        msg(day - 1, 700, 70); // yesterday
        msg(day, 30, 3);
        msg(day, 40, 4);
        drop(db);
        assert_eq!(opencode_bill(&opencode, day), Some(Count { total: 77, output: 7 }));
        // Nothing there: no bill, not an error.
        assert_eq!(zcode_bill(&dir.join("none.sqlite"), day), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    // DeepSeek Harness's log: each answer of its model's carries its usage
    // (inputTokens taking in what was read from the cache), and the day is a
    // range of its own `time`. Its logs are compressed, so the reading is
    // whole; written or not, an empty folder is no bill rather than an error.
    #[test]
    fn dsh_bills_each_answers_usage() {
        let dir = std::env::temp_dir().join(format!("wakuwaku-dsh-bills-{}", std::process::id()));
        let log = dir.join("sessions").join("--p--").join("session-1");
        std::fs::create_dir_all(&log).unwrap();
        let day = parse_time("2026-10-09T00:00:00.000Z").unwrap();
        let answer = |at: u64, input: u64, cache: u64, out: u64| format!(r#"{{"type":"assistant/message","time":{at},"data":{{"turn":1,"step":1,"usage":{{"inputTokens":{input},"outputTokens":{out},"cacheReadTokens":{cache}}}}}}}"#);
        let lines = [
            answer(day - 1, 9_000, 900, 900),  // yesterday
            answer(day, 1_000, 100, 50),
            answer(day + 1, 2_000, 200, 60),
            // Anything else in the log is no usage of the model's.
            r#"{"type":"user/message","time":1000,"data":{"text":"hi"}}"#.to_string(),
        ];
        std::fs::write(log.join("session.v4.jsonl"), lines.join("\n") + "\n").unwrap();
        // Every log written since the day, read whole, its own lines counted.
        let mut files = Vec::new();
        dsh_written_since(&dir.join("sessions"), day, 3, &mut files);
        assert_eq!(files.len(), 1, "the session log is found under its project");
        let text = dsh_text(&files[0]).unwrap();
        let sum = {
            let mut sum = Count::default();
            for line in text.lines() {
                if !line.contains("\"usage\"") || !line.contains("\"assistant/message\"") {
                    continue;
                }
                let e: Value = serde_json::from_str(line).unwrap();
                if at_of(&e).is_none_or(|t| t < day) {
                    continue;
                }
                let usage = &e["data"]["usage"];
                let n = |k: &str| usage[k].as_u64().unwrap_or(0);
                let output = n("outputTokens");
                sum.total += n("inputTokens") + n("cacheReadTokens") + output;
                sum.output += output;
            }
            sum
        };
        assert_eq!(sum, Count { total: (1_000 + 100 + 50) + (2_000 + 200 + 60), output: 110 });
        // No such folder at all: nothing found, no panic.
        let mut none = Vec::new();
        dsh_written_since(&dir.join("gone"), day, 3, &mut none);
        assert!(none.is_empty());

        // Its compressed logs are not one stream but frames appended one
        // after another: every one of them is read, not just the first (the
        // bug this pins: the first frame alone was all that came back).
        let frames: Vec<Vec<u8>> = lines
            .chunks(2)
            .map(|part| ruzstd::encoding::compress_to_vec((part.join("\n") + "\n").as_bytes(), ruzstd::encoding::CompressionLevel::Fastest))
            .collect();
        assert_eq!(frames.len(), 2, "two frames, the one after the other");
        let appended: Vec<u8> = frames.concat();
        std::fs::write(log.join("session.v4.jsonl.zstd"), &appended).unwrap();
        let text = dsh_text(&log.join("session.v4.jsonl.zstd")).unwrap();
        assert_eq!(text.lines().filter(|l| l.contains("assistant/message")).count(), 3, "every frame is read");
        assert_eq!(text.lines().filter(|l| l.contains("\"usage\"")).count(), 3);
        let _ = std::fs::remove_dir_all(&dir);
    }
}


