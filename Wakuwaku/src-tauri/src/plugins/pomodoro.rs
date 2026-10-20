// A pomodoro, as examples/widgets/pomodoro.ps1 had it: focus for a while,
// then a break, round after round, a long break after every fourth; the
// island shows the time left, and opens when it is time to stop or start
// again. Its larger sizes: how much of it is left (a ring: the seconds left
// of all of it), which round of the set it is, and what comes after.
//
// The time left is said every ten seconds (with nobody about, or her out of
// sight, every thirty: a widget lasts sixty), and each stretch ends on its
// second, whoever is about. Its clock is the wall's, as Get-Date was: a
// computer asleep through the end of one wakes to it over.
use std::time::Duration;

use serde_json::{json, Value};

use super::fmt::{plain, round_even};
use super::{Job, Now, Out, Wait};

// A long break after every so many rounds, and how long it is (minutes).
const EVERY: u64 = 4;
const LONG_BREAK: f64 = 15.0;
const TTL: u64 = 60;
const FOCUS: &str = "#ff5c6c";
const REST: &str = "#34d27b";

pub struct Pomodoro {
    work: f64,
    rest: f64,
    round: u64,
    // In the break after the focus of this round.
    resting: bool,
    // When this stretch ends (ms since 1970); none before the first turn.
    end: Option<u64>,
}

impl Pomodoro {
    // Minutes of focus and of a break (25 and 5 when none are given).
    pub fn new(work: Option<f64>, rest: Option<f64>) -> Pomodoro {
        Pomodoro { work: work.unwrap_or(25.0), rest: rest.unwrap_or(5.0), round: 1, resting: false, end: None }
    }

    fn is_long(&self) -> bool {
        self.round.is_multiple_of(EVERY)
    }

    // How long this stretch is (minutes).
    fn minutes(&self) -> f64 {
        match (self.resting, self.is_long()) {
            (false, _) => self.work,
            (true, true) => LONG_BREAK,
            (true, false) => self.rest,
        }
    }

    // This stretch's words: its label, colour, what the island says at its
    // end, and what comes after it.
    fn words(&self) -> (String, &'static str, String, String) {
        let round = self.round;
        let rest = plain(if self.is_long() { LONG_BREAK } else { self.rest });
        if !self.resting {
            (format!("专注 · 第 {round} 轮"), FOCUS, format!("第 {round} 轮专注结束，休息 {rest} 分钟"), format!("之后休息 {rest} 分钟"))
        } else {
            let label = if self.is_long() { "长休息" } else { "休息" };
            (label.into(), REST, format!("休息结束，开始第 {} 轮", round + 1), format!("之后专注 {} 分钟", plain(self.work)))
        }
    }

    fn start(&mut self, now: u64) {
        self.end = Some(now + (self.minutes() * 60_000.0).round().max(0.0) as u64);
    }

    // The time left, counting down (ms left).
    fn left(&self, left_ms: u64) -> Value {
        let (label, color, _, after) = self.words();
        let secs = left_ms as f64 / 1000.0;
        let all = round_even(self.minutes() * 60.0).max(1.0) as i64;
        let in_set = (self.round - 1) % EVERY + 1;
        let detail = json!({ "sizes": ["s", "m"], "extra": after, "series": { "points": [round_even(secs) as i64], "hi": all, "lo": 0, "unit": "s" }, "rows": [{ "kind": "round", "value": in_set.to_string(), "unit": EVERY.to_string() }] });
        let whole = left_ms / 1000;
        json!({ "id": "pomodoro", "icon": "timer", "color": color, "label": label, "value": format!("{}:{:02}", whole / 60, whole % 60), "detail": detail, "ttl": TTL })
    }

    // Its end: 0:00, and the island opens.
    fn over(&self) -> Value {
        let (label, color, words, _) = self.words();
        json!({ "id": "pomodoro", "icon": "timer", "color": color, "label": label, "value": "0:00", "ttl": TTL, "nudge": words })
    }

    fn next(&mut self) {
        if self.resting {
            self.round += 1;
        }
        self.resting = !self.resting;
    }
}

impl Job for Pomodoro {
    fn turn(&mut self, now: Now, out: &mut dyn Out) -> Wait {
        if self.end.is_none() {
            self.start(now.ms);
        }
        let end = self.end.unwrap_or(now.ms);
        // Over: said so, and the next one begins (one a turn: minutes so few
        // that it is over at once too wait a second).
        if end <= now.ms {
            out.post(self.over());
            self.next();
            self.start(now.ms);
        }
        let end = self.end.unwrap_or(now.ms);
        if end <= now.ms {
            return Wait::every(Duration::from_secs(1));
        }
        let left = end - now.ms;
        out.post(self.left(left));
        // Until the next ten seconds, or its end, a second at least.
        let to_end = left.div_ceil(1000).max(1);
        Wait { busy: Duration::from_secs(to_end.min(10)), idle: Duration::from_secs(to_end.min(30)) }
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests::{golden, record, same};
    use super::*;

    // Focus 0.5 minutes, breaks 0.25, from the start to past the fourth
    // round's long break, every turn when it asked for it: each post as
    // pomodoro.ps1 made them, at the same moments.
    #[test]
    fn rounds_and_breaks_read_as_the_script_posted_them() {
        let (mut theirs, _) = golden("pomodoro");
        // Its last, as the script was stopped: its widget taken away (its
        // finally). Hers goes the same way as it stops (sync, stale).
        assert_eq!(theirs.pop().map(|p| p.1), Some(json!({ "id": "pomodoro", "remove": true })));
        let mut p = Pomodoro::new(Some(0.5), Some(0.25));
        let start = 1_792_000_000_000u64;
        let mut now = start;
        let mut mine: Vec<(f64, Value)> = Vec::new();
        while mine.len() < theirs.len() {
            let (posts, said, wait) = record(&mut p, now);
            assert!(said.is_none());
            mine.extend(posts.into_iter().map(|w| ((now - start) as f64 / 1000.0, w)));
            now += wait.busy.as_millis() as u64;
        }
        for (i, ((t, mine), (t2, theirs))) in mine.iter().zip(&theirs).enumerate() {
            assert!(*t == *t2 && same(mine, theirs), "post {i}: at {t} s {mine}\nat {t2} s {theirs}");
        }
    }

    // Quiet, every thirty seconds; the end on its second all the same. 25
    // and 5 when none are given, the long break after the fourth focus.
    #[test]
    fn its_pace_and_its_rounds() {
        let mut p = Pomodoro::new(None, None);
        let (posts, _, wait) = record(&mut p, 0);
        assert_eq!((posts[0]["value"].as_str(), wait), (Some("25:00"), Wait { busy: Duration::from_secs(10), idle: Duration::from_secs(30) }));
        let (_, _, wait) = record(&mut p, 1_480_000);
        assert_eq!(wait, Wait { busy: Duration::from_secs(10), idle: Duration::from_secs(20) });
        let (posts, _, wait) = record(&mut p, 1_499_500);
        assert_eq!((posts[0]["value"].as_str(), wait.idle), (Some("0:00"), Duration::from_secs(1)));
        let (posts, _, _) = record(&mut p, 1_500_000);
        assert_eq!((posts[0]["nudge"].as_str(), posts[1]["label"].as_str(), posts[1]["value"].as_str()), (Some("第 1 轮专注结束，休息 5 分钟"), Some("休息"), Some("5:00")));
        // So few minutes that each is over at once: one a second, not a storm.
        let mut p = Pomodoro::new(Some(0.000_001), Some(0.000_001));
        let (posts, _, wait) = record(&mut p, 0);
        assert_eq!((posts.len(), wait), (1, Wait::every(Duration::from_secs(1))));
        let (posts, _, wait) = record(&mut p, 1000);
        assert_eq!((posts.len(), wait), (1, Wait::every(Duration::from_secs(1))));
    }
}
