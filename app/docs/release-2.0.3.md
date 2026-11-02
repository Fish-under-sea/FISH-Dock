# FISH Dock 2.0.3

补丁版。宿主为 **2.0.3**，兼容 **插件 2.1.x**；不是「宿主与插件同版本」，安装时请按各自的版本号对待。

这一版是**上游同步 + 插件启动链路首批修复**，两半：

- **上游同步**：把 Wakuwaku 从 `27104eab`（v0.3.8）推进到 `ab0eb65a`（v0.3.9），按「本仓是否已有实现」
  逐项判定后纳入四项修复，另修正两处与版本位有关的既有缺陷（来源清单里一条自 2.0.1 起就写错的选择项、
  `plugin.json` 漏掉的版本位）。上游该版共 11 个提交（8 个非合并 + 3 个合并）、16 个文件、`+211/−67`；
  其中两项本仓早在 2.0.1 之前就自己做过且更严格，不重复移植。
- **插件启动链路**（任务书 [`plan-dock-2.0.3.md`](plan-dock-2.0.3.md)，诊断
  [`plugin-startup-diagnosis-2026-11-02.md`](plugin-startup-diagnosis-2026-11-02.md)）：首批 T01 与 T05。
  对应「启用自启动后首次启动插件拉起异常」与「设置页连开十次起十个 `node.exe`」——
  第一条不是「慢」，是**误杀**（见下）。

## 探针超时不再杀进程（T01，诊断 A1）

- `plugin_process.rs` 拆出 `alive()` 与 `exited()`：`running()` 仍然表示「存活 + 握手可应答」，
  但**不再被当成死亡证明**。`plugin_runtime.rs` 的 `view()` 因此只在 `exited()` 真的给出退出码时
  才 `remove` 实例、记一条失败。
- 修的是：以前 `!running()` 就直接 `self.processes.remove(id)`，而 `Process` 的 `Drop` 会
  `child.kill()`——插件进程还活着、只是那 2 s 探针没应答，就被判死并强杀，连它拉起的
  `magpie.exe` / `mihomo.exe` 一起带走。`plugin-failures.log` 里那批 `exitCode:null` 的记录
  就是这么来的。冷启时 I/O 与杀软最重、探针最容易超时，所以它只在「开机自启的首次启动」出现，
  之后「恢复服务」又正常。
- 这一轮没应答时新增 `unresponsive` 字段，插件卡显示「未响应，正在重试」——它不再冒充「启动失败」，
  也不覆盖运行态，下一轮重绘自己会好。

## Node 探测放宽并缓存（T05，诊断 A5）

- `plugin_dependencies::probe` 的死线 2 s → 8 s（冷启那次是被 Defender 与磁盘占走的，不是 Node 慢）。
- 成功的探测按 `(路径, 长度, 修改时刻)` 缓存在进程生命周期里：设置页每次重绘都要走一遍
  `discover()`，以前每轮现起一次 `node --version`，连开十次就是十个进程；现在同一个二进制只探一次，
  换了版本或原地升级会因长度/时间变化自动重探。失败不缓存（用户装好 Node 后下一次重绘就该认出来）。

## 审批快捷键（上游 #97，`3efcc47d`）

- ZCode、DeepSeek Harness 的审批与计划，`Alt+Y` / `Alt+N` 现在能直接回答，和按钮一样。
- 之前 `canAnswerWithHotkey` 把这两个应用（`IN_APP`）排除在外——`IN_APP` 本来只管「去 XX 处理」
  那颗按钮的措辞；结果是按钮能答、快捷键只开岛，按钮上也没有组合键提示。
- `canAnswer === false`（明确答不了）仍然只开岛；问题（`question`）仍然不替人答。
- 回归：`app/test/shortcuts.test.js` 原来钉的是旧行为（`zcode` 只开岛），改为钉新行为，
  并补一条 `canAnswer === false` 的反例。

## 全屏判据（上游 #98，`c019eb70` + `7e8af4a7`）

- `fullscreen.rs` 新增 `fills(style, zoomed, rect, monitor)`：**没有**标题栏的，盖住整块显示器就算全屏；
  样式上**有**标题栏的，「盖住整块且没有最大化」或「窗口矩形恰好等于显示器」才算；最大化的永远不算。
- 修的是 Kazumi 这类播放器：它全屏用的是 `window_manager 0.5.1` 的 `setFullScreen`，保留
  `WS_CAPTION`、靠 `WM_NCCALCSIZE` 不画边框、把窗口设成显示器的 `rcMonitor`。以前「带标题栏一律不算
  全屏」，她的任务栏就压在视频上，底部、顶部都是。
- 新增 `what_fills_a_display`：七种真实窗口形态（Kazumi 全屏、全屏前已最大化、任务栏顶部时被挪到
  每边 +11 像素、差一像素、两种最大化、HELLDIVERS 2 的每边 +1 像素、副屏）各一条断言。
- 本仓原有的截图工具豁免（`SKIP_PROGRAMS` 六项：SnippingTool、ScreenClippingHost、ScreenSketch 等）
  与工作区自愈 `resolved()` 一并保留，未被上游版本覆盖。

## 灵动岛跟随槽位（上游 #98，`ba4c6cab`）

- 任务栏左、中、右任一段的宽度变了（托盘图标几秒后才陆续到、之后新增一个、监控的数字变宽），
  她的胶囊跟着重新对齐。
- 之前胶囊按「画任务栏那一刻」的位置算 `--isl-x`，之后槽位被挤走它却留在原处，压在监控上、
  严重时压住托盘的箭头，要展开一次胶囊才归位。
- 做法：对三段 `zones` 挂 `ResizeObserver`（`island.js` 的 `slotWatch`），只在
  「任务栏已建 + 在岛里 + 家在任务栏」时重放 `placeSlot()`；横向与侧边两条路都走这一个入口。

## 退出时崩溃（上游 #100，`9ca1c269`）

- `jump.rs` 的 `console_window`：找命令行会话的窗口时要临时 `AttachConsole` 到那个会话的控制台，
  断开（`FreeConsole`）会把**她自己的**标准句柄一起关掉。这些句柄号之后被她打开的文件重用，
  任务栏模式下退出时 `tray-icon` 往「错误输出」写一句，写不进去（os error 5），`eprintln!` 当它是
  panic，进程就这么结束了（上游 v0.3.8 的 0xCFFFFFFF 一类退出）。
- 现在连之前先 `GetStdHandle` 记下三个（`STD_HANDLES`：-10 / -11 / -12），断开之后
  `SetStdHandle` 原样放回去。发行版本来没有标准句柄，放回的就是空，行为与从前一致。

## 宠物图库走代理的说明（上游 #99，`513f9979`）

- **代码不移植**：本仓 `fetch.rs` 自 2.0.1 前（`6376a35`）就自己实现了这条路，而且更严格——
  走 `proxy::web_ways`、按 `pet_timeout` 的预算、`http_status_as_error(false)`、站点地址走
  `config::pet_site()`、错误分 `network` / `http` / `notFound` 三档、另有离线读缓存兜底。
- **补的是说明**：设置页「网络代理」的中英文案（`s.net.systemNow` / `s.net.manualNote`）原来列了
  软件更新、崩溃报告、插件联网、LibreHardwareMonitor，唯独漏了宠物图库；`proxy.rs` 与
  `docs/DEVELOPMENT.md` 同样补上。
- **补的是测试**：上游那条「图库真的照网络代理走」的断言本仓没有，补为
  `fetch.rs` 的 `the_gallery_goes_the_way_the_network_proxy_says`（起一个本地代理，钉住它收到的是
  `CONNECT codex-pets.net:443 HTTP/1.1`）。属特性回归（characterization），不是本轮修复的证明。

## 不移植的两项（本仓已有）

- **任务栏「抢任务栏」去抖**（上游 #98，`efbcfb52`）：本仓 `taskbar.rs` 的 `Tug` 早就有同一个守卫
  （`self.seen > 0 && self.at[(self.seen + TUG_TIMES - 1) % TUG_TIMES] == tick` → 同一个 tick 只算一次），
  自 `6376a35` 起如此。上游的写法与它等价，改了只是换个写法。
- **图库走代理的实现**：见上一节。

## 来源清单与版本位修正

- `UPSTREAM.json` 的 `version` → `2.0.3`，`selectedUpstreamThrough` → `ab0eb65a`，
  `selectedChanges` 补本轮四项。
- **删掉一条一直写错的**：`"agy approvals read globalPermissionGrants from config.json (upstream #91, 5a0fee56)"`
  自 2.0.1 起就挂在「已选」里，但本仓没有 `needs_approval` / `classify_tool` / `globalPermissionGrants`
  任何一处代码（`events_agy.rs` 317 行，上游同一文件 1115 行），它没有可附着的审批子系统。
  现移入 `excludedChanges` 并在 notes 里写明原因，对应 #51。
- **`integrations/claude-code/.claude-plugin/plugin.json` 的版本位停在 2.0.1**：2.0.2 只改了
  `Cargo.toml` / `Cargo.lock` / `tauri.conf.json` 三处，漏了它，于是 `updates.rs` 的
  `one_version_everywhere` 在 2.0.2 上一直是红的（它断言四处说的是同一个版本）。本版补到 `2.0.3`。

## 已知限制

- **任务书 T01–T10 只落了首批**：T01（探针不杀进程）与 T05（Node 探测缓存）已合入；
  T02（「插件 → 宿主」回调解耦）、T03（校验缓存 + 重活移出 `CURRENT`）、T04（冷/热分档超时）、
  T06（校验判据改文件指纹）、T07（全量校验移出启动关键路径）、T08（WebView2 ACL 按需）、
  T09（插件页面入口放锁）**本版未做**。其中 T02 是 L 级结构改动，按任务书 §6 建议排 2.1.0。
  也就是说：**「更新后首启特别慢（1.30 GB 全量校验）」这一条本版没有修**，见 [#56](https://github.com/Fish-under-sea/FISH-Dock/issues/56)。
- **agy 工具审批仍未纳入**（#51）：上游那一整套建在 `agent-core` crate 上
  （`agent_core::claude::to_event`、`agent_core::Context`），而本仓在 #47 已删掉这个依赖，
  所以它不是「移植」而是「照 `asks.rs` 重写」。仍归 2.1.0。
- **`cargo test` 在干净克隆上编不过**（先前既有问题，非本版引入）：`plugin_t30_acceptance.rs:192`
  用 `include_bytes!("../../../../plugins/catalog/stable.json")` 引了**插件仓**的文件，本仓没有；
  且 T30 那批测试要求 `FISH_DOCK_T30_ROOT` 指向 `t30-apphome`，否则 panic。
  本版验证用 `--skip t30` 并临时造了该文件，未提交。
- **README 顶部的版本徽标图**（`assets/readme/stat-version-*.png`）由渲染器读取版本位生成，
  本版按 2.0.2 的做法另起一次重渲染提交，不与代码同批。
- 真机验收缺口：`fills` 的三种新形态只在测试虚拟机里核对过上游的取数，本仓侧为纯函数单测；
  Kazumi / 任务栏顶部退回灵动岛这两条需要真机复看。

## 安装

- `FISH-Dock-Setup-2.0.3.exe`：首次安装、资源缺失或修复安装。
- `FISH-Dock-Update-2.0.3.exe`：已有版本且基础资源完整时使用，只含本次变更。
- 两个包都附同名 `.sha256`，另有 `SHA256SUMS.txt` 汇总；安装包当前未做 Authenticode 签名。

### 出包前置：精简更新包的基线（#54）

2.0.2 的教训仍在：`FISH-Dock-Update-2.0.2.exe` 的 payload 是**相对上一版清单的差集**，
而 `resources/licenses/NOTICE.txt`（412 B → 484 B）与 `runtime-sources.json`（1360 B → 1009 B）
在 2.0.1 就变过、2.0.2 视为没变不打包，于是 2.0.0 用户的应用内更新**必失败**
（`install-update.cjs:231` 抛「精简更新包所需基础资源缺失或损坏」）。

2.0.3 出包时如果沿用同一套差集口径，2.0.0 用户会踩同一个坑。至少要做两件事：

1. 确认 payload 覆盖所有受支持起点（2.0.0 / 2.0.1 / 2.0.2 三档），或**给 2.0.0 只发完整包**；
2. 应用侧判断「本地版本低于精简包基线」或「精简包安装失败」时，自动改下完整包并直接给下载入口。

`install-manifest.json` 加 `baseline_version`、跨版本回归测试等长期项按 [#54](https://github.com/Fish-under-sea/FISH-Dock/issues/54) 原文走 2.1.0。