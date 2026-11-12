# 插件自启与冷启四项症状诊断报告

> 结论先行：这四项症状不是四个独立 Bug，而是同一条链上的四个截面——**宿主把「插件启动」实现成了一段持全局锁的同步 I/O 流程**，而冷启（开机自启、更新后首启、新装插件）恰好是这条流程 I/O 最重、超时预算最紧的时刻。真正的根因是 3 条（另有 2 条放大器，见 §7.1）：
>
> 1. 宿主服务「插件 → 宿主」回调的时机被限制在「正在等这个插件自己的响应」的窗口内，窗口外一律排队（`plugin_process.rs:56`、`:92`），SDK 侧只等 5 s，于是冷启必然出现「宿主调用超时」；
> 2. 每次 Dock 启动都对插件包全文 SHA-256（实测 75.9 MB + 53.9 MB），且这段哈希在 `CURRENT` 全局锁内、在拉起进程之前串行执行（`plugin_install.rs:74`、`plugin_process.rs:25`），把其它插件的回调窗口一起吃掉；
> 3. 更新后首启对整棵资源树做全量校验（实测 10,513 文件 / 1.30 GB）并对 WebView2 目录递归重置 ACL（`runtime.rs:361-380`），而「是否需要重校」的判据是 `manifest.json` 的摘要——每次更新都变，所以每次更新后必重跑一遍。
>
> 另有两处「看起来像模板」的硬编码额度补偿（`view.js:44`、patch `:3621-3635`），是签到页「不实时」的直接成因。
>
> 编制：2026-11-02　基线：Dock 2.0.2（`Cargo.toml:6`）／插件 `fish.magpie 2.1.2`、`fish.clash 2.1.2`　本机实测数据出处见 §9。

## 1. 摘要

| # | 症状 | 根因（一句话） | 证据强度 | 归属仓 |
| --- | --- | --- | --- | --- |
| A | 开机自启的首次 Dock 启动，插件自启拉起异常；之后手动恢复就正常 | 冷启时插件包全文哈希与宿主回调串行化叠加，撞穿 5 s / 2 s 超时预算；`view()` 还会把「探针超时」误判成「进程已死」并**杀掉活着的插件** | **实测日志级**（`plugin-failures.log` 2 条、`gateway-error.log` 3 条） | 主仓（宿主）+ 插件仓（重试语义） |
| B | 每次更新后首次启动特别慢，之后恢复正常 | 更新改变了 `manifest.json` 摘要 → 首启全量校验 1.30 GB + WebView2 递归 ACL | **实测数据级**（文件数/字节数/哈希吞吐实测） | 主仓 |
| C | 新装 Magpie 插件后「预装插件美化」图标加载异常、插件报错，过一会儿自愈 | 美化素材跟着**预装供应商插件**走，而预装只登记 npm spec、由 magpie 自己的更新循环异步装；装完前图标无处可取，装完自动恢复 | 代码级 + 设计文档级（未取到发生时刻的日志） | 插件仓（magpie 页面 + 预装）+ 主仓（装箱期） |
| D | 签到页不实时采集部分签到服务，像固定模板 | 读接口按设计**只读台账不采集**（`checkin:state` 注释「只读，不发起任何签到」），且快照只覆盖 5 个固定落盘文件；再叠两处硬编码额度补偿（400 / 100），于是整屏长期不变 | **代码级 + 多源交叉** | 插件仓（页面 + magpie 补丁） |

三条横切结论：

- **超时预算是为热态设计的**：`plugin_activate_secs=5`、SDK 宿主调用 5 s、探针 2 s、Node 探测 2 s。热态实测 `node --version` 只需 17–33 ms（60× 余量），冷态下同样的操作叠加 Defender 扫描与首次读盘就没有余量。
- **全局锁里做重活**：`CURRENT`（`plugin_runtime.rs:46`）在 `resume()` 期间全程持有（`:152-185`），期间还夹着 `recover()`／`cleanup_staging()`／逐插件 `cleanup()`／逐插件 `verify()`。设置页任一次 `extension_view` 都要排在这把锁后面。
- **失败不可自愈**：`resume()` 失败后没有任何后台重试（`plugin_api.rs:160` 只打一行日志），所以「首次失败、手动恢复就好」的体验是必然结果，不是偶发。

## 2. 探测环境与方法

### 2.1 环境

| 项 | 实测值 |
| --- | --- |
| 安装目录 | `C:\Users\Fish\AppData\Local\Programs\FISH Dock` |
| 安装资源清单 | `resources\manifest.json`：`schema=2`、`version=2.0.2`、**10,513 个文件 / 1,392,894,194 字节（约 1.30 GB）** |
| 资源分布 | `windhawk` 8,413 文件 / 585 MB；`webview2` 256 文件 / 705 MB；`node` 1,840 文件 / 103 MB；`licenses` 4 文件 |
| 宿主数据根 | `%USERPROFILE%\.fish-dock`（`storage_context`）／`%LOCALAPPDATA%\FISH Dock`（`runtime::app_home`） |
| 插件数据 | `.fishplugin-magpie`、`.fishplugin-clash`（各含 `config/`、`credentials/`、`tmp/`） |
| 已装插件包 | `fish.magpie 2.1.2`：**80 文件 / 75,938,315 字节**；`fish.clash 2.1.2`：**26 文件 / 53,934,360 字节** |
| 内置 Node | `resources\node\node.exe` 91 MB（91,380,224 字节） |
| 最近一次更新 | `install.log`：2026-11-01T16:36:00Z 完成，「替换 1 个文件，复用 10259 个，耗时 2.1 秒」 |

### 2.2 实测数据（用于本文全部量化判断）

| 测量项 | 结果 | 意义 |
| --- | --- | --- |
| SHA-256 吞吐（`webview2` 全树 256 文件 / 705,180,320 字节） | **2.32 s → 290 MB/s** | 1.30 GB 全量校验热态约 4.8 s；冷启 + Defender 扫描可达数十秒 |
| `node.exe --version` ×3 | 32.9 / 23.1 / 16.6 ms | 2 s 预算热态余量 60×，冷态无余量 |
| 宿主每次启动需哈希的插件包 | **129.9 MB**（75.9 + 53.9） | ≈0.45 s 热态，冷态数秒，且在 `CURRENT` 锁内串行 |
| `plugin-logs\*.log` | 三个插件全部 **0 字节** | stderr 落盘通道正常，失败现场没有插件侧输出（失败发生在协议层） |

### 2.3 现场证据原文

`%USERPROFILE%\.fish-dock\host\plugin-failures.log`（435 字节，末次写入 2026-11-02 15:35:16）：

```json
{"at":1793604881258,"error":"插件响应超时","exitCode":null,"log":"C:\\Users\\Fish\\.fish-dock\\host\\plugin-logs\\fish.clash.log","plugin":"fish.clash","stage":"运行","version":"2.1.2"}
{"at":1793604916395,"error":"插件进程无响应（握手超时），已按未运行处理","exitCode":null,"log":"C:\\Users\\Fish\\.fish-dock\\host\\plugin-logs\\fish.magpie.log","plugin":"fish.magpie","stage":"运行","version":"2.1.2"}
```

（`at` 为毫秒时间戳，折算北京时间 **15:34:41** 与 **15:35:16**；`exitCode:null` 说明进程**还活着**。）

`%USERPROFILE%\.fish-dock\.fishplugin-magpie\gateway-error.log`（399 字节）：

```json
{"time":"2026-11-02T07:31:33.167Z","code":"UNKNOWN_ERROR","nativeCode":null,"phase":null,"port":3425,"message":"宿主调用超时"}
{"time":"2026-11-02T07:31:38.789Z","code":"UNKNOWN_ERROR","nativeCode":null,"phase":null,"port":3425,"message":"宿主调用超时"}
{"time":"2026-11-02T07:31:45.301Z","code":"UNKNOWN_ERROR","nativeCode":null,"phase":null,"port":3425,"message":"宿主调用超时"}
```

（折算北京时间 **15:31:33 / 15:31:38 / 15:31:45**，间隔 5.6 s 与 6.5 s，与 `startWithBackoff(attempts:3, delays:[600,1500])` 的节奏一致；`宿主调用超时` 这句话全仓只有一处产出：`FISH-Dock/sdk/node/plugin.mjs:34`。）

三条日志合起来说明：**冷启这一次，宿主→插件的 RPC 与插件→宿主的回调，两边都撞到了超时**，而且发生在同一分钟量级内。

## 3. 症状 A：开机自启的首次启动，插件拉起异常

### 3.1 现象与复现口径

- 只在**开机自启的那一次**发生；随后手动「恢复服务」（或重启 Dock）即正常。
- 表现为插件卡报错（「插件进程无响应（握手超时），已按未运行处理」／「插件响应超时」），Magpie 网关服务没起来，Clash 内核态不明。

### 3.2 现场链路（按代码还原，并用 §2.3 的时间戳校准）

```text
main.rs:1972  plugin_api::resume()            ← setup 里 spawn 线程，早于窗口显示
  └ plugin_api.rs:152-160
      ├ storage_context::current()            ← 已就绪（main.rs:1701 更早）
      ├ node(context) → plugin_dependencies.rs:27-34 probe()  ← 每次 spawn node.exe --version，死线 2 s
      ├ plugin_development::prepare()         ← 发行版是空实现
      └ runner.resume(&Manager{context}, node)   ← 全程持 CURRENT 锁
          ├ plugin_runtime.rs:154 manager.recover()        ← 读 tmp/ 事务、可能需要回滚
          ├ plugin_runtime.rs:158 cleanup_staging()        ← 扫宿主 + 每个插件命名空间
          ├ plugin_runtime.rs:163-166 逐插件 cleanup()      ← 删旧版本目录、旧摘要、回滚点
          ├ plugin_runtime.rs:170 reclaim()                ← 读 run-state.json 收孤儿内核
          └ plugin_runtime.rs:177-182 逐插件
                └ set_enabled_starting(..., autostart=true)
                    └ plugin_process.rs:25 manager.verify()  ← 该插件包全文 SHA-256（magpie 75.9 MB / clash 53.9 MB）
                    └ plugin_process.rs:36 spawn(node.exe entry)
                    └ plugin_process.rs:47 host.hello  ← 5 s 预算（config.rs:114）
                    └ plugin_process.rs:49 plugin.activate ← 5 s 预算
```

关键机制——**回调服务窗口过窄**：

- `Process::request()`（`plugin_process.rs:56-75`）与 `handshake()`（`:92-109`）在等自己那条响应时，会顺手处理插件发来的 `method` 回调（`storage_call`）。
- 一旦宿主线程离开这两段代码（去 `verify()` 哈希、去 `cleanup()` 删目录、去启动**下一个**插件），**没有任何线程**再读该插件的 stdout，插件的回调就只能躺在管道里。
- 插件侧对宿主的每次调用只等 **5 s**（`sdk/node/plugin.mjs:6` 的 `requestTimeout = 5000`，`:34` 超时即 `reject("宿主调用超时")`）。

Magpie 的自启正是重灾区：`backend.mjs:36` 在 `activate` 里 `void launchGateway()`（不阻塞握手），随后 `runtime.mjs:93-94` 要 `context.secrets.read('keys','web-key')`、`gateway.mjs:22` 的 `load()` 要 `context.storage.read('config','gateway.json')`——**这些回调都落在「宿主正在启动 Clash / 正在哈希 Clash 包」的窗口里**，于是 15:31:33 起连续三次「宿主调用超时」。

同时，`Process::start` 之前的 `verify()` 把 129.9 MB 的哈希塞进了这条单线程路径；`resume()` 又在同一条路径上先跑 `recover/cleanup`。冷启时磁盘队列最深、Defender 正在扫刚替换过的 `node.exe` 与 `magpie.exe`，这段时间只会更长。

### 3.3 为什么「手动恢复就好」

1. 第二次走同一条路径时，文件已在系统缓存里，`verify()` 与 `node.exe` 启动都快；
2. 这时通常只有一个插件在重启，没有「启动 A 期间饿死 B 的回调」的并发窗口；
3. 失败本身不会自动重试（`plugin_api.rs:160` 只 `eprintln`，`plugin_runtime.rs:183` 只汇总一行），所以「第一次坏、手动好」是设计使然。

### 3.4 一个被日志直接证实的独立缺陷：`view()` 会杀掉活着的插件

`plugin_runtime.rs:77-84`：

```rust
let gone = if let Some(process)=self.processes.get_mut(id){ (!process.running()).then(||process.exit_code()) } else { None };
if let Some(code)=gone {
    self.processes.remove(id);   // ← 返回的 Process 在这里被 drop
    ...
}
```

`Process::running()`（`plugin_process.rs:85-89`）在**探针 2 s 无应答**时就返回 `false`；而 `running()` 为假、`exit_code()` 为 `None` 的组合，正好对应日志里那条「插件进程无响应（握手超时）」。`self.processes.remove(id)` 的返回值被丢弃 → `Drop for Process`（`:116`）执行 `child.kill()`。

**结果：任意一次设置页刷新，只要插件在 2 s 内没答上 `host.hello`，它连同 Job 里的子进程（magpie.exe / mihomo.exe）一起被杀。** 这与用户「插件报错 → 服务没了 → 手动恢复才好」的体感完全吻合，也是「插件自启拉取异常」里最不该发生的一环。

### 3.5 影响面

| 维度 | 影响 |
| --- | --- |
| 触发条件 | 冷启（开机自启、更新后首启）、或任意一次重负载时刻（同时装/更两个插件、Defender 全盘扫描） |
| 波及 | 所有 Node 运行时插件（`fish.magpie`、`fish.clash`、`jassdingk.filemgr` 等）；原生运行时插件不受 `verify` 之外的路径影响 |
| 用户可见 | 插件卡红字、网关/内核没起来、标签页进不去；`plugin-failures.log` 与 `gateway-error.log` 各留痕 |
| 数据风险 | 低（不写用户数据）；但 `view()` 杀进程会中断 Clash 内核 → 系统代理租约需要恢复（`clash/runtime.mjs:13-19` 已有降级路径） |

### 3.6 修复方案

**A1（P0，宿主）探针超时不得杀进程**

- 位置：`plugin_runtime.rs:77-84`
- 改法：探针失败只置「未响应」显示态（新增 `unresponsive:true`），连续 3 次探针失败**或** `try_wait()` 明确返回已退出，才从 `processes` 移除。`exit_code()` 为 `None` 时永不 `remove`。
- 影响面：`plugin_runtime.rs`、`plugin_api.rs:36` 的 `view()` 输出、前端 `extension-panel.js` 对 `error` 的渲染（增加一种「未响应，正在重试」）。
- 风险：低。回归面：`plugin_process.rs` 的 `running()` 单测、`plugin_t30_acceptance` 的假活用例。
- 验收：人为让插件 sleep 3 s 再答 `host.hello`，反复刷设置页 20 次，插件进程 pid 不变、`plugin-failures.log` 无新增。

**A2（P0，宿主）把「插件 → 宿主」回调从请求窗口里解耦**

- 位置：`plugin_process.rs:56-75`、`:92-109`；`plugin_process.rs:43` 的读线程
- 改法：读线程收到的帧分成两类——带 `id` 的响应进对应的等待者，带 `method` 的回调**当场**在独立线程（或固定大小线程池）里执行 `storage_call` 并回写。等待者只等自己的 `id`，不再兼职服务回调。
- 影响面：`plugin_process.rs` 结构性改动（协议与 SDK 不变）；`plugin_protocol.rs` 分类逻辑可复用。
- 风险：中。需要重跑 `t29`（热更新 5 轮 ×2 + 自启 10/10）、`t30` 验收用例，确认回调顺序与 `deactivate` 时不丢回调。
- 收益：消灭「宿主调用超时」这一类误报（本次实测 3 条全部属此类）。
- 验收：冷启 20 轮（含 Defender 全盘扫描并行跑一轮），`gateway-error.log` 与 `plugin-failures.log` 均 0 新增。

**A3（P0，宿主）把重活移出 `CURRENT` 锁与启动关键路径**

- 位置：`plugin_runtime.rs:152-185`（`resume`）、`plugin_process.rs:25`（`verify`）、`plugin_install.rs:74-86`
- 改法三选一，建议同时做前两条：
  1. `verify()` 的全文哈希结果按 `(id, version, digest)` 缓存到 `host/plugin-verified/<id>-<version>-<digest>.json`，内容为每文件的 `size + mtime`；命中缓存时只比对 `size + mtime`，不读内容。首次（无缓存）仍全文哈希。
  2. `resume()` 先 `manager.list()` 取出快照 → **释放锁** → 在锁外做 `recover/cleanup/verify` → 再逐插件短持锁登记进程。
  3. `cleanup()` 与 `cleanup_staging()` 挪到启动完成后 30 s 的维护线程（它们本就「永不失败、失败了下次再试」，晚做没有任何副作用）。
- 影响面：`plugin_runtime.rs`、`plugin_install.rs`、新增一个校验缓存目录（不参与配置备份，与 `plugin-failures.log` 同级策略）。
- 风险：中低。校验强度从「每次全文哈希」降为「首次全文 + 常态 size/mtime」，等于 git 的 index 策略；篡改仍会被「首次」或「size/mtime 变化」发现。
- 收益：每次 Dock 启动省 129.9 MB 哈希（实测 ≈0.45 s 热态、数秒冷态），并把哈希彻底移出插件回调的饥饿窗口。
- 验收：冷启 50 轮 0 失败（对应立项 §6 H1）；`host/plugin-verified/` 命中率 ≥ 95%。

**A4（P1，宿主）冷/热分档的超时预算 + 失败重试一次**

- 位置：`config.rs:112-116`、`plugin_process.rs:47`、`:49`、`sdk/node/plugin.mjs:6`
- 改法：
  - 用已有的 `logon::since_logon()`（`logon.rs:446`，目前只用于写日志 `main.rs:379-380`）判断「登录后 5 分钟内」→ 本次 `plugin_activate_secs` 临时取 20 s、`plugin_rpc_secs` 取 30 s；
  - 宿主在 `host.hello` 的 params 里下发 `hostCallTimeoutMs`，SDK 按它设置 `requestTimeout`（默认仍是 5000，冷启时给 20000）；
  - `resume()` 对单个插件失败**退避重试一次**（500 ms），仍失败才记录 failure。
- 风险：低。副作用是冷启时单个插件的失败暴露会晚 20 s，但换来自愈。
- 验收：冷启 20 轮里至少有 1 轮人为注入 3 s 延迟，仍能拉起。

**A5（P2，宿主）Node 探测死线放宽并缓存**

- 位置：`plugin_dependencies.rs:32`（`from_secs(2)`）
- 改法：首次 8 s、成功后缓存到本次进程生命周期（`OnceLock`），后续 `view()`／安装不再重复 spawn。
- 现状副作用：`plugin_api.rs:32` 的 `discovery(context)` 在**每次** `extension_view` 都会再探测一次 Node（每次都 spawn 一个 `node.exe --version`）。
- 风险：低。验收：设置页连续打开 10 次，`node.exe` 启动次数 ≤ 1。

**A6（P1，插件仓）Magpie/Clash 自启分支不要与握手抢时间**

- 位置：`plugins/magpie/backend.mjs:36`、`plugins/clash/backend.mjs:29`
- 现状：Magpie 已经是 `void launchGateway()`（正确）；Clash 走 `void service.view().then(...)`，但 `activate` 里 `await service.view()`（`backend.mjs:25`）本身要读配置，冷启时若撞上 5 s 预算会直接把 `activate` 判死。
- 改法：Clash 的 `activate` 只做同步可完成的初始化，`view()` 延后到 `setImmediate`；`activate` 内不做任何等待磁盘的 `await`。
- 风险：低。验收：`activate` 端到端 ≤ 300 ms。

## 4. 症状 B：每次更新后首次启动特别慢

### 4.1 判据与实测

`runtime.rs:352-384`（`prepare_at`）：

```rust
let (m, digest) = read_manifest(resources)?;              // digest = manifest.json 自身的 SHA-256
let first = remembered(home).is_none_or(|(was, where_)| was != digest || where_ != normalized(resources));
if first { verify_all(resources, &files)?; } else { check_required(resources, &files)?; }
...
if first {
    grant(&resources.join("webview2"))...;                // icacls /T 递归重置 ACL
    remember(home, &digest, resources)?;
}
```

- `remembered()` 记的是 **`manifest.json` 的摘要**（`runtime.rs:230-264`），而 `manifest.json` 含 `version` 与整份文件清单，**每次更新必然变化**。
- 于是每次更新后的第一次启动都走 `first == true` 分支：
  - `verify_all()`（`:145-152`）逐文件比对 **大小 + SHA-256**，覆盖 10,513 个文件 / 1.30 GB；
  - `webview_acl_arguments()`（`:288`）带 `/T`，对 `resources\webview2` **整棵树递归**重置 ACL（256 文件 / 705 MB）。
- 第二次启动起，判据变成「14 个必需文件的大小比对」（`check_required`，`:155-165`，`REQUIRED` 见 `:14-29`）——毫秒级。

这就是「更新后首启慢、之后正常」的全部机制：**它不是缓存没热，而是每次更新都把「一次性全量校验」重新触发了一遍。**

### 4.2 量化

| 项 | 数值 | 说明 |
| --- | --- | --- |
| 全量校验字节 | 1,392,894,194（1.30 GB） | `manifest.json` 实测汇总 |
| 全量校验文件数 | 10,513 | 其中 `windhawk` 8,413（编译器与注入器的小文件多） |
| 哈希吞吐（实测） | 290 MB/s | 纯哈希 ≈4.8 s；小文件多时 IOPS 主导，实测会高于此值 |
| 递归 ACL | 705 MB / 256 文件 | `icacls /T` 逐文件写 DACL，冷态通常 1–5 s |
| 冷启放大因子 | Defender 首次扫描 + 冷页缓存 | 更新后这些文件全是「新写入、未被扫描过」，实际耗时可达上述值的数倍 |
| 日常启动 | 14 个文件的 `stat` | 毫秒级，符合「之后恢复正常」 |

另外两处叠加项（不是主因，但同向）：

- `main.rs:1695` 的 `runtime::prepare()` 在**窗口创建之前**同步执行，所以这段时间用户看到的是「点了没反应」，而不是「界面先出来、后台在忙」。
- `updates::wait_after_update()`（`main.rs:1676`）在更新后首启时还要等旧进程交接，进一步拉长首帧时间。

### 4.3 影响面

| 维度 | 影响 |
| --- | --- |
| 触发条件 | 每次安装/更新之后的第一启动（`manifest.json` 变了就一定触发） |
| 波及 | 全部用户；机械硬盘与低配机器上最明显 |
| 时间窗 | 与「开机自启」重叠时更糟：自启 + 全量校验 + 插件启动三件事抢同一块磁盘，直接造成症状 A 的超时 |
| 数据风险 | 无（纯读 + ACL 写） |
| 权限风险 | 若安装目录 ACL 不是当前用户可写，`icacls /T` 失败会让首启**直接报错退出**（`runtime.rs:375-377` 的报错路径） |

### 4.4 修复方案

**B1（P0，宿主）把「是否重校」的判据从「manifest 摘要」换成「文件指纹集合」**

- 位置：`runtime.rs:230-264`（`remembered/remember`）、`:361`
- 改法：`state/verified` 存 `{ schema, resourcesPath, files: {path: [size, mtime, sha256]} }`（或按目录分片，避免单文件过大）。判据改为「必需文件 + 关键大文件（node/webview2 的 exe、dll）的 size+mtime 未变」。变化时才对这些文件重算哈希。
- 影响面：`runtime.rs` 一个模块 + `state/verified` 兼容（旧格式按「缺失」处理，等价于升一次级）。
- 风险：中低。校验强度略降（同 A3 的取舍），但比「只在安装时记一次」强。
- 收益：更新后首启从「1.30 GB 全量」降到「变化的文件」——安装器本次只替换 1 个文件（见 `install.log`），即实际开销接近 0。

**B2（P0，宿主）全量校验移出启动关键路径**

- 位置：`main.rs:1695`
- 改法：`runtime::prepare()` 拆成「必需文件 quick check（同步，毫秒级，失败即弹错）」+「全量校验（后台，失败则弹错并退出）」。窗口与插件启动不必等全量校验结束。
- 风险：低。注意 `WEBVIEW2_*` 环境变量必须在创建窗口前设置（`runtime.rs:402-403`），这部分保持同步。
- 收益：更新后首启的「点了没反应」消失。

**B3（P1，宿主）ACL 只在实际需要时执行**

- 位置：`runtime.rs:291-301`、`:371-377`
- 改法：不做「首启必递归授权」，改为「先尝试创建 webview 环境；失败且错误指向沙箱读取时，才对 `webview2` 授权并重试一次」；或把 `/T` 限制到 `webview2\*.exe`、`*.dll` 两级而非全树。
- 风险：低-中。需要覆盖 `lhm_setup` 那类 DACL 场景的回归（本机已有三条 known-fail，见立项 §5）。
- 收益：省掉 256 文件 / 705 MB 的递归 DACL 写入。

**B4（P1，主仓安装器）安装时直接产出校验结果**

- 位置：`app/scripts/install-update.cjs`、`app/scripts/installer.mjs`
- 改法：安装器写完文件后顺手写 `state/verified`（它本来就在算每份资源的 SHA-256 并写 `.sha256` 资产）。宿主首启直接信任并只做抽样。
- 风险：中（安装器与宿主两侧格式要一次性对齐，需 `installer.test.mjs`／`install-update.test.mjs` 扩充）。
- 收益：更新后首启的全量校验彻底归零，且比宿主自算更可靠（安装器知道自己写了哪些文件）。

## 5. 症状 C：新装 Magpie 后「美化」图标异常 + 插件报错 → 过一会儿自愈

### 5.1 「美化」在这套代码里具体指什么（先对齐口径）

`page-checkin` 页面顶部那颗开关就是它，代码与文案都在仓内：

- `page-plugins/page-checkin/README.md`「## 预装插件美化」：**「管的是整个 magpie 窗口里的厂商图标：插件页、供应商页、侧边栏、路由与用量卡片，凡是预装插件注册的 provider 都换成 FISH Dock 自己那套，默认开启。」**
- `page/view.js:36-37`：`lookTitle: "预装插件美化"`、`lookNote: "插件、供应商、侧边栏里的图标都换成这一套；关掉就回到原版。"`
- 实现：素材编在 magpie 的 exe 里（`internal/provider/brandicons/`），开关只是 magpie 数据目录下 `icons/brand/on` 一个标记文件；页面通过 `brand:status` / `brand:set` 读写（`page/app.js:141-151`）。

> 口径判别：若用户看到异常的是「设置页 → 外观 → Windows 美化（Windhawk）」那一块，则另一条链（`windhawk_import.rs:122` 与插件操作共用 `storage_context::EDIT`，见 `plugin_install.rs:36-47` 的锁序声明）也有可能，但本报告未取得该面板的失败日志，**不下结论**。本节的结论限定在「预装插件美化」。

### 5.2 为什么「新装之后」必然有一段时间不对

两条独立的时间差，都在 `magpie` 的 `runtime.start()` 里：

```text
runtime.mjs:119-125  同步内置页面目录 → <magpie data>/plugins/page-checkin、page-relay-probe
                     （page-seed.mjs:53-71：先整目录 cp 到 .tmp，再 rename 换名）
runtime.mjs:129      seedPreinstalled() → preinstall.mjs:12-33 只往 plugins.json 写 13 条 spec
                     ★ 不下载、不安装 —— 注释原文：「首次启动由 magpie 自己装，失败下次再试」
```

- **图标侧**：美化按「预装插件注册的 provider」匹配。13 个供应商插件（raccoon / autoclaw / cline / codearts / kiro / loomy / minimax / zen-free / qoder / trae / workbuddy / mimo / zcode）在首次启动时**只是登记了包名**，真正的 npm 包由 magpie 自己的更新循环去装。装完之前，这些 provider 不存在 → 没有图标可换 → 表现为「美化图标加载异常」。
- **页面侧**：`syncPageDirectory` 是「整目录换名发布」。新装那一刻 `<magpie data>/plugins/page-checkin` 还不存在，页面与 `logos/` 也都取不到；`app.js:124` 的 `img.src = logos/<id>.svg` 全部 404，`app.js:16-25` 的读失败分支会把顶部提示切成「读取签到状态失败 / 这个页面需要 magpie 1.1 或更新版本」。
- **自愈**：npm 那批包装完（依赖网络，几十秒到几分钟）→ provider 出现 → 图标与页面同时恢复。这与「过一会就会自动恢复」严格一致，也解释了为什么它只在**新装**时明显、老用户升级时基本看不到（包已在盘上）。

另有一个次要放大项：`runtime.mjs` 的 `syncPageDirectory` 每次网关启动都会算**两次整目录摘要**（`page-seed.mjs:12-35`、`:57-58`），内容一致则不动盘；但如果上一次进程被杀在「发布中途」，`plugins/page-checkin` 会短暂缺失，直到本次重新发布完成。

### 5.2.1 「插件报错」这一半：页面入口与页面 RPC 都在全局锁里

安装/启动的整段重活都持 `CURRENT`（`plugin_api.rs:71` → `plugin_runtime.rs:212-230`），而插件页面的两条入口也要求同一把锁：

| 位置 | 行为 | 后果 |
| --- | --- | --- |
| `plugin_page.rs:20` | `open()` 在发票据前调 `manager.verify(&row)`——**又是全文 SHA-256**（magpie 75.9 MB） | 新装完点开页面，先卡一次全文哈希 |
| `plugin_page.rs:46` | `call()` 取 `CURRENT.lock()` 后直接 `runner.call(...)`，**持全局锁跑最长 15 s 的 RPC**（`config.rs:116`） | 安装窗口内任何页面调用都在锁后排队 |
| `sdk/browser/bridge.js:37` | 页面侧每次调用 `setTimeout(..., 20000)` | 排队超过 20 s 就报「插件页面请求超时」 |
| `plugin_page.rs:58-71`（`resource`） | **每个页面资源**（图标、css、js 都算）都要 `checked()` → `require_plugin` → `github_access::require`（`github_access.rs:219` 取 `EDIT.lock()`，凭据过期时还会走网络）→ 再对该资源算一次 SHA-256（`:66`） | 一次页面加载会触发多次「全局锁 + 哈希」，图标尤其多；安装期 `EDIT` 被 `plugin_install` 持有，页面资源请求会直接卡住或 403 |

所以「新装 Magpie → 图标不对 + 插件报错 → 过一会儿自己好」里的**报错**，主要不是图标子系统出问题，而是**安装这段持锁重活把页面 RPC 堵到了 20 s 之外**；等安装结束、页面 60 s 轮询（`app.js:267`）或用户点重试时，一切又正常了。

此外，`portable-runtime.mjs:69-87` 在每次 Magpie 服务启动时对约 70 MB 的运行副本做「源摘要 → 复制 → 副本摘要 → 再校验」多轮全文哈希，这是「几十秒」这个量级的另一个来源。

### 5.2.2 一个未证实的次要假设：`brandCache.kept` 只在进程内存里

`magpie-v0.1.2336+fish.7.patch:1363` 起，`brandCache` 记住每个 id 落成哪个 `file:` 名；`:1407` 的 `brandIconNames()` 与 `:3231-3234` 的 `pruneIcons` keep 集合**只来自进程内缓存**。理论上进程刚起来、缓存为空时的一次 `pruneIcons` 会删掉上一轮落盘的品牌图标文件，导致 `<img>` 404。

这条**按「推测」处理**：上游 `internal/provider/icon.go` 全文与 `pruneIcons` 的实际调用节奏不在本仓（补丁只含 `@@ -290,6 +290,11` 一段），无法证明它何时跑。要落定需要：上游该文件全文 + `<magpie data>/icons/` 目录快照 + magpie 侧日志。

### 5.3 影响面

| 维度 | 影响 |
| --- | --- |
| 触发条件 | 首次安装 magpie 插件、或清空过 `.fishplugin-magpie`、或换机迁移后首启 |
| 持续时间 | 取决于 npm 拉取 13 个包的耗时（网络差时可达数分钟） |
| 用户可见 | 美化图标空白/回退首字母色块、签到页报「读取失败」、插件卡短暂报错 |
| 数据风险 | 无（只影响呈现） |
| 隐蔽性 | **高**：所有失败都被吞掉（`backend.mjs:23` `.catch(...)` 只记日志，`preinstall` 段 `try/catch` 只跳过），用户只能看到「有点不对」 |

### 5.4 修复方案

**C1（P0，插件仓）把「正在准备」变成可见状态，而不是静默失败**

- 位置：`plugins/magpie/runtime.mjs:126-129`、`plugins/magpie/preinstall.mjs`、`page-plugins/page-checkin/page/app.js:145-151`
- 改法：`preinstall.mjs` 把登记结果与「已装/待装」计数写进 magpie 数据目录的一个小 JSON（如 `config/preinstall-state.json`）；`brand:status` 的返回里带 `pendingPlugins: n`；页面在 `look` 行显示「正在准备 N 个供应商插件（首次安装，约 1 分钟）」。
- 风险：低（只加字段与文案）。
- 验收：清空 `.fishplugin-magpie` 后首启，页面在 3 s 内出现「正在准备」提示，装完后提示消失、图标齐全。

**C2（P1，插件仓）页面读失败不要误导成「版本太低」**

- 位置：`page/app.js:16-25`
- 现状：任何读失败都归到 `needsHost`（「这个页面需要 magpie 1.1 或更新版本」），包括「宿主还在启动」这种临时态。
- 改法：区分 `NEED_HOST`（`magpie.read` 不存在）与 `HOST_BUSY`（抛错但有 `magpie` 对象）→ 后者显示「服务正在启动，稍后自动重试」，并按 1/2/4 s 退避重试 3 次。
- 风险：低。

**C3（P1，主仓）安装完成后的 60 s 内不重复弹错**

- 位置：`extension-panel.js` 的 `error` 渲染、`plugin_runtime.rs:95` 的 `error` 字段
- 改法：插件刚安装/刚启动的 60 s 内，把「正在启动」与「启动失败」分开呈现，失败也先挂在插件卡的一行提示上，不弹窗、不置红。
- 风险：低。

**C4（P0，主仓）插件页面入口与 RPC 不得持全局锁，也不得每次全文校验**

- 位置：`plugin_page.rs:20`（`open()` 里的 `manager.verify`）、`plugin_page.rs:46`（`call()` 持 `CURRENT` 跑 RPC）
- 改法：
  - `open()` 的 `verify` 改为「复用 A3 的校验缓存」，无缓存时才全文哈希，且放到锁外；
  - `call()` 先按 ticket 拿到该插件的进程句柄（`Arc<Mutex<Process>>` 或从 `Runner` 里 clone 出的通道句柄），**释放 `CURRENT` 后**再做 RPC。
- 影响面：`plugin_page.rs`、`plugin_runtime.rs`（需要暴露按 id 取句柄的能力）。
- 风险：中。需要确认「插件被卸载/停用」与「正在 RPC」的竞态（现在靠全局锁天然规避，改后要用 `revoke` + 句柄失效处理）。
- 收益：安装/启动窗口内页面不再报「插件页面请求超时」（`bridge.js:37` 的 20 s 判据）。

## 6. 症状 D：签到页不实时采集，像固定模板

### 6.1 先纠正一个前提

页面**没有**硬编码签到服务清单：`page/app.js:18` 每次 `await magpie.read("checkin:state", {})`，`:267` 还每 60 s 轮询；厂商分组来自 `state.providers`（`view.js:96`），账号来自 `state.accounts`。`app.js:16-25` 的失败分支**不回落任何内置数组**。

真正让整屏长期不变的是三件事叠加。

### 6.2 成因一：读接口按设计只读台账，不采集

`patches/magpie-v0.1.2336+fish.8.patch`：

- `:3289` 原注释：**「只读，不发起任何签到。」**
- `:3317-3318`：`today := checkinTodayEntries(now)`——取已落盘结果；
- `:3380-3388`：快照源是**五个固定文件**：

  ```go
  for _, path := range []string{wbCheckinPath(), traeCheckinPath(), miniMaxCheckinPath(), qoderCheckinPath(), pluginCheckinPath()} {
  ```

- 真正的采集只在 `checkin:run`（`:1167` → `provider.CheckInPlugins`）与 magpie 自身的每日定时里发生；
- `checkin:refresh`（`:1195-1201`）只调 `provider.RefreshUsage`——**只重读额度，不重查签到状态**。

于是：**页面上「今日」这一列，永远等于「上一次签到跑过之后写下的那条台账」**。刚装完、刚跨天、或当天还没跑过定时任务时，看起来就是「一屏不变的模板」。

### 6.3 成因二：五条固定落盘文件的覆盖面有限

`checkinTodayEntries` 只认 workbuddy / trae-cn / minimax-code / qoder / 插件台账这五路。其余 provider（`preinstall.mjs:12-26` 里那 13 家里的大多数，以及 `logos/README.md` 提到的 kiro、cline、joycode、mimo、zen-free 等）即便有账号，也不在这五个文件的覆盖里 → 永远停在 `pending`（「今日尚未签到」）。

### 6.4 成因三：两处硬编码额度补偿把「未知」填成了固定数字

| 位置 | 原文 | 影响 |
| --- | --- | --- |
| `page-plugins/page-checkin/page/view.js:44` | `const DEFAULT_DAILY = {codearts: 100, raccoon: 100};` | 厂商没报数额时，页面上直接写 `+100` |
| `page/view.js:103-106` | `if (shown === null && (line.key === "claimed" \|\| line.key === "done")) { const daily = DEFAULT_DAILY[vendor.id]; ... }` | 补偿生效点 |
| patch `:3621-3635` | `loomyCheckinCredit = 400` / `func loomyCredit(id string, credit float64) float64 { if loomyCheckin(id) && credit <= 0 { return loomyCheckinCredit } ... }` | 讯飞历史记录里 Credit 为 0 时，读取侧一律改写成 **400** |
| patch `:3352` | `r.Credit = loomyCredit(vendor, r.Credit)` | 上游补偿的注入点 |
| patch `:3471-3481` | `CreditEstimates` 读 `credits-daily.json` 做「额度回落推算」 | 页面上的「≈ +N」是推算值，不是厂商回报 |

再叠加页面 60 s 轮询（`app.js:267`）——读是实时的，但内容不会变，观感就是「固定模板」。

### 6.5 影响面

| 维度 | 影响 |
| --- | --- |
| 触发条件 | 常驻；所有未跑过当日签到的账号、所有不在五路快照里的 provider |
| 数据正确性 | **有误导风险**：`+400` / `+100` 是硬编码假设，厂商口径一变就变成错数 |
| 波及 | `page-checkin` 页面、`fish.magpie` 的 `checkin:state` 读、magpie 补丁里的 `internal/provider/checkin_view.go` |
| 回归面 | 插件仓 `test/view.test.mjs`（页面侧纯函数）、magpie 补丁的 Go 侧测试（需在 magpie 仓跑） |

### 6.6 修复方案

**D1（P0，插件仓）删掉两处硬编码额度**

- 位置：`page/view.js:44`、`:103-106`；patch `:3352`、`:3621-3635`
- 改法：页面不再自带 `DEFAULT_DAILY`，改吃宿主回的 `dailyCredits[accountKey]`；宿主把 `loomyCredit` 的 400 补偿改成**原值透传 + `outcome:"claimed", credit:0`**，由页面显示「未告知」。如需保留历史观感，改为一次性数据迁移（把台账里的 0 补成厂商口径），而不是每次读取都改。
- 风险：中（历史账号会从「+400」变成「未告知」，需要一条 release note）。
- 验收：`view.test.mjs` 增两例——「无 dailyCredits 时显示未告知」「有 dailyCredits 时显示原值」。

**D2（P0，插件仓）给读接口加「数据截至」语义**

- 位置：patch `CheckinViewStateAt`（`:3290`）增 `RefreshedAt time.Time` 与 `Source string`（`ledger` / `live`）；`page/app.js:156-173` 的表头显示「数据截至 HH:MM · 台账」。
- 收益：把「不实时」从暗坑变成显式信息，用户不会再误以为是模板。
- 风险：低。

**D3（P1，插件仓）provider 增加「能不能采」的标记**

- 位置：`CheckinProviderState`（`:3246-3250`）增 `Collector string`（`ledger` / `plugin` / `none`）；`CheckinViewStateAt` 里按「是否在五路快照里 / 是否有插件声明签到」填值；页面（`view.js:80-91`）对 `none` 显示「不支持自动采集」而不是「今日尚未签到」。
- 风险：低-中（Go 结构变更，需同步补丁与页面两端）。
- 验收：kiro / cline 一类无采集的 provider 显示「不支持自动采集」。

**D4（P1，插件仓）修准「刷新今日积分」的语义**

- 位置：`page/app.js:252-264`、patch `:1195-1201`
- 二选一：文案改成「重新读取额度」（与 `RefreshUsage` 一致，风险极低）；或在补丁里加一条**只读**探测（需厂商插件提供 status-only 接口——上游目前只有会真签到的 `CheckInPlugins`，误用会重复扣额度，**不推荐**）。
- 建议取前者。

**D5（P2，插件仓）清掉死兜底并修文档**

- 位置：`page/app.js:33-49`（`fetch` 兜底在 CSP `connect-src 'none'` 下永远失败，报错还会误导成网络错误）、`page/README.md:13`（`{providers, accounts, today, credits, running}` → 实际是 `{providers, accounts, today, credits, creditEstimate}`，`running` 字段在 Go 结构里不存在）。
- 风险：低。

## 7. 横切根因与修复优先级

### 7.1 五条横切问题

| 编号 | 问题 | 代码位置 | 关联症状 |
| --- | --- | --- | --- |
| X1 | 插件 → 宿主的回调只在「等这个插件响应」的窗口里被服务 | `plugin_process.rs:56-75`、`:92-109` | A（直接成因） |
| X2 | 全局锁内做重活（全文哈希、目录清理、事务恢复） | `plugin_runtime.rs:152-185`、`plugin_process.rs:25`、`plugin_install.rs:74-86` | A、B |
| X3 | 超时预算按热态设定（5 s / 5 s / 2 s / 2 s），冷启无外挂 | `config.rs:112-116`、`plugin_process.rs:14-15`、`sdk/node/plugin.mjs:6`、`plugin_dependencies.rs:32` | A、C |
| X4 | 「一次性全量校验」的判据绑定在会变化的清单摘要上 | `runtime.rs:361` | B |
| X5 | 插件页面入口与页面 RPC 也持 `CURRENT`，开页前再全文校验一次，每个页面资源还要过一次 `EDIT` 锁 + 哈希 | `plugin_page.rs:20`、`:46`、`:58-71`、`sdk/browser/bridge.js:37` | C |

### 7.2 优先级矩阵

| 优先级 | 任务 | 归属 | 影响面 | 风险 | 收益 |
| --- | --- | --- | --- | --- | --- |
| **P0** | A1 探针超时不杀进程 | 主仓 | `plugin_runtime.rs`、前端插件卡 | 低 | 止损：不再误杀活着的插件与内核 |
| **P0** | A2 回调解耦 | 主仓 | `plugin_process.rs`（结构） | 中 | 消灭「宿主调用超时」 |
| **P0** | A3 重活移出锁 + 校验缓存 | 主仓 | `plugin_runtime.rs`、`plugin_install.rs` | 中低 | 每次启动省 129.9 MB 哈希 |
| **P0** | B1 校验判据改文件指纹 | 主仓 | `runtime.rs` | 中低 | 更新后首启从 1.30 GB 降到变化量 |
| **P0** | D1 删硬编码额度 | 插件仓 | 页面 + magpie 补丁 | 中 | 数据不再造假 |
| **P0** | C4 页面入口/RPC 不持全局锁 | 主仓 | `plugin_page.rs`、`plugin_runtime.rs` | 中 | 安装窗口内页面不再报超时 |
| **P1** | A4 冷热分档 + 重试一次 | 主仓 | `config.rs`、`plugin_process.rs`、SDK | 低 | 冷启自愈 |
| **P1** | B2 全量校验移出关键路径 | 主仓 | `main.rs:1695` | 低 | 首帧不再「点了没反应」 |
| **P1** | B3 ACL 按需 | 主仓 | `runtime.rs` | 低-中 | 省 705 MB 递归 DACL |
| **P1** | C1/C2 准备态可见 | 插件仓 | `preinstall.mjs`、`app.js` | 低 | 新装期的「莫名报错」变成可读提示 |
| **P1** | D2/D3 数据截至 + 可采标记 | 插件仓 | magpie 补丁 + 页面 | 低-中 | 「不实时」变显式 |
| **P2** | A5 Node 探测放宽并缓存 | 主仓 | `plugin_dependencies.rs` | 低 | 少 spawn 进程 |
| **P2** | A6 Clash activate 去阻塞 | 插件仓 | `clash/backend.mjs` | 低 | 握手更稳 |
| **P2** | B4 安装器产出校验结果 | 主仓 | `installer.mjs`、`install-update.cjs` | 中 | 全量校验彻底归零 |
| **P2** | D4/D5 文案与死代码 | 插件仓 | 页面 + README | 低 | 减少误导 |

### 7.3 建议落地顺序

1. **A1 + A3 + B1**（只改宿主、不改协议、不改插件仓）：一次性把「误杀」「锁内重活」「重复全量校验」三件事按下去，可独立出 2.0.3 补丁版。
2. **A2**（结构性）：与 2.1.0 的插件协议工作一起排，需要重跑 `t29`／`t30`／G1 全组。
3. **D1 + D2**（插件仓）：可独立出 `fish.magpie 2.1.3`，但 `D1` 需要一条 release note 说明「+400/+100 改为未告知」。
4. **C1/C2、D3**：跟随下一次 magpie 页面版本。
5. **B2/B3/B4**：与安装器改动一起走 2.1.0（安装器与宿主格式要对齐，不宜单独放补丁版）。

## 8. 探测方法（可复跑）

以下探测不修改任何产品文件，只读日志与文件属性，可在用户机上直接跑。

### 8.1 一次性取证脚本

```powershell
# 保存为 probe-plugin-startup.ps1，按实际路径改两个变量
$data = "$env:USERPROFILE\.fish-dock"
$app  = "$env:LOCALAPPDATA\Programs\FISH Dock"

"== 1. 插件失败台账（最近 20 条） =="
Get-Content "$data\host\plugin-failures.log" -Tail 20 -ErrorAction SilentlyContinue

"== 2. 插件 stderr（应非 0；0 表示失败在协议层，不在插件内部） =="
Get-ChildItem "$data\host\plugin-logs" -ErrorAction SilentlyContinue |
  Select-Object Name, Length, LastWriteTime | Format-Table -AutoSize

"== 3. Magpie 网关失败台账（宿主调用超时=SDK 5 s 超时） =="
Get-Content "$data\.fishplugin-magpie\gateway-error.log" -ErrorAction SilentlyContinue

"== 4. Clash 运行态与系统代理（看是否被误杀后失配） =="
Get-Content "$data\.fishplugin-clash\config\run-state.json" -ErrorAction SilentlyContinue
Get-Content "$data\.fishplugin-clash\config\system-proxy.json" -ErrorAction SilentlyContinue

"== 5. 安装资源规模与校验标记（症状 B 的两个输入） =="
$m = Get-Content "$app\resources\manifest.json" -Raw | ConvertFrom-Json
"version=$($m.version) files=$($m.files.Count) bytes=$((($m.files|Measure-Object bytes -Sum).Sum))"
"verified marker:"
Get-Content "$env:LOCALAPPDATA\FISH Dock\state\verified" -ErrorAction SilentlyContinue
"manifest.json sha256 (marker 第一行应与它相同)："
(Get-FileHash "$app\resources\manifest.json" -Algorithm SHA256).Hash.ToLower()

"== 6. 插件包规模（每次启动的哈希量） =="
foreach ($id in @('fish.magpie','fish.clash')) {
  $d = "$data\host\packages\$id"
  if (Test-Path $d) { Get-ChildItem $d -Directory | ForEach-Object {
    $s = Get-ChildItem $_.FullName -Recurse -File | Measure-Object Length -Sum
    "{0} {1}: files={2} bytes={3}" -f $id, $_.Name, $s.Count, $s.Sum } }
}

"== 7. Node 探测耗时（对比宿主 2 s 死线） =="
$node = "$app\resources\node\node.exe"
1..3 | ForEach-Object { (Measure-Command { $null = & $node --version }).TotalMilliseconds }
```

判读口径：

| 现象 | 判读 |
| --- | --- |
| `plugin-logs\*.log` 全 0 字节，但 `plugin-failures.log` 有记录 | 失败在协议层（超时/握手），不是插件自己崩 → 指 X1/X2/X3 |
| `gateway-error.log` 出现 `宿主调用超时` | 插件 → 宿主回调被饿死 → 指 X1（`sdk/node/plugin.mjs:34` 唯一产出点） |
| `plugin-failures.log` 的 `exitCode` 为 `null` | 进程还活着却被判「未运行」→ 指 A1 的误杀路径 |
| `state/verified` 第一行 ≠ 当前 `manifest.json` 的 sha256 | 下次启动会走全量校验 → 指 B1 |
| `node --version` 单次 > 500 ms | 该机器冷态下 2 s 死线没有余量 → 指 A5 |

### 8.2 冷启压测（对应立项 §6 G1 的加严版）

在现有 `scripts/run-g1-stress.ps1` 基础上补两组判据：

| 组 | 场景 | 新增判据 |
| --- | --- | --- |
| H6 | 更新后首启（先装上一版，再覆盖升级） | 首帧可视 ≤ 3 s；全量校验在后台完成；`state/verified` 在 10 s 内更新 |
| H7 | 探针宽容 | 人为把插件 `activate` 延迟 3 s，反复刷设置页 20 次：进程 pid 不变、`plugin-failures.log` 0 新增 |

### 8.3 验收总口径

| 症状 | 修复后应满足 |
| --- | --- |
| A | 冷启 50 轮 0 失败；`plugin-failures.log` 与 `gateway-error.log` 0 新增；无孤儿 `magpie.exe` / `mihomo.exe` |
| B | 更新后首启到首帧 ≤ 3 s；第二次启动与第一次启动耗时差 ≤ 200 ms；`state/verified` 记录文件级指纹 |
| C | 清空 `.fishplugin-magpie` 后首启，页面 3 s 内显示「正在准备 N 个供应商插件」，装完自动消失 |
| D | 「今日积分」不再出现 `+400` / `+100` 这类无来源数字；无采集能力的 provider 显示「不支持自动采集」；表头显示「数据截至 HH:MM」 |

## 9. 证据索引

### 9.1 宿主侧（`FISH-Dock/app/src-tauri/src/`）

| 事实 | 位置 |
| --- | --- |
| 插件自启在 setup 里 spawn 线程，早于窗口、早于 `proxy::set_web` | `main.rs:1972`（`plugin_api::resume()`）、`main.rs:1974`（`proxy::set_web`） |
| `resume()` 全程持 `CURRENT` 锁，内含 recover/cleanup/cleanup_staging/reclaim | `plugin_runtime.rs:152-185`、`:46` |
| 启动失败只记一行日志，无重试 | `plugin_runtime.rs:179-183`、`plugin_api.rs:160` |
| `view()` 在探针超时时 `remove` 进程（触发 `Drop` → `kill`） | `plugin_runtime.rs:77-84`、`plugin_process.rs:116` |
| 探针 2 s / 缓存 2 s | `plugin_process.rs:14-15`、`:85-89`、`:92-109` |
| 拉进程前先全文 SHA-256 | `plugin_process.rs:25`、`plugin_install.rs:74-86` |
| 握手与激活各 5 s | `plugin_process.rs:47`、`:49`、`config.rs:114`、`:350` |
| 插件 → 宿主回调只在等自己响应时被服务 | `plugin_process.rs:56-75`、`:92-109` |
| Node 探测死线 2 s；`view()` 每次都重新探测 | `plugin_dependencies.rs:27-34`、`plugin_api.rs:32` |
| 锁序声明 `CURRENT → EDIT_REGISTRY → EDIT` | `plugin_install.rs:36-47` |
| 更新后首启全量校验 + 递归 ACL + 记 manifest 摘要 | `runtime.rs:352-384`、`:145-152`、`:230-264`、`:288`、`:291-301` |
| 校验在窗口创建前同步执行 | `main.rs:1695` |
| 登录后时长只有写日志，没有用于任何调度 | `logon.rs:446`、`main.rs:379-380` |
| SDK 宿主调用 5 s 超时 | `FISH-Dock/sdk/node/plugin.mjs:6`、`:29-39` |
| 开页前全文校验、页面 RPC 持全局锁 | `plugin_page.rs:20`、`:46` |
| 页面调用 20 s 判超时 | `FISH-Dock/sdk/browser/bridge.js:37` |

### 9.2 插件侧（`FISH-Dock-Plugins/`）

| 事实 | 位置 |
| --- | --- |
| Magpie 自启路径：`void launchGateway()` + 3 次退避 | `plugins/magpie/backend.mjs:19-26`、`:36` |
| 启动前要读 `keys/web-key`（宿主调用） | `plugins/magpie/runtime.mjs:93-94` |
| 每次启动对 70 MB 自身副本做一次 SHA-256 | `plugins/magpie/portable-runtime.mjs:69` |
| 页面目录整目录同步（算两次全目录摘要） | `plugins/magpie/runtime.mjs:119-125`、`page-seed.mjs:53-71` |
| 13 家供应商插件只登记 spec，靠 magpie 自己装 | `plugins/magpie/preinstall.mjs:12-33`、`runtime.mjs:126-129` |
| 美化 = 预装插件注册的 provider 换图标，开关是 `icons/brand/on` | `page-plugins/page-checkin/README.md`「预装插件美化」、`page/app.js:141-151` |
| 签到页唯一数据入口 `checkin:state`，60 s 轮询 | `page/app.js:18`、`:267` |
| 硬编码额度 100 | `page/view.js:44`、`:103-106` |
| `checkin:state` 只读不采集；快照只扫 5 个文件 | patch `:3289`、`:3317-3318`、`:3380-3388` |
| 硬编码额度 400 | patch `:3621-3635`、`:3352` |
| `checkin:refresh` 只刷额度 | patch `:1195-1201` |

### 9.3 本机实测

| 事实 | 出处 |
| --- | --- |
| `manifest.json`：10,513 文件 / 1,392,894,194 字节 / version 2.0.2 | `%LOCALAPPDATA%\Programs\FISH Dock\resources\manifest.json` |
| 资源分布 windhawk 8,413 / webview2 256 / node 1,840 文件 | 同上目录实测 |
| SHA-256 705,180,320 字节 / 2.32 s = 290 MB/s | 本次实测 |
| 插件包 75,938,315 + 53,934,360 字节 | `%USERPROFILE%\.fish-dock\host\packages\*` |
| `node.exe` 91,380,224 字节；`--version` 17–33 ms | `resources\node\node.exe` |
| 失败台账两条、网关失败台账三条 | `host\plugin-failures.log`、`.fishplugin-magpie\gateway-error.log` |
| 插件 stderr 三个日志全 0 字节 | `host\plugin-logs\*` |
| 更新日志「替换 1 个文件，复用 10259 个，耗时 2.1 秒」 | `install.log` |

## 10. 未证实的假设（明确标注，不作为结论）

| 假设 | 缺什么证据 | 如何证伪 |
| --- | --- | --- |
| 症状 C 若指的是「设置页 → 外观 → Windows 美化（Windhawk）」而非「预装插件美化」 | 该面板在故障时刻的 command 返回与 `windhawk` 日志 | 复现时抓 `windhawk_*` 命令返回；或看 `windhawk-panel.js:84` 的 `role="status"` 文案 |
| 冷启失败的首要项是 Defender 实时扫描 | 未取到 `MpCmdRun`/事件日志 | 用 `Get-MpPreference` + 排除目录对照一轮冷启 |
| `view()` 误杀在故障时刻确实发生过 | 日志只有「无响应」记录，没有进程退出时间线 | 修 A1 后回归；或下次复现时记录插件 pid 前后对比 |
| 上游 magpie 的 `CheckinAccounts` / `shownCheckins` 是否有缓存 | 仓内无 `.go` 源码，只有 8 个 patch | 在 magpie 上游仓核对这两个函数 |
| `brandCache.kept` 为空时的 `pruneIcons` 会删掉落盘品牌图标（§5.2.2） | 上游 `internal/provider/icon.go` 全文与 `pruneIcons` 调用节奏不在本仓 | 取上游该文件全文 + `<magpie data>/icons/` 目录前后快照 + magpie 日志 |

## 11. 对应 issue

| 症状 | issue | 归属仓 |
| --- | --- | --- |
| A 冷启插件自启失败 | [#55](https://github.com/Fish-under-sea/FISH-Dock/issues/55) | 主仓 |
| B 更新后首启慢 | [#56](https://github.com/Fish-under-sea/FISH-Dock/issues/56) | 主仓 |
| C 新装 Magpie 后美化图标/页面暂不可用 | [#16](https://github.com/Fish-under-sea/FISH-Dock-Plugins/issues/16) | 插件仓 |
| D 签到页不实时采集 | [#15](https://github.com/Fish-under-sea/FISH-Dock-Plugins/issues/15) | 插件仓 |

本报告与这四条 issue 同源：issue 里是修复工单（现象、根因、改法、影响面、验收），本报告是完整诊断（含实测数据、探测脚本、逐条证据索引与未证实假设的清单）。