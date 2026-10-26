// What magpie has served today, as it counts it itself: the token card and the
// Today card's token line read this, so the numbers are the gateway's own
// ledger (`usage.jsonl` in `~/.config/magpie`), not a second parsing of Claude
// Code's or Codex's transcripts.
//
// The file holds one JSON object per line, one per request it routed:
//
//   {"t":"2026-10-25T13:32:05.013057+08:00","agent":"dsh",
//    "model":"deepseek-v4.1-flash","in":498,"out":4645,
//    "status":200, ...}
//
// `in` is the prompt side, `out` what it answered (reasoning included, as
// magpie counts it), `t` the local time of the request. A day's total is the
// two added up; only requests that went through (`status` 2xx) count, so a
// failed call is neither her tokens nor her time. Neither a file that was never
// written nor a day with nothing in it is an error: no numbers then, and the
// widgets fall back to what they read before.
use serde_json::Value;
use std::path::PathBuf;

use crate::tokens::Count;

// magpie's own directory: `%APPDATA%\..\.config\magpie` on Windows (its files
// sit beside `logins.json`, `caller-keys.json` and the rest), `~/.config/magpie`
// elsewhere. `MAGPIE_HOME` moves the whole tree, and is honoured the same way.
fn stats_file() -> Option<PathBuf> {
    if let Some(home) = std::env::var_os("MAGPIE_HOME") {
        let candidate = PathBuf::from(home).join("usage.jsonl");
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    #[cfg(windows)]
    let home = std::env::var_os("USERPROFILE").map(PathBuf::from)?;
    #[cfg(not(windows))]
    let home = std::env::var_os("HOME").map(PathBuf::from)?;
    Some(home.join(".config/magpie/usage.jsonl"))
}

/// Today's tokens as magpie served them: the day's total and the output alone.
/// None when it has not run here, served nothing today, or the file cannot be
/// read.
pub fn today(date: &str) -> Option<Count> {
    let text = std::fs::read_to_string(stats_file()?).ok()?;
    parse(&text, date)
}

/// The ledger is one object per line and grows without bound: a line that does
/// not parse (a half-written last one) is skipped, never fatal.
fn parse(text: &str, date: &str) -> Option<Count> {
    let mut total: u64 = 0;
    let mut output: u64 = 0;
    let mut any = false;
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || !line.starts_with('{') {
            continue;
        }
        let Ok(value) = serde_json::from_str::<Value>(line) else { continue };
        let Some(day) = value.get("t").and_then(Value::as_str).and_then(|t| t.get(..10)) else { continue };
        if day != date {
            continue;
        }
        // A request that did not go through took nothing from her.
        let served = value.get("status").and_then(Value::as_u64).is_some_and(|status| (200..300).contains(&status));
        if !served {
            continue;
        }
        let input = value.get("in").and_then(Value::as_u64).unwrap_or(0);
        let answered = value.get("out").and_then(Value::as_u64).unwrap_or(0);
        total += input + answered;
        output += answered;
        any = true;
    }
    // A day it served nothing in says nothing about the tokens the agents spent
    // elsewhere: the cards keep reading the transcripts then.
    any.then(|| Count { total, output })
}

#[cfg(test)]
mod tests {
    use super::*;

    const DAY: &str = "2026-10-19";

    fn line(t: &str, input: u64, out: u64, status: u64) -> String {
        serde_json::json!({
            "route_id": 1,
            "t": t,
            "agent": "dsh",
            "model": "deepseek-v4.1-flash",
            "in": input,
            "out": out,
            "status": status,
        })
        .to_string()
    }

    #[test]
    fn todays_total_and_output_are_summed_over_its_requests() {
        let text = [
            line("2026-10-18T23:59:59.0000000+08:00", 9_999, 9_999, 200),
            line("2026-10-19T00:00:01.0000000+08:00", 900, 100, 200),
            line("2026-10-19T12:00:00.0000000+08:00", 200, 34, 200),
            // A failed one: not her tokens.
            line("2026-10-19T12:00:02.0000000+08:00", 7_000, 7_000, 502),
        ]
        .join("\n");
        assert_eq!(parse(&text, DAY), Some(Count { total: 1_234, output: 134 }));
    }

    // A day it served nothing in, a day it never had, no file at all, an empty
    // file, or a line that is not JSON: no numbers, never a panic.
    #[test]
    fn nothing_today_is_no_numbers() {
        assert_eq!(parse(&line("2026-10-18T10:00:00.0000000+08:00", 1, 2, 200), DAY), None);
        assert_eq!(parse("", DAY), None);
        assert_eq!(parse("not json", DAY), None);
        assert_eq!(parse("[]", DAY), None);
        assert_eq!(parse("{\"t\":\"2026-10-19T10:00:00\",\"in\":10", DAY), None);
        assert_eq!(parse(&line("2026-10-19T10:00:00.0000000+08:00", 1, 2, 500), DAY), None);
    }

    // A line cut in half by a write in progress says nothing, and the rest of
    // the file still counts.
    #[test]
    fn a_half_written_line_is_skipped() {
        let text = [
            line("2026-10-19T10:00:00.0000000+08:00", 10, 5, 200),
            "{\"t\":\"2026-10-19T10:00:01\"".to_string(),
            line("2026-10-19T10:00:02.0000000+08:00", 20, 5, 200),
        ]
        .join("\n");
        assert_eq!(parse(&text, DAY), Some(Count { total: 40, output: 10 }));
    }

    // A request with no `in` or `out` (an odd one) is still a served request.
    #[test]
    fn a_request_without_counts_counts_as_nothing() {
        let text = serde_json::json!({ "t": "2026-10-19T10:00:00.0000000+08:00", "status": 200 }).to_string();
        assert_eq!(parse(&text, DAY), Some(Count { total: 0, output: 0 }));
    }
}
