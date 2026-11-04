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
    /// 这次失败针对的候选版本（回退装旧版时，面板要能看出是哪一版没下到）。
    pub version: String,
    /// 真正请求过的地址（可复制；#68 要求面板给 URL，不只是「某条源」）。
    pub url: String,
    /// HTTP 状态码；拿不到（连接就断了、方法不支持）是 None。JSON 键名 `httpStatus`。
    pub http_status: Option<u16>,
    /// 失败阶段：连接 / 跳转 / HTTP / 校验 / 摘要（五词，见 [`phase`]）。JSON 键名 `phase`。
    pub phase: String,
    /// 给用户看的短原因（一行，如「版本与目录不一致」）。
    pub short: String,
    /// 原始技术串（进 title 属性与日志）。
    pub error: String,
}

impl SourceFailure {
    /// 这个来源这个版本是「制品不存在」而不是网络不通（#68 的分界）。
    /// 谓词只有一处（[`artifact_missing_status`]），classify 也调它——两套口径会漂。
    fn artifact_missing(&self) -> bool {
        artifact_missing_status(self.http_status)
    }
}

/// 「制品缺失」的唯一判定：404（不存在）/ 410（已下架）。
/// **403 不算**（t18 F6 选项①）：403 更常见的是限流、鉴权或 CDN 拒绝 HEAD/GET，
/// 它不能证明「这一版没有制品」。探活遇到 403 时按「不定论」处理，照旧发起完整下载，
/// 让真实的错误串（以及它带来的分类）决定结果——这样既不会因为一次限流就把一条
/// 本来可用的来源判死，也不会把「限流」说成「目录抢跑」。
pub(crate) fn artifact_missing_status(status: Option<u16>) -> bool {
    matches!(status, Some(404 | 410))
}

/// 下载阶段：三条来源都没成功。`message` 只有一句结论，不再拼三源诊断。
///
/// #68：三源全是制品缺失（目录抢跑、资产还没发布）时，这不是网络问题——`kind`
/// 给 `catalog`，`hint` 写明「已回退到哪个版本也没成、不用改代理」；`fallback` 是
/// 宿主按来源回退时试过的最新更早版本，让 hint 能说清退到了哪一步。
/// `attempts` 是全失败时逐候选的尝试记录（t18 F4）：JSON 里带出去，recent.log 里
/// 也就能看到「哪条来源的哪一版、什么状态码、卡在哪个阶段」——不靠面板那 3 行摘要。
pub fn download_failed(failures: &[SourceFailure], latest: Option<&str>, fallback: Option<&str>, attempts: &[serde_json::Value]) -> String {
    let sources: Vec<serde_json::Value> = failures
        .iter()
        .map(|s| serde_json::json!({"origin": s.origin, "version": s.version, "url": s.url, "httpStatus": s.http_status, "phase": s.phase, "short": s.short, "error": s.error}))
        .collect();
    // 全是「制品不存在」才算目录类失败；只要有一条是连不通/校验不过，就仍按网络类说。
    let catalog_miss = !failures.is_empty() && failures.iter().all(SourceFailure::artifact_missing);
    // 全是校验/摘要不符：包下到了但和官方目录对不上（镜像未同步、被改写），既不是
    // 网络也不是「目录抢跑」——按「包与目录不一致」说，让用户重试而不是查代理。
    let verification_miss = !failures.is_empty() && failures.iter().all(|s| matches!(s.phase.as_str(), "校验" | "摘要"));
    let (kind, message, hint) = if catalog_miss {
        (
            "catalog",
            if fallback.is_some() { "目录里的新版暂不可用，插件没能更新。" } else { "目录里的新版暂不可用，无可回退版本。" },
            match fallback {
                Some(version) => format!("目录最新 {version} 暂不可用，已回退到它也没有可下载的制品；无需修改网络或代理。"),
                None => "目录里的新版暂不可用，没有已验证的更早版本可回退；无需修改网络或代理。".to_string(),
            },
        )
    } else if verification_miss {
        ("mismatch", "下载到的包与官方目录对不上，没能更新。", "可能是镜像还没同步，稍后重试即可；无需修改代理。".to_string())
    } else {
        ("download", "三个下载来源都没成功，插件没能更新。", match fallback {
            // 回退路径也断了：既要说清「已经退过一步」，也不能再叫用户去查一个没坏的代理。
            Some(version) => format!("检查网络或代理后重试；已按来源回退到 {version} 也没成功。"),
            None => "检查网络或代理后重试。".to_string(),
        })
    };
    let payload = serde_json::json!({
        MARK: true,
        "stage": "download",
        "kind": kind,
        "message": message,
        "hint": hint,
        "latest": latest,
        "fallback": fallback,
        "sources": sources,
        "attempts": attempts,
    });
    payload.to_string()
}

/// 结构化失败也要落 recent.log（t18 F3）：`wrap()` 见到结构化 JSON 会原样透传、
/// 跳过 `log()`，而下载失败正是被它透传的那一类——全仓只有 plugin_api 那一个调用点，
/// 所以日志在这里补记一次。只认结构化串：非结构化的仍由 wrap() 记，避免同一份记两遍。
pub fn log_structured(data_dir: &Path, stage: &str, raw: &str) {
    if is_structured(raw) {
        log(data_dir, stage, raw);
    }
}

/// 失败阶段（#68）：连接 / 跳转 / HTTP / 校验 / 摘要。Attempt 与来源行共用这一处
/// 判定，前端拿到的 `phase` 与日志里写的是同一个词。
pub fn phase(raw: &str) -> &'static str {
    let has = |needle: &str| raw.contains(needle);
    if has("摘要") || has("SHA-256") {
        "摘要"
    } else if has("校验") || has("不一致") || has("不符") || has("损坏") || has("身份或版本") {
        "校验"
    } else if has("跳转") {
        "跳转"
    } else if has("HTTP") || has("中断") || has("超过大小限制") {
        "HTTP"
    } else {
        "连接"
    }
}

/// 从错误串里读回 HTTP 状态码。后端只在一处拼「HTTP {status}」，读回来也不会错。
pub fn http_status(raw: &str) -> Option<u16> {
    let rest = raw.split("HTTP ").nth(1)?;
    let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
    digits.parse().ok()
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
    } else if artifact_missing_status(http_status(raw)) || has("制品不存在") || has("目录里的最新版") {
        // #68：目录抢跑（写了还没发布的版本）不是网络问题，别再把用户支去查代理。
        // 判定与 SourceFailure 共用 artifact_missing_status，403 不在其中（见那里的理由）。
        ("catalog", "目录里的新版暂不可用，无需修改网络。", "已按来源回退到上一可用版本；目录修正后可再更新。")
    } else if has("HTTP 403") {
        // 403 常是限流/鉴权/CDN 拒绝，既不是「目录没有这一版」也不该让用户改代理。
        ("network", "来源暂时拒绝访问（403），稍后重试。", "403 常见于限流或鉴权，稍后重试即可；不用改网络设置。")
    } else if has("连接") || has("超时") || has("中断") || has("网络") || has("代理") || has("HTTP") {
        ("network", "网络没连上，检查网络或代理后重试。", "确认网络与代理设置，稍后再试。")
    } else {
        ("other", "操作没有完成，请稍后重试。", "详情见日志；若反复出现请附日志反馈。")
    }
}

/// 单条下载来源的短原因（折叠区一行）。保序、尽量具体但不搬整串。
pub fn source_short(raw: &str) -> &'static str {
    let has = |needle: &str| raw.contains(needle);
    // #68：404 就是「这一版没有制品」，不是「跳转被拒绝」也不是网络坏。
    if has("HTTP 404") {
        "版本制品不存在（404）"
    } else if has("HTTP 410") {
        "版本制品已下架（410）"
    } else if has("HTTP 403") {
        // t18 F6：403 不是「制品不存在」，措辞必须留出限流/鉴权的余地。
        "403 可能是限流或鉴权"
    } else if has("跳转不属于官方发行资源服务") {
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

    fn row(origin: &str, version: &str, url: &str, status: Option<u16>, error: &str) -> SourceFailure {
        SourceFailure {
            origin: origin.into(),
            version: version.into(),
            url: url.into(),
            http_status: status,
            phase: phase(error).into(),
            short: source_short(error).into(),
            error: error.into(),
        }
    }

    #[test]
    fn a_download_failure_is_one_sentence_plus_three_source_rows() {
        let failures = vec![
            row("npm 官方源", "2.1.7", "https://registry.npmjs.org/@fish-under-sea/fish-dock-plugin-magpie/-/fish-dock-plugin-magpie-2.1.7.tgz", None, "npm 包里的插件归档大小与官方目录不一致（30859277 ≠ 30957961）"),
            row("npm 镜像", "2.1.7", "https://registry.npmmirror.com/@fish-under-sea/fish-dock-plugin-magpie/-/fish-dock-plugin-magpie-2.1.7.tgz", None, "插件下载跳转不属于官方发行资源服务"),
            row("GitHub 发行包", "2.1.7", "https://github.com/Fish-under-sea/FISH-DOCK-Plugins/releases/download/v2.1.7/magpie.fish-plugin", None, "插件下载中断或超过大小限制"),
        ];
        let json = download_failed(&failures, Some("2.1.7"), None, &[]);
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
        assert_eq!(sources[0]["phase"], "校验", "大小不一致属校验阶段");
        assert_eq!(sources[1]["phase"], "跳转");
        assert_eq!(sources[2]["phase"], "HTTP");
        assert_eq!(value["kind"], "download", "有校验/连接问题时仍按普通下载失败说");
    }

    // #68 的核心分界：三源都 404 是「目录里的版本还没有制品」，不是网络坏。
    #[test]
    fn a_catalog_miss_gets_its_own_kind_and_names_the_version_it_fell_back_to() {
        let failures = vec![
            row("npm 官方源", "2.1.7", "https://registry.npmjs.org/@fish-under-sea/fish-dock-plugin-magpie/-/fish-dock-plugin-magpie-2.1.7.tgz", Some(404), "官方商店请求失败（HTTP 404）"),
            row("npm 镜像", "2.1.7", "https://registry.npmmirror.com/@fish-under-sea/fish-dock-plugin-magpie/-/fish-dock-plugin-magpie-2.1.7.tgz", Some(404), "官方商店请求失败（HTTP 404）"),
            row("GitHub 发行包", "2.1.7", "https://github.com/Fish-under-sea/FISH-DOCK-Plugins/releases/download/v2.1.7/magpie.fish-plugin", Some(404), "官方商店请求失败（HTTP 404）"),
        ];
        let attempts = vec![
            serde_json::json!({"origin":"npmRegistry","url":"https://registry.npmjs.org/fish-dock-plugin-magpie-2.1.7.tgz","ok":false,"elapsedMs":12,"httpStatus":404,"phase":"HTTP"}),
            serde_json::json!({"origin":"githubRelease","url":"https://github.com/Fish-under-sea/FISH-DOCK-Plugins/releases/download/v2.1.7/magpie.fish-plugin","ok":false,"elapsedMs":31,"httpStatus":404,"phase":"HTTP"}),
        ];
        let value: serde_json::Value = serde_json::from_str(&download_failed(&failures, Some("2.1.7"), Some("2.1.6"), &attempts)).unwrap();
        assert_eq!(value["kind"], "catalog", "三源全 404 归目录类：{value}");
        assert_eq!(value["stage"], "download", "仍是下载阶段（前端文案不改阶段）");
        assert_eq!(value["sources"][0]["httpStatus"], 404, "面板要能显示状态码");
        assert_eq!(value["sources"][0]["short"], "版本制品不存在（404）");
        // F4：逐次尝试也要带出去（面板只 3 行，诊断要能看到每一次）。
        assert_eq!(value["attempts"].as_array().unwrap().len(), 2, "attempts 随失败一起出去：{value}");
        assert_eq!(value["attempts"][0]["httpStatus"], 404);
        let hint = value["hint"].as_str().unwrap();
        assert!(hint.contains("2.1.6"), "hint 要写明已回退到哪个版本：{hint}");
        assert!(!hint.contains("检查网络"), "目录类失败不许再把用户支去查代理：{hint}");
        assert_eq!(value["latest"], "2.1.7");
        assert_eq!(value["fallback"], "2.1.6");
        // 没有可回退的候选时也不能空着 hint。
        let none: serde_json::Value = serde_json::from_str(&download_failed(&failures, Some("2.1.7"), None, &[])).unwrap();
        assert_eq!(none["kind"], "catalog");
        assert!(none["hint"].as_str().unwrap().contains("没有已验证"), "{}", none["hint"]);
        // 只要有一条不是 404，就回到网络类口径。
        let mut mixed = failures.clone();
        mixed[2] = row("GitHub 发行包", "2.1.7", "https://github.com/Fish-under-sea/FISH-DOCK-Plugins/releases/download/v2.1.7/magpie.fish-plugin", None, "官方商店连接失败，请检查网络后重试");
        assert_eq!(serde_json::from_str::<serde_json::Value>(&download_failed(&mixed, Some("2.1.7"), Some("2.1.6"), &[])).unwrap()["kind"], "download");
    }

    // t18 F5/F6：403 不是「制品缺失」（限流/鉴权更常见），三条路径必须一致——
    // artifact_missing_status、classify、source_short 与 download_failed 的 kind 同口径。
    #[test]
    fn a_403_is_rate_limiting_not_a_missing_artifact_on_every_path() {
        assert!(!artifact_missing_status(Some(403)), "403 不算制品缺失");
        assert!(artifact_missing_status(Some(404)) && artifact_missing_status(Some(410)));
        assert!(!artifact_missing_status(None), "没有状态码时不下定论");
        let raw = "官方商店请求失败（HTTP 403）";
        assert_eq!(source_short(raw), "403 可能是限流或鉴权", "措辞不许说成「制品不可访问」");
        let (kind, message, hint) = classify(raw);
        assert_eq!(kind, "network", "403 归网络/限流类，不落 catalog：{message} {hint}");
        assert!(hint.contains("限流") || hint.contains("稍后"), "hint 要提示稍后重试：{hint}");
        // 三源全 403：kind 不能是 catalog（那会把限流说成「目录抢跑」）。
        let all_403 = vec![
            row("npm 官方源", "2.1.7", "https://registry.npmjs.org/x.tgz", Some(403), raw),
            row("npm 镜像", "2.1.7", "https://registry.npmmirror.com/x.tgz", Some(403), raw),
            row("GitHub 发行包", "2.1.7", "https://github.com/a/b.fish-plugin", Some(403), raw),
        ];
        assert_eq!(serde_json::from_str::<serde_json::Value>(&download_failed(&all_403, Some("2.1.7"), Some("2.1.6"), &[])).unwrap()["kind"], "download");
        // 404 仍然落 catalog（两条路径一致，不是各说各话）。
        assert_eq!(classify("官方商店请求失败（HTTP 404）").0, "catalog");
    }

    // t18 F3：结构化失败也要落 recent.log（wrap 见到结构化会早退跳过自己的 log）。
    #[test]
    fn a_structured_download_failure_reaches_recent_log_once() {
        let data = std::env::temp_dir().join(format!("fish-failure-log-{}-{}", std::process::id(), crate::now_ms()));
        std::fs::create_dir_all(&data).unwrap();
        let failures = vec![
            row("npm 官方源", "2.1.7", "https://registry.npmjs.org/@fish-under-sea/fish-dock-plugin-magpie/-/fish-dock-plugin-magpie-2.1.7.tgz", Some(404), "官方商店请求失败（HTTP 404）"),
            row("npm 镜像", "2.1.7", "https://registry.npmmirror.com/@fish-under-sea/fish-dock-plugin-magpie/-/fish-dock-plugin-magpie-2.1.7.tgz", Some(404), "官方商店请求失败（HTTP 404）"),
            row("GitHub 发行包", "2.1.7", "https://github.com/Fish-under-sea/FISH-DOCK-Plugins/releases/download/v2.1.7/magpie.fish-plugin", Some(404), "官方商店请求失败（HTTP 404）"),
        ];
        let json = download_failed(&failures, Some("2.1.7"), Some("2.1.6"), &[]);
        log_structured(&data, "download", &json);
        // 走一次 plugin_api 的真实顺序（先 log_structured 再 wrap）：结构化串不重复记。
        let _ = wrap("download", json.clone(), &data);
        let logged = std::fs::read_to_string(data.join("recent.log")).unwrap_or_default();
        assert!(logged.contains("HTTP 404"), "recent.log 要能读到 404：{logged}");
        assert!(logged.contains("registry.npmmirror.com"), "recent.log 要能读到逐源 url：{logged}");
        assert!(logged.contains("cdn") || logged.contains("magpie.fish-plugin"), "github 那一行也要在：{logged}");
        assert_eq!(logged.matches("[plugin] download:").count(), 1, "同一份失败不许记两遍：{logged}");
        // 非结构化串不进这条通道（由 wrap 自己记）。
        log_structured(&data, "download", "短错误");
        assert_eq!(std::fs::read_to_string(data.join("recent.log")).unwrap_or_default().matches("[plugin] download:").count(), 1);
        let _ = std::fs::remove_dir_all(&data);
    }

    // 五词阶段与状态码解析是同一处口径，Attempt 与来源行都靠它。
    #[test]
    fn phases_and_status_codes_come_from_one_reading_of_the_error_text() {
        for (raw, want) in [
            ("官方商店请求失败（HTTP 404）", "HTTP"),
            ("官方商店连接失败，请检查网络后重试", "连接"),
            ("插件下载跳转不属于官方发行资源服务", "跳转"),
            ("npm 包里的插件归档摘要与官方目录不一致", "摘要"),
            ("插件包 SHA-256 校验失败", "摘要"),
            ("插件资源校验失败：backend.mjs", "校验"),
            ("插件下载中断或超过大小限制", "HTTP"),
        ] {
            assert_eq!(phase(raw), want, "{raw}");
        }
        assert_eq!(http_status("官方商店请求失败（HTTP 404）"), Some(404));
        assert_eq!(http_status("官方商店请求失败（HTTP 503）"), Some(503));
        assert_eq!(http_status("官方商店连接失败，请检查网络后重试"), None);
        assert_eq!(source_short("官方商店请求失败（HTTP 404）"), "版本制品不存在（404）");
        assert_eq!(source_short("官方商店请求失败（HTTP 410）"), "版本制品已下架（410）");
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
            // #68：制品不存在属于目录问题，不再落到 network（那是「查代理」的错误指路）。
            ("版本制品不存在（404）", "catalog"),
            ("官方商店请求失败（HTTP 404）", "catalog"),
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
