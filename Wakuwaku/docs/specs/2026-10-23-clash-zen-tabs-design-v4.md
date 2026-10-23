# Clash 与 ZenDesktop 设置页 v4 设计（含公共钉住交互、token/批准分账、本地模型登录开浏览器）

状态：草案，等待审阅。本文是自足全文：v1（`2026-10-22-clash-zen-tabs-design.md`）未被推翻的部分原样收编并按当前代码重标行号，v2、v3 记录在 issue #3 的变更表里，v4 的增量见下表。写作时基线为上游 1.0.2（cd7b742）。

## v4 相对 v3 的变更

| # | 变更 | 原因 |
| --- | --- | --- |
| 0 | **新增 3.7.1 系统代理开关**：dock 自己实现系统代理的接管与还原（三元组快照 + 孤儿补还原），新增 `clash_sysproxy` 命令 | zashboard 与 mihomo 内核都写不了 Windows 系统代理——没有这个开关，dock 的内核拿不到任何系统流量，「实现 Clash 主要功能」不成立。v1 的「用面板里的同名开关」表述有误（zashboard 没有该能力） |
| 1 | **端口策略改为「固定优先 + 占用回退 + 落盘可查」**：默认 mixed 7899 / controller 9099，被占用才回退随机空闲端口；生效端口写 `<data>/clash/ports.json` 并进 `/health` 的 `clash` 段 | 随机端口让本机 agent 与外部工具无法稳定引用代理出口（AGENTS.md 这类回退规则写不了固定值） |
| 2 | **新增第 3.10 节：agent 与外部工具的端口发现契约** | 第 1 条的配套：动态回退要求消费者学会查端口，而不是记端口 |
| 3 | **内核生命周期定为「启动后随 dock 常驻」**：首次进入 Clash 页时 `clash_ensure`，之后一直运行，dock 退出时随 `clash_stop` 停止；设置页提供启动/停止按钮 | 端口发现契约的前提是「文件在 = 内核在」；仅打开标签页才活的话，agent 查到的是死端口 |
| 4 | `socks-port` 从生成配置中移除 | mixed 端口同时服务 HTTP 与 SOCKS，少一个要分配和记录的端口 |
| 5 | 行号按 1.0.2（cd7b742）全部重标；`MODELS_W/H` 1440、`syncModelFocus` 3001、`PAGES` 2815、`generate_handler` 1669 | v1 锚点已整体位移 |
| 6 | 数据目录路径统一为 `%USERPROFILE%\.fish-dock` | 1.0.2 已统一身份目录（`identity.rs:45`），v1 写的 `.dsh-fish` 是旧值 |

## v3 相对 v2、v2 相对 v1（摘自 issue #3，全文以 issue 为准）

- **v2**：公共交互整章以 `model_focus.rs`（159 行）为基线重写；新增 token 与批准分账工作项；Clash 面板定为内核 `external-ui` 同源托管（弃 `tiny_http` 挂 `/clash/`）；行号重标。
- **v3**：新增「本地模型页账号登录真实打开网页、打开即自动取消置顶」一章；新增命令 `settings_step_aside` / `oflm_open_url`；落地顺序、进度勾选、风险各补若干条。

## 1. 目标与范围

设置面板新增 **Clash** 与 **ZenDesktop** 两个标签页；修正 **token 与批准次数的归属**（每个工具只显示自己的数，没有数据源就显示「—」）；修好 **本地模型页的账号登录**（真实打开系统浏览器，打开即自动取消置顶）。不涉及 #1 的 UAC／数据目录迁移，不涉及 #2 的五项。

## 2. 探查结论与证据边界

| 项目 | 已确认（本机或上游源码实测） | 尚待真实环境验证 |
| --- | --- | --- |
| 页签机制 | `settings.js:47` 的 `TABS` 驱动；`:178` 用 `TABS.length` 算指示条宽；`settings.css` 的 `.s-tabs .ind` 是 `calc((100% - 6px) / var(--tabs, 4))` | 8 个页签在最小宽度下是否折行 |
| 锁定 + 置顶生命周期 | Rust 侧 `model_focus.rs`（159 行）已是与页签无关的状态机：全屏询问一轮一次、关置顶后连续全屏 2 秒自动解锁、迟到回答隔离；JS 侧 `settings.js:3001 syncModelFocus` 目前只认 `models`，由 `open`/`close`/`setTab` 三处调用 | 三个钉住页连续切换时原生置顶的实际抖动 |
| Clash Verge 控制器 | **不可借**：HTTP 控制器默认关闭（`verge.yaml:83`），`127.0.0.1:9097` 不监听；命名管道 ACL 只给提权进程（Win32 err 5） | 无（本设计不依赖此路径） |
| 内核可执行文件 | 单文件 Go 程序；`mihomo -v`、`-d`、`-f`、`-ext-ctl`、`-ext-ui`、`-secret`、`-t` 均可用；最新 release v1.19.33，windows-amd64 压缩包约 12–24 MB | 旧版本对 `-ext-ui` 的支持（备选见 3.6） |
| zashboard | `Zephyruso/zashboard`，MIT，官方产物 `dist-no-fonts.zip` 约 1.4 MB；同源时自动连接同源控制器；URL 参数 `hostname` / `port` / `secret` / `disableUpgradeCore` / `disableTunMode` | WebView2 中 PWA 与 Service Worker 的实际行为 |
| Windhawk | 本机未安装；探测顺序与注册表结构见第 4 章；引擎 `RegNotifyChangeKeyValue` 监听注册表，写 `Disabled` 即生效 | 真实 mod 表、启用未编译 mod 的表现 |
| 首次编译 | Windhawk v1.7.3 无 CLI，无法脚本化编译 | 未来版本若并入 CLI 可省掉跳转 |
| 许可证 | mihomo **GPL-3.0**（⚠️ 仓库 API 的 license 字段是 MIT——那是默认分支的无关项目，内核在 Meta 分支）；zashboard **MIT**；Windhawk、ZenDesktop **GPL-3.0**（9 个 mod 中 `windows-animations` 为 MIT）；dock 本体 **MIT** | 分发形态定稿后人工复核声明文本 |
| 身份目录 | 1.0.2 起 `%USERPROFILE%\.fish-dock`（`identity.rs:45`，`FISH_DOCK_DATA` 覆盖，旧 `DSH_FISH_DATA` 已弃用） | 无 |

## 3. Clash 页设计

### 3.1 为什么必须是 dock 自管内核

三条现成路径都走不通：Clash Verge 的 HTTP 控制器默认关闭；命名管道 ACL 提权才可开；起提权副本代价不成比例。所以 dock 自管一套内核与数据目录，与用户已装的 Clash Verge 并行、互不干涉；代价是运行期内本机存在两个代理内核，由 3.7 的端口与 TUN 策略、3.8 的并存提示约束。

### 3.2 内核发现

按顺序取第一个存在的可执行文件，只做路径判定不执行：

1. 托管副本 `<data>/clash/core/mihomo.exe`（3.4 下载所得）。
2. 环境变量 `DSH_FISH_CLASH_CORE` 指定的路径。
3. 注册表 `HKLM\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\Clash Verge`（含 WOW6432Node）的 `InstallLocation`，回退 `%ProgramFiles(x86)%\OTHER APP\Clash Verge\`。
4. PATH 中的 `mihomo.exe`、`verge-mihomo.exe`。
5. `%LOCALAPPDATA%\Programs\*` 下的同名文件。

全部缺失时页面显示「未找到 mihomo 内核」，提供选择本地文件与从官方 release 下载两个动作，不静默失败。版本探测 `mihomo -v` 只读执行一次，失败不阻塞，标注版本未知。

### 3.3 配置与数据目录

```
%USERPROFILE%\.fish-dock\clash\
  config.yaml          生成的配置（每次启动重写）
  ports.json           生效端口（3.7，v4 新增）
  core\mihomo.exe      托管内核（若走 3.4）
  ui\                  zashboard 静态文件
  geoip.dat geosite.dat Country.mmdb ASN.mmdb
  logs\
```

生成配置（v4：去掉 `socks-port`，mixed 同端口已覆盖 HTTP 与 SOCKS）：

```yaml
mixed-port: {MIXED}
external-controller: 127.0.0.1:{CTL}
secret: {SECRET}
external-ui: ui
mode: rule
log-level: warning
unified-delay: true
tcp-concurrent: true
dns:
  enable: true
  ipv6: false
  enhanced-mode: fake-ip
  fake-ip-range: 198.18.0.1/16
  nameserver: [223.5.5.5, 119.29.29.29]
  fallback: [1.1.1.1, 8.8.8.8]
```

订阅与节点由用户在面板里自行导入，最终写入 `config.yaml` 的 `proxies` / `proxy-groups` / `rules` 段。dock 不解析、不翻译、不缓存订阅内容。geo 数据优先从已装 Clash Verge 的数据目录复制（只读字节，不读 profile 与凭据），缺失时按需下载。

### 3.4 内核获取

仅在发现全部失败且用户主动点击时发起：从 `MetaCubeX/mihomo` 的 `Meta` 分支 release 取 `mihomo-windows-amd64-*.zip`（arm64 取对应资产），解压到 `<data>/clash/core/mihomo.exe`，来源 URL 与版本记入 `SOURCE.json`。失败保留临时文件并给可读原因，重试不超过两次。

### 3.5 面板托管：同源，零 CORS

zashboard 解到 `<data>/clash/ui/`，配置 `external-ui: ui`，内核在 `http://127.0.0.1:{CTL}/ui/` 同时提供面板与 API——**同源**。iframe 地址 `http://127.0.0.1:{CTL}/ui/?secret={SECRET}&disableUpgradeCore=1`（`disableUpgradeCore` 必要：面板的「升级内核」会绕过 dock 的版本记录与许可证声明）。不用 `tiny_http` 挂静态路由：跨端口要维护 CORS 白名单，且 `server.rs` 的路由表极小，加静态服务要处理路径穿越、MIME 与缓存，得不偿失。备用路径（某内核版本忽略 `external-ui`）才启用 `GET /clash/<path>`，届时同步把 dock 源写进 `external-controller-cors.allow-origins`，并做路径规范化、扩展名白名单、无目录索引。

### 3.6 内核版本过旧的备选

`-ext-ui` 不被支持时按 3.5 的备用路径走；面板功能降级为只读观察，并在页面标注内核版本过旧。

### 3.7 端口与 TUN 策略（v4 重写）

**固定优先、占用回退、落盘可查**：

1. 首选固定端口：mixed **7899**、controller **9099**（刻意避开 Clash Verge 的 7897/7898/9097 与 mihomo 官方默认 7890）。
2. `std::net::TcpListener::bind(("127.0.0.1", port))` 探测：被占用时对每个端口回退为 `bind(("127.0.0.1", 0))` 取随机空闲，取到即释放再交给内核（竞态窗口重试一次）。
3. **生效端口写 `<data>/clash/ports.json`**：

   ```json
   { "mixed": 7899, "controller": 9099, "panel": "/ui/", "pid": 12345, "startedAt": 1792712345678 }
   ```

   字段固定为这五个；**不写 secret**（控制器凭据只存在于 `config.yaml`）。`clash_ensure` 成功时写入，`clash_stop` 成功时删除——**文件存在即内核在跑**，这是对外契约。
4. `/health` 增加 `clash` 段：`"clash": { "running": true, "mixed": 7899, "controller": 9099, "version": "v1.19.33" }`，未运行时为 `{ "running": false }`。`/health` 与 `ports.json` 并存，前者权威，后者是无需 HTTP 的文件级契约。
5. TUN 默认不开（不写 `tun` 段），开启入口只在面板里。**注意：TUN 需要内核提权运行，dock 的内核按普通权限跑，本设计的 TUN 仅指在面板里对已提权内核的开关，dock 不代提权。**
6. 并存提示：检测到 `clash_verge_service` 或 `verge-mihomo` 进程时，页顶提示「Clash Verge 正在运行，两个内核同时接管流量会互相覆盖」。仅提示，不阻止，不停对方的服务。

### 3.7.1 系统代理开关（v4 增补：dock 必须自己实现）

**动机**：mihomo 内核只负责在端口上监听，**不会**设置 Windows 系统代理（把系统流量引到端口上是 GUI 的职责，zashboard 是纯内核控制台，同样写不了）；`proxy.rs` 现状也只有读取（`proxy.rs:196 system()`），没有写入口。没有这个开关，dock 的内核只能服务 §3.10 里显式配置了端口的 agent——浏览器等系统流量永远不会经过它，Clash 页就成了摆设。

**设计**：Clash 页动作条上加 dock 自己的「系统代理」开关（不是 zashboard 里的控件）：

1. **开**：写 `HKCU\Software\Microsoft\Windows\CurrentVersion\Internet Settings` 的 `ProxyEnable=1`、`ProxyServer=127.0.0.1:{mixed}`；`ProxyOverride` 保留用户原条目并确保含 `localhost;127.*;<local>`。写完调 `InternetSetOption` 通知系统生效（`winreg` 已是依赖，`InternetSetOption` 走 `windows-sys`）。
2. **快照与还原**：首次接管前把原 `(ProxyEnable, ProxyServer, ProxyOverride)` 三元组快照到 `<data>/clash/sysproxy.json`。关闭开关、`clash_stop`、dock 退出三条路径都按快照还原——快照本身就是「原本没开代理」时也能正确还原成没开。
3. **崩溃安全**：照 `shell.rs` 任务栏还原的模式，接管标记落盘；下次启动检测到「标记在、内核没在跑」的孤儿接管就先还原再走，避免 dock 崩溃后系统代理指着一个死端口。
4. **与 Clash Verge 互斥**：接管前若当前 `ProxyServer` 不是 dock 的（即 Verge 在管系统代理），提示「接管将覆盖 Clash Verge 的系统代理，关闭后恢复接管前的状态」。不轮询、不抢占：接管期间用户在 Verge 里又开了代理，最后写者赢，页顶的并存提示已覆盖该场景。
5. **默认关**：不静默修改用户的系统配置；但开关功能完整，打开即接管——这与「默认不接管」的克制不矛盾，克制的是默认值，不是能力。
6. 命令：`clash_sysproxy(app, enable) -> SysProxyState`（加入 §8 清单）；开关状态与快照是否存在的展示走 `clash_state()`。

### 3.8 进程生命周期

新增 `src-tauri/src/clash.rs`，形状照 `oflm.rs`：

| 命令 | 语义 |
| --- | --- |
| `clash_ensure(app, origin) -> ClashReady \| ClashFailure` | 未运行则发现内核、分配端口、写配置与 `ports.json`、拉起子进程、轮询 `GET /version`（Bearer secret，200ms 间隔上限 20s）就绪；已运行则直接返回现状。返回 `{ url, version, ports, warning? }` |
| `clash_state() -> Value` | 只读：是否在跑、内核路径与版本、端口、错误码 |
| `clash_stop(app)` | 先请求内核退出，超时强杀进程树，删除 `ports.json`，回收端口 |

- 生命周期为**启动后常驻**：首次进入 Clash 页触发 `clash_ensure`，此后与 dock 同生共死（`shutdown()` 挂进 dock 退出路径，同 oflm）；设置页提供「停止内核／启动内核」按钮。**不随 dock 开机自启**。
- 子进程隐藏控制台（`oflm.rs:99 no_console`）；`origin` 白名单沿用 `oflm.rs:111 embed_ancestors`（`http://tauri.localhost` 与 `http://localhost:*` / `127.0.0.1:*`），其余 `ORIGIN_REJECTED`；状态放 `static Mutex`，实现 `shutdown()`。
- 失败码沿用 oflm 风格：`KERNEL_MISSING`、`SPAWN_FAILED`、`READY_TIMEOUT`、`PORT_TAKEN`、`ORIGIN_REJECTED`，前端按码取文案。

### 3.9 前端宿主

新增 `src/pet/clash.js`，宿主模式照 `models.js`：单例层 `#clash-host`，`position: fixed` 停放在 `left: -99999px`（不用 `display: none`——隐藏的 frame 会被节流，切回要重载）；导出 `window.Clash = { enter, park, solids, holds, hovered }`，与 `Models` 同名同形；槽位选择器 `#island-settings .s-clash-slot`；CSS 把 `#models-host` 的规则改成 `#models-host, #clash-host` 共用。首帧建占位 iframe，`invoke('clash_ensure', { origin })` 拿到 `url` 再赋 `src`；失败显示错误文案与「重试」，形同 models 页的 note/retry。

### 3.10 agent 与外部工具的端口发现契约（v4 新增）

**文件契约**：`ports.json` 存在 = 内核在跑；路径固定 `<data>/clash/ports.json`；字段固定 `mixed` / `controller` / `panel` / `pid` / `startedAt`；不含 secret。**HTTP 契约**：`/health` 的 `clash` 段字段同源，`running: false` 时只有这一个键。两者不一致时以 `/health` 为准（文件是写给不便于发 HTTP 的消费方的）。

mixed 端口是标准 HTTP/SOCKS 代理，无需凭据；controller API 带 secret，本机 agent 与外部工具**只该用 mixed，不该碰 controller**。推荐的本机 agent 回退规则（可直接写进 AGENTS.md）：

```
1. 默认直连。
2. 直连失败 → 试 http://127.0.0.1:7897（用户已装的 Clash Verge，若在运行）。
3. 仍失败 → 查 dock 内核端口并走它：
   端口取 ~/.fish-dock/clash/ports.json 的 mixed（文件不存在 = 内核没在跑，跳过本条）；
   然后 export HTTPS_PROXY=http://127.0.0.1:<mixed> HTTP_PROXY=同值 重试一次。
4. 都失败 → 如实报告并停止，不在两条路之间来回重试。
```

该契约的收益：固定优先让常见情形稳定在 7899；回退到随机时消费者靠查文件跟上；`disableUpgradeCore` 与 `SOURCE.json` 保证内核版本变化不影响契约本身。

## 4. ZenDesktop 页设计

（v1 第 4 章全文有效，此处保留要点，全文见 v1。）

- 页面三块：Windhawk 状态卡（已安装/未安装/引擎未运行）、mod 列表（勾选只改本地待提交集合）、动作条（应用 / 一键部署 9 个 mod / 打开 Windhawk）。
- 探测照 `deploy.ps1:84-107`：两个 Uninstall 注册表键 → 两个固定路径 → `<dir>\windhawk.exe` 存在为准；数据根照 `deploy.ps1:133-170`（`AppDataPath` 或 portable）。未安装是默认路径，必须先做。
- mod 四态（已编译·开启 / 已编译·关闭 / 未编译 / 无源文件）决定勾选框可用性；`ModsSource` 文件名与 id 对应照 `deploy.ps1:269`。
- 应用 = 一次提权一个 diff：已提权（`lhm_setup.rs is_elevated`）直接写；否则 JSON 请求文件 + `lhm_setup.rs:1110 elevate` 拉起自身 exe，参数 `--wakuwaku-zen apply <request.json>`，子进程只校验形状、写 `HKLM\SOFTWARE\Windhawk\Engine\Mods\<id>\Disabled`、写结果文件。**不改** Include/Exclude/Architecture/Version/LibraryFileName。引擎自己感知注册表变化，不需要重启服务。
- 一键部署照 `deploy.ps1:266-350`：下载 9 个 `local@*.wh.cpp`（记录 URL 与 SHA-256）→ 复制进 `ModsSource`（同名同内容跳过、不同先备份）→ 按头注释写注册表项（`Disabled=1` 只装不开）→ 建 `ModsWritable\<id>` → 引擎 `Settings\Include` 仅追加 `dwm.exe`，不写 Exclude。对已存在 mod 保留用户当前 `Disabled`，重复执行幂等。
- 首次编译必须跳转 Windhawk（v1.7.3 无 CLI），不伪造编译进度、不把写注册表表述成已生效。
- dock 不内建卸载，只给目录与文档链接。

## 5. 公共交互：以 `model_focus.rs` 为基线

### 5.1 Rust 侧（已实现，不用重做）

`model_focus.rs` 的 `ModelFocus` 已是与页签无关的状态机：`enter` / `set(pin, top)` / `observe(full)`（全屏时询问一轮一次；关置顶后连续全屏 2 秒自动 `flags=(false,false)`，原生时钟中断即作废）/ `answer`（迟到回答按序号隔离）。三个钉住页共用这一个实例，Rust 只在 `settings_focus`、`keep_on_top`（`island.rs:1495`）处消费，无需新增。

### 5.2 JS 侧：集合化

`settings.js:3001` 的 `syncModelFocus` 改集合判断（保留 `modelFocus` 作上次提交值；`snap.modelFullscreenPrompt` 的清理原样保留）：

```js
const PINNED = new Set(['models', 'clash', 'zen'])
function syncPinnedFocus() {
  const active = isOpen && PINNED.has(tab)
  ...
}
```

`open`/`close`/`setTab` 三处调用点改名。随 v4 重标的三处硬编码：

1. **宽高**：`MODELS_W/H` 在 `settings.js:1440-1441`；加 `CLASH_W = 1120`、`CLASH_H = 760`，建 `SIZED = { models: [MODELS_W, MODELS_H], clash: [CLASH_W, CLASH_H] }`；`:2904`、`:2921` 的三元改为查表。Zen 页不嵌外部界面，走普通宽度。
2. **宿主停放**：`:2868`、`:3021` 的 `Models.park()` 旁加 `Clash.park()`。
3. **页签与文案**：`:47` 的 `TABS` 追加 `'clash', 'zen'`；`:48` 的 `TAB_KEY` 加 `clash: 's.tabClash', zen: 's.tabZen'`；`:2815` 的 `PAGES` 加 `clash: pageClash`（返回 `''`）与 `zen: pageZen`。

`island.js` 里 `Models` 的 `layerHover` / `hovered` 钩子同样为 `Clash` 加一份，否则面板 hover 时宿主层不跟随。

## 6. token 与批准分账（v2 新增工作项）

**目标**：每个工具只显示自己的数，没有数据源就显示「—」；不串、不混。

- **批准**：`pet_answer` 在应答前从 asks 取提出方 agent，写入分账（本仓库分支已实现：`widgets.rs` 的 `Today::approve(agent)` 与 `today.json` 的 `agents` 字段；若本设计先于该改动合入上游，则把这两处一并带来）。
- **token**：`tokens.rs` 本就按工具读各自的会话记录（Claude Code、Codex、Antigravity），归属天然清晰；缺的是「按工具单独呈现」——在钉住页头部与 Agent 页每行只渲染该工具自己的今日 token，数据源缺失显示「—」。
- **轮次与耗时**：沿用 `Today` 的 per-agent 分账（同上，随分支合入）。
- **验收**：单装 Claude Code、单装 DSH 两种环境下各自只出现自己的数。

## 7. 本地模型登录开浏览器（v3 第 5 章）

现状：OFM 登录页在置顶面板内触发不了浏览器。方案：

1. 新增 `oflm_open_url(app, url) -> Result<(), String>`：只放行 `http`/`https`，`ShellExecuteW` 打开系统默认浏览器；失败（受限策略）时返回错误，前端显示可复制的完整 URL，不弹窗。
2. 打开登录页时先调 `settings_step_aside`：一次性 `set(Some(false), Some(false))` 取消锁定与置顶——**自动取消，不走全屏询问**；用户回来再进页面时按正常进入流程重新锁定。
3. EAC 分支里的可点链接同走 `oflm_open_url`。

## 8. 命令与参数清单

新增 Tauri 命令（注册进 `main.rs:1669` 的 `generate_handler!`，紧邻 oflm 三件套）：

```
clash_ensure(app, origin)   -> ClashReady | ClashFailure
clash_state()               -> Value
clash_stop(app)             -> ()
clash_sysproxy(app, enable) -> SysProxyState | SysProxyFailure
zen_state()                 -> ZenState
zen_apply(app, changes)     -> ZenApplied | ZenFailure
zen_deploy(app)             -> ZenDeployed | ZenFailure
settings_step_aside(app)    -> ()
oflm_open_url(app, url)     -> Result<(), String>
```

新增命令行分支（`main.rs` 的 `args` 分派，照 `lhm_setup.rs:94 ADMIN_FLAG` 的模式）：

```
--wakuwaku-zen apply  <request.json>
--wakuwaku-zen deploy <request.json>
```

request 与 result 文件放系统临时目录，写完即删。

## 9. 测试与验收

### 9.1 前端

- 扩 `test/settings-focus.test.js`：`TABS` 含三个钉住页时，进 `zen` 发 `focus(true)`、切 `mail` 发 `focus(false)`、三页连续互切只发必要布尔、关闭后迟到快照不重绘；现有四条断言不回归。
- 新增 `test/clash-layer.test.js`（照 `models-layer.test.js`）：`Clash.enter` 不重复建层、`park` 是 off-screen 而非 `display:none`、`clash_ensure` 失败可重试、`holds`/`hovered` 形状与 `Models` 一致。
- `i18n.test.js` 校验中英键成对，新增键两边各一处。

### 9.2 Rust（纯函数级，不需要真实内核与 Windhawk）

- 端口策略：首选端口空闲时返回 7899/9099；被占用时回退且三次分配互不相同；`ports.json` 的写入内容与 `clash_stop` 后的删除。
- 配置生成：secret 只出现一次、`external-ui: ui` 存在、无 `tun` 段、无 `socks-port` 行。
- 内核发现顺序与 `KERNEL_MISSING`；`wh.cpp` 头注释缺省值；请求/结果 JSON 形状校验（拒绝多余字段与越界 id）；`local@x.wh.cpp` → `local@x`。
- `/health` 的 `clash` 段：未运行时只有 `running:false`。

### 9.3 实机验收

| 项 | 判据 |
| --- | --- |
| Clash 页进入 | 打开即锁定+置顶；关闭或切页解除；手动取消不被重绘覆盖 |
| 内核拉起 | 首选 7899/9099 生效且 `ports.json` 落盘；面板可交互（点节点、看延迟、切模式、看连接表）；`clash_stop` 后端口释放、`ports.json` 删除、无残留进程 |
| 端口被占 | 预置占用 7899 的监听后，回退随机端口且 `ports.json`/`/health` 如实反映 |
| agent 回退 | 模拟直连失败，按 3.10 的规则走 mixed 端口可出网；内核停止后规则正确跳过 |
| 系统代理 | 开关打开后浏览器等系统流量走 dock 内核（`ProxyServer=127.0.0.1:{mixed}`、`ProxyOverride` 原条目保留）；关闭或退出 dock 按快照还原；强杀 dock 后下次启动补还原 |
| 并存提示 | Clash Verge 运行时页顶出现提示 |
| 失败分支 | 移除内核后可读报错并重试；面板 URL 变更后旧 iframe 不残留 |
| ZenDesktop | 未安装显示入口不抛错；启停一次 UAC 生效；一键部署幂等且不关用户已开的 mod；未编译提示明确 |
| 页签 | 8 个页签最小宽度不折行、指示条正确 |
| 分账 | 单装 Claude Code、单装 DSH 两种环境下只显示自己的数 |
| 登录 | 点登录弹系统浏览器且置顶自动取消；`Start-Process` 被拒时显示可复制 URL |
| 回归 | `node --test test/*.test.js` 与 `cargo test` 基线不退化 |

### 9.4 许可证交付物

新增仓库根目录 `THIRD_PARTY_NOTICES.md` 并挂进「关于」区（`settings.js:2548 aboutHTML`）的可展开项：zashboard（MIT，保留声明）、mihomo 内核二进制（GPL-3.0，附全文与 `SOURCE.json` 的源码获取方式）、ZenDesktop 9 个 mod 源码（GPL-3.0，`windows-animations` 为 MIT，5 个基于 m417z Windows 11 *Styler 需署名）、Windhawk（GPL-3.0，不分发不链接，仅文档说明）。dock 本体保持 MIT。

## 10. 落地顺序

1. 抽 `PINNED` 集合 + `SIZED` 表 + 两个页签空壳（后续两项的前置，可独立验证）。
2. Clash 页：`clash.rs` 三件套（含端口策略与 `ports.json`）→ `/health` 段 → `clash.js` 宿主 → 配置生成 → 同源托管 → 失败分支。
3. ZenDesktop 页：`zen.rs` 只读探测 → mod 四态 → 提权写入 → 一键部署 → 未编译提示。
4. token/批准分账展示。
5. 登录开浏览器（`settings_step_aside` / `oflm_open_url`）。
6. `THIRD_PARTY_NOTICES.md` + 关于区入口 + 文档。

- [x] 调查上游协议与机制；读 models 页、`models.js`、`oflm.rs`、提权路径
- [x] 端口策略与 agent 发现契约定稿（本文 3.7、3.10）
- [ ] 抽 `PINNED` 与 `SIZED`，落两个空壳页签
- [ ] Clash 页实现与联调（含 agent 回退实测）
- [ ] ZenDesktop 页实现与联调
- [ ] token/批准分账展示
- [ ] 登录开浏览器
- [ ] 第三方声明与文档

## 11. 已知风险

1. **双代理内核并存**：dock 内核与 Clash Verge 争系统代理与 TUN。缓解：默认不开 TUN、不接管系统代理、页顶提示、端口错开；系统代理的接管有快照还原与孤儿补还原（3.7.1），dock 崩溃不会把系统留在死端口上。
2. **固定端口被第三方长期抢占**：7899/9099 被别的软件占用时长期走随机端口，外部引用漂移。缓解：`ports.json` 与 `/health` 契约让消费者跟着走；设置页显示实际端口，用户可自行清理占用者。
3. **`ports.json` 陈旧**：崩溃等异常退出可能留下文件。缓解：`clash_ensure` 每次启动重写；契约约定文件存在 ≠ 一定可连，消费方连接失败按「内核没在跑」处理并回退。
4. **Windhawk mod 与 Windows 累积更新冲突**：系统更新后 mod 失效甚至资源管理器异常。缓解：页面常驻说明；一键部署默认全关；不代用户编译。
5. **首次编译无法脚本化**：v1.7.3 无 CLI。缓解：显式 UI 流程跳转，不伪造成功状态。
6. **面板内核升级绕过 dock**：`disableUpgradeCore=1`，升级只走托管路径。
7. **提权面**：提权子进程读外部 JSON。缓解：只解析固定形状、只碰 `Mods\<id>\Disabled`，其余拒绝。
8. **许可证**：分发内核二进制与 mod 源码等于分发 GPL-3.0 作品。缓解：`THIRD_PARTY_NOTICES.md` 与关于区入口随首次分发交付；mihomo 的协议以 Meta 分支 LICENSE 为准，不采信仓库 API 元数据。

本地已有用户改动保持原样；本文不含自动提交、推送或整包合并上游的计划。
