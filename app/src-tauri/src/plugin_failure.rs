//! 插件操作的失败文案分级（t45）：一句人话 + 可展开的逐源详情。
//!
//! 用户实测到的旧提示是把三个下载源的内部诊断用「；」拼成一堵墙。问题不在诊断
//! 不准，而在**全摊给用户**。这里把失败整理成结构化 JSON：`message` 只留一句
//! 结论，`sources` 每条一行短原因，原始技术串放 `detail`/`sources[].error`，
//! 同时整份原始诊断写进 `recent.log`（可诊断性不能丢）。
//!
//! 只在 API 边界做「长中文错误 → 短句」的分级；下载/校验/重试顺序与
//! plugin_progress 语义都不动。
use std::path::Path;

/// 我们自己的结构化失败标记：前端见到它就按「一句人话 + 展开详情」渲染。
pub const MARK: &str = "fishFailure";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourceFailure {
    /// 来源的展示名（Origin::label，如「npm 官方源」）。
    pub origin: String,
    /// 给用户看的短原因（一行，如「版本与目录不一致」）。
    pub short: String,
    /// 原始技术串（进 title 属性与日志）。
    pub error: String,
}

/// 下载阶段：三条来源都没成功。`message` 只有一句结论，不再拼三源诊断。
pub fn download_failed(failures: &[(String, String)]) -> String {
    let sources: Vec<SourceFailure> = failures
        .iter()
        .map(|(origin, error)| SourceFailure { origin: origin.clone(), short: source_short(error).to_string(), error: error.clone() })
        .collect();
    let payload = serde_json::json!({
        MARK: true,
        "stage": "download",
        "message": "三个下载来源都没成功，插件没能更新。",
        "hint": "检查网络或代理后重试。",
        "sources": sources.iter().map(|s| serde_json::json!({"origin": s.origin, "short": s.short, "error": s.error})).collect::<Vec<_>>(),
    });
    payload.to_string()
}

/// 是不是我们自己的结构化失败 JSON（前端/其它层据此判断要不要解析）。
pub fn is_structured(raw: &str) -> bool {
    raw.trim_start().starts_with('{') && raw.contains(&format!("\"{MARK}\":true")) || raw.contains(&format!("\"{MARK}\": true"))
}

/// 把「一条原始错误」包成结构化 JSON：
/// - 已经是结构化 JSON：原样返回；
/// - 短的人话（≤ 32 字，且不像技术串）：原样返回，别把好提示改坏；
/// - 长中文技术错误：按类别给一句短话，原始串进 `detail`，并写日志。
pub fn wrap(stage: &str, raw: String, data_dir: &Path) -> String {
    if is_structured(&raw) {
        return raw;
    }
    let chars = raw.chars().count();
    let technical = raw.contains("不一致") || raw.contains("校验") || raw.contains("摘要")
        || raw.contains("跳转") || raw.contains("连接") || raw.contains("超时") || raw.contains("中断")
        || raw.contains("超过大小限制") || raw.contains("失败") || raw.contains("越界");
    if chars <= 32 && !technical {
        return raw; // 本来就够短、够人话
    }
    let (kind, short, hint) = classify(&raw);
    log(data_dir, stage, &raw);
    serde_json::json!({
        MARK: true,
        "stage": stage,
        "kind": kind,
        "message": short,
        "hint": hint,
        "detail": raw,
        "sources": Vec::<serde_json::Value>::new(),
    })
    .to_string()
}

/// 按「用户能做什么」归类（网络 / 来源不一致 / 校验失败 / 磁盘 / 权限 / 其他）。
pub fn classify(raw: &str) -> (&'static str, &'static str, &'static str) {
    let has = |needle: &str| raw.contains(needle);
    if has("磁盘") || has("空间不足") || has("写入失败") || has("资源目录创建失败") {
        ("disk", "写入磁盘失败，检查磁盘空间后重试。", "检查磁盘空间与目录权限。")
    } else if has("权限") || has("拒绝") || has("越界") {
        ("permission", "没有权限访问目标位置。", "检查目录权限或安全软件拦截。")
    } else if has("与官方目录") || has("不一致") || has("身份或版本") {
        ("mismatch", "下载到的包与官方目录对不上，请重试。", "可能是镜像未同步或网络被改写，稍后再试。")
    } else if has("校验") || has("摘要") || has("损坏") || has("不符") {
        ("verify", "包可能损坏，请重试。", "重新下载；若反复出现请联系插件维护者。")
    } else if has("连接") || has("超时") || has("中断") || has("网络") || has("代理") || has("HTTP") {
        ("network", "网络没连上，检查网络或代理后重试。", "确认网络与代理设置，稍后再试。")
    } else {
        ("other", "操作没有完成，请稍后重试。", "详情见日志；若反复出现请附日志反馈。")
    }
}

/// 单条下载来源的短原因（折叠区一行）。保序、尽量具体但不搬整串。
pub fn source_short(raw: &str) -> &'static str {
    let has = |needle: &str| raw.contains(needle);
    if has("跳转不属于官方发行资源服务") {
        "跳转被拒绝"
    } else if has("大小与官方目录不一致") || has("摘要与官方目录不一致") || has("身份或版本与官方目录不一致") {
        "版本与目录不一致"
    } else if has("中断") {
        "下载中断"
    } else if has("超过大小限制") {
        "超过大小限制"
    } else if has("连接失败") || has("连接不上") || has("连接超时") || has("超时") {
        "连接超时"
    } else if has("HTTP") {
        "服务器返回错误"
    } else if has("校验") || has("摘要") || has("损坏") {
        "校验失败"
    } else if has("磁盘") || has("写入") {
        "写入磁盘失败"
    } else if has("权限") || has("拒绝") {
        "没有权限"
    } else {
        "下载失败"
    }
}

/// 完整原始诊断写进 recent.log（`<data>/recent.log`）。换行压成 ⏎，一行一条。
/// 直接追加而非走 crash::tail 的全局句柄缓存——同一目录、同一文件，但不受别的
/// 模块（先开过别的目录的句柄）影响，也能在单元测试里对着临时目录验。
pub fn log(data_dir: &Path, stage: &str, raw: &str) {
    use std::io::Write;
    let line = format!("{} [plugin] {stage}: {}\n", crate::now_ms(), raw.replace(['\r', '\n'], "⏎"));
    let path = data_dir.join("recent.log");
    if let Ok(mut file) = std::fs::OpenOptions::new().create(true).append(true).open(&path) {
        let _ = file.write_all(line.as_bytes());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_download_failure_is_one_sentence_plus_three_source_rows() {
        let failures = vec![
            ("npm 官方源".to_string(), "npm 包里的插件归档大小与官方目录不一致（30859277 ≠ 30957961）".to_string()),
            ("npm 镜像".to_string(), "插件下载跳转不属于官方发行资源服务".to_string()),
            ("GitHub 发行包".to_string(), "插件下载中断或超过大小限制".to_string()),
        ];
        let json = download_failed(&failures);
        assert!(is_structured(&json), "应是带标记的结构化 JSON：{json}");
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();
        let message = value["message"].as_str().unwrap();
        assert!(!message.contains('；'), "message 不许再把三源拼成墙：{message}");
        assert!(!message.contains("npm 官方源"), "message 不含逐源诊断：{message}");
        assert!(message.chars().count() <= 24, "message 一句人话：{message}");
        let sources = value["sources"].as_array().unwrap();
        assert_eq!(sources.len(), 3, "三条来源各一行");
        assert_eq!(sources[0]["origin"], "npm 官方源");
        assert_eq!(sources[0]["short"], "版本与目录不一致");
        assert_eq!(sources[1]["short"], "跳转被拒绝");
        assert_eq!(sources[2]["short"], "下载中断");
        // 原始串仍在（可诊断）：逐源 error 带完整串。
        assert!(sources[0]["error"].as_str().unwrap().contains("30859277 ≠ 30957961"));
    }

    #[test]
    fn classification_covers_every_actionable_category() {
        for (raw, kind) in [
            ("官方商店连接失败，请检查网络后重试", "network"),
            ("插件下载中断或超过大小限制", "network"),
            ("npm 包里的插件归档摘要与官方目录不一致", "mismatch"),
            ("插件资源校验失败：backend.mjs", "verify"),
            ("插件资源目录创建失败", "disk"),
            ("插件文件越界或过大", "permission"),
            ("某种没想到的错误", "other"),
        ] {
            assert_eq!(classify(raw).0, kind, "{raw}");
        }
    }

    #[test]
    fn a_short_human_message_is_left_untouched_and_a_long_one_is_graded_and_logged() {
        let data = std::env::temp_dir().join(format!("fish-failure-{}-{}", std::process::id(), crate::now_ms()));
        std::fs::create_dir_all(&data).unwrap();
        // 短人话原样返回。
        assert_eq!(wrap("uninstall", "请确认插件安装或管理操作".into(), &data), "请确认插件安装或管理操作");
        // 长技术错误：变结构化短句，原始串进日志。
        let raw = "npm 包里的插件归档大小与官方目录不一致（30859277 ≠ 30957961）";
        let out = wrap("download", raw.into(), &data);
        assert!(is_structured(&out), "{out}");
        let value: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert!(!value["message"].as_str().unwrap().contains("30859277"), "message 不带整串：{out}");
        assert!(value["detail"].as_str().unwrap().contains("30859277"), "原始串进 detail（可诊断）：{out}");
        let logged = std::fs::read_to_string(data.join("recent.log")).unwrap_or_default();
        assert!(logged.contains(raw), "完整原始串必须进 recent.log，实际：{logged}");
        // 已是结构化 JSON：原样透传。
        assert_eq!(wrap("download", out.clone(), &data), out);
        let _ = std::fs::remove_dir_all(&data);
    }
}
