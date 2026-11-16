# FISH Dock 2.3.4

2.3.4 是承接 2.3.x 的一次集中收尾版本：补齐 #87 之后的宿主 dialogs 能力与文档、落地 #88 外部原生窗口 Hook 的全项修复、落实 #89 任务视图图标与上游 3.2 的一系列稳定性修复，并按产品决策实现顶栏 + dock、嵌入任务栏、任意显示器、日历 / 正在播放、HTML 邮件净化、外部审批 `POST /ask` 与 CI。**主岛的外观、布局与动画没有任何改动。**

只发布 Setup 完整安装包，与 2.2.1 起一致。

> 本发行说明只写本轮**实际交付**的代码与测试，以及明确标注的**未测边界**。整页 TypeScript 化（DEC-8）本轮**撤销**，页面保持 JS，转 2.4.0 跟踪，不计入本版交付。

## 修复

- **覆盖窗 TTL 未按代次销毁会误杀新窗（#88 H9 覆盖窗，审查 F1）。** 快速连续两次拖放时，第一次拖放的覆盖窗口延迟销毁定时器可能在新窗创建后触发，把新窗一起销毁。现在覆盖窗使用单调递增代次 `GOO_GENERATION`，`play_absorb` 领取新代次，TTL 定时器只在「代次仍是当前」时才销毁；`goo_done` / `cancel_goo` 推进代次，使在途的旧 TTL 失效。新增测试覆盖「连续两次拖放旧 TTL 不误杀新窗」。
- **邮件 HTML 净化的 `<script>/<style>` 内容漏成正文（审查 F2）。** 净化对 `script/style/iframe/object/embed/svg/math` 改为**跳过整段内容**到匹配闭合标签（`skip_until_close`，带 `depth` 与闭合标签边界校验，含嵌套 `svg`），CSS 与脚本源码不再作为文本漏出，正常正文与容器文本不受影响。新增测试覆盖内容不漏、容器文本保留、嵌套只跳到匹配闭合标签。
- **旧插件清单里的退役字段会让整张注册表读不出来。** 2.1.x 清单带的 `islandSub` / `islandPanel` 在删除子岛后成了未知字段，`deny_unknown_fields` 让整张表反序列化失败。现在这两个字段被容忍（读进来丢掉），注册表改为**逐条读**，一条坏不影响其他条。

## 新增

### 宿主 dialogs 能力（#87 收尾）

- 插件清单新增 `dialogs` 能力；`host_call` 按 method 三分派，`host.dock` 归 `Window`。
- 新增 `plugin_dialog.rs`：`chooseFolder`（互斥、owner 归属、复用 `paper::pick`、平台门控、i18n）。
- 工作线程写回；SDK 新增 `context.dialogs.chooseFolder`（`chooseFolder` 无超时），页面侧不提供该入口。

### #88 外部原生窗口 Hook 全项

建窗线程化（修 `0x8007139F`，`browser_args` / `data_directory`）、透明窗剥原生标题栏 / 边框、`dock-bottom` 取底边中点、拖拽松手落点 1.5s 快照兜底、贴顶恒报 compact 尺寸、Z 序遮挡跳过 `WS_EX_TRANSPARENT` 全屏宿主窗、`windowEvent` 三断点贯通、`update rect` 支持 `duration` / `easing` 补间、可靠「点窗口外」事件、主岛 pointermove / click / 状态事件、单窗多形态 `keepAlive`（球↔条不闪断、订阅不失效）、实时几何流 + capsule 实时 + stowed 跟随、浮窗参与主岛布局（占位预留 / 命中面）。

### #89 与上游 3.2

- 移植 `pri.rs` + `taskbar_task_view_icons`：Win11 双色任务视图图标，Win10 回退字形；页面侧用字符串而非字节数组，明暗方向修正。
- 上游 3.2 稳定性：抢任务栏去抖 8/10s → 24/30s + `WM_DISPLAYCHANGE` 30s 重置；卡住 10s 先还任务栏（HangBack + `taskbar-hang.json` + `/debug/hang`）；`answer_rect` 第二次答宽高（微信 / QQ 越界）；首点岛开设置（`pressOn` kept）；Esc 关权限询问、重开设置不残留；加载期抓栈重试。

### 产品决策实现（DEC-2 / 3 / 4 / 5 / 6 / 7 / 9）

- **顶栏 + dock（DEC-2）**：新增「顶栏」第三种家模式，保留任务栏与灵动岛，三者可切换、各有独立尺寸 / 避让 / 收起语义；与既有 island 几何、拖放命中面、插件窗口锚点兼容。
- **嵌入 Windows 任务栏（DEC-3）**：`SetParent` 嵌入任务栏子窗口，含开关、失败回落独立窗口、任务栏重启后重挂的决策核心（平台层与开关已落地）。
- **任意显示器（DEC-4）**：统一 home / display 抽象，「家」可选任意显示器，复用既有 `WM_DISPLAYCHANGE` 去抖 / 宽限逻辑。
- **日历 / 正在播放（DEC-5）**：宿主内建，正在播放只读系统媒体会话、无媒体时隐藏；日历不联网、不写外部账户。
- **HTML 邮件净化（DEC-6，安全默认）**：去脚本 / iframe / form / object / 内联事件 / 危险 URL scheme；远程图片默认阻止、可一次性加载；链接点击显示真实目标域名；解析失败回落纯文本。
- **外部审批 `POST /ask`（DEC-7，安全默认）**：仅 loopback 监听、复用 Bearer 与 Host / Origin 校验、默认关闭需显式开启、每请求原生确认、超时自动拒绝、审计日志。
- **GitHub Actions CI（DEC-9）**：Windows runner 上跑 Rust / Node / 文档 / README 版本一致性检查，失败阻断合并；不存放发布凭据。

## 兼容性

- 新增能力与字段全部可选，缺省行为与 2.2.3 一致；已有接口只增不删。
- 插件清单退役字段改为容忍读入，不再导致注册表整体失效。
- 安装包仍为 Setup-only；更新器自 2.2.0 起只认 Setup 载荷及其摘要。

## 测试

- Rust 全量：**1025 passed / 1 failed / 24 ignored**。唯一失败为 `lhm_setup::tests::a_folder_made_with_her_dacl_is_closed_to_the_user`（`lhm_setup::tests::this_machine_is_looked_at` 同属该组），是沙箱 DACL / 任务计划服务限制造成的环境项，与基线一致，不是产品缺陷。
- Node 逻辑子集（`app/test/*.test.js` + `sdk/node/*.test.mjs`）：**491 passed / 0 failed**。
- 2.3.4 新增 / 修改的测试覆盖：覆盖窗代次与连续拖放、净化内容跳过（含嵌套）、注册表退役字段容忍与条目级容错、dialogs 能力与参数、#88 各边界、#89 任务视图图标、上游 3.2 各项。

## 未测边界（如实记录，不代表通过）

以下为真机 / 沙箱外项目，本构建轮**未取得运行证据**，需要真机或不受沙箱约束的会话复核：

- 真实鼠标连续拖放（覆盖窗 F1、#88 P5 / B2）、多屏与 DPI 换算、任意显示器热插拔。
- Win11 任务视图双色图标与 Win10 回退的目视对比；嵌入任务栏在 Explorer 崩溃 / 任务栏重启后的重挂；顶栏 / 任务栏 / 岛三种模式切换的实机外观与避让。
- seven 条原生模态框（弹框 / 取消 / 中文路径 / 缺能力 / 并发 / 退出 / 等待 30s）的逐条真机验证。
- 真实任务计划下的登录任务归属清理；默认安装目录 `%LOCALAPPDATA%\Programs\FISH Dock` 的真实安装（本沙箱对 `icacls /grant` 返回拒绝，安装时只读授权会失败并按设计停下提示，真机目录归本人所有时应成功——此点未经实测确认）。
- 本版未做 Authenticode 签名，SmartScreen 仍可能提示。

## 下载

| 安装包 | 适用场景 |
| --- | --- |
| `FISH-Dock-Setup-2.3.4.exe` | 首次安装、修复安装、旧版覆盖升级，含 Node.js |

2.2.1 起只提供 Setup，不再提供 Update 精简包。
