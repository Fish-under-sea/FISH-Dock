//! 资源获取的单一入口（2.0.4 T-F，方案 §7.1–§7.3，审计 A05/A06/A07）。
//!
//! 这里的每一条取字节的路都必须**自己**带上「原地址 + 代理回退 + 镜像候选」：
//! 没有镜像映射时也走原地址，并且仍按 `proxy::web_ways` 逐路试，绝不退成裸请求
//! （captain 复核 §13.4）。因此：
//!
//! - [`picks`] 只回答「这个地址有哪些镜像映射」，空 vec 表示没有镜像，**不是**
//!   「不用下载」；[`candidates`] 才是一次下载真正要试的完整顺序（原地址在内）。
//! - 「Gitee 附件」这一条映射（附加条目 A / S2）**只在 `url.gitee_repo` 配了、
//!   且目标资产属于同一个项目**（GitHub 的 `owner/repo` 与配置的 Gitee 仓库同名）
//!   时才产生：默认空配置下候选表逐条与 2.0.4 现状一致。插件仓是**另一个 repo**，
//!   因此 `.fish-plugin` 永远不会被改指到 Dock 的 Gitee 仓——插件三源仍是
//!   npm 官方 / npmmirror / GitHub Release（工作区 `AGENTS.md` §八硬约束）。
//!   Gitee 侧目前只有 tags、没有发行版本（`releases` API 返回 `[]`，
//!   `releases/download/<tag>/<file>` 实测 404），所以这条映射**配了也常落空**，
//!   靠 [`candidates`] 的失败回落兜住；实测证据见
//!   `docs/domestic-mirror-matrix-2026-11-04.md`。
//! - 缺口如实标注，不伪造镜像：pet 图库（`codex-pets.net`，实测 DNS 污染级不可达）、
//!   Windhawk、LHM 都**没有已确认的免费国内镜像**，它们只有原地址 + 系统代理这一路；
//!   `url.pet_mirror` / `url.gh_proxy` 默认空就是空，不拿第三方站点冒充官方镜像，
//!   也不为此放宽 CSP 或下载域名白名单（`secure_url` 的判据不变）。
//! - 一次调用只有一个 [`Duration`] 总预算，跨全部候选与全部代理路共用，换源不
//!   重置（方案 §7.3.2）；每次请求前都按剩余预算夹一次连接/响应预算。
//! - 重定向手动跟（`max_redirects(0)`），逐跳要求标准 HTTPS，跳数取自
//!   `config::plugin_redirect_hops()`。
//! - [`fetch_to_file`] 流式写**请求唯一的临时文件**，成功才 `rename` 到调用方给的
//!   路径；失败清掉临时文件，不留半个包。这里不校验摘要、也不决定「这是不是最终
//!   受管文件」——调用方把 `out` 指到自己的私有暂存路径，摘要/长度核过之后再发布
//!   到最终位置（captain 复核 §13.4）。
//! - 错误聚合保留每个候选的 label 与原因，不只剩最后一条。

use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// 一个候选地址：给人看的来源名，和真正要请求的地址。
#[derive(Clone, Debug, PartialEq)]
pub struct Pick {
    pub label: &'static str,
    pub url: String,
}

/// 一条试过的候选与它为什么没成。
#[derive(Clone, Debug, PartialEq)]
pub struct Attempt {
    pub label: &'static str,
    pub url: String,
    pub error: String,
}

/// 失败的大类。取「最可诊断」的那一类报给上层：配置 / 摘要 > 限额 > HTTP >
/// 重定向 > 离线 > IO > 地址格式。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(dead_code)]
pub enum FailKind {
    Config,
    Url,
    Http,
    Redirect,
    SizeLimit,
    Digest,
    Offline,
    Io,
}

impl FailKind {
    fn rank(self) -> u8 {
        match self {
            FailKind::Config => 8,
            FailKind::Digest => 7,
            FailKind::SizeLimit => 6,
            FailKind::Http => 5,
            FailKind::Redirect => 4,
            FailKind::Offline => 3,
            FailKind::Io => 2,
            FailKind::Url => 1,
        }
    }

    fn slug(self) -> &'static str {
        match self {
            FailKind::Config => "config",
            FailKind::Url => "url",
            FailKind::Http => "http",
            FailKind::Redirect => "redirect",
            FailKind::SizeLimit => "size",
            FailKind::Digest => "digest",
            FailKind::Offline => "offline",
            FailKind::Io => "io",
        }
    }

    fn worse(self, other: FailKind) -> FailKind {
        if other.rank() > self.rank() {
            other
        } else {
            self
        }
    }
}

/// 一次取字节全都失败：每个候选的原因都在 `tried` 里，`timeout` 表示总预算耗尽。
#[derive(Clone, Debug, PartialEq)]
pub struct Fail {
    pub kind: FailKind,
    pub timeout: bool,
    pub tried: Vec<Attempt>,
}

impl Fail {
    /// 只要一条结论也用得上的一句话（每候选的名字与原因都在）。
    pub fn reason(&self) -> String {
        self.tried.iter().map(|a| format!("{}（{}）：{}", a.label, a.url, a.error)).collect::<Vec<_>>().join("；")
    }
}

impl std::fmt::Display for Fail {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let head = if self.timeout { "取资源失败（总预算已用完）" } else { "取资源失败" };
        write!(f, "{head}[{}]：{}", self.kind.slug(), self.reason())
    }
}

/// 落地的一个文件：来源名、实际地址、字节数。
#[derive(Clone, Debug, PartialEq)]
pub struct Source {
    pub label: &'static str,
    pub url: String,
    pub bytes: u64,
}

/// `https://raw.githubusercontent.com/<o>/<r>/<ref>/<path>` 拆成 (o, r, ref, path)。
fn raw_parts(url: &str) -> Option<(&str, &str, &str, &str)> {
    let rest = url.strip_prefix("https://raw.githubusercontent.com/")?;
    let mut parts = rest.splitn(4, '/');
    let owner = parts.next()?;
    let repo = parts.next()?;
    let git_ref = parts.next()?;
    let path = parts.next()?;
    (!owner.is_empty() && !repo.is_empty() && !git_ref.is_empty() && !path.is_empty()).then_some((owner, repo, git_ref, path))
}

/// `https://github.com/<o>/<r>/releases/download/<tag>/<file>` 的判定（只认这一形态）。
fn release_asset(url: &str) -> bool {
    let Some(rest) = url.strip_prefix("https://github.com/") else { return false };
    let parts: Vec<&str> = rest.split('/').collect();
    parts.len() >= 6 && parts.get(2) == Some(&"releases") && parts.get(3) == Some(&"download") && !parts[4].is_empty() && !parts[5].is_empty()
}

/// 前缀式加速：`{前缀}/{原完整地址}`（ghproxy 一类服务的通行形态）。
fn prefixed(prefix: &str, url: &str) -> String {
    format!("{}/{}", prefix.trim_end_matches('/'), url)
}

/// 配置里的 Gitee 仓库拆成 `(owner, repo)`：只认 `https://gitee.com/<owner>/<repo>`
/// 这一形态（尾部斜杠可有）。拆不出来就是「没配好」＝不产生候选，不猜、不兜底。
fn gitee_parts(configured: &str) -> Option<(&str, &str)> {
    let rest = configured.strip_prefix("https://gitee.com/")?;
    let mut parts = rest.trim_end_matches('/').splitn(3, '/');
    let owner = parts.next()?;
    let repo = parts.next()?;
    // 第三段还有内容（子路径）就不是仓库根地址，不当配置用。
    if parts.next().is_some() {
        return None;
    }
    (!owner.is_empty() && !repo.is_empty()).then_some((owner, repo))
}

/// GitHub Release 资产 → **同一个项目**在 Gitee 上的同名附件（附加条目 A / S2）。
///
/// 只在 `url.gitee_repo` 配了、且 `owner/repo` 与它同名（大小写不敏感，Gitee 会
/// 规范化仓名）时给出 `<gitee_repo>/releases/download/<tag>/<file>`；其余一律 None。
/// 三条硬约束：
///
/// 1. **不改指别的项目**：插件仓（`FISH-DOCK-Plugins`）与 Dock 仓不是同一个 repo，
///    所以插件包的三源不会多出 Gitee 这一路；
/// 2. **不放宽地址判据**：产物仍是标准 HTTPS，`secure_url` / `updates::allowed`
///    对它的判定与对原地址一致（前缀就是用户自己配的 `gitee_repo`）；
/// 3. **不伪造摘要**：候选只是「多试一条地址」，字节仍由调用方按官方摘要核验
///    （`updates.rs` 的 `digest` 比对、`plugin_store` 的 catalog `sha256`）。
fn gitee_asset(url: &str) -> Option<String> {
    let configured = crate::config::gitee_repo();
    let (owner, repo) = gitee_parts(&configured)?;
    let rest = url.strip_prefix("https://github.com/")?;
    let mut parts = rest.splitn(4, '/');
    let (got_owner, got_repo, releases, tail) = (parts.next()?, parts.next()?, parts.next()?, parts.next()?);
    if !got_owner.eq_ignore_ascii_case(owner) || !got_repo.eq_ignore_ascii_case(repo) || releases != "releases" {
        return None;
    }
    let after = tail.strip_prefix("download/")?;
    let (tag, file) = after.split_once('/')?;
    // 只认 `<tag>/<file>` 这一层：嵌套路径不是 Release 资产的形状。
    if tag.is_empty() || file.is_empty() || file.contains('/') {
        return None;
    }
    Some(format!("{}/releases/download/{tag}/{file}", configured.trim_end_matches('/')))
}

/// 这个地址的镜像候选：**只含有明确映射的**，不含原地址（原地址在 [`candidates`]）。
/// 空 vec 表示没有镜像映射——调用方仍必须走原地址。
pub fn picks(url: &str) -> Vec<Pick> {
    let mut out: Vec<Pick> = Vec::new();
    if let Some((owner, repo, git_ref, path)) = raw_parts(url) {
        // jsDelivr 的 gh 形态：结构化改写，不是前缀。
        out.push(Pick { label: "jsDelivr", url: format!("https://cdn.jsdelivr.net/gh/{owner}/{repo}@{git_ref}/{path}") });
        let prefix = crate::config::raw_mirror();
        if !prefix.is_empty() {
            out.push(Pick { label: "raw 镜像", url: prefixed(&prefix, url) });
        }
    }
    if let Some(rest) = url.strip_prefix("https://registry.npmjs.org/") {
        let mirror = crate::config::plugin_npm_mirror();
        if !mirror.is_empty() {
            out.push(Pick { label: "npm 镜像", url: format!("{}/{rest}", mirror.trim_end_matches('/')) });
        }
    }
    if release_asset(url) {
        let prefix = crate::config::gh_proxy();
        if !prefix.is_empty() {
            out.push(Pick { label: "Release 加速", url: prefixed(&prefix, url) });
        }
    }
    // 附加条目 A / S2：Gitee 附件候选。**追加在最后**，既有候选的相对顺序不动；
    // 未配置 `url.gitee_repo` 时 `gitee_asset` 恒为 None，默认候选表与现状逐条一致。
    if let Some(gitee) = gitee_asset(url) {
        out.push(Pick { label: "Gitee 附件", url: gitee });
    }
    if let Some(rest) = url.strip_prefix(crate::config::PET_SITE_DEFAULT) {
        let prefix = crate::config::pet_mirror();
        if !prefix.is_empty() {
            out.push(Pick { label: "宠物镜像", url: format!("{}{rest}", prefix.trim_end_matches('/')) });
        }
    }
    out
}

/// 一次下载真正要试的顺序：原地址 + 它的镜像，按 `mirror.prefer` 排。
///
/// `auto`：原地址优先、镜像兜底（现状）；`domestic`：镜像优先、原地址兜底；
/// `overseas`：原地址优先（与 auto 相同，只是明确不把镜像提前）。
pub fn candidates(url: &str) -> Vec<Pick> {
    let origin = Pick { label: "原地址", url: url.to_string() };
    let mirrors = picks(url);
    let mut all = Vec::with_capacity(mirrors.len() + 1);
    match crate::config::mirror_prefer() {
        "domestic" if !mirrors.is_empty() => {
            all.extend(mirrors);
            all.push(origin);
        }
        _ => {
            all.push(origin);
            all.extend(mirrors);
        }
    }
    all
}

// --- 地址与重定向 ---------------------------------------------------------------

/// 只接受标准的 HTTPS 地址：无身份信息、无片段（与 plugin_store 的判定同款）。
pub fn secure_url(value: &str) -> Result<tauri::Url, String> {
    let url = tauri::Url::parse(value).map_err(|_| "资源地址无效".to_string())?;
    if url.scheme() != "https" || !url.username().is_empty() || url.password().is_some() || url.fragment().is_some() {
        return Err("资源必须是标准 HTTPS 地址（不带账号、密码或片段）".into());
    }
    Ok(url)
}

/// 跳转目标：`Location` 拼成完整地址；只去 https（测试里放行 http，供纯函数夹具）。
pub fn redirected(from: &str, to: &str) -> Option<String> {
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
    (url.starts_with("https://") || (cfg!(test) && url.starts_with("http://"))).then_some(url)
}

/// ureq 错误 → 一句人话，保留类别（DNS / TLS / 超时 / 拒绝 / 重置分开说）。
/// 与 plugin_thirdparty 原有的 `npm_plain_error` 同款；那边改成委托这里，避免两套。
pub fn net_words(e: &ureq::Error) -> String {
    match e {
        ureq::Error::HostNotFound => "域名解析失败（DNS）".into(),
        ureq::Error::Timeout(_) => "连接或响应超时".into(),
        ureq::Error::Tls(message) => tls_words(message),
        ureq::Error::StatusCode(status) => format!("HTTP {status}"),
        ureq::Error::Io(io) => {
            let kind = io.kind();
            let text = io.to_string();
            let lower = text.to_ascii_lowercase();
            if kind == std::io::ErrorKind::InvalidData
                || lower.contains("invalidmessage")
                || lower.contains("invalidcontenttype")
                || lower.contains("corrupt message")
                || lower.contains("tls")
                || lower.contains("certificate")
                || lower.contains("handshake")
            {
                tls_words(&text)
            } else if kind == std::io::ErrorKind::UnexpectedEof {
                "连接被提前关闭（可能是 DNS 解析失败、代理拦截或对方拒绝）".into()
            } else if kind == std::io::ErrorKind::TimedOut {
                "连接或响应超时".into()
            } else if kind == std::io::ErrorKind::ConnectionRefused {
                "连接被拒绝".into()
            } else if kind == std::io::ErrorKind::ConnectionReset {
                "连接被重置".into()
            } else {
                format!("网络 IO 失败（{io}）")
            }
        }
        other => {
            let text = other.to_string();
            let lower = text.to_ascii_lowercase();
            if lower.contains("tls") || lower.contains("certificate") || lower.contains("handshake") || lower.contains("ssl") || lower.contains("invalidcontenttype") || lower.contains("corrupt message") {
                tls_words(&text)
            } else {
                format!("连接失败（{text}）")
            }
        }
    }
}

fn tls_words(detail: &str) -> String {
    format!("TLS 握手失败（可能：代理拦截 / 端口不是 https / 证书不被信任）：{detail}")
}

/// 剩余预算（已耗尽就是 None，让调用方直接判超时，不再发新请求）。
fn left(deadline: Instant) -> Option<Duration> {
    let now = Instant::now();
    if now >= deadline {
        None
    } else {
        Some(deadline.saturating_duration_since(now))
    }
}

/// 把一次 HTTP 的失败折算成大类（用于聚合时挑最可诊断的那一类）。
fn kind_of(error: &str) -> FailKind {
    let lower = error.to_ascii_lowercase();
    if lower.contains("超时") || lower.contains("timeout") {
        FailKind::Offline
    } else if lower.contains("超过大小") || lower.contains("size") {
        FailKind::SizeLimit
    } else if lower.contains("http ") {
        FailKind::Http
    } else if lower.contains("跳转") {
        FailKind::Redirect
    } else if lower.contains("tls") || lower.contains("dns") || lower.contains("连接") || lower.contains("代理") {
        FailKind::Offline
    } else {
        FailKind::Io
    }
}

// --- 一次「候选 × 代理路」的请求 ----------------------------------------------------

/// 一个候选地址的一次获取：各代理路依次试；重定向手动跟、逐跳校验。
/// 返回读到的字节，或这一候选的人话失败原因。
fn get_once(url: &str, deadline: Instant, limit: u64) -> Result<Vec<u8>, (FailKind, String)> {
    if secure_url(url).is_err() {
        return Err((FailKind::Url, "资源地址无效或不是 HTTPS".into()));
    }
    get_with_ways(url, deadline, limit, |mut response| {
        response.body_mut().with_config().limit(limit).read_to_vec().map_err(|error| format!("响应读取失败或超过大小限制（{error}）"))
    })
}

/// 一个候选地址的一次流式落地：`sink` 决定把正文写到哪里（测试据此不碰磁盘契约）。
fn get_to_file(url: &str, deadline: Instant, limit: u64, out: &Path, on_bytes: &mut dyn FnMut(u64, Option<u64>)) -> Result<u64, (FailKind, String)> {
    if secure_url(url).is_err() {
        return Err((FailKind::Url, "资源地址无效或不是 HTTPS".into()));
    }
    let temp = temp_beside(out);
    let written = get_with_ways(url, deadline, limit, |mut response| {
        let total = response.body().content_length();
        on_bytes(0, total);
        let mut reader = response.body_mut().with_config().limit(limit).reader();
        let mut file = std::fs::File::create(&temp).map_err(|e| format!("写入失败：{e}"))?;
        let mut got = 0u64;
        let mut buf = vec![0u8; 64 * 1024];
        loop {
            let n = reader.read(&mut buf).map_err(|e| format!("下载中断：{e}"))?;
            if n == 0 {
                break;
            }
            got = got.saturating_add(n as u64);
            if got > limit {
                return Err(format!("超过大小限制（{limit} 字节）"));
            }
            std::io::Write::write_all(&mut file, &buf[..n]).map_err(|e| format!("写入失败：{e}"))?;
            on_bytes(got, total);
        }
        std::io::Write::flush(&mut file).map_err(|e| format!("写入失败：{e}"))?;
        Ok(got)
    });
    match written {
        Ok(bytes) => {
            if let Err(error) = std::fs::rename(&temp, out) {
                let _ = std::fs::remove_file(&temp);
                return Err((FailKind::Io, format!("落盘失败：{error}")));
            }
            Ok(bytes)
        }
        Err(error) => {
            let _ = std::fs::remove_file(&temp);
            Err(error)
        }
    }
}

/// 候选地址的公共骨架：代理路 × 手动重定向；每个 200 的正文交给 `take`。
fn get_with_ways<R>(url: &str, deadline: Instant, _limit: u64, mut take: impl FnMut(ureq::http::Response<ureq::Body>) -> Result<R, String>) -> Result<R, (FailKind, String)> {
    let start = tauri::Url::parse(url).map_err(|_| (FailKind::Url, "资源地址无效".to_string()))?;
    let mut failures: Vec<String> = Vec::new();
    let mut kind = FailKind::Url;
    for way in crate::proxy::web_ways(url) {
        let Some(remaining) = left(deadline) else {
            return Err((FailKind::Offline, "总预算已用完".into()));
        };
        let Some(agent) = way.agent_with(remaining, |config| config.max_redirects(0).http_status_as_error(false)) else {
            failures.push(format!("{}: 代理不可用", way.name()));
            continue;
        };
        let mut at = start.clone();
        let mut hops = 0usize;
        loop {
            if left(deadline).is_none() {
                return Err((FailKind::Offline, "总预算已用完".into()));
            }
            let response = match agent.get(at.as_str()).header("user-agent", "FISH-Dock").header("accept", "*/*").call() {
                Ok(response) => response,
                Err(error) => {
                    let words = net_words(&error);
                    kind = kind.worse(kind_of(&words));
                    failures.push(format!("{}: {words}", way.name()));
                    break;
                }
            };
            let status = response.status().as_u16();
            if (300..400).contains(&status) {
                if hops >= crate::config::plugin_redirect_hops() {
                    return Err((FailKind::Redirect, format!("跳转次数超过限制（{status}）")));
                }
                let Some(next) = response.headers().get("location").and_then(|v| v.to_str().ok()).and_then(|l| redirected(at.as_str(), l)) else {
                    return Err((FailKind::Redirect, format!("跳转地址无效或不是 HTTPS（{status}）")));
                };
                if secure_url(&next).is_err() {
                    return Err((FailKind::Redirect, "跳转目标不是标准 HTTPS 地址".into()));
                }
                at = tauri::Url::parse(&next).map_err(|_| (FailKind::Redirect, "跳转地址无效".to_string()))?;
                hops += 1;
                continue;
            }
            if status == 404 {
                return Err((FailKind::Http, "HTTP 404（不存在）".into()));
            }
            if status != 200 {
                return Err((FailKind::Http, format!("HTTP {status}")));
            }
            return take(response).map_err(|error| (kind_of(&error), error));
        }
    }
    Err((kind, if failures.is_empty() { "没有可用的网络路径".into() } else { failures.join("；") }))
}

/// `out` 同目录下的唯一临时名：`.<名字>.<pid>-<计数>.part`。
fn temp_beside(out: &Path) -> PathBuf {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let n = NEXT.fetch_add(1, Ordering::Relaxed);
    let name = out.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "download".into());
    out.with_file_name(format!(".{name}.{}-{n}.part", std::process::id()))
}

// --- 候选循环（注入版与真实版共用同一形状） ------------------------------------------

fn walk_bytes<F>(url: &str, budget: Duration, limit: u64, mut fetch: F) -> Result<Vec<u8>, Fail>
where
    F: FnMut(&Pick, Duration, u64) -> Result<Vec<u8>, (FailKind, String)>,
{
    let deadline = Instant::now() + budget;
    let mut tried = Vec::new();
    let mut kind = FailKind::Url;
    let mut timeout = false;
    for pick in candidates(url) {
        let Some(remaining) = left(deadline) else {
            timeout = true;
            tried.push(Attempt { label: pick.label, url: pick.url, error: "总预算已用完".into() });
            break;
        };
        match fetch(&pick, remaining, limit) {
            Ok(bytes) => return Ok(bytes),
            Err((error_kind, error)) => {
                kind = kind.worse(error_kind);
                tried.push(Attempt { label: pick.label, url: pick.url, error });
            }
        }
    }
    // 候选都试完时若时间也刚好用完，同样算「总预算耗尽」（不是只有「没来得及开始」）。
    Err(Fail { kind, timeout: timeout || Instant::now() >= deadline, tried })
}

fn walk_file<F>(url: &str, budget: Duration, limit: u64, _out: &Path, mut fetch: F) -> Result<Source, Fail>
where
    F: FnMut(&Pick, Duration, u64) -> Result<u64, (FailKind, String)>,
{
    let deadline = Instant::now() + budget;
    let mut tried = Vec::new();
    let mut kind = FailKind::Url;
    let mut timeout = false;
    for pick in candidates(url) {
        let Some(remaining) = left(deadline) else {
            timeout = true;
            tried.push(Attempt { label: pick.label, url: pick.url, error: "总预算已用完".into() });
            break;
        };
        match fetch(&pick, remaining, limit) {
            Ok(bytes) => return Ok(Source { label: pick.label, url: pick.url, bytes }),
            Err((error_kind, error)) => {
                kind = kind.worse(error_kind);
                tried.push(Attempt { label: pick.label, url: pick.url, error });
            }
        }
    }
    // 候选都试完时若时间也刚好用完，同样算「总预算耗尽」（不是只有「没来得及开始」）。
    Err(Fail { kind, timeout: timeout || Instant::now() >= deadline, tried })
}

// --- 对外：注入版 + 真实版 --------------------------------------------------------

/// 按 [`candidates`] 的顺序取一份小东西（元数据、JSON、图片）：第一个成功的就停。
/// `fetch` 是注入点（`(url, limit) -> Result<bytes, reason>`），测试不碰真网。
pub fn fetch_bytes_with<F>(url: &str, budget: Duration, limit: u64, mut fetch: F) -> Result<Vec<u8>, Fail>
where
    F: FnMut(&str, u64) -> Result<Vec<u8>, String>,
{
    walk_bytes(url, budget, limit, |pick, _remaining, limit| fetch(&pick.url, limit).map_err(|error| (kind_of(&error), error)))
}

/// 真实取一份小东西（元数据、图片、目录 JSON）。总预算跨候选共用。
pub fn fetch_bytes(url: &str, budget: Duration, limit: u64) -> Result<Vec<u8>, Fail> {
    walk_bytes(url, budget, limit, |pick, remaining, limit| get_once(&pick.url, Instant::now() + remaining, limit))
}

/// 流式落到调用方的私有暂存路径；摘要与「这是不是最终受管文件」由调用方决定。
#[cfg(test)]
pub fn fetch_to_file_with<F>(url: &str, budget: Duration, limit: u64, out: &Path, mut fetch: F) -> Result<Source, Fail>
where
    F: FnMut(&str, u64, &Path) -> Result<u64, String>,
{
    walk_file(url, budget, limit, out, |pick, _remaining, limit| fetch(&pick.url, limit, out).map_err(|error| (kind_of(&error), error)))
}

/// 真实流式下载（大包：更新包、LHM、PawnIO）。`out` 必须是调用方的私有暂存路径。
pub fn fetch_to_file(url: &str, budget: Duration, limit: u64, out: &Path) -> Result<Source, Fail> {
    if let Some(parent) = out.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    walk_file(url, budget, limit, out, |pick, remaining, limit| {
        get_to_file(&pick.url, Instant::now() + remaining, limit, out, &mut |_, _| {})
    })
}

/// 同上，但按已写字节回报进度（`get, total`；total 未知是 None）。LHM 的 zip 要在
/// 设置页显示 MB 进度，又不许为它另写一套网络实现。
pub fn fetch_to_file_with_progress<F>(url: &str, budget: Duration, limit: u64, out: &Path, mut on_bytes: F) -> Result<Source, Fail>
where
    F: FnMut(u64, Option<u64>),
{
    if let Some(parent) = out.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    walk_file(url, budget, limit, out, |pick, remaining, limit| {
        get_to_file(&pick.url, Instant::now() + remaining, limit, out, &mut on_bytes)
    })
}

/// 镜像类配置（`FISH_DOCK_GITEE_REPO` / `FISH_DOCK_GH_PROXY` / `FISH_DOCK_MIRROR_PREFER` …）
/// 的测试互斥锁。`sources` 与 `updates` 的用例都要临时改这些环境变量，跨模块共用
/// 一把，避免并行跑时一个用例读到另一个用例设的值（附加条目 A / S2 的新用例）。
#[cfg(test)]
pub(crate) static MIRROR_ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[cfg(test)]
mod tests {
    use super::*;

    fn env_lock() -> std::sync::MutexGuard<'static, ()> {
        MIRROR_ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// 临时改一个环境变量，**panic 也要还原**（Drop 守卫）。
    /// 早先的写法在闭包里断言失败时会把改过的值留在进程环境里，
    /// 后面并行/串行跑到的用例就读到脏值（本批实测踩过：一个用例把
    /// `FISH_DOCK_MIRROR_PREFER=domestic` 泄漏出去，连累另外四条用例失败）。
    struct EnvGuard {
        key: String,
        old: Option<std::ffi::OsString>,
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            match &self.old {
                Some(value) => std::env::set_var(&self.key, value),
                None => std::env::remove_var(&self.key),
            }
        }
    }

    fn with_env<T>(key: &str, value: &str, f: impl FnOnce() -> T) -> T {
        let _guard = EnvGuard { key: key.to_string(), old: std::env::var_os(key) };
        std::env::set_var(key, value);
        f()
    }

    /// 与 [`with_env`] 相反：这一段时间里该键**不存在**（模拟「默认没配」）。
    fn without_env<T>(key: &str, f: impl FnOnce() -> T) -> T {
        let _guard = EnvGuard { key: key.to_string(), old: std::env::var_os(key) };
        std::env::remove_var(key);
        f()
    }

    // 没有镜像映射就不许伪造镜像：picks 只回答「有明确映射的候选」。
    #[test]
    fn picks_only_maps_what_it_knows() {
        // 与环境相关的用例（with_env 设 FISH_DOCK_GH_PROXY 等）共用一把锁，避免并行时串扰。
        let _g = env_lock();
        assert!(picks("https://example.com/whatever.json").is_empty(), "未知地址无镜像");
        let raw = picks("https://raw.githubusercontent.com/o/r/main/a/b.json");
        assert_eq!(raw.first().map(|p| p.label), Some("jsDelivr"));
        assert_eq!(raw[0].url, "https://cdn.jsdelivr.net/gh/o/r@main/a/b.json");
        // 默认 raw 镜像为空：只剩 jsDelivr，不编造第三家。
        assert_eq!(raw.len(), 1, "默认不加 raw 前缀候选：{raw:?}");
        let npm = picks("https://registry.npmjs.org/@scope/name");
        assert_eq!(npm[0].url, "https://registry.npmmirror.com/@scope/name");
        // 前缀类默认空：Release 与宠物都没有候选。
        assert!(picks("https://github.com/o/r/releases/download/v1/x.exe").is_empty());
        assert!(picks("https://codex-pets.net/api/pets").is_empty());
    }

    #[test]
    fn mirrors_appear_when_configured_and_prefer_reorders_without_dropping_origin() {
        let _g = env_lock();
        assert_eq!(crate::config::mirror_prefer(), "auto");
        let none = candidates("https://example.com/x");
        assert_eq!(none.len(), 1, "无镜像时只有原地址");
        assert_eq!(none[0].label, "原地址");
        with_env("FISH_DOCK_GH_PROXY", "https://gh.example/", || {
            let auto = candidates("https://github.com/o/r/releases/download/v1/x.exe");
            assert_eq!(auto[0].label, "原地址");
            assert_eq!(auto[1].label, "Release 加速");
            assert_eq!(auto[1].url, "https://gh.example/https://github.com/o/r/releases/download/v1/x.exe");
            // domestic：镜像在前，但原地址仍在（不是删掉回退）。
            let domestic = with_env("FISH_DOCK_MIRROR_PREFER", "domestic", || candidates("https://github.com/o/r/releases/download/v1/x.exe"));
            assert_eq!(domestic[0].label, "Release 加速");
            assert_eq!(domestic.last().map(|p| p.label), Some("原地址"));
            // overseas：原地址在前。
            let overseas = with_env("FISH_DOCK_MIRROR_PREFER", "overseas", || candidates("https://github.com/o/r/releases/download/v1/x.exe"));
            assert_eq!(overseas[0].label, "原地址");
        });
    }

    // 关键契约（captain §13.4）：没有镜像也要在 sources 内部走原地址本身。
    #[test]
    fn the_origin_is_always_tried_even_without_mirrors() {
        let seen = std::cell::RefCell::new(Vec::new());
        let got = fetch_bytes_with("https://example.com/data.json", Duration::from_secs(5), 64, |url, limit| {
            seen.borrow_mut().push((url.to_string(), limit));
            Err("HTTP 500".to_string())
        });
        assert!(got.is_err());
        assert_eq!(seen.borrow().len(), 1, "无镜像时只走原地址一次");
        assert_eq!(seen.borrow()[0].0, "https://example.com/data.json");
        assert_eq!(seen.borrow()[0].1, 64, "限额透传");
        let fail = got.err().unwrap();
        assert_eq!(fail.kind, FailKind::Http, "HTTP 类失败应报 HTTP");
        assert!(!fail.timeout);
        assert!(fail.reason().contains("原地址"));
    }

    // 第一次失败第二次成功：换源不提前放弃。
    #[test]
    fn a_failed_mirror_falls_through_to_the_next() {
        let _g = env_lock();
        with_env("FISH_DOCK_GH_PROXY", "https://gh.example", || {
            let mut calls = 0;
            let got = fetch_bytes_with("https://github.com/o/r/releases/download/v1/x.exe", Duration::from_secs(5), 32, |url, _| {
                calls += 1;
                if url.starts_with("https://github.com/") {
                    Err("连接被拒绝".to_string())
                } else {
                    assert_eq!(url, "https://gh.example/https://github.com/o/r/releases/download/v1/x.exe");
                    Ok(vec![1, 2, 3])
                }
            });
            assert_eq!(got.unwrap(), vec![1, 2, 3], "第二路（镜像）应接手");
            assert_eq!(calls, 2);
        });
    }

    // 两路都失败：错误里两边原因都在（方案 §7.5 首条验收）。
    #[test]
    fn both_failing_keeps_each_side_reason() {
        let _g = env_lock();
        with_env("FISH_DOCK_GH_PROXY", "https://gh.example", || {
            let fail = fetch_bytes_with("https://github.com/o/r/releases/download/v1/x.exe", Duration::from_secs(5), 32, |url, _| {
                Err(if url.starts_with("https://github.com/") { "HTTP 404（不存在）".to_string() } else { "连接被重置".to_string() })
            })
            .err()
            .unwrap();
            let reason = fail.reason();
            assert!(reason.contains("原地址") && reason.contains("404"), "{reason}");
            assert!(reason.contains("Release 加速") && reason.contains("连接被重置"), "{reason}");
            assert_eq!(fail.tried.len(), 2);
            assert_eq!(fail.kind, FailKind::Http, "404 比连接重置更可诊断");
        });
    }

    // 总预算是一次 deadline：候选轮换不重置（方案 §7.3.2）。
    #[test]
    fn the_budget_is_one_deadline_across_candidates() {
        let _g = env_lock();
        with_env("FISH_DOCK_GH_PROXY", "https://gh.example", || {
            let began = Instant::now();
            let fail = fetch_bytes_with("https://github.com/o/r/releases/download/v1/x.exe", Duration::from_millis(120), 32, |_, _| {
                std::thread::sleep(Duration::from_millis(90));
                Err("连接或响应超时".to_string())
            })
            .err()
            .unwrap();
            let spent = began.elapsed();
            // 两个候选各等 90 ms，总预算 120 ms：共享 deadline 时必然整体超时；
            // 若每个候选各自重置预算，则两次都「还没超时」而总数不会标 timeout。
            assert!(spent < Duration::from_millis(240), "总时长受一次预算约束：{spent:?}");
            assert!(fail.timeout, "预算耗尽要标超时");
            assert!(!fail.tried.is_empty(), "至少试过一个候选");
        });
    }

    // 流式下载：失败不留半个文件（真实实现清理临时文件）。
    #[test]
    fn a_failed_download_leaves_no_half_file() {
        let dir = std::env::temp_dir().join(format!("fish-sources-tmp-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let out = dir.join("pkg.bin");
        let fail = fetch_to_file_with("https://example.com/pkg.bin", Duration::from_secs(5), 16, &out, |_, _, _| Err("连接被重置".to_string())).err().unwrap();
        assert!(fail.reason().contains("连接被重置"));
        assert!(!out.exists(), "失败不得留下半个包");
        // 注入版本身不落盘；真实版另测（见下一条）。
        let _ = std::fs::remove_dir_all(&dir);
    }

    // 真实实现的临时名按请求唯一，且失败回收。
    #[test]
    fn temp_names_are_unique_and_cleaned_up() {
        let dir = std::env::temp_dir().join(format!("fish-sources-unique-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let out = dir.join("pkg.bin");
        let a = temp_beside(&out);
        let b = temp_beside(&out);
        assert_ne!(a, b, "两个并发下载的临时名不能相同");
        assert!(a.file_name().unwrap().to_string_lossy().ends_with(".part"));
        assert_eq!(a.parent(), Some(dir.as_path()), "临时文件与目标同目录（rename 才原子）");
        // 地址不合法时立即失败，且不创建任何文件。
        let fail = fetch_to_file("http://example.com/x", Duration::from_secs(1), 16, &out).err().unwrap();
        assert_eq!(fail.kind, FailKind::Url);
        assert!(!out.exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    // 真实实现：地址不合法时不必联网，错误类别是 url 且没有候选被悄悄跳过。
    #[test]
    fn an_invalid_address_fails_before_any_network() {
        let fail = fetch_bytes("http://example.com/x", Duration::from_secs(1), 32).err().unwrap();
        assert_eq!(fail.kind, FailKind::Url);
        assert_eq!(fail.tried.len(), 1);
        assert!(fail.tried[0].error.contains("HTTPS"));
    }

    // 重定向判定：Location 解析与 https 限制。
    #[test]
    fn location_parsing_is_strict() {
        assert_eq!(redirected("https://a.example/x/y", "/z"), Some("https://a.example/z".into()));
        assert_eq!(redirected("https://a.example/x", "//b.example/q"), Some("https://b.example/q".into()));
        assert_eq!(redirected("https://a.example/x", "https://c.example/y"), Some("https://c.example/y".into()));
        assert_eq!(redirected("https://a.example/x", "ftp://c.example/y"), None);
        assert_eq!(redirected("https://a.example/x", "relative/path"), None);
        assert_eq!(redirected("https://a.example/x", "javascript:alert(1)"), None);
    }

    #[test]
    fn only_standard_https_addresses_pass() {
        assert!(secure_url("https://example.com/a").is_ok());
        for bad in ["http://example.com/a", "https://user:pw@example.com/a", "https://example.com/a#frag", "not a url"] {
            assert!(secure_url(bad).is_err(), "{bad} 不该通过");
        }
    }

    // —— 附加条目 A / S2：Gitee 附件候选（配了才启用，且只映射同一个项目）——

    /// Dock 自己的发行资产（`updates::installer_names` 的真实形状）。
    const DOCK_ASSET: &str = "https://github.com/Fish-under-sea/FISH-Dock/releases/download/v2.0.4/FISH-Dock-Setup-2.0.4.exe";
    /// 配好之后期望的 Gitee 附件地址（`AGENTS.md` §八的双仓路径）。
    const DOCK_GITEE: &str = "https://gitee.com/Fish-under-sea/FISH-Dock";
    const DOCK_GITEE_ASSET: &str = "https://gitee.com/Fish-under-sea/FISH-Dock/releases/download/v2.0.4/FISH-Dock-Setup-2.0.4.exe";
    /// 插件仓的资产：另一个 repo，永远不该被改指到 Dock 的 Gitee 仓。
    const PLUGIN_ASSET: &str = "https://github.com/Fish-under-sea/FISH-DOCK-Plugins/releases/download/v2.2.0/feishu.fish-plugin";

    // 「默认启用、可显式关掉」（2.1.1 / #72：Gitee 侧已经有发行资产）：默认就带一条
    // Gitee 候选，`off` 之后一条都没有；auto/domestic/overseas 的排序语义与原地址
    // 兜底都不变。
    #[test]
    fn the_gitee_attachment_candidate_is_on_by_default_and_off_switches_it_away() {
        let _g = env_lock();
        with_env("FISH_DOCK_GITEE_REPO", "off", || {
            assert_eq!(crate::config::gitee_repo(), "", "off 表示关掉");
            assert!(picks(DOCK_ASSET).is_empty(), "关掉之后不许冒出 Gitee 候选：{:?}", picks(DOCK_ASSET));
            assert_eq!(candidates(DOCK_ASSET).iter().map(|p| p.label).collect::<Vec<_>>(), ["原地址"], "关掉之后只有原地址");
        });
        without_env("FISH_DOCK_GITEE_REPO", || {
            with_env("FISH_DOCK_MIRROR_PREFER", "auto", || {
                let got = picks(DOCK_ASSET);
                assert_eq!(got.len(), 1, "默认就带一条 Gitee 候选：{got:?}");
                assert_eq!(got[0].label, "Gitee 附件");
                assert_eq!(got[0].url, DOCK_GITEE_ASSET, "只换仓库前缀，tag 与文件名原样");
                assert_eq!(candidates(DOCK_ASSET).iter().map(|p| p.label).collect::<Vec<_>>(), ["原地址", "Gitee 附件"]);
                let domestic = with_env("FISH_DOCK_MIRROR_PREFER", "domestic", || candidates(DOCK_ASSET));
                assert_eq!(domestic.iter().map(|p| p.label).collect::<Vec<_>>(), ["Gitee 附件", "原地址"]);
                let overseas = with_env("FISH_DOCK_MIRROR_PREFER", "overseas", || candidates(DOCK_ASSET));
                assert_eq!(overseas.iter().map(|p| p.label).collect::<Vec<_>>(), ["原地址", "Gitee 附件"]);
                assert!(secure_url(DOCK_GITEE_ASSET).is_ok());
            });
        });
        with_env("FISH_DOCK_GITEE_REPO", DOCK_GITEE, || {
            // 显式钉住 auto：不依赖跑测试时的进程环境（别的用例曾泄漏过 domestic）。
            with_env("FISH_DOCK_MIRROR_PREFER", "auto", || {
                let got = picks(DOCK_ASSET);
                assert_eq!(got.len(), 1, "{got:?}");
                assert_eq!(got[0].label, "Gitee 附件");
                assert_eq!(got[0].url, DOCK_GITEE_ASSET, "只换仓库前缀，tag 与文件名原样");
                // auto：原地址优先、Gitee 兜底；domestic：Gitee 优先但原地址仍在（不是删掉回退）。
                assert_eq!(candidates(DOCK_ASSET).iter().map(|p| p.label).collect::<Vec<_>>(), ["原地址", "Gitee 附件"]);
                let domestic = with_env("FISH_DOCK_MIRROR_PREFER", "domestic", || candidates(DOCK_ASSET));
                assert_eq!(domestic.iter().map(|p| p.label).collect::<Vec<_>>(), ["Gitee 附件", "原地址"]);
                let overseas = with_env("FISH_DOCK_MIRROR_PREFER", "overseas", || candidates(DOCK_ASSET));
                assert_eq!(overseas.iter().map(|p| p.label).collect::<Vec<_>>(), ["原地址", "Gitee 附件"]);
                // 候选地址本身仍要过 `secure_url`（不放宽地址判据）。
                assert!(secure_url(DOCK_GITEE_ASSET).is_ok());
            });
        });
    }

    // 只映射同一个项目：插件仓、LHM、Windhawk、PawnIO 都是别的 repo，配了 Dock 的
    // Gitee 也不许多出这一路（`AGENTS.md` §八：插件三源仍是 npm 官方 / npmmirror / GitHub）。
    #[test]
    fn the_gitee_candidate_never_repoints_another_project() {
        let _g = env_lock();
        with_env("FISH_DOCK_GITEE_REPO", DOCK_GITEE, || {
            for other in [
                PLUGIN_ASSET,
                "https://github.com/LibreHardwareMonitor/LibreHardwareMonitor/releases/download/v0.9.6/LibreHardwareMonitor.zip",
                "https://github.com/ramensoftware/windhawk/releases/download/v1.7.3/windhawk_setup_offline.exe",
                "https://github.com/namazso/PawnIO.Setup/releases/download/2.2.0/PawnIO_setup.exe",
            ] {
                assert!(picks(other).is_empty(), "{other} 不该被映射到 Dock 的 Gitee 仓：{:?}", picks(other));
                assert_eq!(candidates(other).len(), 1, "{other} 仍只有原地址");
            }
            // 插件包的 npm 双源同理：Gitee 不参与（三源不变）。npm 地址本来就默认
            // 映射到 npmmirror，所以这里断言的是「没有多出 Gitee 附件这一条」。
            let npm = picks("https://registry.npmjs.org/@fish-under-sea/fish-dock-plugin-feishu/-/fish-dock-plugin-feishu-2.1.5.tgz");
            assert!(!npm.iter().any(|p| p.label == "Gitee 附件"), "{npm:?}");
            assert_eq!(npm.iter().map(|p| p.label).collect::<Vec<_>>(), ["npm 镜像"], "{npm:?}");
        });
    }

    // 配置写坏＝没配：不猜、不兜底、不产生候选；仓名大小写与尾部斜杠按 Gitee 的规范化容忍。
    #[test]
    fn a_malformed_gitee_repo_yields_no_candidate() {
        let _g = env_lock();
        // 注意：空串/空白**不再**是「没配」——2.1.1 起它们是「没覆盖，用默认仓库」。
        for bad in ["off ", "gitee.com/Fish-under-sea/FISH-Dock", "http://gitee.com/Fish-under-sea/FISH-Dock", "https://github.com/Fish-under-sea/FISH-Dock", "https://gitee.com/Fish-under-sea", "https://gitee.com/Fish-under-sea/FISH-Dock/extra", "https://gitee.com//FISH-Dock"] {
            with_env("FISH_DOCK_GITEE_REPO", bad, || assert!(picks(DOCK_ASSET).is_empty(), "{bad:?} 不该产生候选：{:?}", picks(DOCK_ASSET)));
        }
        // 非 Release 资产的形状（tag 页、API、缺文件名、嵌套路径）也不映射。
        for shape in ["https://github.com/Fish-under-sea/FISH-Dock/releases/tag/v2.0.4", "https://api.github.com/repos/Fish-under-sea/FISH-Dock/releases/latest", "https://github.com/Fish-under-sea/FISH-Dock/releases/download/v2.0.4", "https://github.com/Fish-under-sea/FISH-Dock/releases/download/v2.0.4/a/b.exe"] {
            with_env("FISH_DOCK_GITEE_REPO", DOCK_GITEE, || assert!(picks(shape).is_empty(), "{shape} 不该产生候选：{:?}", picks(shape)));
        }
        // Gitee 会规范化仓名（`AGENTS.md` §八），所以大小写不敏感、尾部斜杠可有。
        for good in ["https://gitee.com/fish-under-sea/FISH-Dock", "https://gitee.com/Fish-under-sea/FISH-Dock/", "https://gitee.com/FISH-UNDER-SEA/fish-dock"] {
            let got = with_env("FISH_DOCK_GITEE_REPO", good, || picks(DOCK_ASSET));
            assert_eq!(got.len(), 1, "{good:?} → {got:?}");
        }
    }

    // 基线状态（四个镜像键一个都没配、且 Gitee 通道显式关掉）候选表逐条锁定：
    // 没有任何一类资源在这里被悄悄换源（附加条目 A 的硬验收）。Gitee 的默认候选
    // 另由 `the_gitee_attachment_candidate_is_on_by_default_and_off_switches_it_away`
    // 与 `updates::the_gitee_candidate_stays_inside_the_same_allowance` 锁住。
    #[test]
    fn default_candidates_are_locked_with_the_gitee_channel_off() {
        let _g = env_lock();
        with_env("FISH_DOCK_GITEE_REPO", "off", || {
            without_env("FISH_DOCK_GH_PROXY", || {
                without_env("FISH_DOCK_RAW_MIRROR", || {
                    without_env("FISH_DOCK_PET_MIRROR", || {
                        without_env("FISH_DOCK_MIRROR_PREFER", || {
                            assert_eq!(crate::config::mirror_prefer(), "auto", "择源偏好默认 auto");
                            // 分两类锁：一类**没有任何镜像映射**（默认只有原地址，
                            // 新增的 Gitee 分支不许给它凭空造候选）；一类有既有映射
                            // （jsDelivr / npm 镜像，顺序与内容都按 2.0.4 现状）。
                            let plain: &[&str] = &[
                                "https://example.com/whatever.json",
                                DOCK_ASSET,
                                PLUGIN_ASSET,
                                "https://codex-pets.net/api/pets",
                                "https://github.com/LibreHardwareMonitor/LibreHardwareMonitor/releases/download/v0.9.6/LibreHardwareMonitor.zip",
                                "https://github.com/ramensoftware/windhawk/releases/download/v1.7.3/windhawk_setup_offline.exe",
                            ];
                            let mirrored: &[(&str, &[&str])] = &[
                                ("https://raw.githubusercontent.com/o/r/main/a/b.json", &["原地址", "jsDelivr"]),
                                ("https://registry.npmjs.org/@scope/name", &["原地址", "npm 镜像"]),
                            ];
                            for url in plain {
                                let got = candidates(url);
                                assert_eq!(got.iter().map(|p| p.label).collect::<Vec<_>>(), ["原地址"], "{url} 默认只该有原地址");
                                assert_eq!(got[0].url, *url, "{url} 默认第一条必须是原地址本身");
                            }
                            for (url, labels) in mirrored {
                                let got = candidates(url);
                                assert_eq!(got.iter().map(|p| p.label).collect::<Vec<_>>(), labels.to_vec(), "{url} 的默认候选被改动了");
                                assert_eq!(got[0].url, *url, "{url} 默认第一条必须是原地址本身");
                            }
                            // domestic 只改顺序、不增不减：没有映射的仍只有原地址（自然回落）；
                            // 有映射的镜像提前、原地址兜底（不是删掉回退）。
                            with_env("FISH_DOCK_MIRROR_PREFER", "domestic", || {
                                for url in plain {
                                    assert_eq!(candidates(url).iter().map(|p| p.label).collect::<Vec<_>>(), ["原地址"], "domestic 下 {url} 多了或少了候选");
                                }
                                for (url, labels) in mirrored {
                                    let got = candidates(url);
                                    assert_eq!(got.len(), labels.len(), "domestic 下 {url} 候选条数变了：{got:?}");
                                    assert_eq!(got.last().map(|p| p.label), Some("原地址"), "domestic 下 {url} 的原地址要兜底");
                                    let mut sorted = got.iter().map(|p| p.label).collect::<Vec<_>>();
                                    let mut want = labels.to_vec();
                                    sorted.sort_unstable();
                                    want.sort_unstable();
                                    assert_eq!(sorted, want, "domestic 只许改顺序：{url}");
                                }
                            });
                        })
                    })
                })
            })
        });
    }

    // Gitee 附件参与同一次 walk：原地址失败就换它，两个都失败则两条原因都在，
    // 且共用一次 deadline（不因多了一个候选而各算一份预算）。
    #[test]
    fn the_gitee_attachment_takes_part_in_the_same_walk() {
        let _g = env_lock();
        with_env("FISH_DOCK_GITEE_REPO", DOCK_GITEE, || {
            // 显式钉住 auto：候选顺序不依赖跑测试时的进程环境。
            with_env("FISH_DOCK_MIRROR_PREFER", "auto", || {
                let began = Instant::now();
                let fail = fetch_bytes_with(DOCK_ASSET, Duration::from_millis(120), 32, |_, _| {
                    std::thread::sleep(Duration::from_millis(90));
                    Err("连接或响应超时".to_string())
                })
                .err()
                .unwrap();
                assert_eq!(fail.tried.len(), 2, "{:?}", fail.tried);
                assert!(fail.timeout, "两个候选共用一次 deadline：整体必然标超时");
                assert!(began.elapsed() < Duration::from_millis(240), "总时长受一次预算约束：{:?}", began.elapsed());
                let reason = fail.reason();
                assert!(reason.contains("原地址") && reason.contains("Gitee 附件"), "两边原因都要留下：{reason}");
                // 第一条失败第二条成功：Gitee 附件接手（Gitee 侧 404 时正是这条回落救场）。
                let mut seen = Vec::new();
                let got = fetch_bytes_with(DOCK_ASSET, Duration::from_secs(5), 32, |url, _| {
                    seen.push(url.to_string());
                    if url.starts_with("https://github.com/") {
                        Err("HTTP 404（不存在）".to_string())
                    } else {
                        Ok(vec![9, 9])
                    }
                })
                .unwrap();
                assert_eq!(got, vec![9, 9]);
                assert_eq!(seen, vec![DOCK_ASSET.to_string(), DOCK_GITEE_ASSET.to_string()], "顺序：原地址 → Gitee 附件");
            });
        });
    }
}