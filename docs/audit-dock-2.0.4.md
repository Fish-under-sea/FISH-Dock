# FISH Dock 2.0.4 方案与遗留审查

> 审查日期：2026-11-03（Asia/Shanghai）。代码基线：`main` @ `84db441`，宿主 2.0.3。
> 本轮只读产品代码、修改任务文档；未删除源码或用户资料，未编译发行包，未提交、推送或更改远端议题。
> 审查开始时工作树只有未跟踪的 `docs/plan-dock-2.0.4.md`。本文行号指代码基线，不随方案改写迁移。

## 1 结论与证据边界

原方案需要修订后再实施。最明确的阻断项不是命名风格，而是 **直接删除示例会破坏构建**、**重试预算与三分钟验收相矛盾**、**资源配置的新键没有贯通实际入口**、**插件验包仍要求已退役 OFM 且绑定宿主版本**。

本地源码树已无原 OFM 服务副本与旧暂存脚本，但宿主仍含 OFM 专项开发配置、网关桥特判、旧备份解析及界面文案。应执行「业务退出宿主、已有独立插件兼容明确隔离」，而非字符串全删。

### 1.1 本轮做了什么

- 阅读方案全文，按文件位置追查启动、资源、身份、插件、SDK、安装器及集成脚本。
- 用 `git ls-files` 区分已跟踪文件；搜索结果逐条区分生产路径、调试路径、测试、历史记录、版权来源。
- 对仓库内已知载荷位置执行存在性检查；未读取用户主目录、真实账号、现装 Dock 数据或外部插件服务资料。
- 核对浏览器布局与 GitHub issue 模板的官方说明，用于限定后续实现契约，未把静态推断写成真机结论。

### 1.2 本轮未证明的事项

- 未拿到本次 Windows 登录的启动时序日志，因此「超过 300 秒导致双超时」只是待检验假设。
- 未录制真机窗口逐帧视频，52 px 是插件记录初值，且服务工具栏有 44–320 px 限制；它不等于整个窗口的真实初始高度。
- 未检查当前机器已安装包或历史发行二进制，源码树没有 OFM 载荷不等于现装环境和全部旧包都没有。
- 未重跑完整 Rust/Node 测试；原方案的 742/398/8 等数字仅为前轮记录，不是本轮验证结果。
- 未重新查询远端 issue 状态；#64 按前轮记录列入回归，不声称本轮重新确认其状态。

## 2 方案审查发现

| 编号 | 分级 | 证据与问题 | 修订要求 / 任务 |
|---|---|---|---|
| A01 | 必须修复 | `scripts.rs:15` 编译嵌入 `examples/widgets/waku.ps1`，`:32-45` 仍写出；`plugins.rs:649-662` 逐个读取示例做参数契约测试。原 T-K 称 13 个文件全是死代码。 | 先迁移正式包装器和契约夹具，再按白名单退役目录；T-K。 |
| A02 | 必须修复 | `plugin_process.rs:76-78` 分别为 hello 和 activate 计时。原案四次、每阶段 60 秒，单插件理论等待可达 `4×(60+60)+0.5+2+5=487.5` 秒，尚未算验证和进程创建；`plugin_runtime.rs:195-223` 又串行处理。 | 用单次共享 deadline、整轮总预算和公平调度取代预算乘法；T-E。 |
| A03 | 建议修改 | 冷态判据、串行失败和缺少追赶均有源码证据，但「轮到插件常已过 300 秒」「Node 必然超时」没有本次运行数据。 | 根因降级为假设；日志先区分资源、verify、spawn、hello、activate、业务服务健康；T-E。 |
| A04 | 必须修复 | 原案仅用 `present()` 判断待启动；在途启动尚未进实例表。现有代次保护见 `plugin_runtime.rs:179-219`，新追赶若另起一套会重复 spawn 或回填停用/升级前实例。 | 每插件单在途、代次与摘要绑定、取消信号；启动前后复核 enabled/autostart/版本；T-E。 |
| A05 | 必须修复 | 原案写「双源谁先有结果用谁」，同时承诺两边摘要冲突阻止安装。若提前返回，则尚未收集另一边结果。 | 元数据比较和下载择路分离；相同版本/平台/资产做摘要比较，有界等待并标记单源状态；T-F。 |
| A06 | 建议修改 | pet 和 Release 镜像默认空，raw 的 jsDelivr 不是任意站点镜像；因此「全资源国内外自动双源」尚未满足。 | 每类资源列实际可用候选、覆盖缺口及离线路径；保留免费、零自建服务约束；T-F。 |
| A07 | 建议修改 | 原 `get()->Vec<u8>` 设计拟覆盖更新包和 LHM，未规定大包流式处理、总预算、重定向和摘要来源。 | 小元数据与大包下载分别定义契约，逐跳校验、流式临时文件、独立可信摘要、原子落地；T-F。 |
| A08 | 建议修改 | 范围表写 registry 下沉，T-A 正文却暂留宿主；各 agent 原始转换输出未逐项盘点。T-I 的跨轮过滤又需要状态。 | 固定最小切片边界，提取与行为改动分提交；过滤状态保留在现有会话适配器；T-A/T-I/T-J。 |
| A09 | 建议修改 | `/v1` 未定 SSE 工作线程上限、重启后的 seq、单事件字节上限、token 权限与轮换；「客户端验 proof 才发送 token」也不是服务端单测能单独证明的。 | 先验证现有服务器分发方式；定义 epoch、快照/订阅原子交接、背压、客户端协议测试；T-B。 |
| A10 | 建议修改 | T-C 自行增加「已核对清单」，超出用户选择的 npm 发现强化；按包名打标会让后续不同版本继承审核暗示。 | 移除此本轮扩项；发现、分页、去重、取消过期请求和安装告知足够；T-C。 |
| A11 | 建议修改 | Windhawk 原问题是 Bool；原案顺带把 Null 变空串，缺类型语义证据，且 busy 文案固定 20 秒与可配置超时相冲突。 | 只修已证 Bool，Null 保持显式错误直至有规范与样本；文案来自实际阶段/预算；T-D。 |
| A12 | 必须修复 | `crash.rs:428-451` 已用 `Redactor::here` 脱敏所有文本、栈帧和日志。原案「搬进 Rust、40 行正则」忽略现有实现。 | 复用 `crash/redact.rs`，只补缺漏测试；同一脱敏对象供预览、预填与导出；T-M。 |
| A13 | 建议修改 | 1500 个正文字符不等于最终百分号编码 URL 长度；YAML issue form 与普通 body 参数也不是同一契约。 | 按最终编码 URL 字节裁剪，模板字段联测；默认 Markdown 模板加导出降级；T-M。 |
| A14 | 建议修改 | SDK 在 `bridge.js:55,61` 两次发 ready；`extension-page.js:102-118` 收 ready 即取消超时，却未保证收到 layout。隐藏 iframe 后等 layout 可能永久空白。 | ready/layout 两阶段，首个有效尺寸独立 deadline、迟到消息隔离；宿主负责缓存，不要求沙盒 iframe 访问 localStorage；T-N。 |
| A15 | 建议修改 | `extension-page.js:48` 的 52 px 实际用于服务工具栏默认值。T-N 将其描述成整窗高度，并把「删 40ms + reflow」等同原生窗口同帧到位，证据不足。 | 区分工具栏/普通页/业务 iframe；观测原生 resize 与 DOM 布局时序；避免盲目删延迟；T-N。 |
| A16 | 建议修改 | `ResizeObserver` 内再次改尺寸可能造成循环；DOM 夹具不等于布局引擎，原案只用单测证明无跳变不充分。 | 帧合并、去重与断开；实浏览器测量 + WebView2 原生录屏，两类证据分开；T-N。 |
| A17 | 建议修改 | 分支标成独立，但多任务同时改 `settings.js`、`island.js`、`config.rs`、`server.rs`、`main.rs` 和 Cargo。时钟/Wi-Fi 与 sources 也不构成业务依赖。 | 按文件所有权及合并前置编排，P0 自启靠前，先取消假并行；§9。 |
| A18 | 建议修改 | 固定新测条数替代覆盖契约；Node 命令只收 `*.test.js`，漏 `*.test.mjs` 与 `app/scripts`；版本检查漏 Cargo/插件清单/来源元数据。 | 建覆盖矩阵、全命令清单、环境失败精确豁免及跨版本回归；T-R。 |
| A19 | 必须修复 | `settings.js:502` 将 `g.preview` 直接写进 img；`fetch.rs:294-301` 返回原站图片 URL，`index.html:7` 允许 codex-pets 远程图片。API 经 Rust 不代表缩略图也经统一代理/择源。 | 缩略图/预览图也由宿主受限获取并缓存，给前端本地 URL；测试浏览器不再直连原站；T-F。 |
| A20 | 必须修复 | `sensors.rs:977-995` 为 PawnIO 单独构造裸 ureq，固定 600 秒；`:1009,1049` 下载模块/安装器，原出口表完全遗漏。 | 接入共享代理/择源与流式总预算，保留固定摘要和提权来源校验；T-F/T-Q。 |

A19/A20 的「必须修复」针对用户要求的统一资源获取契约，不表示本轮已发现这些第三方资源遭篡改。补扫还发现 `index.html:9` 的活动标题仍是 DSH-FISH，已纳入 T-P。

## 3 OFM 残留：逐类处置

### 3.1 已确认不在本地源码树中的载荷

存在性检查结果均为 absent：`dsh-our-free-model/`、`app/.runtime/`、`app/src-tauri/resources/`、`app/resources/`、`app/scripts/stage-ofm.mjs`、`app/src-tauri/src/oflm.rs`。

安装器已有防回流：`app/scripts/installer.mjs:96-123` 对 `ofm/`、`dsh-our-free-model/`、`our-free-model/` 清单前缀及未登记文件做检查。保留并补测试，而不是为了「零命中」删掉防线。

### 3.2 活动业务耦合与兼容层

| 位置（仓库相对路径） | 核实性质 | 处置 |
|---|---|---|
| `app/src-tauri/src/plugin_development.rs:16,26` | debug 入口仍接受 OFM，并代写 38080 网关配置；不是 release 默认内置服务 | 移除 OFM 开发自动安装/配置；Magpie 业务默认也不应继续向宿主扩散。 |
| `app/src-tauri/src/plugin_page.rs:71-77` | `gateway.open` 显式允许 OFM 和 Magpie | 隔离既有独立插件 ABI 兼容；维持 loopback、票据、身份约束，避免无条件扩大给所有插件。 |
| `app/src-tauri/src/identity_store.rs:8-35` | OFM 文件白名单、账号/凭据分类仍在宿主共用模块 | 拆出历史识别边界；宿主当前备份/恢复只处理宿主资料，通用 ZIP/事务/ACL 功能保留。 |
| `app/src-tauri/src/identity_archive.rs:23-47,61-119` | 旧 OFM 备份校验与生成逻辑；host 已挡含 OFM 的旧包 | 下线宿主 OFM 导出/恢复业务，保留格式识别与明确反馈；去掉「去已下架插件入口」的失效建议。 |
| `app/src-tauri/src/identity.rs:98-102,286-343` | OFM 命名管道/锁逻辑；`identity_api` 和 `identity_transfer` 仍调用 operation_guard | 邮件/宿主操作使用宿主自己的事务互斥；旧 OFM 锁仅留在确实访问旧资料的显式迁移边界。 |
| `app/src/pet/identity-panel.js:55`、`identity_api.rs:93` | 通用备份预览仍输出 OFM 账号字段/文案 | 当前格式显示文件数、命名空间和邮件摘要；历史格式识别才给兼容提示。 |
| `app/src-tauri/src/storage.rs:70,178` | OFM 旧命名空间映射；每个命名空间 README 都带 OFM | 映射收进受限历史兼容表，防已有安装失联；通用 README 移除特定业务名称。 |
| `app/src-tauri/src/plugin_package.rs:59-70` | 可选验包测试仍要求四包并断言插件版等于宿主版 | 三个现行插件按独立 catalog 的版本/摘要验包；旧 OFM 仅合成兼容测试。 |

**不是活动内置业务的命中**：`plugin_install.rs:710` 后已进入 `mod tests`，`:722` 的 OFM 名称是合成包夹具；`github_access.rs:225-228` 已只约束三个当前官方插件。禁止把这些命中写成「仍会内置启动 OFM」的证据。

通用测试换为中性插件或真实 Magpie 名称；退役 catalog 拒收、旧安装可管理、命名空间隔离和用户数据保留等 OFM 专用回归保留，并统一标记 legacy。`sizesOfMember`、`nameOfMember` 是宽泛 OFM 搜索的误报。

### 3.3 清理边界

- 零内置指：不携带服务载荷，不新增自动安装/网关配置，不维护模型渠道/账号业务，不在当前界面推广退役服务。
- 保留前轮「已有独立 OFM 插件可管理」承诺：不自动停用、卸载、重命名数据目录或映射成 Magpie。现有独立插件所需协议、存储映射显式列入最小兼容表。
- 原始许可、作者署名、历史 Release 说明和防回流测试保留。仅为降低 grep 命中量而抹掉归属，不是有效清理。
- 混合旧备份包含模型资料时，先明确报告不支持的范围并保持输入、现有资料不变；本轮不新增暗中拆包迁移或自动修复用户账号功能。

## 4 上游命名与旧配置

| 证据 | 结论 / 任务 |
|---|---|
| `app/.claude-plugin/marketplace.json:2-11`、Claude 插件 manifest、`hooks/hooks.json` | 当前输出仍用 wakuwaku 身份及 hook 标记；改名需覆盖仓库 source 相对路径、连接/移除/重装，不是只改显示名。T-P。 |
| `app/integrations/dsh/{package.json,wakuwaku.js}`，`events_dsh.rs:493` | 发送方和接收方共同依赖 `WakuwakuLoaded`；双读新旧、只写新名，并防双加载。T-P。 |
| `app/integrations/opencode/wakuwaku.js:25,213`，OMP 模板 | 文件、插件 ID、globalThis 哨兵、hook 标记和子进程环境共同构成协议。要按归属迁移，保留真实上游共存。T-P/T-Q。 |
| Thunderbird manifest、build.ps1、README | 产品名/主页/输出包仍是上游；扩展 ID 变更可能形成第二份扩展，需要显式升级方案。T-P。 |
| `app/src/shared/i18n.js:824,1370-1423,1521` 及英文对应 | 当前帮助仍提示 Wakuwaku 文件、DSH 配置项与 `/plugin uninstall wakuwaku@wakuwaku`；跟生成逻辑同时改。T-P。 |
| `app/scripts/gitee-release.ps1:25-28,44,79,92` | 实际默认仓库是 zhoupengjie/Wakuwaku，且有远端写入代码；不是纯注释。先退役或参数化并校验目标归属，禁用上游默认。T-P。 |
| `app/relay/wrangler.toml`、README | 保留上游部署目标与「已部署」指令，与零部署决策冲突；归档/移除活动部署入口，保留必要历史来源。T-M/T-R。 |
| `config.rs:320-322` 对比 `runtime.rs:534-538` 与 `windhawk_bundle.rs:21` | config 接受 FISH_DOCK_RESOURCES，但实际资源路径仍直接读 DSH_FISH_RESOURCES；文档宣称新键已接线不成立。T-Q。 |
| `config.rs:58`、`server.rs:109`、`updates.rs:71-72,178`、`desk.rs` | 数据覆盖、debug、更新测试与追踪仍只有旧名入口；建立新名优先的集中解析，保留只读旧别名。T-Q。 |
| `config-keys.md`、README 的 49 键描述 | 文档缺冷启动键，路径 `dock/app/...` 也过时；从 KEYS 生成/核验，消除手工计数漂移。T-Q/T-R。 |

法律归属与资产来源类保留：LICENSE、`licenses/Our-Free-Model.txt`、`licenses/Wakuwaku.txt` 的分发对应、UPSTREAM 历史对象、原宠物作者。图标文件名可更名但字节和授权记录不改。旧卸载键、旧 exe 名、旧 Run 项及兼容读入口保留到受限清单，不按品牌替换误删其他安装。

## 5 旧文件与文档债

- `SECURITY.md:5` 仍称维护 1.1.x；`SOURCE.md:1,16` 标 2.0.1 并链接 2.0.0；这些是当前入口，需更新。
- `docs/installer-build.md` 当前构建说明混入 1.0.0 的内置 OFM 目录、命令和产物；拆分当前步骤与历史记录，避免照旧文档重新带回载荷。
- `app/.gitignore:5-9` 仍描述已删除 stage-ofm 脚本；忽略规则可保留作为防误提交，注释改为退役资源隔离。
- `dist/portable-2.0.1/` 两个脚本和 `build/review/2.0.1-预览怎么用.md` 是已跟踪历史预览材料，不是未经确认可删的垃圾；明确归档状态，不当 2.0.4 构建输入。
- `app/src-tauri/examples/taskbar_spike.rs` 与 `app/examples/widgets` 不同；未证实无消费者，先盘点 Cargo 示例目标再决定，避免一并误删。
- 两处 `.github` 均不存在；原计划「同步 CI」应改成先确定测试入口，新增 CI 属明确交付项而非假定已有。

## 6 修订后任务映射

原 T-A–T-N 保留任务 ID，修订错误契约；新增：

- **T-O：OFM 业务解耦与最小旧安装兼容**。
- **T-P：活动品牌/集成输出改名及归属迁移**。
- **T-Q：资源与环境变量配置入口收口**。
- **T-R：旧文档/旧材料分层、发布契约和回归总门**。

实施细则、依赖与验收以 [修订方案](plan-dock-2.0.4.md) 为准。本文记录审查证据，不等于相应代码问题已修复。

## 7 官方行为参考

- [MDN ResizeObserver](https://developer.mozilla.org/en-US/docs/Web/API/ResizeObserver)：本轮成功获取全文。回调造成尺寸循环时，通知可能延后到下次绘制；此证据不支持承诺 Windows 原生窗口与 WebView 同帧。
- [GitHub Docs：创建 issue](https://docs.github.com/en/issues/tracking-your-work-with-issues/creating-an-issue)：本轮成功获取全文。title/body/template 与 issue form 自定义字段的参数需要分别处理，过长 URL 可能返回 414；浏览器预填不等于已经提交。
- jsDelivr 官方文档本轮抓取超时；未核实当前国内可达性或全部覆盖范围。方案仅把它作为 raw 候选，明确要求后续实测，不引用其为“免费国内镜像已可用”的证据。
