// What Our Free Model has served today, as it counts it itself: the token
// card and the Today card's token line read this, so the numbers are the
// local model service's own (`stats.json` in its data directory), not a
// second parsing of Claude Code's or Codex's transcripts.
//
// The file holds `days[YYYY-MM-DD] = { total, models: { id: { input,
// output, reasoning, calls, failed } } }`; the day's `total` is input plus
// output, and a model's `output` is what it answered. Neither a file that
// was never written nor a day with nothing in it is an error: no numbers
// then, and the widgets fall back to what they read before.
use serde_json::Value;
use std::path::PathBuf;

use crate::tokens::Count;

// The local model service's data directory (`~/.dsh-fish`, or DSH_FISH_DATA).
fn stats_file() -> Option<PathBuf> {
    let dir = crate::oflm::data_directory().ok()?;
    Some(dir.join("stats.json"))
}

/// Today's tokens as the local model service served them: the day's total
/// and the output alone. None when it has not run here, served nothing
/// today, or the file cannot be read.
pub fn today(date: &str) -> Option<Count> {
    let text = std::fs::read_to_string(stats_file()?).ok()?;
    parse(&text, date)
}

fn parse(text: &str, date: &str) -> Option<Count> {
    let value: Value = serde_json::from_str(text).ok()?;
    let day = value.get("days")?.get(date)?;
    let total = day.get("total").and_then(Value::as_u64).unwrap_or(0);
    let output = day
        .get("models")
        .and_then(Value::as_object)
        .map(|models| models.values().filter_map(|m| m.get("output").and_then(Value::as_u64)).sum())
        .unwrap_or(0);
    // A day it served nothing in says nothing about the tokens the agents
    // spent elsewhere: the cards keep reading the transcripts then.
    (total > 0).then(|| Count { total, output })
}

#[cfg(test)]
mod tests {
    use super::*;

    const DAY: &str = "2026-10-19";

    fn stats(days: Value) -> String {
        serde_json::json!({ "version": 3, "requests": 12, "days": days }).to_string()
    }

    #[test]
    fn todays_total_and_output_are_summed_over_the_models() {
        let text = stats(serde_json::json!({
            "2026-10-18": { "total": 9_999, "models": { "old": { "input": 1, "output": 2 } } },
            "2026-10-19": {
                "total": 1_234,
                "models": {
                    "big-pickle": { "input": 900, "output": 100, "reasoning": 7 },
                    "beam-free": { "input": 200, "output": 34 },
                },
            },
        }));
        assert_eq!(parse(&text, DAY), Some(Count { total: 1_234, output: 134 }));
    }

    // A day it served nothing in, a day it never had, no file at all, or a
    // file that is not JSON: no numbers, never a panic.
    #[test]
    fn nothing_today_is_no_numbers() {
        let empty_day = stats(serde_json::json!({ DAY: { "total": 0, "models": {} } }));
        assert_eq!(parse(&empty_day, DAY), None);
        let other_day = stats(serde_json::json!({ "2026-10-18": { "total": 40, "models": {} } }));
        assert_eq!(parse(&other_day, DAY), None);
        assert_eq!(parse("{}", DAY), None);
        assert_eq!(parse("not json", DAY), None);
        assert_eq!(parse("[]", DAY), None);
    }

    // A day saved without its models (an older file): the total still counts.
    #[test]
    fn a_day_without_models_keeps_its_total() {
        let text = stats(serde_json::json!({ DAY: { "total": 77 } }));
        assert_eq!(parse(&text, DAY), Some(Count { total: 77, output: 0 }));
    }
}
