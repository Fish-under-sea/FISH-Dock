# FISH Dock 2.0.3 任务书（宿主侧，与双插件 2.1.3 同批）

> 结论先行：2.0.3 是**只装宿主侧修复**的补丁版，范围严格锁在「插件自启冷启失败」「更新后首启全量校验」两条链，外加插件页面入口的锁问题；**不含**任何插件页面视觉与签到数据改动。后者在插件仓 `2.1.3` 里走（`fish.magpie` + `fish.clash`）。
>
> 依据：《插件自启与冷启四项症状诊断报告》`docs/plugin-startup-diagnosis-2026-11-02.md`；主仓 issue [#55](https://github.com/Fish-under-sea/FISH-Dock/issues/55)、[#56](https://github.com/Fish-under-sea/FISH-Dock/issues/56)。
> 编制：2026-11-02。基线：Dock 2.0.1（`a6ea519` 之后）／2.0.2（`1997de0` 版本位）。目标：**Dock 2.0.3**，插件 **`fish.magpie 2.1.3` + `fish.clash 2.1.3`**（`fish.feishu` 保持 2.1.0 不动）。

## 1. 范围与边界

| 类别 | 纳入 2.0.3 | 说明 |
| --- | --- | --- |
| 插件运行态与超时 | ✅ T01–T05 | 冷启失败的直接成因 |
| 启动资源校验 | ✅ T06–T08 | 更新后首启慢的直接成因 |
| 插件页面入口 | ✅ T09 | 安装/启动窗口内页面报超时的直接成因 |
| 版本位与出包 | ✅ T10 | 四处一致 + 目录回填规则 |
| 插件页面视觉 / 下拉重做 | ❌ | 插件仓 #17（`fish.clash 2.1.3`） |
| 签到数据口径（400/100 硬编码） | ❌ | 插件仓 #15（`fish.magpie 2.1.3`） |
| 预装供应商插件可见性 | ❌ | 插件仓 #16（`fish.magpie 2.1.3`） |
| 安装器产出校验结果（B4） | ❌ 留 2.1.0 | 需要安装器与宿主格式一次性对齐，不放补丁版 |

红线：2.0.3 只含 T01–T10。任何功能改动（设置页版位、签名策略、UI 重排）退回 2.1.0。

## 2. 任务清单

规模口径沿用 2.0.2：S ≤ 半天、M ≤ 2 天、L > 2 天（单人）。

| # | 任务 | 改动面 | 验收 | 规模 | 依赖 |
| --- | --- | --- | --- | --- | --- |
| **T01** | 探针超时不得杀进程（诊断 A1） | `plugin_runtime.rs:77-84`；`plugin_api.rs:36` 的 `view` 输出增 `unresponsive`；`extension-panel.js` 增一种「未响应，正在重试」呈现 | 人为让插件 `activate` 延迟 3 s，刷设置页 20 次：进程 pid 不变、`plugin-failures.log` 0 新增 | M | — |
| **T02** | 「插件 → 宿主」回调解耦（诊断 A2 / X1） | `plugin_process.rs:43/56-75/92-109`：读线程分帧，`method` 帧当场落到回调线程执行 `storage_call` | 冷启 20 轮（其中一轮并行跑 Defender 全盘扫描）：`gateway-error.log` 与 `plugin-failures.log` 均 0 新增 | L | T01 |
| **T03** | 校验缓存 + 重活移出 `CURRENT`（诊断 A3 / X2） | 新增 `host/plugin-verified/<id>-<version>-<digest>.json`（每文件 `size+mtime`）；`plugin_runtime.rs:152-185` 先取快照再放锁；`cleanup()`/`cleanup_staging()` 挪到启动后 30 s 维护线程 | 冷启不再对 129.9 MB 做全文哈希（实测包体：magpie 75.9 MB + clash 53.9 MB）；缓存命中率 ≥ 95%；`t29`/`t38` 全绿 | M | — |
| **T04** | 冷/热分档超时 + 单插件重试一次（诊断 A4 / X3） | `config.rs:112-116`；`logon::since_logon()`（`logon.rs:446`，现仅用于 `main.rs:379-380` 日志）判断「登录后 5 分钟内」；`host.hello` 的 params 下发 `hostCallTimeoutMs`；SDK `sdk/node/plugin.mjs:6` 读它 | 冷启 20 轮里人为注入 3 s 延迟仍能拉起；热态超时值不变 | S | — |
| **T05** | Node 探测死线放宽并缓存（诊断 A5） | `plugin_dependencies.rs:32`（2 s → 首次 8 s）；成功结果缓存到进程生命周期；`plugin_api.rs:32` 的 `view()` 不再每次重新探测 | 设置页连开 10 次，`node.exe` 启动次数 ≤ 1 | S | — |
| **T06** | 资源校验判据改文件指纹（诊断 B1） | `runtime.rs:230-264`（`state/verified` 格式）、`:361`（`first` 判据） | 更新后首启只校验变化文件；`manifest.json` 变化不再等价于全量重校；旧格式按缺失处理（升一次级自愈） | M | — |
| **T07** | 全量校验移出启动关键路径（诊断 B2） | `main.rs:1695`：拆成同步 quick check + 后台全量校验 | 更新后首启到首帧 ≤ 3 s；`WEBVIEW2_*` 环境变量仍在建窗前设置（`runtime.rs:402-403`） | S | T06 |
| **T08** | WebView2 ACL 按需（诊断 B3） | `runtime.rs:291-301`、`:371-377` | 日常启动不再递归 `icacls /T`；缺权限时仍有可读报错 | S | — |
| **T09** | 插件页面入口放锁 + 复用校验（诊断 C4 / X5） | `plugin_page.rs:20`（`open` 里的 `verify`）、`:46`（`call` 持 `CURRENT` 跑 RPC）、`:58-71`（每资源过 `EDIT` 锁 + 哈希） | 安装 magpie 期间打开插件页不再出现「插件页面请求超时」；`revoke` 与在途 RPC 的竞态有用例 | M | T03 |
| **T10** | 版本位与出包准备 | `app/src-tauri/Cargo.toml:6`、`tauri.conf.json:4`、`Cargo.lock` 的 `fish-dock` 条目、`installer.mjs --version` 四处一致；出包后 `7z t` 验包 | 四处全是 2.0.3；`7z t` 输出 `Everything is Ok`；资产带 `.sha256` | S | T01–T09 |

依赖：`T01 → T02`；`T03 → T09`；`T06 → T07`；`T01–T09 → T10 → G1 → G2 → R1`。

## 3. 版本位与插件目录协同

| 项 | 值 |
| --- | --- |
| Dock | `2.0.3`（`Cargo.toml:6`、`tauri.conf.json:4`、`Cargo.lock` 的 `fish-dock`、`installer.mjs --version`） |
| `fish.clash` | `2.1.2` → **`2.1.3`** |
| `fish.magpie` | `2.1.2` → **`2.1.3`** |
| `fish.feishu` | 保持 `2.1.0` |

`catalog/stable.json` 的三条记录要跟着改 `version` / `url`（指到 `v2.1.3/<name>.fish-plugin`）/ `sha256` / `bytes`。

**回填红线（2.0.2 的教训，不再重复）**：`sha256` 与 `bytes` 必须是真实包的值，由 `build-release` 在成包时写入并在**同一次提交**里回填。占位值会让宿主 `plugin_store::parse` 以「官方条目包大小或摘要无效」拒绝整份目录（`plugin_store.rs:69-83` 是全有或全无）：有缓存的老用户看不到 2.1.3，新装用户整页空白。回填前禁止 push 到远端、禁止据此发版。

## 4. 门禁

### 4.1 G1 自启高压测试（加严）

沿用 `scripts/run-g1-stress.ps1` 的 H1–H5，**新增两组**：

| 组 | 场景 | 轮次 | 通过判据 |
| --- | --- | --- | --- |
| H6 | 更新后首启：先装 2.0.2，再覆盖升级到 2.0.3 | 5 | 到首帧 ≤ 3 s；全量校验在后台完成；`state/verified` 在 10 s 内更新为新指纹 |
| H7 | 探针宽容：人为把插件 `activate` 延迟 3 s，反复刷设置页 | 20 | 插件进程 pid 不变；`plugin-failures.log` 0 新增；标签页不消失 |

原有判据保持：冷启 50 轮失败率 0、两插件 TTFB 在阈值内、无孤儿 `magpie.exe` / `mihomo.exe`、自启期间 WebView 主线程单次长任务 < 200 ms。

### 4.2 G2 本地成包校验

沿用 2.0.2 的 §6.5 全流程（前置检查 → 清单重算 → 独立进程出双包 → `7z t` → `diff-manifest` → 实装冒烟 S1–S6 → 人工签署）。**新增冒烟项**：

| # | 场景 | 通过判据 |
| --- | --- | --- |
| S7 | 覆盖升级后立刻断网首启 | 首帧 ≤ 3 s；插件自启失败只报一次且 `plugin-failures.log` 有可读原因；恢复网络后下一次启动自愈 |
| S8 | 安装 magpie 期间打开插件页 | 页面不再报「插件页面请求超时」；安装完成后自动正常 |

## 5. 执行顺序

1. **阶段一（实现）**：T01 → T02 → T03 → T09；并行 T04/T05/T06 → T07/T08；最后 T10。
2. **阶段二（候选包与门禁）**：出 2.0.3 候选双包 → G1（H1–H7）全绿 → G2 实装校验并签署。
3. **阶段三（发布）**：与插件 `2.1.3`（magpie + clash）同批发；发布说明引用 #55 / #56 的验收证据，插件侧引用 #15 / #16 / #17。

## 6. 风险与回滚

| 风险 | 处置 |
| --- | --- |
| T02 回调解耦引入回调顺序问题 | 保留「单插件单回调线程」的保守实现；`t29`/`t30` 全绿前不合并 |
| T03 校验缓存降级完整性 | 首次仍全文哈希；`size+mtime` 变化即重哈希；设置页保留「手动全量校验」入口（2.1.0 版位） |
| T06 旧 `state/verified` 格式 | 按缺失处理 → 等价于多升一次级；不写迁移脚本 |
| T09 放锁后与卸载/停用竞态 | `revoke(id)` 后句柄失效并拒绝；配在途 RPC 用例 |
| 插件 2.1.3 与 Dock 2.0.3 跨版本混发 | 版本纪律：目录 `sha256`/`bytes` 与实际资产一致；不同批发 |
| 出包受阻（工具链/清单） | 退回 2.0.2 现状发布，宿主修复留在分支；不改已发布版本位 |

## 7. 与文档和 issue 的关系

| 物件 | 位置 | 作用 |
| --- | --- | --- |
| 完整诊断报告 | [`plugin-startup-diagnosis-2026-11-02.md`](plugin-startup-diagnosis-2026-11-02.md) | 四项症状的根因、实测数据、探测脚本、证据索引 |
| 宿主侧工单 | [#55](https://github.com/Fish-under-sea/FISH-Dock/issues/55)、[#56](https://github.com/Fish-under-sea/FISH-Dock/issues/56) | 现象 + 根因 + 修复 + 验收 |
| 插件侧工单 | 插件仓 #15（签到数据）、#16（预装可见性）、#17（Clash 视觉与下拉） | 同名口径 |
| 本文 | `docs/plan-dock-2.0.3.md` | 把宿主侧任务从诊断里分离成可执行清单与门禁 |

> 本文只写 Dock 宿主侧。插件视觉（含节点下拉重做、浅色主题色板）在插件仓 #17，按 `fish.clash 2.1.3` 交付，不与 2.0.3 混在同一个包里改。

## 0. 交付状态（2026-11-02 晚）

2.0.3 现在由**两半**组成，本文只管第二半：

| 半 | 内容 | 状态 |
| --- | --- | --- |
| 上游同步 | Wakuwaku v0.3.9（上游 #97 / #98 / #99 / #100），判定见 [`plan-20261102-upstream-039.md`](plan-20261102-upstream-039.md) | **已合入** `52d113d` |
| 宿主侧插件启动 | 本文 T01–T10 | 首批 **T01 / T05 已合入**（见 §2.1）；T02 / T03 / T04 / T06 / T07 / T08 / T09 未开始；T10 的版本位已随上游同步升到 `2.0.3`，`installer.mjs --version` 与出包校验待构建机 |

据此，`§1`「2.0.3 只含 T01–T10」的口径修正为：**2.0.3 = 上游同步 + T01–T10**；T01–T10 之外的东西仍然退回 2.1.0。

## 2.1 首批已合入（T01 / T05）

| # | 改了什么 | 落点 | 验收 |
| --- | --- | --- | --- |
| **T01** | 探针超时不再等于「进程已退出」。`process.rs` 新增 `alive()` / `exited()`：`view()` 只有 `exited()` 说没了才 `remove`（remove 会 `Drop` → `kill`，连插件的 `magpie.exe` / `mihomo.exe` 一起带走），没应答只把这一轮报成 `unresponsive` | `plugin_process.rs`（`alive` / `exited` / `running` 注释）、`plugin_runtime.rs`（新增 `Liveness` + `liveness()`，`view()` 改判据、行里多一个 `unresponsive` 字段）、`extension-panel.js`（「未响应，正在重试」） | Rust `an_unresponsive_plugin_is_not_a_dead_one`（没应答→Unresponsive，真没了→Gone）；JS `探针没应答报「未响应，正在重试」，不冒充启动失败`。真机 H7 待出包后跑 |
| **T05** | Node 探测死线 2 s → 8 s，成功结果按 `(路径, 长度, 修改时刻)` 缓存在进程生命周期里；文件一变自动重探 | `plugin_dependencies.rs`（`probe` 加缓存、新增 `PROBED` / `stamp` / `cached` / `remember`） | Rust `a_probed_node_is_remembered_until_the_binary_changes`；真机「连开 10 次设置页，`node.exe` 启动次数 ≤ 1」待出包后跑 |

**T01 为什么排在最前**：它不是「慢」，是**杀**。`plugin-failures.log` 里那批 `exitCode:null` 的记录，每条背后都是一次「进程还活着、被宿主判死并 kill」，插件拉起的服务跟着消失——这解释了「首次启动插件自启异常、之后恢复服务又正常」为什么只在冷启那阵子出现：冷启时 I/O 与杀软最重，2 s 探针最容易超时。

**仍未做的那条硬骨头**：T02（回调解耦）。诊断里它是 L 级结构改动，现实现是「单插件单回调线程 + 只在等这个插件响应时才服务回调」，改成读线程分帧要配 `t29` / `t30` / G1 全组重跑；`§6` 的风险表里也写了「`t29`/`t30` 全绿前不合并」。在 T01 落地后，它的紧迫性下降（误杀已经不会再发生），建议按 `§6` 的处置排到 2.1.0。