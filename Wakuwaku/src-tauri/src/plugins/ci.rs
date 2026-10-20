// CI and pull requests from GitHub, as examples/widgets/ci.ps1 had them: how
// the latest Actions run on main went, and (Prs) the checks on the open pull
// requests. The island opens when a run seen going finishes: what was seen
// is kept from turn to turn (the script, started anew each turn while she
// ran it so, forgot it, and never said so).
//
// Public repositories need no token (GitHub allows 60 requests an hour then;
// a turn is one request, four more with Prs). For private ones, or more
// often: GITHUB_TOKEN in her environment, or the GitHub CLI's (gh auth
// token) if it is there, asked once as the plugin starts.
use std::collections::HashMap;
use std::time::Duration;

use serde_json::{json, Value};

use super::fmt::{escape, id_part};
use super::web::{self, Answer};
use super::{Fetch, Job, Now, Out, Wait};

const MINUTES: u64 = 3;
const TTL: u64 = MINUTES * 60 + 180;
// The branch whose runs are shown (the script's -Branch, which she never set).
const BRANCH: &str = "main";

pub struct Ci {
    repo: String,
    prs: bool,
    // None: not asked for yet; Some(None): none to be had.
    token: Option<Option<String>>,
    // The latest run seen and how it was then.
    seen: HashMap<u64, String>,
    lang: &'static str,
    fetch: Fetch,
}

impl Ci {
    pub fn new(repo: &str, prs: bool, token: Option<Option<String>>, lang: &'static str, fetch: Fetch) -> Ci {
        Ci { repo: repo.trim().to_string(), prs, token, seen: HashMap::new(), lang, fetch }
    }

    fn get(&self, url: &str) -> Result<Value, String> {
        let auth = self.token.clone().flatten().map(|t| format!("Bearer {t}"));
        let mut headers = vec![("user-agent", "wakuwaku"), ("accept", "application/vnd.github+json")];
        if let Some(auth) = &auth {
            headers.push(("authorization", auth));
        }
        match (self.fetch)(url, &headers) {
            Ok(Answer { status: 200..=299, body }) => serde_json::from_str(&body).map_err(|e| e.to_string()),
            Ok(page) => Err(web::refused(&page)),
            Err(fail) => Err(web::trouble(self.lang, &fail)),
        }
    }

    // A turn: the run's widget, then the pull requests'; the first that
    // fails ends it (said, as the script's warning was).
    fn look(&mut self, out: &mut dyn Out) -> Result<(), String> {
        let base = format!("https://api.github.com/repos/{}", self.repo);
        let key = id_part(&self.repo);
        let runs = self.get(&format!("{base}/actions/runs?branch={}&per_page=1", escape(BRANCH)))?;
        out.post(run_widget(&key, &runs, &mut self.seen));
        if self.prs {
            let pulls = self.get(&format!("{base}/pulls?state=open&per_page=20"))?;
            let pulls = pulls.as_array().cloned().unwrap_or_default();
            // Three fit in a widget's few characters.
            let mut marks = Vec::new();
            for pr in pulls.iter().take(3) {
                let sha = pr["head"]["sha"].as_str().unwrap_or("");
                let checks = self.get(&format!("{base}/commits/{sha}/check-runs?per_page=50"))?;
                marks.push(format!("#{}{}", pr["number"], checks_mark(&checks["check_runs"])));
            }
            out.post(prs_widget(&key, pulls.len(), &marks));
        }
        Ok(())
    }
}

impl Job for Ci {
    fn turn(&mut self, _now: Now, out: &mut dyn Out) -> Wait {
        if self.token.is_none() {
            self.token = Some(token());
        }
        if let Err(line) = self.look(out) {
            out.say(line);
        }
        Wait::every(Duration::from_secs(MINUTES * 60))
    }
}

// GITHUB_TOKEN, else the GitHub CLI's, else none.
fn token() -> Option<String> {
    if let Some(t) = std::env::var("GITHUB_TOKEN").ok().filter(|t| !t.trim().is_empty()) {
        return Some(t.trim().to_string());
    }
    let gh = std::env::var_os("PATH").and_then(|p| std::env::split_paths(&p).map(|d| d.join(if cfg!(windows) { "gh.exe" } else { "gh" })).find(|p| p.is_file()))?;
    let mut command = std::process::Command::new(gh);
    command.args(["auth", "token"]).stdin(std::process::Stdio::null()).stderr(std::process::Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // CREATE_NO_WINDOW
        command.creation_flags(0x0800_0000);
    }
    let out = command.output().ok().filter(|o| o.status.success())?;
    Some(String::from_utf8_lossy(&out.stdout).trim().to_string()).filter(|t| !t.is_empty())
}

// A run or a check's state, in a mark and a colour.
fn mark(status: &Value, conclusion: &Value) -> (&'static str, &'static str) {
    if status.as_str() != Some("completed") {
        return ("⏳", "#5e9bff");
    }
    match conclusion.as_str() {
        Some("success") => ("✓", "#34d27b"),
        Some("skipped" | "neutral") => ("✓", "#8e8e93"),
        Some("cancelled") => ("–", "#8e8e93"),
        _ => ("✗", "#ff5c6c"),
    }
}

// The latest run's widget; and, a run seen going now done, the island says
// how it went.
pub fn run_widget(key: &str, runs: &Value, seen: &mut HashMap<u64, String>) -> Value {
    let run = &runs["workflow_runs"][0];
    if runs["total_count"].as_u64().unwrap_or(0) == 0 || !run.is_object() {
        return json!({ "id": format!("ci-{key}"), "icon": "check", "color": "#8e8e93", "label": format!("{BRANCH} · CI"), "value": "没有运行", "ttl": TTL });
    }
    let (status, conclusion) = (&run["status"], &run["conclusion"]);
    let (m, color) = mark(status, conclusion);
    let done = status.as_str() == Some("completed");
    let value = if !done {
        format!("{m} 运行中")
    } else if m == "✗" {
        format!("{m} 失败")
    } else {
        format!("{m} 通过")
    };
    let name = run["name"].as_str().unwrap_or("");
    let mut widget = json!({ "id": format!("ci-{key}"), "icon": "check", "color": color, "label": format!("{BRANCH} · {name}"), "value": value, "ttl": TTL });
    let id = run["id"].as_u64().unwrap_or(0);
    if done && seen.get(&id).is_some_and(|was| was != "completed") {
        let title = run["display_title"].as_str().unwrap_or("");
        widget["nudge"] = json!(if m == "✗" { format!("CI 失败了：{title}") } else { format!("CI 通过了：{title}") });
    }
    // Only the latest run is ever looked at: the one to remember.
    seen.retain(|k, _| *k == id);
    seen.insert(id, status.as_str().unwrap_or("").to_string());
    widget
}

// A pull request's checks in one mark: none yet, one still going, one that
// did not pass, or all passed.
pub fn checks_mark(checks: &Value) -> &'static str {
    let all = checks.as_array().map(Vec::as_slice).unwrap_or(&[]);
    if all.is_empty() {
        "·"
    } else if all.iter().any(|c| c["status"].as_str() != Some("completed")) {
        "⏳"
    } else if all.iter().any(|c| !matches!(c["conclusion"].as_str(), Some("success" | "skipped" | "neutral"))) {
        "✗"
    } else {
        "✓"
    }
}

pub fn prs_widget(key: &str, open: usize, marks: &[String]) -> Value {
    let value = if open > 0 { marks.join(" ") } else { "没有".to_string() };
    let color = if value.contains('✗') {
        "#ff5c6c"
    } else if value.contains('⏳') {
        "#5e9bff"
    } else {
        "#34d27b"
    };
    json!({ "id": format!("prs-{key}"), "icon": "code", "color": color, "label": format!("PR · {open} 个开着"), "value": value, "ttl": TTL })
}

#[cfg(test)]
mod tests {
    use super::super::tests::{golden, record, same, served_with, Route};
    use super::*;

    fn fixture(name: &str) -> String {
        std::fs::read_to_string(format!("{}/../test/fixtures/plugins/{name}", env!("CARGO_MANIFEST_DIR"))).unwrap()
    }

    // Three turns against the recorded answers, as ci.ps1 posted them kept
    // running (the run going, then failed: the island told; then passed: a
    // run seen done already, nothing); the pull requests' marks each turn.
    #[test]
    fn runs_and_pull_requests_read_as_the_script_posted_them() {
        let turn = std::sync::Arc::new(std::sync::Mutex::new(0));
        let at = turn.clone();
        let runs = move || {
            let mut r: Value = serde_json::from_str(&fixture("gh-runs.json")).unwrap();
            let t = *at.lock().unwrap();
            if t >= 1 {
                r["workflow_runs"][0]["status"] = json!("completed");
                r["workflow_runs"][0]["conclusion"] = json!(if t >= 2 { "success" } else { "failure" });
            }
            (200, r.to_string())
        };
        let pulls: Value = serde_json::from_str(&fixture("gh-pulls.json")).unwrap();
        let mut routes: Vec<(String, Route)> = vec![
            ("https://api.github.com/repos/astral-sh/ruff/actions/runs?branch=main&per_page=1".into(), Box::new(runs)),
            ("https://api.github.com/repos/astral-sh/ruff/pulls?state=open&per_page=20".into(), Box::new(|| (200, fixture("gh-pulls.json")))),
        ];
        for pr in pulls.as_array().unwrap() {
            let n = pr["number"].clone();
            routes.push((format!("https://api.github.com/repos/astral-sh/ruff/commits/{}/check-runs?per_page=50", pr["head"]["sha"].as_str().unwrap()), Box::new(move || (200, fixture(&format!("gh-checks-{n}.json"))))));
        }
        let (fetch, asked) = served_with(routes);
        let mut ci = Ci::new(" astral-sh/ruff ", true, Some(None), "zh", fetch);
        let mut mine = Vec::new();
        for t in 0..3 {
            *turn.lock().unwrap() = t;
            let (posts, said, wait) = record(&mut ci, t * 180_000);
            assert_eq!((said, wait), (None, Wait::every(Duration::from_secs(180))));
            mine.extend(posts);
        }
        let (theirs, warnings) = golden("ci");
        assert!(warnings.is_empty());
        assert_eq!(mine.len(), theirs.len());
        for (mine, (_, theirs)) in mine.iter().zip(&theirs) {
            assert!(same(mine, theirs), "\n{mine}\n{theirs}");
        }
        // No token: none sent.
        assert_eq!(asked.lock().unwrap().len(), 15);
    }

    #[test]
    fn a_repository_with_no_runs_and_no_pull_requests() {
        let (fetch, _) = served_with(vec![
            ("https://api.github.com/repos/Octo.Cat/Hello_World/actions/runs?".into(), Box::new(|| (200, fixture("gh-runs-none.json")))),
            ("https://api.github.com/repos/Octo.Cat/Hello_World/pulls?".into(), Box::new(|| (200, "[]".into()))),
        ]);
        let (posts, said, _) = record(&mut Ci::new("Octo.Cat/Hello_World", true, Some(None), "zh", fetch), 0);
        let (theirs, _) = golden("ci-none");
        assert_eq!((posts.len(), said), (2, None));
        assert!(same(&posts[0], &theirs[0].1) && same(&posts[1], &theirs[1].1), "{posts:?}");
    }

    // GitHub saying no (a repository not there, too many requests): its own
    // words, and nothing after it asked for.
    #[test]
    fn github_saying_no_is_said() {
        let (fetch, asked) = served_with(vec![("https://api.github.com/".into(), Box::new(|| (404, r#"{"message":"Not Found","documentation_url":"https://docs.github.com/rest","status":"404"}"#.into())))]);
        let (posts, said, _) = record(&mut Ci::new("no/such", true, Some(Some("t0ken".into())), "zh", fetch), 0);
        assert!(posts.is_empty());
        assert_eq!(said.as_deref(), Some("404: Not Found"));
        assert_eq!(asked.lock().unwrap().len(), 1);
    }

    #[test]
    fn marks_for_runs_and_checks() {
        let m = |s: &str, c: Value| mark(&json!(s), &c);
        assert_eq!((m("queued", Value::Null), m("completed", json!("success")), m("completed", json!("neutral")), m("completed", json!("cancelled")), m("completed", json!("timed_out"))), (("⏳", "#5e9bff"), ("✓", "#34d27b"), ("✓", "#8e8e93"), ("–", "#8e8e93"), ("✗", "#ff5c6c")));
        let checks = |list: Value| checks_mark(&list);
        assert_eq!(checks(json!([])), "·");
        assert_eq!(checks(Value::Null), "·");
        assert_eq!(checks(json!([{ "status": "completed", "conclusion": "success" }, { "status": "in_progress", "conclusion": null }])), "⏳");
        assert_eq!(checks(json!([{ "status": "completed", "conclusion": "skipped" }, { "status": "completed", "conclusion": "action_required" }])), "✗");
        assert_eq!(checks(json!([{ "status": "completed", "conclusion": "neutral" }])), "✓");
        assert_eq!(prs_widget("k", 2, &["#1⏳".into(), "#2✓".into()])["color"], "#5e9bff");
        // Cancelled counts as passed in its words, as the script had it.
        let mut seen = HashMap::new();
        let runs = json!({ "total_count": 1, "workflow_runs": [{ "id": 7, "name": "ci", "status": "completed", "conclusion": "cancelled", "display_title": "x" }] });
        assert_eq!(run_widget("k", &runs, &mut seen)["value"], "– 通过");
        assert!(run_widget("k", &runs, &mut seen).get("nudge").is_none());
    }
}
