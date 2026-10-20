// A reminder, as examples/widgets/stretch.ps1 had it: every so often the
// island opens and says it (a nudge), unless a session needs you just then.
// Between reminders it shows when the next one comes, looked at each minute.
use std::time::Duration;

use serde_json::json;

use super::{Job, Now, Out, Wait};

const WORDS: &str = "坐久了，起来走走";
const MINUTE: u64 = 60_000;

pub struct Stretch {
    minutes: i64,
    // When the next reminder comes (ms since 1970); none before the first turn.
    next: Option<u64>,
}

impl Stretch {
    // Every so many minutes (50 when none are given), a whole number as the
    // script's [int] made it ("2.5" is 2).
    pub fn new(minutes: Option<i64>) -> Stretch {
        Stretch { minutes: minutes.unwrap_or(50), next: None }
    }

    fn after(&self, now: u64) -> u64 {
        now.saturating_add_signed(self.minutes.saturating_mul(MINUTE as i64))
    }
}

impl Job for Stretch {
    fn turn(&mut self, now: Now, out: &mut dyn Out) -> Wait {
        let next = *self.next.get_or_insert_with(|| now.ms.saturating_add_signed(self.minutes.saturating_mul(MINUTE as i64)));
        let left = ((next as f64 - now.ms as f64) / MINUTE as f64).ceil().max(0.0);
        if left <= 0.0 {
            out.post(json!({ "id": "stretch", "icon": "bell", "color": "#34d27b", "label": WORDS, "value": "现在", "ttl": 600, "nudge": WORDS }));
            self.next = Some(self.after(now.ms));
        } else {
            out.post(json!({ "id": "stretch", "icon": "bell", "color": "#34d27b", "label": WORDS, "value": format!("{left} 分钟后"), "ttl": 180 }));
        }
        Wait::every(Duration::from_secs(60))
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests::{golden, record, same};
    use super::*;

    // Every 3 minutes, each minute's post as stretch.ps1 made it: 3, 2, 1
    // minutes to go, now (the island told), then 2 to go (a minute of the
    // next three gone by the time it looks).
    #[test]
    fn reminders_read_as_the_script_posted_them() {
        let (theirs, _) = golden("stretch");
        let mut s = Stretch::new(Some(3));
        for (t, theirs) in &theirs {
            let (posts, said, wait) = record(&mut s, 1_000_000 + (*t * 1000.0) as u64);
            assert_eq!((posts.len(), said, wait), (1, None, Wait::every(Duration::from_secs(60))));
            assert!(same(&posts[0], theirs), "at {t} s:\n{}\n{theirs}", posts[0]);
        }
        let (posts, _, _) = record(&mut Stretch::new(None), 0);
        assert_eq!(posts[0]["value"], "50 分钟后");
        // 0 minutes (a "0.4" typed): every minute, as the script did.
        let mut s = Stretch::new(Some(0));
        assert_eq!(record(&mut s, 0).0[0]["value"], "现在");
        assert_eq!(record(&mut s, 60_000).0[0]["value"], "现在");
    }
}
