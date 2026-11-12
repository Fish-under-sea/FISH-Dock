# FISH Dock 2.0.4 实施方案（修订版）

> 状态：2026-11-03 批判性审查后修订，待用户审阅；本轮没有开始实现。
> 基线：`main` @ `84db441`（2.0.3）。目标版本 **2.0.4**。
> 审查证据与限制见 [audit-dock-2.0.4.md](audit-dock-2.0.4.md)。
> 决策来源：Gitee 通道「先不依赖、配了才启用」；加速前缀「默认关、可配置」；版本 2.0.4；
> 上游未纳入项按用户勾选的 6 项补齐；插件市场与资源包**不做**（改为强化 npm 发现式检索）；
> `/v1` 与 agent-core 抽 crate **两件一起做**；
> 崩溃上报按用户决定走 **issue 预填链接 + 导出兜底**（不部署中转服务）；
> 用户实测的窗口/弹层两段式生长（插件页「先半屏」、审批框「二次轻微拓展」）列为 T-N。

---

## 0 范围

| 编号 | 内容 | 类型 | 依赖 |
|---|---|---|---|
| T-A | **agent-core 抽 crate**（中性事件与可独立转换逻辑，registry/asks 留宿主） | 架构 | — |
| T-B | **`/v1` 本地 API**（info / sessions / events SSE + token + 防 rebound） | 新功能 | T-A |
| T-C | **#63** 插件卡「更多」/ 卸载标红 / 搜索框 / npm 发现式检索（强化版） | UI + 前端编排 | — |
| T-D | **#65 / #66** Windhawk 下载出口、布尔设置、失败可见性 | Bug | T-F |
| T-E | **开机自启时插件后端拉不起来**（clash / magpie 双超时） | P0 Bug | — |
| T-F | **双源获取**（国内镜像 / 国外自动切换）+ **更新提醒**（Gitee 配了才启用） | 架构 | — |
| T-G | 任务栏时钟跟随 Windows 显示秒 | 小功能 | — |
| T-H | Wi-Fi 信号格数取 Windows 的 0–5 格 | 小功能 | — |
| T-I | Codex 桌面版内部请求不算会话（上游 #74） | 小功能 | T-A |
| T-J | 设置页会话行展开看改动文件 / 最近步骤（上游 #89/#92） | 功能 | T-A |
| T-K | 解除依赖后分层退役 `app/examples/` 冗余示例 | 清理 | T-P |
| T-L | `build-release` 洗掉 exe 里的本机路径 | 发布工程 | — |
| T-M | **崩溃报告上报**：issue 预填链接 + 导出兜底（零部署、零密钥） | 功能 | — |
| T-N | **窗口与弹层测量**：插件页「先半屏」、审批框「二次轻微拓展」、面板跟随 | UI 缺陷 | — |
| T-O | **OFM 业务解耦与最小旧安装兼容**：宿主不再内置、安装或开发自动配置 OFM；保留明确的旧数据/旧插件兼容边界 | 清理 + 兼容 | — |
| T-P | **活动品牌与集成输出迁移**：去除默认活动输出中的 Wakuwaku/上游仓命名，保留协议兼容、版权与历史归属 | 迁移 | T-O |
| T-Q | **配置入口收口**：资源路径、测试覆盖、旧环境变量和文档统一由新入口解析 | 工程 | T-E / T-F / T-P / T-G / T-H |
| T-R | **旧材料分层、发布契约与总验收门**：历史文档归档、全量测试入口、版本与包内容审查 | 发布工程 | T-A–T-Q 全部 |

**不做**：上游的「别人写的插件（web addons）+ 插件市场 + 资源包」（`addons.rs`/`market.rs`/`packs.rs`/`zip.rs` + `pet/addons.js`/`addonui.js`/`views.js`/`addon/sdk.js`）。理由：那是一套 iframe 沙盒插件模型，与我们现有 Node 子进程 + npm 体系并列会形成双模型；本轮改为把 #63 的 npm 发现式检索做强。

**2.0.4 的硬边界**：Dock 不再携带 OFM 服务载荷、OFM 业务源码、OFM 自动安装入口、OFM 开发预览自动配置或 OFM 账号业务。对已经安装的旧插件、旧数据目录、旧备份格式，只保留 T-O 中列明的识别、隔离和可卸载兼容；不自动迁移账号、不自动改名、不把 OFM 映射成 Magpie。

---

## 1 事实核查（已逐条读代码确认）

### 1.1 #63

- `extension-panel.js:493` `<details class="extension-more">` 里两个按钮。
- `extensions.css:58` `display:flex;flex-wrap:wrap;gap:6px`；`:59` 两枚按钮 `border-color:transparent;background:transparent;color:var(--g4)` → 展开区是裸灰字。
- `extensions.css:62` 危险色只挂 `:hover` / `:focus-visible` → 常态与「打开插件日志」同色。
- `extension-panel.js:515` placeholder 是整句命名规则，窄窗被截断。
- `plugin_thirdparty.rs:313` `search(query)` 要求 query 非空（前端 `:377` 空输入直接提示）→ 不输入就看不到任何可安装插件。
- `plugin_store.rs:75` 官方目录条目 id 白名单**硬编码** `fish.feishu` / `fish.clash` / `fish.magpie`。

### 1.2 #65 / #66

- `windhawk_collection.rs:52-55` `download_source()`：裸 `ureq::Agent`，**不取 `proxy::web_ways`**（ureq 只读环境变量代理，不读 Windows 系统代理），超时硬编码 20 秒。
- `windhawk_source.rs:43-53` `flatten_settings()` 只处理 `Mapping`/`Sequence`/`String`/`Number`，`Y::Bool` 落到 `_ => Err(...)` → 含布尔默认值的 mod 必然失败（#66 根因）。
- `windhawk-panel.js:84` `view ? 内置引擎已就绪 : (busy ? …)` → 引擎就绪时 busy 无文案变化。
- `windhawk-panel.js:88` `<details${preview?' open':''}>` → 失败即收起。
- `windhawk-panel.js:86` `error` 渲染在折叠区之外，离按钮很远。
- `windhawk-panel.js:7` `run()` 的 `if(busy)return` 静默吞点击。

### 1.3 开机自启

- `plugin_process.rs:76/78`：`host.hello` 与 `plugin.activate` 各用一次 `plugin_activate_budget()`。
- `config.rs:384`：冷态 20 s / 热态 **5 s**；`config.rs:373` 冷态判据是**登录后 ≤300 秒**。
- `plugin_process.rs:40/47`：冷态退避 500 ms 重试**一次**，热态不重试。
- `plugin_runtime.rs:195` `spawn_starts` 逐插件串行，失败即写卡终态（`:206-211`），**无后续追赶**。

静态结论：冷态窗口只锚在登录时刻，且存在串行等待、阶段独立超时和失败无追赶路径。它们能解释超时风险，但尚未取得本次登录日志，不能证明当时已超过 300 秒，也不能认定是唯一根因。T-E 先补分阶段时序并复现，再以可控夹具和真实登录验证修复。

### 1.4 双源获取

已定位的主要下载/获取出口（尚非全部网络行为；T-F 补全静态资产、头像、图库 API/详情/精灵图与嵌套资源清单）：

| 出口 | 位置 | 代理多路 | 国内镜像 |
|---|---|---|---|
| 宠物列表 API / 详情 / 精灵图下载 | `fetch.rs:139,175-191,282` | 是 | 无 |
| 宠物缩略图 / 预览条 | `settings.js:502`、`fetch.rs:294`、`index.html:7` | 绕过 Rust，由 img 直连 | 无 |
| 官方插件目录 | `plugin_store.rs:101` | 是 | 无（raw + api 都是 GitHub） |
| 官方插件包 | `plugin_store.rs:102` | 是 | **有**（npm → npmmirror → GitHub） |
| 第三方插件 | `plugin_thirdparty.rs:175` | 是 | **有**（npm ↔ npmmirror） |
| 自身更新检查 | `updates.rs:434` | 是 | 无（见 1.5） |
| 自身更新包下载 | `updates.rs:1399` | 是 | 无 |
| 更新公告 | `updates.rs:781` | 是 | 部分（认 Gitee，需配仓） |
| Windhawk mod 源码 | `windhawk_collection.rs:52` | **否** | 无 |
| LHM 下载 | `lhm_setup.rs:937/950` | 是 | 无 |
| PawnIO 安装器 / 模块 | `sensors.rs:977-1009,1049` | 否，裸 ureq / 600 秒 | 无 |
| 崩溃报告上传 | `crash.rs:549` | 是 | 生产中转地址为空，当前无部署；T-M 改走预填/导出 |
| GitHub 授权 | `github_access.rs:76` | 是 | 不可镜像（OAuth） |
| DSH bridge | `dsh_bridge.rs:489` | pnpm 自身 | 有（`--registry=npmmirror`） |

### 1.5 更新提醒

- `updates.rs:434` `endpoints()`：`if !testing() { return vec![(GitHub, …)] }` → **生产路径只有 GitHub**。
- `updates.rs:60` `const GITEE_REPO: &str = "";` —— 空占位。
- `settings.rs:579` / `settings.js:3128` 却把 `updateFrom` 做成 auto/gitee/github 三选 → **选了 Gitee 无任何效果**。
- `updates.rs:570` `tell()` 的灵动岛提醒**已实现**（`update.out` = 「有新版本 v{version}」、`update.tap` = 「点一下去更新」，与上游文案一致），`island.js:5806` 已接。
- `updates.rs:640` `schedule_checks()` 安装版启动 20 秒后首查、每 10 分钟一轮。

静态结论：提示链路已存在，生产候选源不足；本轮尚未证实提醒时机、可见性和去重在用户现场全部正常。T-F 同时测试取到新版、宠物显示、点击入口、不可见延迟提示和重启去重。

### 1.6 上游补齐项现状

| 项 | 我们 | 证据 |
|---|---|---|
| 时钟显示秒 | 无 | `ShowSecondsInSystemClock` / `seconds_on` 0 命中 |
| Wi-Fi 格数 | 无 | `GetSignalBars` / `wifi_bars` 0 命中 |
| Codex 内部请求过滤 | 无 | `app-generated` 0 命中 |
| 会话行展开 | 无 | `state.rs:161` 有 `files: Vec<String>`，`:325` 只输出 `files` 计数；`settings.js:447` 会话行无展开 |
| `/v1` | 无 | 全仓 `"/v1` 0 命中 |
| examples 退役候选 | 待解除依赖 | `app/examples/widgets/` 13 个文件；`waku.ps1` 和参数契约仍有消费者 |
| build-release | 无 | 只有 `app/scripts/build-installer.ps1`，无 remap-path |

### 1.7 旧配置、旧文件与上游命名审计

- 已确认本地源码树没有 `dsh-our-free-model/`、`app/.runtime/`、`app/src-tauri/resources/ofm/`、`app/scripts/stage-ofm.mjs` 或 `src-tauri/src/oflm.rs`；安装器仍有退役载荷防回流检查，必须保留。
- 已确认宿主仍有活动 OFM 耦合：`plugin_development.rs` 接受 `ofm` 并写 38080 配置；身份归档/锁逻辑、旧命名空间、`fish.ofm` 页面与 storage 映射仍存在；这些归入 T-O，而不是把 grep 命中全部删除。
- 已确认活动上游命名仍出现在 Claude/DSH/OpenCode/OMP/Thunderbird 集成、hook、release 脚本、relay 部署材料、预览夹具和帮助文案中；归入 T-P，采用“新名写出、旧名只读”迁移。
- 已确认配置入口存在漂移：`config.rs` 接受 `FISH_DOCK_RESOURCES`，而 `runtime.rs` 和 `windhawk_bundle.rs` 仍直接读取 `DSH_FISH_RESOURCES`；归入 T-Q。
- `app/examples/widgets/` 不能直接判定为死代码：`scripts.rs` 通过 `include_bytes!` 使用 `waku.ps1`，`plugins.rs` 参数测试读取多个示例，先按 T-K 解除依赖再决定删除。

---

## 2 T-A：agent-core 抽 crate（最小切片）

### 2.1 为什么是「最小切片」而不是照搬上游

前轮上游核查将 agent-core/agent-hub 作为参考。本轮不新增外部仓库依赖：在本仓实现最小 crate，使依赖、测试和发布节奏可控；这不是断言上游依赖不可使用。外部标签和提交数量不作为本任务验收依据。

自建分两档：

| 档 | 内容 | 风险 |
|---|---|---|
| 完整对齐 | 事件、registry、asks 全下沉，`state.rs` 只留 Look | 动 `state.rs`(46.6 KB) + `events_*`(8 文件 ≈200 KB) + `asks.rs`(46.6 KB)，2.0.4 不可控 |
| **最小切片（本方案）** | `AgentEvent` 中性化 + 各 agent 的 `to_event` 下沉；registry **暂留宿主**；`/v1` 从宿主投影读 | T-A 不改注册表算法；T-I/T-J/T-B 可有独立的宿主状态适配变更 |

选最小切片。完整 registry 下沉与 asks 下沉留后续版本（列入本文「风险与已知限制」）。

### 2.2 结构

```text
app/src-tauri/
  Cargo.toml            → 加 [workspace] members = ["crates/agent-core"]
  crates/agent-core/
    Cargo.toml
    src/lib.rs
    src/event.rs        → AgentEvent（中性事件）
    src/claude.rs codex.rs agy.rs opencode.rs omp.rs zcode.rs reasonix.rs dsh.rs
                          （每个 agent 的 to_event，从 events_*.rs 迁来）
  src/events_*.rs       → 保留宿主侧：日志行、mood、demo 适配；转换改为调用 agent-core
```

`AgentEvent` 是拟定内部结构，不是已存在 API；实施前按八类现有事件逐字段映射，避免为统一接口丢掉关联信息：

```rust
pub struct AgentEvent {
    pub agent: Agent,                 // claude | codex | agy | opencode | omp | zcode | reasonix | dsh
    pub session: String,
    pub subagent: bool,
    pub cwd: String,
    pub turn: u64,
    pub name: String,
    pub last: String,                 // 最后一句话
    pub doing: Doing,                 // start | end | toolStart | toolEnd | toolFailed |
                                      // waiting(..) | working | resting | stopped |
                                      // done | failed | taskDone
    pub at: u64,
}
```

- `Doing` 里只放「会话是什么状态」，**不放 mood / seen / 停留多久**——那些是宿主的展示层（`state.rs` 的 `Look`）。
- 先盘点八类适配器输入/输出、依赖的会话状态和异常路径，再提取可独立转换的纯逻辑。需要历史会话或宿主 I/O 的部分留在适配器，不强行规定所有 agent 都必须是无状态 `JSON -> Option<AgentEvent>`；接口变化不得丢失 tool/turn 关联、顺序或失败状态。

### 2.3 验收

- 按 §14 执行 workspace 与独立 crate 检查；新增失败为零，已有环境异常逐项记录。纯 crate 不依赖 Tauri/Windows 即可运行。
- 八类 agent 分别覆盖正常事件、异常/未知输入和顺序依赖；以现有行为断言验收，不用固定条数替代覆盖。
- **等价性证据**：先确认现有夹具能覆盖哪些 agent，不假定 fixtures 已齐全；对旧适配器建立基准，再与新路径比较会话可见状态、顺序和副作用。旧路径没有 AgentEvent 结构时不声称可直接逐字段比较。

---

## 3 T-B：`/v1` 本地 API

### 3.1 端点与一致性

| 端点 | 鉴权 | 契约 |
|---|---|---|
| `GET /v1/info` | 无 | 版本、app、启动 epoch、seq、支持能力；nonce 校验后附 `proof`。不输出路径、会话内容或 token。token 路径通过本地文档约定。 |
| `GET /v1/sessions` | Bearer | 返回同一快照的 `{epoch,seq,sessions}`；支持 agent 枚举与 cwd 路径组件包含关系过滤，不使用裸字符串前缀。 |
| `GET /v1/events` | Bearer | SSE；`id` 使用 `epoch:seq`，可用 Last-Event-ID 或 since 恢复；二者冲突返回参数错误。失效 epoch、游标过旧或领先时发 reset，客户端重新拉快照。 |

- 环形缓冲上限 512 条 / 8 MiB；单事件最大 64 KiB，超限不切断 JSON，转 reset 以便重新读取有界快照。
- 订阅积压上限 256 条 / 2 MiB，任一触顶即断开；全局最多 8 个 SSE 连接；每 15 秒心跳；写入超时、客户端断开和宿主退出都释放队列、线程与连接。
- 初始快照与订阅注册必须在同一事件序列边界交接，避免快照之后、订阅之前漏事件；接受的会话变更与 seq 增长同序。
- 会话删除、轮次结束、放弃工作均有明确事件；原始 prompt、文件正文和本机路径按现有「显示具体内容」开关投影，关掉后不经 SSE 继续泄露旧缓存。

### 3.2 本地身份与请求校验

- 复用 `server.rs` 的监听端口，监听地址限定 loopback；Host 按解析后的主机和实际监听端口精确校验，拒绝重复/缺失 Host 和伪 localhost 后缀。
- `/v1` 默认不开放跨源 CORS；Origin 出现且不是明确支持的本地调用来源时拒绝；Bearer 只放请求头，不放 URL、日志或错误正文。
- token 用操作系统随机源生成至少 128 bit，保存在当前宿主私有数据目录的 `agent-api/token`，沿用用户私有 ACL；损坏或权限异常必须明确报错，不静默换 token；显式轮换会断开旧订阅。
- 客户端读取本地 token 后，用新随机 nonce 验证 `HMAC-SHA256(token, nonce)` 才发 Bearer。nonce 长度受限且 proof 禁缓存；此机制只验证 token 持有者，不宣称能防同用户读取 token 的进程。

### 3.3 落点与验收

- 新增 `agent_hub.rs`，定义有界投影、事件环、订阅者与 proof。先读现有 tiny_http 请求分发，验证 SSE 不阻塞 `/health`、hook、审批或退出；不假定现有架构为“每请求一个线程”。
- API 复用宿主会话注册表，T-A 不为此下沉 registry；事件提取的等价重构与新增广播分别提交。
- 服务端用真实 HTTP/SSE 夹具验证鉴权、Host/Origin、过滤、游标、重启 reset、快照交接、背压、写超时和退出释放；客户端协议夹具验证 proof 失败时从未发出 Bearer。
- 手工使用 info、带 Bearer 的 sessions 和 `curl -N` 查看事件；token 不出现在命令日志、截图或提交记录中。具体命令按实际端口注入，不固定生产端口。

---

## 4 T-C：#63

### 4.1 「更多」展开区改行式动作列表

- `extensions.css:58-63` 重写：`.extension-more-body` → `flex-direction:column;gap:0`；每行 `.extension-more-row` `min-height:32px`、整行可点、`hover` 整行 `var(--g2)`；「打开插件日志」在上，`1px solid var(--line)` 发丝线隔 8 px，「卸载」在下；禁用/空态同高不塌陷。
- `extension-panel.js:493` 同步结构。

### 4.2 卸载常态即危险色

- `extensions.css:62` 改常态 `color: var(--bad-ink)`；`hover:not(:disabled)` 追加 `background: color-mix(in srgb, var(--bad) 8%, transparent)`；`:focus-visible` 保留可见描边。
- 作废 2.0.3 的「红色只留 hover/focus」口径。

### 4.3 搜索框

- placeholder → `搜索 npm 包，如 fish-dock-plugin-weather`；命名规则移到下方 note 行；`.extension-search-input` 加 `text-overflow:ellipsis`；`aria-label` 不动。

### 4.4 npm 发现式检索

- 首次进入第三方页以 `fish-dock-plugin` 做默认发现；精确包名走明确查询，普通文字走关键字搜索。
- 延用包名规则，分页 `from`/`size=20`，保留 registry total 与过滤后数量的区别；按规范包名去重，不将一个 registry 页的空结果误当全部结束。
- 搜索有 request generation，旧请求迟到不得覆盖新词；加载更多、断网重试和切页取消不重复追加条目。
- 空结果、离线缓存、registry 报错三态明确；安装前继续说明包结构/摘要校验不等于行为审核。
- 删除原方案自行增加的 `thirdparty-reviewed.json` 与「已核对」标记，不在本轮增加审核维护体系。

后端落点：`plugin_thirdparty.rs`、搜索命令参数/返回值与 `extension-panel.js` 同步迁移；保留旧调用参数的默认值兼容。

### 4.5 验收

- 默认发现、精确包名、关键字、跨页去重、过滤计数、迟到结果隔离、离线重试、安装确认均有行为断言；卸载常态危险色与动作顺序另测。
- 480 / 900 两视口检查布局、键盘焦点、加载与错误态；行高、颜色和裁切以真实浏览器测量与截图验收，DOM 字符串断言只证结构。

---

## 5 T-D：#65 / #66

- `download_source()` 改走 `proxy::web_ways(url)`（照 `plugin_store.rs:104`），超时取 `config::plugin_download_timeout()`；重定向仍手动跟（`max_redirects(0)` + 逐跳 `secure_url`）；DNS/连接重置/TLS/HTTP 码分别出人话（抽公共函数，与 `plugin_thirdparty.rs:214` 同款）。
- `flatten_settings` 增加 `Y::Bool(b) => 0/1`，以 #66 的真实 mod 样本和现有下游类型断言为准；`Y::Null` 暂保留显式错误，不把缺失值静默转成空字符串。表格类（`Y::Tagged`）仍拒。
- `windhawk-panel.js`：状态行按实际阶段和配置预算生成 busy 文案；`<details>` 展开状态由 `sourceOpen` 记住，失败不收起；`error` 渲染到触发按钮下一行并带「重试」；`if(busy)return` 改为按钮 disabled。
- 验收：`windhawk_source.rs` 布尔/Null 单测；`windhawk_collection.rs` 多路回退单测（第一次失败第二次成功）；`windhawk-panel.test.js` 3 条；真机用 #66 的 `taskbar-dock-animation.wh.cpp` 导入成功。

---

## 6 T-E：开机自启拉起（P0）

### 6.1 诊断与冷态锚点

- 先记录登录、进程启动、资源准备、插件校验、spawn、hello、activate、服务健康时间戳。冷态判据建议为 `since_logon <= 300s || since_process_start <= 120s`，在进程入口标记单调时钟；日志未知值单独表示，不当作 0。
- 保留现有冷态键，新增 `timeout.plugin_cold_after_start_secs`；用户显式配置与默认值按 `config` 一致解析。

### 6.2 一个调度器管理总预算与重试

- 原方案的进程内部重试与外层追赶合为一个调度状态机，避免四次重试再乘四次追赶。每插件首次加至多 3 次重试，失败后退避 0.5 / 2 / 5 秒；20 秒扫描只补漏，不再叠加一层重试。
- 一次启动尝试的 hello + activate 共用剩余 deadline：冷态默认 60 秒、热态 10 秒；整次自启生命周期默认 180 秒，从 Dock 自启调度开始计时，排队/校验/退避/重试均计入。到期不再启动新尝试，并回收本次子进程。
- 文件校验与阻塞 I/O 也需有界/可取消；耗尽 60 秒的尝试允许在 180 秒生命周期内重试，但不保证总能完成四次。日志显示实际尝试次数，不承诺必然起来。
- 默认最多两个插件同时启动，每插件只有一个在途 token；慢插件不得阻塞其他候选首次启动。UI 通过可观察的 starting/retrying/ready/failed 状态显示进度，不因 RPC 页桥 20/30 秒超时重复创建启动任务。
- 仅瞬时连接/超时/进程异常重试；身份协议不匹配、摘要失败、非法配置等永久错误立即终止，并显示原因。

### 6.3 代次、锁与取消

- 在途 token 绑定 id、generation、安装摘要、版本；启停、卸载、更新、宿主退出都能作废它。启动前后都在锁外复核注册表的 enabled/autostart 与包版本，回填时在 CURRENT 下只做代次比较和登记。
- 不持 CURRENT 等待 I/O，也不在 MAINTENANCE/EDIT_REGISTRY/EDIT 的反向锁序里回填；占用期间更新/卸载应取消对应启动，不能以“下轮自然排除”替代竞态处理。
- 启动失败、重试中和用户取消分别显示；未到生命周期结束即可显示阶段错误和重试计划，不把可观测性延迟到终态。

### 6.4 验收

- 用注入时钟验证 60 秒阶段共享、180 秒生命周期、最大尝试次数、两槽公平调度和退出取消；使用可控进程夹具验证 hello/activate 超时、永久错误不重试、在途去重及 generation 迟到回填。
- 成功至少覆盖 Clash 与 Magpie 各自的 Node 后端和业务服务健康；宿主实例存在不等于网关就绪。若插件自身超时另有根因，列出插件仓后续修复和宿主兼容要求，不以加大预算遮蔽。
- 真机注销/重启后不操作 Dock，记录登录到 Dock、Dock 到两后端健康的耗时；以调度启动后 180 秒为目标窗口，验证无重复进程、无孤儿服务。到期失败须有明确阶段与日志；单测不能代替登录现场验收。

---

## 7 T-F：双源获取 + 更新提醒

### 7.1 源层

新增 `src/sources.rs`，但把**元数据择源**与**大文件下载**分开：

```rust
pub struct Pick { pub label: &'static str, pub url: String }
pub fn picks(url: &str) -> Vec<Pick>;   // 仅返回有明确映射的候选
pub fn fetch_bytes(url: &str, timeout: Duration, limit: u64) -> Result<Bytes, Fail>;
pub fn fetch_to_file(url: &str, timeout: Duration, limit: u64, out: &Path) -> Result<Source, Fail>;
```

每个候选内部仍走 `proxy::web_ways`。候选按 `mirror.prefer` 排序，但必须保留另一侧回退；不得在未完成摘要/版本比较前以「谁先返回」决定可安装结果。大文件流式写临时文件，逐跳限制大小，校验通过后原子落地。

### 7.2 映射（按用户决定，逐项声明覆盖范围）

| 逻辑地址 | 候选 | 默认 |
|---|---|---|
| `registry.npmjs.org` | `registry.npmmirror.com` | 开（已有，收进 sources） |
| `raw.githubusercontent.com/{o}/{r}/{ref}/{path}` | `cdn.jsdelivr.net/gh/{o}/{r}@{ref}/{path}` | 开，但仅用于 raw 文件 |
| `api.github.com/.../releases/latest` | Gitee API（需 `url.gitee_repo`） | **配了才启用** |
| `github.com/{o}/{r}/releases/download/…` | `{url.gh_proxy}` 前缀 | **默认空** |
| `codex-pets.net` | `{url.pet_mirror}` 前缀 | **默认空**，没有配置时保留原站 |
| Windhawk / LHM | 没有已确认的免费国内镜像 | 原地址 + 系统代理；不得伪造镜像 |

配置键进 `config.rs` Key 表 + 设置页：`url.raw_mirror`、`url.gh_proxy`、`url.gitee_repo`、`url.pet_mirror`、`mirror.prefer`（auto/domestic/overseas）。每个键必须有 getter、实际调用点和回退测试；不能只写入 `KEYS`。

### 7.3 落点（逐项验证，不宣称覆盖所有资源）

`fetch.rs:139`、`plugin_store.rs:101-102`、`updates.rs:1399`、`windhawk_collection.rs:52`、`lhm_setup.rs:937/950`、`sensors.rs:977-1009,1049`，加 `updates.rs:434`（见 7.4）。`crash.rs` / `github_access.rs` / `dsh_bridge.rs` 保持不动，除非测试证明它们属于本轮双源范围。验收矩阵必须列出每个网络出口的候选、超时、摘要和失败回退。

### 7.3.1 宠物预览与其他嵌套资源

- `settings.js:502` 直接输出远程 img；`fetch.rs:294` 只把远程 preview URL 传给前端。这条路径不经 Rust 的代理/择源层，必须一并接入，否则“pet 已覆盖”的验收不成立。
- 宿主通过有域名/协议约束的图片获取与缓存返回本地资源 URL；列表 API、详情、poster、preview、spritesheet 分别验证，版本/缓存键与图片格式/体积限制明确。原站不可达但缓存可用时继续显示离线预览。
- 新镜像主机不得简单放宽 CSP 或下载域名白名单；逻辑来源、候选地址和落盘路径分离，第三方页面不获得通用网络代理。测试应证明 WebView 不再另发原站图片请求。
- bundled 默认宠物、图标、字体、壁纸分为内嵌/本地/远程；本地资源无需双下载。邮件服务、OAuth、用户 CI/天气服务和插件业务网络登记为服务接口，不拿代码/资产镜像替换认证和业务端点。

### 7.3.2 PawnIO 与免费通道缺口

- `sensors.rs:977-995` 当前裸 ureq 下载带 SHA-256 校验，超时 600 秒；安装器与模块均走此路径。接入共享择源/代理/流式预算，同时保留固定摘要和提权进程的来源约束；配置传递按明确值做校验，不把提权路径变成任意 URL 下载执行。
- `auto` 初始按现有区域策略排候选，无有效区域时原站优先；失败后回退并短期记忆同一主机可用来源，避免每张图片重等坏源。所有源×代理共用一次请求总 deadline，非每候选重置预算。
- raw CDN 仅是候选，不保证国内可达。动态分支内容可能陈旧，目录必须校验 schema、版本、资产摘要和缓存时效；同一版本冲突不能以更快的 CDN 值覆盖。
- 无 Gitee、pet mirror 或 Release 加速配置时，尚未满足“每类资源默认国内外双源”；先调研现有免费渠道的真实覆盖、字节一致与可用性，再列实测来源矩阵。未找到时明确标记缺口，不能把默认空前缀算作已完成，也不要求用户部署服务。

### 7.4 更新提醒

- `updates.rs:60` `GITEE_REPO` 改为读 `config::gitee_repo()`（默认空 = 不启用）。
- 配仓后对 GitHub/Gitee 在一个检查 deadline 内合并。只比较同版本、同平台、同资产角色的摘要；镜像落后一版记同步滞后，不误报摘要冲突。另一源超时/离线时使用完整可校验的单源结果并标明降级，已有同版冲突不得由后续单源结果清掉。
- 若采用并行请求，必须等待达到「可信结果」或明确超时后再合并，不能用先返回的未比较结果直接更新。
- `updateFrom=gitee` 从此**真的有效**（修复空转设置项），未配置 Gitee 时设置页不显示可用的 Gitee 强制选项，或明确显示未配置。
- `tell()` 的可见性条件不变（不可见就等下次、不写 `updateTold`），加测试锁住。

### 7.5 验收

- `sources.rs`：区域顺序、镜像失败回落、两路都失败时错误含两边原因。
- 注入式：`url.raw_mirror` 指到坏端口 + `registry` 指到 404 → 官方目录仍能读到（走 GitHub 回退）。
- `updates.rs`：配仓时生产 `endpoints()` 含 Gitee；`updateFrom=gitee` 时 Gitee 排第一；Gitee 失败 GitHub 成功仍返回版本。
- `about-update.test.js`：nudge 出现 → 点击打开更新页 → 写入 `updateTold` 后不重复弹。
- 真机隔离数据根使用新的 FISH_DOCK_TEST_UPDATE_URL 与 source 模式测试开关；源码模式只提示，绝不下载或覆盖正式安装。记录首查 20 秒等待和请求耗时，响应成功后的下一次可见展示应提示；不把“启动 20 秒首查”等同“20 秒内一定显示”。新键在 T-F 当场实现，T-Q 再做全量收口；旧键只做兼容读回归。

---

## 8 T-G / T-H / T-I / T-J / T-K / T-L

### 8.1 T-G 时钟显示秒

- 读 `HKCU\Software\Microsoft\Windows\CurrentVersion\Explorer\Advanced\ShowSecondsInSystemClock`（与现有 `End task` 读取同法），随 look 下发 `seconds`。
- `island.js` / `bar.js` 的 `tickClock` 在开启时画 `HH:mm:ss` 并按秒重绘；侧边条（44 px）字号降到 11 px 以免挤掉托盘。
- 验收：单测读注册表映射（0/1/缺省）；真机开关该设置看任务栏与其它显示器。

### 8.2 T-H Wi-Fi 格数

- 使用 WinRT 的信号 API，并按所用 windows crate 版本核实实际方法名、返回类型和所需 feature；区分 null、断开与 0–5。TUN/代理出口不是 Wi-Fi 物理接口时，依据 Windows 已连接 Wi-Fi profile 取信号，不凭接口名猜实际出口。
- `showNet` 保留 0–5 源值；显示映射与 Windows 真机对照确认。未知/断开使用中性/断开态，不以全亮信号伪装数据。
- 验收：单测映射表；真机与 Windows 自己的任务栏对照截图。

### 8.3 T-I Codex 内部请求不算会话

- agent-core 仅提取内部请求的分类信号；会话适配器按 session/turn 保存过滤状态。已由内部轮开启的会话暂不发欢迎，后续真实用户轮进入时恢复展示；已有用户会话只过滤内部轮的步骤和结算，不改变前一用户轮。SessionStart 已到达时也要延迟展示，不能事后假装挥手未发生。
- 验收：`to_event` 单测 3 条（纯内部轮 / 混合会话 / 仅图片无文字保留原名）。

### 8.4 T-J 会话行展开

- 后端：`state.rs` 的 `files: Vec<String>`（:161）已存在，`view()`（:325）加 `fileList`（上限 15 条，超出给「等 N 个」）；新增 `recentSteps` 环（最近 5 步，含 `ok`/`failed`）。
- 前端会话行可展开查看文件与步骤；「显示具体内容」关闭时，后端投影即移除详情，前端关闭展开并清空已缓存详情；API/SSE 同步遵守，避免只隐藏 DOM。
- 验收：`state.rs` 单测（文件上限、步骤环、跨轮清空）；`settings-order.test.js` 或新测试 2 条；真机看展开与折叠。

### 8.5 T-K 示例目录退役（先解除编译与测试依赖）

**2026-11-03 审查纠正：该目录并非全是死代码，原直接删除方案作废。**

- `src-tauri/src/scripts.rs:15` 通过 `include_bytes!` 编译嵌入 `examples/widgets/waku.ps1`，`:32-45` 仍将它写入运行目录。整目录直接删除会导致编译失败。
- `src-tauri/src/plugins.rs:649-662` 的参数契约测试仍逐个读取示例 PowerShell；`test/fixtures/plugins/ps/record.ps1:22` 及开发文档也引用该目录。
- 先把仍使用的命令包装器迁入正式维护目录，并同步嵌入路径、写出名称与兼容策略；参数契约改为独立、明确的测试夹具，保留行为断言，避免以删除测试换取通过。
- 再逐文件核对 13 个文件的消费者、替代物与文档链接，形成删除白名单，经实现阶段批准后退役冗余示例。本轮仅修订方案，不删除源文件。
- 验收：普通构建与 installed 构建均通过；包装器功能不退化；参数契约与夹具生成测试通过；活动文档无失效链接。

### 8.6 T-L build-release 洗路径

- 新增 `app/scripts/build-release.ps1`：`--remap-path-prefix` 把仓库、cargo home、工具链路径映射成 `crates\…` / `rust\…` / `fish-dock\…`（经 `CARGO_ENCODED_RUSTFLAGS`，带空格的路径保持完整）；构建后扫 exe 里是否残留 `X:\Users\`、仓库路径、cargo 路径，有就失败。
- 必须接入实际 `build-installer.ps1` 双包构建链，处理已有 RUSTFLAGS/CARGO_ENCODED_RUSTFLAGS、Unicode/空格、Rust/本地编译依赖与调试路径。扫描 exe 及最终包的 UTF-8/UTF-16 实际构建根、用户根与工具链根；记录体积差供审阅，不用任意 2% 阈值掩盖功能或内容检查。

### 8.7 T-M 崩溃报告上报（零部署）

**用户已选择零部署路径**：使用 GitHub issue 预填与脱敏文件导出，由用户在浏览器审阅并提交。无需客户端携带 GitHub token。本轮不新增 Gitee 反馈入口，因为尚未配置对应仓库；人工提交与自动 relay 的行为不同，不称为全自动等价替代。

当前基线 `crash.rs:68-69` 的生产 relay 常量为空；不设置测试覆盖时没有生产上报地址。现有报告在本地保存并经过脱敏，缺少可用反馈入口；本轮不据此断言全部历史版本或本地信息已经丢失。

**做法**：

1. **复用现有脱敏器**：`crash.rs:428-451` 已通过 `Redactor::here` 处理报告文本、栈帧和日志；不另写第二套正则。先补 token、用户目录、URL 凭证和非 ASCII 私密词的特征测试，再让预览、导出和预填链接共用同一份脱敏报告。
2. **`issue_link(report) -> String`**：拼 `https://github.com/Fish-under-sea/FISH-Dock/issues/new?title=…&body=…`，并按最终 URL 编码后的长度做上限裁剪，不把“1500 个正文字符”等同于 URL 安全长度。正文包含签名、版本、kind、退出码、栈顶摘要、Windows 版本；超长部分改为提示用户附上导出的完整文件。本轮选择 Markdown 模板并测试 template/title/body 参数；若以后换 YAML form，需另做字段 id 的兼容测试。最终编码 URL 暂限 6000 ASCII 字节，先裁日志再裁栈并保留签名/版本/导出提示；这只是客户端上限，浏览器打开失败仍允许导出。
3. **两个按钮**（关于页 → 崩溃报告区）：
   - 「报告这个问题」→ `open_url(issue_link)`；
   - 「导出报告」→ 存到用户选的目录（`.txt`，已脱敏），可拖进 issue。
4. **中转路径不作为 2.0.4 活动入口**：`can_send()` 与 relay 代码是否保留，按 T-R 标记为历史/可删除材料；本版预填链接和导出是唯一用户路径，不在空地址上制造“已启用上报”的状态。
5. **去重**：沿用现有签名（`crash.rs` 的 `signature`），同一签名只在本地提示一次。
6. **`.github/ISSUE_TEMPLATE/crash.md`**：固定版本、系统、退出码、复现步骤与附件段落，预填参数与模板一同测试；只准备本地模板，不代用户提交。

**验收**：

- `crash.rs` 单测：复用 `Redactor` 验证 token 形状 / 用户目录 / URL 凭证；`issue_link()` 按最终编码 URL 长度裁剪且参数编码正确；超长栈被截断并带提示。
- 前端：关于页有未发送报告时显示「有 N 份 · 报告这个问题」，无报告时不显示（`about-update.test.js` 2 条）。
- 真机：造一次 panic → 关于页出现入口 → 点「报告这个问题」→ 浏览器打开预填好的 issue 页面，正文可读、无敏感路径。

### 8.8 T-N 窗口与弹层测量（一次到位）

用户实测两个现象。代码显示占位测量、ready/layout 两阶段和二次重画路径，但尚未证明它们各自对现场问题的贡献；先采样后修正，不把推测写成确定根因。

**N-1 插件页打开先只有半屏，随后跳到正常高度**

链路（静态代码已核实，窗口根因仍需真机时序证据）：

1. `sdk/browser/bridge.js:54-60` 在 `observeLayout()` 中发 `fish:ready` 后只挂 `ResizeObserver`；文件顶层 `:61` 又重复发一次 ready。首个 `fish:layout` 的到达时序没有独立契约。
2. `extension-page.js:100` `record.height` 初值为 **52**，但 `:48` 它实际是服务工具栏 `44–320 px` 的 fallback，不等于普通插件页整窗高度。
3. `extension-page.js:115-119` 收到 ready 只标记可用，收到 layout 才执行 `toolbarHeight()` → `report()` → `onLayout`（`settings.js:94-98`）→ 面板重绘。

静态证据只能证明存在占位高度和二次测量路径；“半屏”是否由这条链路单独造成，必须用首帧 DOM 高度、原生窗口尺寸和消息时间戳逐帧确认。

修法：
- `bridge.js` 合并重复 ready；ready 后在 DOM 可测时同步调用一次 `reportLayout()`，`ResizeObserver` 只作后续跟踪，并用帧内去重避免循环通知；
- `extension-page.js` 区分普通页高度与 service toolbar 高度：首个有效 layout 前使用宿主侧 per-key 记忆（不要求沙盒 iframe 访问 `localStorage`），无记忆时用明确的稳定占位；收到首个有效 layout 后再显示并定位；首个 layout 有独立超时，超时显示可重试状态，禁止永久隐藏；
- 只在测量证据证明需要时再调整 40 ms 延迟，不把“同帧”当作必然成立的原生窗口行为。

**N-2 审批框弹出后二次轻微拓展**

链路（已核实）：

1. `island.js:4897` `want = sizeOf(view)` 第一次量。
2. `island.js:4901` → `setRoom`（`:1083`）`if (grows) apply()` **立即**长窗口。
3. `island.js:4922` `if (grows) setTimeout(grow, 40)` —— **40 ms 后**岛才长到目标尺寸。
4. `island.js:4931-4938` ask 的「二次测量兜底」：下一帧再量，若更高就**整轮 `update()` 重画**。

这三条路径可能造成分段生长，但原生窗口 resize 与 WebView 合成有异步时序，不能只靠源码确定用户看到的帧数。通过录屏和时间戳判断是否需要调整延迟及重绘。

修法：
- **按目标宽度测量**：内容先在目标宽度参与布局，读取尺寸时计算已完成的布局；额外 offsetHeight 读取仅在采样证实需要时加入。它不保证异步字体、图片和后续内容已经稳定；
- **字体先就位**：页面启动时预热 `document.fonts.ready`；首次 ask 在字体尚未就绪时最多等待 300 ms，超时进入正常测量并保留后续有界尺寸校正；
- **窗口与岛的合成时序**：先用录屏和时间戳确认 40 ms 延迟是否是二次拓展来源；若确认可合并，改为同一帧调度 `window.pet.panel(next)` 与 `setSize(want)`，否则保留延迟并消除多余重绘；不把“同一帧”写成跨 WebView/原生窗口的必然保证；
- **兜底改成不闪**：`remeasure` 差值 ≤2 px 忽略；>2 px 时只调 `setRoom` + `setSize`，**不再整轮 `update()`**（不重画内容，只补尺寸）。

**N-3 「她旁边」的面板只在打开时量一次**

`panel.js:276-282` `report()` 只在 rAF 里测一次 `offsetWidth/offsetHeight`，没有后续观察；内容是后来变高的（`panel.js:270-271` 隐藏行转显示、按钮换行）就靠各处手动补调。

修法：给 panel 挂 ResizeObserver，与首次 report 统一去重；同一帧只发送最后尺寸，变化 >1 px 才上报。回调不直接反向改变观察目标尺寸，关闭/销毁即 disconnect；连续跨帧变化按需更新，避免承诺“任何两次变化都只发一次”。

**验收**

- `extension-page.test.js`：普通页与 service toolbar 分别验证；首个有效 `fish:layout` 到达前后可见性、首个 layout 超时、宿主侧 per-key 记忆隔离各一条；不依赖跨源 iframe 的 `localStorage`。
- 岛侧：`sizeOf('ask')` 前的 reflow、字体等待超时、40 ms 调度策略和「差值 ≤2 px 不重画」各一条单测（走现有 jsdom 夹具）；原生窗口时序只能由真机录屏验收。
- panel 测试同帧合并、跨帧动态内容、尺寸未变不发、关闭后断开、消息与布局循环终止。
- 真机录屏三个场景：① 打开插件页（无跳变）② 短审批（一次到底）③ 带 diff 的长审批（无二次生长）。逐帧确认。

---

## 9 T-O：OFM 业务解耦与最小旧安装兼容

### 9.1 必须移除的活动业务

- `plugin_development.rs` 不再接受 `ofm` 开发条目，不再写入 OFM 的 `gateway.json` 或 38080 默认端口。
- 宿主不再携带 OFM 服务载荷、OFM staging 脚本、OFM 服务模块、OFM 业务页面、渠道注册或账号业务；安装器继续拒绝 `ofm/`、`dsh-our-free-model/`、`our-free-model/` 资源前缀。
- `plugin_package.rs` 的官方发行测试只验证现行 `feishu`、`clash`、`magpie`，不再把退役 OFM 当成发行包；测试夹具中的 OFM 仅保留在 legacy 兼容测试，并明确命名为 `retired`。
- `identity-panel.js`、`identity_api.rs`、`storage.rs`、`identity_store.rs`、`identity_archive.rs` 的当前宿主路径不再展示或生成 OFM 业务备份；旧格式只做识别、隔离和“不支持当前宿主导入”的明确反馈。

### 9.2 必须保留的兼容边界

- 已安装的 fish.ofm 不因 2.0.4 自动停用、卸载、改名或迁移；仍可通过独立插件通用管理启停/卸载。新增官方目录不再提供它；旧包解析支持不得误当默认安装或更新源。
- 旧命名空间 `.fishplugin-ourfreemodel`、旧安装清单和旧备份格式集中到显式 legacy 分支；新安装不创建退役业务目录。已有独立插件的验证/回滚/数据保留继续可用。通用整仓备份可按用户现有命名空间原样归档，不新增 OFM 账号解析、专项导出或自动恢复；不以清理为由静默遗漏用户资料。
- 宿主邮件/配置事务使用宿主自己的锁；OFM 专用管道/锁检查只在确实访问旧 OFM 资料的迁移边界保留，禁止让普通宿主导出依赖退役服务锁。不复制或删除旧模型资料。
- `plugin_page.rs` 的本机 gateway 能力按已安装插件清单和 loopback 票据校验；若保留 `fish.ofm` 兼容特判，必须由 legacy 常量集中定义，不扩展为任意插件可打开本机服务。

### 9.3 验收

- 新安装资源清单和解包目录中无 OFM 载荷；安装器对 3 种退役前缀、未列文件和空目录均有回归测试。
- 新用户开发预览无法安装/配置 OFM；现行三插件发行验包不依赖宿主版本号相等，而以 catalog 的插件版本、平台、字节数和 SHA-256 为准。
- 已装旧插件可列出、启停、卸载；卸载后只移除其受管包目录，不删除用户资料和其他插件。
- 旧 OFM 包、混合包、宿主包分别测试：只识别不误导、拒绝范围明确、现有文件字节不变。

## 10 T-P：活动品牌与集成输出迁移

- 盘点并迁移 `.claude-plugin/marketplace.json`、Claude plugin manifest、DSH/OpenCode/OMP/Thunderbird 集成的文件名、插件 ID、包名、主页、hook query、全局哨兵和生成文案；新安装只写 FISH Dock 归属。
- 对必须兼容的协议采用“新名写出、旧名只读”：旧 `WakuwakuLoaded`、旧 hook 参数、旧环境变量和旧文件仍能识别，但新代码不继续生成新的上游归属文件。
- `gitee-release.ps1`、`app/relay/*`、预览夹具和文档中指向 `zhoupengjie/Wakuwaku` 的活动操作，改为 FISH Dock 参数化入口或标记为历史归属；默认禁止向上游仓库发布/开 issue。
- 保留 LICENSE、UPSTREAM.json、SOURCE.md、原作者署名和历史发行说明中的事实归属；不把版权归属当作活动产品命名。
- 覆盖前端页面 title（index.html 仍为 DSH-FISH）、健康标识与 install-stop 消费方、User-Agent、临时目录和 CODEX_CHAIN 的迁移。旧安装用文件摘要/路径和受管配置证明归属后备份再替换；独立上游实例保留。Thunderbird 扩展 ID 改动需提供显式替换/卸载提示，防双扩展重复上报。

验收：全仓命名扫描按四类输出（活动输出、只读兼容、版权归属、历史文档），活动输出不再默认写上游名称；新旧集成各做一次安装/卸载/重复运行测试；真实第三方配置路径不在测试中被覆盖。

## 11 T-Q：配置入口收口

- `runtime.rs:534-538`、`windhawk_bundle.rs` 等实际资源路径统一调用 `config::resources_override()`；`FISH_DOCK_RESOURCES` 成为新名，`DSH_FISH_RESOURCES` 仅作为受限旧别名。
- `WAKUWAKU_*`、`DSH_FISH_*` 的读取按用途分类：产品兼容、开发测试、历史迁移。每个旧入口必须有新入口优先级、弃用说明和测试；没有新入口的测试专用变量不得进入产品配置面。
- `config.rs::KEYS`、`docs/config-keys.md`、README 和安装器清单从同一事实源核对；补齐冷启动、sources、镜像和资源路径键，修正 2.0.0/2.0.1/49 键等过时描述。
- 更新测试环境变量到 `FISH_DOCK_*` 或 `FISH_DOCK_TEST_*`；仅在迁移测试中保留旧名，并确保旧名不会覆盖新名。

验收：对每个 `KEYS` 条目做“元数据—getter—调用点—测试”四项核对；新旧环境变量优先级、空白值、相对路径和资源目录不存在均有测试；installed 构建不再直接读取未登记的旧资源键。

## 12 T-R：旧材料分层、发布契约与总验收门

- 把 `docs/installer-build.md`、1.x/2.0.x 历史 release 文档、`app/relay/` 部署材料、旧预览包和 `app/examples/` 逐项标为“活动文档 / 历史归档 / 测试夹具 / 可删除文件”；更新 SECURITY.md、SOURCE.md、配置文档和中英文 README 的当前入口，历史 release 原文只加归档边界，不直接删除有归属或可复现价值的材料。
- `app/examples/widgets/` 先按 T-K 解除 `include_bytes!`、参数契约测试和 fixture 依赖，再按删除白名单退役；`src-tauri/examples/` 单独盘点，不能与 widgets 目录混删。
- 测试入口覆盖 Rust workspace、Rust integration/ignored 分类、`app/test/*.test.js`、`app/test/*.test.mjs`、`app/scripts/*test.mjs`、SDK 测试和安装器测试；环境性失败必须记录实际错误、命令和豁免边界。
- 版本 2.0.4 核对 `Cargo.toml`、`tauri.conf.json`、插件 manifest、安装器资源 manifest、README 生成物、Setup/Update 文件名和 `.sha256`；构建包内容必须再次扫描 OFM 载荷、上游活动输出和本机绝对路径。

验收：审计报告中的 A01–A20 均有对应任务、代码证据或明确关闭理由；全量命名/OFM/资源扫描产生可审阅清单；合成回归、布局真机、双包校验和升级现场分别记录，全部必需项通过后才给出发布结论。

---

## 13 实施顺序、文件所有权与任务依赖

原七分支「相互独立」的描述撤销。以下是建议分支边界，不在本轮创建；按顺序交接重叠文件，未经再次切分不并行写同一文件。

| 顺序 | 建议分支 | 任务 / 前置 | 主要共享文件与交接 |
|---|---|---|---|
| 1 | `fix/autostart-204` | T-E | config/main/plugin_process/plugin_runtime/plugin_api；优先完成 P0 |
| 2 | `feat/sources-update` | T-F，承接 T-E | config/updates/fetch/plugin_store/lhm_setup；产出下载接口供 T-D 使用 |
| 3 | `fix/windhawk-204` | T-D，依赖 T-F | windhawk_collection/source/panel；不复制第二套 sources 逻辑 |
| 4 | `feat/plugin-card-204` | T-C | extension-panel/extensions.css/搜索命令接口 |
| 5 | `fix/layout-jump-204` | T-N | SDK/extension-page/island/panel/settings；先时序采样再修复 |
| 6 | `feat/agent-core-hub` | T-A → T-I → T-J → T-B | events/state/main/server/settings/Cargo；等价提取与行为变更分提交 |
| 7 | `feat/crash-report` | T-M | crash/关于页/模板；承接 settings 修改 |
| 8 | `chore/host-decouple-204` | T-O → T-P → T-K | identity/storage/插件 ABI/集成/scripts；先固定 legacy 清单，再迁移品牌与包装器 |
| 9 | `feat/taskbar-indicators-204` | T-G / T-H | island/bar/widgets/windows features；业务不依赖 sources，但串行交接共享文件 |
| 10 | `chore/config-contract-204` | T-Q，依赖 T-E/F/P/G/H | 统一已有新配置和旧别名；任务增加键时就接线，不延迟到此步才生效 |
| 11 | `chore/release-build-204` | T-L，承接全部代码 | build-installer/build-release；接入实际双包入口并做路径扫描 |
| 12 | `release/2.0.4-gate` | T-R，依赖 T-A–T-Q 全部通过 | 版本、文档、包内容和总验收；发布仍须明确指令 |

语义前置以范围表为准；上表额外规定共享文件的串行交接。T-O 对 T-C/T-M 的等待是合并顺序，不是 OFM 清理的技术前提。T-P 安装归属迁移先于 T-K 包装器最终改名。T-R 可以提前维护测试清单，但最终结果只能在全部任务通过后给出。

## 14 验收与基线

### 14.1 命令与证据

前轮记录的 742 passed / 2 failed / 17 ignored、Node 398/397/1 与 SDK 8/8 是历史参考。本轮只改文档，未运行产品测试；实施前重新记录基线，新增用例按行为覆盖评估，不设凑数指标。

Rust 命令从 `app/src-tauri` 执行；workspace 由 T-A 添加后使用 `--workspace`，之前对宿主运行同等命令：

```powershell
cargo test -j 1 --locked --no-default-features --workspace -- --test-threads=1 --skip t30
cargo test -j 1 --locked -p agent-core
cargo check -j 1 --locked --workspace
cargo check -j 1 --locked --features installed
```

Node 命令从仓库根执行，覆盖 JS、MJS 和 SDK：

```powershell
node --test --test-isolation=none "app/test/*.test.js" "app/test/*.test.mjs" "app/scripts/*.test.mjs"
node --test --test-isolation=none sdk/node/plugin.test.mjs
node tools/check-readme.mjs
```

- 实现前先列出仓库的实际测试文件和可选环境门，确认上述 glob 不漏子目录/不同后缀；`t30` 和 ignored 用例单列替代证据与真实运行环境，不以 skip 等同通过。
- `lhm_setup` DACL、任务计划程序 `0x80070003` 和受限进程环境只对已复现的具体用例记环境异常；新失败不自动归入豁免。记录命令、退出码、用例名和错误证据。
- 发布构建需先加载本工作区已有双包构建资产，沿实际入口执行；`cargo check` 不代替 installed release 构建或启动验收。

### 14.2 覆盖矩阵

| 任务 | 必须闭环的证据 |
|---|---|
| T-A / I / J | 八类 agent 等价转换与异常输入、跨轮内部请求隔离、文件/步骤上限、隐私开关关闭不向前端/API输出详情 |
| T-B | HTTP 鉴权/Host/Origin、客户端 proof、快照订阅交接、重启游标、SSE 背压/写超时、退出释放及其他端点不被阻塞 |
| T-C / D | 搜索分页/去重/迟到请求、键盘与危险动作、Bool 类型保真、Null 错误、下载 fallback/动态超时、真实 #66 mod |
| T-E | 生命周期/单次 deadline、有限并发、在途去重、代次取消、永久错误、两个真实后端健康和登录现场日志 |
| T-F | 每个出口的候选来源与缺口、源切换×代理切换、重定向、缓存陈旧/损坏、摘要冲突、下载原子落地、宠物提醒全链路 |
| T-G / H / N | Windows 秒显示、网络未知/断开/TUN、插件工具栏/普通页/业务页分别测量、首个 layout 超时、缩放/多屏/长 diff 录屏 |
| T-K / L | 包装器实际可用、参数契约不依赖删除目录、普通与 installed 构建、UTF-8/UTF-16 本机路径扫描 |
| T-M | 同一脱敏对象预览/导出/预填、最终 URL 长度、正确仓库/模板、取消不视为已提交、不把浏览器打开当作发送成功 |
| T-O / P / Q | 新安装零 OFM 载荷、旧插件可管理/保留数据、上游真实安装共存、新名写出/旧名只读、每个配置的实际调用点 |
| T-R | #63/#65/#66 验收与 #64 安装配置回归、当前文档与版本位一致、全量扫描、双包安装升级/回滚、精确记录未验收项 |

### 14.3 发布总门

- 构建前完成合成数据回归和布局浏览器检查；构建后再验安装/升级与真实 WebView2，不把所有真机验证推迟到打包之后。
- Setup 和 Update 均 `7z t`；各自带 `<exe>.sha256`，核对真实字节与 Release 资产；最终包扫描 OFM 载荷、未登记文件、本机绝对路径和活动上游目标。
- 版本位核对 Cargo.toml/Cargo.lock 宿主条目、tauri.conf.json、Claude manifest、UPSTREAM、双包元数据、README 生成物与文件名。业务插件版本从独立 catalog 核对，不能强行改成 2.0.4。
- 审计 A01–A20 每项附修复、证据或明确未关闭原因；已知缺少的默认国内镜像和待真机验收必须写入发行限制。未实现项不从清单中静默消失。

## 15 风险与已知限制

| 风险 | 约束 |
|---|---|
| 2.0.4 含架构变更 | 保留用户版本决策；T-A 只提取可独立逻辑，registry/asks 留宿主，不额外建设市场或审核体系 |
| 自启永久失败 | 有界 deadline 和明确终态，不承诺“最终一定起来”；保留旧配置解析，记录插件侧业务超时 |
| OFM 清理误伤 | 当前业务退出与旧安装兼容分开；不自动删除用户数据，不改写历史许可；最小 legacy 分支有回归 |
| 上游集成改名 | 迁移需校验文件/配置归属并备份；真实 Wakuwaku 与本产品并存，双加载及回滚必测 |
| 镜像覆盖不足 | 免费 CDN 可作为候选而非国内可达保证；无 pet/Release 镜像时如实显示单源，不宣称全资源默认双源 |
| `/v1` 扩大数据出口 | 本地私有 token、loopback、受限 Origin、投影与隐私开关联动；不扩大现有审批端点能力 |
| 窗口测量 | 强制布局不解决异步字体/图片；首个 layout 有 deadline，晚消息按代次隔离，最终以 WebView2 真机验收 |
| 崩溃反馈 | 浏览器预填需用户提交；编码 URL 上限与导出降级，不部署 relay，不将未提交标成已发送 |
| 历史基线过时 | 实施前复测，环境豁免精确到用例；本轮文档审查不声称产品检查已通过 |

## 16 明确不做（归档）

- 上游 web addons、市场与资源包全套；本轮只强化现有 npm 发现。
- 桌面直说、agy PreToolUse 子系统、完整 registry/asks 下沉、完整 replay 平台。
- `pri.rs` 资源解析；沿用既有任务视图等价实现。任务栏拔河与宠物代理已有实现仅做回归，不重复移植。
- 用户自行部署中转、默认为第三方加速前缀、自动删除旧模型资料、自动改写用户真实第三方配置。
- 本轮不提交、推送、发版或关闭远端 issue；这些不由文档修订自动触发。

## 17 追加项（2026-11-03 用户指定）：#67 插件页外链

**问题**（GitHub `Fish-under-sea/FISH-Dock#67`，P1）：`app/src/pet/extension-page.js` 给 iframe 设的
`sandbox="allow-scripts"` 既无 `allow-popups` 也无 `allow-top-navigation`，页面自身 CSP 又是
`connect-src 'none'`，于是插件页里 `<a href>`、`target=_blank`、`window.open` 全部无效——Magpie 签到页、
中转站探针与任何第三方页面里的文档链接都点不动。属框架级限制，不是单个插件的问题。

**选择**：采用 issue 里更可控的第二条（宿主受控 `openExternal`），**不放开 iframe 沙箱**。

| 层 | 改动 |
|---|---|
| 宿主 | `plugin_page.rs::call` 增 `dock.open_external` 分支（与 `dock.autostart` 同层，先过票据 `checked()`）；URL 交给既有 `browser::open`，其 `web_url()` 已限制 `http/https`、host 非空、无 userinfo、含 NUL 拒绝 |
| 桥 | `sdk/browser/bridge.js` 暴露 `FishDock.openExternal(url)`；拦截页内 `a[href]` 点击，仅 `event.isTrusted` 且 scheme 为 `http/https` 时 `preventDefault` 后转宿主（覆盖 `target=_blank`） |
| 约束 | 程序化调用要求 `navigator.userActivation?.isActive`；不支持的实现按“仅 trusted 点击”处理；同页短时速率限制；`file:` / `javascript:` / `data:` / `vbscript:` 一律不拦也不开 |
| 不改 | `extension-page.js` 的 sandbox 保持 `allow-scripts`；不新增 `allow-popups` / `allow-top-navigation` |

**验收**：插件页点 `https://` 链接能在系统浏览器打开；`file:`/`javascript:`/`data:` 被拒；非用户手势不弹；
既有 ready/layout/theme 与 `fish:call` 通道不回退。**“真机点开系统浏览器”属现场验收项**，单测只能证到
“请求发出且 URL 经过校验”。

**任务**：T-S1（实现：`plugin_page.rs` + `bridge.js` + 测试）→ T-S2（非作者复审）→ 并入 T-R 总门与双包。
