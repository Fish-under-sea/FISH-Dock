//! 第三方插件：按 npm 包名检索与安装。包只要按插件格式发布即可，宿主不审核其行为。
//! 与官方商店的区别只在来源：身份一律按第三方命名空间登记，摘要取自下载到的 tarball。
//!
//! #t19 源链单一真相：registry 的**前缀表、顺序、tarball 主机校验**全部从
//! `crate::config::plugin_npm_registry()` / `plugin_npm_mirror()` 派生（环境变量
//! `FISH_DOCK_PLUGIN_NPM_REGISTRY` / `_NPM_MIRROR` 可把源指到任何兼容 registry，
//! 第三方 registry 与镜像都真能生效）。不再有第二张写死的主机表：识别「这是
//! registry 地址」用当前配置值本身推出的前缀。
//!
//! 源顺序：registry 在前、镜像兜底（官方优先，首次失败走镜像——目标③）。
//! 错误分类保留（DNS / TLS / 超时 / HTTP 状态 / 大小超限分开报），每一源的结局
//! 都进日志，不吞任何一条。
use std::{collections::{BTreeMap, BTreeSet}, io::Read};
use serde::{Deserialize, Serialize};
use crate::storage::Source;
const SEARCH_LIMIT: usize = 512 * 1024;
const META_LIMIT: usize = 2 * 1024 * 1024;
const ARCHIVE_LIMIT: u64 = crate::plugin_package::TOTAL_LIMIT as u64;
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Hit { pub name: String, pub version: String, pub description: String, pub publisher: String, pub updated: String }

// 检索结果：合规包列表 + 被命名规范过滤掉的数量。hidden 必须回传前端，
// 否则不合规包被静默丢弃，用户会以为「搜索坏了」（用户 2026-10-31 要求）。
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchReport { pub hits: Vec<Hit>, pub hidden: usize }

#[derive(Deserialize)]
struct SearchAnswer { objects: Vec<SearchObject> }
#[derive(Deserialize)]
struct SearchObject { package: SearchPackage }
#[derive(Deserialize)]
struct SearchPackage { name: String, version: String, #[serde(default)] description: String, #[serde(default)] date: String, #[serde(default)] publisher: Option<Publisher> }
#[derive(Deserialize)]
struct Publisher { #[serde(default)] username: String }

// ── 源链（单一真相，全部从 config 派生）────────────────────────────────
// 一次要尝试的 registry 列表：第一个是主源（检索、元数据都从它读），后面是兜底
// （主源失败时按同路径改写到兜底源再试）。默认 [npmjs, npmmirror]；两个配置键
// 都可指向第三方兼容 registry。镜像与主源相同（配置成一样）时只留一个，不原地
// 重试。
#[derive(Clone, Debug)]
pub(crate) struct NpmSources {
    /// 主源地址（无尾斜杠），检索与元数据的起点；同时是「识别 registry 前缀」的唯一来源。
    primary: String,
    /// 兜底源（无尾斜杠）；与主源相同时为空（不重试同一源）。
    fallback: Option<String>,
}

impl NpmSources {
    // 归一化：去尾斜杠，主源与兜底源相同时不留兜底（避免原地重试同一地址）。
    fn normalized(primary: String, fallback: String) -> Self {
        let primary = primary.trim_end_matches('/').to_string();
        let fallback = fallback.trim_end_matches('/').to_string();
        let fallback = (fallback != primary).then_some(fallback);
        NpmSources { primary, fallback }
    }

    // 从 config 读（运行时唯一入口；测试注入构造，不碰进程环境）。
    pub(crate) fn from_config() -> Self {
        NpmSources::normalized(crate::config::plugin_npm_registry(), crate::config::plugin_npm_mirror())
    }

    // 测试注入。
    #[cfg(test)]
    fn new(primary: &str, fallback: &str) -> Self {
        NpmSources::normalized(primary.to_string(), fallback.to_string())
    }

    // 一次 GET 依次经过的源地址（主源在前）。
    fn order(&self) -> Vec<&str> {
        let mut all = vec![self.primary.as_str()];
        if let Some(fallback) = &self.fallback { all.push(fallback.as_str()); }
        all
    }

    // 把「主源地址」改写成「该源的同一资源」：URL 以主源前缀开头才改写（前缀
    // 就取自配置值本身，没有第二张写死主机表）。其他域名（tarball 常落在 registry
    // 自己的 assets/cdn 域）原样保留——它们指向哪里就从哪里读，不猜等价路径。
    fn rewrite(&self, url: &str, source: &str) -> Option<String> {
        if let Some(rest) = url.strip_prefix(self.primary.as_str()) {
            return Some(format!("{source}{rest}"));
        }
        Some(url.to_string())
    }

    // tarball 的主机是否与「当前任一配置源」同源。判定全部从配置值派生（主机相等 /
    // 子域 / 同站点域的 cdn），不是写死白名单——配置指到第三家时，第三家的 tarball
    // 域名同样通过；npmmirror 的 cdn 子域也在同站点域内。
    fn tarball_from_configured(&self, tarball: &tauri::Url) -> bool {
        let Some(host) = tarball.host_str().map(str::to_ascii_lowercase) else { return false };
        self.order().iter().any(|source| {
            tauri::Url::parse(source).ok()
                .and_then(|u| u.host_str().map(str::to_ascii_lowercase))
                .is_some_and(|source_host| host_within_site(&host, &source_host))
        })
    }
}

// 主机是否落在某个配置源的「站点」内（全从配置值派生，无第二张写死表）：
//   1. 与源主机完全相同；
//   2. 是源主机的子域（<x>.<source_host>）；
//   3. 同站点域：源主机去掉最左一段后（registry.npmmirror.com → npmmirror.com），
//      tarball 落在该站点域或其子域上（cdn.npmmirror.com 一类 registry 自有 cdn
//      因此通过）。第 3 条只对 ≥3 段的源主机放宽，避免两段源主机被放宽到整个 TLD。
fn host_within_site(host: &str, source_host: &str) -> bool {
    if host == source_host || host.ends_with(&format!(".{source_host}")) { return true; }
    let labels: Vec<&str> = source_host.split('.').filter(|label| !label.is_empty()).collect();
    if labels.len() >= 3 {
        let site = labels[1..].join(".");
        if host == site || host.ends_with(&format!(".{site}")) { return true; }
    }
    false
}

// 每一源 / 每一路 web 的一行结局都攒进 log，最后统一打到 stderr（与 plugin_store 的
// 多源日志同风格：不让任何一条失败被后一条覆盖）。
fn drain_log(log: &[String]) {
    for line in log { eprintln!("FISH Dock: {line}"); }
}

// 一次 GET：主源在前，失败（连不上、超时、非 200）按同路径改写到兜底源再试。
// 每一源的结局都进 `log`（一句话人话，与 plugin_store 的多源日志同风格），
// 错误返回的是「各源结局」的合并，不是只留最后一条。
fn get(url: &str, limit: usize, log: &mut Vec<String>) -> Result<Vec<u8>, String> {
    let sources = NpmSources::from_config();
    get_via(&sources, url, limit, log)
}

// get 的注入版（测试用）：源链由参数给出，不读 config。
fn get_via(sources: &NpmSources, url: &str, limit: usize, log: &mut Vec<String>) -> Result<Vec<u8>, String> {
    get_via_with(sources, url, limit, log, get_one)
}

// 日志里的「来源」按**实际发起请求的 URL 主机**标注：配置源与实际主机不同时写成
// 「npm 源 <配置源> → 实际 <主机>」，绝不按迭代序号把兜底源的地址记到主源头上
// （t24 实测：主源 127.0.0.1:1 时，npmmirror 的绝对 tarball 曾被记成「npm 源
// http://127.0.0.1:1 读到 3619 字节」——因为 rewrite 对不匹配前缀的绝对 URL 原样
// 透传，迭代变量还是主源）。以 URL 主机为准则归属才对。
fn source_label(source: &str, actual_url: &str) -> String {
    match (crate::proxy::host_of(source), crate::proxy::host_of(actual_url)) {
        (Some(source_host), Some(actual_host)) if source_host != actual_host => format!("npm 源 {source} → 实际 {actual_host}"),
        _ => format!("npm 源 {source}"),
    }
}

// 源链 + 单地址 GET 都注入的版本：测试据此验证「主源失败 → 兜底源拿到不同 URL」
// 以及「每一源各自的结局都进 log / 最终错误」，不依赖真实网络。
fn get_via_with<F>(sources: &NpmSources, url: &str, limit: usize, log: &mut Vec<String>, mut fetch: F) -> Result<Vec<u8>, String>
where F: FnMut(&str, usize) -> Result<Vec<u8>, String> {
    let mut failures: Vec<String> = Vec::new();
    for source in sources.order() {
        let Some(url) = sources.rewrite(url, source) else { continue };
        // 来源以实际请求的 URL 主机为准（改写后可能与配置源主机不同）。
        let label = source_label(source, &url);
        match fetch(&url, limit) {
            Ok(bytes) => {
                log.push(format!("{label} 读到 {} 字节", bytes.len()));
                return Ok(bytes);
            }
            Err(error) => {
                log.push(format!("{label} 失败：{error}"));
                failures.push(format!("{label}：{error}"));
            }
        }
    }
    Err(failures.join("；"))
}

// 一次对单一地址的 GET：各 Way 依次试（web_ways），错误按类别报——
// DNS 解析失败、TLS、超时、HTTP 状态、读超限分开说，不笼统「连接失败」。
fn get_one(url: &str, limit: usize) -> Result<Vec<u8>, String> {
    let mut failures: Vec<String> = Vec::new();
    for way in crate::proxy::web_ways(url) {
        let Some(agent) = way.agent_with(crate::config::pet_timeout(), |config| config.max_redirects(crate::config::http_redirects() as u32).http_status_as_error(false)) else { continue };
        match agent.get(url).header("User-Agent", "FISH-Dock-Plugin-Store").header("Accept", "application/json").call() {
            Ok(mut response) => {
                let status = response.status().as_u16();
                if status != 200 { failures.push(http_status_words(status)); continue; }
                match response.body_mut().with_config().limit(limit as u64).read_to_vec() {
                    Ok(bytes) => return Ok(bytes),
                    Err(e) => failures.push(format!("响应读取失败或超过大小限制（{e}）")),
                }
            }
            // DNS / TLS / 超时 / 连接失败各自成词：Err(e) 里 ureq 的 Error 枚举
            // 分得清（HostNotFound / Timeout / Tls / ConnectionFailed / Io），
            // 丢了它们就只剩一句「连接失败」（plugin_store.rs:178 的教训）。
            Err(e) => failures.push(npm_plain_error(&e)),
        }
    }
    Err(if failures.is_empty() { "npm 无可用网络路径（代理配置均不可用）".to_string() } else { failures.join("；") })
}

// HTTP 非 200 的分档：404 明确「不存在」、429 明确「限流」，其余只报状态码。
fn http_status_words(status: u16) -> String {
    if status == 404 { format!("HTTP {status}（不存在）") }
    else if status == 429 { format!("HTTP {status}（限流）") }
    else { format!("HTTP {status}") }
}

// TLS 类错误的人话：带一句可能原因，让人知道去查代理 / 端口 / 证书（t24 实测：
// https 连明文 http 端口时 ureq 给 Io(InvalidData, InvalidMessage(InvalidContentType))，
// 旧实现落到「网络 IO 失败」，既看不懂也没方向）。
fn tls_words(detail: &str) -> String {
    format!("TLS 握手失败（可能：代理拦截 / 端口不是 https / 证书不被信任）：{detail}")
}

// ureq 错误 → 一句话人话（保类别）。
fn npm_plain_error(e: &ureq::Error) -> String {
    match e {
        ureq::Error::HostNotFound => "域名解析失败（DNS）".into(),
        ureq::Error::Timeout(_) => "连接或响应超时".into(),
        ureq::Error::Tls(message) => tls_words(message),
        ureq::Error::StatusCode(status) => format!("HTTP {status}"),
        ureq::Error::Io(io) => {
            let kind = io.kind();
            let text = io.to_string();
            let lower = text.to_ascii_lowercase();
            // TLS 类 Io：rustls 把握手失败包成 Io(InvalidData ...)，底层文案是
            // "received corrupt message of type InvalidContentType"——按类型或关键词认出来。
            let tls_like = kind == std::io::ErrorKind::InvalidData
                || lower.contains("invalidmessage")
                || lower.contains("invalidcontenttype")
                || lower.contains("corrupt message")
                || lower.contains("tls")
                || lower.contains("certificate")
                || lower.contains("handshake");
            if tls_like { tls_words(&text) }
            // 本机实测：straight 连不存在的域名（.invalid）得到的是 Io(UnexpectedEof)，
            // 而不是 HostNotFound（网络层把 DNS 失败吞成 EOF）。这里补一句人话提示；
            // HostNotFound 分支仍保留——会明确报 DNS 失败的网络栈走那条。
            else if kind == std::io::ErrorKind::UnexpectedEof { "连接被提前关闭（可能是 DNS 解析失败、代理拦截或对方拒绝）".into() }
            else if kind == std::io::ErrorKind::TimedOut { "连接或响应超时".into() }
            else if kind == std::io::ErrorKind::ConnectionRefused { "连接被拒绝".into() }
            else if kind == std::io::ErrorKind::ConnectionReset { "连接被重置".into() }
            else { format!("网络 IO 失败（{io}）") }
        }
        other => {
            // rustls / native-tls 的具体错误（Tls 变体之外的证书类错误）也归到 TLS，
            // 不让它们退回成一句笼统的「连接失败」。
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

fn segment(value: &str) -> bool {
    !value.is_empty() && value.len() <= 214 && !value.starts_with('.') && !value.starts_with('_')
        && value.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'-' | b'.' | b'_'))
}
// npm 包名：`name`，或带 scope 的 `@scope/name`。
pub fn package_name(value: &str) -> Result<String, String> {
    let name = value.trim();
    let body = name.strip_prefix('@').unwrap_or(name);
    let valid = match body.split_once('/') {
        Some((scope, rest)) => name.starts_with('@') && segment(scope) && segment(rest),
        None => segment(body),
    };
    if !valid || name.len() > 214 { return Err("npm 包名无效（示例：@acme/fish-weather 或 fish-weather）".into()); }
    Ok(name.to_string())
}

// 第三方与官方插件共用同一条包名主干 `fish-dock-plugin-`。这里从
// plugin_store::NPM_PACKAGE_PREFIX（带 scope 的 `@fish-under-sea/fish-dock-plugin-`）
// 取出「不带 scope 的前缀」，不再把字面量抄第二/第三份（用户要求单一判据）。
fn plugin_stem() -> &'static str {
    crate::plugin_store::NPM_PACKAGE_PREFIX.rsplit('/').next().unwrap_or(crate::plugin_store::NPM_PACKAGE_PREFIX)
}

// 第三方插件包名的单一判据：合法 npm 名，且去掉可选 `@scope/` 后必须以
// `fish-dock-plugin-` 开头、前缀之后的部分非空且合法（复用 segment）。返回规范化后的
// 原名（含 scope）。合规示例：`fish-dock-plugin-x`、`@any/fish-dock-plugin-x`。
// 拒绝：`lodash`、`fish-plugin-x`、`fish-dock-x`、`fish-dock-plugin-`（空后缀）、非法字符。
pub(crate) fn plugin_package_name(value: &str) -> Result<String, String> {
    let name = package_name(value)?;
    let body = match name.strip_prefix('@') {
        Some(rest) => rest.split_once('/').map(|(_, bare)| bare).unwrap_or(rest),
        None => name.as_str(),
    };
    match body.strip_prefix(plugin_stem()).filter(|rest| segment(rest)) {
        Some(_) => Ok(name),
        None => Err(format!("第三方插件包名必须是 `fish-dock-plugin-<名字>`（可带 `@scope/`），`{value}` 不符合")),
    }
}
// `name` 或 `name@1.2.3`；版本交给 registry 解析。
fn spec(value: &str) -> Result<(String, Option<String>), String> {
    let raw = value.trim();
    let (name, version) = match raw.rsplit_once('@') {
        Some((name, version)) if !name.is_empty() && !version.is_empty() => (name, Some(version.to_string())),
        _ => (raw, None),
    };
    let name = package_name(name)?;
    if let Some(version) = &version {
        if version.len() > 64 || !version.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'+')) { return Err("npm 包版本无效".into()); }
    }
    Ok((name, version))
}

// 检索：精确包名优先 + 关键词 search 兜底。
// npm 的 /-/v1/search 按相关度排序，输入精确包名（如 fish-dock-plugin-filemgr）时它
// 反而排不进前 20（用户 2026-10-31 实测）——所以合法包名先做一次元数据精确查并置顶，
// 再并入 search 结果、按包名去重。非合法包名只走 search，不把任意输入拼进 URL。
pub fn search(query: &str) -> Result<SearchReport, String> {
    let sources = NpmSources::from_config();
    let mut log = Vec::new();
    let result = search_via(&sources, query, &mut log, get_one);
    // 精确查与 search 两段、每一源各自的结局都打进日志，不吞。
    drain_log(&log);
    result
}

// 元数据地址：urlencode 后还原 @scope 的斜杠与 @（npmmirror 承重要求，勿删）。
fn metadata_url(primary: &str, name: &str) -> String {
    format!("{}/{}", primary, urlencode(name).replace("%2F", "/").replace("%40", "@"))
}

// search 的注入版（测试用）：源链与单地址 GET 都注入，不读 config、不碰网络。
fn search_via<F>(sources: &NpmSources, query: &str, log: &mut Vec<String>, mut fetch: F) -> Result<SearchReport, String>
where F: FnMut(&str, usize) -> Result<Vec<u8>, String> {
    let text = query.trim();
    if text.is_empty() || text.len() > 120 { return Err("请输入要检索的包名或关键字".into()); }
    let mut hits: Vec<Hit> = Vec::new();
    let mut hidden = 0usize;
    let mut seen: BTreeSet<String> = BTreeSet::new();

    // ① 精确包名优先：查询串本身是合法 npm 包名时先按元数据精确查（主源失败走镜像）。
    // 命中就置顶；404 / 网络失败都不致命——静默回退到 search（错误分类由 search 失败体现）。
    if let Ok(name) = package_name(text) {
        let url = metadata_url(&sources.primary, &name);
        if let Ok(bytes) = get_via_with(sources, &url, META_LIMIT, log, &mut fetch) {
            if let Ok(meta) = serde_json::from_slice::<Meta>(&bytes) {
                if let Some(latest) = meta.dist_tags.get("latest").cloned() {
                    if plugin_package_name(&name).is_ok() {
                        hits.push(Hit {
                            name: name.clone(),
                            version: latest,
                            description: truncate(&meta.description, 240),
                            publisher: meta.maintainers.first().map(|m| m.name.clone()).unwrap_or_default(),
                            updated: truncate(meta.time.get("modified").map(String::as_str).unwrap_or(""), 40),
                        });
                        seen.insert(name);
                    } else {
                        // 精确命中但不合规范：拒收，计入 hidden。
                        hidden += 1;
                    }
                }
            }
        }
    }

    // ② 并入 search；按包名去重（精确那条优先，同名不再出现第二条）。
    let url = format!("{}/-/v1/search?text={}&size=20", sources.primary, urlencode(text));
    let bytes = get_via_with(sources, &url, SEARCH_LIMIT, log, &mut fetch)?;
    let answer: SearchAnswer = serde_json::from_slice(&bytes).map_err(|_| "npm 检索结果格式无效")?;
    for object in answer.objects.into_iter().take(20) {
        let row = object.package;
        if plugin_package_name(&row.name).is_err() { hidden += 1; continue; }
        if !seen.insert(row.name.clone()) { continue; }
        hits.push(Hit {
            publisher: row.publisher.map(|p| p.username).unwrap_or_default(),
            description: truncate(&row.description, 240),
            updated: truncate(&row.date, 40),
            name: row.name,
            version: row.version,
        });
    }
    Ok(SearchReport { hits, hidden })
}

fn truncate(value: &str, max: usize) -> String {
    let clean: String = value.chars().filter(|c| !c.is_control()).take(max).collect();
    clean
}
fn urlencode(value: &str) -> String {
    let mut out = String::new();
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => out.push(byte as char),
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

#[derive(Deserialize)]
struct Meta {
    #[serde(rename = "dist-tags")] dist_tags: BTreeMap<String, String>,
    versions: BTreeMap<String, Version>,
    // 精确包名兜底要用的展示字段（registry 的包文档根上有 description / maintainers / time）。
    #[serde(default)] description: String,
    #[serde(default)] maintainers: Vec<Maintainer>,
    #[serde(default)] time: BTreeMap<String, String>,
}
#[derive(Deserialize)]
struct Maintainer { #[serde(default)] name: String }
#[derive(Deserialize)]
struct Version { dist: Dist }
#[derive(Deserialize)]
struct Dist { tarball: String }

// 元数据 → 版本与 tarball 地址。URL 从**主源**拼（urlencode 后还原 @scope 的
// 斜杠与 @：npmmirror 实测对 %40/%2F 编码的 scope 返 422、对原始 @scope/name
// 返 200——这两处 replace 是承重代码，不许删）；get_via 里失败才改写到兜底源。
// tarball 主机校验：与**当前配置的任一源**同源（同主机 / 子域 / 同站点域的 cdn），
// 不再是写死的白名单——第三家 registry 的 tarball 域名同样通过；不属于任何配置源
// 时给明确错误（列出配置源，用户知道去哪改）。
fn resolve(name: &str, version: Option<&str>) -> Result<(String, String), String> {
    let sources = NpmSources::from_config();
    let mut log = Vec::new();
    let result = resolve_via(&sources, name, version, &mut log, get_one);
    // 元数据这轮的每一源结局也要落到日志（以前只打 tarball 那轮，元数据源失败被吞）。
    drain_log(&log);
    result
}

// resolve 的注入版（测试用）：源链与单地址 GET 都注入，不读 config、不碰网络。
fn resolve_via<F>(sources: &NpmSources, name: &str, version: Option<&str>, log: &mut Vec<String>, mut fetch: F) -> Result<(String, String), String>
where F: FnMut(&str, usize) -> Result<Vec<u8>, String> {
    // 下载前先过命名判据：不合规包在这里就返回明确错误，绝不发起任何网络请求
    //（obtain → resolve → resolve_via 都走这一处，单一判据、下载前拒绝）。
    plugin_package_name(name)?;
    // 元数据地址走 metadata_url（同样保留 npmmirror 对 @scope/name 的原始形态：
    // 编码返 422、原始返 200，承重勿删）。
    let url = metadata_url(&sources.primary, name);
    let bytes = get_via_with(sources, &url, META_LIMIT, log, &mut fetch)?;
    let meta: Meta = serde_json::from_slice(&bytes).map_err(|_| "npm 包信息格式无效")?;
    let wanted = version.map(str::to_string).or_else(|| meta.dist_tags.get("latest").cloned()).ok_or("npm 包没有可安装的版本")?;
    let row = meta.versions.get(&wanted).ok_or_else(|| format!("npm 包没有 {wanted} 这个版本"))?;
    let tarball = tauri::Url::parse(&row.dist.tarball).map_err(|_| "npm 包下载地址无效")?;
    if tarball.scheme() != "https" || !tarball.username().is_empty() || tarball.password().is_some() || tarball.fragment().is_some() {
        return Err("npm 包下载必须使用无身份信息的 HTTPS 地址".into());
    }
    if !sources.tarball_from_configured(&tarball) {
        return Err(format!("npm 包下载地址 {} 不属于当前配置的任何 npm 源（{}）——请在 FISH_DOCK_PLUGIN_NPM_REGISTRY / _NPM_MIRROR 中加入该源，或换一个包", tarball.host_str().unwrap_or(""), sources.order().join("、")));
    }
    Ok((wanted, tarball.to_string()))
}

// ustar：512 字节头，名字、大小、类型、前缀都在固定位置；npm 的包里只有普通文件。
fn untar(bytes: &[u8]) -> Result<Vec<(String, Vec<u8>)>, String> {
    let mut out = Vec::new();
    let mut at = 0usize;
    while at + 512 <= bytes.len() {
        let header = &bytes[at..at + 512];
        if header.iter().all(|byte| *byte == 0) { break; }
        let field = |start: usize, len: usize| String::from_utf8_lossy(&header[start..start + len]).trim_end_matches(['\0', ' ']).to_string();
        let name = field(0, 100);
        let prefix = field(345, 155);
        let size = usize::from_str_radix(field(124, 12).trim(), 8).map_err(|_| "npm 包条目大小无效")?;
        let kind = header[156];
        let full = if prefix.is_empty() { name } else { format!("{prefix}/{name}") };
        let start = at + 512;
        if start + size > bytes.len() { return Err("npm 包内容被截断".into()); }
        if kind == b'0' || kind == 0 { out.push((full, bytes[start..start + size].to_vec())); }
        at = start + size.div_ceil(512) * 512;
    }
    Ok(out)
}

// unpack 供官方插件的 npm 来源复用（plugin_store）：npm 单包里就是那份 .fish-plugin。
pub(crate) fn unpack(bytes: &[u8]) -> Result<BTreeMap<String, Vec<u8>>, String> {
    let mut decoder = flate2::read::GzDecoder::new(bytes);
    let mut plain = Vec::new();
    decoder.read_to_end(&mut plain).map_err(|_| "npm 包解压失败")?;
    if plain.len() as u64 > ARCHIVE_LIMIT { return Err("npm 包解压后超过大小限制".into()); }
    let mut files = BTreeMap::new();
    for (name, body) in untar(&plain)? {
        // npm 的归档把包内容放在 `package/` 下。
        let relative = name.strip_prefix("./").unwrap_or(&name);
        let Some(relative) = relative.strip_prefix("package/").or_else(|| relative.strip_prefix("package\\")) else { continue };
        if relative.is_empty() { continue; }
        crate::plugin_manifest::package_path(std::path::Path::new("."), relative).map_err(|_| format!("npm 包包含无效路径：{relative}"))?;
        if files.insert(relative.to_string(), body).is_some() { return Err(format!("npm 包包含重复文件：{relative}")); }
        if files.len() > 4096 { return Err("npm 包文件数量超过限制".into()); }
    }
    Ok(files)
}

// 下载并按插件格式校验；返回可直接交给安装器的包与 tarball 摘要。
// 每一源（主源/兜底）的结局进 stderr 日志（与 plugin_store 多源日志同风格），
// 失败时错误里带着各源的各自结局，不是只留最后一条。
pub fn obtain(context: &crate::storage_context::Context, value: &str) -> Result<(crate::plugin_package::Package, String), String> {
    use sha2::{Digest, Sha256};
    let (name, version) = spec(value)?;
    let (version, tarball) = resolve(&name, version.as_deref())?;
    let mut log = Vec::new();
    let bytes = get(&tarball, ARCHIVE_LIMIT as usize, &mut log)?;
    drain_log(&log);
    let digest = format!("{:x}", Sha256::digest(&bytes));
    let files = unpack(&bytes)?;
    let raw = files.get("plugin.json").cloned().ok_or("这个 npm 包不是 FISH Dock 插件：缺少 plugin.json")?;
    if !files.contains_key("README.md") { return Err("这个 npm 包缺少当前版本 README.md".into()); }
    let manifest = crate::plugin_manifest::Manifest::parse(&raw, &context.root, Source::ThirdParty, &crate::plugin_install::platform())?;
    if files.len() != manifest.files.len() + 1 { return Err("npm 包包含未声明的文件或缺少文件".into()); }
    for (file, expected) in &manifest.files {
        let body = files.get(file).ok_or_else(|| format!("npm 包缺少声明的资源：{file}"))?;
        if !format!("{:x}", Sha256::digest(body)).eq_ignore_ascii_case(expected) { return Err(format!("npm 包资源校验失败：{file}")); }
    }
    if manifest.version != version { return Err("npm 包清单版本与发布的版本不一致".into()); }
    Ok((crate::plugin_package::Package { manifest, files }, digest))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn package_names_and_specs_are_strict() {
        assert_eq!(package_name("fish-weather").unwrap(), "fish-weather");
        assert_eq!(package_name("@acme/fish-weather").unwrap(), "@acme/fish-weather");
        for bad in ["", "Fish-Weather", "../evil", "@acme/", ".hidden", "a".repeat(215).as_str()] {
            assert!(package_name(bad).is_err(), "{bad}");
        }
        assert_eq!(spec("fish-weather@1.2.3").unwrap(), ("fish-weather".to_string(), Some("1.2.3".to_string())));
        assert_eq!(spec("@acme/plugin").unwrap(), ("@acme/plugin".to_string(), None));
        assert!(spec("fish@1.2.3;rm -rf").is_err());
    }
    #[test]
    fn the_archive_reader_only_takes_files_under_package() {
        let mut plain = Vec::new();
        for (name, body) in [("package/plugin.json", b"{}".as_slice()), ("package/backend.mjs", b"x".as_slice()), ("other.txt", b"no".as_slice())] {
            let mut header = [0u8; 512];
            header[..name.len()].copy_from_slice(name.as_bytes());
            let size = format!("{:o}", body.len());
            header[124..124 + size.len()].copy_from_slice(size.as_bytes());
            header[156] = b'0';
            plain.extend_from_slice(&header);
            plain.extend_from_slice(body);
            plain.resize(plain.len() + (512 - body.len() % 512) % 512, 0);
        }
        plain.extend_from_slice(&[0u8; 1024]);
        let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
        std::io::Write::write_all(&mut encoder, &plain).unwrap();
        let files = unpack(&encoder.finish().unwrap()).unwrap();
        assert_eq!(files.len(), 2);
        assert!(files.contains_key("plugin.json"));
        assert!(!files.keys().any(|name| name.starts_with("other")));
    }
    #[test]
    fn a_package_without_a_manifest_is_named_as_such() {
        let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
        std::io::Write::write_all(&mut encoder, &[0u8; 1024]).unwrap();
        let files = unpack(&encoder.finish().unwrap()).unwrap();
        assert!(files.is_empty());
    }

    // b) 官方源失败 → 镜像拿到**不同**的 URL（证明不是原地重试），且每一源各自的
    // 结局都进 log 与最终错误（不是只留最后一条）。
    #[test]
    fn official_failure_hands_the_mirror_a_different_url() {
        let sources = NpmSources::new("https://registry.npmjs.org", "https://registry.npmmirror.com");
        assert_eq!(sources.order(), vec!["https://registry.npmjs.org", "https://registry.npmmirror.com"]);
        let url = format!("{}/-/v1/search?text=fish", sources.primary);
        let mut asked = Vec::new();
        let mut log = Vec::new();
        let error = get_via_with(&sources, &url, 1024, &mut log, |u, _| {
            asked.push(u.to_string());
            Err("连接被拒绝".to_string())
        }).unwrap_err();
        assert_eq!(asked.len(), 2, "两个源都该被尝试：{asked:?}");
        assert_ne!(asked[0], asked[1], "兜底源必须是不同 URL，否则退化成原地重试");
        assert_eq!(asked[0], "https://registry.npmjs.org/-/v1/search?text=fish");
        assert_eq!(asked[1], "https://registry.npmmirror.com/-/v1/search?text=fish");
        assert!(log.iter().any(|line| line.contains("registry.npmjs.org") && line.contains("失败")), "{log:?}");
        assert!(log.iter().any(|line| line.contains("registry.npmmirror.com") && line.contains("失败")), "{log:?}");
        assert!(error.contains("registry.npmjs.org") && error.contains("registry.npmmirror.com"), "{error}");
    }

    // a) registry 指向第三家（fixture）：元数据与 tarball 都能走通。
    #[test]
    fn a_third_party_registry_serves_metadata_and_tarball() {
        let sources = NpmSources::new("https://registry.third.test", "https://cdn-mirror.third.test");
        // 带 scope 的合规包名保持原始 @scope/name（t7 承重：npmmirror 对 %40/%2F 返 422）。
        let meta_path = urlencode("@acme/fish-dock-plugin-weather").replace("%2F", "/").replace("%40", "@");
        assert_eq!(meta_path, "@acme/fish-dock-plugin-weather");
        let meta = serde_json::json!({
            "dist-tags": { "latest": "1.0.0" },
            "versions": { "1.0.0": { "dist": { "tarball": "https://cdn.third.test/acme/fish-dock-plugin-weather/-/fish-dock-plugin-weather-1.0.0.tgz" } } }
        }).to_string();
        let mut log = Vec::new();
        let (version, tarball) = resolve_via(&sources, "@acme/fish-dock-plugin-weather", None, &mut log, |url, _| {
            if url == "https://registry.third.test/@acme/fish-dock-plugin-weather" { Ok(meta.clone().into_bytes()) }
            else { Err(format!("元数据不该改写到 {url}")) }
        }).expect("第三家 registry 的元数据与 tarball 都应走通");
        assert_eq!(version, "1.0.0");
        assert_eq!(tarball, "https://cdn.third.test/acme/fish-dock-plugin-weather/-/fish-dock-plugin-weather-1.0.0.tgz");
        // tarball 也能真的取下来（cdn 子域与配置源同源，通过主机校验）。
        let mut log2 = Vec::new();
        let body = get_via_with(&sources, &tarball, 1024, &mut log2, |url, _| {
            if url == tarball { Ok(b"TGZ".to_vec()) } else { Err(format!("tarball 地址被改写：{url}")) }
        }).unwrap();
        assert_eq!(body, b"TGZ");
    }

    // c) 主机不属于任何已配置 registry：给明确错误（列出配置源），不是笼统 Err；
    // 后缀嫁接的伪装域名也不放过。
    #[test]
    fn a_tarball_outside_every_configured_registry_is_rejected_explicitly() {
        let sources = NpmSources::new("https://registry.npmjs.org", "https://registry.npmmirror.com");
        let meta = |tarball: &str| serde_json::json!({
            "dist-tags": { "latest": "1.0.0" },
            "versions": { "1.0.0": { "dist": { "tarball": tarball } } }
        }).to_string();
        let mut log = Vec::new();
        let error = resolve_via(&sources, "fish-dock-plugin-weather", None, &mut log, |_, _| Ok(meta("https://evil.attacker.example/evil.tgz").into_bytes())).unwrap_err();
        assert!(error.contains("不属于当前配置的任何 npm 源"), "错误应说明不属于任何配置源：{error}");
        assert!(error.contains("registry.npmjs.org") && error.contains("registry.npmmirror.com"), "错误应列出配置源：{error}");
        for host in ["https://evil.registry.npmjs.org.attacker.example/x.tgz", "https://registry.npmjs.org.evil.example/x.tgz"] {
            let mut log = Vec::new();
            assert!(resolve_via(&sources, "fish-dock-plugin-weather", None, &mut log, |_, _| Ok(meta(host).into_bytes())).is_err(), "{host} 不该通过同源校验");
        }
    }

    // d) 错误分类各一条：DNS / TLS / 超时 / HTTP 状态 / 不存在 各自成词，不塌成一句。
    #[test]
    fn errors_keep_their_category_instead_of_collapsing() {
        assert_eq!(npm_plain_error(&ureq::Error::HostNotFound), "域名解析失败（DNS）");
        // TLS 变体不再只回一句裸文案，统一带「可能原因」。
        assert_eq!(npm_plain_error(&ureq::Error::Tls("证书无效")), tls_words("证书无效"));
        assert!(npm_plain_error(&ureq::Error::Tls("证书无效")).starts_with("TLS 握手失败"));
        assert_eq!(npm_plain_error(&ureq::Error::Timeout(ureq::Timeout::Global)), "连接或响应超时");
        assert_eq!(npm_plain_error(&ureq::Error::Io(std::io::Error::new(std::io::ErrorKind::TimedOut, "x"))), "连接或响应超时");
        assert_eq!(npm_plain_error(&ureq::Error::Io(std::io::Error::new(std::io::ErrorKind::ConnectionRefused, "x"))), "连接被拒绝");
        // 本机实测：DNS 失败表现为 Io(UnexpectedEof)，给的是「连接被提前关闭…」这句人话；
        // 上面的 HostNotFound 分支断言仍在（别的网络栈会走那条）。
        assert_eq!(npm_plain_error(&ureq::Error::Io(std::io::Error::new(std::io::ErrorKind::UnexpectedEof, "x"))), "连接被提前关闭（可能是 DNS 解析失败、代理拦截或对方拒绝）");
        // HTTP 状态与「不存在」在 get_one 里按状态分档（http_status_as_error(false) 后
        // 状态不走 Err，而是这里分档）。
        assert_eq!(http_status_words(404), "HTTP 404（不存在）");
        assert_eq!(http_status_words(429), "HTTP 429（限流）");
        assert_eq!(http_status_words(500), "HTTP 500");
        assert_ne!(http_status_words(404), http_status_words(500));
    }

    // 修 1：把 t24 实测的 TLS 类 Io（https 连明文 http 端口）合成出来，断言归到
    // 「TLS 握手失败」并带可能原因；不再落到笼统的「网络 IO 失败」。
    #[test]
    fn tls_like_io_is_reported_as_a_tls_handshake_failure() {
        // 复刻 t24 抓到的那一种形态：Io(InvalidData, "received corrupt message of type InvalidContentType")
        let real = ureq::Error::Io(std::io::Error::new(std::io::ErrorKind::InvalidData, "received corrupt message of type InvalidContentType"));
        let words = npm_plain_error(&real);
        assert!(words.starts_with("TLS 握手失败"), "应归 TLS：{words}");
        assert!(words.contains("代理拦截") && words.contains("端口不是 https") && words.contains("证书不被信任"), "应给可能原因：{words}");
        assert!(words.contains("InvalidContentType"), "应带上底层文案：{words}");
        assert!(!words.contains("网络 IO 失败"), "不该退回笼统 IO：{words}");
        // 只有关键词、类型不是 InvalidData 的 Io 也要认出来。
        let by_text = ureq::Error::Io(std::io::Error::new(std::io::ErrorKind::Other, "peer certificate rejected"));
        assert!(npm_plain_error(&by_text).starts_with("TLS 握手失败"), "{}", npm_plain_error(&by_text));
    }

    // 修 2：日志与最终错误里的「来源」按实际请求的 URL 主机标注，不按迭代序号把
    // 兜底源的地址记到主源头上（t24：npmmirror 的绝对 tarball 曾被标成 127.0.0.1:1）。
    #[test]
    fn logs_credit_the_actual_url_host_not_the_iteration_source() {
        let sources = NpmSources::new("http://127.0.0.1:1", "https://registry.npmmirror.com");
        // ① 成功路径：请求的就是 npmmirror 的绝对 tarball（不匹配主源前缀，rewrite 原样透传）。
        let tarball = "https://registry.npmmirror.com/left-pad/-/left-pad-1.3.0.tgz";
        let mut log = Vec::new();
        let bytes = get_via_with(&sources, tarball, 1024, &mut log, |u, _| {
            assert_eq!(u, tarball);
            Ok(b"TGZ".to_vec())
        }).unwrap();
        assert_eq!(bytes, b"TGZ");
        assert_eq!(log.len(), 1);
        assert!(log[0].contains("→ 实际 registry.npmmirror.com 读到"), "来源应写实际主机：{}", log[0]);
        // ② 失败路径：主源失败、兜底成功；兜底那条按自己的实际主机标注。
        let url = "http://127.0.0.1:1/left-pad";
        let mut log2 = Vec::new();
        let ok = get_via_with(&sources, url, 1024, &mut log2, |u, _| {
            if u.contains("127.0.0.1") { Err("连接被拒绝".to_string()) } else { Ok(b"OK".to_vec()) }
        }).unwrap();
        assert_eq!(ok, b"OK");
        assert!(log2[0].contains("127.0.0.1") && log2[0].contains("失败"), "{}", log2[0]);
        assert!(log2[1].contains("registry.npmmirror.com") && log2[1].contains("读到"), "{}", log2[1]);
        // ③ 全失败时，最终错误里的来源也按实际主机而非迭代序号。
        let mut log3 = Vec::new();
        let error = get_via_with(&sources, url, 1024, &mut log3, |_, _| Err("连接被拒绝".to_string())).unwrap_err();
        assert!(error.contains("127.0.0.1") && error.contains("registry.npmmirror.com"), "{error}");
    }

    // 前缀识别由「当前主源配置值」派生：换主源，识别前缀跟着变；非主源前缀
    // （cdn tarball 等）原样保留——没有第二张写死主机表。
    #[test]
    fn rewriting_is_driven_by_the_configured_primary_prefix() {
        let sources = NpmSources::new("https://registry.npmjs.org", "https://registry.npmmirror.com");
        assert_eq!(sources.rewrite("https://registry.npmjs.org/fish", "https://registry.npmmirror.com").as_deref(), Some("https://registry.npmmirror.com/fish"));
        assert_eq!(sources.rewrite("https://registry.npmjs.org/-/v1/search?text=x", "https://registry.npmmirror.com").as_deref(), Some("https://registry.npmmirror.com/-/v1/search?text=x"));
        assert_eq!(sources.rewrite("https://cdn.example.org/pkg.tgz", "https://registry.npmmirror.com").as_deref(), Some("https://cdn.example.org/pkg.tgz"));
        let other = NpmSources::new("https://registry.third.test", "https://mirror.third.test");
        assert_eq!(other.rewrite("https://registry.third.test/fish", "https://mirror.third.test").as_deref(), Some("https://mirror.third.test/fish"));
        assert_eq!(other.rewrite("https://registry.npmjs.org/fish", "https://mirror.third.test").as_deref(), Some("https://registry.npmjs.org/fish"));
    }

    // tarball 同源校验：同主机 / 子域 / 同站点域的 cdn 通过；外来域与后缀嫁接拒绝。
    #[test]
    fn tarball_host_must_share_the_configured_registry_site() {
        let npm = NpmSources::new("https://registry.npmjs.org", "https://registry.npmmirror.com");
        for allowed in ["https://registry.npmjs.org/x.tgz", "https://registry.npmmirror.com/x.tgz", "https://cdn.npmmirror.com/x.tgz"] {
            assert!(npm.tarball_from_configured(&tauri::Url::parse(allowed).unwrap()), "{allowed} 应与配置源同源");
        }
        for rejected in ["https://evil.attacker.example/x.tgz", "https://registry.npmjs.org.attacker.example/x.tgz"] {
            assert!(!npm.tarball_from_configured(&tauri::Url::parse(rejected).unwrap()), "{rejected} 不应通过");
        }
    }

    // 命名判据：接受两种形态、拒绝五种形态（用户 2026-10-31：只接受 fish-dock-plugin-*）。
    #[test]
    fn only_fish_dock_plugin_packages_are_accepted() {
        // 接受：无 scope 与带 scope 两种。
        assert_eq!(plugin_package_name("fish-dock-plugin-weather").unwrap(), "fish-dock-plugin-weather");
        assert_eq!(plugin_package_name("@acme/fish-dock-plugin-weather").unwrap(), "@acme/fish-dock-plugin-weather");
        // 拒绝五种：lodash / fish-plugin-x / fish-dock-x / 空后缀（含带 scope）/ 非法字符。
        for bad in ["lodash", "fish-plugin-x", "fish-dock-x", "fish-dock-plugin-", "@acme/fish-dock-plugin-"] {
            let error = plugin_package_name(bad).unwrap_err();
            assert!(error.contains("必须是 `fish-dock-plugin-<名字>`"), "{bad} 的报错：{error}");
            assert!(error.contains(bad), "{bad} 的报错应带原名：{error}");
        }
        // 带非法字符（大写 / 斜杠）由底层 npm 名校验挡掉，同样是拒绝。
        for bad in ["fish-dock-plugin-Bad", "fish-dock-plugin-a/b"] {
            assert!(plugin_package_name(bad).is_err(), "{bad} 应被拒");
        }
    }

    // 下载前拒绝：不合规包在 resolve_via 就返回错误，fetch 一次都不许发生（无网络）。
    // （obtain → resolve → resolve_via 都走这一处，故 obtain 层同样在下载前被拦。）
    #[test]
    fn a_noncompliant_package_is_rejected_before_any_network() {
        let sources = NpmSources::new("https://registry.npmjs.org", "https://registry.npmmirror.com");
        let asked = std::cell::Cell::new(0usize);
        let mut log = Vec::new();
        let error = resolve_via(&sources, "lodash", None, &mut log, |_, _| { asked.set(asked.get() + 1); Ok(b"{}".to_vec()) }).unwrap_err();
        assert_eq!(asked.get(), 0, "不合规包不该发起任何请求");
        assert!(log.is_empty(), "不该有任何取数日志：{log:?}");
        assert!(error.contains("必须是 `fish-dock-plugin-<名字>`") && error.contains("lodash"), "{error}");
    }

    // ── 精确包名兜底（t37）────────────────────────────────────────────
    fn search_fixture(objects: serde_json::Value) -> Vec<u8> {
        serde_json::json!({ "objects": objects }).to_string().into_bytes()
    }
    fn meta_fixture(name: &str, version: &str) -> Vec<u8> {
        serde_json::json!({
            "dist-tags": { "latest": version },
            "versions": {},
            "description": format!("desc {name}"),
            "maintainers": [ { "name": "acme" } ],
            "time": { "modified": "2026-10-20T00:00:00.000Z" }
        }).to_string().into_bytes()
    }

    // a) 输入精确包名 → 先 GET 元数据、命中置顶、search 里的同名被去重。
    #[test]
    fn an_exact_package_name_is_looked_up_and_pinned_first() {
        let sources = NpmSources::new("https://registry.npmjs.org", "https://registry.npmmirror.com");
        let search = search_fixture(serde_json::json!([
            { "package": { "name": "fish-dock-plugin-filemgr", "version": "1.0.5", "description": "dup", "date": "2026-10-21" } },
            { "package": { "name": "fish-dock-plugin-other", "version": "2.0.0", "description": "o", "date": "2026-10-01" } }
        ]));
        let meta = meta_fixture("fish-dock-plugin-filemgr", "1.0.5");
        let asked = std::cell::RefCell::new(Vec::<String>::new());
        let mut log = Vec::new();
        let report = search_via(&sources, "fish-dock-plugin-filemgr", &mut log, |url, _| {
            asked.borrow_mut().push(url.to_string());
            if url.contains("/-/v1/search") { Ok(search.clone()) } else { Ok(meta.clone()) }
        }).unwrap();
        let asked = asked.into_inner();
        // 请求序列证据：精确查 URL → search URL
        assert_eq!(asked.len(), 2, "{asked:?}");
        assert_eq!(asked[0], "https://registry.npmjs.org/fish-dock-plugin-filemgr");
        assert_eq!(asked[1], "https://registry.npmjs.org/-/v1/search?text=fish-dock-plugin-filemgr&size=20");
        // 置顶 + 去重
        assert_eq!(report.hits[0].name, "fish-dock-plugin-filemgr");
        assert_eq!(report.hits[0].version, "1.0.5");
        assert_eq!(report.hits.iter().filter(|h| h.name == "fish-dock-plugin-filemgr").count(), 1, "同名不许出现两条");
        assert!(report.hits.iter().any(|h| h.name == "fish-dock-plugin-other"));
        assert_eq!(report.hidden, 0);
    }

    // c) 非法包名（带空格）→ 不发元数据查，只走 search。
    #[test]
    fn a_non_package_query_skips_the_exact_lookup() {
        let sources = NpmSources::new("https://registry.npmjs.org", "https://registry.npmmirror.com");
        let search = search_fixture(serde_json::json!([
            { "package": { "name": "fish-dock-plugin-x", "version": "1.0.0", "description": "x", "date": "2026" } }
        ]));
        let asked = std::cell::RefCell::new(Vec::<String>::new());
        let mut log = Vec::new();
        let report = search_via(&sources, "file manager", &mut log, |url, _| {
            asked.borrow_mut().push(url.to_string());
            Ok(search.clone())
        }).unwrap();
        let asked = asked.into_inner();
        assert_eq!(asked.len(), 1, "只应有 search 一次：{asked:?}");
        assert!(asked[0].contains("/-/v1/search") && asked[0].contains("text=file%20manager"), "{}", asked[0]);
        assert_eq!(report.hits[0].name, "fish-dock-plugin-x");
    }

    // b) 精确查 404 → 静默回退 search，结果照旧、不报错。
    #[test]
    fn an_exact_lookup_404_falls_back_to_search_without_error() {
        let sources = NpmSources::new("https://registry.npmjs.org", "https://registry.npmmirror.com");
        let search = search_fixture(serde_json::json!([
            { "package": { "name": "fish-dock-plugin-x", "version": "1.0.0", "description": "x", "date": "2026" } }
        ]));
        let asked = std::cell::RefCell::new(Vec::<String>::new());
        let mut log = Vec::new();
        let report = search_via(&sources, "fish-dock-plugin-ghost", &mut log, |url, _| {
            asked.borrow_mut().push(url.to_string());
            if url.contains("/-/v1/search") { Ok(search.clone()) } else { Err("HTTP 404（不存在）".to_string()) }
        }).expect("404 不该报错");
        let asked = asked.into_inner();
        assert!(asked.last().unwrap().contains("/-/v1/search"));
        assert_eq!(report.hits.len(), 1);
        assert_eq!(report.hits[0].name, "fish-dock-plugin-x");
    }

    // d) 精确命中但不合规范（lodash）→ 拒收、hidden +1。
    #[test]
    fn an_exact_hit_without_the_prefix_is_filtered_and_counted() {
        let sources = NpmSources::new("https://registry.npmjs.org", "https://registry.npmmirror.com");
        let meta = meta_fixture("lodash", "4.17.21");
        let search = search_fixture(serde_json::json!([]));
        let mut log = Vec::new();
        let report = search_via(&sources, "lodash", &mut log, |url, _| {
            if url.contains("/-/v1/search") { Ok(search.clone()) } else { Ok(meta.clone()) }
        }).unwrap();
        assert!(report.hits.is_empty(), "lodash 不该出现在可安装列表：{:?}", report.hits);
        assert_eq!(report.hidden, 1, "精确命中不合规应计入 hidden");
    }

    // e) 主源精确查失败、镜像成功 → 结果正常（同一 NpmSources 换源）。
    #[test]
    fn an_exact_lookup_falls_to_the_mirror_when_the_primary_fails() {
        let sources = NpmSources::new("https://registry.npmjs.org", "https://registry.npmmirror.com");
        let meta = meta_fixture("fish-dock-plugin-filemgr", "1.0.5");
        let search = search_fixture(serde_json::json!([]));
        let asked = std::cell::RefCell::new(Vec::<String>::new());
        let mut log = Vec::new();
        let report = search_via(&sources, "fish-dock-plugin-filemgr", &mut log, |url, _| {
            asked.borrow_mut().push(url.to_string());
            if url.contains("/-/v1/search") { Ok(search.clone()) }
            else if url.contains("registry.npmjs.org") { Err("连接被拒绝".to_string()) }
            else { Ok(meta.clone()) }
        }).unwrap();
        let asked = asked.into_inner();
        assert_eq!(asked[0], "https://registry.npmjs.org/fish-dock-plugin-filemgr");
        assert_eq!(asked[1], "https://registry.npmmirror.com/fish-dock-plugin-filemgr");
        assert_eq!(report.hits[0].name, "fish-dock-plugin-filemgr");
        assert_eq!(report.hits[0].version, "1.0.5");
    }
}
