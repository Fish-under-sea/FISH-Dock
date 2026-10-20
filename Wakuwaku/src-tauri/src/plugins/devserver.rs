// Your dev servers, as examples/widgets/devserver.ps1 had them: which are up
// ("3000 ✓ 5173 ✗"); the island opens when one that was up goes down. Any
// answer counts as up, an error page or a redirect too; no answer within
// three seconds is down. Every 15 seconds; with nobody about (quiet) or her
// out of sight, every 45 (a widget lasts 75: one is always there), and at
// once as someone comes back.
use std::collections::HashMap;
use std::time::Duration;

use serde_json::json;

use super::{Job, Now, Out, Wait};

const SECONDS: u64 = 15;
const QUIET_SECONDS: u64 = 45;
const TTL: u64 = SECONDS + 60;
// How long one may take to answer.
pub const ANSWER_IN: Duration = Duration::from_secs(3);

// Whether anything answers at an address (web::answers; a test's own).
pub type Probe = Box<dyn Fn(&str) -> bool + Send>;

pub struct Devserver {
    urls: Vec<String>,
    // Whether each was up the last time.
    was: HashMap<String, bool>,
    lang: &'static str,
    probe: Probe,
}

impl Devserver {
    // "a,b" as typed, its parts; none given, localhost:3000 as the script's
    // own default.
    pub fn new(urls: &str, lang: &'static str, probe: Probe) -> Devserver {
        let urls = if urls.trim().is_empty() { vec!["http://localhost:3000".to_string()] } else { urls.split(',').map(str::trim).filter(|u| !u.is_empty()).map(String::from).collect() };
        Devserver { urls, was: HashMap::new(), lang, probe }
    }
}

// An address as one is asked: one typed without its http:// (localhost:3000)
// is given it (PowerShell would have asked a scheme called "localhost", and
// shown no name for it).
fn address(u: &str) -> String {
    if u.contains("://") {
        u.to_string()
    } else {
        format!("http://{u}")
    }
}

// The few characters a server goes by: its port, or its host when the port
// is its scheme's own.
pub fn short(u: &str) -> Option<String> {
    let url = tauri::Url::parse(&address(u)).ok()?;
    let host = url.host_str()?.to_string();
    Some(match url.port() {
        Some(port) => port.to_string(),
        None => host,
    })
}

impl Job for Devserver {
    fn turn(&mut self, _now: Now, out: &mut dyn Out) -> Wait {
        let wait = Wait { busy: Duration::from_secs(SECONDS), idle: Duration::from_secs(QUIET_SECONDS) };
        // One that is no address: said, and nothing shown (the script's turn
        // failed the same way).
        if let Some(bad) = self.urls.iter().find(|u| short(u).is_none()) {
            out.say(if self.lang == "zh" { format!("看不懂这个地址：{bad}") } else { format!("Not an address: {bad}") });
            return wait;
        }
        let mut marks = Vec::new();
        let mut down = Vec::new();
        for u in &self.urls {
            let up = (self.probe)(&address(u));
            let name = short(u).unwrap_or_default();
            marks.push(format!("{name} {}", if up { "✓" } else { "✗" }));
            if !up && self.was.get(u) == Some(&true) {
                down.push(name);
            }
            self.was.insert(u.clone(), up);
        }
        let all_up = !marks.iter().any(|m| m.contains('✗'));
        let mut widget = json!({ "id": "devserver", "icon": "server", "color": if all_up { "#34d27b" } else { "#ff5c6c" }, "label": "开发服务器", "value": marks.join(" "), "ttl": TTL });
        if !down.is_empty() {
            widget["nudge"] = json!(format!("开发服务器 {} 停了", down.join("、")));
        }
        out.post(widget);
        wait
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests::{golden, record, same};
    use super::*;
    use std::sync::{Arc, Mutex};

    // Four turns as devserver.ps1 posted them: up and an error page (both
    // up), one gone down (the island told), the next gone down, all back.
    #[test]
    fn servers_up_and_down_read_as_the_script_posted_them() {
        let (a, b, c) = ("http://localhost:3000", "http://localhost:5173/api/health", "https://example.com");
        let turns = [[true, true, false], [false, true, false], [false, false, true], [true, true, true]];
        let turn = Arc::new(Mutex::new(0usize));
        let at = turn.clone();
        let asked = Arc::new(Mutex::new(Vec::new()));
        let heard = asked.clone();
        let urls = [a, b, c];
        let probe: Probe = Box::new(move |u: &str| {
            heard.lock().unwrap().push(u.to_string());
            turns[*at.lock().unwrap()][urls.iter().position(|x| *x == u).unwrap()]
        });
        let mut d = Devserver::new(&format!("{a}, {b},{c},"), "zh", probe);
        let (theirs, _) = golden("devserver");
        for (t, (_, theirs)) in theirs.iter().enumerate() {
            *turn.lock().unwrap() = t;
            let (posts, said, wait) = record(&mut d, t as u64 * 15_000);
            assert_eq!((said, wait), (None, Wait { busy: Duration::from_secs(15), idle: Duration::from_secs(45) }));
            assert!(same(&posts[0], theirs), "turn {t}:\n{}\n{theirs}", posts[0]);
        }
        assert_eq!(asked.lock().unwrap().len(), 12);
    }

    #[test]
    fn servers_go_by_their_port_or_host() {
        assert_eq!(short("http://localhost:3000").as_deref(), Some("3000"));
        assert_eq!(short("localhost:5173").as_deref(), Some("5173"));
        assert_eq!(short("https://Example.com").as_deref(), Some("example.com"));
        assert_eq!(short("https://example.com:443/x").as_deref(), Some("example.com"));
        assert_eq!(short("http://[::1]:8080").as_deref(), Some("8080"));
        assert_eq!(short("http://").as_deref(), None);
        assert_eq!(address("localhost:3000"), "http://localhost:3000");
        // None given: localhost:3000. One not an address: said, nothing shown.
        let (posts, _, _) = record(&mut Devserver::new(" ", "zh", Box::new(|u: &str| u == "http://localhost:3000")), 0);
        assert_eq!(posts[0]["value"], "3000 ✓");
        let (posts, said, _) = record(&mut Devserver::new("http://localhost:3000,http://", "en", Box::new(|_: &str| true)), 0);
        assert!(posts.is_empty());
        assert_eq!(said.as_deref(), Some("Not an address: http://"));
    }
}
