# FISH Dock 2.0.3

补丁版。宿主为 **2.0.3**，兼容 **插件 2.1.x**；不是「宿主与插件同版本」，安装时请按各自的版本号对待。

这一版是**上游同步 + 插件启动链路修复**，两半：

- **上游同步**：把 Wakuwaku 从 `27104eab`（v0.3.8）推进到 `ab0eb65a`（v0.3.9），按「本仓是否已有实现」
  逐项判定后纳入四项修复，另修正两处与版本位有关的既有缺陷（来源清单里一条自 2.0.1 起就写错的选择项、
  `plugin.json` 漏掉的版本位）。上游该版共 11 个提交（8 个非合并 + 3 个合并）、16 个文件、`+211/−67`；
  其中两项本仓早在 2.0.1 之前就自己做过且更严格，不重复移植。
- **插件启动链路**（任务书 [`plan-dock-2.0.3.md`](../plans/plan-dock-2.0.3.md)，诊断
  [`plugin-startup-diagnosis-2026-11-02.md`](../guides/plugin-startup-diagnosis-2026-11-02.md)）：任务书十项里落了九项
  ——T01、T03、T04、T05、T06、T07、T08、T09 与维护线程的互斥收口（t19）；T02 留 2.1.0（见「已知限制」）。
  起点是两条用户可见的病症：「启用自启动后首次启动插件拉起异常」与「设置页连开十次起十个 `node.exe`」——
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
  `docs/guides/DEVELOPMENT.md` 同样补上。
- **补的是测试**：上游那条「图库真的照网络代理走」的断言本仓没有，补为
  `fetch.rs` 的 `the_gallery_goes_the_way_the_network_proxy_says`（起一个本地代理，钉住它收到的是
  `CONNECT codex-pets.net:443 HTTP/1.1`）。属特性回归（characterization），不是本轮修复的证明。

## 不移植的两项（本仓已有）

- **任务栏「抢任务栏」去抖**（上游 #98，`efbcfb52`）：本仓 `taskbar.rs` 的 `Tug` 早就有同一个守卫
  （`self.seen > 0 && self.at[(self.seen + TUG_TIMES - 1) % TUG_TIMES] == tick` → 同一个 tick 只算一次），
  自 `6376a35` 起如此。上游的写法与它等价，改了只是换个写法。
- **图库走代理的实现**：见上一节。

## 资源校验改文件指纹（T06，诊断 B1）

- `state/verified` 从「两行：清单摘要 + 资源目录」改成一份 JSON：`{schema, resources, files: {path: {bytes, mtime}}}`。
- 启动判据 `pending()` 逐 manifest 文件比 size+mtime，只对指纹变了的文件重算 SHA-256（`verify_pending_at`）；
  `read_manifest` 不再对 `manifest.json` 算摘要，因此**换一份清单不再等价于全量重校**。
- 旧两行格式解析失败即按「没有记录」处理（等价于升一次级自愈），不写迁移脚本；
  用例 `the_old_two_line_marker_counts_as_missing`。
- 标记写入改成原子替换：同目录临时文件 `.verified-<pid>-<n>` → `write_all + sync_all` → `storage::replace`，
  任何一步失败只删临时文件、旧记录原封不动，并印一行「旧记录保留（下次启动再校验）」。
- 用例：`the_start_checks_the_required_files_and_remembers_a_fingerprint_per_file`、
  `one_changed_file_is_verified_again_and_the_unchanged_ones_are_not`、
  `a_new_manifest_alone_does_not_ask_for_the_tree_again`、
  `the_marker_records_the_stamps_the_judgement_saw_not_a_later_stat`、
  `a_failed_write_of_the_marker_keeps_the_old_record`。

## 全量校验移出首帧（T07）

- 同步半 `prepare_at` 只做清单校验与必需文件 `symlink_metadata`；逐文件哈希交给后台线程。
  后台校验失败与从前一样 `show_failure + exit(1)`，只是晚几秒而不是挡住首帧。
- 用例 `the_start_up_path_hashes_nothing_and_a_damaged_file_is_named_in_the_background`、
  `the_background_half_is_told_where_and_says_so_when_it_is_not`。

## WebView2 授权按需（T08）

- 只在校验通过、且判据认为缺权限时做一次，不再每次启动递归授权：判据是目录与 `msedgewebview2.exe` 两处 DACL 的启发式
  （含 `S-1-15-2-1` 在本机被写作别名 `AC` 的情况），不逐文件试读。
- 用例 `the_webview_permission_is_written_only_where_the_sandbox_cannot_read`、
  `a_refused_grant_for_a_new_directory_is_not_remembered`。

## 插件校验缓存（T03）

- 新增 `host/plugin-verified/<id>-<version>-<digest>.json`，逐文件记 size+mtime。`verify()` 改成快照优先：
  `plugin.json` 照读 → 逐文件 stat → 全等即通过（一个字节都不进 SHA-256）→ 只有变了的文件才重算并回写快照。
- 清理面：`cleanup()` 增 `prune_snapshots()`（只留当前版本与当前摘要，别的插件不碰），`uninstall()` 收掉该 id 的整族快照。
- `resume()` 不再同步跑 `cleanup()` / `cleanup_staging()`，改由启动 30 s 后的维护线程 `maintain()` 做。
- 用例 `verification_is_cached_by_size_and_mtime_and_a_missing_snapshot_hashes_again`、
  `snapshots_follow_the_active_version_and_vanish_on_uninstall`、`upgrade_leaves_only_the_current_version_and_current_digest`。

## 插件页面入口放锁（T09）

- `open()` 去掉同步 `verify`；`present()` 只查实例表，把目录、清单与指纹表记进 Grant。
  `call()` 与 `gateway.open` 走新的 `call_outside()`：借 `Arc<Mutex<Process>>` 句柄、放锁等回答、回来按代次结账；
  等待期间被停用或卸载的实例，其回答被丢弃，不写进新代次的失败记录。
- `resource()` 不再读注册表（无 EDIT 锁）、不再每资源哈希：指纹命中直接发，变了才重算那一个文件。
- 实例表改成 `Arc<Mutex<Process>> + generations`，`view()` 用 `try_lock`（在途 RPC ⇒ 活着），`event()` 跳过，
  `stop_all()` 立 stopping 闸；T01 的 Liveness 判据与 `liveness()` 原样保留。
- 用例 `resources_are_served_from_the_grant_fingerprint_without_rehashing`、
  `page_grants_bind_identity_version_enabled_state_and_lifetime`、
  `an_in_flight_page_call_is_discarded_once_its_instance_is_retired`。

## 冷/热分档超时（T04）

- 冷态判据 `config::plugin_startup_cold`：登录后 0–300 s（**含端点**）算冷；取不到登录时刻（非 Windows / WTS 查询失败）
  或时钟回拨算热。
- 调用超时热态 5000 ms、冷态 20000 ms，`plugin_host_call_timeout_ms_of` 夹在 1–30000 ms；同一组数经 `host.hello`
  的 `hostCallTimeoutMs` 下发，插件侧缺字段回落 5000（SDK 注释同口径）。
- 冷态 `Process::start` 失败重试一次（500 ms 退避），热态不重试；错误文案带「（冷启已重试一次，首次：…）」。
- 用例 `cold_window_is_five_minutes_with_none_and_rollback_as_warm`、
  `a_cold_start_retries_once_and_a_warm_start_does_not`、
  `a_cold_failure_retries_exactly_once_and_a_warm_one_does_not`、`a_failed_start_removes_the_new_version_directory`（gated）。

## 维护线程与安装的互斥收口（t19）

- 模块级上层串行锁 `MAINTENANCE`：`install()` / `uninstall()` / `maintain()` / `set_autostart()` /
  `set_enabled_starting()` 五个入口先过它，锁序 `CURRENT → MAINTENANCE → EDIT_REGISTRY → EDIT`。
- 修的是「维护线程把正在写入的新版本目录当过期版本删掉」：改动前 cleanup 与 install 同在 `CURRENT` 下天然互斥，
  挪到线程后这条互斥丢了。
- 为不造自死锁（`std::Mutex` 不可重入），两个开关入口各拆出 `set_autostart_locked` / `set_enabled_starting_locked`
  无锁体，命名即契约：锁内体绝不回头调用会再次取锁的公开入口。
- 结算段（`spawn_starts`）锁内只剩 `install_instance(&id, generation, process)`——签名里没有 `Manager`，
  想验哈希或写注册表在类型上写不出来。
- 用例 `maintenance_waits_for_the_install_lock_before_it_prunes_anything`、
  `the_toggle_entries_wait_for_the_maintenance_lock`、
  `a_pending_journal_is_recovered_under_the_lock_and_the_new_version_survives_maintenance`。

## 插件页卡面重做（t11）

每张卡按「一台在跑的服务」重排：左身份（名字）→ 中信号（状态灯 + 运行态 + 版本迁移）→ 右操作（一颗实心）。

| 位置 | 改前 | 改后 |
| --- | --- | --- |
| 状态 | 3 个并列标签（版本 / 运行中 / 有更新） | 1 条 `● 运行中 · v2.1.2 → v2.1.3`；同号重发写「v2.0.0 → 同号新版」，不能更新时不画箭头 |
| 卡内按钮 | 5 颗同高（更新 / 关闭自启动 / 停用 / 卸载 / 打开插件日志） | 4 颗（更新实心 + 关闭自启动描边 + 停用描边 + 更多），卸载与日志收进「更多」 |
| 实心按钮 | 红色的「关闭自启动」（另有绿色「去 Star」） | 卡内唯一实心是「更新」；红色只留卸载的 hover/focus |
| 元数据 | 2 行正文级（路径、安装来源各一行） | 1 行 12px 次级（路径与来源同排） |
| 内容行 | 带功能页的卡 4 行 ~145px、纯服务卡 3 行 ~92px | 3 行 ~117px（−19%）、2 行 ~77px（−16%） |
| 功能页入口 | 与按钮同款的小胶囊 | 32px 高、带 `›` 的整行入口 |
| 视图切换 | 3 颗按钮靠 `aria-pressed` 假装切换 | 真 `tablist`：`aria-selected` + roving tabindex + ←/→/Home/End，与 tabpanel 关联 |
| 检索 | 与八个按钮同权重的「检索」 | 输入框内的字段按钮，回车即搜；输入框补 `aria-label` |

高度与行数是按 CSS 盒模型与渲染出的 HTML 结构算的，**没有截图核对**，见「已知限制」第 10 条。

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

下列 12 条全部如实保留，其中第 1、9、10 条决定了这一版能承诺到什么程度。

1. **T02「插件 → 宿主」回调解耦仍留 2.1.0**：它是 L 级结构改动，需要 t29 / t30 / G1 全组重跑，本版没动。
2. **`plugin_api.rs` 的 install 提交段仍持 `CURRENT`**：T03 只把 `Runner` 侧的重活挪出锁，API 提交段这一小段仍在全局锁里（t13-F2、t17-F5）。
3. **冷态重试在「用户点启用 / 装完即启」路径仍占 `CURRENT`**：这两条走的是同步 `Process::start`，冷窗最坏约 40.5 s（20 s 首试 + 0.5 s 退避 + 20 s 重试），期间插件页的其他操作要等。
4. **`is_running` 对「已死但实例锁被占」的实例会短暂答 `true`**：取的是 T01 同向取舍——`try_lock` 抢不到即视为在途 RPC，错在少杀而不误杀，下一轮重绘自愈。
5. **`sandbox_may_read` 只判 ACE 类型、不判权限位**：只认 DACL 里有没有对应的 ACE 条目，不去核实它给的是 `RX` 还是别的位。
6. **后台校验失败走 `show_failure + exit(1)` 硬退**：T07 把全量校验挪出首帧，但失败仍是硬退（与 2.0.2「校验通过才启动」同语义），只是晚几秒。
7. **标记写失败被当成致命**：`state/verified` 写不进去时会弹「安装资源损坏，请重装」并 `exit(1)`，与同一处「旧记录保留、下次再校验」的日志口径矛盾。
8. **崩溃残留的 `.verified-*` 临时文件没有启动期清理**：原子写留在 `home/state/` 下的临时文件只在写失败时删，进程被强杀后不会有人收。
9. **真机未验**：H6（更新后首启首帧 ≤ 3 s、`state/verified` 10 s 内更新为指纹）、H7、G2-S7-S8、断电注入全部没有真机证据；本机只有代码位置、单测与本机安装树 0.75 ms 的 stat 实测。
10. **插件页未做视觉截图核对**：本环境没有渲染能力——没有截图、没有真人 400–600 ms 计时、没有 480px 视口实拍；第「插件页卡面重做」一节的行数与高度是按 CSS 盒模型与渲染出的 HTML 结构算的。**不许用 `check-contrast` 的 334 条或检测器的 87 条冒充截图验收。**
11. **`plugin_page.rs:64` 的 `dock.autostart` 直接调 `Manager::set_autostart`，不经 `MAINTENANCE`**：这条页桥路径与维护线程仍有同类竞态（同行的 `set_autostart` 内部会 `recover_unlocked()`）。本轮只把 `Runner` 侧的五个入口（install / uninstall / maintain / set_autostart / set_enabled_starting）收进锁内，页桥这条留 2.1.0。
12. **`set_enabled` / `set_autostart` 现在持 `MAINTENANCE` 走 `Process::start`**：冷态最长到 activate 预算（约 20 s），后台维护线程会多等一次。install / uninstall 本来就要 `CURRENT`，没有新增等待。

其它仍未收口、但不属本轮范围的：

- **agy 工具审批仍未纳入**（#51）：上游那一整套建在 `agent-core` crate 上
  （`agent_core::claude::to_event`、`agent_core::Context`），而本仓在 #47 已删掉这个依赖，
  所以它不是「移植」而是「照 `asks.rs` 重写」。仍归 2.1.0。
- **`cargo test` 在干净克隆上编不过**（先前既有问题，非本版引入）：`plugin_t30_acceptance.rs:192`
  用 `include_bytes!("../../../../plugins/catalog/stable.json")` 引了**插件仓**的文件，本仓没有；
  且 T30 那批测试要求 `FISH_DOCK_T30_ROOT` 指向 `t30-apphome`，否则 panic。
  本版验证用 `--skip t30` 并临时造了该文件，未提交。
- **真机验收缺口**：`fills` 的三种新形态只在测试虚拟机里核对过上游的取数，本仓侧为纯函数单测；
  Kazumi / 任务栏顶部退回灵动岛这两条需要真机复看。
- README 与 `assets/readme` 的版本徽标图本版**在同一轮重渲染**（不是另起一次提交），
  版本号只从 `app/src-tauri/tauri.conf.json` 读，文档与图上数字不存在第二份来源。

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