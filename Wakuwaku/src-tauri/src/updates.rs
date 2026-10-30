// Her updates, for the exe downloaded from a release (a build run from the
// source tree updates by a build, and is never looked for): Wakuwaku's
// latest release looked at as she starts, then once a day (settings:
// autoUpdateCheck, on unless turned off; "check now" any time), on GitHub
// and on Gitee, its copy for China (the same release there: the exe, and
// <exe>.sha256 beside it), both at once; the newer goes. Gitee's API is
// not used: without an account it allows an address some sixty calls, and
// then refuses it for over an hour (many in China share one address); its
// tags are read instead as git reads them, and the newest one's
// <exe>.sha256 fetched from where its release keeps it. A newer one is
// told on the island once (updateTold). On the person's word (Update, on
// the General page): its exe downloaded into her data folder, from Gitee
// first in China and GitHub first elsewhere (updateFrom: auto, or one of
// them), from the other should that fail; checked against the SHA-256
// GitHub gives for it, or Gitee's <exe>.sha256 when GitHub gives none (none
// at all, the two differing, or the download another: not installed), put
// where hers is, hers kept beside it as <name>.old.exe, and started with
// --after-update <her pid>, plainly, as any program starts another
// (detach::start). It says on her port that it is up (POST /after-update),
// then waits for her to be gone (the port, the taskbar); she quits as the
// menu does once it has said so and run a few seconds more (hand_over).
// Should it end first, its exe go, or no word come (an antivirus taking it
// for a dropper: Kaspersky's behaviour protection took it away as she
// started it with Explorer borrowed as its parent, and she had quit at
// once, zhoupengjie/Wakuwaku#48), it is ended, hers put back, and she
// stays, saying so on the island. A prompt waiting for an answer is
// answered first. The old exe goes once the new one has run a minute; that
// one taken after all before then, the old one started again takes its
// name back (take_name_back, by the note an update leaves in her data
// folder: NOTE). DEVELOPMENT.md says more.
//
// For tests: WAKUWAKU_UPDATE_URL, GitHub's release JSON from elsewhere, and
// WAKUWAKU_UPDATE_GITEE_URL, a Gitee repo elsewhere (its <url>.git/info/refs
// and <url>releases/download/…; only those given are asked then, their exes
// taken from anywhere); WAKUWAKU_UPDATE_FORCE=1, a build from the source
// tree looks and updates too.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use crate::Shared;

const GITHUB_LATEST: &str = "https://api.github.com/repos/Fish-under-sea/FISH-Dock/releases/latest";
// The releases list, for the About page's 更新公告: the five newest, with
// their bodies (the API's /releases/latest says the newest one alone, and
// no body at all).
const GITHUB_RELEASES: &str = "https://api.github.com/repos/Fish-under-sea/FISH-Dock/releases?per_page=5";
// Gitee 的公告源：Gitee 没有「列表发行说明」的公开 API（它的 /api/v5/repos/<owner>/<repo>/releases
// 需要访问令牌且返回形状与 GitHub 不同）。前置条件（配齐才启用，见 notes_sources）：
//   1) 一个可匿名访问的 Gitee 仓库，README 或 releases 页面可作为公告来源；
//   2) FISH_DOCK_NOTES_GITEE 指向它（如 https://gitee.com/fish-under-sea/FISH-Dock）；
//   3) 该仓库的发行说明按 GitHub 同名约定维护（tag_name/name/body/published_at）。
// 未配置时这一源**明确跳过并记录「未配置」**（notes_sources 返回 Skip），不伪装可用、
// 不静默空白（t10 硬要求）。GITEE_REPO 是更新检查那一路的旧占位，仍为空串。
const GITEE_REPO: &str = "";
// 公告源的环境变量：与 plugin_store 的 setting 入口共用读取方式。
const ENV_NOTES_GITHUB: &str = "FISH_DOCK_NOTES_GITHUB";
const ENV_NOTES_GITEE: &str = "FISH_DOCK_NOTES_GITEE";
// Gitee 侧按 GitHub 兼容形状读发行列表的 API 路径（配置了仓库才有意义）。
const GITEE_RELEASES_API: &str = "https://gitee.com/api/v5/repos/{repo}/releases?per_page=5&direction=desc";
// The notes as last read, in her data folder: an offline look still shows
// the last ones read instead of nothing (NOTES_FILE).
const NOTES_FILE: &str = "update-notes.json";
// 公告缓存保留条数走 crate::config::notes_keep()（FISH_DOCK_NOTES_KEEP）。
const GITHUB_REPO: &str = "https://github.com/Fish-under-sea/FISH-Dock/";
const GITHUB_TEST: &str = "WAKUWAKU_UPDATE_URL";
const GITEE_TEST: &str = "WAKUWAKU_UPDATE_GITEE_URL";
pub const AFTER_UPDATE: &str = "--after-update";
const DAY_MS: u64 = 86_400_000;
const HOUR_MS: u64 = 3_600_000;
// How long a new one started by her has to say it is up, and to run on
// after it has (an antivirus that objects to it ends it in its first
// moments) before she goes.
const WORD_WAIT: Duration = Duration::from_secs(20);
const SETTLE: Duration = Duration::from_secs(3);
// In her data folder from the new one's start till it has run a minute:
// what an update put where ({ from, to, exe, old, at }).
const NOTE: &str = "update.json";

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Source {
    GitHub,
    Gitee,
}

impl Source {
    pub fn name(self) -> &'static str {
        match self {
            Source::GitHub => "GitHub",
            Source::Gitee => "Gitee",
        }
    }
}

// What one place says of its latest release: the version, its page, its
// exe by name and the SHA-256 given for it.
#[derive(Clone, Debug, PartialEq)]
pub struct Found {
    pub source: Source,
    pub version: String,
    pub page: String,
    pub exe: Option<String>,
    pub digest: Option<String>,
    pub size: u64,
}

// The release to go by: the newest version either place has, what each
// that has it says of it (GitHub first), and the SHA-256 to check by: the
// one given; none if none is, or if the two differ (`conflict`).
#[derive(Clone, Debug, PartialEq)]
pub struct Release {
    pub version: String,
    pub from: Vec<Found>,
    pub digest: Option<String>,
    pub conflict: bool,
}

impl Release {
    // Its places, the one preferred first.
    fn ordered(&self, gitee_first: bool) -> Vec<&Found> {
        let mut all: Vec<&Found> = self.from.iter().collect();
        all.sort_by_key(|f| (f.source == Source::Gitee) != gitee_first);
        all
    }

    fn page(&self, gitee_first: bool) -> String {
        self.ordered(gitee_first).iter().map(|f| f.page.clone()).find(|p| !p.is_empty()).unwrap_or_default()
    }

    fn can_install(&self) -> bool {
        self.digest.is_some() && self.from.iter().any(|f| f.exe.is_some())
    }
}

struct State {
    // When last looked (ms), and when to look again.
    checked_at: u64,
    next_at: u64,
    checking: bool,
    latest: Option<Release>,
    // What went wrong last: { code, detail }.
    error: Option<Value>,
    // An update under way: download, verify, waiting (a prompt open), restart;
    // where it is downloaded from.
    stage: Option<&'static str>,
    from: Option<Source>,
    got: u64,
    total: u64,
}

static STATE: Mutex<State> = Mutex::new(State { checked_at: 0, next_at: 0, checking: false, latest: None, error: None, stage: None, from: None, got: 0, total: 0 });
// Started by an update, after this pid, and whether that one heard her
// say she was up: said in the log once she is up.
static AFTER: Mutex<Option<(u32, Result<(), String>)>> = Mutex::new(None);
// The word of a new one she started (POST /after-update): its pid and version.
static HEARD: Mutex<Option<(u32, String)>> = Mutex::new(None);
// An update that did not last, its version: told on the island once.
static BLOCKED: Mutex<Option<String>> = Mutex::new(None);
// Her name taken back as she started (take_name_back): said in the log once she is up.
static NAMED_BACK: Mutex<Option<String>> = Mutex::new(None);

pub fn current() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

// A downloaded exe (or one told to, for tests): only those update.
#[derive(Debug,PartialEq)]
struct UpdateMode { installed:bool, source:bool, may_check:bool, may_replace:bool }
fn update_mode(installed:bool,source:bool,forced:bool)->UpdateMode {
    let may_replace=!installed&&(forced||!source);
    UpdateMode{installed,source:!installed&&source,may_check:installed||may_replace,may_replace}
}
fn current_mode()->UpdateMode { update_mode(cfg!(feature="installed"),crate::data::from_source(),std::env::var("WAKUWAKU_UPDATE_FORCE").as_deref()==Ok("1")) }
pub fn may_update() -> bool { current_mode().may_replace }

fn test_url(var: &str) -> Option<String> {
    std::env::var(var).ok().filter(|u| !u.trim().is_empty())
}

fn testing() -> bool {
    test_url(GITHUB_TEST).is_some() || test_url(GITEE_TEST).is_some()
}

// In China, as far as Windows says: its time zone China's, or its language
// Chinese as written there. Asked once.
pub(crate) fn in_china() -> bool {
    static CHINA: OnceLock<bool> = OnceLock::new();
    *CHINA.get_or_init(|| {
        let locale = sys_locale::get_locale().unwrap_or_default().to_ascii_lowercase();
        if locale == "zh" || locale.starts_with("zh-cn") || locale.starts_with("zh-hans") {
            return true;
        }
        #[cfg(windows)]
        {
            use winreg::enums::HKEY_LOCAL_MACHINE;
            use winreg::RegKey;
            let zone = RegKey::predef(HKEY_LOCAL_MACHINE)
                .open_subkey(r"SYSTEM\CurrentControlSet\Control\TimeZoneInformation")
                .and_then(|k| k.get_value::<String, _>("TimeZoneKeyName"))
                .unwrap_or_default();
            if zone.split('\0').next() == Some("China Standard Time") {
                return true;
            }
        }
        false
    })
}

// Gitee first: as set (updateFrom), or by itself in China.
fn gitee_first(sh: &Shared) -> bool {
    match sh.setting("updateFrom").as_str() {
        Some("gitee") => true,
        Some("github") => false,
        _ => in_china(),
    }
}

// Whether one version is newer than another: their numbers one by one (so
// 0.10.0 is after 0.9.0), a missing one 0; one with a part after "-" (a
// prerelease) before the same without.
pub fn newer(theirs: &str, ours: &str) -> bool {
    fn parts(v: &str) -> (Vec<u64>, bool) {
        let v = v.trim().trim_start_matches(['v', 'V']);
        let v = v.split('+').next().unwrap_or(v);
        let (core, pre) = match v.split_once('-') {
            Some((core, _)) => (core, true),
            None => (v, false),
        };
        (core.split('.').map(|n| n.trim().parse().unwrap_or(0)).collect(), pre)
    }
    let ((a, a_pre), (b, b_pre)) = (parts(theirs), parts(ours));
    for i in 0..a.len().max(b.len()) {
        let (x, y) = (a.get(i).copied().unwrap_or(0), b.get(i).copied().unwrap_or(0));
        if x != y {
            return x > y;
        }
    }
    !a_pre && b_pre
}

fn same_version(a: &str, b: &str) -> bool {
    !newer(a, b) && !newer(b, a)
}

fn exe_name(version: &str) -> String {
    format!("wakuwaku-v{version}-windows-x64.exe")
}

// A release's version by its tag, if it counts (no draft, no prerelease).
fn version_of(v: &Value) -> Option<String> {
    if v["draft"] == true || v["prerelease"] == true {
        return None;
    }
    let version = v["tag_name"].as_str()?.trim().trim_start_matches(['v', 'V']).to_string();
    (!version.is_empty()).then_some(version)
}

// A SHA-256 in hex, lowercase.
fn sha256_hex(h: &str) -> Option<String> {
    let h = h.trim();
    (h.len() == 64 && h.chars().all(|c| c.is_ascii_hexdigit())).then(|| h.to_ascii_lowercase())
}

// An <exe>.sha256 as sha256sum writes it ("<hex>  <name>"), or the hex alone.
pub fn sums_of(text: &str) -> Option<String> {
    sha256_hex(text.trim_start_matches('\u{feff}').split_whitespace().next()?)
}

// GitHub's release JSON: its exe by name with the SHA-256 given for it.
pub fn github_release(v: &Value) -> Option<Found> {
    let version = version_of(v)?;
    let name = exe_name(&version);
    let asset = v["assets"].as_array().and_then(|all| all.iter().find(|a| a["name"].as_str() == Some(name.as_str())));
    Some(Found {
        source: Source::GitHub,
        page: v["html_url"].as_str().unwrap_or("").to_string(),
        exe: asset.and_then(|a| a["browser_download_url"].as_str()).map(str::to_string),
        digest: asset.and_then(|a| a["digest"].as_str()).and_then(|d| d.strip_prefix("sha256:")).and_then(sha256_hex),
        size: asset.and_then(|a| a["size"].as_u64()).unwrap_or(0),
        version,
    })
}

// The newest of a repo's tags as git lists them (info/refs: pkt-lines of
// "<sha> refs/tags/<tag>", a peeled one again with ^{}, the first with
// "\0<capabilities>"): of those that are versions (v and a number), none
// with a "-" in it: a prerelease, which tags alone cannot tell otherwise.
pub fn newest_tag(refs: &str) -> Option<String> {
    refs.lines()
        .filter_map(|line| line.split_once(" refs/tags/").and_then(|(_, tag)| tag.split(['\0', ' ']).next()))
        .map(str::trim)
        .filter(|tag| !tag.ends_with("^{}") && !tag.contains('-') && tag.trim_start_matches(['v', 'V']).starts_with(|c: char| c.is_ascii_digit()))
        .map(str::to_string)
        .reduce(|a, b| if newer(&b, &a) { b } else { a })
}

// Gitee: its newest tag, and that tag's release by where Gitee keeps its
// files (<repo>releases/download/<tag>/<file>): its <exe>.sha256 there,
// put up after the exe, so the release is whole; or none of it yet (404).
fn gitee_found(repo: &str) -> Result<Option<Found>, String> {
    let repo = format!("{}/", repo.trim_end_matches('/'));
    let refs = get_text(&format!("{}.git/info/refs?service=git-upload-pack", repo.trim_end_matches('/')), 1 << 20)?.unwrap_or_default();
    let Some(tag) = newest_tag(&refs) else { return Ok(None) };
    let version = tag.trim_start_matches(['v', 'V']).to_string();
    let exe = format!("{repo}releases/download/{tag}/{}", exe_name(&version));
    let Some(sums) = get_text(&format!("{exe}.sha256"), 4096)? else { return Ok(None) };
    Ok(Some(Found { source: Source::Gitee, page: format!("{repo}releases/tag/{tag}"), exe: Some(exe), digest: sums_of(&sums), size: 0, version }))
}

// The newest either place has: the places that have it, and its SHA-256.
pub fn merge(found: Vec<Found>) -> Option<Release> {
    let version = found.iter().map(|f| f.version.clone()).reduce(|a, b| if newer(&b, &a) { b } else { a })?;
    let mut from: Vec<Found> = found.into_iter().filter(|f| same_version(&f.version, &version)).collect();
    from.sort_by_key(|f| f.source == Source::Gitee);
    let mut digests: Vec<&String> = from.iter().filter_map(|f| f.digest.as_ref()).collect();
    digests.dedup();
    let conflict = digests.len() > 1;
    let digest = if conflict { None } else { digests.first().map(|d| d.to_string()) };
    Some(Release { version, from, digest, conflict })
}

// From GitHub or Gitee itself (their release pages, and the downloads they
// redirect to), unless a test gave the release from elsewhere.
fn allowed(url: &str) -> bool {
    testing() || [GITHUB_REPO, GITEE_REPO].iter().any(|repo| !repo.is_empty() && url.get(..repo.len()).is_some_and(|p| p.eq_ignore_ascii_case(repo)))
}

// At most so many redirects (GitHub's exe: one, to release-assets.githubusercontent.com).
const HOPS: usize = 5;

// A GET answered: the answer, the way it came (proxy::Way's name) and the
// hosts it went by ("github.com → release-assets.githubusercontent.com").
struct Answer {
    final_url:String,
    res: ureq::http::Response<ureq::Body>,
    way: String,
    path: String,
}

// A GET not answered: not there (404, as the server says it), or what went
// wrong each way it was tried: "<way>, <hosts>: <error>", "; " between.
enum Miss {
    NotThere,
    Failed(String),
}

// A GET, each way in turn (proxy::web_ways: Windows' proxy first, where one
// is set; then as is) till one is answered. The seconds are the caller's
// whole-call budget (proxy.rs); CONNECT/HEAD clamp inside agent_with.
fn get(url: &str, seconds: u64, accept: Option<&str>) -> Result<Answer, Miss> {
    let mut failed = Vec::new();
    for way in crate::proxy::web_ways(url) {
        match get_way(&way, url, seconds, accept) {
            Ok(answer) => return Ok(answer),
            Err(None) => return Err(Miss::NotThere),
            Err(Some(e)) => failed.push(format!("{}, {e}", way.name())),
        }
    }
    Err(Miss::Failed(failed.join("; ")))
}

// One way: its redirects followed by hand (to https only, unless a test's),
// so what goes wrong says at which host. Err(None): not there (404).
fn get_way(way: &crate::proxy::Way, url: &str, seconds: u64, accept: Option<&str>) -> Result<Answer, Option<String>> {
    let agent = way.agent_with(Duration::from_secs(seconds), |c| c.max_redirects(0)).ok_or_else(|| Some(format!("{}: not a proxy to go through", way.name())))?;
    let mut at = url.to_string();
    let mut hosts = vec![crate::proxy::host_of(url).unwrap_or_default()];
    for _ in 0..=HOPS {
        let path = hosts.join(" → ");
        let mut req = agent.get(&at).header("user-agent", &format!("FISH-Dock/{}", current()));
        if let Some(accept) = accept {
            req = req.header("accept", accept);
        }
        let res = match req.call() {
            Err(ureq::Error::StatusCode(404)) => return Err(None),
            Err(e) => return Err(Some(format!("{path}: {e}"))),
            Ok(res) => res,
        };
        if !res.status().is_redirection() {
            return Ok(Answer { res, way: way.name(), path, final_url:at });
        }
        let to = res.headers().get("location").and_then(|l| l.to_str().ok()).and_then(|l| redirected(&at, l));
        at = to.ok_or_else(|| Some(format!("{path}: a {} to nowhere it may go", res.status().as_u16())))?;
        hosts.push(crate::proxy::host_of(&at).unwrap_or_default());
    }
    Err(Some(format!("{}: more than {HOPS} redirects", hosts.join(" → "))))
}

// Where a redirect goes from `from`: its Location made whole (one that
// starts with / or // is on the same host or scheme); https only, unless a
// test's.
fn redirected(from: &str, to: &str) -> Option<String> {
    let to = to.trim();
    let (scheme, rest) = from.split_once("://")?;
    let url = if to.contains("://") {
        to.to_string()
    } else if let Some(net) = to.strip_prefix("//") {
        format!("{scheme}://{net}")
    } else if to.starts_with('/') {
        format!("{scheme}://{}{to}", rest.split('/').next().unwrap_or(""))
    } else {
        return None;
    };
    (url.starts_with("https://") || (testing() && url.starts_with("http://"))).then_some(url)
}

// A JSON from there; none if it is not there (404: no release yet). The
// budget comes from proxy.rs (FISH_DOCK_UPDATE_TIMEOUT_MS, default 20 s):
// one place sets every timeout (red line).
fn get_json(url: &str, accept: &str) -> Result<Option<Value>, String> {
    match get(url, crate::proxy::update_timeout().as_secs(), Some(accept)) {
        Err(Miss::NotThere) => Ok(None),
        Err(Miss::Failed(e)) => Err(e),
        Ok(mut a) => a.res.body_mut().read_json().map(Some).map_err(|e| format!("{}, {}: {e}", a.way, a.path)),
    }
}

// A text from there (at most `limit` bytes); none if it is not there (404).
fn get_text(url: &str, limit: u64) -> Result<Option<String>, String> {
    match get(url, crate::proxy::update_timeout().as_secs(), None) {
        Err(Miss::NotThere) => Ok(None),
        Err(Miss::Failed(e)) => Err(e),
        Ok(mut a) => a.res.body_mut().with_config().limit(limit).read_to_string().map(Some).map_err(|e| format!("{}, {}: {e}", a.way, a.path)),
    }
}

// Where each place's release is looked at (GitHub's API, Gitee's repo):
// both, or those a test gives.
fn endpoints() -> Vec<(Source, String)> {
    if !testing() {
        return vec![(Source::GitHub, crate::config::update_latest())];
    }
    [(Source::GitHub, test_url(GITHUB_TEST)), (Source::Gitee, test_url(GITEE_TEST))].into_iter().filter_map(|(source, url)| Some((source, url?))).collect()
}

// API 限流时只取官方 Release 重定向的版本和发行页，不制造未验证的安装载荷。
fn public_release_url(url:&str)->Option<Found>{
    let url=tauri::Url::parse(url).ok()?;
    if url.scheme()!="https"||url.host_str()!=Some("github.com")||url.port().is_some()||!url.username().is_empty()||url.password().is_some()||url.query().is_some()||url.fragment().is_some(){return None;}
    let tag=url.path().strip_prefix("/Fish-under-sea/FISH-Dock/releases/tag/")?;
    let version=tag.strip_prefix('v').unwrap_or(tag);crate::plugin_manifest::version(version).ok()?;
    Some(Found{source:Source::GitHub,version:version.into(),page:url.as_str().into(),exe:None,digest:None,size:0})
}
fn public_latest()->Result<Option<Found>,String>{
    match get(&format!("{}releases/latest", crate::config::update_repo()),10,None){
        Ok(answer)=>public_release_url(&answer.final_url).map(Some).ok_or_else(||"GitHub 官方发行页没有有效的正式版本重定向".into()),
        Err(Miss::NotThere)=>Ok(None),Err(Miss::Failed(error))=>Err(error),
    }
}
fn github_sources(api:impl FnOnce()->Result<Option<Found>,String>,page:impl FnOnce()->Result<Option<Found>,String>)->Result<Option<Found>,String>{
    match api(){
        Ok(Some(found))=>Ok(Some(found)),
        Ok(None)=>page(),
        Err(api_error)=>match page(){Ok(found)=>Ok(found),Err(page_error)=>Err(format!("API: {api_error}; official release page: {page_error}"))},
    }
}

// What one place says.
fn fetch(source: Source, url: &str) -> Result<Option<Found>, String> {
    match source {
        Source::GitHub => {
            let api=||Ok(get_json(url,"application/vnd.github+json")?.as_ref().and_then(github_release));
            if url==crate::config::update_latest().as_str() { github_sources(api,public_latest) } else {api()}
        },
        Source::Gitee => gitee_found(url),
    }
}

// Both places looked at together, and the newest of what they say. Neither
// answering is an error; one not, only said in the log.
fn fetch_latest(sh: &Shared) -> Result<Option<Release>, String> {
    let asked = endpoints();
    let got: Vec<(Source, Result<Option<Found>, String>)> = std::thread::scope(|s| {
        let looks: Vec<_> = asked.iter().map(|(source, url)| (*source, s.spawn(move || fetch(*source, url)))).collect();
        looks.into_iter().map(|(source, look)| (source, look.join().unwrap_or_else(|_| Err("stopped".into())))).collect()
    });
    let mut found = Vec::new();
    let mut failed = Vec::new();
    for (source, got) in got {
        match got {
            Ok(f) => found.extend(f),
            Err(e) => failed.push(format!("{}: {e}", source.name())),
        }
    }
    if !failed.is_empty() && failed.len() == asked.len() {
        return Err(failed.join("; "));
    }
    for e in &failed {
        sh.log(&format!("update: could not look on {e}"));
    }
    Ok(merge(found))
}

fn error(code: &str, detail: impl Into<String>) -> Value {
    json!({ "code": code, "detail": detail.into() })
}

fn places(r: &Release) -> String {
    r.from.iter().map(|f| f.source.name()).collect::<Vec<_>>().join(", ")
}

// Looked at now (one look at a time); told on the island if newer.
pub fn check_now(sh: &Arc<Shared>) {
    // 安装版可检查官方发行，但仍禁止独立替换宿主 EXE（may_update）。
    {
        let mut s = STATE.lock().unwrap();
        if s.checking {
            return;
        }
        s.checking = true;
    }
    sh.push_settings();
    let got = fetch_latest(sh);
    let now = crate::now_ms();
    {
        let mut s = STATE.lock().unwrap();
        s.checking = false;
        s.checked_at = now;
        // An update that did not last says so till Update is pressed again.
        let blocked = s.error.as_ref().is_some_and(|e| e["code"] == "blocked");
        match &got {
            Ok(release) => {
                s.latest = release.clone();
                s.next_at = now + DAY_MS;
                if s.stage.is_none() && !blocked {
                    // GitHub and Gitee not agreeing on a newer one's SHA-256.
                    s.error = release.as_ref().filter(|r| r.conflict && newer(&r.version, current())).map(|r| error("mismatch", r.version.clone()));
                }
            }
            // Quietly again in an hour.
            Err(e) => {
                s.next_at = now + HOUR_MS;
                if !blocked {
                    s.error = Some(error("check", e.clone()));
                }
            }
        }
    }
    match &got {
        Ok(Some(r)) if newer(&r.version, current()) => sh.log(&format!("update: v{} out ({}), newer than v{}", r.version, places(r), current())),
        Ok(Some(r)) => sh.log(&format!("update: v{} is the latest ({}); this is v{}", r.version, places(r), current())),
        Ok(None) => sh.log("update: no release to go by (none, a draft or a prerelease)"),
        Err(e) => sh.log(&format!("update: could not look ({e}); again in an hour")),
    }
    if let Ok(Some(r)) = &got {
        if r.conflict {
            sh.log(&format!("update: v{}: GitHub and Gitee give different SHA-256s; not to be installed", r.version));
        }
    }
    tell(sh);
    // 公告一小时（或上次失败）才再读一次，不跟着每次检查都问一遍。
    let stale = {
        let n = NOTES.lock().unwrap();
        n.as_ref().map(|c| c.error.is_some() || now.saturating_sub(c.at) > HOUR_MS).unwrap_or(true)
    };
    if stale {
        refresh_notes(sh);
    }
    sh.push_settings();
}

// A newer one told on the island, once for each, and an update that did
// not last, once (before anything else): only while she is in sight and
// no session needs you (else at a later look).
fn tell(sh: &Arc<Shared>) {
    if !sh.is_visible() || sh.pet.lock().unwrap().wants_you() {
        return;
    }
    let lang = sh.lang();
    let blocked = BLOCKED.lock().unwrap().take();
    if let Some(version) = blocked {
        let words = crate::i18n::t(lang, "update.blocked").replace("{version}", &version);
        sh.to_pages("pet:nudge", json!({ "id": "update", "icon": "flag", "words": words, "label": crate::i18n::t(lang, "update.blockedTap"), "value": "", "private": false, "open": { "tab": "update" } }));
        sh.log(&format!("update: v{version} not lasting told on the island"));
        return;
    }
    let Some(r) = STATE.lock().unwrap().latest.clone().filter(|r| newer(&r.version, current())) else { return };
    if sh.setting("updateTold").as_str() == Some(r.version.as_str()) {
        return;
    }
    let words = crate::i18n::t(lang, "update.out").replace("{version}", &r.version);
    sh.to_pages("pet:nudge", json!({ "id": "update", "icon": "star", "words": words, "label": crate::i18n::t(lang, "update.tap"), "value": "", "private": false, "open": { "tab": "update" } }));
    sh.change(json!({ "updateTold": r.version }));
    sh.log(&format!("update: v{} told on the island", r.version));
}

// As she starts: a download cut off cleared away, the old exe gone after a
// minute's run, and the looks
// (twenty seconds in, then as due; every ten minutes it is seen whether a
// look is due, or a newer one waits to be told). Her name taken back
// (take_name_back): told on the island a few seconds in, once her pages
// are up.
pub fn start(sh: &Arc<Shared>) {
    // 更新公告不跟「能不能装」绑定：安装版也要有；盘上缓存先顶上，读不到再说
    // 读不到。所以放在 may_update 判断之前。
    refresh_notes(sh);
    if cfg!(feature = "installed") {
        sh.log("update: installed release; official checks enabled, upgrades use package installers");
        schedule_checks(sh);
        return;
    }
    if let Some((pid, told)) = AFTER.lock().unwrap().clone() {
        match told {
            Ok(()) => sh.log(&format!("update: v{} up, after pid {pid} ended (told it she was up)", current())),
            Err(e) => sh.log(&format!("update: v{} up, after pid {pid} ended (could not tell it she was up: {e})", current())),
        }
    }
    if let Some(line) = NAMED_BACK.lock().unwrap().take() {
        sh.log(&line);
    }
    if BLOCKED.lock().unwrap().is_some() {
        let soon = sh.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_secs(5));
            tell(&soon);
        });
    }
    // A download cut off (she quit in the middle of it): its .part gone; and an
    // installer an update has already used (#27): nobody will need it again.
    for name in sweep_update_downloads(&sh.dir) {
        sh.log(&format!("update: {name}, left by an update, removed"));
    }
    let tidy = sh.clone();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_secs(60));
        remove_old(&tidy);
    });
    if !may_update() {
        sh.log("update: run from the source tree: not looked for");
        return;
    }
    schedule_checks(sh);
}

fn schedule_checks(sh:&Arc<Shared>){
    let sh = sh.clone();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_secs(20));
        loop {
            let due = crate::now_ms() >= STATE.lock().unwrap().next_at;
            if due && sh.setting("autoUpdateCheck") != false {
                check_now(&sh);
            } else {
                tell(&sh);
            }
            std::thread::sleep(Duration::from_secs(600));
        }
    });
}

// --- 更新公告（About 页）：最近几版发行说明 -------------------------------------------
// 从前没有这一块：view() 只给 latest（一个版本号、页面、有无 exe），前端
// aboutHTML 却读 snap.update.notes —— 永远是 undefined，于是永远停在「正在读取
// 更新公告…」。这里补上：读 GitHub 的 /releases（带代理、20 秒超时，get_json），
// 取前五条，落盘缓存；view 给 { state, entries, at, offline, error }。
//
// state: loading（还没读过）| ok（有内容，缓存那份也算）| error（读失败且没有内容，
// 带 error）| empty（读通了，但仓库里还没有发行）| offline（一次都没读到过，盘上也
// 没有缓存）。次序见 notes_state；前端按这五态画「有内容 / 骨架 / 还没有 / 读不到 +
// 重试」，另有 8 秒兜底，见 settings.js notesKind 与 aboutNotes。empty 与 offline
// 分开，是因为仓库没有发行时不是「加载不出来」——那是「还没有」，不该让人以为网坏了；
// 读失败但盘上有旧的，仍报 ok 并把旧公告给出去，同时 error 字段带着这次为什么没读到。
// source：这份公告是哪条源读到的（GitHub/Gitee/缓存回填时空），前端据此标「来源」。
#[derive(Clone, Default)]
struct NotesCache {
    entries: Vec<Value>,
    at: u64,
    error: Option<Value>,
    offline: bool,
    source: Option<String>,
}
static NOTES: Mutex<Option<NotesCache>> = Mutex::new(None);
static NOTES_BUSY: Mutex<bool> = Mutex::new(false);

// 一份 releases 列表 JSON 里的条目：草稿与预发布跳过，最多 keep 条；
// 每条 { version, title, body, at }。body 是 Markdown，前端按纯文本显示。
// 时间字段：GitHub 叫 published_at，Gitee 叫 created_at（实测 mindspore 仓库
// 200 应答：tag_name/name/body 兼容、时间字段是 created_at）——两个都认。
pub fn notes_entries(list: &Value, keep: usize) -> Vec<Value> {
    let Some(rows) = list.as_array() else { return Vec::new() };
    rows.iter()
        .filter_map(|v| {
            let version = version_of(v)?;
            let at = v["published_at"].as_str().or_else(|| v["created_at"].as_str()).unwrap_or("");
            Some(json!({
                "version": version,
                "title": v["name"].as_str().unwrap_or("").trim(),
                "body": v["body"].as_str().unwrap_or(""),
                "at": at,
            }))
        })
        .take(keep)
        .collect()
}

fn notes_path(dir: &Path) -> PathBuf {
    dir.join(NOTES_FILE)
}

// 落盘的那份（上次读到的）：没有或读不懂就没有 —— 不能让它挡住新的读取。
// source（上次哪条源读到的）一起落盘，重开还是那句话。
fn notes_load(dir: &Path) -> Option<NotesCache> {
    let text = std::fs::read_to_string(notes_path(dir)).ok()?;
    let v: Value = serde_json::from_str(&text).ok()?;
    let entries = v["entries"].as_array().cloned().unwrap_or_default();
    let source = v["source"].as_str().map(str::to_string);
    Some(NotesCache { entries, at: v["at"].as_u64().unwrap_or(0), error: None, offline: false, source })
}

fn notes_save(dir: &Path, c: &NotesCache) {
    let body = json!({ "entries": c.entries, "at": c.at, "source": c.source });
    if let Ok(text) = serde_json::to_string(&body) {
        let _ = std::fs::write(notes_path(dir), text);
    }
}

// ── 公告的多源读取（t10）──────────────────────────────────────────────
// 一条公告源：读它的列表（GitHub 兼容形状），或为什么没读它。Skip 是「明确跳过」：
// 未配置、或显式指定了别的源——记录在案但不冒充失败（失败=试了没成）。
enum NotesSource {
    Read { label: &'static str, url: String },
    Skipped { label: &'static str, why: &'static str },
}

// 一次要读哪些源、按什么顺序（t10：自动 = GitHub → Gitee；显式指定就只读指定的）。
// preferring 来自设置（updateFrom：auto/gitee/github），决定 Gitee 的位置：
//   auto → GitHub 先、Gitee 兜底；gitee → Gitee 先、GitHub 兜底；github → 只 GitHub。
// Gitee 未配置（FISH_DOCK_NOTES_GITEE 空）时**明确跳过**：Skip{why:"未配置"}，
// 不伪装可用、不静默空白。测试时读 WAKUWAKU_UPDATE_URL 给的地址（原有行为）。
// 公告源的配置（注入式：测试传值，运行时从环境读一次——读取在这里，别处不碰
// 进程全局，红线 9）。github 是列表地址（默认 GITHUB_RELEASES）；gitee 是仓库
// 配置（空 = 未配置）。preferring 是设置里的 updateFrom。
#[derive(Clone, Debug, Default)]
pub(crate) struct NotesConfig {
    pub(crate) github: String,
    pub(crate) gitee: String,
    pub(crate) preferring: String,
}

impl NotesConfig {
    // 运行时从环境与设置读（preferring 由 refresh_notes 传入：它持有 Shared）。
    fn from_environment(preferring: &str) -> Self {
        NotesConfig {
            github: crate::plugin_store::setting(ENV_NOTES_GITHUB, &crate::config::update_releases()),
            gitee: crate::plugin_store::setting(ENV_NOTES_GITEE, ""),
            preferring: preferring.to_string(),
        }
    }
}

// Gitee 配置值接受两种写法：完整地址（https://gitee.com/<owner>/<repo>，主机名校验
// 后取 owner/repo 段）或裸 <owner>/<repo>。别的形状是配置错误：Skip 且写明原因。
fn gitee_repo_path(configured: &str) -> Result<String, &'static str> {
    let trimmed = configured.trim().trim_end_matches('/');
    let path = match trimmed.split_once("://") {
        Some((scheme, rest)) => {
            if !scheme.eq_ignore_ascii_case("https") || !rest.split('/').next().unwrap_or("").eq_ignore_ascii_case("gitee.com") {
                return Err("配置的不是 gitee.com 的仓库地址");
            }
            // gitee.com/<owner>/<repo> → <owner>/<repo>
            rest.split_once('/').map(|(_, after)| after.to_string()).unwrap_or_default()
        }
        None => trimmed.to_string(),
    };
    let parts: Vec<&str> = path.split('/').filter(|p| !p.is_empty()).collect();
    match parts.len() {
        2 if parts.iter().all(|p| p.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_'))) => Ok(parts.join("/")),
        _ => Err("需要 <owner>/<repo> 或完整的 Gitee 仓库地址"),
    }
}

// 一次要读哪些源、按什么顺序（t10：自动 = GitHub → Gitee；显式指定就只读指定的）。
//   auto → GitHub 先、Gitee 兜底；gitee → Gitee 先、GitHub 兜底；github → 只 GitHub。
// Gitee 未配置时**明确跳过**（Skip 写明原因），不伪装可用、不静默空白。
// 测试模式下只读 WAKUWAKU_UPDATE_URL 给的地址（原有行为）。
fn notes_sources(config: &NotesConfig) -> Vec<NotesSource> {
    let github = |url: String| NotesSource::Read { label: "GitHub", url };
    let gitee_url = |repo: &str| GITEE_RELEASES_API.replace("{repo}", repo);
    let gitee = || match config.gitee.as_str() {
        configured if configured.is_empty() => NotesSource::Skipped { label: "Gitee", why: "未配置（FISH_DOCK_NOTES_GITEE 为空）" },
        configured => match gitee_repo_path(configured) {
            Ok(repo) => NotesSource::Read { label: "Gitee", url: gitee_url(&repo) },
            Err(why) => NotesSource::Skipped { label: "Gitee", why },
        },
    };
    if testing() {
        return vec![match test_url(GITHUB_TEST) {
            Some(url) => github(url),
            None => NotesSource::Skipped { label: "GitHub", why: "测试变量为空" },
        }];
    }
    match config.preferring.as_str() {
        "gitee" => vec![gitee(), github(config.github.clone())],
        "github" => vec![github(config.github.clone())],
        _ => vec![github(config.github.clone()), gitee()],
    }
}

// 拉一次公告：按顺序试每一条源，第一条读到的就返回；每条源的结局（读到/失败/跳过）
// 都进 `log`（一句话人话）。返回 (entries, 用的源名)；全部失败返回 Err（人话合并）。
// 配置注入（不读进程环境），测试与运行时共用同一条逻辑。
fn fetch_notes_once(config: &NotesConfig, log: &mut Vec<String>) -> Result<(Vec<Value>, &'static str), String> {
    let mut failures: Vec<String> = Vec::new();
    for source in notes_sources(config) {
        match source {
            NotesSource::Skipped { label, why } => {
                log.push(format!("{label} 源跳过：{why}"));
            }
            NotesSource::Read { label, url } => {
                // 只读 GitHub 自己的 API、配置过的 Gitee API（或测试给的地址），
                // 别的一律不去（原有安全边界）。
                let ours = testing() || url.starts_with("https://api.github.com/") || url.starts_with("https://gitee.com/api/v5/");
                if !ours {
                    failures.push(format!("{label}：地址不在允许列表（{url}）"));
                    continue;
                }
                match get_json(&url, "application/vnd.github+json") {
                    Ok(Some(list)) => {
                        let entries = notes_entries(&list, crate::config::notes_keep());
                        if entries.is_empty() {
                            failures.push(format!("{label}：读到了列表但没有可用条目"));
                            continue;
                        }
                        log.push(format!("{label} 源读到 {} 条", entries.len()));
                        return Ok((entries, label));
                    }
                    Ok(None) => {
                        failures.push(format!("{label}：仓库还没有发行（404）"));
                    }
                    Err(e) => {
                        failures.push(format!("{label}：{}", notes_plain_error(&e)));
                    }
                }
            }
        }
    }
    Err(failures.join("；").trim_start_matches('；').to_string())
}

// 人话错误：ureq/超时/代理的原始字符串常常是英文长串或异常文字，这里换成一句
// 用户能懂的话（细节仍带在后面，但先给结论）。供 UI「失败原因」一行用。
fn notes_plain_error(raw: &str) -> String {
    if raw.contains("timed out") || raw.contains("timeout") {
        format!("连接超时（{raw}）")
    } else if raw.contains("connect") || raw.contains("Connection") || raw.contains("proxy") {
        format!("连不上（{raw}）")
    } else if raw.contains("403") {
        format!("接口限流（稍后再试；{raw}）")
    } else if raw.contains("404") {
        format!("接口不存在（{raw}）")
    } else {
        raw.to_string()
    }
}

// 公告源顺序跟着「从哪里下载（updateFrom）」设置走（规格 6：一处选择管两件事）。
// refresh_notes 读设置、NotesConfig::from_environment 读环境，都在刷新线程里做，
// fetch_notes_once 只用注入值（测试不碰进程环境，红线 9）。
fn fetch_notes(sh: &Shared, config: &NotesConfig) -> Result<Vec<Value>, String> {
    // 顺序记录进 her 日志（每源一句），刷新失败原因进快照。
    let mut log: Vec<String> = Vec::new();
    let got = fetch_notes_once(config, &mut log);
    for line in &log {
        sh.log(&format!("update: notes {line}"));
    }
    *NOTES_FAILURE.lock().unwrap() = got.as_ref().err().cloned();
    if let Ok((_, label)) = got.as_ref() {
        *NOTES_LAST_SOURCE.lock().unwrap() = Some(label.to_string());
    }
    got.map(|(entries, _)| entries)
}
// 最近一次公告读取的人话失败原因（全部源都失败时才有值），进 notes_view 给前端。
static NOTES_FAILURE: Mutex<Option<String>> = Mutex::new(None);
// 最近一次读到的源名（GitHub / Gitee），进 NotesCache.source 给前端。
static NOTES_LAST_SOURCE: Mutex<Option<String>> = Mutex::new(None);

// 读一次（一次只读一次），读完落盘并推给设置页。缓存的源标记与「离线/上次同步」
// 一起进快照（stale-while-revalidate：先给旧的，读到新的再换）。
pub fn refresh_notes(sh: &Arc<Shared>) {
    {
        let mut busy = NOTES_BUSY.lock().unwrap();
        if *busy {
            return;
        }
        *busy = true;
    }
    let sh = sh.clone();
    std::thread::spawn(move || {
        // 源顺序跟着设置走；环境变量在这里读一次（测试走注入，不碰进程全局）。
        let config = NotesConfig::from_environment(sh.setting("updateFrom").as_str().unwrap_or("auto"));
        // 先在盘上找上次那份：第一次打开时就有东西看，离线不会永远是空。
        if NOTES.lock().unwrap().is_none() {
            if let Some(old) = notes_load(&sh.dir) {
                *NOTES.lock().unwrap() = Some(old);
                sh.push_settings();
            }
        }
        let got = fetch_notes(&sh, &config);
        let now = crate::now_ms();
        {
            let mut slot = NOTES.lock().unwrap();
            let prior: Option<NotesCache> = (*slot).clone();
            let had = prior.as_ref().map(|c| !c.entries.is_empty()).unwrap_or(false);
            let keep_entries = prior.as_ref().map(|c| c.entries.clone()).unwrap_or_default();
            let keep_at = prior.as_ref().map(|c| c.at).unwrap_or(0);
            let keep_source = prior.as_ref().and_then(|c| c.source.clone());
            let cache = match got {
                Ok(entries) if !entries.is_empty() => NotesCache { entries, at: now, error: None, offline: false, source: NOTES_LAST_SOURCE.lock().unwrap().clone() },
                Ok(_) => NotesCache { entries: keep_entries, at: keep_at, error: None, offline: true, source: keep_source },
                Err(e) => {
                    sh.log(&format!("update: release notes not read ({e})"));
                    NotesCache { entries: keep_entries, at: keep_at, error: Some(error("notes", e)), offline: !had, source: keep_source }
                }
            };
            let has = !cache.entries.is_empty();
            *slot = Some(cache.clone());
            if has {
                notes_save(&sh.dir, &cache);
            }
        }
        *NOTES_BUSY.lock().unwrap() = false;
        sh.push_settings();
    });
}

// 公告快照该报哪一态：有内容 → ok（缓存优先，读失败也先把上次那份给出去）；读失败
// 且没有内容 → error；读通了但仓库里没有发行 → empty；一次都没读到过 → offline。
// 前端 settings.js notesKind 按同样的次序画四态；单独一个函数是为了能直接测。
fn notes_state(c: &NotesCache) -> &'static str {
    if !c.entries.is_empty() {
        "ok"
    } else if c.error.is_some() {
        "error"
    } else if c.offline {
        "empty"
    } else {
        "offline"
    }
}

fn notes_view() -> Value {
    let slot = NOTES.lock().unwrap().clone();
    let failure = NOTES_FAILURE.lock().unwrap().clone();
    match slot {
        None => json!({ "state": "loading", "entries": [], "at": 0, "offline": false }),
        Some(c) => json!({
            "state": notes_state(&c),
            "entries": c.entries,
            "at": c.at,
            "offline": c.offline,
            "error": c.error,
            // 这次读到用的哪条源（缓存那份也一样带着）；全部失败时没有值。
            "source": c.source,
            // 最近一次全部源失败的人话原因（有缓存时前端在列表下补一行）。
            "failure": failure,
        }),
    }
}

// For the settings: this version, the latest (its page the preferred
// place's), and how an update goes.
pub fn view(sh: &Shared) -> Value {
    let mode=current_mode();
    let first = gitee_first(sh);
    let s = STATE.lock().unwrap();
    let latest = s.latest.as_ref().map(|r| {
        json!({
            "version": r.version,
            "page": r.page(first),
            "newer": newer(&r.version, current()),
            // 安装版也「能装」：装的是发行包里的安装器（Update 增量包，其次 Setup
            // 完整包），不是把她自己换掉（install_installer）。
            "canInstall": mode.installed || r.can_install(),
            "installer": mode.installed,
            "places": r.from.iter().map(|f| f.source.name()).collect::<Vec<_>>(),
        })
    });
    json!({
        "current": current(),
        "may": mode.may_replace,
        "mayCheck": mode.may_check,
        "installed": mode.installed,
        "source": mode.source,
        "checking": s.checking,
        "checkedAt": s.checked_at,
        "latest": latest,
        "error": s.error,
        "stage": s.stage,
        "from": s.from.map(Source::name),
        "got": s.got,
        "total": s.total,
        "notes": notes_view(),
    })
}

// 安装版要的包：优先增量 Update 包，其次完整 Setup 包（两份都在同一条发行里，
// 见 docs/release-2.0.0.md 的「双包」）。
pub fn installer_names(version: &str) -> [String; 2] {
    [format!("FISH-Dock-Update-{version}.exe"), format!("FISH-Dock-Setup-{version}.exe")]
}

// 下载地址按 releases/download 的固定形状拼出来（和 Gitee 那边一样，不依赖
// API 给的资产列表），tag 是 v 开头。
fn asset_url(repo: &str, version: &str, name: &str) -> String {
    format!("{}/releases/download/v{version}/{name}", repo.trim_end_matches('/'))
}

// 一个 <name>.sha256（sha256sum 的形状："<hex>  <name>"，或者只有 hex），
// 或者一份 SHA256SUMS：按名字挑那一行。
pub fn digest_of(text: &str, name: &str) -> Option<String> {
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let mut it = line.split_whitespace();
        let hex = it.next()?;
        let only = it.next().unwrap_or("");
        if only.is_empty() || only.trim_start_matches('*') == name {
            if let Some(h) = sha256_hex(hex) {
                return Some(h);
            }
        }
    }
    None
}

// 安装版的一次更新：下载发行里的安装器（优先 Update 增量包，其次 Setup 完整包），
// 按发行给出的 SHA-256 校验，然后拉起它并退出她。全程不打开浏览器：没有
// settings_open_site，也没有 open_page。
fn install_installer(sh: &Arc<Shared>, r: &Release) -> Result<(), Value> {
    if r.conflict {
        return Err(error("mismatch", r.version.clone()));
    }
    // 每一处按「哪来的」配一对（不是只有页面）：下载中的进度要说真话——从哪里
    // 下就报哪里（原来这里写死 GitHub，选 Gitee 的用户会看到错的一行）。
    let places: Vec<(Source, String)> = r.ordered(gitee_first(sh)).iter().map(|f| (f.source, f.page.clone())).filter(|(_, p)| !p.is_empty()).collect();
    let names = installer_names(&r.version);
    let mut failed = error("noExe", "");
    for (source, page) in places {
        let Some((repo, _tag)) = page.split_once("/releases/tag/") else { continue };
        let repo = repo.to_string();
        for name in &names {
            let url = asset_url(&repo, &r.version, name);
            if !allowed(&url) {
                failed = error("elsewhere", url);
                continue;
            }
            let digest = match get_text(&format!("{url}.sha256"), 4096).ok().flatten().and_then(|t| digest_of(&t, name)) {
                Some(d) => d,
                None => {
                    failed = error("noDigest", name.clone());
                    continue;
                }
            };
            let which = if name.contains("Update") { "update" } else { "setup" };
            let part = sh.dir.join(format!("installer-v{}-{which}.exe", r.version));
            {
                let mut s = STATE.lock().unwrap();
                s.stage = Some("download");
                s.from = Some(source);
                s.got = 0;
                s.total = 0;
            }
            sh.push_settings();
            let (got, how) = match download(sh, &url, &part) {
                Ok(got) => got,
                Err(e) => {
                    let _ = std::fs::remove_file(&part);
                    sh.log(&format!("update: {name} not downloaded: {e}"));
                    failed = error("download", format!("{name}: {e}"));
                    continue;
                }
            };
            stage(sh, "verify");
            if got != digest {
                let _ = std::fs::remove_file(&part);
                sh.log(&format!("update: {name}: SHA-256 {got}, not {digest}"));
                failed = error("digest", format!("{name}: {got} ≠ {digest}"));
                continue;
            }
            sh.log(&format!("update: {name} downloaded ({how}), its SHA-256 as given"));
            stage(sh, "restart");
            run_installer(&part).map_err(|e| error("start", e))?;
            sh.log(&format!("update: installer {name} started; she quits for it"));
            sh.quit(crate::data::Quit::ByChoice, "an update's handover");
            return Ok(());
        }
    }
    Err(failed)
}

// 拉起下载好的安装器（NSIS 的 /S 是静默；它自己会停下正在运行的副本，见
// scripts/install-update.cjs 的「Stopping the running copy」），然后她退出。
fn run_installer(exe: &Path) -> Result<(), String> {
    if crate::detach::spawn(exe, &["/S"]) {
        Ok(())
    } else {
        Err(format!("could not start {}", exe.display()))
    }
}

// The person said Update: off on its own thread. 安装版也走这里：装的是发行包里的
// 安装器（见 install_installer），不是打开浏览器。
pub fn update_now(sh: &Arc<Shared>) {
    let release = {
        let mut s = STATE.lock().unwrap();
        if s.stage.is_some() {
            return;
        }
        let Some(r) = s.latest.clone().filter(|r| newer(&r.version, current())) else { return };
        s.stage = Some("download");
        s.from = None;
        s.got = 0;
        s.total = 0;
        s.error = None;
        r
    };
    sh.push_settings();
    let sh = sh.clone();
    std::thread::spawn(move || {
        if let Err(e) = install(&sh, &release) {
            sh.log(&format!("update: v{} not installed: {e}", release.version));
            let mut s = STATE.lock().unwrap();
            s.stage = None;
            s.from = None;
            s.error = Some(e);
            drop(s);
            sh.push_settings();
            // One that did not last, said on the island.
            tell(&sh);
        }
    });
}

fn stage(sh: &Shared, stage: &'static str) {
    STATE.lock().unwrap().stage = Some(stage);
    sh.push_settings();
}

fn install(sh: &Arc<Shared>, r: &Release) -> Result<(), Value> {
    // 安装版走「下载安装器并拉起」；便携版走「换掉自己的 exe」。
    if current_mode().installed {
        return install_installer(sh, r);
    }
    if !may_update() {
        return Err(error("source", ""));
    }
    if r.conflict {
        return Err(error("mismatch", r.version.clone()));
    }
    let digest = r.digest.as_deref().ok_or_else(|| error("noDigest", ""))?;
    let exes: Vec<(Source, &str, u64)> = r.ordered(gitee_first(sh)).into_iter().filter_map(|f| Some((f.source, f.exe.as_deref()?, f.size))).collect();
    let part = sh.dir.join(format!("update-v{}.exe.part", r.version));
    // From the place preferred; the other should that fail.
    let mut failed = error("noExe", "");
    let mut done = false;
    for (source, url, size) in exes {
        if !allowed(url) {
            failed = error("elsewhere", url);
            continue;
        }
        {
            let mut s = STATE.lock().unwrap();
            s.stage = Some("download");
            s.from = Some(source);
            s.got = 0;
            s.total = size;
        }
        sh.push_settings();
        let (got, how) = match download(sh, url, &part) {
            Ok(got) => got,
            Err(e) => {
                let _ = std::fs::remove_file(&part);
                sh.log(&format!("update: v{} not downloaded from {}: {e}", r.version, source.name()));
                failed = error("download", format!("{}: {e}", source.name()));
                continue;
            }
        };
        stage(sh, "verify");
        if got == digest {
            sh.log(&format!("update: v{} downloaded from {} ({how}), its SHA-256 as given", r.version, source.name()));
            done = true;
            break;
        }
        let _ = std::fs::remove_file(&part);
        sh.log(&format!("update: v{} from {}: SHA-256 {got}, not {digest}", r.version, source.name()));
        failed = error("digest", format!("{}: {got} ≠ {digest}", source.name()));
    }
    if !done {
        return Err(failed);
    }
    // A prompt waiting for an answer: answered first (the new one would
    // not have it).
    if !sh.asks.lock().unwrap().is_empty() {
        stage(sh, "waiting");
        sh.log("update: a prompt is open: waiting for its answer");
        while !sh.asks.lock().unwrap().is_empty() {
            std::thread::sleep(Duration::from_secs(1));
        }
    }
    stage(sh, "restart");
    let exe = replace(&part).map_err(|e| {
        let _ = std::fs::remove_file(&part);
        error("replace", e)
    })?;
    sh.log(&format!("update: v{} in place of v{} ({}), hers kept as {}", r.version, current(), exe.display(), old_path(&exe).display()));
    write_note(sh, &r.version, &exe);
    let by = format!("her update from v{} (pid {})", current(), std::process::id());
    match hand_over(sh, &exe, &r.version, &by) {
        Ok(()) => {
            sh.log(&format!("update: v{} up; she quits for it", r.version));
            sh.quit(crate::data::Quit::Wakeable, "an update's handover");
            Ok(())
        }
        Err(Failed::Start(e)) => {
            put_back(sh, &exe);
            Err(error("start", e))
        }
        Err(Failed::Lost(why)) => {
            sh.log(&format!("update: v{} did not last ({why}); she stays", r.version));
            put_back(sh, &exe);
            *BLOCKED.lock().unwrap() = Some(r.version.clone());
            Err(error("blocked", r.version.clone()))
        }
    }
}

// Why a new one did not take over from her: it would not start at all, or
// it did and did not last.
enum Failed {
    Start(String),
    Lost(String),
}

// The new one started (plainly: detach::start) and waited for, as she must
// not be gone before it is up: its word (POST /after-update, said as it
// starts: wait_after_update), then SETTLE more with it still running and
// its exe still there. Should it end first, its exe go, or no word come in
// WORD_WAIT, it is ended if it still runs: an antivirus ends what it takes
// for a dropper, and takes its exe away, in its first moments. `by`: what
// started it, for its log (starts.rs).
fn hand_over(sh: &Shared, exe: &Path, version: &str, by: &str) -> Result<(), Failed> {
    *HEARD.lock().unwrap() = None;
    let me = std::process::id().to_string();
    // Refused by Windows for a virus (ERROR_VIRUS_INFECTED, _DELETED: an
    // antivirus scanning it as it starts), or gone already: blocked too.
    let mut new = crate::detach::start(exe, &[AFTER_UPDATE, &me, crate::starts::BY_FLAG, by]).map_err(|e| {
        let why = format!("{} would not start ({e})", exe.display());
        if matches!(e.raw_os_error(), Some(225 | 226)) || !exe.is_file() { Failed::Lost(why) } else { Failed::Start(why) }
    })?;
    sh.log(&format!("update: v{version} started, pid {}; waiting for its word", new.pid));
    let began = Instant::now();
    let mut heard_at: Option<Instant> = None;
    let why = loop {
        let ended = new.ended_within(100);
        let ms = began.elapsed().as_millis();
        if let Some(code) = ended {
            break format!("it ended {ms} ms in, {} its word, exit code 0x{code:08X}", if heard_at.is_some() { "after" } else { "before" });
        }
        if !exe.is_file() {
            break format!("{} gone {ms} ms in", exe.display());
        }
        match heard_at {
            None => {
                let heard = HEARD.lock().unwrap().clone().filter(|(pid, _)| *pid == new.pid);
                if let Some((_, said)) = heard {
                    let other = if same_version(&said, version) { String::new() } else { format!(" (not v{version})") };
                    sh.log(&format!("update: pid {} says v{said} is up{other}, {ms} ms in", new.pid));
                    heard_at = Some(Instant::now());
                } else if began.elapsed() >= WORD_WAIT {
                    break format!("no word from it in {} s", WORD_WAIT.as_secs());
                }
            }
            Some(at) if at.elapsed() >= SETTLE => {
                sh.log(&format!("update: pid {} still running {} s after its word, its exe in place", new.pid, SETTLE.as_secs()));
                return Ok(());
            }
            Some(_) => {}
        }
    };
    if new.ended_within(0).is_none() {
        new.end();
        let gone = new.ended_within(5000).is_some();
        sh.log(&format!("update: pid {} ended by her{}", new.pid, if gone { "" } else { ", and still not gone 5 s later" }));
    }
    Err(Failed::Lost(why))
}

// A new one's word that it is up (server.rs: POST /after-update).
pub fn heard(pid: u32, version: &str) {
    *HEARD.lock().unwrap() = Some((pid, version.to_string()));
}

// Hers back where it was, and what is left of the new one gone; should the
// rename fail, she runs on as <name>.old.exe, and says so (data::exe).
fn put_back(sh: &Shared, exe: &Path) {
    let old = old_path(exe);
    match std::fs::remove_file(exe) {
        Ok(()) => sh.log(&format!("update: {} removed", exe.display())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => sh.log(&format!("update: {} already gone", exe.display())),
        Err(e) => sh.log(&format!("update: could not remove {}: {e}", exe.display())),
    }
    match std::fs::rename(&old, exe) {
        Ok(()) => sh.log(&format!("update: {} back as {}", old.display(), exe.display())),
        Err(e) => {
            sh.log(&format!("update: could not rename {} back to {}: {e}; she runs on as it", old.display(), exe.display()));
            crate::data::exe_moved(old);
        }
    }
    let _ = std::fs::remove_file(sh.dir.join(NOTE));
}

// The note of an update under way: what it put where.
fn write_note(sh: &Shared, version: &str, exe: &Path) {
    let note = json!({ "from": current(), "to": version, "exe": exe, "old": old_path(exe), "at": crate::now_ms() });
    if let Err(e) = std::fs::write(sh.dir.join(NOTE), note.to_string()) {
        sh.log(&format!("update: could not write {NOTE}: {e}"));
    }
}

// The same file, as Windows names them: either slash, any case.
fn same_path(a: &Path, b: &Path) -> bool {
    let norm = |p: &Path| {
        let p = p.to_string_lossy().replace('/', "\\");
        if cfg!(windows) { p.to_lowercase() } else { p }
    };
    norm(a) == norm(b)
}

// Whether, started as `me` with her data folder's note saying `note`, she is
// an update's old exe whose new one is gone: the note naming her as the
// old one, its new one not there. The path to take back then.
fn left_behind(me: &Path, note: &Value, exists: impl Fn(&Path) -> bool) -> Option<PathBuf> {
    let is_old = me.file_name().map(|n| n.to_string_lossy().to_lowercase()).is_some_and(|n| n.ends_with(".old.exe"));
    let exe = PathBuf::from(note["exe"].as_str()?);
    let old = PathBuf::from(note["old"].as_str()?);
    (is_old && same_path(&old, me) && same_path(&old_path(&exe), me) && !exists(&exe)).then_some(exe)
}

// Started as the exe an update left (<name>.old.exe), its new one gone from
// where it was put (taken by an antivirus after it had started, most
// likely), as the update's note says: her name taken back, so that her Run
// value, the sessions' hooks and her next update find her; said in the log
// and on the island once she is up (start), and in the settings. Before
// anything names her exe (main.rs, once she holds her port).
pub fn take_name_back(dir: &Path) {
    let Ok(me) = std::env::current_exe() else { return };
    let Some(note) = std::fs::read_to_string(dir.join(NOTE)).ok().and_then(|t| serde_json::from_str::<Value>(&t).ok()) else { return };
    let Some(exe) = left_behind(&me, &note, |p| p.is_file()) else { return };
    let to = note["to"].as_str().unwrap_or("?").to_string();
    let line = match std::fs::rename(&me, &exe) {
        Ok(()) => {
            crate::data::exe_moved(exe.clone());
            format!("update: v{to} gone from {} after it started (an antivirus?): {} renamed back to it", exe.display(), me.display())
        }
        Err(e) => format!("update: v{to} gone from {} after it started (an antivirus?); could not rename {} back to it: {e}", exe.display(), me.display()),
    };
    *BLOCKED.lock().unwrap() = Some(to.clone());
    STATE.lock().unwrap().error = Some(error("blocked", to));
    let _ = std::fs::remove_file(dir.join(NOTE));
    *NAMED_BACK.lock().unwrap() = Some(line);
}

// Downloaded to `to`: its SHA-256 as it comes (lowercase hex), and the way
// it came ("<way>, <hosts>"); the progress for the settings. Each way in
// turn (proxy::web_ways); the only one tried again once, three seconds on,
// should it fail before the download began (a connection that did not
// come, as one straight through a VPN's TUN may not).
fn download(sh: &Shared, url: &str, to: &Path) -> Result<(String, String), String> {
    let ways = crate::proxy::web_ways(url);
    let tries = if ways.len() == 1 { 2 } else { ways.len() };
    let mut failed: Vec<String> = Vec::new();
    for i in 0..tries {
        let way = &ways[i.min(ways.len() - 1)];
        if i > 0 && ways.len() == 1 {
            std::thread::sleep(Duration::from_secs(3));
        }
        STATE.lock().unwrap().got = 0;
        match download_way(sh, way, url, to) {
            Ok(done) => {
                if !failed.is_empty() {
                    sh.log(&format!("update: downloaded {}, after: {}", done.1, failed.join("; ")));
                }
                return Ok(done);
            }
            Err((e, began)) => {
                let _ = std::fs::remove_file(to);
                failed.push(format!("{}, {e}", way.name()));
                if began && ways.len() == 1 {
                    break;
                }
            }
        }
    }
    Err(failed.join("; "))
}

// One way; Err: what went wrong, and whether the download had begun (an answer came).
fn download_way(sh: &Shared, way: &crate::proxy::Way, url: &str, to: &Path) -> Result<(String, String), (String, bool)> {
    let mut a = match get_way(way, url, 600, None) {
        Ok(a) => a,
        Err(None) => return Err((format!("{}: not there (404)", crate::proxy::host_of(url).unwrap_or_default()), false)),
        Err(Some(e)) => return Err((e, false)),
    };
    let total = a.res.body().content_length().unwrap_or(0);
    if total > 0 {
        STATE.lock().unwrap().total = total;
    }
    let mb = |n: u64| format!("{:.1}", n as f64 / 1_048_576.0);
    let mut reader = a.res.body_mut().with_config().limit(512 * 1024 * 1024).reader();
    let mut file = std::fs::File::create(to).map_err(|e| (format!("{}: {e}", to.display()), true))?;
    let mut hash = Sha256::new();
    let mut buf = vec![0u8; 64 * 1024];
    let mut got = 0u64;
    loop {
        let n = reader.read(&mut buf).map_err(|e| (format!("{}: cut off at {} of {} MB: {e}", a.path, mb(got), mb(total)), true))?;
        if n == 0 {
            break;
        }
        file.write_all(&buf[..n]).map_err(|e| (format!("{}: {e}", to.display()), true))?;
        hash.update(&buf[..n]);
        got += n as u64;
        STATE.lock().unwrap().got = got;
        sh.push_settings();
    }
    file.flush().map_err(|e| (format!("{}: {e}", to.display()), true))?;
    Ok((hash.finalize().iter().map(|b| format!("{b:02x}")).collect(), format!("{}, {}", a.way, a.path)))
}

// Beside hers, its name kept (she may have been renamed): <name>.old.exe.
fn old_path(exe: &Path) -> PathBuf {
    let stem = exe.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "wakuwaku".into());
    exe.with_file_name(format!("{stem}.old.exe"))
}

// The new exe where hers is (a running exe can be renamed, not removed),
// hers kept as <name>.old.exe. Its path.
fn replace(part: &Path) -> Result<PathBuf, String> {
    let exe = crate::data::exe().map_err(|e| e.to_string())?;
    let old = old_path(&exe);
    let _ = std::fs::remove_file(&old);
    std::fs::rename(&exe, &old).map_err(|e| format!("{} → {}: {e}", exe.display(), old.display()))?;
    let put = std::fs::rename(part, &exe).or_else(|_| std::fs::copy(part, &exe).map(|_| ()));
    if let Err(e) = put {
        let _ = std::fs::rename(&old, &exe);
        return Err(format!("{}: {e}", exe.display()));
    }
    let _ = std::fs::remove_file(part);
    Ok(exe)
}

// Her own exe started again, to wait for her to end (crash.rs: WebView2's
// browser process gone, her pages with it); as an update's, she goes only
// once it is up (hand_over), and stays should it not come up.
pub fn start_again(sh: &Shared) -> Result<(), String> {
    let exe = crate::data::exe().map_err(|e| e.to_string())?;
    let by = format!("her restart, WebView2's browser gone (pid {})", std::process::id());
    hand_over(sh, &exe, current(), &by).map_err(|failed| match failed {
        Failed::Start(why) | Failed::Lost(why) => why,
    })
}

// 软件内更新留下的下载文件（#27「清理后仍残留」）：`installer-v<ver>-<which>.exe`
// 是更新时下下来、交给 NSIS 跑的那份安装器；跑完就再也没人用它，一份几十 MB，
// 一次更新攒一份。`update-v<ver>.exe.part` 是下载被打断的半截文件。
// 判据只看名字里的版本：**比当前版本新**的是还没装上的那份，留着；其余一律删。
// 删不掉（安装器/杀软还占着句柄）就跳过，下一次启动接着收——不报错、不阻断启动。
fn sweep_update_downloads(dir: &Path) -> Vec<String> {
    let mut removed = Vec::new();
    let Ok(entries) = std::fs::read_dir(dir) else { return removed };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        let installed = name.strip_prefix("installer-v").and_then(|rest| rest.split('-').next());
        let stale = match installed {
            Some(version) => !newer(version, current()),
            None => name.starts_with("update-v") && name.ends_with(".exe.part"),
        };
        if !stale {
            continue;
        }
        if std::fs::remove_file(entry.path()).is_ok() {
            removed.push(name);
        }
    }
    removed.sort();
    removed
}

// After a minute's run, the exe an update left: gone (tried a few times:
// the old one's taskbar guard may still have it open a moment), and the
// update's note with it. Not while an update of hers is under way (her own
// exe is <name>.old.exe then).
fn remove_old(sh: &Shared) {
    if STATE.lock().unwrap().stage.is_some() {
        return;
    }
    let _ = std::fs::remove_file(sh.dir.join(NOTE));
    let Ok(exe) = crate::data::exe() else { return };
    let old = old_path(&exe);
    for _ in 0..6 {
        if !old.exists() {
            return;
        }
        if std::fs::remove_file(&old).is_ok() {
            sh.log(&format!("update: {} removed, the new one having run a minute", old.display()));
            return;
        }
        std::thread::sleep(Duration::from_secs(10));
    }
    sh.log(&format!("update: could not remove {}", old.display()));
}

// Started by an update or a restart (--after-update <pid>): said to that
// one on her port, where it still listens, that she is up (it goes only
// then: hand_over), and it waited for, at most a minute, before she takes
// the port and the taskbar. One before 2026-10-15 had quit already, and
// knows no such word (404), or is too busy going to answer.
pub fn wait_after_update(args: &[String], port: u16) {
    let Some(pid) = args.iter().position(|a| a == AFTER_UPDATE).and_then(|i| args.get(i + 1)).and_then(|p| p.parse::<u32>().ok()) else { return };
    #[cfg(windows)]
    {
        #[link(name = "kernel32")]
        extern "system" {
            fn OpenProcess(access: u32, inherit: i32, pid: u32) -> isize;
            fn WaitForSingleObject(handle: isize, ms: u32) -> u32;
            fn CloseHandle(handle: isize) -> i32;
        }
        const SYNCHRONIZE: u32 = 0x0010_0000;
        // SAFETY: a handle we close; waits on it alone.
        unsafe {
            let process = OpenProcess(SYNCHRONIZE, 0, pid);
            let told = say_up(port, || process == 0 || WaitForSingleObject(process, 0) == 0);
            *AFTER.lock().unwrap() = Some((pid, told));
            if process != 0 {
                WaitForSingleObject(process, 60_000);
                CloseHandle(process);
            }
        }
    }
    #[cfg(not(windows))]
    {
        let told = say_up(port, || false);
        *AFTER.lock().unwrap() = Some((pid, told));
    }
}

// POST /after-update { pid, version } on her port, never through a proxy;
// tried again a moment later should it not get through, while that one is
// still there (`gone`).
fn say_up(port: u16, gone: impl Fn() -> bool) -> Result<(), String> {
    let agent = ureq::Agent::config_builder().timeout_global(Some(Duration::from_secs(2))).proxy(None).build().new_agent();
    let word = json!({ "pid": std::process::id(), "version": current() });
    let mut why = String::new();
    for _ in 0..3 {
        match agent.post(&format!("http://127.0.0.1:{port}/after-update")).send_json(&word) {
            Ok(_) => return Ok(()),
            Err(ureq::Error::StatusCode(code)) => return Err(format!("it answered {code}")),
            Err(e) => why = e.to_string(),
        }
        if gone() {
            return Err(format!("{why}; it had gone"));
        }
        std::thread::sleep(Duration::from_millis(300));
    }
    Err(why)
}

#[cfg(test)]
#[path="updates_installed_tests.rs"]
mod installed_tests;

#[cfg(test)]
mod tests {
    use serde_json::json;

    use std::io::{Read, Write};

    use super::{allowed, digest_of, error, get_way, github_release, installer_names, merge, newer, newest_tag, notes_entries, notes_load, notes_plain_error, notes_save, notes_sources, notes_state, redirected, sums_of, Found, NotesCache, NotesConfig, NotesSource, Source, NOTES_FILE};

    #[cfg(feature = "installed")]
    #[test]
    fn an_installed_build_cannot_install_upstream_updates() {
        assert!(!super::may_update());
    }

    // Her version said the same everywhere it is said: what updates go by
    // (Cargo.toml), the exe's own and the settings' (tauri.conf.json), and
    // the Claude Code plugin's; a test build once had only one of them changed.
    #[test]
    fn one_version_everywhere() {
        let ours = env!("CARGO_PKG_VERSION");
        for (file, text) in [("tauri.conf.json", include_str!("../tauri.conf.json")), ("plugin.json", include_str!("../../integrations/claude-code/.claude-plugin/plugin.json"))] {
            let v: serde_json::Value = serde_json::from_str(text).unwrap();
            assert_eq!(v["version"].as_str(), Some(ours), "{file} says another version than Cargo.toml");
        }
    }

    #[test]
    fn versions_compared() {
        assert!(newer("v0.2.1", "0.2.0"));
        assert!(newer("0.10.0", "0.9.0"));
        assert!(newer("1.0", "0.99.9"));
        assert!(!newer("0.2.0", "0.2.0"));
        assert!(!newer("v0.2.0", "0.2.0"));
        assert!(!newer("0.1.9", "0.2.0"));
        assert!(!newer("0.9.0", "0.10.0"));
        assert!(newer("0.2.0", "0.2.0-beta"));
        assert!(!newer("0.2.0-beta", "0.2.0"));
    }

    #[test]
    fn github_read() {
        let asset = |name: &str, digest: &str| json!({ "name": name, "size": 7, "digest": digest, "browser_download_url": format!("https://github.com/zhoupengjie/Wakuwaku/releases/download/x/{name}") });
        let good = json!({
            "tag_name": "v0.2.1", "draft": false, "prerelease": false,
            "html_url": "https://github.com/zhoupengjie/Wakuwaku/releases/tag/v0.2.1",
            "assets": [asset("notes.txt", ""), asset("wakuwaku-v0.2.1-windows-x64.exe", &format!("sha256:{}", "AB".repeat(32)))],
        });
        let r = github_release(&good).unwrap();
        assert_eq!(r.source, Source::GitHub);
        assert_eq!(r.version, "0.2.1");
        assert_eq!(r.digest.as_deref(), Some("ab".repeat(32).as_str()));
        assert!(r.exe.unwrap().ends_with("wakuwaku-v0.2.1-windows-x64.exe"));
        let mut draft = good.clone();
        draft["draft"] = json!(true);
        assert!(github_release(&draft).is_none());
        let mut pre = good.clone();
        pre["prerelease"] = json!(true);
        assert!(github_release(&pre).is_none());
        // No SHA-256 given, or not one: none to check by.
        let mut bare = good.clone();
        bare["assets"] = json!([asset("wakuwaku-v0.2.1-windows-x64.exe", "md5:xyz")]);
        assert_eq!(github_release(&bare).unwrap().digest, None);
        // The exe by another name: none to install.
        let mut other = good;
        other["assets"] = json!([asset("wakuwaku.exe", "")]);
        assert_eq!(github_release(&other).unwrap().exe, None);
    }

    // A repo's tags as git's info/refs gives them (as Gitee's does): the
    // highest version, by number; not a prerelease ("-"), a peeled line
    // (^{}) or a tag that is no version.
    #[test]
    fn tags_read() {
        let sha = "a".repeat(40);
        let pkt = |line: String| format!("{:04x}{line}", line.len() + 4);
        let mut refs = pkt("# service=git-upload-pack\n".into()) + "0000";
        refs += &pkt(format!("{sha} HEAD\0multi_ack thin-pack side-band symref=HEAD:refs/heads/main agent=git/2\n"));
        refs += &pkt(format!("{sha} refs/heads/main\n"));
        for tag in ["v0.2.1", "v0.2.1^{}", "v0.9.0", "v0.10.0", "v0.10.0^{}", "v0.11.0-beta", "nightly"] {
            refs += &pkt(format!("{sha} refs/tags/{tag}\n"));
        }
        refs += "0000";
        assert_eq!(newest_tag(&refs).as_deref(), Some("v0.10.0"));
        // The first line's capabilities after \0, when it is a tag.
        assert_eq!(newest_tag(&pkt(format!("{sha} refs/tags/v0.2.1\0multi_ack\n"))).as_deref(), Some("v0.2.1"));
        assert_eq!(newest_tag(&(pkt("# service=git-upload-pack\n".into()) + "0000" + &pkt(format!("{sha} refs/heads/main\n")) + "0000")), None);
        assert_eq!(newest_tag(""), None);
    }

    #[test]
    fn sums_read() {
        let hex = "AB".repeat(32);
        assert_eq!(sums_of(&format!("{hex}  wakuwaku-v0.2.2-windows-x64.exe\n")).as_deref(), Some("ab".repeat(32).as_str()));
        assert_eq!(sums_of(&format!("\u{feff}{hex} *wakuwaku.exe\r\n")).as_deref(), Some("ab".repeat(32).as_str()));
        assert_eq!(sums_of(&hex).as_deref(), Some("ab".repeat(32).as_str()));
        assert_eq!(sums_of("not a sum"), None);
        assert_eq!(sums_of(""), None);
    }

    #[test]
    fn release_notes_read() {
        let list = json!([
            { "tag_name": "v2.0.0", "name": "FISH Dock 2.0.0", "body": "首版", "published_at": "2026-10-29T10:00:00Z" },
            { "tag_name": "v1.9.0", "name": "", "body": "", "published_at": "2026-10-01T00:00:00Z", "draft": true },
            { "tag_name": "v1.8.0", "name": "旧", "body": "x", "published_at": "", "prerelease": true },
            { "tag_name": "v1.7.0", "name": "更旧", "body": "y", "published_at": "2026-09-01T00:00:00Z" }
        ]);
        let rows = notes_entries(&list, 5);
        // 草稿与预发布都不算。
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0]["version"], "2.0.0");
        assert_eq!(rows[0]["title"], "FISH Dock 2.0.0");
        assert_eq!(rows[0]["body"], "首版");
        assert_eq!(rows[0]["at"], "2026-10-29T10:00:00Z");
        assert_eq!(rows[1]["version"], "1.7.0");
        assert_eq!(notes_entries(&list, 1).len(), 1);
        // 不是数组（读回来的东西不对）是空，不是崩。
        assert!(notes_entries(&json!({}), 5).is_empty());
        // Gitee 形状（实测：tag_name/name/body 兼容，时间字段是 created_at）也认。
        let gitee_shaped = json!([{ "tag_name": "v2.0.0", "name": "FISH Dock 2.0.0", "body": "首版", "created_at": "2026-10-29T10:00:00+08:00", "prerelease": false }]);
        let rows = notes_entries(&gitee_shaped, 5);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0]["at"], "2026-10-29T10:00:00+08:00", "Gitee 的 created_at 当时间");
    }

    #[test]
    fn release_notes_states() {
        // 四态与前端 settings.js notesKind 一一对上（缓存优先，失败也要说清）。
        assert_eq!(notes_state(&NotesCache::default()), "offline"); // 一次都没读到过
        // 读通了、列表是空的：refresh_notes 把这种情况标成 offline=true。
        let read_nothing = NotesCache { offline: true, ..Default::default() };
        assert_eq!(notes_state(&read_nothing), "empty");
        let failed = NotesCache { error: Some(error("notes", "boom")), ..Default::default() };
        assert_eq!(notes_state(&failed), "error");
        // 有内容就是 ok —— 哪怕这次读失败，上次那份照样给出去（失败的话由 error
        // 字段带着，前端在那份列表下面补一行「最新一次没读到」+ 重试）。
        let stale = NotesCache {
            entries: vec![json!({ "version": "1.9.0" })],
            error: Some(error("notes", "boom")),
            ..Default::default()
        };
        assert_eq!(notes_state(&stale), "ok");
        assert_eq!(stale.error.as_ref().unwrap()["detail"], "boom");
    }

    // ── 公告多源（t10）────────────────────────────────────────────────
    // notes_sources 的形状只在这里直测（fetch_notes_once 的网络层用同一形状）。
    // 配置全部注入（NotesConfig），不碰进程环境变量——并发全量跑不与任何测试串扰。
    fn source_labels(sources: &[NotesSource]) -> Vec<(&'static str, bool, String)> {
        sources.iter().map(|s| match s {
            NotesSource::Read { label, url } => (*label, true, url.clone()),
            NotesSource::Skipped { label, why } => (*label, false, why.to_string()),
        }).collect()
    }
    fn notes_config(gitee: &str, github: &str, preferring: &str) -> NotesConfig {
        NotesConfig { github: github.to_string(), gitee: gitee.to_string(), preferring: preferring.to_string() }
    }
    const DEFAULT_GITHUB: &str = super::GITHUB_RELEASES;

    // 多源顺序：auto = GitHub 先、Gitee 兜底（未配置时明确跳过）；gitee = Gitee 先；
    // github = 只 GitHub。配置过的 Gitee 换成它自己的 API 地址（owner/repo 归一）。
    #[test]
    fn notes_sources_order_follows_the_setting() {
        let unconfigured = notes_config("", DEFAULT_GITHUB, "auto");
        let auto = source_labels(&notes_sources(&unconfigured));
        assert_eq!(auto.len(), 2, "auto 是两条源：GitHub + Gitee");
        assert_eq!(auto[0].0, "GitHub");
        assert!(auto[0].1, "GitHub 永远可读");
        assert_eq!(auto[1].0, "Gitee");
        assert!(!auto[1].1, "Gitee 未配置时是跳过，不是可读");
        assert!(auto[1].2.contains("未配置"), "跳过要写明原因：{}", auto[1].2);
        let github_only = source_labels(&notes_sources(&notes_config("", DEFAULT_GITHUB, "github")));
        assert_eq!(github_only.len(), 1);
        assert_eq!(github_only[0].0, "GitHub");
        // 配置了 Gitee：gitee 顺序下它在前，且地址指向 Gitee 的 API（{repo} 已替换、
        // 末尾斜杠已去掉、https://gitee.com/ 前缀已归一成 owner/repo）。
        let gitee_first = source_labels(&notes_sources(&notes_config("https://gitee.com/fish-under-sea/FISH-Dock/", DEFAULT_GITHUB, "gitee")));
        assert_eq!(gitee_first[0], ("Gitee", true, "https://gitee.com/api/v5/repos/fish-under-sea/FISH-Dock/releases?per_page=5&direction=desc".to_string()));
        assert_eq!(gitee_first[1].0, "GitHub", "Gitee 失败还有 GitHub 兜底");
        // auto 顺序下配置过的 Gitee 排第二，且可读。
        let auto_configured = source_labels(&notes_sources(&notes_config("fish-under-sea/FISH-Dock", DEFAULT_GITHUB, "auto")));
        assert_eq!(auto_configured[1], ("Gitee", true, gitee_first[0].2.clone()), "裸 owner/repo 也认");
        // GitHub 地址可经配置覆盖；乱形状的 Gitee 配置明确跳过且写明原因。
        let overridden = source_labels(&notes_sources(&notes_config("https://gitee.com/a/b/c", "https://api.github.com/repos/other/repo/releases?per_page=5", "github")));
        assert_eq!(overridden[0].2, "https://api.github.com/repos/other/repo/releases?per_page=5");
        let bad_gitee = source_labels(&notes_sources(&notes_config("not gitee", DEFAULT_GITHUB, "auto")));
        assert_eq!(bad_gitee[1].0, "Gitee");
        assert!(!bad_gitee[1].1);
        assert!(bad_gitee[1].2.contains("需要"), "配置错误写明原因：{}", bad_gitee[1].2);
    }

    // 缓存命中：notes_load/notes_save 带着 source 与 at 落盘，重开还是那句话。
    #[test]
    fn notes_cache_round_trip_keeps_the_source_and_time() {
        let dir = std::env::temp_dir().join(format!("wakuwaku-notes-cache-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let cache = NotesCache { entries: vec![json!({ "version": "2.0.0" })], at: 123_456, error: None, offline: false, source: Some("GitHub".into()) };
        notes_save(&dir, &cache);
        let back = notes_load(&dir).unwrap();
        assert_eq!(back.at, 123_456);
        assert_eq!(back.source.as_deref(), Some("GitHub"));
        assert_eq!(back.entries.len(), 1);
        // 旧格式（没有 source 字段）照旧读得回：source 为 None，不报错。
        std::fs::write(dir.join(NOTES_FILE), r#"{"entries":[{"version":"1.0.0"}],"at":42}"#).unwrap();
        let old = notes_load(&dir).unwrap();
        assert_eq!(old.at, 42);
        assert_eq!(old.source, None);
        assert_eq!(old.entries.len(), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }

    // 全失败可重试：notes_plain_error 把异常字符串换成人话（超时/连不上/限流），
    // 未知原因原样保留——重试入口的文案永远是一句人话。
    #[test]
    fn notes_failures_are_said_in_plain_words() {
        assert!(notes_plain_error("through Windows' proxy: timed out").contains("连接超时"));
        assert!(notes_plain_error("proxy connect refused").contains("连不上"));
        assert!(notes_plain_error("HTTP 403 rate limit").contains("接口限流"));
        assert!(notes_plain_error("HTTP 404 missing").contains("接口不存在"));
        assert_eq!(notes_plain_error("something else"), "something else", "没认出的原样给出，不吞");
    }

    // 超时可配：公告读取的预算来自 proxy::update_timeout 的同一解析（不再写死秒数）。
    // 断言打到 timeout_ms_of（注入文本），不碰进程环境（红线 9）——env 变量层只做
    // 「读一次环境」这一件事，解析规则在这里一次测全。
    #[test]
    fn notes_timeout_comes_from_the_shared_entry_not_a_literal() {
        assert_eq!(crate::proxy::timeout_ms_of(None, 20_000).as_millis(), 20_000, "没设 → 默认 20 秒");
        assert_eq!(crate::proxy::timeout_ms_of(Some("5000"), 20_000).as_millis(), 5_000, "设 5000 → 5 秒");
        assert_eq!(crate::proxy::timeout_ms_of(Some("not-a-number"), 20_000).as_millis(), 20_000, "乱值 → 默认");
        assert_eq!(crate::proxy::timeout_ms_of(Some("999999999"), 20_000).as_millis(), 20_000, "超上限（>600s）→ 当没设");
        assert_eq!(crate::proxy::timeout_ms_of(Some("600000"), 20_000).as_millis(), 600_000, "恰好上限 → 生效");
        assert_eq!(crate::proxy::timeout_ms_of(Some("0"), 20_000).as_millis(), 20_000, "0 视为没设");
    }

    #[test]
    fn installer_packages_named_and_checked() {
        assert_eq!(installer_names("2.0.0"), ["FISH-Dock-Update-2.0.0.exe".to_string(), "FISH-Dock-Setup-2.0.0.exe".to_string()]);
        let hex = "cd".repeat(32);
        // 一份 SHA256SUMS：按名字挑那一行，别的行不算。
        let sums = format!("{}  other.exe\n{hex}  FISH-Dock-Setup-2.0.0.exe\n", "ab".repeat(32));
        assert_eq!(digest_of(&sums, "FISH-Dock-Setup-2.0.0.exe").as_deref(), Some(hex.as_str()));
        assert_eq!(digest_of(&sums, "FISH-Dock-Update-2.0.0.exe"), None);
        // 只有 hex 的 <name>.sha256。
        assert_eq!(digest_of(&hex, "whatever.exe").as_deref(), Some(hex.as_str()));
        assert_eq!(digest_of("not a sum", "x"), None);
    }

    fn found(source: Source, version: &str, digest: Option<&str>) -> Found {        Found { source, version: version.into(), page: format!("{}/{version}", source.name()), exe: Some(format!("{}/{version}.exe", source.name())), digest: digest.map(str::to_string), size: 0 }
    }

    #[test]
    fn places_merged() {
        let (a, b) = ("a".repeat(64), "b".repeat(64));
        // The same release in both: GitHub's SHA-256, both places to download from.
        let r = merge(vec![found(Source::Gitee, "0.2.2", Some(&a)), found(Source::GitHub, "0.2.2", Some(&a))]).unwrap();
        assert_eq!(r.version, "0.2.2");
        assert_eq!(r.from.iter().map(|f| f.source).collect::<Vec<_>>(), [Source::GitHub, Source::Gitee]);
        assert_eq!(r.digest.as_deref(), Some(a.as_str()));
        assert!(!r.conflict && r.can_install());
        // The preferred place first.
        assert_eq!(r.ordered(true)[0].source, Source::Gitee);
        assert_eq!(r.ordered(false)[0].source, Source::GitHub);
        assert_eq!(r.page(true), "Gitee/0.2.2");
        // Gitee without a .sha256 read: GitHub's still checks Gitee's exe.
        let r = merge(vec![found(Source::GitHub, "0.2.2", Some(&a)), found(Source::Gitee, "0.2.2", None)]).unwrap();
        assert_eq!(r.digest.as_deref(), Some(a.as_str()));
        // One newer: only it, by its own SHA-256.
        let r = merge(vec![found(Source::GitHub, "0.2.1", Some(&a)), found(Source::Gitee, "0.2.2", Some(&b))]).unwrap();
        assert_eq!(r.version, "0.2.2");
        assert_eq!(r.from.len(), 1);
        assert_eq!(r.digest.as_deref(), Some(b.as_str()));
        // Two that differ: none trusted.
        let r = merge(vec![found(Source::GitHub, "0.2.2", Some(&a)), found(Source::Gitee, "v0.2.2", Some(&b))]).unwrap();
        assert!(r.conflict);
        assert_eq!(r.digest, None);
        assert!(!r.can_install());
        // None given: nothing to install.
        assert!(!merge(vec![found(Source::Gitee, "0.2.2", None)]).unwrap().can_install());
        assert_eq!(merge(vec![]), None);
    }

    // Both places as they answer now (network; `cargo test live -- --ignored`).
    #[test]
    #[ignore]
    fn live_places() {
        let github = super::fetch(Source::GitHub, super::GITHUB_LATEST).unwrap();
        let gitee = super::fetch(Source::Gitee, super::GITEE_REPO).unwrap();
        println!("GitHub: {github:?}\nGitee: {gitee:?}");
        assert!(github.as_ref().is_some_and(|f| f.digest.is_some() && f.exe.is_some()));
        assert!(gitee.as_ref().is_some_and(|f| f.digest.is_some() && f.exe.is_some()));
        let r = merge(github.into_iter().chain(gitee).collect()).unwrap();
        println!("{r:?}");
        assert!(!r.conflict);
        // Another repo on Gitee: its newest tag read, no .sha256 of hers there (404), nothing.
        assert_eq!(super::fetch(Source::Gitee, "https://gitee.com/cxasm/notepad--").unwrap(), None);
    }

    // Against test/fake-release.js (started with --version 0.2.2), the test
    // URLs set: `cargo test fake_places -- --ignored --test-threads 1`.
    #[test]
    #[ignore]
    fn fake_places() {
        let set = |m: &str| {
            ureq::get(&format!("http://127.0.0.1:47390/mode?{m}")).call().unwrap();
        };
        let look = || merge(super::endpoints().into_iter().filter_map(|(s, u)| super::fetch(s, &u).unwrap()).collect()).unwrap();
        set("githubOld=0&giteeOld=0&noDigest=0&noSums=0&badSums=0&giteeEmpty=0");
        let r = look();
        assert_eq!((r.version.as_str(), r.from.len(), r.conflict), ("0.2.2", 2, false));
        assert_eq!(r.from[1].digest, r.digest, "Gitee's .sha256, through its redirect, agrees with GitHub");
        // Gitee's exe through its redirect, each hop said, as given.
        let mut a = super::get_way(&crate::proxy::Way::Straight, r.from[1].exe.as_deref().unwrap(), 20, None).ok().unwrap();
        assert_eq!(a.path, "127.0.0.1 → 127.0.0.1");
        let mut exe = Vec::new();
        a.res.body_mut().with_config().limit(1 << 30).reader().read_to_end(&mut exe).unwrap();
        use sha2::Digest;
        assert_eq!(Some(sha2::Sha256::digest(&exe).iter().map(|b| format!("{b:02x}")).collect::<String>()), r.digest);
        set("badSums=1");
        assert!(look().conflict);
        set("badSums=0&noDigest=1");
        assert!(look().digest.is_some(), "Gitee's .sha256 when GitHub gives none");
        // No .sha256 on Gitee: its release not there (yet); GitHub's alone, without a digest.
        set("noSums=1");
        let r = look();
        assert_eq!((r.from.len(), r.can_install()), (1, false));
        set("noDigest=0&noSums=0&githubOld=1");
        let r = look();
        assert_eq!((r.version.as_str(), r.from.len(), r.from[0].source), ("0.2.2", 1, Source::Gitee));
        // Gitee without tags, or its newest tag without a release: GitHub's alone.
        set("githubOld=0&giteeEmpty=1");
        assert_eq!(look().from.len(), 1);
        set("giteeEmpty=0&giteeOld=1");
        assert_eq!(look().from.len(), 1);
        set("giteeOld=0");
    }

    // The exe an update left takes its name back only when the update's
    // note names it as the old one and its new one is not there.
    #[test]
    fn name_taken_back_only_when_left_behind() {
        use std::path::{Path, PathBuf};
        let note = json!({ "from": "0.3.0", "to": "0.3.1", "exe": r"D:\Apps\Waku\wakuwaku.exe", "old": r"D:\Apps\Waku\wakuwaku.old.exe" });
        let gone = |_: &Path| false;
        let there = |_: &Path| true;
        let me = Path::new(r"D:\Apps\Waku\wakuwaku.old.exe");
        assert_eq!(super::left_behind(me, &note, gone), Some(PathBuf::from(r"D:\Apps\Waku\wakuwaku.exe")));
        if cfg!(windows) {
            assert!(super::left_behind(Path::new("d:/apps/waku/WAKUWAKU.OLD.EXE"), &note, gone).is_some(), "as Windows names it");
        }
        // The new one there after all; or another exe; or another update's note.
        assert_eq!(super::left_behind(me, &note, there), None);
        assert_eq!(super::left_behind(Path::new(r"D:\Apps\Waku\wakuwaku.exe"), &note, gone), None);
        assert_eq!(super::left_behind(Path::new(r"E:\Other\wakuwaku.old.exe"), &note, gone), None);
        let mut other = note.clone();
        other["exe"] = json!(r"D:\Apps\Waku\pet.exe");
        assert_eq!(super::left_behind(me, &other, gone), None);
        assert_eq!(super::left_behind(me, &json!({}), gone), None);
    }

    #[test]
    fn only_her_repos() {
        assert!(allowed("https://github.com/Fish-under-sea/FISH-Dock/releases/download/v0.3.4/FISH-Dock-Setup-0.3.4.exe"));
        assert!(allowed("https://github.com/fish-under-sea/fish-dock/releases/tag/v0.3.4"));
        assert!(!allowed("https://github.com/zhoupengjie/Wakuwaku/releases/tag/v0.3.4"));
        assert!(!allowed("https://gitee.com/zhoupengjie/Wakuwaku/releases/tag/v0.3.4"));
        assert!(!allowed("https://github.com/someone/FISH-Dock/releases/download/x.exe"));
        assert!(!allowed("https://github.com.evil.example/Fish-under-sea/FISH-Dock/"));
        assert!(!allowed("http://github.com/Fish-under-sea/FISH-Dock/releases/download/x.exe"));
        assert!(!allowed("https://github.com/Fish-under-sea/FISH-Dock"));
    }

    #[test]
    fn redirects_made_whole_https_only() {
        let from = "https://github.com/zhoupengjie/Wakuwaku/releases/download/v0.3.3/x.exe";
        let assets = "https://release-assets.githubusercontent.com/github-production-release-asset/1?sig=x";
        assert_eq!(redirected(from, assets).as_deref(), Some(assets));
        assert_eq!(redirected(from, "/a/b").as_deref(), Some("https://github.com/a/b"));
        assert_eq!(redirected(from, "//objects.example.com/c").as_deref(), Some("https://objects.example.com/c"));
        assert_eq!(redirected(from, "http://plain.example.com/x"), None);
        assert_eq!(redirected(from, "relative/x"), None);
    }

    // A local server that answers each connection with the next of `answers`
    // (a whole HTTP response), then closes; its port.
    fn serve(answers: Vec<String>) -> u16 {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        std::thread::spawn(move || {
            for answer in answers {
                let Ok((mut s, _)) = listener.accept() else { return };
                let mut buf = [0u8; 4096];
                let _ = s.read(&mut buf);
                let _ = s.write_all(answer.replace("{port}", &port.to_string()).as_bytes());
            }
        });
        port
    }

    #[test]
    fn each_hop_said_when_it_goes_wrong() {
        let straight = crate::proxy::Way::Straight;
        // Answered: the way and the hosts it went by.
        let port = serve(vec!["HTTP/1.1 200 OK\r\ncontent-length: 2\r\nconnection: close\r\n\r\nhi".into()]);
        let mut a = get_way(&straight, &format!("http://127.0.0.1:{port}/x"), 5, None).ok().unwrap();
        assert_eq!(a.path, "127.0.0.1");
        assert_eq!(a.res.body_mut().read_to_string().unwrap(), "hi");
        // Not there: none, not an error.
        let port = serve(vec!["HTTP/1.1 404 Not Found\r\ncontent-length: 0\r\nconnection: close\r\n\r\n".into()]);
        assert!(matches!(get_way(&straight, &format!("http://127.0.0.1:{port}/x"), 5, None), Err(None)));
        // Redirected somewhere it may not go (plain http, outside a test): said with its host.
        let port = serve(vec!["HTTP/1.1 302 Found\r\nlocation: http://localhost:1/y\r\ncontent-length: 0\r\nconnection: close\r\n\r\n".into()]);
        let e = get_way(&straight, &format!("http://127.0.0.1:{port}/x"), 5, None).err().flatten().unwrap();
        assert!(e.starts_with("127.0.0.1: a 302 to nowhere"), "{e}");
        // Nothing there to connect to: the host, and ureq's words.
        let gone = std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
        let e = get_way(&straight, &format!("http://127.0.0.1:{gone}/x"), 5, None).err().flatten().unwrap();
        assert!(e.starts_with("127.0.0.1: "), "{e}");
    }

    // #27：「软件内更新」下下来的安装器跑完就没人要了，以前一直留在数据目录里，
    // 一次更新攒一份几十 MB。这里钉住判据：名字里的版本不高于当前版本的一律删，
    // 还没装上的新版安装器、用户自己的文件一个都不动。
    #[test]
    fn used_update_downloads_are_swept_and_a_pending_one_is_kept() {
        let dir = std::env::temp_dir().join(format!("fish-update-sweep-{}-{}", std::process::id(), crate::now_ms()));
        std::fs::create_dir_all(&dir).unwrap();
        let here = super::current();
        let write = |name: &str| std::fs::write(dir.join(name), b"fixture").unwrap();
        write(&format!("installer-v{here}-setup.exe"));
        write(&format!("installer-v{here}-update.exe"));
        write("installer-v0.0.1-setup.exe");
        write("installer-v99.0.0-setup.exe");
        write("update-v0.9.0.exe.part");
        write("notes.txt");
        let mut want = vec![format!("installer-v{here}-setup.exe"), format!("installer-v{here}-update.exe"), "installer-v0.0.1-setup.exe".into(), "update-v0.9.0.exe.part".into()];
        want.sort();
        assert_eq!(super::sweep_update_downloads(&dir), want);
        // 留着的东西：还没装上的新版安装器 + 不是我们下的文件。
        assert!(dir.join("installer-v99.0.0-setup.exe").exists(), "比当前版本新的安装器是还没装上的那份，不许删");
        assert!(dir.join("notes.txt").exists(), "不是更新下下来的文件，一个都不碰");
        assert!(!dir.join(format!("installer-v{here}-setup.exe")).exists(), "用过的安装器必须删掉");
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
