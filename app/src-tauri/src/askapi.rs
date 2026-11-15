// DEC-7（上游 v0.4.3 #149 `POST /ask` 的等价实现，安全默认）：让外部程序经本机
// loopback 请求一次人工审批，把结果只回给发起连接。
//
// 安全默认（与 decisions-captain.md 一致，不得为过测试而放宽）：
//   - 仅 loopback：绑定/校验都按 127.0.0.1/localhost/[::1] 与端口（复用 agent_hub 的
//     host_ok / origin_ok / bearer_ok，与 `/v1` 同一套）；
//   - 默认关闭：只有 FISH_DOCK_ASK=1（或设置 askApi 为 true）才开；关时一律 404「未开启」；
//   - Bearer + Host/Origin 校验：缺 token、错 token、坏 Host、坏 Origin 都拒；
//   - 每请求弹一次原生确认（含调用方路径与用途）：同一请求不重复弹（按 request id 去重）；
//   - 超时自动拒绝：等多久由调用方给（有上限），超时按「拒绝」结束；
//   - 审计日志：每个请求记一行（来方、用途、结论），不记 token；
//   - 拒绝与超时可区分：body 里 decision=allow|deny|timeout。
//
// 本模块只放**纯逻辑**（便于单测）：开启判定、参数校验、审计行、结论归类。真正的
// 原生确认走宿主既有的 asks 队列（server.rs 里用 Responder 回写，与 hook 审批同一条路）。

use serde_json::{json, Value};

/// 一次 /ask 请求解析出的、要交给宿主确认的东西。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AskRequest {
    /// 调用方给的一次性 id（用于去重，不重复弹窗）。
    pub request_id: String,
    /// 谁在问（可执行文件路径或名字），给确认面板显示。
    pub caller: String,
    /// 用途（一句话，给面板显示）。
    pub purpose: String,
    /// 等多久（毫秒），已按上限裁剪。
    pub wait_ms: u64,
}

/// 一次 /ask 的结论。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Decision {
    Allow,
    Deny,
    Timeout,
}

impl Decision {
    pub fn as_str(self) -> &'static str {
        match self {
            Decision::Allow => "allow",
            Decision::Deny => "deny",
            Decision::Timeout => "timeout",
        }
    }
}

/// 从一次审批的选择（宿主 Asks 回给的结果）归类成 Decision：选 allow/approve 类为 Allow，
/// 明确 deny/reject 类为 Deny，没回（None，含超时）为 Timeout。拒绝与超时因此可区分。
pub fn classify(outcome: Option<&Value>) -> Decision {
    match outcome {
        None => Decision::Timeout,
        Some(v) => {
            let s = v.as_str().map(str::to_string).or_else(|| v.get("action").and_then(|a| a.as_str()).map(str::to_string)).unwrap_or_default();
            let s = s.to_ascii_lowercase();
            if s == "allow" || s == "approve" || s == "yes" || v.get("allow").and_then(|b| b.as_bool()) == Some(true) {
                Decision::Allow
            } else {
                Decision::Deny
            }
        }
    }
}

/// 是否开启（默认关）。环境变量 FISH_DOCK_ASK 为 1/true 才开；设置里的 askApi 为 true 也开。
pub fn enabled_from(env: Option<&str>, setting: Option<bool>) -> bool {
    if let Some(raw) = env {
        let v = raw.trim();
        if v == "1" || v.eq_ignore_ascii_case("true") || v.eq_ignore_ascii_case("yes") {
            return true;
        }
    }
    setting == Some(true)
}

/// 反复弹窗的上限：同一个 caller+purpose 在短时间内不重复弹（由宿主侧按 request_id 去重；
/// 这里只校验 request_id 合法）。request_id 只允许可见 ASCII，长度 1..=64。
pub fn valid_request_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 64 && id.chars().all(|c| c.is_ascii_graphic())
}

/// 把请求体解析成 AskRequest。缺字段、id 非法、wait 非法都返回给调用方的错误文案。
/// 字段：{ "id": "...", "caller": "...", "purpose": "...", "wait_ms": 30000 }。
pub fn parse(body: &Value) -> Result<AskRequest, String> {
    let id = body.get("id").and_then(|v| v.as_str()).unwrap_or("").to_string();
    if !valid_request_id(&id) {
        return Err("bad id".into());
    }
    let caller = body.get("caller").and_then(|v| v.as_str()).unwrap_or("an external program").chars().take(200).collect::<String>();
    let purpose = body.get("purpose").and_then(|v| v.as_str()).unwrap_or("").chars().take(500).collect::<String>();
    if purpose.trim().is_empty() {
        return Err("purpose is required".into());
    }
    // 等多久：默认 30 秒，夹在 1 秒到 10 分钟之间。
    let wait_ms = body.get("wait_ms").and_then(|v| v.as_u64()).unwrap_or(30_000).clamp(1_000, 600_000);
    Ok(AskRequest { request_id: id, caller, purpose, wait_ms })
}

/// 给确认面板的 event（与 hook 审批同一形状，asks.rs view_of 认得）。
pub fn event_for(req: &AskRequest) -> Value {
    json!({
        "hook_event_name": "ExternalAsk",
        "kind": "permission",
        // 面板上显示调用方与用途；canAnswer 保持默认（可答）。
        "tool": req.caller,
        "input": { "purpose": req.purpose },
        "external": true,
    })
}

/// 审计日志的一行（不记 token）：时间由调用方补，这里只拼「谁、为何、结果」。
pub fn audit_line(req: &AskRequest, decision: Decision) -> String {
    format!("ask {} from {} (\"{}\"): {}", req.request_id, req.caller, req.purpose, decision.as_str())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_off() {
        assert!(!enabled_from(None, None));
        assert!(!enabled_from(Some("0"), None));
        assert!(!enabled_from(Some("false"), None));
        assert!(enabled_from(Some("1"), None));
        assert!(enabled_from(Some("true"), None));
        assert!(enabled_from(None, Some(true)));
        assert!(!enabled_from(None, Some(false)));
    }

    #[test]
    fn parse_requires_a_purpose_and_a_sane_id() {
        assert!(parse(&json!({})).is_err());
        assert!(parse(&json!({ "id": "x", "purpose": "" })).is_err());
        assert!(parse(&json!({ "id": "", "purpose": "do" })).is_err());
        // 不可见字符的 id 也拒。
        assert!(parse(&json!({ "id": "a\tb", "purpose": "do" })).is_err());
        let ok = parse(&json!({ "id": "req-1", "purpose": "install a plugin", "caller": "setup.exe" })).unwrap();
        assert_eq!(ok.request_id, "req-1");
        assert_eq!(ok.caller, "setup.exe");
        assert_eq!(ok.purpose, "install a plugin");
        assert_eq!(ok.wait_ms, 30_000, "默认 30 秒");
        // wait 上限裁剪。
        let capped = parse(&json!({ "id": "r", "purpose": "p", "wait_ms": 9_999_999 })).unwrap();
        assert_eq!(capped.wait_ms, 600_000);
        let floored = parse(&json!({ "id": "r", "purpose": "p", "wait_ms": 1 })).unwrap();
        assert_eq!(floored.wait_ms, 1_000);
    }

    #[test]
    fn reject_and_timeout_are_distinct() {
        assert_eq!(classify(None), Decision::Timeout);
        assert_eq!(classify(Some(&json!({ "action": "allow" }))), Decision::Allow);
        assert_eq!(classify(Some(&json!("approve"))), Decision::Allow);
        assert_eq!(classify(Some(&json!({ "allow": true }))), Decision::Allow);
        assert_eq!(classify(Some(&json!({ "action": "deny" }))), Decision::Deny);
        assert_eq!(classify(Some(&json!("reject"))), Decision::Deny);
        assert_eq!(classify(Some(&json!({}))), Decision::Deny);
        // 三种结论文本各不相同。
        assert_ne!(Decision::Deny.as_str(), Decision::Timeout.as_str());
        assert_ne!(Decision::Allow.as_str(), Decision::Timeout.as_str());
    }

    #[test]
    fn audit_never_carries_a_token() {
        let req = parse(&json!({ "id": "r9", "purpose": "read inbox", "caller": "curl" })).unwrap();
        let line = audit_line(&req, Decision::Allow);
        assert!(line.contains("r9") && line.contains("curl") && line.contains("allow"));
        assert!(!line.to_ascii_lowercase().contains("bearer"));
        assert!(!line.contains("Authorization"));
    }

    #[test]
    fn event_shows_caller_and_purpose() {
        let req = parse(&json!({ "id": "r9", "purpose": "wipe data", "caller": "evil.exe" })).unwrap();
        let e = event_for(&req);
        assert_eq!(e["tool"], "evil.exe");
        assert_eq!(e["input"]["purpose"], "wipe data");
        assert_eq!(e["external"], true);
    }
}
