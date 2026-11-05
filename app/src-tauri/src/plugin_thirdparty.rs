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

// 检索结果：合规包列表 + 被命名规范过滤掉的数量 + 被判定为官方插件排除掉的数量。
// hidden 必须回传前端，否则不合规包被静默丢弃，用户会以为「搜索坏了」（用户
// 2026-10-31 要求）。official 同理（#72）：官方三个插件在 npm 上也有包名，
// 默认发现词 `fish-dock-plugin` 会把它们排在前面；它们不是第三方，必须排除，
// 但也要**说出来**被排除了几个，不能让人以为「官方插件不见了」。
// 分页（T-C / A10）：total 是 registry 自己报的命中总数（未知时 0），filtered 是
// **本页**通过命名规范、可展示的条数——两者不是一件事，不能只报一个数字；
// hasMore 让前端知道「这一页不是终点」，不把一页空结果误当全部结束。
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchReport { pub hits: Vec<Hit>, pub hidden: usize, pub official: usize, pub total: usize, pub filtered: usize, pub has_more: bool, pub next_from: usize }

// 一页检索的参数。discovery=true 是「发现式浏览」：查询串为空时用默认前缀，
// 且不做精确包名元数据查——用户没点名要某个包，不该先打一次元数据。
#[derive(Clone, Debug)]
pub struct SearchPage { pub from: usize, pub size: usize, pub discovery: bool }
impl Default for SearchPage { fn default() -> Self { SearchPage { from: 0, size: 20, discovery: false } } }

// 首次进入第三方页的默认发现词（与包名规范同一个前缀，用户不用先认识任何包名）。
const DISCOVERY_QUERY: &str = "fish-dock-plugin";
// 一页最多要多少条；npm /-/v1/search 的 size 上限是 250，这里取小值防止一次拉爆。
const MAX_PAGE_SIZE: usize = 50;

#[derive(Deserialize)]
struct SearchAnswer { #[serde(default)] total: Option<u64>, objects: Vec<SearchObject> }
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
// pub(crate)：plugin_store::hop 的下载跳转判据复用同一套（#68 S2 / t18 F2），
// 不许再写第二份走样的实现。
pub(crate) fn host_within_site(host: &str, source_host: &str) -> bool {
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
fn get(url: &str, limit: usize, log: &mut Vec<String>, progress: Option<&str>) -> Result<Vec<u8>, String> {
    let sources = NpmSources::from_config();
    get_via(&sources, url, limit, log, progress)
}

// get 的注入版（测试用）：源链由参数给出，不读 config。
fn get_via(sources: &NpmSources, url: &str, limit: usize, log: &mut Vec<String>, progress: Option<&str>) -> Result<Vec<u8>, String> {
    get_via_with(sources, url, limit, log, |url, limit| get_one(url, limit, progress))
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

// 2.0.4 T-F（resources，审计 A05/A07）：一次对「逻辑地址」的 GET。候选（原地址
// + 镜像，见 sources::candidates）与每候选的代理路统一在 sources 里展开；这里只
// 保留既有分块进度与取消，错误仍按类别说人话（每候选原因由 sources 聚合）。
fn get_one(url: &str, limit: usize, progress: Option<&str>) -> Result<Vec<u8>, String> {
    crate::sources::fetch_bytes_with(url, crate::config::pet_timeout(), limit as u64, |candidate, limit| single_get(candidate, limit as usize, progress))
        .map_err(|fail| fail.reason())
}

// 单一候选的一次 GET：各 Way 依次试（web_ways），错误按类别报——
// DNS 解析失败、TLS、超时、HTTP 状态、读超限分开说，不笼统「连接失败」。
fn single_get(url: &str, limit: usize, progress: Option<&str>) -> Result<Vec<u8>, String> {
    let mut failures: Vec<String> = Vec::new();
    for way in crate::proxy::web_ways(url) {
        let Some(agent) = way.agent_with(crate::config::pet_timeout(), |config| config.max_redirects(crate::config::http_redirects() as u32).http_status_as_error(false)) else { continue };
        match agent.get(url).header("User-Agent", "FISH-Dock-Plugin-Store").header("Accept", "application/json").call() {
            Ok(mut response) => {
                let status = response.status().as_u16();
                if status != 200 { failures.push(http_status_words(status)); continue; }
                let total = response.headers().get("Content-Length").and_then(|value| value.to_str().ok()).and_then(|value| value.parse::<u64>().ok());
                if let Some(id) = progress { crate::plugin_progress::know_total(id, total); }
                let mut reader = response.body_mut().with_config().limit(limit as u64).reader();
                match crate::plugin_progress::read_chunked(progress, &mut reader, limit as u64) {
                    Ok(bytes) => return Ok(bytes),
                    Err(e) if e == "已取消" || e.contains("超过大小限制") => return Err(e),
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
#[cfg(test)]
fn tls_words(detail: &str) -> String {
    format!("TLS 握手失败（可能：代理拦截 / 端口不是 https / 证书不被信任）：{detail}")
}

// ureq 错误 → 一句话人话（保类别）。2.0.4 T-F（resources）：实现只留在
// `sources::net_words` 一处，第三方插件、官方商店、Windhawk 与 PawnIO 共用，
// 不再各写一套正则或各留一个 match。
fn npm_plain_error(e: &ureq::Error) -> String {
    crate::sources::net_words(e)
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

// #72：官方插件在 npm 上也有合规包名（`@fish-under-sea/fish-dock-plugin-<slug>`），
// 默认发现词 `fish-dock-plugin` 一搜就把这三个排在前面。它们**不是**第三方：
// 身份由官方目录授予（`Source::OfficialCatalog` + 主仓 Star），走 npm 这条通道会被
// 登记成第三方身份、装完也和商店条目对不上；搜索结果里重复出现更会让用户以为
// 「商店和 npm 是两套东西」。所以第三方页与安装口一律排除。
//
// 判据只认官方那份包名：`@fish-under-sea/fish-dock-plugin-magpie`。无 scope 的
// `fish-dock-plugin-magpie` 或 `@acme/fish-dock-plugin-magpie` 不是官方包，照常按
// 第三方处理——排除的依据是「官方身份」，不是「名字里带 magpie」。
// 官方身份与包名构造都取自 plugin_store（`OFFICIAL_IDS` / `npm_package`），
// 这里不另抄一份数组或前缀。
fn official_package(name: &str) -> Option<&'static str> {
    let name = name.trim();
    crate::plugin_store::OFFICIAL_IDS.iter().copied()
        .find(|id| crate::plugin_store::npm_package(id).eq_ignore_ascii_case(name))
}

// 命中官方包时说给人听的理由：检索计数与安装拒绝共用一句，不各写一份。
fn official_words(id: &str) -> String {
    format!("`{id}` 是官方插件：请到「官方插件」页安装或更新；第三方页只收第三方包")
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
    search_page(query, &SearchPage::default())
}

// 一页检索（T-C）：分页、发现式浏览与计数一起进来。默认页 = 2.0.3 的行为。
pub fn search_page(query: &str, page: &SearchPage) -> Result<SearchReport, String> {
    let sources = NpmSources::from_config();
    let mut log = Vec::new();
    let result = search_page_via(&sources, query, page, &mut log, |url, limit| get_one(url, limit, None));
    // 精确查与 search 两段、每一源各自的结局都打进日志，不吞。
    drain_log(&log);
    result
}

// 元数据地址：urlencode 后还原 @scope 的斜杠与 @（npmmirror 承重要求，勿删）。
fn metadata_url(primary: &str, name: &str) -> String {
    format!("{}/{}", primary, urlencode(name).replace("%2F", "/").replace("%40", "@"))
}

// search 的注入版（测试用，默认页）：源链与单地址 GET 都注入，不读 config、不碰网络。
fn search_via<F>(sources: &NpmSources, query: &str, log: &mut Vec<String>, fetch: F) -> Result<SearchReport, String>
where F: FnMut(&str, usize) -> Result<Vec<u8>, String> {
    search_page_via(sources, query, &SearchPage::default(), log, fetch)
}

// 分页版的注入实现。第一页且非发现式时先做精确包名元数据查；分页只追加本页，
// 不重复那条精确命中（from>0 时它就是上一页的事）。
fn search_page_via<F>(sources: &NpmSources, query: &str, page: &SearchPage, log: &mut Vec<String>, mut fetch: F) -> Result<SearchReport, String>
where F: FnMut(&str, usize) -> Result<Vec<u8>, String> {
    let raw = query.trim();
    if raw.len() > 120 { return Err("请输入要检索的包名或关键字".into()); }
    let text = if raw.is_empty() {
        if !page.discovery { return Err("请输入要检索的包名或关键字".into()); }
        DISCOVERY_QUERY
    } else { raw };
    let size = page.size.clamp(1, MAX_PAGE_SIZE);
    let from = page.from;
    let mut hits: Vec<Hit> = Vec::new();
    let mut hidden = 0usize;
    // 本页判定为官方插件、被排除掉的条数（#72）：与 hidden 分开计数，两者的理由
    // 完全不同——「命名不合规」和「这是官方插件」不能用同一个数字糊在一起。
    let mut official = 0usize;
    // 本页通过命名规范、可展示的条数（去重前）——与 registry 的 total 区分开。
    let mut filtered = 0usize;
    let mut seen: BTreeSet<String> = BTreeSet::new();

    // ① 精确包名优先：查询串本身是合法 npm 包名时先按元数据精确查（主源失败走镜像）。
    // 命中就置顶；404 / 网络失败都不致命——静默回退到 search（错误分类由 search 失败体现）。
    // 发现式浏览与第二页起都不做：前者不是点名要包，后者那条命中已在第一页。
    if !page.discovery && from == 0 {
        if let Ok(name) = package_name(text) {
            let url = metadata_url(&sources.primary, &name);
            if let Ok(bytes) = get_via_with(sources, &url, META_LIMIT, log, &mut fetch) {
                if let Ok(meta) = serde_json::from_slice::<Meta>(&bytes) {
                    if let Some(latest) = meta.dist_tags.get("latest").cloned() {
                        if official_package(&name).is_some() {
                            // #72：用户点名搜官方包也不在第三方页展示，只计数（安装口另有明确拒绝）。
                            official += 1;
                        } else if plugin_package_name(&name).is_ok() {
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
    }

    // ② 并入 search；按包名去重（精确那条优先，同名不再出现第二条）。
    // 第一页的 URL 与 2.0.3 逐字一致（不带 from），老回归可以照旧对照。
    let url = if from > 0 {
        format!("{}/-/v1/search?text={}&size={}&from={}", sources.primary, urlencode(text), size, from)
    } else {
        format!("{}/-/v1/search?text={}&size={}", sources.primary, urlencode(text), size)
    };
    let bytes = get_via_with(sources, &url, SEARCH_LIMIT, log, &mut fetch)?;
    let answer: SearchAnswer = serde_json::from_slice(&bytes).map_err(|_| "npm 检索结果格式无效")?;
    // registry 的页偏移按**本页实际展示的对象数**前进（过滤与去重都不改变它，
    // 但也不能超过请求的 size）——这是前端翻页唯一正确的 next_from
    // （t19 F2：用可见条数会把整页被过滤的页原地卡死；t42 F-B：registry 返回的
    // 对象多于 size 时，按整个数组前进会跳过没展示给用户的那一段）。
    let rows = answer.objects.len().min(size);
    for object in answer.objects.into_iter().take(size) {
        let row = object.package;
        // #72：官方三个插件的 npm 包先排除、单独计数——它们的包名本身合规，
        // 混进 hidden 的「不符合命名规范」里等于把理由说错。
        if official_package(&row.name).is_some() { official += 1; continue; }
        if plugin_package_name(&row.name).is_err() { hidden += 1; continue; }
        // 去重后再计数：精确命中已置顶的同名包不会再算一次「本页可用」（t19 F5）。
        if !seen.insert(row.name.clone()) { continue; }
        filtered += 1;
        hits.push(Hit {
            publisher: row.publisher.map(|p| p.username).unwrap_or_default(),
            description: truncate(&row.description, 240),
            updated: truncate(&row.date, 40),
            name: row.name,
            version: row.version,
        });
    }
    let total = answer.total.map(|n| n as usize).unwrap_or(0);
    let next_from = from.saturating_add(rows);
    // registry 报了总数就按总数判；没报时只能看这一页有没有装满。
    let has_more = if total > 0 { next_from < total } else { rows >= size };
    Ok(SearchReport { hits, hidden, official, total, filtered, has_more, next_from })
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
    let result = resolve_via(&sources, name, version, &mut log, |url, limit| get_one(url, limit, None));
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
    // #72：官方三个插件的 npm 包同样在这里拒绝——不是「搜不到就算了」，而是
    // 明确告诉用户去哪儿装。零网络，也不给「装成第三方身份」留后门。
    if let Some(id) = official_package(name) { return Err(official_words(id)); }
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
    let bytes = get(&tarball, ARCHIVE_LIMIT as usize, &mut log, Some(value))?;
    drain_log(&log);
    let digest = format!("{:x}", Sha256::digest(&bytes));
    crate::plugin_progress::report(value,"校验", bytes.len() as u64, None)?;
    let files = unpack(&bytes)?;
    crate::plugin_progress::report(value,"解包", bytes.len() as u64, None)?;
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

    // ── #72：官方三个插件不许当成第三方 ───────────────────────────────
    #[test]
    fn the_three_official_plugins_are_never_third_party() {
        use std::cell::Cell;
        for id in crate::plugin_store::OFFICIAL_IDS {
            let package = crate::plugin_store::npm_package(id);
            assert_eq!(official_package(&package), Some(id), "{package} 应被认成官方 {id}");
            // 大小写无关：npm 名只有小写，但用户手打大写时也得认出来。
            assert_eq!(official_package(&package.to_ascii_uppercase()), Some(id), "{package} 大写也应被认出");
            assert!(official_words(id).contains(id), "提醒文案要带官方身份：{}", official_words(id));
        }
        // 只认官方那份包名：别的 scope、无 scope 的同名包都不是官方包，照常按第三方处理。
        for third in ["fish-dock-plugin-magpie", "@acme/fish-dock-plugin-magpie", "fish-dock-plugin-filemgr"] {
            assert_eq!(official_package(third), None, "{third} 不是官方包");
        }

        // 检索：发现词 `fish-dock-plugin` 会把官方三个排在前面——全部排除并单独计数；
        // 合规第三方照常留下，hidden（命名不合规）不该被官方包污染。
        let sources = NpmSources::new("https://registry.npmjs.org", "https://registry.npmmirror.com");
        let search = search_fixture(serde_json::json!([
            { "package": { "name": "@fish-under-sea/fish-dock-plugin-clash", "version": "2.1.5", "description": "c", "date": "2026-10-01" } },
            { "package": { "name": "@fish-under-sea/fish-dock-plugin-magpie", "version": "2.1.6", "description": "m", "date": "2026-10-01" } },
            { "package": { "name": "@fish-under-sea/fish-dock-plugin-feishu", "version": "2.1.5", "description": "f", "date": "2026-10-01" } },
            { "package": { "name": "fish-dock-plugin-filemgr", "version": "1.0.10", "description": "file manager", "date": "2026-10-01" } },
            { "package": { "name": "lodash", "version": "4.17.21", "description": "not a plugin", "date": "2026-10-01" } }
        ]));
        let mut log = Vec::new();
        let report = search_via(&sources, "fish-dock-plugin", &mut log, |_, _| Ok(search.clone())).unwrap();
        assert_eq!(report.hits.iter().map(|hit| hit.name.as_str()).collect::<Vec<_>>(), ["fish-dock-plugin-filemgr"], "{:?}", report.hits);
        assert_eq!(report.official, 3, "三个官方包都要被认出来并单独计数");
        assert_eq!(report.hidden, 1, "只有真正命名不合规的 lodash 进 hidden");
        assert_eq!(report.filtered, 1);

        // 安装口：官方包名在 resolve_via 就拒绝，一次网络请求都不发（不是静默搜不到）。
        let asked = Cell::new(0usize);
        let mut log = Vec::new();
        let error = resolve_via(&sources, "@fish-under-sea/fish-dock-plugin-clash", None, &mut log, |_, _| {
            asked.set(asked.get() + 1);
            Ok(b"{}".to_vec())
        })
        .unwrap_err();
        assert_eq!(asked.get(), 0, "官方包不许发起任何请求");
        assert!(log.is_empty(), "不该有任何取数日志：{log:?}");
        assert!(error.contains("官方插件") && error.contains("fish.clash"), "拒绝理由要指回官方商店：{error}");
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

    // ── 分页与发现式浏览（T-C / A10）────────────────────────────────
    fn page(from: usize, size: usize, discovery: bool) -> SearchPage { SearchPage { from, size, discovery } }
    fn paged_fixture(total: u64, count: usize) -> Vec<u8> {
        let objects: Vec<serde_json::Value> = (0..count).map(|i| {
            // 每 10 个里放 1 个不合规名字，用来验证 hidden/filtered 是两个数。
            let name = if i % 10 == 9 { format!("lodash-{i:02}") } else { format!("fish-dock-plugin-p{i:02}") };
            serde_json::json!({ "package": { "name": name, "version": "1.0.0", "description": "d", "date": "2026" } })
        }).collect();
        serde_json::json!({ "total": total, "objects": objects }).to_string().into_bytes()
    }

    // 发现式浏览：空查询用默认前缀，且不先打一次精确元数据（用户没点名要包）。
    #[test]
    fn discovery_uses_the_default_prefix_and_skips_the_exact_lookup() {
        let sources = NpmSources::new("https://registry.npmjs.org", "https://registry.npmmirror.com");
        let search = paged_fixture(1, 1);
        let asked = std::cell::RefCell::new(Vec::<String>::new());
        let mut log = Vec::new();
        let report = search_page_via(&sources, "   ", &page(0, 20, true), &mut log, |url, _| {
            asked.borrow_mut().push(url.to_string());
            Ok(search.clone())
        }).unwrap();
        let asked = asked.into_inner();
        assert_eq!(asked.len(), 1, "发现式浏览只该发 search 一次：{asked:?}");
        assert!(asked[0].contains("/-/v1/search") && asked[0].contains("text=fish-dock-plugin&size=20"), "{}", asked[0]);
        assert_eq!(report.hits[0].name, "fish-dock-plugin-p00");
    }

    // 非发现式的空查询仍然是错误：不是每个空输入都该变成「浏览全部」。
    #[test]
    fn an_empty_query_without_discovery_is_still_an_error() {
        let sources = NpmSources::new("https://registry.npmjs.org", "https://registry.npmmirror.com");
        let mut log = Vec::new();
        let error = search_page_via(&sources, "", &page(0, 20, false), &mut log, |_, _| Ok(paged_fixture(0, 0))).unwrap_err();
        assert!(error.contains("请输入要检索的包名或关键字"), "{error}");
    }

    // 第二页：带 from、不重复精确查（那条命中已在第一页），并夹住 size。
    #[test]
    fn a_later_page_asks_for_from_and_repeats_no_exact_lookup() {
        let sources = NpmSources::new("https://registry.npmjs.org", "https://registry.npmmirror.com");
        let asked = std::cell::RefCell::new(Vec::<String>::new());
        let mut log = Vec::new();
        search_page_via(&sources, "fish-dock-plugin-filemgr", &page(20, 0, false), &mut log, |url, _| {
            asked.borrow_mut().push(url.to_string());
            Ok(paged_fixture(41, 1))
        }).unwrap();
        let asked = asked.into_inner();
        assert_eq!(asked.len(), 1, "第二页只发 search 一次（精确查属于第一页）：{asked:?}");
        assert!(asked[0].contains("size=1&from=20"), "size 要夹到 ≥1：{}", asked[0]);
    }

    // registry 的总数、本页过滤后可展示的条数、还有没有下一页：三个数各归各。
    #[test]
    fn the_page_separates_registry_total_filtered_count_and_has_more() {
        let sources = NpmSources::new("https://registry.npmjs.org", "https://registry.npmmirror.com");
        let mut log = Vec::new();
        let first = search_page_via(&sources, "fish-dock-plugin", &page(0, 20, true), &mut log, |_, _| Ok(paged_fixture(41, 20))).unwrap();
        assert_eq!(first.total, 41, "registry 报的总数要原样带出");
        assert_eq!(first.hidden, 2, "两条不合规名字");
        assert_eq!(first.filtered, 18, "本页通过命名规范、去重后可展示的条数");
        assert_eq!(first.hits.len(), 18);
        assert!(first.has_more, "0+20 < 41，后面还有");
        let last = search_page_via(&sources, "fish-dock-plugin", &page(40, 20, true), &mut log, |_, _| Ok(paged_fixture(41, 20))).unwrap();
        assert!(!last.has_more, "40+20 ≥ 41，到底了");
    }

    // 整页名字都不合规的夹具：用来验证「整页被过滤时偏移仍前进」。
    fn all_hidden_fixture(total: u64, count: usize) -> Vec<u8> {
        let objects: Vec<serde_json::Value> = (0..count)
            .map(|i| serde_json::json!({ "package": { "name": format!("lodash-{i:02}"), "version": "1.0.0", "description": "d", "date": "2026" } }))
            .collect();
        serde_json::json!({ "total": total, "objects": objects }).to_string().into_bytes()
    }

    // t19 F2：翻页偏移是 registry 的**对象**偏移，不是可见条数——
    // 本页 20 个对象里有 2 个被过滤，下一页仍要从 20 开始；整页被过滤时更要前进，
    // 否则「加载更多」永远返回同一页（前端已按 nextFrom 走，这里锁后端口径）。
    #[test]
    fn next_from_advances_by_registry_objects_not_by_visible_hits() {
        let sources = NpmSources::new("https://registry.npmjs.org", "https://registry.npmmirror.com");
        let mut log = Vec::new();
        let paged = search_page_via(&sources, "fish-dock-plugin", &page(0, 20, true), &mut log, |_, _| Ok(paged_fixture(41, 20))).unwrap();
        assert_eq!(paged.hits.len(), 18, "可见 18 条");
        assert_eq!(paged.next_from, 20, "偏移按对象数前进：18 条可见也要从 20 接着翻");
        // 整页都被过滤：可见 0 条，偏移照旧前进 20。
        let hidden_page = all_hidden_fixture(41, 20);
        let empty = search_page_via(&sources, "fish-dock-plugin", &page(0, 20, true), &mut log, |_, _| Ok(hidden_page.clone())).unwrap();
        assert!(empty.hits.is_empty(), "整页过滤后没有可安装条目");
        assert_eq!(empty.hidden, 20);
        assert_eq!(empty.next_from, 20, "整页被过滤也必须前进，否则前端原地打转");
        assert!(empty.has_more, "20 < 41，后面还有");
    }

    // t42 F-B：registry 返回的对象多于请求的 size 时，没展示给用户的那一段
    // 不能被跳过——next_from 只按本页实际消费（=take(size)）的条数前进。
    #[test]
    fn next_from_never_advances_past_what_the_page_shows() {
        let sources = NpmSources::new("https://registry.npmjs.org", "https://registry.npmmirror.com");
        let mut log = Vec::new();
        // 请求 20，registry 却给了 25 个对象：页面只消费 20 个（其中 2 个不合规 → 18 可见）。
        let paged = search_page_via(&sources, "fish-dock-plugin", &page(0, 20, true), &mut log, |_, _| Ok(paged_fixture(41, 25))).unwrap();
        assert_eq!(paged.hits.len(), 18, "只消费前 size=20 个对象（每 10 个有 1 个不合规）");
        assert_eq!(paged.next_from, 20, "多余 5 个对象没展示，不能被跳过");
        // 短页：本页只有 5 个对象，total 仍说后面还有，偏移只能前进 5。
        let short = search_page_via(&sources, "fish-dock-plugin", &page(20, 20, true), &mut log, |_, _| Ok(paged_fixture(41, 5))).unwrap();
        assert_eq!(short.hits.len(), 5);
        assert_eq!(short.next_from, 25, "短页按实际 5 条前进（不是 40）");
        assert!(short.has_more, "25 < 41，后面还有");
    }

    // t19 F5：filtered 是「去重后可展示」的条数，精确命中与 search 里同名的那条
    // 不能算两次（旧实现把 filtered += 1 放在去重之前，多算 1）。
    #[test]
    fn filtered_counts_each_package_once_after_dedupe() {
        let sources = NpmSources::new("https://registry.npmjs.org", "https://registry.npmmirror.com");
        let search = search_fixture(serde_json::json!([
            { "package": { "name": "fish-dock-plugin-filemgr", "version": "1.0.5", "description": "dup", "date": "2026-10-21" } },
            { "package": { "name": "fish-dock-plugin-other", "version": "2.0.0", "description": "o", "date": "2026-10-01" } }
        ]));
        let meta = meta_fixture("fish-dock-plugin-filemgr", "1.0.5");
        let mut log = Vec::new();
        let report = search_via(&sources, "fish-dock-plugin-filemgr", &mut log, |url, _| {
            if url.contains("/-/v1/search") { Ok(search.clone()) } else { Ok(meta.clone()) }
        }).unwrap();
        assert_eq!(report.hits.len(), 2, "置顶的 filemgr + other");
        assert_eq!(report.filtered, 1, "search 段只有 other 是新条目：同名不许算两次");
        assert_eq!(report.next_from, 2, "偏移仍按本页对象数（2）前进");
    }

    // registry 没报总数时只能看这一页有没有装满——装满就当还有，空页就结束。
    #[test]
    fn without_a_registry_total_a_full_page_still_suggests_more() {
        let sources = NpmSources::new("https://registry.npmjs.org", "https://registry.npmmirror.com");
        let mut log = Vec::new();
        let full = search_page_via(&sources, "fish-dock-plugin", &page(0, 2, true), &mut log, |_, _| Ok(paged_fixture(0, 2))).unwrap();
        assert!(full.has_more, "满页且无 total：按「可能还有」处理");
        let short = search_page_via(&sources, "fish-dock-plugin", &page(0, 2, true), &mut log, |_, _| Ok(paged_fixture(0, 1))).unwrap();
        assert!(!short.has_more, "不满一页：到底了");
    }
}
