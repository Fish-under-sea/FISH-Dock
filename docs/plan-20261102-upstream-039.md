# 上游 v0.3.9 合入 FISH Dock 的可行性与排期（2026-11-02 / 目标 2.0.3）

> 结论先行：**v0.3.9 可合入，已在本轮完成（2.0.3）**；其中四项里只有三项需要真的移植，
> 另有一项的代码本仓早已自己实现且更严格，只补说明与测试。
> **#51（agy 工具审批整套）不可合入 2.0.3**：上游那套建在本仓已删除的 `agent-core` 依赖上，
> 属「按 `asks.rs` 重写」而非「移植」，仍归 2.1.0。

## 一、上游这一版是什么

| 项 | 值 |
| --- | --- |
| 仓库 | `zhoupengjie/Wakuwaku` |
| 本仓来源基线 | `27104eab`（v0.3.8），见 `UPSTREAM.json` 的 `selectedUpstreamThrough` |
| 上游最新 | `ab0eb65a`（v0.3.9，2026-10-31 19:35 UTC）+ tag `v0.3.9` |
| 提交区间 | `27104eab..ab0eb65a` |
| 提交数 | 11（8 个非合并 + 3 个合并） |
| 改动量 | 16 个文件，`+211 / −67` |

功能面（上游 release note 的口径）：

| 上游 issue | 提交 | 内容 |
| --- | --- | --- |
| #97 | `3efcc47d` | ZCode、DeepSeek Harness 的审批/计划也能用 `Alt+Y` / `Alt+N` 回答 |
| #98 | `c019eb70` + `7e8af4a7` | 带标题栏的窗口「盖住整块显示器且没最大化」或「恰好铺满」也算全屏（Kazumi 这类播放器） |
| #98 | `ba4c6cab` | 任务栏三段宽度变化时，胶囊跟着自己的槽位走 |
| #98 | `efbcfb52` | 同一 tick 内的多次「任务栏被重新显示」只算一次（不再误判成抢任务栏） |
| #99 | `513f9979` | 宠物图库与下载照「网络代理」走 |
| #100 | `9ca1c269` | 找会话控制台窗口时，标准句柄被一起关掉导致的退出崩溃 |
| — | `ab0eb65a` | 版本位与文档 |

另有两个纯版本位文件（`Cargo.toml` / `Cargo.lock` / `tauri.conf.json` /
`integrations/claude-code/.claude-plugin/plugin.json`），以及 `docs/GUIDE*.md`、`docs/DEVELOPMENT.md` 的说明。

## 二、逐项判定

判定口径：**能不能合**看三件事——本仓有没有这块代码、上游的改动是否依赖本仓没有的东西、验收能不能在本仓闭环。

### #97 审批快捷键 —— 合入（1 行逻辑 + 1 条测试）

| | |
| --- | --- |
| 上游做法 | `panel.js` 的 `canAnswerWithHotkey` 去掉 `!IN_APP.has(ask.from)`；`IN_APP` 只留给「去 XX 处理」那颗按钮的措辞 |
| 本仓现状 | `app/src/pet/panel.js:31` 与上游 v0.3.8 逐字相同，旧行为被 `app/test/shortcuts.test.js:51-53` 钉着 |
| 判定 | **可合入**。改动 1 行，但要同时改那条钉旧行为的测试，否则测试会红 |
| 风险 | 低。`canAnswer === false` 与 `question` 两条边界不变；已有测试覆盖 |

### #98 全屏判据 `fills` —— 合入（函数 + 判据 + 七条断言）

| | |
| --- | --- |
| 上游做法 | 新增 `fills(style, zoomed, rect, monitor)`：无标题栏看「盖住」；有标题栏看「盖住且没最大化」或「恰好等于显示器」；最大化永远不算 |
| 本仓现状 | `fullscreen.rs` 只有「带标题栏一律不算全屏」的早退（原 L154-156）；仓库另有本仓自己的两处扩展：`SKIP_PROGRAMS` 六项（截图工具豁免，上游只有三项）、`resolved()` + `recovery_tests`（工作区自愈，`#43`） |
| 判定 | **可合入**。判据本身是纯函数，与上面两处扩展不冲突，替换 `covering()` 里的早退与 `covers` 计算即可 |
| 风险 | 中。语义变宽了：**任何**带标题栏、未最大化、盖住整块显示器的窗口，以前不算全屏、现在算。任务栏会让开。需要真机复看两类场景：截图工具（`SKIP_PROGRAMS` 已挡）、以及自绘标题栏的应用 |

### #98 胶囊跟随槽位 —— 合入（10 行）

| | |
| --- | --- |
| 上游做法 | 对 `zones` 三段挂 `ResizeObserver`，条件满足时重放 `placeSlot()` |
| 本仓现状 | `island.js` 已有 `placeSlot()`（L2083，含侧边分派 `placeSideSlot`）、`zones`（L1737）、`taskbarBuilt`（L1711）、`isOn()`（L393）、`home()`（L395）；`solidWatch` 已经是同类用法（L4663） |
| 判定 | **可合入**。守卫按本仓的名字写（`isOn()` / `home()`），比上游多覆盖侧边条一条路（本仓 `placeSlot` 自己分派） |
| 风险 | 低。只重算坐标；`placeSlot` 内部已有 `seatSent` 去重，不会把 seat 事件刷爆 |

### #98 抢任务栏去抖 —— **不移植**（本仓早已有等价实现）

本仓 `taskbar.rs:2401` 的 `Tug::shown` 已有同一个守卫：

```rust
if self.seen > 0 && self.at[(self.seen + TUG_TIMES - 1) % TUG_TIMES] == tick {
    return false;
}
```

上游 v0.3.9 的写法是 `self.at[(self.seen - 1) % TUG_TIMES] == tick`——同一个 tick 只算一次，等价。
两者的来源都是 2.0.1 的工作树基线 `6376a35`，本仓先做。移植只会把已有注释换一批措辞，收益为零。

### #99 宠物图库走代理 —— **代码不移植**，补说明与测试

| | |
| --- | --- |
| 上游做法 | `fetch.rs` 原本自建 `ureq::Agent`，改为逐个 `proxy::web_ways(url)` 试、第一个应答算数 |
| 本仓现状 | `fetch.rs:139` 的 `get()` 早就走 `proxy::web_ways`，而且更严格：按 `proxy::pet_timeout()` 的预算、`http_status_as_error(false)`、`max_redirects(0)`、站点地址走 `config::pet_site()`、错误分 `network` / `http` / `notFound`、`gallery()` 另有离线读缓存兜底 |
| 判定 | **代码不移植**（移植等于降级）。但本仓**缺两样**：① 设置页「网络代理」的中英文案没把「宠物图库」列进去；② 没有「图库真的问代理」的断言 |
| 本轮做法 | 文案（`s.net.systemNow` / `s.net.manualNote`）、`proxy.rs` 头注释、`docs/DEVELOPMENT.md` 补全；新增 `fetch.rs` 的 `the_gallery_goes_the_way_the_network_proxy_says`，钉住代理收到的是 `CONNECT codex-pets.net:443 HTTP/1.1` |

### #100 控制台句柄还原 —— 合入（24 行）

| | |
| --- | --- |
| 上游做法 | `console_window` 在 `AttachConsole` 前记下 `GetStdHandle` 的三个，`FreeConsole` 后 `SetStdHandle` 放回 |
| 本仓现状 | `jump.rs:511` 与上游 v0.3.8 相同，没有记也不会放回 |
| 判定 | **可合入**。本仓同一函数后半段还有「终端归属窗口」的取法（`GetAncestor` + `GA_ROOTOWNER`），改动只在前半段，互不影响 |
| 风险 | 低。`Handle` 在本仓是 `isize`，无空指针判断负担；`Attach` 失败时提前返回，不改任何状态 |
| 验收 | 纯靠代码审读 + 上游的真机记录（发行版没有标准句柄）。本仓无自动化复现手段，**记为验收缺口** |

## 三、#51（agy 工具审批整套）为什么不能进 2.0.3

`#51` 列的上游四提交 `237197d3 + 44664d4c + e0bf5ceb + 5a0fee56`（约 `+848/−221`）确实**不在**
`27104eab..ab0eb65a` 区间里，它们是 v0.3.7/v0.3.8 的东西。本轮同步不会顺带把它们带进来。

更关键的是它在本仓**没有落脚点**，实测证据：

| 证据 | 上游 `27104eab` | 本仓 `2.0.2` |
| --- | --- | --- |
| `Cargo.toml` 的依赖 | `agent-core = { git = "…/agent-hub", tag = "v0.2.7" }` | **无任何 agent 依赖**（#47 已删） |
| `events_agy.rs` | 1115 行 | 317 行 |
| `needs_approval` / `classify_tool` / `for_ask` / `to_output` / `with_seen` | 全有 | 全无 |
| `globalPermissionGrants` / `enableTerminalSandbox` / `AGY_PROMPT_TIMEOUT` | 有 | 0 处 |
| `connection` 结构 | 目录 11 个模块（含 `agy.rs`） | 单文件 4268 行 |
| 审批路由 | `agent_core::claude::to_event(event, Agent::Agy)`、`agent_core::Context` | 无此 API；本仓自有一套 `asks.rs`（953 行） |

上游 `main.rs` 的 `agy_hook` 新分支、`server.rs` 的 `read_event` / `event_for` / `take` 三处都要调
`agent_core`，而本仓把这个 crate 整个删了——所以这不是「照着提交 cherry-pick」，而是
**照本仓 `asks.rs` + `events.rs` 的行为模型把审批重写一遍**。

工作量估计（供 2.1.0 立项）：

| 块 | 内容 | 估计 |
| --- | --- | --- |
| `events_agy.rs` | `needs_approval` 的过滤规则（内部工具、工作区/产物路径、Turbo / Eager、`globalPermissionGrants`）、`classify_tool` 映射、`for_ask` 的 Claude Code 措辞、`to_output` 的 `auto_approve` / `deny` / `allow`、`with_seen` | 500–700 行 |
| `connection.rs` | 装 `PreToolUse`（`matcher: ""`，`timeout` 300 s），并翻转现有那条「故意不装 `PreToolUse`」的钉住测试（`connection.rs:3317-3332`） | ~20 行 |
| `main.rs` | `agy_hook` 的同步分支（`needs_approval` 为假走快路 `{"decision":"allow"}`） | ~40 行 |
| `server.rs` | `PermissionRequest` 的路由、`with_seen`、agy 的等待上限压到 60 s | ~20 行 |
| 界面 | `panel.js` / `agents.js` / `i18n.js`（`FROM`、审批能力徽标、中英文案） | ~30 行 |
| 固件 | 三档规则（allow / deny / ask）+「只在一处定义」的两种情形回放 | 6–10 条 |

**卡点不是工作量，是验收**：审批的错法恰恰是「该拦的没拦」，而本机没有可驱动的 agy 会话做端到端
验证（`#51` 正文已写明这一点）。所以本轮维持 `#51` 的两条状态：仍 OPEN，仍归 2.1.0。

## 四、`#50`（设置页会话行展开）

上游 `8f834c54` 已在 `27104eab` **之前**（`git merge-base --is-ancestor` 证实是 v0.3.8 的祖先），
不属于本轮区间，本轮与它无关。`#50` 维持 2.1.0。

## 五、本轮落地清单（2.0.3）

1. `app/src/pet/panel.js` —— `canAnswerWithHotkey` 去掉 `IN_APP` 排除，注释说明理由。
2. `app/test/shortcuts.test.js` —— 钉新行为 + `canAnswer === false` 反例。
3. `app/src-tauri/src/fullscreen.rs` —— 头注释、`IsZoomed` 声明、`fills()`、`covering()` 改用它、
   `what_fills_a_display` 七条断言。
4. `app/src/pet/island.js` —— `slotWatch`（`ResizeObserver` + 守卫）。
5. `app/src-tauri/src/jump.rs` —— `GetStdHandle` / `SetStdHandle` 声明、`STD_HANDLES`、`console_window` 寄回。
6. `app/src-tauri/src/fetch.rs` —— `the_gallery_goes_the_way_the_network_proxy_says`。
7. 文案与文档 —— `i18n.js` 两条、`proxy.rs` 头注释、`docs/DEVELOPMENT.md` 三处。
8. 版本位 —— `Cargo.toml` / `Cargo.lock` / `tauri.conf.json` / `integrations/claude-code/.claude-plugin/plugin.json`
   （第四处 2.0.2 漏了，见下）→ `2.0.3`。
9. `UPSTREAM.json` —— `version` → `2.0.3`、`selectedUpstreamThrough` → `ab0eb65a`、`selectedChanges` 补四项、
   **删掉一条一直写错的**（`5a0fee56` 被列为「已选」，但本仓没有可附着的审批代码）。
10. `docs/release-2.0.3.md`。

## 五之二、顺带查出的两条既有缺陷（非上游带来）

| 缺陷 | 证据 | 处置 |
| --- | --- | --- |
| `plugin.json` 版本位停在 `2.0.1` | `updates.rs::one_version_everywhere` 断言 `Cargo.toml` / `tauri.conf.json` / `plugin.json` 三处同版本；2.0.2 的 `cc66aaf` 只改了前三处里的两处 + `Cargo.lock`，`plugin.json` 没动（`git log` 显示它最后一次改动是 `6376a35`）。也就是说 **2.0.2 的这套测试本来就是红的** | 本版补到 `2.0.3`，测试转绿 |
| `UPSTREAM.json` 把 `5a0fee56` 列为「已选」 | 本仓 `needs_approval` / `classify_tool` / `globalPermissionGrants` 均 0 处；`events_agy.rs` 317 行，上游 1115 行 | 移入 `excludedChanges`，并在 notes 里写明它随 #51 一起挂起 |

## 六、验收

| # | 项 | 手段 | 状态 |
| --- | --- | --- | --- |
| V1 | JS 侧（快捷键、i18n、岛） | `node --test --test-isolation=none 'app/test/*.test.js'` | 见提交记录 |
| V2 | Rust 侧（`fills`、标准句柄、图库代理、版本位） | `cargo test --locked --no-default-features -- --test-threads=1 --skip t30` | 714 → 716 通过（含本轮新增 2 条） |
| V3 | 版本位一致 | `grep` 三处都是 `2.0.3` | ✅ |
| V4 | 真机：Kazumi 全屏时任务栏让开 | 测试虚拟机手工 | ⏳ 出包后 |
| V5 | 真机：任务栏在顶部，胶囊不再压住托盘 | 测试虚拟机手工，150% 缩放 | ⏳ 出包后 |
| V6 | 真机：任务栏模式退出不崩 | 连点会话跳转后退出，看 `crashes/` | ⏳ 出包后 |

**V2 的环境前提**（先前既有缺口，非本版引入）：`plugin_t30_acceptance.rs:192` 用
`include_bytes!` 引了**插件仓**的 `plugins/catalog/stable.json`，干净克隆里没有这个文件，
`cargo test` 直接编不过；等号右边那条路径还要求 `FISH_DOCK_T30_ROOT` 指向 `t30-apphome`，
否则那批测试 panic。本轮验证用 `--skip t30` + 在仓库外临时造该文件完成，未提交。

另有两条测试在本机跑不过，与本轮改动无关（都不碰 `lhm_setup.rs`），属环境依赖：

| 测试 | 失败原因 |
| --- | --- |
| `lhm_setup::tests::a_folder_made_with_her_dacl_is_closed_to_the_user` | 造带 DACL 的目录时 `Os { code: 5, PermissionDenied }`——需要能设置 ACL 的权限 |
| `lhm_setup::tests::this_machine_is_looked_at` | `the Task Scheduler's root folder: 0x80070003`——本机没有可用的任务计划程序根目录 |

## 七、遗留

- `#51` 维持 2.1.0（见第三节）；若要做，先补「可驱动的 agy 会话」再动代码。
- `#50` 维持 2.1.0，与本轮无关。
- README 顶部版本徽标图（`assets/readme/stat-version-*.png`）按 2.0.2 的做法单独一次重渲染提交。
- `cargo test` 干净克隆编不过，建议单开一条 issue：把 T30 的 `include_bytes!` 改成 `option_env!` /
  构建期注入，或在仓内放一份最小合成 catalog。