//! 飞书自定义机器人（Incoming Webhook）的单向通知。
//!
//! 她的三种动静（main.rs alert: 需要确认、完成、出错）可以同时推到飞书群。
//! 飞书的自定义机器人只能发、不能收消息（官方文档：单向推送，不能响应用户），
//! 所以这里只有发。
//!
//! 接口：POST https://open.feishu.cn/open-apis/bot/v2/hook/<token>，
//! body {"msg_type":"text","content":{"text":"..."}}。飞书的业务错误也回
//! HTTP 200，成功与否只看 body 的 code（0 才对）——不能拿 HTTP 状态当成功。
//!
//! webhook 地址等同群的发消息钥匙：只用当前用户 DPAPI 加密保存在她的
//! 数据目录（feishu.dpapi），页面只见到末尾四位，日志里只有结果码。

use std::collections::VecDeque;
use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::Serialize;
use serde_json::{json, Value};
use tauri::AppHandle;

use crate::proxy::web_ways;

/// 加密后的 webhook：%LOCALAPPDATA%\DSH-FISH\data\feishu.dpapi。
const FILE: &str = "feishu.dpapi";
/// 一次请求最多等这么久（飞书一般立刻答）。
const TIMEOUT: Duration = Duration::from_secs(15);
/// 只认飞书的这两个域名和这一条路径：别的地址不存也不发（也挡 SSRF）。
const HOSTS: [&str; 2] = ["open.feishu.cn", "open.larksuite.com"];
const PATH: &str = "open-apis/bot/v2/hook/";
/// 一分钟最多发这么多条：飞书的硬顶是 100/分钟、5/秒，这里留足余量，
/// 免得一次批量任务把群刷屏（也免得吃 11232）。
const MOST_A_MINUTE: usize = 20;
/// 同一句话（同一种动静、同一个项目）这么多秒内只发一次。
const SAME_WITHIN: Duration = Duration::from_secs(5);

/// 最近一次发送的结果，说给设置页听（成功与否、错误码、时间）。
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct Last {
    ok: bool,
    code: String,
    at: u64,
}

static LAST: Mutex<Option<Last>> = Mutex::new(None);
/// 最近发过的句子与时间：去重用。
static SENT: Mutex<VecDeque<(String, Instant)>> = Mutex::new(VecDeque::new());
/// 最近一分钟的发件时间：限流用。
static TIMES: Mutex<VecDeque<Instant>> = Mutex::new(VecDeque::new());

// --- 设置 ----------------------------------------------------------------------------

/// "feishu": { "on": bool, "waiting": bool, "done": bool, "error": bool }，别的
/// 一概不收（settings.rs is_ok；收下就会写进 config.json）。
pub fn is_ok(v: &Value) -> bool {
    v.as_object().is_some_and(|m| {
        !m.is_empty()
            && m.len() <= 4
            && m.iter().all(|(k, v)| matches!(k.as_str(), "on" | "waiting" | "done" | "error") && v.is_boolean())
    })
}

// --- 地址 ----------------------------------------------------------------------------

/// 地址里的 token：https、飞书的域名、机器人那条路径，缺一不可。
fn token_of(url: &str) -> Option<&str> {
    let rest = url.trim().strip_prefix("https://")?;
    let (host, path) = rest.split_once('/')?;
    if !HOSTS.contains(&host) {
        return None;
    }
    let token = path.strip_prefix(PATH)?;
    (!token.is_empty() && token.len() <= 200 && token.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.')))
        .then_some(token)
}

pub fn url_ok(url: &str) -> bool {
    token_of(url).is_some()
}

/// 页面上只给末尾四位。
fn tail_of(url: &str) -> String {
    let token = token_of(url).unwrap_or("");
    let cut = token.len().saturating_sub(4);
    format!("...{}", &token[cut..])
}

fn file(sh: &crate::Shared) -> PathBuf {
    sh.dir.join(FILE)
}

/// 存着的 webhook，解出来的；没存、存坏、不合规，都没有。
pub fn url_of(sh: &crate::Shared) -> Option<String> {
    let bytes = fs::read(file(sh)).ok()?;
    let plain = crate::mail_secrets::unprotect(&bytes).ok()?;
    let url = String::from_utf8(plain).ok()?.trim().to_string();
    url_ok(&url).then_some(url)
}

/// 加密写下去：先写暂存文件，再原子替换（照 mail_secrets.rs 的做法）。
fn keep(sh: &crate::Shared, url: &str) -> Result<(), String> {
    let blob = crate::mail_secrets::protect(url.as_bytes())?;
    let path = file(sh);
    let tmp = sh.dir.join(format!(".feishu-{}-{}.tmp", std::process::id(), crate::now_ms()));
    let mut out = fs::OpenOptions::new().write(true).create_new(true).open(&tmp).map_err(|_| "write".to_string())?;
    if out.write_all(&blob).and_then(|_| out.sync_all()).is_err() {
        let _ = fs::remove_file(&tmp);
        return Err("write".to_string());
    }
    drop(out);
    crate::mail_secrets::replace_file(&tmp, &path)
}

fn forget(sh: &crate::Shared) -> Result<(), String> {
    match fs::remove_file(file(sh)) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(_) => Err("clear".to_string()),
    }
}

// --- 发 ------------------------------------------------------------------------------

fn payload(text: &str) -> Value {
    json!({ "msg_type": "text", "content": { "text": text } })
}

/// 消息里的动态内容不能把飞书的标签（如 `<at user_id="all">`）带进来：尖括号
/// 换成全角，最多少见地难看，不会被当成 @所有人。
fn safe(value: &str) -> String {
    value.chars().map(|c| match c { '<' => '＜', '>' => '＞', _ => c }).collect()
}

fn group_of(mood: &str) -> &'static str {
    match mood {
        "waiting" => "waiting",
        "error" => "error",
        _ => "done",
    }
}

/// 一张飞书卡片：标题按动静变色，正文一句通用的话（不带名字，名字在下面的
/// 字段里），再是仓库（有才算）、对话、工作区，最后是时间和状态。
fn card(lang: &str, alert: &crate::state::Alert, sentence: &str) -> Value {
    let group = group_of(&alert.mood);
    let (title, template, status) = match group {
        "waiting" => ("feishu.cardWaiting", "orange", "feishu.statusWaiting"),
        "error" => ("feishu.cardError", "red", "feishu.statusError"),
        _ => ("feishu.cardDone", "green", "feishu.statusDone"),
    };
    let t = |key: &str| crate::i18n::t(lang, key);
    let mut lines = Vec::new();
    if !alert.repo.is_empty() {
        lines.push(format!("**{}**：{}", t("feishu.labelRepo"), safe(&alert.repo)));
    }
    let chat = if alert.chat.is_empty() { t("feishu.chatUnknown") } else { safe(&alert.chat) };
    lines.push(format!("**{}**：{}", t("feishu.labelChat"), chat));
    if !alert.workspace.is_empty() {
        lines.push(format!("**{}**：{}", t("feishu.labelWorkspace"), safe(&alert.workspace)));
    }
    let minutes = crate::widgets::local_now().3 / 60_000;
    let clock = format!("{:02}:{:02}", minutes / 60, minutes % 60);
    let foot = format!("**{}**：{}　**{}**：{}", t("feishu.labelTime"), clock, t("feishu.labelStatus"), t(status));
    json!({
        "msg_type": "interactive",
        "card": {
            "schema": "2.0",
            "header": {
                "title": { "tag": "plain_text", "content": t(title) },
                "subtitle": { "tag": "plain_text", "content": t("feishu.cardSubtitle") },
                "template": template,
            },
            "body": {
                "elements": [
                    { "tag": "markdown", "content": safe(sentence) },
                    { "tag": "hr" },
                    { "tag": "markdown", "content": lines.join("\n") },
                    { "tag": "markdown", "content": foot },
                ],
            },
        },
    })
}

fn post(url: &str, body: &Value) -> Result<(), String> {
    let text = body.to_string();
    let mut last = "network:no way".to_string();
    for way in web_ways(url) {
        let Some(agent) = way.agent_with(TIMEOUT, |c| c.http_status_as_error(false)) else { continue };
        let sent = agent
            .post(url)
            .header("content-type", "application/json")
            .header("user-agent", &format!("wakuwaku/{}", crate::updates::current()))
            .send(text.as_str());
        match sent {
            Ok(mut res) => {
                let status = res.status().as_u16();
                let said = res.body_mut().with_config().limit(64 * 1024).read_to_string().unwrap_or_default();
                return judge(status, &said);
            }
            Err(error) => last = format!("network:{error}"),
        }
    }
    Err(last)
}

/// 飞书的成功与否只看 body 的 code：业务错误也是 HTTP 200，拿状态当成功会
/// 把"没发出去"当"发出去"。错误码换成页面能翻的话（feishu.err.*）。
fn judge(status: u16, body: &str) -> Result<(), String> {
    let parsed = serde_json::from_str::<Value>(body).unwrap_or(Value::Null);
    match parsed.get("code").and_then(Value::as_i64) {
        Some(0) => Ok(()),
        Some(code) => Err(format!("api:{code}")),
        None => Err(format!("http:{status}")),
    }
}

fn remember(outcome: &Result<(), String>) {
    let last = match outcome {
        Ok(()) => Last { ok: true, code: "ok".to_string(), at: crate::now_ms() },
        Err(code) => Last { ok: false, code: code.clone(), at: crate::now_ms() },
    };
    *LAST.lock().unwrap_or_else(|e| e.into_inner()) = Some(last);
}

/// 这一次还发不发：同一句话刚发过就不发，一分钟到量也不发。
fn take_turn(said: &str) -> bool {
    let now = Instant::now();
    let mut sent = SENT.lock().unwrap_or_else(|e| e.into_inner());
    loop {
        let stale = sent.front().is_some_and(|entry| now.duration_since(entry.1) >= SAME_WITHIN);
        if !stale {
            break;
        }
        sent.pop_front();
    }
    if sent.iter().any(|entry| entry.0.as_str() == said) {
        return false;
    }
    let mut times = TIMES.lock().unwrap_or_else(|e| e.into_inner());
    loop {
        let stale = times.front().is_some_and(|at| now.duration_since(*at) >= Duration::from_secs(60));
        if !stale {
            break;
        }
        times.pop_front();
    }
    if times.len() >= MOST_A_MINUTE {
        return false;
    }
    sent.push_back((said.to_string(), now));
    times.push_back(now);
    true
}

// --- 页面与她的动静 ------------------------------------------------------------------

/// 设置页看见的：存没存、末尾四位、上一次发送的结果（不给完整地址）。
pub fn view(sh: &crate::Shared) -> Value {
    let url = url_of(sh);
    let last = LAST.lock().unwrap_or_else(|e| e.into_inner()).clone();
    json!({
        "saved": url.is_some(),
        "tail": url.as_deref().map(tail_of),
        "last": last,
    })
}

/// 她的动静（main.rs alert）：飞书开着、这一类也开着、地址存着，才发。
/// 发在一个自己的线程上：连不上飞书不能让她卡住。卡片被机器人的安全设置
/// 拒绝时，退一步发纯文本（至少话能到）。
pub fn alert(app: &AppHandle, alert: &crate::state::Alert) {
    let sh = crate::shared(app);
    let setting = sh.setting("feishu");
    if setting.get("on").and_then(Value::as_bool) != Some(true) {
        return;
    }
    let group = group_of(&alert.mood);
    if setting.get(group).and_then(Value::as_bool) != Some(true) {
        return;
    }
    let Some(url) = url_of(&sh) else { return };
    let lang = sh.lang();
    let said = crate::i18n::t(
        lang,
        match group {
            "waiting" => "feishu.sentenceWaiting",
            "error" => "feishu.sentenceError",
            _ => "feishu.sentenceDone",
        },
    );
    if !take_turn(&format!("{group}:{}", alert.whose)) {
        return;
    }
    let card = card(lang, alert, &said);
    let fallback = payload(&safe(&said));
    let app = app.clone();
    std::thread::spawn(move || {
        let outcome = match post(&url, &card) {
            Ok(()) => Ok(()),
            Err(code) if code.starts_with("api:") || code.starts_with("http:") => {
                crate::shared(&app).log(&format!("feishu: card refused ({code}), the words only"));
                post(&url, &fallback).map_err(|fallback_code| format!("{code};{fallback_code}"))
            }
            Err(code) => Err(code),
        };
        let sh = crate::shared(&app);
        match &outcome {
            Ok(()) => sh.log("feishu: sent"),
            Err(code) => sh.log(&format!("feishu: not sent ({code})")),
        }
        remember(&outcome);
    });
}

// --- 页面的命令 ----------------------------------------------------------------------

#[tauri::command]
pub fn feishu_save(app: AppHandle, url: String) -> Result<Value, String> {
    let sh = crate::shared(&app);
    let url = url.trim().to_string();
    if !url_ok(&url) {
        return Err("invalid".to_string());
    }
    keep(&sh, &url)?;
    sh.log("feishu: webhook kept");
    sh.redraw();
    Ok(crate::settings::snapshot(&sh))
}

#[tauri::command]
pub fn feishu_clear(app: AppHandle) -> Result<Value, String> {
    let sh = crate::shared(&app);
    forget(&sh)?;
    sh.log("feishu: webhook removed");
    sh.redraw();
    Ok(crate::settings::snapshot(&sh))
}

/// 发一条测试消息，等它答（最多 TIMEOUT）：页面上的"发送测试消息"。
#[tauri::command]
pub async fn feishu_test(app: AppHandle) -> Result<Value, String> {
    let sh = crate::shared(&app);
    let Some(url) = url_of(&sh) else { return Err("missing".to_string()) };
    let lang = sh.lang();
    let text = crate::i18n::t(lang, "feishu.testText");
    // 一条样例：走和真实消息同一个卡片，只是数据是假的。
    let sample = crate::state::Alert {
        mood: "waiting".to_string(),
        whose: "FISH Dock".to_string(),
        chat: crate::i18n::t(lang, "feishu.testChat"),
        repo: String::new(),
        workspace: String::new(),
    };
    let card = card(lang, &sample, &crate::i18n::t(lang, "feishu.testSentence"));
    let fallback = payload(&text);
    // 阻塞的请求放到别的线程上：界面不等它。
    let outcome = tokio::task::spawn_blocking(move || match post(&url, &card) {
        Ok(()) => Ok(()),
        Err(code) if code.starts_with("api:") || code.starts_with("http:") => post(&url, &fallback).map_err(|fallback_code| format!("{code};{fallback_code}")),
        Err(code) => Err(code),
    })
    .await
    .map_err(|_| "thread".to_string())?;
    remember(&outcome);
    let sh = crate::shared(&app);
    match &outcome {
        Ok(()) => sh.log("feishu: test sent"),
        Err(code) => sh.log(&format!("feishu: test not sent ({code})")),
    }
    outcome?;
    sh.redraw();
    Ok(crate::settings::snapshot(&sh))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_bot_address_is_kept() {
        // 合成地址：绝不把真实的 webhook 写进源码或测试。
        assert!(url_ok("https://open.feishu.cn/open-apis/bot/v2/hook/00000000-0000-0000-0000-000000000000"));
        assert!(url_ok("https://open.larksuite.com/open-apis/bot/v2/hook/abc_DEF-1.2"));
        // 明文 http、别的域名、别的路径、空的 token：都不收（也挡 SSRF 与误填）。
        assert!(!url_ok("http://open.feishu.cn/open-apis/bot/v2/hook/abc"));
        assert!(!url_ok("https://open.feishu.cn.evil.example/open-apis/bot/v2/hook/abc"));
        assert!(!url_ok("https://open.feishu.cn/open-apis/bot/v2/hook/"));
        assert!(!url_ok("https://open.feishu.cn/open-apis/bot/v2/hook/abc/../x"));
        assert!(!url_ok("https://example.com/open-apis/bot/v2/hook/abc"));
        assert!(!url_ok(""));
    }

    #[test]
    fn the_page_sees_only_the_tail() {
        assert_eq!(tail_of("https://open.feishu.cn/open-apis/bot/v2/hook/0123456789abcd"), "...abcd");
        assert_eq!(tail_of("https://open.feishu.cn/open-apis/bot/v2/hook/ab"), "...ab");
    }

    #[test]
    fn success_is_the_code_in_the_body() {
        assert_eq!(judge(200, r#"{"StatusCode":0,"code":0,"msg":"success","data":{}}"#), Ok(()));
        // 业务错误也回 200：只看状态会当成功。
        assert_eq!(judge(200, r#"{"code":19024,"msg":"Key Words Not Found"}"#), Err("api:19024".to_string()));
        assert_eq!(judge(200, r#"{"code":19021,"msg":"sign match fail"}"#), Err("api:19021".to_string()));
        assert_eq!(judge(429, ""), Err("http:429".to_string()));
    }

    #[test]
    fn a_card_of_a_mood() {
        let alert = |mood: &str, repo: &str| crate::state::Alert {
            mood: mood.to_string(),
            whose: "FISH Dock".to_string(),
            chat: "修飞书通知".to_string(),
            repo: repo.to_string(),
            workspace: "D:\\Mochen\\Project\\FISH-Dock".to_string(),
        };
        let waiting = card("zh", &alert("waiting", "FISH-Dock"), "FISH Dock需要你确认");
        assert_eq!(waiting["msg_type"], "interactive");
        assert_eq!(waiting["card"]["schema"], "2.0");
        assert_eq!(waiting["card"]["header"]["title"]["content"], "⚠️ 需要你确认");
        assert_eq!(waiting["card"]["header"]["template"], "orange");
        let fields = waiting["card"]["body"]["elements"][2]["content"].as_str().unwrap();
        assert!(fields.contains("仓库"), "{fields}");
        assert!(fields.contains("修飞书通知"), "{fields}");
        assert!(fields.contains("D:\\Mochen\\Project\\FISH-Dock"), "{fields}");
        // 没有仓库就不该有那一行。
        let done = card("zh", &alert("done", ""), "x");
        assert_eq!(done["card"]["header"]["template"], "green");
        let fields = done["card"]["body"]["elements"][2]["content"].as_str().unwrap();
        assert!(!fields.contains("仓库"), "{fields}");
        assert_eq!(card("zh", &alert("error", ""), "x")["card"]["header"]["template"], "red");
    }

    #[test]
    fn tags_in_the_words_are_not_feishu_mentions() {
        let alert = crate::state::Alert {
            mood: "waiting".to_string(),
            whose: String::new(),
            chat: "<at user_id=\"all\">所有人</at>".to_string(),
            repo: String::new(),
            workspace: String::new(),
        };
        let shown = card("zh", &alert, "x")["card"]["body"]["elements"][2]["content"].as_str().unwrap().to_string();
        assert!(!shown.contains("<at"), "{shown}");
        assert!(shown.contains("＜at"), "{shown}");
    }

    #[test]
    fn settings_accept_only_the_four_switches() {
        assert!(is_ok(&json!({ "on": true, "waiting": true, "done": false, "error": true })));
        assert!(!is_ok(&json!({})));
        assert!(!is_ok(&json!({ "on": "yes" })));
        assert!(!is_ok(&json!({ "on": true, "url": "https://x" })));
    }
}
