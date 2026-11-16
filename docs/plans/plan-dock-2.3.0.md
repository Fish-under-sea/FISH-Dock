# FISH Dock 2.3.0：本地端口安全修复与插件选择文件夹（发布准备）

> 已被 `plan-dock-2.3.4.md` 取代，仅作历史记录。

> **已被 `docs/plans/plan-dock-2.3.4.md` 替代（2.3.4）**：本文件保留为历史设计输入。其「非目标」一节对 #88 其余项、#89 与上游产品级项的延期/排除**在 2.3.4 一律作废**；以 2.3.4 计划与 `.work/2.3.4/requirements.json` 为准。
>
> 日期：2026-11-15。基线：2.2.3（`0626a19`）。状态：**计划待评审，尚未改代码**。
> 本版三件事：安全修复（网页不能再调本地端口）、插件宿主能力（`dialogs.chooseFolder`）、把两者需要的文档与 issue 台账补齐。
> 关联文档：[安全 issue 整理](issue-2026-11-15-local-port-web-pages.md)、[上游 0.4.x 评估](plan-20261115-upstream-043.md)、[dialogs 规格](../specs/2026-11-15-host-dialogs-design.md)。

## 一、目标与非目标

### 目标

1. **安全**：`server.rs` 在 `handle()` 之前拒绝网页请求（`Origin` / `Host`），`/v1` 保持自己的校验。
2. **修一个既有路由缺陷**：后端 `host.dock.*` 现在落进存储分支，回 `HOST_OPERATION_FAILED`（#88 的 P3）；顺手抽成可单测的分派。
3. **新宿主能力**：清单能力 `dialogs` + 后端 `host.dialogs.chooseFolder`，供插件（首要是 magpie 网页模式的「选文件夹」）弹出原生目录选择框。
4. **文档与 issue**：安全 issue、dialogs issue、#88 / #89 的分流，以及 2.3.0 发行说明与指南更新。

### 非目标

- #88 的其余 5 处宿主补丁（建窗线程化、剥标题栏、`dock-bottom` 锚点、松手落点快照、贴顶胶囊矩形）与 Hook 层 10 项缺口：不在本版，另开排期。
- #89（任务视图图标）：UI 项，与本版无关，留给 2.4.0。
- 上游 0.4.x 的顶栏 + dock、嵌入任务栏、HTML 邮件、外部审批、TypeScript 重写：只做评估，不移植（见上游评估文档）。
- 页面侧对话框、文件选择、多选、插件窗口作为 owner：`dialogs` 第一版不做。
- 安装器结构、运行时版本、更新协议：不改。

## 二、issue 台账（先把版本位建好，再开条目）

先建 Milestone `2.3.0`（与 `tauri.conf.json` / `Cargo.toml` 同口径），再开下面两条新 issue。仓库目前没有 `security` 类型标签；建议同时补一个，并在 `CONTRIBUTING.md` 的类型表里加一行（可选，不阻塞发布）。

| 条目 | 类型 | 标题 / 内容 | 处置 |
| --- | --- | --- | --- |
| 新 issue A | `bug`（建议加 `security`） | `[安全] 本地端口接受网页请求：可退出、伪造审批、注入会话（Origin / Host 未校验，2.2.3 已复现）`；正文用 [安全 issue 整理](issue-2026-11-15-local-port-web-pages.md) | 本版修（T1）；Milestone `2.3.0`；Assignee `Fish-under-sea` |
| 新 issue B | `enhancement` | `[宿主能力] 插件选择文件夹：dialogs.chooseFolder`；正文引用插件仓交接文档与 [dialogs 规格](../specs/2026-11-15-host-dialogs-design.md) | 本版做（T3）；Milestone `2.3.0`；与 #88 的 P3 相关 |
| #88 | `bug` / `enhancement` | 子灵动岛所需宿主修复 + Hook 缺口 | 只做 P3（`host.dock.*` 路由，T2）；其余 5 处补丁与 Hook 缺口留在原 issue，不关、不改 Milestone |
| #89 | `ui` | Windows 11 任务视图按钮字形 | 不动；留给 2.4.0，等它自己排期 |

开 issue 时按 `.github/CONTRIBUTING.md` 补侧边栏六项（Assignees、Labels、Projects = 无、Milestone、Relationships、Development = 未开工）。**不要**在 issue 正文里贴可直接运行攻击页；接口清单与复现结论足够。

## 三、任务拆分

### T1 本地端口拒绝网页请求（安全）

| 项 | 内容 |
| --- | --- |
| 改动 | [`server.rs`](../../app/src-tauri/src/server.rs)：`header` / `is_local_host` / `is_page_origin` / `turned_away`；`serve()` 循环在 `handle()` 之前拒绝；`mod tests` 加原始 TCP 用例 |
| 依据 | [安全 issue 整理](issue-2026-11-15-local-port-web-pages.md) 第六、九节；上游 `e143563e` |
| 验收 | 被拒的 Origin / Host 逐字匹配；放行的五类请求 200；`/v1` 不归 `turned_away` 管、实际由 `agent_hub` 判；被拒 POST 正文未读时同连接下一请求仍 200；`run-repro.ps1` 的 C1/C2 403、C4/C5 无伪造审批、C6 不退出 |
| 命令 | `cargo test --locked --no-default-features --manifest-path app/src-tauri/Cargo.toml web_pages -- --test-threads=1`（测试名沿用上游的 `a_web_pages_requests_are_turned_away`） |

### T2 `host.dock.*` 路由修复

| 项 | 内容 |
| --- | --- |
| 改动 | [`plugin_process.rs:136`](../../app/src-tauri/src/plugin_process.rs#L136)：把 `window_call` 的字符串判断换成 `host_call(method) -> HostCall`（`Window` / `Dialog` / `Storage`）；`host.dock.*` 归 `Window`，`host.dialogs.*` 归 `Dialog` |
| 依据 | #88 的 P3；dialogs 规格 §4.1 |
| 验收 | 单测钉住三类前缀；真实插件后端调 `host.dock.rect` 能拿到快照（不再是 `HOST_OPERATION_FAILED`） |
| 风险 | 低。以前拿到错误的插件从此拿到真实结果；没有插件依赖「错误」本身 |

### T3 `dialogs.chooseFolder` 宿主能力

| 项 | 内容 |
| --- | --- |
| 改动 | 清单允许列表 [`plugin_manifest.rs:110`](../../app/src-tauri/src/plugin_manifest.rs#L110) 加 `dialogs`；新增 `app/src-tauri/src/plugin_dialog.rs`；[`plugin_window.rs`](../../app/src-tauri/src/plugin_window.rs) 暴露 `installed` / `require_capability`（或由 `plugin_dialog` 自己走安装表）；[`i18n.rs`](../../app/src-tauri/src/i18n.rs) 加 `dialog.chooseFolder`；[`sdk/node/plugin.mjs`](../../sdk/node/plugin.mjs) 加 `context.dialogs.chooseFolder` 与「无超时」选项 |
| 依据 | [dialogs 规格](../specs/2026-11-15-host-dialogs-design.md)；插件仓交接 P1 |
| 验收 | 单测覆盖能力、参数、互斥、延迟响应；手工覆盖取消、中文路径、owner、未声明能力、两个插件、退出时无残留（规格 §6.2） |
| 风险 | 中。对话框是原生模态，真机验收不可替代；超时与回调线程若不按规格做，会在用户手上 5 秒失败 |

### T4 文档与来源清单

| 文件 | 改什么 |
| --- | --- |
| [`docs/guides/DEVELOPMENT.md`](../guides/DEVELOPMENT.md) | 「HTTP 接口」一节补：除 `/v1` 外，网页来源的请求一律 403；哪些客户端不带 Origin；日志格式 |
| [`docs/specs/widgets.md`](../specs/widgets.md) | `/widget` 一节注明网页来源 403，脚本与 Thunderbird 扩展照常 |
| [`docs/guides/plugin-windows-2.2.0.md`](../guides/plugin-windows-2.2.0.md) | 能力表加 `dialogs`；Node SDK 表加 `chooseFolder`；补示例、超时与「页面侧不提供」 |
| [`UPSTREAM.json`](../../UPSTREAM.json) | `version` → 2.3.0；`selectedUpstreamThrough` → `e143563e…`；`selectedChanges` / `excludedChanges` / `notes` 按上游评估 §5 更新 |
| `docs/releases/release-2.3.0.md`（草案已随 5fc81eb 删除，不再保留） | 发行说明（原文件已并入库根发布流程，改由 `docs/releases/release-2.3.4.md` 承接） |

### T5 版本位与 README

| 位置 | 值 |
| --- | --- |
| [`app/src-tauri/Cargo.toml`](../../app/src-tauri/Cargo.toml) | `2.3.0` |
| [`app/src-tauri/Cargo.lock`](../../app/src-tauri/Cargo.lock) | 包 `fish-dock` 的 `version` 同步（第 909 行附近） |
| [`app/src-tauri/tauri.conf.json`](../../app/src-tauri/tauri.conf.json) | `2.3.0` |
| [`integrations/claude-code/.claude-plugin/plugin.json`](../../integrations/claude-code/.claude-plugin/plugin.json) | `2.3.0`（`updates.rs::one_version_everywhere` 钉住 `Cargo.toml`、`tauri.conf.json`、`plugin.json` 三处；`Cargo.lock` 的包版本由 cargo 同步） |
| [`README.md`](../../README.md) / [`README.en.md`](../../README.en.md) | 版本卡、Release 链接、包名、更新记录链接；头图与状态卡用 `tools/docs/render-readme.mjs` 重渲，**不手改 PNG** |

### T6 验证

按顺序：

1. `cargo test --locked --no-default-features --manifest-path app/src-tauri/Cargo.toml -- --test-threads=1`
2. `node --test --test-isolation=none 'app/test/*.test.js' 'app/test/*.test.mjs' 'app/scripts/*.test.mjs' 'sdk/node/*.test.mjs'`
3. `node tools/docs/check-readme.mjs`（若改了 README）
4. 编译 2.3.0 release exe，对隔离数据根重跑 `run-repro.ps1`，核对 C1–C6
5. 手工验 `dialogs.chooseFolder` 的七条（规格 §6.2）
6. 回归：Claude Code / Codex hook 各发一次、Thunderbird 扩展推一次 `/widget`、`/v1` 的鉴权用例

### T7 出包

从仓库根：

```powershell
# 资源目录（含由 writeManifest 生成的 manifest.json）
node app/scripts/prepare-installer.mjs <参数按 installer-build.md>
pwsh app/scripts/build-installer.ps1 -Resources <资源目录>
```

- 清单只能由 `prepare-installer.mjs` 的 `writeManifest` 生成，不手写。
- 出包后 `7z t` 验证，抽取清单与 NOTICE 核对，校验 `FISH-Dock-Setup-2.3.0.exe.sha256`。
- 只发 Setup；2.2.0 之后不再提供 Update 精简包。

### T8 发布（需单独确认）

推送、打 tag、GitHub Release、Gitee 同步发行都必须由用户明确确认后执行；本地构建与提交不等于授权发布。Gitee 按 `app/scripts/gitee-release.ps1` 的参数化入口，先确认令牌存在；无令牌时脚本只校验不写入。

## 四、验收矩阵

| # | 项 | 手段 | 通过标准 |
| --- | --- | --- | --- |
| V1 | 安全判据 | `server.rs` 单测 | 第九节的用例全绿 |
| V2 | 安全回归 | `run-repro.ps1` | C1/C2 403；C4/C5 无伪造会话；C6 不退出 |
| V3 | `host.dock.*` 路由 | 单测 + 真实插件后端 | 不再回 `HOST_OPERATION_FAILED` |
| V4 | `dialogs` 能力 | 单测 + 手工七条 | 成功 / 取消 / 忙 / 缺能力 / 退出都符合规格 |
| V5 | SDK 不超时 | `sdk/node` 测试（延迟响应） | 6 秒后仍 resolve |
| V6 | 全量测试 | 两条命令 | 与 2.2.3 相比无新增失败；环境性失败单独列出 |
| V7 | 版本位 | `one_version_everywhere` + `Cargo.lock` + README 渲染 | 全部为 2.3.0 |
| V8 | 包 | `7z t`、清单、`sha256` | 全部对上 |

已知环境性失败沿用 2.2.3 的口径（`lhm_setup` 的 DACL 用例等），不是本版引入；报告中如实标注。

## 五、风险与回退

| 风险 | 处置 |
| --- | --- |
| 403 误伤正常客户端 | [安全 issue 整理](issue-2026-11-15-local-port-web-pages.md) 第七节的调用方表逐类回归；日志里有 `turned away` 一行；必要时回退 T1 一个提交 |
| 对话框在用户手上超过 SDK 时限 | 按规格 §4.3 做「工作线程 + 延迟响应 + SDK 无超时」；真机专门等 30 秒再选 |
| `dialogs` 能力被页面滥用 | 页面没有这个方法；只有后端能调；能力缺失两层都拒 |
| `host.dock.*` 从报错变成可用，插件行为变化 | 这是修缺陷；单测 + 真实插件验证；没有插件依赖错误 |
| 安全修复被功能项拖住 | 若 T3 在一周内无法通过真机验收，T1 单独切 2.2.4 先发，T3 留在 2.3.0；分支与文档都按这个顺序准备 |
| 上游/插件仓同时发版造成版本错位 | 插件仓交接文档明确 P1 依赖宿主 2.3.0；宿主发行说明写清能力名，插件侧按能力探测 |

## 六、排期（建议）

| 日期 | 事项 |
| --- | --- |
| 11-15 | 本计划、安全 issue、dialogs 规格评审 |
| 11-16 | T1 + T2，单测与 `run-repro` |
| 11-17 至 11-18 | T3 实现与 SDK；T4 文档 |
| 11-19 | T5 版本位、全量测试、出包 |
| 11-20 | 真机验收、发行说明定稿；确认后发布 |

## 七、开放问题

1. 是否补 `security` 标签与 `SECURITY.md`？本版先开 issue；流程文件可随后补。
2. `dialogs` 是否要给页面侧同名能力？本版不做，等真实需求。
3. #88 其余 5 处补丁是否作为 2.4.0 的主线？需要单独的排期与真机验收计划。
4. 插件仓的 magpie P1 何时接入？宿主 2.3.0 发布后由插件仓发版说明引用；两边版本不同步时，插件应探测能力是否存在再启用按钮。
