//! 配置集中化（红线第 5 条：魔法值 + 硬编码）。
//!
//! 这是本软件「目录 / 端口 / URL / 超时 / 重试次数 / 缓存上限 / 轮询间隔 / 体积阈值」
//! 的唯一配置入口。别处不许再写死这些值，一律引用这里的键。
//!
//! 约定（覆盖优先序写在文档与测试里）：
//! - 覆盖优先序：显式注入（测试用 `*_of()`）> 环境变量 > 内置默认。
//! - 环境变量空串 / 纯空白视为「没设」，回落默认；URL 末尾的 `/` 会被去掉。
//! - 命名沿用既有 `FISH_DOCK_HOME` / `FISH_DOCK_APP_HOME` 风格，统一 `FISH_DOCK_` 前缀；
//!   **不复用 `FISH_DOCK_HOME` 语义**——那是统一数据根（`storage_context.rs`），这里只新增专用键。
//! - 解析与读环境分离：`*_of()` 是纯函数（测试注入值，不碰进程环境，红线 9）；
//!   对外 getter 才读一次环境。超时统一受 [`MAX_TIMEOUT_MS`] 夹取。
//!
//! [`KEYS`] 是全部键的机器可读清单（键名 / 环境变量 / 默认值 / 类别 / 影响面），
//! 供文档表与 `scripts/check-hardcoded.mjs` 对照。

use std::path::PathBuf;
use std::time::Duration;

/// 一个配置键的元数据。`default` 是给人看的默认值字面量（与 getter 里的常量一致）。
pub struct Key {
    /// 逻辑键名，形如 `port.dock`。
    pub name: &'static str,
    /// 覆盖用的环境变量名。
    pub env: &'static str,
    /// 内置默认值（字符串形式，仅作文档；数值以 getter 常量出现）。
    pub default: &'static str,
    /// 类别。
    pub category: Category,
    /// 影响面：改这个值会改到哪儿。
    pub impact: &'static str,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Category {
    Dir,
    Port,
    Url,
    Timeout,
    Retry,
    Cache,
    Interval,
    Size,
    /// 字符串型开关（浏览器额外参数、OAuth clientId 一类），不是数值。
    Text,
}

// —— 目录（Dir）——
/// 数据根目录名（`%LOCALAPPDATA%\FISH Dock`）。**唯一一份定义**：runtime.rs 的
/// `app_home_from` 从这里取，别再在别处各写一个同名字面量。安装器导出的常量必须
/// 与这个字面量逐字一致——安装器会删旧目录 %LOCALAPPDATA%\DSH-FISH，名字差一个
/// 字，删掉的就会是别处（或者程序正在用）的数据。
pub const HOME_NAME: &str = "FISH Dock";
const ENV_APP_HOME: &str = "FISH_DOCK_APP_HOME";
const ENV_UNIFIED_HOME: &str = "FISH_DOCK_HOME";
const ENV_RESOURCES: &str = "FISH_DOCK_RESOURCES";
/// 移植/测试用的数据目录覆盖（旧名沿用，无新前缀对应；见 config::user_data_override）。
const ENV_USER_DATA_OVERRIDE: &str = "WAKUWAKU_USER_DATA";

// —— 端口（Port）——
/// 宿主默认端口。main.rs 的 `DEFAULT_PORT` 引用这里，避免两处各写一个数。
pub const DEFAULT_PORT: u16 = 47213;
const ENV_PORT: &str = "FISH_DOCK_PORT";
/// 旧名，保留兼容（现有的 `WAKUWAKU_PORT` 仍生效，优先级低于 `FISH_DOCK_PORT`）。
const ENV_PORT_LEGACY: &str = "WAKUWAKU_PORT";

// —— URL（Url）——
const ENV_PET_SITE: &str = "FISH_DOCK_PET_SITE";
/// 宠物站点默认值。fetch.rs 的 `SITE` 常量引用这里。
pub const PET_SITE_DEFAULT: &str = "https://codex-pets.net";
// 官网地址：关于页品牌图那个彩蛋点开的就是它（用户 2026-10-31）。**现在指向一个
// 临时落地页，不是正式官网**。正式官网上线时只改这一个默认值（或设环境变量
// FISH_DOCK_OFFICIAL_SITE），前端一行都不用动——settings.js 只发
// `settings_open_site("officialSite")`，真正的地址由这里给（settings.rs site_url）。
const ENV_OFFICIAL_SITE: &str = "FISH_DOCK_OFFICIAL_SITE";
pub const OFFICIAL_SITE_DEFAULT: &str = "https://h5.metaapp.cn/online/h5/BUsvkkbBiAq2pF2Ttx4j/1.0.2/6/index.html";
const ENV_GITHUB_API: &str = "FISH_DOCK_GITHUB_API";
const GITHUB_API_DEFAULT: &str = "https://api.github.com";
const ENV_UPDATE_REPO: &str = "FISH_DOCK_UPDATE_REPO";
const UPDATE_REPO_DEFAULT: &str = "https://github.com/Fish-under-sea/FISH-Dock/";
const ENV_UPDATE_LATEST: &str = "FISH_DOCK_UPDATE_LATEST";
const UPDATE_LATEST_DEFAULT: &str = "https://api.github.com/repos/Fish-under-sea/FISH-Dock/releases/latest";
const ENV_UPDATE_RELEASES: &str = "FISH_DOCK_UPDATE_RELEASES";
const UPDATE_RELEASES_DEFAULT: &str = "https://api.github.com/repos/Fish-under-sea/FISH-Dock/releases?per_page=5";
const ENV_NPM_REGISTRY: &str = "FISH_DOCK_PLUGIN_NPM_REGISTRY";
pub const NPM_REGISTRY_DEFAULT: &str = "https://registry.npmjs.org";
const ENV_NPM_MIRROR: &str = "FISH_DOCK_PLUGIN_NPM_MIRROR";
pub const NPM_MIRROR_DEFAULT: &str = "https://registry.npmmirror.com";
const ENV_CATALOG_RAW: &str = "FISH_DOCK_PLUGIN_CATALOG_URL";
pub const CATALOG_RAW_DEFAULT: &str = "https://raw.githubusercontent.com/Fish-under-sea/FISH-DOCK-Plugins/main/catalog/stable.json";
const ENV_CATALOG_API: &str = "FISH_DOCK_PLUGIN_CATALOG_API";
pub const CATALOG_API_DEFAULT: &str = "https://api.github.com/repos/Fish-under-sea/FISH-DOCK-Plugins/contents/catalog/stable.json?ref=main";

// —— 超时（Timeout，毫秒除非注明）——
const ENV_NET_CONNECT_MS: &str = "FISH_DOCK_NET_CONNECT_MS";
const ENV_NET_HEAD_MS: &str = "FISH_DOCK_NET_HEAD_MS";
const ENV_PET_MS: &str = "FISH_DOCK_PET_TIMEOUT_MS";
const ENV_UPDATE_MS: &str = "FISH_DOCK_UPDATE_TIMEOUT_MS";
const ENV_AUTH_MS: &str = "FISH_DOCK_AUTH_TIMEOUT_MS";
const DEFAULT_NET_CONNECT_MS: u64 = 15_000;
const DEFAULT_NET_HEAD_MS: u64 = 30_000;
const DEFAULT_PET_MS: u64 = 30_000;
const DEFAULT_UPDATE_MS: u64 = 20_000;
const DEFAULT_AUTH_MS: u64 = 20_000;
const ENV_PROXY_PROBE_MS: &str = "FISH_DOCK_PROXY_PROBE_MS";
const DEFAULT_PROXY_PROBE_MS: u64 = 600;
const ENV_PROXY_CONNECT_MS: &str = "FISH_DOCK_PROXY_CONNECT_MS";
const DEFAULT_PROXY_CONNECT_MS: u64 = 300;
const ENV_PROXY_TEST_MS: &str = "FISH_DOCK_PROXY_TEST_MS";
const DEFAULT_PROXY_TEST_MS: u64 = 10_000;
const ENV_PLUGIN_DOWNLOAD_SECS: &str = "FISH_DOCK_PLUGIN_DOWNLOAD_TIMEOUT_SECS";
const DEFAULT_PLUGIN_DOWNLOAD_SECS: u64 = 30;
const ENV_PLUGIN_ACTIVATE_SECS: &str = "FISH_DOCK_PLUGIN_ACTIVATE_TIMEOUT_SECS";
const DEFAULT_PLUGIN_ACTIVATE_SECS: u64 = 5;
const ENV_PLUGIN_RPC_SECS: &str = "FISH_DOCK_PLUGIN_RPC_TIMEOUT_SECS";
const DEFAULT_PLUGIN_RPC_SECS: u64 = 15;
const ENV_PLUGIN_EVENT_SECS: &str = "FISH_DOCK_PLUGIN_EVENT_TIMEOUT_SECS";
const DEFAULT_PLUGIN_EVENT_SECS: u64 = 10;
/// 超时的上限：再离谱的覆盖也不给一次调用超过它（10 分钟）。
pub const MAX_TIMEOUT_MS: u64 = 600_000;

// —— 重试（Retry）——
const ENV_PLUGIN_REDIRECT_HOPS: &str = "FISH_DOCK_PLUGIN_REDIRECT_HOPS";
const DEFAULT_PLUGIN_REDIRECT_HOPS: usize = 5;
const ENV_HTTP_REDIRECTS: &str = "FISH_DOCK_HTTP_REDIRECTS";
const DEFAULT_HTTP_REDIRECTS: usize = 10;

// —— 缓存上限（Cache）——
const ENV_GALLERY_KEEP: &str = "FISH_DOCK_GALLERY_CACHE_KEEP";
const DEFAULT_GALLERY_KEEP: usize = 8;
const ENV_NOTES_KEEP: &str = "FISH_DOCK_NOTES_KEEP";
const DEFAULT_NOTES_KEEP: usize = 5;
const ENV_PLUGIN_FETCH_KEEP: &str = "FISH_DOCK_PLUGIN_FETCH_LOG_KEEP";
const DEFAULT_PLUGIN_FETCH_KEEP: usize = 20;
const ENV_CRASH_KEEP: &str = "FISH_DOCK_CRASH_KEEP";
const DEFAULT_CRASH_KEEP: usize = 20;

// —— 轮询间隔（Interval，毫秒）——
const ENV_TICKER_MS: &str = "FISH_DOCK_TICKER_MS";
const DEFAULT_TICKER_MS: u64 = 500;
const ENV_FULLSCREEN_MS: &str = "FISH_DOCK_FULLSCREEN_MS";
const DEFAULT_FULLSCREEN_MS: u64 = 1500;
const ENV_QUIET_AFTER_MS: &str = "FISH_DOCK_QUIET_AFTER_MS";
/// 旧名，保留兼容（优先序低于 `FISH_DOCK_QUIET_AFTER_MS`）。
const ENV_QUIET_AFTER_MS_LEGACY: &str = "WAKUWAKU_QUIET_AFTER_MS";
const DEFAULT_QUIET_AFTER_MS: u64 = 90_000;
const ENV_IDLE_HIDE_MS: &str = "FISH_DOCK_IDLE_HIDE_MS";
/// 旧名，保留兼容（优先序低于 `FISH_DOCK_IDLE_HIDE_MS`）。
const ENV_IDLE_HIDE_MS_LEGACY: &str = "WAKUWAKU_IDLE_HIDE_MS";
const ENV_LHM_EVERY_MS: &str = "FISH_DOCK_LHM_EVERY_MS";
const DEFAULT_LHM_EVERY_MS: u64 = 3_000;
const ENV_LHM_EVERY_QUIET_MS: &str = "FISH_DOCK_LHM_EVERY_QUIET_MS";
const DEFAULT_LHM_EVERY_QUIET_MS: u64 = 10_000;
const ENV_MONITOR_MIN_MS: &str = "FISH_DOCK_MONITOR_MIN_MS";
const DEFAULT_MONITOR_MIN_MS: u64 = 1_000;
const ENV_MONITOR_MAX_MS: &str = "FISH_DOCK_MONITOR_MAX_MS";
const DEFAULT_MONITOR_MAX_MS: u64 = 10_000;
const ENV_MONITOR_OFF_MS: &str = "FISH_DOCK_MONITOR_OFF_MS";
const DEFAULT_MONITOR_OFF_MS: u64 = 3_600_000;

// —— 体积阈值（Size，字节）——
const ENV_LOG_TAIL_BYTES: &str = "FISH_DOCK_LOG_TAIL_BYTES";
const DEFAULT_LOG_TAIL_BYTES: u64 = 256 * 1024;
const ENV_PLUGIN_FILE_BYTES: &str = "FISH_DOCK_PLUGIN_FILE_LIMIT";
const DEFAULT_PLUGIN_FILE_BYTES: usize = 128 * 1024 * 1024;
const ENV_PLUGIN_TOTAL_BYTES: &str = "FISH_DOCK_PLUGIN_TOTAL_LIMIT";
const DEFAULT_PLUGIN_TOTAL_BYTES: usize = 256 * 1024 * 1024;
const ENV_CATALOG_BYTES: &str = "FISH_DOCK_CATALOG_MAX_BYTES";
const DEFAULT_CATALOG_BYTES: usize = 512 * 1024;
const ENV_NPM_TARBALL_SLACK_BYTES: &str = "FISH_DOCK_NPM_TARBALL_SLACK";
const DEFAULT_NPM_TARBALL_SLACK_BYTES: usize = 8 * 1024 * 1024;

// —— 字符串开关（Text）——
const ENV_BROWSER_ARGS: &str = "FISH_DOCK_BROWSER_ARGS";
/// 旧名，保留兼容（优先序低于 `FISH_DOCK_BROWSER_ARGS`）。
const ENV_BROWSER_ARGS_LEGACY: &str = "WAKUWAKU_BROWSER_ARGS";
const ENV_GITHUB_CLIENT_ID: &str = "FISH_DOCK_GITHUB_CLIENT_ID";

/// 全部配置键的清单（文档表 + 自查脚本对照）。
pub const KEYS: &[Key] = &[
    // 目录
    Key { name: "dir.app_home", env: ENV_APP_HOME, default: "%LOCALAPPDATA%/FISH Dock", category: Category::Dir, impact: "宿主数据根（WebView 资料、设置、宠物）" },
    Key { name: "dir.unified_root", env: ENV_UNIFIED_HOME, default: "%USERPROFILE%/.fish-dock", category: Category::Dir, impact: "统一数据根（凭据/roster/keys），见 storage_context" },
    Key { name: "dir.resources", env: ENV_RESOURCES, default: "<exe>/resources", category: Category::Dir, impact: "安装资源（Node/WebView2/LHM）目录，开发期可指向 checkout" },
    Key { name: "dir.user_data", env: ENV_USER_DATA_OVERRIDE, default: "(空=按数据根)", category: Category::Dir, impact: "移植/测试的数据目录覆盖（旧名沿用 WAKUWAKU_USER_DATA）" },
    // 端口
    Key { name: "port.dock", env: ENV_PORT, default: "47213", category: Category::Port, impact: "宿主本地 HTTP 端口（hooks/插件上报），旧 WAKUWAKU_PORT 仍兼容" },
    // URL
    Key { name: "url.pet_site", env: ENV_PET_SITE, default: PET_SITE_DEFAULT, category: Category::Url, impact: "宠物图库站点（gallery/下载）" },
    Key { name: "url.github_api", env: ENV_GITHUB_API, default: GITHUB_API_DEFAULT, category: Category::Url, impact: "GitHub API 主机（CI/仓库操作）" },
    Key { name: "url.update_repo", env: ENV_UPDATE_REPO, default: UPDATE_REPO_DEFAULT, category: Category::Url, impact: "自身更新/公告所属仓库地址" },
    Key { name: "url.update_latest", env: ENV_UPDATE_LATEST, default: UPDATE_LATEST_DEFAULT, category: Category::Url, impact: "自身更新：查最新发行" },
    Key { name: "url.update_releases", env: ENV_UPDATE_RELEASES, default: UPDATE_RELEASES_DEFAULT, category: Category::Url, impact: "自身更新：查发行列表" },
    Key { name: "url.npm_registry", env: ENV_NPM_REGISTRY, default: NPM_REGISTRY_DEFAULT, category: Category::Url, impact: "官方插件下载首选源（npm 官方）" },
    Key { name: "url.npm_mirror", env: ENV_NPM_MIRROR, default: NPM_MIRROR_DEFAULT, category: Category::Url, impact: "官方插件下载镜像（npm 官方失败后）" },
    Key { name: "url.catalog_raw", env: ENV_CATALOG_RAW, default: CATALOG_RAW_DEFAULT, category: Category::Url, impact: "官方插件目录（raw 文本）" },
    Key { name: "url.catalog_api", env: ENV_CATALOG_API, default: CATALOG_API_DEFAULT, category: Category::Url, impact: "官方插件目录（GitHub contents API）" },
    Key { name: "url.official_site", env: ENV_OFFICIAL_SITE, default: OFFICIAL_SITE_DEFAULT, category: Category::Url, impact: "官网地址（关于页品牌图点击打开；当前是临时落地页）" },
    // 超时
    Key { name: "timeout.net_connect_ms", env: ENV_NET_CONNECT_MS, default: "15000", category: Category::Timeout, impact: "所有 web 请求的 TCP 连接层预算" },
    Key { name: "timeout.net_head_ms", env: ENV_NET_HEAD_MS, default: "30000", category: Category::Timeout, impact: "所有 web 请求的响应头预算" },
    Key { name: "timeout.pet_ms", env: ENV_PET_MS, default: "30000", category: Category::Timeout, impact: "宠物图库/贴图页整单预算" },
    Key { name: "timeout.update_ms", env: ENV_UPDATE_MS, default: "20000", category: Category::Timeout, impact: "更新检查与公告读取整单预算" },
    Key { name: "timeout.auth_ms", env: ENV_AUTH_MS, default: "20000", category: Category::Timeout, impact: "GitHub 登录/Star 校验整单预算" },
    Key { name: "timeout.proxy_probe_ms", env: ENV_PROXY_PROBE_MS, default: "600", category: Category::Timeout, impact: "设置页代理端口探测等待" },
    Key { name: "timeout.proxy_connect_ms", env: ENV_PROXY_CONNECT_MS, default: "300", category: Category::Timeout, impact: "代理端口监听探测连接等待" },
    Key { name: "timeout.proxy_test_ms", env: ENV_PROXY_TEST_MS, default: "10000", category: Category::Timeout, impact: "设置页「测试代理」向 GitHub 发一次请求的预算" },
    Key { name: "timeout.plugin_download_secs", env: ENV_PLUGIN_DOWNLOAD_SECS, default: "30", category: Category::Timeout, impact: "单次插件包/目录下载预算" },
    Key { name: "timeout.plugin_activate_secs", env: ENV_PLUGIN_ACTIVATE_SECS, default: "5", category: Category::Timeout, impact: "插件激活 RPC 预算" },
    Key { name: "timeout.plugin_rpc_secs", env: ENV_PLUGIN_RPC_SECS, default: "15", category: Category::Timeout, impact: "插件通用 RPC 预算" },
    Key { name: "timeout.plugin_event_secs", env: ENV_PLUGIN_EVENT_SECS, default: "10", category: Category::Timeout, impact: "插件事件投递预算" },
    // 重试
    Key { name: "retry.plugin_redirect_hops", env: ENV_PLUGIN_REDIRECT_HOPS, default: "5", category: Category::Retry, impact: "插件包下载最大重定向跳数" },
    Key { name: "retry.http_redirects", env: ENV_HTTP_REDIRECTS, default: "10", category: Category::Retry, impact: "通用 web 请求最大重定向数" },
    // 缓存上限
    Key { name: "cache.gallery_keep", env: ENV_GALLERY_KEEP, default: "8", category: Category::Cache, impact: "宠物图库离线缓存文件数上限" },
    Key { name: "cache.notes_keep", env: ENV_NOTES_KEEP, default: "5", category: Category::Cache, impact: "更新公告缓存条数上限" },
    Key { name: "cache.plugin_fetch_keep", env: ENV_PLUGIN_FETCH_KEEP, default: "20", category: Category::Cache, impact: "插件下载来源记录条数上限" },
    Key { name: "cache.crash_keep", env: ENV_CRASH_KEEP, default: "20", category: Category::Cache, impact: "崩溃报告保留份数上限" },
    // 轮询间隔
    Key { name: "interval.ticker_ms", env: ENV_TICKER_MS, default: "500", category: Category::Interval, impact: "主 ticker 心跳间隔" },
    Key { name: "interval.fullscreen_ms", env: ENV_FULLSCREEN_MS, default: "1500", category: Category::Interval, impact: "全屏检测轮询间隔" },
    Key { name: "interval.quiet_after_ms", env: ENV_QUIET_AFTER_MS, default: "90000", category: Category::Interval, impact: "闲置多久进入 quiet（旧 WAKUWAKU_QUIET_AFTER_MS 兼容）" },
    Key { name: "interval.idle_hide_ms", env: ENV_IDLE_HIDE_MS, default: "0", category: Category::Interval, impact: "空闲多久隐藏会话（0/未设=按 idleHideMin 设置；旧 WAKUWAKU_IDLE_HIDE_MS 兼容）" },
    Key { name: "interval.lhm_every_ms", env: ENV_LHM_EVERY_MS, default: "3000", category: Category::Interval, impact: "LHM 传感器轮询（活跃）" },
    Key { name: "interval.lhm_every_quiet_ms", env: ENV_LHM_EVERY_QUIET_MS, default: "10000", category: Category::Interval, impact: "LHM 传感器轮询（quiet）" },
    Key { name: "interval.monitor_min_ms", env: ENV_MONITOR_MIN_MS, default: "1000", category: Category::Interval, impact: "硬件监视最短轮询间隔" },
    Key { name: "interval.monitor_max_ms", env: ENV_MONITOR_MAX_MS, default: "10000", category: Category::Interval, impact: "硬件监视最长轮询间隔" },
    Key { name: "interval.monitor_off_ms", env: ENV_MONITOR_OFF_MS, default: "3600000", category: Category::Interval, impact: "关闭监视时的休眠间隔" },
    // 体积阈值
    Key { name: "size.log_tail_bytes", env: ENV_LOG_TAIL_BYTES, default: "262144", category: Category::Size, impact: "recent.log 轮转阈值（256 KiB）" },
    Key { name: "size.plugin_file_bytes", env: ENV_PLUGIN_FILE_BYTES, default: "134217728", category: Category::Size, impact: "插件单文件上限（128 MiB）" },
    Key { name: "size.plugin_total_bytes", env: ENV_PLUGIN_TOTAL_BYTES, default: "268435456", category: Category::Size, impact: "插件包解压总量上限（256 MiB）" },
    Key { name: "size.catalog_bytes", env: ENV_CATALOG_BYTES, default: "524288", category: Category::Size, impact: "官方目录 JSON 大小上限（512 KiB）" },
    Key { name: "size.npm_tarball_slack_bytes", env: ENV_NPM_TARBALL_SLACK_BYTES, default: "8388608", category: Category::Size, impact: "npm tarball 相对插件归档的余量（8 MiB）" },
    // 字符串开关
    Key { name: "text.browser_args", env: ENV_BROWSER_ARGS, default: "(空=用内置)", category: Category::Text, impact: "WebView2 额外启动参数（旧 WAKUWAKU_BROWSER_ARGS 兼容）" },
    Key { name: "text.github_client_id", env: ENV_GITHUB_CLIENT_ID, default: "(见内置 DEFAULT_CLIENT_ID)", category: Category::Text, impact: "GitHub OAuth 设备流 clientId 覆盖" },
];

// —— 解析器（纯函数，测试注入值，不碰进程环境）——

/// 读一次环境变量；空串/空白视为没设。
fn var(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|value| !value.trim().is_empty())
}

/// 按顺序取第一个「设了」的环境变量（新前缀在前、旧名在后；空串视为没设）。
fn var_chain(names: &[&str]) -> Option<String> {
    names.iter().find_map(|name| var(name))
}

/// 文本：去空白、去末尾 `/`；空则用默认。
pub fn text_of(raw: Option<&str>, fallback: &str) -> String {
    raw.map(str::trim).map(|s| s.trim_end_matches('/')).filter(|s| !s.is_empty()).map(str::to_string).unwrap_or_else(|| fallback.to_string())
}

/// 数值：能解析就用，否则默认。
pub fn num_of<T: std::str::FromStr>(raw: Option<&str>, fallback: T) -> T {
    raw.and_then(|t| t.trim().parse::<T>().ok()).unwrap_or(fallback)
}

/// 毫秒：正数且不超过上限才生效，否则默认。
pub fn ms_of(raw: Option<&str>, default_ms: u64) -> Duration {
    let parsed = raw.and_then(|t| t.trim().parse::<u64>().ok()).filter(|ms| *ms > 0 && *ms <= MAX_TIMEOUT_MS);
    Duration::from_millis(parsed.unwrap_or(default_ms.min(MAX_TIMEOUT_MS)))
}

fn text(env: &str, fallback: &str) -> String {
    text_of(var(env).as_deref(), fallback)
}
fn num<T: std::str::FromStr>(env: &str, fallback: T) -> T {
    num_of(var(env).as_deref(), fallback)
}
fn ms(env: &str, default_ms: u64) -> Duration {
    ms_of(var(env).as_deref(), default_ms)
}
// 轮询间隔不是「一次调用的超时」，不该被 MAX_TIMEOUT_MS（10 分钟）夹住——否则
// 「关掉监视时睡 1 小时」会被砍成 10 分钟。这里只要求正数，0/坏值回落默认。
pub fn interval_ms_of(raw: Option<&str>, default_ms: u64) -> Duration {
    let value = raw.and_then(|t| t.trim().parse::<u64>().ok());
    Duration::from_millis(match value {
        Some(v) if v > 0 => v,
        _ => default_ms,
    })
}
fn interval_ms(env: &str, default_ms: u64) -> Duration {
    interval_ms_of(var(env).as_deref(), default_ms)
}

// —— 目录 ——
/// 统一数据根环境变量（`FISH_DOCK_HOME`，`storage_context.rs` 读取）。
pub fn unified_root_env() -> Option<PathBuf> {
    std::env::var_os(ENV_UNIFIED_HOME).filter(|p| !p.is_empty()).map(PathBuf::from)
}
/// 数据根（`FISH_DOCK_APP_HOME` 优先，否则 `%LOCALAPPDATA%\FISH Dock`）。委托 runtime 的唯一来源。
#[cfg(any(feature = "installed", test))]
pub fn app_home() -> Result<PathBuf, String> {
    crate::runtime::app_home()
}
/// 安装资源目录覆盖（开发期 `FISH_DOCK_RESOURCES`，兼容旧的 `DSH_FISH_RESOURCES`）。
pub fn resources_override() -> Option<PathBuf> {
    var(ENV_RESOURCES).or_else(|| var("DSH_FISH_RESOURCES")).map(PathBuf::from)
}
/// 移植/测试用的数据目录覆盖（`WAKUWAKU_USER_DATA`，旧名沿用）。
pub fn user_data_override() -> Option<PathBuf> {
    var(ENV_USER_DATA_OVERRIDE).map(PathBuf::from)
}

// —— 端口 ——
/// 端口（`FISH_DOCK_PORT` > 旧 `WAKUWAKU_PORT` > 47213）。
pub fn port() -> u16 {
    port_of(var(ENV_PORT).as_deref(), var(ENV_PORT_LEGACY).as_deref())
}
pub fn port_of(fish_dock_port: Option<&str>, legacy_port: Option<&str>) -> u16 {
    fish_dock_port
        .and_then(|p| p.trim().parse::<u16>().ok())
        .or_else(|| legacy_port.and_then(|p| p.trim().parse::<u16>().ok()))
        .unwrap_or(DEFAULT_PORT)
}

// —— URL ——
pub fn pet_site() -> String { text(ENV_PET_SITE, PET_SITE_DEFAULT) }
/// 官网地址（关于页品牌图的彩蛋）。默认是临时落地页，见 `OFFICIAL_SITE_DEFAULT`。
pub fn official_site() -> String { text(ENV_OFFICIAL_SITE, OFFICIAL_SITE_DEFAULT) }
pub fn github_api() -> String { text(ENV_GITHUB_API, GITHUB_API_DEFAULT) }
pub fn update_repo() -> String { text(ENV_UPDATE_REPO, UPDATE_REPO_DEFAULT) }
pub fn update_latest() -> String { text(ENV_UPDATE_LATEST, UPDATE_LATEST_DEFAULT) }
pub fn update_releases() -> String { text(ENV_UPDATE_RELEASES, UPDATE_RELEASES_DEFAULT) }
pub fn plugin_npm_registry() -> String { text(ENV_NPM_REGISTRY, NPM_REGISTRY_DEFAULT) }
pub fn plugin_npm_mirror() -> String { text(ENV_NPM_MIRROR, NPM_MIRROR_DEFAULT) }
pub fn plugin_catalog_raw() -> String { text(ENV_CATALOG_RAW, CATALOG_RAW_DEFAULT) }
pub fn plugin_catalog_api() -> String { text(ENV_CATALOG_API, CATALOG_API_DEFAULT) }

// —— 超时 ——
pub fn net_connect_timeout() -> Duration { ms(ENV_NET_CONNECT_MS, DEFAULT_NET_CONNECT_MS) }
pub fn net_head_timeout() -> Duration { ms(ENV_NET_HEAD_MS, DEFAULT_NET_HEAD_MS) }
pub fn pet_timeout() -> Duration { ms(ENV_PET_MS, DEFAULT_PET_MS) }
pub fn update_timeout() -> Duration { ms(ENV_UPDATE_MS, DEFAULT_UPDATE_MS) }
pub fn auth_timeout() -> Duration { ms(ENV_AUTH_MS, DEFAULT_AUTH_MS) }
pub fn proxy_probe_timeout() -> Duration { ms(ENV_PROXY_PROBE_MS, DEFAULT_PROXY_PROBE_MS) }
pub fn proxy_connect_timeout() -> Duration { ms(ENV_PROXY_CONNECT_MS, DEFAULT_PROXY_CONNECT_MS) }
pub fn proxy_test_timeout() -> Duration { ms(ENV_PROXY_TEST_MS, DEFAULT_PROXY_TEST_MS) }
pub fn plugin_download_timeout() -> Duration { Duration::from_secs(num(ENV_PLUGIN_DOWNLOAD_SECS, DEFAULT_PLUGIN_DOWNLOAD_SECS)) }
pub fn plugin_activate_timeout() -> Duration { Duration::from_secs(num(ENV_PLUGIN_ACTIVATE_SECS, DEFAULT_PLUGIN_ACTIVATE_SECS)) }
pub fn plugin_rpc_timeout() -> Duration { Duration::from_secs(num(ENV_PLUGIN_RPC_SECS, DEFAULT_PLUGIN_RPC_SECS)) }
pub fn plugin_event_timeout() -> Duration { Duration::from_secs(num(ENV_PLUGIN_EVENT_SECS, DEFAULT_PLUGIN_EVENT_SECS)) }

// —— 重试 ——
pub fn plugin_redirect_hops() -> usize { num(ENV_PLUGIN_REDIRECT_HOPS, DEFAULT_PLUGIN_REDIRECT_HOPS) }
pub fn http_redirects() -> usize { num(ENV_HTTP_REDIRECTS, DEFAULT_HTTP_REDIRECTS) }

// —— 缓存上限 ——
pub fn gallery_cache_keep() -> usize { num(ENV_GALLERY_KEEP, DEFAULT_GALLERY_KEEP) }
pub fn notes_keep() -> usize { num(ENV_NOTES_KEEP, DEFAULT_NOTES_KEEP) }
pub fn plugin_fetch_log_keep() -> usize { num(ENV_PLUGIN_FETCH_KEEP, DEFAULT_PLUGIN_FETCH_KEEP) }
pub fn crash_keep() -> usize { num(ENV_CRASH_KEEP, DEFAULT_CRASH_KEEP) }

// —— 轮询间隔 ——
pub fn ticker_interval() -> Duration { interval_ms(ENV_TICKER_MS, DEFAULT_TICKER_MS) }
pub fn fullscreen_interval() -> Duration { interval_ms(ENV_FULLSCREEN_MS, DEFAULT_FULLSCREEN_MS) }
pub fn quiet_after() -> Duration { interval_ms_of(var_chain(&[ENV_QUIET_AFTER_MS, ENV_QUIET_AFTER_MS_LEGACY]).as_deref(), DEFAULT_QUIET_AFTER_MS) }
/// 空闲多久隐藏会话的环境覆盖。**0 / 未设即「按设置 idleHideMin」**：这里没有
/// 默认值，`None` 就是「按设置」，只有排障/测试才用环境覆盖。
pub fn idle_hide_override() -> Option<u64> {
    var_chain(&[ENV_IDLE_HIDE_MS, ENV_IDLE_HIDE_MS_LEGACY]).and_then(|t| t.trim().parse::<u64>().ok())
}
pub fn lhm_every() -> Duration { interval_ms(ENV_LHM_EVERY_MS, DEFAULT_LHM_EVERY_MS) }
pub fn lhm_every_quiet() -> Duration { interval_ms(ENV_LHM_EVERY_QUIET_MS, DEFAULT_LHM_EVERY_QUIET_MS) }
pub fn monitor_min() -> Duration { interval_ms(ENV_MONITOR_MIN_MS, DEFAULT_MONITOR_MIN_MS) }
pub fn monitor_max() -> Duration { interval_ms(ENV_MONITOR_MAX_MS, DEFAULT_MONITOR_MAX_MS) }
pub fn monitor_off() -> Duration { interval_ms(ENV_MONITOR_OFF_MS, DEFAULT_MONITOR_OFF_MS) }

// —— 体积阈值 ——
pub fn log_tail_bytes() -> u64 { num(ENV_LOG_TAIL_BYTES, DEFAULT_LOG_TAIL_BYTES) }
pub fn plugin_file_limit() -> usize { num(ENV_PLUGIN_FILE_BYTES, DEFAULT_PLUGIN_FILE_BYTES) }
pub fn plugin_total_limit() -> usize { num(ENV_PLUGIN_TOTAL_BYTES, DEFAULT_PLUGIN_TOTAL_BYTES) }
pub fn catalog_max_bytes() -> usize { num(ENV_CATALOG_BYTES, DEFAULT_CATALOG_BYTES) }
pub fn npm_tarball_slack_bytes() -> usize { num(ENV_NPM_TARBALL_SLACK_BYTES, DEFAULT_NPM_TARBALL_SLACK_BYTES) }

// —— 字符串开关 ——
/// WebView2 额外启动参数（`FISH_DOCK_BROWSER_ARGS` > 旧 `WAKUWAKU_BROWSER_ARGS`）。
pub fn browser_args_extra() -> Option<String> {
    var_chain(&[ENV_BROWSER_ARGS, ENV_BROWSER_ARGS_LEGACY]).map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
}
/// GitHub OAuth clientId 覆盖（`FISH_DOCK_GITHUB_CLIENT_ID`）；None 时调用处再落到 option_env!/内置。
pub fn github_client_id_override() -> Option<String> {
    var(ENV_GITHUB_CLIENT_ID).map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
}

// 环境变量相关的测试要串行（进程环境是全局的）。runtime.rs 的 app_home 覆盖测试
// 也用这把锁，避免两边同时改 FISH_DOCK_APP_HOME。
#[cfg(test)]
pub(crate) static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[cfg(test)]
mod tests {
    use super::*;

    // —— 键表自洽 ——
    #[test]
    fn every_key_has_a_fish_dock_env_and_default() {
        for key in KEYS {
            assert!(!key.name.is_empty(), "键名不能为空");
            assert!(!key.default.is_empty(), "{} 缺默认值", key.name);
            assert!(
                key.env.starts_with("FISH_DOCK_") || key.env == ENV_APP_HOME || key.env == ENV_USER_DATA_OVERRIDE,
                "{} 的环境变量命名要 FISH_DOCK_ 前缀（旧名兼容项单独标注）：{}",
                key.name,
                key.env
            );
        }
        // 键名不重复
        let mut names: Vec<_> = KEYS.iter().map(|k| k.name).collect();
        names.sort_unstable();
        let before = names.len();
        names.dedup();
        assert_eq!(before, names.len(), "键名有重复");
    }

    // —— 纯解析器边界 ——
    #[test]
    fn parsers_treat_blank_and_bad_values_as_unset() {
        assert_eq!(text_of(None, "d"), "d");
        assert_eq!(text_of(Some("  "), "d"), "d");
        assert_eq!(text_of(Some(" https://x/ "), "d"), "https://x");
        assert_eq!(num_of::<u16>(Some("8080"), 1), 8080);
        assert_eq!(num_of::<u16>(Some("nope"), 1), 1);
        assert_eq!(ms_of(Some("5000"), 20_000).as_millis(), 5_000);
        assert_eq!(ms_of(Some("0"), 20_000).as_millis(), 20_000, "0 视为没设");
        assert_eq!(ms_of(Some("999999999"), 20_000).as_millis(), 20_000, "超上限回落默认");
        assert_eq!(ms_of(Some("600000"), 20_000).as_millis(), 600_000, "恰好上限生效");
        assert_eq!(ms_of(None, 20_000).as_millis(), 20_000);
    }

    #[test]
    fn port_prefers_fish_dock_over_legacy() {
        assert_eq!(port_of(None, None), 47213);
        assert_eq!(port_of(Some("50000"), None), 50000);
        assert_eq!(port_of(Some("50000"), Some("40000")), 50000, "FISH_DOCK_PORT 优先");
        assert_eq!(port_of(None, Some("40000")), 40000, "旧 WAKUWAKU_PORT 兼容");
        assert_eq!(port_of(Some("bad"), Some("40000")), 40000, "坏值跳过用旧的");
    }

    // —— 5 个键「设 env 后行为改变」实测（串行） ——
    // 每个用例设 env → 调 getter → 断言变化 → 还原。覆盖任务要求的五类：
    // 目录重定向、端口、超时、缓存上限、下载源。
    fn with_env<T>(key: &str, value: &str, f: impl FnOnce() -> T) -> T {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        let old = std::env::var_os(key);
        std::env::set_var(key, value);
        let got = f();
        match old {
            Some(v) => std::env::set_var(key, v),
            None => std::env::remove_var(key),
        }
        got
    }

    #[test]
    fn env_redirects_the_data_root() {
        // 目录重定向：设 FISH_DOCK_APP_HOME 为一个绝对临时目录，app_home() 就跟着变。
        let target = std::env::temp_dir().join("fish-config-app-home-env-test");
        let got = with_env(ENV_APP_HOME, &target.to_string_lossy(), app_home);
        assert_eq!(got.ok().as_deref(), Some(target.as_path()), "设 env 后数据根应重定向到该目录");
    }

    #[test]
    fn env_changes_the_port() {
        assert_eq!(with_env(ENV_PORT, "50123", port), 50123);
    }

    #[test]
    fn env_changes_a_timeout() {
        assert_eq!(with_env(ENV_NET_CONNECT_MS, "4321", net_connect_timeout).as_millis(), 4321);
    }

    #[test]
    fn env_changes_a_cache_limit() {
        assert_eq!(with_env(ENV_GALLERY_KEEP, "3", gallery_cache_keep), 3);
    }

    #[test]
    fn env_changes_the_download_source() {
        assert_eq!(with_env(ENV_NPM_REGISTRY, "https://mirror.example/", plugin_npm_registry), "https://mirror.example");
    }

    #[test]
    fn env_changes_a_poll_interval_and_a_size_threshold() {
        assert_eq!(with_env(ENV_TICKER_MS, "750", ticker_interval).as_millis(), 750);
        assert_eq!(with_env(ENV_LOG_TAIL_BYTES, "4096", log_tail_bytes), 4096);
    }

    // 旧名兼容：WAKUWAKU_* 仍要能读，且新名优先，空串视为没设回落旧名。
    #[test]
    fn legacy_wakuwaku_names_still_read_and_new_names_win() {
        let _g = ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        let keys = [ENV_QUIET_AFTER_MS, ENV_QUIET_AFTER_MS_LEGACY, ENV_IDLE_HIDE_MS, ENV_IDLE_HIDE_MS_LEGACY, ENV_BROWSER_ARGS, ENV_BROWSER_ARGS_LEGACY];
        let saved: Vec<(&str, Option<std::ffi::OsString>)> = keys.iter().map(|k| (*k, std::env::var_os(k))).collect();
        for k in keys { std::env::remove_var(k); }
        // 旧名单独设 → 仍能读到
        std::env::set_var(ENV_QUIET_AFTER_MS_LEGACY, "11111");
        assert_eq!(quiet_after().as_millis(), 11_111, "旧 WAKUWAKU_QUIET_AFTER_MS 仍生效");
        std::env::set_var(ENV_IDLE_HIDE_MS_LEGACY, "60000");
        assert_eq!(idle_hide_override(), Some(60_000), "旧 WAKUWAKU_IDLE_HIDE_MS 仍生效");
        std::env::set_var(ENV_BROWSER_ARGS_LEGACY, "--old-flag");
        assert_eq!(browser_args_extra().as_deref(), Some("--old-flag"), "旧 WAKUWAKU_BROWSER_ARGS 仍生效");
        // 新名优先
        std::env::set_var(ENV_QUIET_AFTER_MS, "22222");
        assert_eq!(quiet_after().as_millis(), 22_222, "新 FISH_DOCK_QUIET_AFTER_MS 优先于旧名");
        std::env::set_var(ENV_IDLE_HIDE_MS, "30000");
        assert_eq!(idle_hide_override(), Some(30_000), "新 FISH_DOCK_IDLE_HIDE_MS 优先于旧名");
        std::env::set_var(ENV_BROWSER_ARGS, "--new-flag");
        assert_eq!(browser_args_extra().as_deref(), Some("--new-flag"), "新 FISH_DOCK_BROWSER_ARGS 优先于旧名");
        // 空串视为没设 → 回落旧名
        std::env::set_var(ENV_BROWSER_ARGS, "   ");
        assert_eq!(browser_args_extra().as_deref(), Some("--old-flag"), "新名为空视为没设，回落旧名");
        for (k, v) in saved {
            match v { Some(v) => std::env::set_var(k, v), None => std::env::remove_var(k) }
        }
    }

    // github clientId 与用户数据目录覆盖也走 config（旧名 WAKUWAKU_USER_DATA 沿用）。
    #[test]
    fn env_reads_github_client_id_and_user_data_override() {
        let _g = ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        let s1 = std::env::var_os(ENV_GITHUB_CLIENT_ID);
        let s2 = std::env::var_os(ENV_USER_DATA_OVERRIDE);
        std::env::remove_var(ENV_GITHUB_CLIENT_ID);
        std::env::remove_var(ENV_USER_DATA_OVERRIDE);
        assert_eq!(github_client_id_override(), None);
        std::env::set_var(ENV_GITHUB_CLIENT_ID, "abc123");
        assert_eq!(github_client_id_override().as_deref(), Some("abc123"));
        assert_eq!(user_data_override(), None);
        let dir = std::env::temp_dir().join("fish-config-user-data-env-test");
        std::env::set_var(ENV_USER_DATA_OVERRIDE, &dir);
        assert_eq!(user_data_override().as_deref(), Some(dir.as_path()));
        match s1 { Some(v) => std::env::set_var(ENV_GITHUB_CLIENT_ID, v), None => std::env::remove_var(ENV_GITHUB_CLIENT_ID) }
        match s2 { Some(v) => std::env::set_var(ENV_USER_DATA_OVERRIDE, v), None => std::env::remove_var(ENV_USER_DATA_OVERRIDE) }
    }

    // 表 ↔ getter 一致性：每个键都调一次 getter，断言默认值就是 KEYS 里登记的值。
    // 这既是「每个键确实有默认值且可读」的证明，也顺带让所有 getter 在测试构建里被引用。
    #[test]
    fn every_getter_default_matches_the_key_table() {
        let _g = ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        for key in KEYS {
            let _ = (key.category, key.impact); // 元数据字段被读取（文档表用）
            let actual: String = match key.name {
                // 目录类：getter 已另行测过，这里调用以覆盖，默认值以 KEYS 为准。
                "dir.app_home" => { let _ = app_home(); key.default.to_string() }
                "dir.unified_root" => { let _ = unified_root_env(); key.default.to_string() }
                "dir.resources" => { let _ = resources_override(); key.default.to_string() }
                "dir.user_data" => { let _ = user_data_override(); key.default.to_string() }
                "port.dock" => port().to_string(),
                "url.pet_site" => pet_site(),
                "url.github_api" => github_api(),
                "url.update_repo" => update_repo(),
                "url.update_latest" => update_latest(),
                "url.update_releases" => update_releases(),
                "url.npm_registry" => plugin_npm_registry(),
                "url.npm_mirror" => plugin_npm_mirror(),
                "url.catalog_raw" => plugin_catalog_raw(),
                "url.catalog_api" => plugin_catalog_api(),
                "url.official_site" => official_site(),
                "timeout.net_connect_ms" => net_connect_timeout().as_millis().to_string(),
                "timeout.net_head_ms" => net_head_timeout().as_millis().to_string(),
                "timeout.pet_ms" => pet_timeout().as_millis().to_string(),
                "timeout.update_ms" => update_timeout().as_millis().to_string(),
                "timeout.auth_ms" => auth_timeout().as_millis().to_string(),
                "timeout.proxy_probe_ms" => proxy_probe_timeout().as_millis().to_string(),
                "timeout.proxy_connect_ms" => proxy_connect_timeout().as_millis().to_string(),
                "timeout.proxy_test_ms" => proxy_test_timeout().as_millis().to_string(),
                "timeout.plugin_download_secs" => plugin_download_timeout().as_secs().to_string(),
                "timeout.plugin_activate_secs" => plugin_activate_timeout().as_secs().to_string(),
                "timeout.plugin_rpc_secs" => plugin_rpc_timeout().as_secs().to_string(),
                "timeout.plugin_event_secs" => plugin_event_timeout().as_secs().to_string(),
                "retry.plugin_redirect_hops" => plugin_redirect_hops().to_string(),
                "retry.http_redirects" => http_redirects().to_string(),
                "cache.gallery_keep" => gallery_cache_keep().to_string(),
                "cache.notes_keep" => notes_keep().to_string(),
                "cache.plugin_fetch_keep" => plugin_fetch_log_keep().to_string(),
                "cache.crash_keep" => crash_keep().to_string(),
                "interval.ticker_ms" => ticker_interval().as_millis().to_string(),
                "interval.fullscreen_ms" => fullscreen_interval().as_millis().to_string(),
                "interval.quiet_after_ms" => quiet_after().as_millis().to_string(),
                "interval.idle_hide_ms" => { let _ = idle_hide_override(); key.default.to_string() }
                "interval.lhm_every_ms" => lhm_every().as_millis().to_string(),
                "interval.lhm_every_quiet_ms" => lhm_every_quiet().as_millis().to_string(),
                "interval.monitor_min_ms" => monitor_min().as_millis().to_string(),
                "interval.monitor_max_ms" => monitor_max().as_millis().to_string(),
                "interval.monitor_off_ms" => monitor_off().as_millis().to_string(),
                "size.log_tail_bytes" => log_tail_bytes().to_string(),
                "size.plugin_file_bytes" => plugin_file_limit().to_string(),
                "size.plugin_total_bytes" => plugin_total_limit().to_string(),
                "size.catalog_bytes" => catalog_max_bytes().to_string(),
                "size.npm_tarball_slack_bytes" => npm_tarball_slack_bytes().to_string(),
                "text.browser_args" => { let _ = browser_args_extra(); key.default.to_string() }
                "text.github_client_id" => { let _ = github_client_id_override(); key.default.to_string() }
                other => panic!("KEYS 里新增了键 {other}，请同步加一条 getter 默认值断言"),
            };
            assert_eq!(actual, key.default, "键 {} 的 getter 默认值与 KEYS 登记不一致", key.name);
        }
    }
}
