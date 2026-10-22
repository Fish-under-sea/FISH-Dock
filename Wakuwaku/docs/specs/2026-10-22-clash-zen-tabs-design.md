# Fish Dock 新增 Clash 与 ZenDesktop 设置页设计

日期：2026-10-22（Asia/Shanghai）

状态：设计交付，等待审阅。本文不是实现完成报告。尚未修改产品代码，尚未安装或改写任何 Windhawk 注册表项，未改动本机 Clash Verge 的配置与数据。仓库内已有的用户改动保持原样。

关联：issue #3（`Fish-under-sea/FISH-Dock`）。

## 1. 目标与范围

在设置面板现有 6 个标签页之外新增两个，并复用「本地模型」页已经验证过的窗口生命周期：

1. **Clash 页**：dock 自己拉起 mihomo 内核，控制台在 dock 内嵌展示，节点组／延迟／模式切换／连接列表都在 dock 内完成。
2. **ZenDesktop 页**：管 Windhawk mod 的启停，勾选后点「应用」一次性生效；另含一键部署 ZenDesktop 的 9 个 mod。

两项共同的硬要求：**与「本地模型」页的锁定 + 置顶生命周期完全一致** —— 进入该页默认开启锁定与置顶，用户主动关闭面板、或切到别的顶级标签页时自动解除；当前次进入中用户手动取消的状态不被重绘覆盖。

明确排除：

- 不接管、不修改、不停止用户已经安装的 Clash Verge（含它的服务、配置、profile、内核）。
- 不实现代理规则编辑器，不内建订阅解析器；导入订阅沿用 mihomo 自身能力。
- 不代用户在 Windhawk UI 里完成首次编译（v1.7.3 无 CLI，见 4.5）。
- 不引入新的 Rust 依赖。注册表读写用已在 `src-tauri/Cargo.toml:80` 的 `winreg = "0.56.0"`，HTTP 用已在用的 `ureq`，本地服务用已在用的 `tiny_http`。
- 不改动 dock 之外的任何用户数据目录。

## 2. 探查结论与证据边界

| 项目 | 已确认（本机或上游源码实测） | 尚待真实环境验证 |
| --- | --- | --- |
| Docker 现有页签机制 | `src/pet/settings.js:47` 的 `TABS` 驱动；`:178` 用 `TABS.length` 算指示条宽度；CSS `settings.css:274-283` 的 `.s-tabs .ind` 是 `calc((100% - 6px) / var(--tabs, 4))` | 8 个页签在窄屏（`MODELS_W` 之外的最小宽度）下的文字折行 |
| 锁定 + 置顶生命周期 | `settings.js:2954 syncModelFocus` 由 `open`(:2906)、`close`(:2965)、`setTab`(:2990) 三处调用；后端 `settings.rs:693 settings_focus`、`island.rs:1495 keep_on_top` 的 `topmost = !(open && locked && !top)` 已就绪 | 三页签连续切换时原生置顶的实际抖动 |
| Clash Verge 控制器可否借用 | **不可借**。`verge.yaml:83 enable_external_controller: false`，`127.0.0.1:9097` 实测 closed；`\\.\pipe\verge-mihomo` 与 `\\.\pipe\clash-verge-service` 存在，但非提权进程打开被拒（Win32 err 5 / Access denied） | 提权会话下是否可开（本设计不依赖此路径） |
| 内核可执行文件 | 本机已有 `C:\Program Files (x86)\OTHER APP\Clash Verge\verge-mihomo.exe`，`-v` 输出 `Mihomo Meta v1.19.21 windows amd64 with go1.26.1` | 其它机器上只有官方 `mihomo` 或完全没有内核时的分支 |
| mihomo 命令行 | `-d` 配置目录、`-f` 配置文件、`-ext-ctl` 覆盖控制器地址、`-ext-ui` 覆盖 UI 目录、`-secret` 覆盖密钥、`-t` 测试配置、`-v` 版本 | 更早版本是否同样支持 `-ext-ui`（备选见 3.6） |
| zashboard 静态托管 | 官方产物 `dist-no-fonts.zip` 1.44 MB；README 明示服务在同源时自动连接同源控制器；支持 URL 参数 `hostname` / `port` / `secret` / `disableUpgradeCore` / `disableTunMode` | WebView2 中 PWA 与 Service Worker 的实际行为 |
| Windhawk 安装与 mod 表 | deploy.ps1 的探测顺序与注册表项结构（见 4.2）；`winreg` 依赖已存在 | 本机**未安装** Windhawk，安装分支与真实 mod 表待实测 |
| 启停是否需要重启 | 引擎用 `RegNotifyChangeKeyValue(..., REG_NOTIFY_CHANGE_NAME \| REG_NOTIFY_CHANGE_LAST_SET, ...)` 监听注册表（`engine/storage_manager.cpp`），写 `Disabled` 即被察觉 | 启用一个未编译 mod 后的真实表现（预期不加载） |
| 首次编译可否脚本化 | **不可以**。v1.7.3 tag 源码树无 `src/windhawk-core/cli/**`；命令行开关只有 `-check-for-updates` / `-new-updates-found` / `-app-settings-changed` / `-exit` / `-restart` / `-restart-bg` | 未来版本若并入 CLI，本设计可省掉跳转步骤 |
| 许可证 | mihomo **GPL-3.0**、Clash Verge Rev **GPL-3.0**、Windhawk **GPL-3.0**、ZenDesktop **GPL-3.0**、zashboard MIT、dock 本体 MIT | 分发形态定稿后由人工复核声明文本 |

两处容易误判的地方，写清楚备查：

- `api.github.com/repos/MetaCubeX/mihomo` 的 `license` 字段返回 MIT，但那属于默认分支 `main` —— 一个无关的 Python 项目。Go 内核在 `Meta` 分支，LICENSE 是 GPL-3.0（blob `f288702d2fa16d3cdf0035b15a9fcbc552cd88e7`，35149 字节）。**不能按仓库元数据字段判断内核协议。**
- ZenDesktop 的 9 个 mod 里，5 个基于 m417z 的 Windows 11 *Styler（GPL-3.0），`wobbly-windows` 原 GPL-3.0，只有 `windows-animations`（ReDrag）是 MIT。

## 3. Clash 页设计

### 3.1 为什么必须是 dock 自管内核

三条现成路径都走不通：

1. 用 Clash Verge 的 HTTP 控制器 —— 它默认关闭（`verge.yaml:83`），端口不监听。
2. 用 Clash Verge 的命名管道 —— 管道存在，但 ACL 只给提权进程，dock 常规运行拿不到。
3. 起 dock 自己的提权副本来读管道 —— 为一个面板长期挂一个提权进程，代价与风险都不划算。

所以 dock 用自己的一套内核＋自己的一套数据目录，与用户已装的 Clash Verge 并行、互不干涉。代价是内核运行期内本机存在两个代理内核，这一点由 3.7 的端口与 TUN 策略约束。

### 3.2 内核发现

按顺序取第一个存在的可执行文件，全部只做路径判定，不做执行探测：

1. dock 数据目录下的托管副本 `<data>/clash/core/mihomo.exe`（3.4 下载所得）。
2. 环境变量 `DSH_FISH_CLASH_CORE` 指定的路径。
3. Clash Verge 安装目录：先读 `HKLM\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\Clash Verge` 与 `...\WOW6432Node\...` 的 `InstallLocation`，再回退 `%ProgramFiles(x86)%\OTHER APP\Clash Verge\`。
4. PATH 中的 `mihomo.exe`、`verge-mihomo.exe`。
5. `%LOCALAPPDATA%\Programs\*` 下的同名文件。

一个都找不到时，页面显示「未找到 mihomo 内核」并提供两个动作：选择本地文件、或从官方 release 下载（3.4）。不静默失败，也不去拉起安装程序。

版本探测用 `mihomo -v` 只读执行一次，把版本串显示在页面上；`-v` 无法执行时不阻塞主流程，只标注版本未知。

### 3.3 配置与数据目录

dock 的数据目录沿用 `oflm.rs:235 data_directory()` 的约定（`%USERPROFILE%\.dsh-fish`，可由 `DSH_FISH_DATA` 覆盖），Clash 相关文件全部放在其下：

```
<data>/clash/
  config.yaml          生成的配置（每次启动重写）
  core/mihomo.exe      托管内核（若走 3.4 下载）
  ui/                  zashboard 静态文件（3.5）
  geoip.dat geosite.dat Country.mmdb ASN.mmdb
  logs/
```

生成配置只包含本设计负责的字段，避免把用户的订阅内容写进代码路径：

```yaml
mixed-port: {MIXED_PORT}
socks-port: {SOCKS_PORT}
mode: rule
log-level: warning
external-controller: 127.0.0.1:{CTL_PORT}
secret: {SECRET}
external-ui: ui
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

- 三个端口都用 `std::net::TcpListener::bind(("127.0.0.1", 0))` 取空闲（`widgets.rs:1610` 已有相同写法），避免与用户已装的 Clash Verge 抢 7897 / 7898 / 9097。取到后立刻释放再交给内核，存在极小的竞态窗口，重试一次即可。
- `SECRET` 每次启动生成 32 位十六进制随机串，只存在于内存与这份配置文件里，不回传到前端日志，不进 dock 的 settings 存储。
- `external-ui: ui` 是相对 `-d` 目录的路径，让 mihomo 自己在控制器端口上把面板服务出来 —— 这是本设计解决跨域的关键，见 3.5。
- geo 数据文件优先从用户已装的 Clash Verge 数据目录 `%APPDATA%\io.github.clash-verge-rev.clash-verge-rev\` 复制（本机实测四个文件均在）；缺失时再按需下载。**只读取它们的字节，不读 profile、不读订阅、不读凭据。**

订阅与节点由用户在面板里自行导入，最终写入 `<data>/clash/config.yaml` 的 `proxies` / `proxy-groups` / `rules` 段。dock 不解析、不翻译、不缓存订阅内容。

### 3.4 内核获取

仅在 3.2 前三条都失败、且用户主动点击「下载内核」时才发起：

1. 从 `MetaCubeX/mihomo` 的 `Meta` 分支 release 取 `mihomo-windows-amd64-{version}.zip`（arm64 取对应资产）。
2. 下载到 `<data>/clash/core/` 下的临时文件，校验 zip 完整性后解压，落到 `mihomo.exe`。
3. 记录来源 URL 与版本号到 `<data>/clash/core/SOURCE.json`，供许可证声明引用。

失败时保留临时文件路径并给出可读原因，不重试超过两次。

### 3.5 面板托管：同源，零 CORS

采用 **mihomo 自带 `external-ui`** 方案，而不是让 dock 的 `tiny_http` 去挂静态文件：

- zashboard 的 `dist-no-fonts.zip` 解到 `<data>/clash/ui/`，启动时用 `external-ui: ui`（或 `-ext-ui <绝对路径>`）交给内核。
- 内核在 `http://127.0.0.1:{CTL_PORT}/ui/` 上同时提供面板与 API，**同源**。
- iframe 地址：`http://127.0.0.1:{CTL_PORT}/ui/?secret={SECRET}&disableUpgradeCore=1`。zashboard README 的「自动连接同源控制器」一节明确这条路可行，`secret` 参数会应用到自动探测出的后端。

放弃的方案及原因：让 `server.rs` 挂 `/clash/` 前缀 —— 面板与控制器仍跨端口，必须给内核配 `external-controller-cors.allow-origins` 加 dock 的源，而 dock 页面在打包后是 `http://tauri.localhost`、开发时是另一个源，等于把 CORS 白名单维护成一个易错点；同时 `server.rs:174` 的 `match (method, path)` 只有 `POST /hook` 与 `GET /health` 两条，加静态文件服务还要处理路径穿越、缓存头与 MIME，工作量与风险都比同源方案大。

`disableUpgradeCore=1` 是必要的：面板的「升级内核」会替换 dock 托管的二进制，绕过 dock 的版本记录，也绕过分发侧的许可证声明。

### 3.6 备用托管路径

若某个内核版本忽略 `external-ui`，回退到 dock 的 `tiny_http`：新增 `GET /clash/<path>` 路由，只服务 `<data>/clash/ui/` 下的文件，路径规范化后必须仍在该目录内（拒绝 `..` 与符号链接逃逸），只允许已知扩展名，返回固定 `Content-Type`，不加 `Directory` 索引。此时必须同时把 `http://127.0.0.1:{dock_port}` 写进生成配置的 `external-controller-cors.allow-origins`。

本设计以 3.5 为主线；3.6 只在实测证明 `external-ui` 不可用时启用，并在文档里记录实际走的是哪条。

### 3.7 端口与 TUN 策略

- 检测到 `clash_verge_service` 正在运行或 `verge-mihomo` 进程存在时，页面顶部给一条提示：「Clash Verge 正在运行，两个内核同时接管流量会互相覆盖」。仅提示，不阻止，不停服务。
- dock 生成配置**默认不开 TUN**（不写 `tun` 段）。开启入口留在面板自身，由用户显式操作；dock 不在自己的设置里提供 TUN 开关，避免第二个 TUN 虚拟网卡与 Clash Verge 冲突时无人知道是谁开的。
- 系统代理（Windows 的 `ProxyServer`）默认不接管。若用户要全局走 dock 内核，用面板里的「系统代理」开关，其效果与 Clash Verge 的同名开关互斥。

### 3.8 进程生命周期

新增 `src-tauri/src/clash.rs`，形状照 `oflm.rs`：

| 命令 | 语义 |
| --- | --- |
| `clash_ensure(app, origin) -> Result<ClashReady, ClashFailure>` | 首次调用时发现内核、分配端口、写配置、拉起子进程、轮询控制器就绪；返回 `{ url, version, ports, warning? }`。`origin` 只用于校验调用方是 dock 自己的页面 |
| `clash_state() -> Value` | 只读查询：是否在跑、内核路径与版本、端口、面板是否可打开、错误码 |
| `clash_stop(app)` | 有序终止：先请求内核退出，超时后强杀进程树，回收端口 |

细节：

- 子进程隐藏控制台，用 `oflm.rs:99 no_console`（`CREATE_NO_WINDOW` / `0x0800_0000`）。
- 就绪判定：轮询控制器 `GET /version`（带 `Authorization: Bearer {SECRET}`），间隔 200 ms，上限 20 s。超时返回 `ClashFailure`，并附内核 stderr 的尾部若干行（`logs/` 里已有全文）。
- `origin` 白名单沿用 `oflm.rs:111 embed_ancestors` 的思路：只放行 `http://tauri.localhost` 与 `http://localhost:*` / `http://127.0.0.1:*`，其它返回 `ORIGIN_REJECTED`。
- 进程状态与 `oflm.rs` 一样放在一个 `static Mutex` 里，实现 `shutdown()` 供 dock 退出时调用。
- 失败码与 oflm 的做法保持一致（`KERNEL_MISSING`、`SPAWN_FAILED`、`READY_TIMEOUT`、`PORT_TAKEN`、`ORIGIN_REJECTED`），前端按码取文案以便本地化。

### 3.9 前端宿主

新增 `src/pet/clash.js`，宿主模式照 `models.js`：

- 单例层 `#clash-host`，`position: fixed`，off-screen 停放（`left: -99999px`）而**不是** `display: none` —— 被隐藏的 WebView2 frame 会被节流，切回时面板需要重新加载。
- 导出 `window.Clash = { enter, park, solids, holds, hovered }`，与 `Models` 同名同形，好让 `settings.js` 与 `island.js` 用同一套钩子。
- 槽位选择器 `#island-settings .s-clash-slot`；CSS 把 `#models-host` 的规则改成 `#models-host, #clash-host` 共用一份，避免复制粘贴引入偏差。
- 首帧只建一个占位 iframe，`invoke('clash_ensure', { origin })` 拿到 `url` 后赋给 `src`；失败时层内显示错误文案与「重试」按钮，形同 `models.js` 的 note／retry。

## 4. ZenDesktop 页设计

### 4.1 页面职责

三块内容，自上而下：

1. **Windhawk 状态卡**：已安装（显示路径与版本）／未安装（给安装入口）／安装但引擎未运行。
2. **mod 列表**：每行一个 mod，左侧勾选框，右侧显示状态。勾选只改本地待提交集合，不立刻写盘。
3. **动作条**：`应用`（提交 diff，一次 UAC）、`一键部署 ZenDesktop 9 个 mod`、`打开 Windhawk`（跳转首次编译）。

### 4.2 Windhawk 探测

照 ZenDesktop `deploy.ps1:84-107` 的顺序，全部用 `winreg` 只读：

1. `HKLM\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\Windhawk` 的 `InstallLocation`。
2. `HKLM\SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall\Windhawk` 的 `InstallLocation`。
3. `%ProgramFiles%\Windhawk`、`C:\Program Files\Windhawk`。
4. 判定标志统一是「`<dir>\windhawk.exe` 存在」。

数据根判定照 `deploy.ps1:133-170`：默认 `%ProgramData%\Windhawk`；若 `<dir>\windhawk.ini` 中有 `AppDataPath = ...` 则展开环境变量后采用（相对路径拼接到 `<dir>`）；否则以 `<dir>\AppData\ModsSource` 是否存在判断 portable 布局。

本机当前**未安装** Windhawk（四个候选路径、进程、服务全无），所以「未安装」不是边界情况而是默认路径，必须先做。

### 4.3 mod 列表与状态

枚举 `HKLM\SOFTWARE\Windhawk\Engine\Mods` 的每个子键，读取：

| 值 | 类型 | 用途 |
| --- | --- | --- |
| `Disabled` | DWORD | 1 = 关闭，0 = 开启（v4.2 起 deploy 默认写 1） |
| `Version` | String | 显示用 |
| `Include` / `Exclude` | String | 显示该 mod 作用的进程，帮助用户判断影响面 |
| `LibraryFileName` | String | 空 = 尚未编译 |
| `LoggingEnabled` | DWORD | 透传，不在本页修改 |

每个 mod 归入四态之一，决定勾选框是否可用：

| 状态 | 判据 | 勾选行为 |
| --- | --- | --- |
| 已编译·开启 | `LibraryFileName` 非空且 `Disabled = 0` | 可勾选 |
| 已编译·关闭 | `LibraryFileName` 非空且 `Disabled = 1` | 可勾选 |
| 未编译 | `LibraryFileName` 为空，且 `ModsSource\<id>.wh.cpp` 存在 | 勾选后提示「应用后需在 Windhawk 里确认编译一次」，并亮出「打开 Windhawk」 |
| 无源文件 | `LibraryFileName` 为空且 `ModsSource` 里没有对应源文件 | 勾选框禁用，行内说明「源文件缺失」 |

`ModsSource` 下的文件名与注册表 id 的对应关系照 `deploy.ps1:269`：`$id = $file.BaseName -replace '\.wh$',''`，即 `local@zen-taskbar-acrylic.wh.cpp` 对应 id `local@zen-taskbar-acrylic`。

### 4.4 应用：一次提权，一个 diff

前端只维护 `dirty` 集合（id → 期望的 `Disabled` 值）。点「应用」时：

1. 前端把 diff 交给 `zen_apply(app, changes)`。
2. 若当前已提权（`lhm_setup.rs:1722 is_elevated()` 为真），直接写。
3. 否则把请求写成 JSON 到临时文件，用 `lhm_setup.rs:1850 elevate(owner, file, params, dir, wait_ms)` 以 `runas` 拉起**自身 exe**，参数 `--wakuwaku-zen apply <request.json>`，子进程写完把结果写到旁边的 result 文件，父进程轮询读取。这与 `settings.rs:945` 注释描述的「每个动作启动自身 exe 提权、一次 UAC」是同一套既有机制，`main.rs:1453` 起的参数分派里加一个分支即可（照 `:1519 lhm_setup::ADMIN_FLAG` 的写法）。

提权子进程只做三件事：校验请求 JSON 的形状、写注册表、写结果文件。它不启动 pet、不占端口、不读用户数据。

写入范围严格限制在 `HKLM\SOFTWARE\Windhawk\Engine\Mods\<id>` 的 `Disabled` 一个值。**不改** `Include` / `Exclude` / `Architecture` / `Version` / `LibraryFileName` —— 那属于 mod 的部署语义，不是启停语义，改错会静默改变 mod 的作用面。

写完不需要重启 Windhawk 服务：引擎的 `RegNotifyChangeKeyValue` 会自己察觉。页面在应用成功后重新读一次注册表并刷新勾选状态，以注册表为准而非以本地 diff 为准。

### 4.5 一键部署 9 个 mod

对应 `deploy.ps1:266-350` 的等价流程，作为 `--wakuwaku-zen deploy <request.json>` 在同一个提权进程里完成：

1. 下载 ZenDesktop 仓库根目录 7 个与 `optional/` 下 2 个 `local@*.wh.cpp`，合计约 3.5 MB；每个文件记录来源 URL 与 SHA-256。
2. 复制进 `<windhawkData>\ModsSource`。同名文件已存在时：内容一致则跳过，不一致则先备份成 `.bak-<时间戳>` 再覆盖。
3. 从源文件头部注释取默认值，规则照 `deploy.ps1`：`// @version` 缺省 `4.1.0`、`// @architecture` 缺省 `x86-64`、`// @include` 缺省 `explorer.exe`、`// @exclude` 可空。
4. 写 `HKLM\SOFTWARE\Windhawk\Engine\Mods\<id>`：`Disabled=1`、`Include`、`Exclude`、`Architecture`、`Version`、`LibraryFileName`（空串）、`LoggingEnabled=0`。
5. 建 `HKLM\SOFTWARE\Windhawk\Engine\ModsWritable\<id>`。
6. 引擎进程表：仅在 `HKLM\SOFTWARE\Windhawk\Engine\Settings` 的 `Include` 中**追加** `dwm.exe`，已存在则跳过。**不写** `Exclude` 里的游戏与反作弊列表 —— 那是 deploy.ps1 包内默认值的主观策略，dock 不替用户做这个决定。

对**已存在**的 mod，保留用户当前的 `Disabled` 值不覆盖，遵循 deploy.ps1 的 v4.2+ 原则「只装不开」。这样重复点「一键部署」是幂等的，也不会把用户先前打开的 mod 静默关掉。

部署完成后刷新列表，所有新 mod 处于「未编译」，页面给出下一步提示。

### 4.6 首次编译必须跳转 Windhawk

v1.7.3 没有 CLI（`src/windhawk-core/cli/**` 不在该 tag 的源码树里，只在 `main` 分支），命令行开关只有 `-check-for-updates` / `-new-updates-found` / `-app-settings-changed` / `-exit` / `-restart` / `-restart-bg`，**没有编译类开关**。所以：

- 应用或部署之后，若存在「已勾选但 `LibraryFileName` 为空」的 mod，页面显示明确的下一步：「在 Windhawk 里确认编译一次」，并给按钮直接 `ShellExecuteW` 打开 `windhawk.exe`。
- 不在 dock 内伪造编译进度条，也不把「已写注册表」表述成「已生效」。

### 4.7 卸载与还原

ZenDesktop 的 `uninstall.ps1` 提供 `-Yes` 与 `-RemoveData`。dock 不内建卸载，只在页面给出「打开 ZenDesktop 目录」与文档链接，避免 dock 承担删除用户注册表内容的后果。若后续需要，作为独立动作与用户在 issue 里确认后另议。

## 5. 公共交互：把 models 专用的钉住逻辑抽成集合

现状 `settings.js:2954` 只认 models：

```js
function syncModelFocus() {
  const active = isOpen && tab === 'models'
  if (active === modelFocus) return
  modelFocus = active
  if (snap?.settings) {
    snap.settings.settingsPin = active
    snap.settings.settingsTop = active
  }
  window.pet.settings.focus(active).catch(e => console.log('[settings focus]', e))
}
```

改为集合判断，并保留 `modelFocus` 作为「上次提交的布尔值」不变：

```js
const PINNED = new Set(['models', 'clash', 'zen'])
function syncPinnedFocus() {
  const active = isOpen && PINNED.has(tab)
  if (active === modelFocus) return
  modelFocus = active
  ...
}
```

`open`(:2906)、`close`(:2965)、`setTab`(:2990) 三处调用点改名即可。`open()` 里 `if (TABS.includes(next)) tab = next` 已经能接受新增页签，`PART_OF`(:49) 也不需要改。

同步要改的三处硬编码：

1. **宽度与高度**。`:1396-1397` 的 `MODELS_W` / `MODELS_H` 加一对 `CLASH_W = 1120` / `CLASH_H = 760`，再建 `const SIZED = { models: [MODELS_W, MODELS_H], clash: [CLASH_W, CLASH_H] }`；`draw()`(:2858、:2874) 用 `const [pw, ph] = SIZED[tab] || [0, 460]` 取代三元表达式。ZenDesktop 页不嵌入外部界面，走普通宽度与默认高度，不进 `SIZED`。
2. **宿主停放**。`:2814` 的 `if (tab === 'models')` 分支旁边加 `if (tab === 'clash')`；`:2823` 的 `Models.park()` 变成 `Models.park(); Clash.park()`。
3. **页签与文案**。`:47` 的 `TABS` 追加 `'clash', 'zen'`；`:48` 的 `TAB_KEY` 加 `clash: 's.tabClash', zen: 's.tabZen'`；`:2770` 的 `PAGES` 加 `clash: pageClash`（返回 `''`，与 `pageModels` 同样理由）与 `zen: pageZen`（真正返回标记）。

CSS 侧 `settings.css:279` 的 `.s-tabs .ind` 用 `var(--tabs, 4)`，而 `:178` 已经 `tabs.style.setProperty('--tabs', TABS.length)`，所以 8 个页签不需要改 CSS。需要确认的是 8 个按钮在最小宽度下不换行 —— `settings.css:285-296` 的 `padding: 6px 0` 与 `font-size: 12.5px` 在中文双字标签下应仍可容纳，实测后按需把 `font-size` 降到 12px。

`island.js` 的 `Models` 引用（`layerHover` / `hovered` 钩子，:4586、:4604、:5759、:5766、:5785）需要同样为 `Clash` 加一份，否则面板 hover 时宿主层不会跟随。

## 6. 命令与参数清单

新增 Tauri 命令（注册进 `main.rs:1625` 的 `generate_handler!`，紧邻 oflm 三件套）：

```
clash_ensure(app, origin) -> ClashReady | ClashFailure
clash_state()             -> Value
clash_stop(app)           -> ()
zen_state()               -> ZenState
zen_apply(app, changes)   -> ZenApplied | ZenFailure
zen_deploy(app)           -> ZenDeployed | ZenFailure
```

新增命令行分支（`main.rs:1453` 起的 `args` 分派，照 `:1519` 的模式）：

```
--wakuwaku-zen apply  <request.json>
--wakuwaku-zen deploy <request.json>
```

`request.json` 与 result 文件都放在 `<data>/clash/` 之外的临时目录，写完即删。

## 7. 测试与验收

### 7.1 前端

- 扩 `test/settings-focus.test.js`（56 行，用 `vm` 切片 `lifecycle`）：把 `TABS` 改为含三个钉住页，断言进入 `zen` 也发 `focus(true)`、切到 `mail` 发 `focus(false)`、三个钉住页连续互切只发必要的布尔值、关闭后迟到快照不重绘。现有四条断言不得回归。
- 新增 `test/clash-layer.test.js`，照 `models-layer.test.js` 的做法验证：`Clash.enter` 建层且不重复建、`park` 是 off-screen 而非 `display: none`、`clash_ensure` 失败时进入错误态并可重试、`holds` / `hovered` 的返回值形状与 `Models` 一致。
- `i18n.test.js` 会校验中英键成对，新增键必须两边各一处，跑它即验证。

### 7.2 Rust

纯函数级测试，不需要真实内核与真实 Windhawk：

- 内核发现顺序：给定候选存在性组合，返回第一个命中项；全缺失时返回 `KERNEL_MISSING`。
- 端口分配：连续三次分配得到三个不同端口。
- 配置生成：随机 secret 只出现在生成文本里一次；`external-ui: ui` 存在；不出现 `tun` 段。
- `wh.cpp` 头注释解析：缺 `@version` 取 `4.1.0`、缺 `@architecture` 取 `x86-64`、缺 `@include` 取 `explorer.exe`；有 `@exclude` 时原样保留。
- 请求与结果 JSON 的形状校验：拒绝多余字段与越界 id；result 文件缺失时返回明确错误而非 panic。
- 注册表 id 推导：`local@x.wh.cpp` → `local@x`。

### 7.3 实机验收

| 项 | 判据 |
| --- | --- |
| Clash 页进入 | 打开即锁定 + 置顶；关闭面板或切到别的页后两者都解除；当前次进入中手动取消不被重绘覆盖 |
| Clash 页运行 | 内核拉起成功、面板可交互（点节点、看延迟、切模式、看连接表）、`clash_stop` 后端口全部释放、无残留进程 |
| Clash 页失败分支 | 移除内核后可读报错并重试；面板 URL 变更后旧 iframe 不残留 |
| ZenDesktop 未安装 | 页面明确显示未安装并给安装入口，不抛错、不弹提权 |
| ZenDesktop 启停 | 勾选 → 应用 → 一次 UAC → 注册表值改变 → 引擎感知 → 面板刷新后勾选状态与注册表一致 |
| ZenDesktop 一键部署 | 9 个源文件落盘、9 个注册表项建立、重复执行幂等、已开启的 mod 未被改回关闭 |
| 未编译提示 | 勾选未编译 mod 后出现「需在 Windhawk 编译」提示，且不宣称已生效 |
| 页签布局 | 8 个页签在最小宽度下不折行、指示条位置正确 |
| 回归 | `node --test "test/*.test.js"` 现有基线不退化；`cargo test` 现有基线不退化 |

### 7.4 许可证交付物

新增仓库根目录 `THIRD_PARTY_NOTICES.md`（当前不存在），并把它作为「关于」区（`settings.js:2504 aboutHTML`）的一个可展开项：

| 组件 | 协议 | 交付义务 |
| --- | --- | --- |
| zashboard | MIT | 保留版权与许可声明 |
| mihomo 内核二进制 | GPL-3.0 | 附全文 + 对应版本源码获取方式（3.4 的 `SOURCE.json` 记录） |
| ZenDesktop 的 9 个 mod 源码 | GPL-3.0（`windows-animations` 为 MIT） | 附全文 + 仓库出处 + 各 mod 作者；5 个基于 m417z Windows 11 *Styler 需一并署名 |
| Windhawk | GPL-3.0 | dock 不链接、不分发，仅在文档说明依赖关系 |

dock 本体保持 MIT。

## 8. 落地顺序

1. 抽 `PINNED` 集合 + `SIZED` 表 + 两个页签的空壳。可独立验证，是后续两项的前置。
2. Clash 页：`clash.rs` 三件套 → `clash.js` 宿主 → 配置文件生成 → 面板同源托管 → 失败分支。
3. ZenDesktop 页：`zen.rs` 只读探测 → mod 列表四态 → `--wakuwaku-zen apply` 提权写入 → 一键部署 → 未编译提示。
4. `THIRD_PARTY_NOTICES.md` + 关于区入口 + 文档。

2 与 3 互不依赖，可并行；1 是它们的前置。

- [x] 调查 Clash 与 ZenDesktop 的上游协议与机制
- [x] 读现有 models 页、`models.js`、`oflm.rs` 与提权路径，确认可复用的部分
- [ ] 抽 `PINNED` 与 `SIZED`，落两个空壳页签
- [ ] Clash 页实现与联调
- [ ] ZenDesktop 页实现与联调
- [ ] 第三方声明与文档

## 9. 已知风险

1. **两个代理内核并存**。dock 内核与 Clash Verge 同时运行会争系统代理与 TUN。缓解：默认不开 TUN、不接管系统代理、运行前提示，端口全部动态分配。
2. **Windhawk mod 与 Windows 累积更新冲突**。这类 mod 钩 `explorer.exe` / `dwm.exe`，系统更新后失效甚至导致资源管理器异常。缓解：页面常驻一句说明；一键部署默认全关；不代替用户在 Windhawk 里编译。
3. **首次编译无法脚本化**。v1.7.3 无 CLI，短期内只能跳转 UI。缓解：把这一步显式做成 UI 流程，不伪造成功状态。
4. **面板内核升级绕过 dock**。缓解：`disableUpgradeCore=1`，升级只走 3.4 的托管路径。
5. **提权面**。写 `HKLM` 必须提权，且提权子进程读外部 JSON。缓解：子进程只解析固定形状、只碰 `Mods\<id>\Disabled` 一个值，其余一律拒绝。
6. **许可证**。一键部署 mod 等于分发 GPL-3.0 作品。缓解：`THIRD_PARTY_NOTICES.md` 与「关于」区入口，随首次分发一起交付。

本地已有用户改动保持原样；本文不含自动提交、推送或整包合并上游的计划。
