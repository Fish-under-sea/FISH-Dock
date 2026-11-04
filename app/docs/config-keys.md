# 配置键清单（FISH Dock 2.0.0 · 配置集中化 / 红线 5）

> 唯一入口：Rust 侧 `dock/app/src-tauri/src/config.rs`（`KEYS` 为机器可读清单）。前端**没有**配置覆盖通道：页面的界面节奏/上限是就地具名常量（见「前端」一节），不谎称可覆盖。
> 覆盖优先序：显式注入（测试 `*_of()`）> 环境变量 > 内置默认；空串/空白视为没设；URL 末尾 `/` 去掉；超时统一受 `MAX_TIMEOUT_MS`=600000 夹取。环境变量统一 `FISH_DOCK_` 前缀，不复用 `FISH_DOCK_HOME` 语义。

| 键 | 默认值 | 环境变量 | 类别 | 影响面 |
|---|---|---|---|---|
| `dir.app_home` | `%LOCALAPPDATA%/FISH Dock` | `FISH_DOCK_APP_HOME` | 目录 | 宿主数据根（WebView 资料、设置、宠物） |
| `dir.unified_root` | `%USERPROFILE%/.fish-dock` | `FISH_DOCK_HOME` | 目录 | 统一数据根（凭据/roster/keys），见 storage_context |
| `dir.resources` | `<exe>/resources` | `FISH_DOCK_RESOURCES` | 目录 | 安装资源（Node/WebView2/LHM）目录，开发期可指向 checkout |
| `dir.user_data` | `(空=按数据根)` | `WAKUWAKU_USER_DATA` | 目录 | 移植/测试的数据目录覆盖（旧名沿用 WAKUWAKU_USER_DATA） |
| `port.dock` | `47213` | `FISH_DOCK_PORT` | 端口 | 宿主本地 HTTP 端口（hooks/插件上报），旧 WAKUWAKU_PORT 仍兼容 |
| `url.pet_site` | `https://codex-pets.net` | `FISH_DOCK_PET_SITE` | URL | 宠物图库站点（gallery/下载） |
| `url.github_api` | `https://api.github.com` | `FISH_DOCK_GITHUB_API` | URL | GitHub API 主机（CI/仓库操作） |
| `url.update_repo` | `https://github.com/Fish-under-sea/FISH-Dock/` | `FISH_DOCK_UPDATE_REPO` | URL | 自身更新/公告所属仓库地址 |
| `url.update_latest` | `https://api.github.com/repos/Fish-under-sea/FISH-Dock/releases/latest` | `FISH_DOCK_UPDATE_LATEST` | URL | 自身更新：查最新发行 |
| `url.update_releases` | `https://api.github.com/repos/Fish-under-sea/FISH-Dock/releases?per_page=5` | `FISH_DOCK_UPDATE_RELEASES` | URL | 自身更新：查发行列表 |
| `url.npm_registry` | `https://registry.npmjs.org` | `FISH_DOCK_PLUGIN_NPM_REGISTRY` | URL | 官方插件下载首选源（npm 官方） |
| `url.npm_mirror` | `https://registry.npmmirror.com` | `FISH_DOCK_PLUGIN_NPM_MIRROR` | URL | 官方插件下载镜像（npm 官方失败后） |
| `url.catalog_raw` | `https://raw.githubusercontent.com/Fish-under-sea/FISH-DOCK-Plugins/main/catalog/stable.json` | `FISH_DOCK_PLUGIN_CATALOG_URL` | URL | 官方插件目录（raw 文本） |
| `url.catalog_api` | `https://api.github.com/repos/Fish-under-sea/FISH-DOCK-Plugins/contents/catalog/stable.json?ref=main` | `FISH_DOCK_PLUGIN_CATALOG_API` | URL | 官方插件目录（GitHub contents API） |
| `url.official_site` | `https://h5.metaapp.cn/online/h5/BUsvkkbBiAq2pF2Ttx4j/1.0.2/6/index.html` | `FISH_DOCK_OFFICIAL_SITE` | URL | 官网地址：关于页品牌图彩蛋点开的那一页。**当前默认值是临时落地页**，正式官网上线后只改 `config.rs` 的 `OFFICIAL_SITE_DEFAULT`（或设这个环境变量），前端不动 |
| `url.raw_mirror` | `(空)` | `FISH_DOCK_RAW_MIRROR` | URL | 2.0.4 T-F：raw.githubusercontent.com 文件的前缀镜像（sources::picks 候选；空＝只走原站） |
| `url.gh_proxy` | `(空)` | `FISH_DOCK_GH_PROXY` | URL | 2.0.4 T-F：GitHub Release 资产下载的前缀加速（sources::picks 候选；空＝不加） |
| `url.gitee_repo` | `(空)` | `FISH_DOCK_GITEE_REPO` | URL | 2.0.4 T-F：Gitee 仓库地址（`updates::endpoints` 的更新检查候选；空＝不启用 Gitee）。附加条目 A（2026-11-04）：同一个键还驱动 `sources::picks` 的「Gitee 附件」候选（`<gitee_repo>/releases/download/<tag>/<file>`，**只映射同名项目**——插件仓是另一个 repo，不会被改指过来），并决定 `updates::allowed` 的放行边界（前缀 + 边界字符，同名前缀的别的仓不放行）。只认 `https://gitee.com/<owner>/<repo>` 这一形态，写坏＝没配。实测口径与缺口见 `app/docs/domestic-mirror-matrix-2026-11-04.md` |
| `url.pet_mirror` | `(空)` | `FISH_DOCK_PET_MIRROR` | URL | 2.0.4 T-F：宠物图库图片的前缀镜像（fetch 的预览候选；空＝保留原站） |
| `timeout.net_connect_ms` | `15000` | `FISH_DOCK_NET_CONNECT_MS` | 超时 | 所有 web 请求的 TCP 连接层预算 |
| `timeout.net_head_ms` | `30000` | `FISH_DOCK_NET_HEAD_MS` | 超时 | 所有 web 请求的响应头预算 |
| `timeout.pet_ms` | `30000` | `FISH_DOCK_PET_TIMEOUT_MS` | 超时 | 宠物图库/贴图页整单预算 |
| `timeout.update_ms` | `20000` | `FISH_DOCK_UPDATE_TIMEOUT_MS` | 超时 | 更新检查与公告读取整单预算 |
| `timeout.auth_ms` | `20000` | `FISH_DOCK_AUTH_TIMEOUT_MS` | 超时 | GitHub 登录/Star 校验整单预算 |
| `timeout.proxy_probe_ms` | `600` | `FISH_DOCK_PROXY_PROBE_MS` | 超时 | 设置页代理端口探测等待 |
| `timeout.proxy_connect_ms` | `300` | `FISH_DOCK_PROXY_CONNECT_MS` | 超时 | 代理端口监听探测连接等待 |
| `timeout.proxy_test_ms` | `10000` | `FISH_DOCK_PROXY_TEST_MS` | 超时 | 设置页「测试代理」向 GitHub 发一次请求的预算 |
| `timeout.plugin_download_secs` | `30` | `FISH_DOCK_PLUGIN_DOWNLOAD_TIMEOUT_SECS` | 超时 | 单次插件包/目录下载预算 |
| `timeout.plugin_activate_secs` | `5` | `FISH_DOCK_PLUGIN_ACTIVATE_TIMEOUT_SECS` | 超时 | 插件停用 RPC 预算；2.0.4 起同时作启动预算的**只读下限**（旧用户配置只放宽不收紧） |
| `timeout.plugin_cold_until_secs` | `300` | `FISH_DOCK_PLUGIN_COLD_UNTIL_SECS` | 超时 | T04 冷态判据：登录后多少秒内算冷启 |
| `timeout.plugin_activate_cold_secs` | `20` | `FISH_DOCK_PLUGIN_ACTIVATE_COLD_TIMEOUT_SECS` | 超时 | T04 冷态激活预算；2.0.4 起降级为**只读下限**（与 `timeout.plugin_start_cold_secs` 取 max） |
| `timeout.plugin_cold_after_start_secs` | `120` | `FISH_DOCK_PLUGIN_COLD_AFTER_START_SECS` | 超时 | 2.0.4 冷态判据第二锚点：进程启动后多少秒内也算冷启（与登录锚点取「或」） |
| `timeout.plugin_start_cold_secs` | `60` | `FISH_DOCK_PLUGIN_START_COLD_SECS` | 超时 | 2.0.4 冷态一次启动尝试**共享**预算（hello+activate+服务探测）；旧激活键作只读下限 |
| `timeout.plugin_start_warm_secs` | `10` | `FISH_DOCK_PLUGIN_START_WARM_SECS` | 超时 | 2.0.4 热态一次启动尝试共享预算（旧激活键作只读下限） |
| `timeout.plugin_start_lifecycle_secs` | `180` | `FISH_DOCK_PLUGIN_START_LIFECYCLE_SECS` | 超时 | 2.0.4 一次自启生命周期总预算：排队/校验/退避/重试全计入，到期不再开新尝试 |
| `timeout.plugin_rpc_secs` | `15` | `FISH_DOCK_PLUGIN_RPC_TIMEOUT_SECS` | 超时 | 插件通用 RPC 预算 |
| `timeout.plugin_event_secs` | `10` | `FISH_DOCK_PLUGIN_EVENT_TIMEOUT_SECS` | 超时 | 插件事件投递预算 |
| `retry.plugin_redirect_hops` | `5` | `FISH_DOCK_PLUGIN_REDIRECT_HOPS` | 重试 | 插件包下载最大重定向跳数 |
| `retry.http_redirects` | `10` | `FISH_DOCK_HTTP_REDIRECTS` | 重试 | 通用 web 请求最大重定向数 |
| `cache.gallery_keep` | `8` | `FISH_DOCK_GALLERY_CACHE_KEEP` | 缓存上限 | 宠物图库离线缓存文件数上限 |
| `cache.notes_keep` | `5` | `FISH_DOCK_NOTES_KEEP` | 缓存上限 | 更新公告缓存条数上限 |
| `cache.plugin_fetch_keep` | `20` | `FISH_DOCK_PLUGIN_FETCH_LOG_KEEP` | 缓存上限 | 插件下载来源记录条数上限 |
| `cache.crash_keep` | `20` | `FISH_DOCK_CRASH_KEEP` | 缓存上限 | 崩溃报告保留份数上限 |
| `interval.ticker_ms` | `500` | `FISH_DOCK_TICKER_MS` | 轮询间隔 | 主 ticker 心跳间隔 |
| `interval.fullscreen_ms` | `1500` | `FISH_DOCK_FULLSCREEN_MS` | 轮询间隔 | 全屏检测轮询间隔 |
| `interval.quiet_after_ms` | `90000` | `FISH_DOCK_QUIET_AFTER_MS` | 轮询间隔 | 闲置多久进入 quiet（旧 WAKUWAKU_QUIET_AFTER_MS 兼容） |
| `interval.idle_hide_ms` | `0` | `FISH_DOCK_IDLE_HIDE_MS` | 轮询间隔 | 空闲多久隐藏会话（0/未设=按 idleHideMin 设置；旧 WAKUWAKU_IDLE_HIDE_MS 兼容） |
| `interval.lhm_every_ms` | `3000` | `FISH_DOCK_LHM_EVERY_MS` | 轮询间隔 | LHM 传感器轮询（活跃） |
| `interval.lhm_every_quiet_ms` | `10000` | `FISH_DOCK_LHM_EVERY_QUIET_MS` | 轮询间隔 | LHM 传感器轮询（quiet） |
| `interval.monitor_min_ms` | `1000` | `FISH_DOCK_MONITOR_MIN_MS` | 轮询间隔 | 硬件监视最短轮询间隔 |
| `interval.monitor_max_ms` | `10000` | `FISH_DOCK_MONITOR_MAX_MS` | 轮询间隔 | 硬件监视最长轮询间隔 |
| `interval.monitor_off_ms` | `3600000` | `FISH_DOCK_MONITOR_OFF_MS` | 轮询间隔 | 关闭监视时的休眠间隔 |
| `size.log_tail_bytes` | `262144` | `FISH_DOCK_LOG_TAIL_BYTES` | 体积阈值 | recent.log 轮转阈值（256 KiB） |
| `size.plugin_file_bytes` | `134217728` | `FISH_DOCK_PLUGIN_FILE_LIMIT` | 体积阈值 | 插件单文件上限（128 MiB） |
| `size.plugin_total_bytes` | `268435456` | `FISH_DOCK_PLUGIN_TOTAL_LIMIT` | 体积阈值 | 插件包解压总量上限（256 MiB） |
| `size.catalog_bytes` | `524288` | `FISH_DOCK_CATALOG_MAX_BYTES` | 体积阈值 | 官方目录 JSON 大小上限（512 KiB） |
| `size.npm_tarball_slack_bytes` | `8388608` | `FISH_DOCK_NPM_TARBALL_SLACK` | 体积阈值 | npm tarball 相对插件归档的余量（8 MiB） |
| `size.plugin_rollback_bytes` | `8388608` | `FISH_DOCK_PLUGIN_ROLLBACK_LIMIT` | 体积阈值 | 插件升级回滚点导出上限（单文件与总量共用，8 MiB）；超限逐项跳过、不阻断更新（#37） |
| `text.browser_args` | `(空=用内置)` | `FISH_DOCK_BROWSER_ARGS` | 字符串 | WebView2 额外启动参数（旧 WAKUWAKU_BROWSER_ARGS 兼容） |
| `text.github_client_id` | `(见内置 DEFAULT_CLIENT_ID)` | `FISH_DOCK_GITHUB_CLIENT_ID` | 字符串 | GitHub OAuth 设备流 clientId 覆盖 |
| `text.debug` | `(空=关)` | `FISH_DOCK_DEBUG` | 字符串 | 2.0.4 排障日志开关（只认 `1`）；旧 `WAKUWAKU_DEBUG` 只读兼容，新名优先 |
| `mirror.prefer` | `auto` | `FISH_DOCK_MIRROR_PREFER` | 字符串 | 2.0.4 T-F：sources::picks 的候选排序（auto/domestic/overseas；非法值回落 auto） |

## 旧名兼容 / 弃用（t15 收口第二入口）

以下旧 `WAKUWAKU_*` 名仍能读，**优先序固定为新名 > 旧名 > 默认**；空串/空白视为没设，会回落旧名，绝不静默丢弃用户已写在环境里的旧变量。旧名仅作兼容保留，新代码与新文档一律用 `FISH_DOCK_*`：

| 新键（推荐） | 旧名（兼容保留） | 说明 |
|---|---|---|
| `FISH_DOCK_QUIET_AFTER_MS` | `WAKUWAKU_QUIET_AFTER_MS` | 闲置多久进入 quiet（t5 内存预算相关） |
| `FISH_DOCK_IDLE_HIDE_MS` | `WAKUWAKU_IDLE_HIDE_MS` | 空闲多久隐藏会话（0/未设=按 idleHideMin 设置） |
| `FISH_DOCK_BROWSER_ARGS` | `WAKUWAKU_BROWSER_ARGS` | WebView2 额外启动参数（字符串） |
| `FISH_DOCK_PORT` | `WAKUWAKU_PORT` | 宿主端口（t6 已收口） |
| `FISH_DOCK_RESOURCES` | `DSH_FISH_RESOURCES` | 安装资源目录（2.0.4 T-Q 收口：`config::resources_override` 是唯一解析点） |
| `FISH_DOCK_DEBUG` | `WAKUWAKU_DEBUG` | 排障日志开关（2.0.4 T-Q 收口到 `config::debug_enabled`，新名优先） |
| `FISH_DOCK_APP_HOME` | —（无旧名） | 数据根（`config::app_home` 委托 runtime） |
| `WAKUWAKU_USER_DATA` | 旧名沿用（无新名） | 移植/测试的数据目录覆盖，`config::user_data_override` 统一读取 |

> `FISH_TEST_*`/`FISH_DOCK_TEST_*`、`WAKUWAKU_REDACT_*` 是排障/测试开关，不进产品配置面（t15 明确不动）。`WAKUWAKU_DEBUG` 只读兼容，实际读取统一走 `config::debug_enabled`（2.0.4 T-Q）。

## 前端：只读默认，无覆盖通道（t20 诚实收缩）

- **没有前端配置键，也没有环境变量覆盖**。之前 `src/shared/config.js` 读 `window.__FISH_DOCK_CONFIG__`，但全仓无任何写入方（无 `initialization_script`、无宿主注入），等于「看起来能覆盖其实不能」——按方案 B 已删除该文件与该注入幻想，页面不再引用它。
- 界面节奏/上限是**就地具名常量**（红线 5：进常量即合规；它们是 UI 动画节奏，不是可配置的目录/端口/URL/超时部署项）：
  - `desk.js` `LOADING_MS=5000`、`ICONS_FRESH_MS=5000`、`TOAST_MS=2000`；`bar.js` / `island.js` 各一份 `CLOCK_TICK_MS=5000`（各自页面 bundle，不跨 realm 共享）。
  - `extension-page.js` `PARK_DISCARD_MS=5*60*1000`（内存预算：跨源 renderer 不许常驻）；`capsule.js` `MAX_PINS=5`、`MAX_MEMBERS=64`。
- 已删除的 4 个零消费者键（`toastMs`/`iconsFreshMs`/`deskLoadingMs`/`clockTickMs`）在上面对应的具名常量处就地定义，不再保留空壳键。
- 若将来真需要前端可调，正确做法是宿主在创建各 webview 时用 `initialization_script` 注入，且默认值只由 `config.rs` 一处导出；当前判断收益不抵耦合成本，故不做。

## 已接线的调用点（改键即改行为，均有测试）

- 端口 `port.dock`：`main.rs` resolve_port / 传感器子命令默认端口。
- 超时 `timeout.*`：`proxy.rs`（全部委托 config）、`plugin_store.rs`、`plugin_process.rs`/`plugin_runtime.rs`、`updates.rs`/`github_access.rs`/`fetch.rs` 经 proxy 生效。
- 间隔 `interval.ticker_ms`/`fullscreen_ms`/`quiet_after_ms`/`idle_hide_ms`/`lhm_every(_quiet)_ms`：`main.rs`/`lhm.rs`。
- 缓存 `cache.*`：`fetch.rs`/`crash.rs`/`updates.rs`/`plugin_store.rs`。体积 `size.*`：`crash.rs`/`plugin_store.rs`；`size.plugin_rollback_bytes` 由 `plugin_install.rs`（`write_rollback_point` → `storage_archive::export_rollback`）读取，作为升级回滚点的单文件/总量上限。
- 字符串 `text.browser_args`（`main.rs` browser_args）、`text.github_client_id`（`github_access.rs` client）、`text.debug`（`server.rs` 排障日志开关，经 `config::debug_enabled`）。
- 自启预算 `timeout.plugin_start_cold_secs`/`plugin_start_warm_secs`/`plugin_start_lifecycle_secs`/`plugin_cold_after_start_secs`：`plugin_start.rs`（调度器）、`plugin_process.rs`（一次尝试的共享 deadline）、`plugin_runtime.rs`（生命周期状态）；旧 `timeout.plugin_activate_*` 作只读下限。
- 数据目录 `dir.user_data`（`data.rs`/`main.rs`/`connection.rs`/`runtime.rs` 统一走 `config::user_data_override`）。

## Gitee 双仓路径与镜像键的默认值口径（附加条目 A，2026-11-04）

- **双仓路径**（与工作区 `AGENTS.md` §八同源；实测证据见 `app/docs/domestic-mirror-matrix-2026-11-04.md`）：
  - Dock 宿主：GitHub `https://github.com/Fish-under-sea/FISH-Dock`（发行与摘要权威）／Gitee `https://gitee.com/Fish-under-sea/FISH-Dock`（国内仓库入口）。
  - 官方插件：GitHub `https://github.com/Fish-under-sea/FISH-DOCK-Plugins`（`catalog/stable.json` 的 `url` 全部指向它的 Release）／Gitee `https://gitee.com/Fish-under-sea/FISH-DOCK-Plugins`。⚠️ 2026-11-04 实测该 Gitee 仓**匿名 API 404、raw 403**（`AGENTS.md` §八记的「200」已不成立，需人工确认可见性），**不作为 `.fish-plugin` 下载源**。
- **默认值口径**：`url.raw_mirror` / `url.gh_proxy` / `url.gitee_repo` / `url.pet_mirror` 四个镜像键默认全空，空＝「没配」＝**不加候选**，getter 不以任何内置地址兜底（`config.rs` 的 `RAW_MIRROR_DEFAULT=""` 与三个 `text(ENV_*, "")`）。`mirror.prefer` 默认 `auto`（原地址优先、镜像兜底），无候选时 `domestic` 自然回落原地址。
- **不内置第三方镜像**：实测确认可用的公开前缀加速（`ghfast.top` / `gh-proxy.com`，字节与官方摘要一致）只作为用户可自行填入 `url.gh_proxy` 的参考值记在矩阵 §6，**产品默认值保持空**——第三方前缀会看到完整目标地址且随时可能停服或换字节，这个决定不替用户做。
- **缺口如实标注**：pet 图库（`codex-pets.net` 实测 DNS 污染级不可达）、Windhawk、LHM **均无已确认的免费国内镜像**；Dock 更新 exe 的 Gitee 附件**当前全 404**（Gitee 主仓只有 tags、没有发行版本）。缺口如实标注，不拿默认空前缀算作已完成，也不要求用户自建服务。
- **AEGIS**：接镜像候选不改 CSP、不放宽下载域名白名单（`updates::allowed` 反而按「前缀 + 边界」收窄了）、不伪造镜像地址与摘要；任何镜像来的字节仍要过既有摘要校验（`updates.rs` 的 `digest` 比对、`plugin_store` 的 catalog `sha256`）才落地。

## 证明生效的测试

`cargo test -- config::` 全部通过：五类键各一条（目录重定向/端口/超时/缓存上限/下载源）+ 间隔&体积 + 旧名兼容与优先级 + github clientId/用户数据目录 + 键表↔getter 一致性；2.0.4 另加：一次尝试共享预算与旧键只读下限、冷态双锚点、资源目录/排障开关新名优先。

镜像四键与 `mirror.prefer` 的「配了才启用 / 默认不变形」由择源层与更新器的用例锁住（附加条目 A，2026-11-04）：

- `cargo test -- sources`（27 passed）：`default_candidates_are_locked_without_any_mirror_key` 逐条锁定四键全空时的默认候选表；`the_gitee_attachment_candidate_needs_the_repo_configured`、`the_gitee_candidate_never_repoints_another_project`、`a_malformed_gitee_repo_yields_no_candidate`、`the_gitee_attachment_takes_part_in_the_same_walk` 锁 `url.gitee_repo` 的开关语义、同名项目限定、坏配置＝没配、共用一次 deadline。
- `cargo test -- updates`（35 passed）：`the_gitee_endpoint_appears_only_when_the_repo_is_configured` 锁 `endpoints()` 的开关；`the_gitee_candidate_stays_inside_the_same_allowance`、`the_allowance_stops_at_the_repository_boundary` 锁放行边界不因配置 Gitee 而放宽；`both_installer_names_get_the_gitee_candidate` 锁两个真实安装器名与它们的 `.sha256` 都能换源。

> 这批用例的**测试环境基础设施**（t10 复审后收口）：镜像类环境变量（`FISH_DOCK_GITEE_REPO` / `FISH_DOCK_GH_PROXY` / `FISH_DOCK_RAW_MIRROR` / `FISH_DOCK_PET_MIRROR` / `FISH_DOCK_MIRROR_PREFER`）在 `sources` 与 `updates` 两个测试模块之间**只有一把锁** `crate::sources::MIRROR_ENV_LOCK`（`updates` 里原来的 `UPDATE_ENV_LOCK` 已退役成它的别名，既有调用点不用改），所以「设镜像键」的用例之间必然串行；改键一律走 Drop 守卫（`GiteeEnvGuard` / `GhProxyEnvGuard` / `EmptyMirrorsGuard` / `sources` 的 `EnvGuard`），断言 panic 也会还原，不会把脏值留给同进程后面的用例；读「当下候选表」的用例再用 `with_empty_mirrors` 把四个镜像键显式清成不存在，不依赖环境刚好干净。少任何一条，`cargo test -- updates` 都会在并行下随机红。

