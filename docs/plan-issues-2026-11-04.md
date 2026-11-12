# 主仓 OPEN issue 计划实现文档（plan 阶段）

> 生成：2026-11-04。**同日已获批准实施并完成合并验收**（结论见 §11）。
> 本文件前半是 PLAN 阶段的探测与方案，后半（§11）是实施与验收记录；未被实施的条目保持 OPEN。
>
> 探测基线：
> - 主仓 `FISH-Dock` HEAD `3c90d94 chore(release): 2.0.4`（工作树干净）；
> - 插件仓 `FISH-Dock-Plugins` HEAD `af4728d`（2.2.0，已发布）；
> - issue 来源：`gh issue list/view --repo Fish-under-sea/FISH-Dock`，6 个 OPEN、均无评论；
> - 本机运行时证据：`C:\Users\Fish\.fish-dock\host\`（catalog 缓存、plugin-failures.log、installed-plugins.json），只读采样。
>
> 行号均指上述基线的工作树文件；跨仓文件以「主仓 / 插件仓」前缀标注。

---

## 0. 总览

| # | 标题（简） | 类别 | 归属 | 优先级 | 依赖 |
| --- | --- | --- | --- | --- | --- |
| 68 | 插件下载三源全灭时应回退上一可用版本 | 缺陷 / 健壮性 | **主仓** | P0 | 插件仓目录数据 |
| 69 | 插件卡「更多」留白、标题字号、Dock 弹窗滚动锁 | UI/UX | **主仓前端** | P1 | — |
| 70 | 首次自启（含开机自启）失败 | 缺陷 / 需复核 | **主仓 + 真机实测** | P1 | 用户配合重启 |
| 71 | 预装登记只做一次，老用户拿不到 Meoo | 缺陷 / 数据迁移 | **插件仓**（issue 开在主仓） | P1 | 插件仓发版 |
| 29 | 内存长跑验收单（iframe 5 分钟退场） | 验收执行 | **主仓工具 + 真机** | P2 | 装机 2.0.4+ |
| 30 | GPU 217–246 MB 归因 | 归因实验 | **主仓 CSS + 真机** | P2 | #29 的 GPU 差值 |
| **附加 A**（无 issue 号） | **国内镜像下载（Gitee 双仓 + 历史欠账）** | 健壮性 / 历史欠账 | **主仓 + 工作区 AGENTS.md** | P1 | #68 的版本候选框架 |

**先决结论（完整探测的最重要发现，逐条有证据）**

1. **#70 的 issue 描述与当前 2.0.4 代码基线不符**：`activate` 预算与重试已在 2.0.4 T-E 落地且远超诉求（冷态 60 s、4 次尝试、180 s 生命周期、阶段分档）。要先真机复核再决定是否改代码，不能照 issue 复选框重做。详见 §3。
2. **#68 的原始复现条件已消失**：本机 `official-plugin-catalog.json` 缓存已是 2.2.0，v2.1.7 及之后的所有资产均已发布。回归必须用测试目录 URL 或注入夹具构造「目录抢跑」，不能等自然复现。
3. **#71 不在主仓**：主仓全树 `preinstall` 只命中文档与 README 声明，实现全在插件仓 `plugins/magpie/preinstall.mjs` / `runtime.mjs`。计划按跨仓处理，建议把 issue 迁移或在本仓记录「修复在插件仓」。
4. **#29 的预算原文缺失**：仓库内 glob 不到 issue 引用的 `红线-禁止屎山十条.md`；执行验收单前需先拿到口径，否则 PASS/FAIL 判定无依据。
5. **#68/#69 的前端文案/结构已有部分实现**（结构化失败 JSON、details 菜单、DialogAnchor），计划是「扩展与收口」，不是从零重写。
6. **Gitee 双仓都存在，但插件仓没有发行版本**：`GET https://gitee.com/api/v5/repos/<owner>/<repo>` 两仓均 200；主仓有 tags（`v1.0.0` 起 5 个），插件仓有 tags 但 `releases` 返回 0。主仓已有「配了才启用」的镜像配置（`FISH_DOCK_RAW_MIRROR` / `FISH_DOCK_GH_PROXY` / `FISH_DOCK_GITEE_REPO` / `FISH_DOCK_PET_MIRROR` + `mirror.prefer`），**国内镜像下载的历史欠账是「没调研、没实测、没默认可用来源」，不是「没有代码框架」**。详见 §10 与工作区 `AGENTS.md` §八。

**排期建议**：P0 #68 → P1 #71（跨仓、可独立发版）→ P1 #69 → P1 #70 实测 → P1 附加 A（与 #68 共用版本候选/摘要口径，排在同批或紧随其后）→ P2 #29 → P2 #30（#30 必须在 #29 拿到 GPU 差值后）。#70 的实测可与 #68 并行（不冲突）。

---

## 1. #68 插件下载失败自动回退上一可用版本（P0）

### 1.1 现象与诉求（issue 原文要点）

目录里写了 `magpie 2.1.7`（sha256/bytes 都填了），但 npm 官方源 404、npmmirror 302→cdn 404、GitHub release 404；三源全灭后用户看到「检查网络或代理后重试」，而网络其实是好的。诉求 5 条：**界面保留新版可见**、**三源全失败自动回退上一可用版本**、**按来源分别降级**、**下载前探活**、**降级仍校验摘要**；附带「错误文案要打 URL / HTTP 状态码 / 失败阶段」。

### 1.2 完整探测：代码地图

| 层 | 文件:行 | 做什么 |
| --- | --- | --- |
| 配置 | 主仓 `app/src-tauri/src/config.rs:85-101` | `FISH_DOCK_PLUGIN_CATALOG_URL` / `..._CATALOG_API`（raw + contents 两源）、npm registry / mirror 默认值 |
| 目录读取 | `plugin_store.rs:101` `catalog_urls()`、`:138-157` `fetch_catalog_with/refresh` | 两源取 `stable.json`，写缓存 `config/official-plugin-catalog.json`；取不到用缓存 |
| 安装入口 | `plugin_api.rs:68` | `plugin_store::obtain(context,&id)` → 失败经 `plugin_failure::wrap("download", ...)` |
| 单版本取包 | `plugin_store.rs:242-260` `obtain/obtain_traced` | **只读缓存目录里该 id+platform 的一条 entry**（version/url/sha256/bytes），构造三源 |
| 三源定义 | `plugin_store.rs:253-255` | `npm_tarball(官方)`、`npm_tarball(镜像)`、`entry.url`（GitHub release） |
| 逐源尝试 | `plugin_store.rs:187-223` `fetch_from` | 依次尝试，成功即停；失败收集 `(origin,error)` → `download_failed(failures)` |
| 校验 | `plugin_store.rs:174-184` `package_from_npm`、`:197-203` GitHub 分支 | size + sha256 + manifest id/version 三重比对（**没有因来源放宽**） |
| 网络层 | `plugin_store.rs:102-137` `download/single_download/download_with` | `max_redirects(0)` 手动跟随 302；**`:125` 白名单只放 `release-assets.githubusercontent.com` / `objects.githubusercontent.com`**，其余跳转报「插件下载跳转不属于官方发行资源服务」 |
| 统一代理/镜像 | `sources.rs:154-...` `fetch_bytes_with`、`proxy::web_ways` | 候选展开与代理路径 |
| 失败文案（后端） | `plugin_failure.rs:26-39` `download_failed`、`:76-91` `classify`、`:94-117` `source_short` | 结构化 JSON（`fishFailure`，message + hint + sources[]）；`hint` 固定「检查网络或代理后重试。」；`source_short` 把 302 拒映射成「跳转被拒绝」 |
| 失败文案（前端） | 主仓 `app/src/pet/extension-panel.js:113-139` `parseFailure/failureHtml` | 渲染 `origin · short`，原始串放 `title`；**没有 URL、HTTP 状态码、失败阶段** |
| 安装事务 | `plugin_install.rs:1079` 注释、`:523-524` / `:695-699` fetch 记录 | digest 用目录 sha256；`plugin-fetch.json` 记录「哪条源装的」 |

**数据链一句话**：`catalog 缓存 → 单条 entry（唯一版本）→ 三源 URL → fetch_from → 校验 → 安装事务`。**整条链没有「版本候选集合」这个概念**，这是回退能力的根本缺口。

**本机证据**（2026-11-04 18:20 采样）：
- `official-plugin-catalog.json`：feishu/clash/magpie 均 `2.2.0`（bytes 36,368 / 17,178,572 / 33,861,986），与已发布 v2.2.0 资源一致 → issue 描述的「v2.1.7 不存在」窗口已关闭。
- `plugin-failures.log` 末尾 4 条仍是 2.1.5/2.1.6 时代的「插件响应超时 / 进程已退出」，**没有本次 404 的结构化下载失败记录**（该记录进 `recent.log`，由 `plugin_failure::log` 写）。

**附带发现**：`download_with:125` 的跳转白名单与 `plugin_thirdparty.rs:104-125` 的「同站点域」判据不一致——后者已允许 `cdn.npmmirror.com` 一类自有 CDN，前者一刀切拒。这正是「跳转被拒绝」四个字的技术来源（不是代理坏）。

### 1.3 根因判定

1. 目录数据（上游）可以先于资产发布；宿主把目录当成「唯一版本 + 必然可下载」的契约，没有制品存在性验证。
2. `obtain_traced` 只用一条 entry，无历史版本来源 → 无法回退。
3. 探活缺失：三条链各跑完整下载 + 校验才发现 404，浪费且误导。
4. 错误分级把「404 / 302→404」归到 network 类，hint 指向代理；`SourceFailure` 结构里没有 url/status/phase，前端想展示也拿不到。

### 1.4 实现计划（主仓，分 5 步）

**S1 版本候选来源（新增能力，不破坏现有单版本路径）**
- 在 `plugin_store.rs` 增加 `version_candidates(context, id) -> Vec<Entry>`，来源优先级：
  1. **目录历史条目**（推荐 A，需插件仓配合）：`stable.json` 增加 `versions: [{version,url,sha256,bytes}]`（最近 N 个已发布版本），宿主按序尝试；摘要与目录同源，最可信；
  2. **运行时查版本索引**（备用 B）：npm packument `GET registry.npmjs.org/@fish-under-sea/fish-dock-plugin-<slug>` 取 `versions`/`dist-tags.latest`（`plugin_thirdparty.rs:313/457` 已有元数据 URL 与 scope 编码经验可复用）+ GitHub `releases?per_page=10`（`updates.rs` 已有 releases API 封装）；
  3. **内置已知可用版本表**（保底 C）：随宿主发版写死最近一个已验证版本摘要，仅当 1/2 都不可用时使用。
- **信任边界（硬约束）**：候选版本只能来自官方目录/官方 registry/官方 release；回退版本的 `sha256` 必须来自 `release SHA256SUMS.txt` 或目录历史条目，**禁止用下载到的包自己算摘要当期望值**；任一候选校验失败按该来源失败处理，继续下一个候选，不放宽 `package_from_npm` / `inspect` 的任何比对。

**S2 下载前探活**
- `fetch_from` 之前对每个 `(origin, candidate)` 做轻量探活：npm tarball / GitHub release 用 `HEAD`（复用 `sources::fetch_bytes_with` 的代理路径，新增 `fetch_head`）；镜像 302 允许跳到「同站点域」白名单（复用 `plugin_thirdparty.rs:104-125` 判据，收敛 `download_with:125` 的硬编码）。
- 探活结果记入 `Attempt`：`url`、`http_status`、`phase ∈ {连接, 跳转, HTTP, 校验, 摘要}`、`elapsed_ms`；探活 404 直接跳过该来源该版本，不再跑完整下载。

**S3 按来源链分别降级**
- `fetch_from` 签名扩为 `fetch_from(entry_candidates, sources_fn, ...)`：对每条来源，按候选版本由新到旧找「该来源最近一个可用版本」；成功即返回 `(package, digest, record)` 并带上 `effective_version`（实际装到的版本）。
- 「界面保留新版」：卡片更新提示仍读 `catalog.stable.json` 的最新版，不因下载失败隐藏/降级目录数据；安装完成的 notice 写「已安装 Magpie 2.1.6；目录最新 2.1.7 暂不可用」（`InstallNotice`，`plugin_install.rs:21`）。

**S4 错误分级与面板补字段**
- `SourceFailure` 增加 `url`、`status`、`phase`；`plugin_failure.rs:26-39` 新增 kind `catalog`（判定：三源均 404/制品缺失），hint 改为「目录里的新版暂不可用，已自动回退到 X；无需修改网络或代理」；`source_short` 对 `HTTP 404` 输出「版本制品不存在（404）」而不是「跳转被拒绝」。
- `extension-panel.js:135` 的 `failureHtml` 每行展示 `origin · short · HTTP 状态码 · URL（可复制）`，`title` 保留原始技术串；`stageText/defaultHint` 增加 `catalog` 文案。

**S5 回归夹具**
- 单测（注入 `fetch` 闭包，不触网）：目录指向不存在版本 + 三源 404 → 断言回退到上一版本成功、摘要仍校验、notice 文案、面板字段齐全；「npm 只有旧版 / GitHub 有新版」→ 断言按来源分别降级；全候选失败 → 断言不装任何包且提示不改网络建议。
- 真机回归：用 `FISH_DOCK_PLUGIN_CATALOG_URL` 指向一份测试目录（伪造指向不存在版本），不改线上目录。

### 1.5 验收映射（issue 5 条 + 附带）

| issue 诉求 | 验收命令/断言 |
| --- | --- |
| ① 界面保留新版 | 卡片仍显示目录最新版；测试断言 catalog 读取不被下载结果改写 |
| ② 三源全失败自动回退 | 单测 `test_fallback_to_previous_published_version`；装完后 `installed-plugins.json` 版本 = 上一可用版本 |
| ③ 按来源分别降级 | 单测 `test_per_source_version_fallback` |
| ④ 下载前探活 | 单测断言探活 404 时 `fetch` 完整下载调用次数 = 0 |
| ⑤ 降级仍校验摘要 | 单测用错摘要断言安装失败且不落事务 |
| 附带：URL/状态码/阶段 | `failureHtml` 快照断言含 `404` 与 URL；`recent.log` 含逐源 status/phase |

### 1.6 风险与未决

- 「版本候选」决定装哪个版本，**必须限定可信来源**，否则等于把版本选择权交给网络；建议 A（目录历史）优先，B 只作补充并校验 release 摘要。
- npmmirror 302 白名单放宽需严格限定同站点域，避免开放重定向面。
- 回退装旧版可能与宿主新特性不兼容：notice 明确提示，且不自动再升级（等目录修正）。
- 需插件仓配合 S1-A；若 YG 不愿意改目录 schema，则走 B+C，摘要可信链更绕，要在方案评审时定。

---

## 2. #69 插件卡与浮层视觉规范（P1）

### 2.1 现象与诉求（issue 原文要点）

三处同属插件卡/浮层：①「更多」展开区约 210px 只有 4 个元素，要压到 ≤120px、行高 32–36px、路径与入口合并一行；② 已安装卡标题 15px 偏小、行内留白多，要 17px/600、状态点与版本号间距、行高由内容决定；③ 弹窗打开后底层列表仍可滚动，要锁滚动并恢复 scrollTop。建议一起做「视觉规范」防止 #63 式反复。

### 2.2 完整探测：代码地图

| 点 | 文件:行 | 现状 |
| --- | --- | --- |
| 「更多」HTML | 主仓 `app/src/pet/extension-panel.js:557-560` | `moreRow()` 生成两行按钮（logs / uninstall），`<details class="extension-more">` 包 `<summary>更多</summary>` + `.extension-more-body` |
| 卡片结构 | `extension-panel.js:567` | `<article class="extension-card">` = header（标题 + actions）+ `metaLine(storage.directory, sourceText)` + error/notice + `.extension-pages`（`extension-page-entry` 按钮，`:566` 生成） |
| 「更多」CSS | `app/src/pet/extensions.css:51-72` | summary 24px 胶囊；`.extension-more-body` `padding-block:8px 0`、flex column；`.extension-more-row` `min-height:32px`、`font-size:12.5px`；`row + row` `margin-top:8px` + `border-top`；danger 常态红（`:70-72`） |
| 标题 CSS | `extensions.css:24-26` | `.extension-card-header` flex + `gap:8px 12px`；`.extension-card-title strong` **13px/600**；meta 12px 次级（`:46-47`） |
| 入口 CSS | `extensions.css:75-77`、`:314-318` | `.extension-page-entry` 32px、12.5px/600；窄屏 `width:100%` |
| 特异性补丁 | `extensions.css:102-108` | `.extension-panel .extension-more-row` 覆盖通用 `.pbtn` 的 min-height |
| 弹窗 | `app/src/pet/settings.js:225-308` | `<dialog class="s-confirm-dialog">`（`:227`）、`.s-body`（`:225` 滚动容器）、`pumpConfirm:253-274` `showModal()` + `seatDialog` + `DialogAnchor.watch`、`finishConfirm:292-302` `close()` |
| 弹窗样式 | `settings.css:2813` `s-confirm-dialog overflow:auto`；`style.css:688-689` `overflow:hidden auto; overscroll-behavior:contain`；`extensions.css:179` `overscroll-behavior:contain` | 已有 `contain`，但**没有「弹窗期间锁底层滚动」的统一 hook** |
| 弹窗锚定 | `app/src/pet/dialog-anchor.js:3` | `<dialog showModal>` 进 top layer、UA 会钉屏幕正中的根因说明 |

**现状与 issue 截图的偏差（重要）**：当前 `more-body` 里只有 **2 个** 按钮；`.fishplugin-magpie` 路径在 `metaLine`（details 外），`Magpie 网关 >` 入口在 `.extension-pages`（details 外）。用户量到的「展开区 210px、4 个元素」是**展开后整张卡片下半区**的观感。因此修复对象应是「卡片下半区整体密度」，而不是只改 `<details>`；否则改完仍达不到 ≤120px。

### 2.3 实现计划（主仓前端，4 步）

**S1 卡片下半区重排**（`extension-panel.js:560/566/567` + `extensions.css`）
- `metaLine` 的目录名（12px 次级、单行省略、`title` 全路径、点击复制）与 `.extension-pages` 的入口按钮**合并成一行**：左路径右入口，两侧 `min-width:0`。
- `more-body` 保留两行动作，但改为紧凑列表：行高 32–36px、整行 hover 底色、左侧 16px 图标；logs 与 uninstall 同组、组内仅一次 1px 分隔线、上下 8–10px；danger 保持常态红、置组末、与上方间距 8–12px。
- 展开区目标 ≤120px；在 `extensions.css` 顶部写「插件卡与浮层视觉规范」注释块（字号/行高/间距/危险色/滚动锁口径），防回归。

**S2 标题行**（`extensions.css:24-26`）
- `strong` 13px → **17px/600**，与按钮基线垂直居中；状态点↔状态文字 6px、状态文字↔版本号 8px；版本号 ≥12px 次级或改小胶囊；header 上下 padding 收紧到约 10px，**由行高决定按钮高度**（不改按钮 min-height 以外撑高行的属性）。

**S3 统一滚动锁**（`settings.js` 为主，覆盖 `confirm` 与插件详情/设置浮层）
- 新增 `lockScroll()/unlockScroll()`：记录 `.s-body` 与 island 滚动容器的 `scrollTop`，加 class `scroll-locked`（`overflow:hidden` + `overscroll-behavior:contain`）；引用计数支持多弹窗嵌套。
- `pumpConfirm:266` 之后加锁；`finishConfirm:296`、取消、点背景关闭、Escape 全部解锁并恢复 `scrollTop`。
- 键盘 PgUp/PgDn/方向键与 `wheel`/触控惯性在锁定期 `preventDefault`（弹窗自身可滚，`s-confirm-dialog` 保留 `overflow:auto`）。
- 弹窗定位继续用 `DialogAnchor`（fixed/top layer），不参与列表滚动；`reseatConfirm:286-291` 的时机保持。

**S4 视觉与交互回归**
- 用既有浏览器测试夹具（仓库内 `*.test.mjs`/CDP 脚本风格）量：展开区总高、卸载与日志间距、标题行上下留白、行内间距；弹窗打开时滚轮/触控/键盘翻页无效且弹窗不动、关闭后滚动位置恢复；重开插件页功能完整。

### 2.4 验收映射

| issue 验收 | 检查 |
| --- | --- |
| 展开区 436px 内无可见空行、间距 ≤40px | CDP `getBoundingClientRect()` 断言，目标总高 ≤120px |
| 标题与按钮视觉重量接近、行内留白 ≤12px | 同视角截图 + 计算样式断言 17px/600 |
| 弹窗锁滚动（滚轮/触控/键盘）、关闭恢复 | 事件注入断言 `scrollTop` 不变且恢复 |

### 2.5 风险与未决

- `<details>` 原生展开/收起与自定义滚动锁叠加时的焦点管理；多弹窗嵌套的引用计数必须在所有关闭路径（含 Escape、背景点击）都减一。
- WebView2 下 `overscroll-behavior` 对触控惯性的实际效果需真机验证；若无效需补 `wheel` 事件兜底。
- 视觉数值（17px、120px 等）来自 issue 截图目测，批准前建议 YG 确认一次目标截图。

---

## 3. #70 首次自启（含开机自启）失败（P1，**先复核、后改码**）

### 3.1 issue 诉求与证据

现象：开机那次「插件进程起来了、本机服务没起来」，再点一次启动就好；证据是 `plugin-failures.log` 里 2.1.5/2.1.6 时期的「插件响应超时 / 握手超时」。诉求 4 条：冷启放宽 activate 预算、失败后重试、失败记录带阶段与耗时、确认「系统开机自启」是否真的把 Dock 拉起。

### 3.2 完整探测：**issue 描述与 2.0.4 基线不符**

| 诉求 | 当前实现（证据） | 判定 |
| --- | --- | --- |
| 冷启 activate 预算放宽到 15s | `config.rs:122-160`：`DEFAULT_PLUGIN_ACTIVATE_SECS=5`（旧键，降级为只读下限）、`PLUGIN_ACTIVATE_COLD_SECS=20`、`PLUGIN_START_COLD_SECS=60`、`PLUGIN_START_WARM_SECS=10`、`PLUGIN_START_LIFECYCLE_SECS=180`；`config.rs:461-472` `plugin_start_budget_of(cold)` 冷态取 `max(60, 旧热态, 旧冷态)`；`config.rs:430-448` 冷态双锚点（登录后 300s / 进程启动后 120s） | **已实现且远超 15s**（60s 一次尝试共享预算） |
| 自启动失败后重试 | `plugin_runtime.rs:151` `CURRENT.deferred=true` → `resume:566-568` 走 `spawn_starts`（`:206-231`）→ `plugin_start::drive`（`plugin_start.rs:607-693`）→ `scheduled_attempt`（`plugin_runtime.rs:235-282`）；`plugin_start.rs:26-35` `MAX_ATTEMPTS=4`（首次+3 重试）、`BACKOFF=[0.5,2,5]s`、`MAX_START_PARALLEL=2`；`Phase{Verify,Spawn,Hello,Activate,Service}`（`:64-79`）；`log_finish:695-703` 带 `attempt/state` | **开机自启路径已实现重试（4 次）** |
| 失败记录带阶段与耗时 | `plugin_runtime.rs:86-93` `Failure{plugin,version,stage,code,at,raw,log}`、`:344-347` `failure()`；落盘 `plugin-failures.log`（`:19`）；调度器的 phase/attempt/state 只在 `eprintln!`（`:262/280`、`plugin_start.rs:695-703`），**未进结构化失败记录** | **部分缺口：stdout 有，落盘无 phase/耗时/attempt** |
| 系统开机自启是否把 Dock 拉起 | `logon.rs:36/47-83` Task Scheduler XML（参数 `--wakuwaku-at-login task`）、`:359-404` `xml/register/run/remove`、`:446-465` `since_logon()` | **代码存在，缺本机实测** |

**路径分叉（关键）**：`resume()` 在 `deferred=false`（单测 / 自建 Runner）走 `:570-577` 逐插件 `start_instance`（`:524-538`），**失败即进 `self.failures`，不重试**；用户手动「启用/重试启用」也走 `set_enabled_starting`（`:502-519`）→ `start_instance`。即：**开机自启有 4 次重试，手动启用没有**，两段行为不对称。

**本机证据**（2026-11-04 18:20）：
- `plugin-failures.log` 末尾 4 条仍是 2.1.5（clash 响应超时）与 2.1.6（magpie 退出码 `1073807364`）时期，**没有 2.1.9/2.2.0 之后的新记录**；
- `installed-plugins.json` 3 个插件（clash / magpie / jassdingk.filemgr）；catalog 缓存已 2.2.0；
- 插件侧 2.1.9 已修（Clash `backend.mjs` activate 立刻返回、内核路径自愈、0/1.5/4s 三次重试），但**本机是否真的升到 2.1.9+ 并重启验证过，无证据**。

### 3.3 根因判定（分层）

1. issue 记录的失败发生在 2.1.5/2.1.6 + Dock 2.0.2/2.0.3 时代；宿主 2.0.4 与插件 2.1.9 两侧都已改过，**不能直接照旧结论改码**。
2. 仍存在的真实缺口：① 结构化失败记录缺 phase/耗时/attempt；② 手动启用路径无重试；③ 开机项本身无实测结论。
3. 「第一次开机没起来」也可能是 Dock 根本没被登录任务拉起（若托盘没出现，插件侧任何修复都无关）。

### 3.4 实现计划（先实测，后按需改码）

**S0（必须先做，不改码）真机复核**
- 装 2.2.0 三插件 → 重启或注销/登录 → 记录：托盘是否出现、`recent.log` 中 `FISH Dock: [plugin-start] ...` 与 `插件自启调度第 N 轮` 行、`plugin-failures.log` 是否新增、各插件 `config/run-state.json`、服务是否起来。
- 判定矩阵：
  - 托盘未出现 → 问题在 `logon.rs` 任务计划/安装器，转 S3；
  - 托盘出现 + failures 新增 → 取 `phase/attempt` 日志定位，转 S1/S2；
  - 全部正常 → #70 可关闭（附实测时间戳与日志路径），仅保留 S1 作为可观测性改进（可选）。

**S1（可观测性，主仓）失败记录结构化**
- `Failure` 增 `phase`、`attempt`、`elapsed_ms`、`budget_cold`（向后兼容：`json()` 增字段，旧记录解析不受影响）；`failure()` 与 `record()`（`:340-342`）相应扩展。
- `scheduled_attempt` 失败分支（`:259-264`）与 `log_finish` 改为复用 `plugin_failure::log` 写 `recent.log` 的结构化行（含 id/version/phase/attempt/elapsed/state/budget），并回写 `plugin-failures.log`。
- 前端 `extension-panel.js` 失败卡可展示「卡在 activate 第 2 次尝试（耗时 X ms）」。

**S2（对称性，主仓）手动启用接入重试**
- `set_enabled_starting` / `start_instance` 失败时，按同一 `plugin_start` 调度器投一次 token（或至少一次 3s 退避重试），复用 `in_flight` 防重；避免与 `spawn_starts` 并发重复 spawn。

**S3（开机项实测，主仓）**
- PowerShell 读 Task Scheduler 中该任务的 XML（`logon.rs::seen` 的判据）、启用状态、上次运行时间与结果；确认 `--wakuwaku-at-login task` 参数与当前 exe 路径一致（旧名清理 #27 的遗留风险）。
- 若任务缺失/被禁用/指向旧 exe → 修复登记路径（安装器侧），另开子任务，不混进插件启动链路。

**预算**：冷态 60s / 生命周期 180s 已足够；**不改默认值**。若 S0 实测显示仍不够，用 `FISH_DOCK_PLUGIN_START_COLD_SECS` 环境变量调档验证，再决定是否改默认（热态 10s 不动）。

### 3.5 验收映射

| 检查 | 命令/证据 |
| --- | --- |
| 重启后三插件服务自动运行 | `run-state.json` + 各插件 view；failures 无新增 |
| 失败可定位 | `plugin-failures.log` 一条含 `phase/attempt/elapsed_ms`；`recent.log` 含 `[plugin-start]` 结构化行 |
| 手动启用也有重试 | 单测：注入首次失败，断言第二次尝试成功 |
| 开机项 | 任务 XML + 上次运行结果截图/输出 |

### 3.6 风险与未决

- 扩展 `Failure` 需兼容既有 `plugin-failures.log` 与任何诊断脚本（增字段不删字段）。
- S2 与 `spawn_starts` 的并发：必须走 `in_flight` 与 `generation` 判据（`plugin_runtime.rs:140-146/240-249`），否则可能双 spawn。
- S0 需要 YG 配合重启机器；未拿到实测前不写 S1/S2 代码（避免基于过期结论的无效改动）。

---

## 4. #71 预装登记只做一次（P1，**修复在插件仓**）

### 4.1 现象与根因（issue 原文 + 本机证据）

老用户（2.1.1 首启，逐版升到 2.1.9）供应商页没有 Meoo；`.fish-dock-preinstalled.json` 13 项无 meoo（mtime 11/1），`plugins.json` 无 meoo spec（mtime 11/2），`node_modules` 13 包无 `magpie-plugin-meoo-agent`，`plugin-providers.json` 21 provider 无 meoo，`config/preinstall-state.json` 的 `registered` 含 meoo 但 `added: []`（状态文件撒谎）。Meoo 是 2.1.4 才进基线（同版移出 Zen Free）。

### 4.2 完整探测：代码地图与跨仓归属

| 层 | 文件:行 | 做什么 |
| --- | --- | --- |
| 主仓 | `app/README.en.md:9`（声明不预装业务服务）、`docs/plugin-startup-diagnosis-2026-11-02.md:300/345/346/400/480/600`（诊断文档） | **主仓没有任何预装登记实现**；grep `preinstall` 全树只命中文档与 `storage_context.rs:126` 的测试名 |
| 插件仓调用点 | `plugins/magpie/runtime.mjs:12`（import）、`:133-136` | `seedPreinstalled(...)` → `ensurePreinstallState(...)`，异常吞掉（仅 AbortError 抛出） |
| 插件仓核心 | `plugins/magpie/preinstall.mjs:107` `markerName='.fish-dock-preinstalled.json'`、`:168-206` `seedPreinstalled` | **`:170-175` 见 marker 即 `return {skipped:true}`**，不再调 `addPreinstalled`；`:176-188` 读 `plugins.json` → `addPreinstalled` 补缺 → 写；`:190-197` 写 marker（`wx` + EEXIST 并发保护）；`:199-204` 写状态文件 |
| 插件仓幂等能力 | `preinstall.mjs:76-105` `addPreinstalled` | 「只补缺席的包，已有同名包条目保留原样」——**本身幂等且正是修复所需** |
| 插件仓状态 | `preinstall.mjs:120-125` `pendingStems`、`:156-166` `ensurePreinstallState`、`:114-116` `stateRelativePath` | `ensurePreinstallState` 只补状态文件不补 spec；`registered` 直接取 `PREINSTALLED.map(package)`，不是实际登记结果 |
| 插件仓测试 | `preinstall.test.mjs:150-205`（首次/二次/并发）、`:223-253`（状态与调用顺序） | 现有用例覆盖「首次登记」与「跳过」，**没有「基线变更后补登记」用例** |

**根因**：marker 被当成「一次性开关」而不是「已登记集合」；基线增量（新增 meoo）只对首启生效；`ensurePreinstallState` 不补 spec；状态文件的 `registered` 由基线推导而非实际结果，造成「看起来已登记」。

### 4.3 实现计划（插件仓，5 步；主仓只做 issue 归档说明）

**S1 marker 语义改造**（`preinstall.mjs:168-206`）
- marker 内容从 `{schema,registered:[13 项]}` 升级为 `{schema, baseline: <PREINSTALLED 的 spec 列表>, registeredAt}`，**保留 `registered` 字段**以兼容旧版读取。
- `seedPreinstalled` 不再一见 marker 就早退：读 marker → 与当前 `PREINSTALLED` 求差集（新增项）→ 对差集走 `addPreinstalled`（幂等补缺）→ 有变化才写 `plugins.json` 并更新 marker；无变化纯读，不写盘。
- 未在新基线里的旧项（zen-free）**不主动删除/卸载**（与既有「移出基线、已装用户不动」口径一致）。

**S2 最小改动备选**（若 YG 不想改 marker 格式）
- 直接去掉 `:170-175` 的早退，每次启动都跑一次 `addPreinstalled`（纯内存比较 + 幂等写，成本可忽略）；marker 仅保留「首次初始化」用途；保留 `wx`/EEXIST 并发保护。
- 与 S1 二选一，方案评审时定；推荐 S1（可解释、状态可信）。

**S3 状态文件说真话**（`preinstall.mjs:156-166` + `runtime.mjs:133-136`）
- `ensurePreinstallState` 的 `registered` 取 `seedPreinstalled` 返回的**实际登记结果**，`added` 取实际新增的 spec；让 `brand:status` 的「正在准备 N 个供应商插件」可信。

**S4 数据迁移与验收路径**
- 老数据根（2.1.1 的 13 项 marker）升级到新插件 → 首次启动补写 meoo spec → magpie 更新循环按 `github:RexVane/magpie-plugin-meoo-agent` 安装 → `plugin-providers.json` 出现 meoo。
- 回归用例（`preinstall.test.mjs` 新增）：
  1. 老 marker（无 meoo）+ 新基线（含 meoo）→ `plugins.json` 出现 meoo spec、状态文件 `added` 含 meoo；
  2. 重复启动不重复写（spec 不重复、`mtime` 不变）；
  3. 新装用户行为不变；
  4. 已装 zen-free 不被卸载；
  5. marker 为旧格式时按「缺字段」处理，不抛错。

**S5 跨仓发布与归档**
- 插件仓出新版（版本位、`catalog/stable.json`、发行说明、README 版本卡按既有流程），主仓不动代码；
- 在本 issue 回帖说明修复仓与版本，或由 YG 把 issue 迁移到 `FISH-DOCK-Plugins`。

### 4.4 验收映射

| 检查 | 文件 |
| --- | --- |
| 老用户升级后 spec 出现 | `plugins.json` 含 `github:RexVane/magpie-plugin-meoo-agent` |
| 包真的装上 | `plugins/node_modules` 出现 `magpie-plugin-meoo-agent` |
| 供应商页可见 | `plugin-providers.json` 含 meoo provider |
| 状态文件可信 | `config/preinstall-state.json` 的 `registered/added` 与实际一致 |
| 不重复、不误删 | 单测 2/4 |

### 4.5 风险与未决

- 旧版 magpie 读新 marker：保留 `registered` 字段、新字段可忽略，向后兼容。
- 并发启动两个实例：`wx` + EEXIST 已在，S1 的「读-比-写」需保持原子（写临时 + rename，或复用 `writeManifest` 的既有实现）。
- 需要插件仓发版（本会话已具备完整出包/发布流程）；主仓 issue 是否迁移由 YG 决定。

---

## 5. #29 内存长跑验收单（P2，验收执行型）

### 5.1 诉求与已知数字

在**装好的 2.0.0+ 且有插件**的真实形态上，采 5/30/60min 三档逐进程清单；按预算逐条 PASS/FAIL（窗口 30→60min、进程年龄 ≥30min）；专验 iframe `PARK_DISCARD_MS=5min` 到点退场（T0+3min 仍在且 ≥150MB、T0+5min±30s 消失、renderer 回 2、T0+6min dock 私有 ≤510MB 且降 ≥150MB、重开可用）；报「退场前后 GPU 差值」只报数不设线；采样期不跑重活；结束 taskkill 收干净并查新旧 exe 名。

### 5.2 完整探测：工具与代码地图

| 类别 | 文件:行 | 状态 |
| --- | --- | --- |
| 采样工具 | 主仓 `app/scripts/MemSample.psm1`、`mem-sample.ps1`、`mem-report.mjs`、`mem-ab.ps1`、`mem-report.test.mjs` | 存在，issue 说「2.0.0 已含可直接用」 |
| 泄漏判据 | `mem-report.mjs:133` `LEAK_MIN_AGE_MIN=30`、`:166` `LEAK_MIN_WINDOW_MIN=30`、`:175-180` 两道拦截文案、`:328` 输出 `dock 私有/GPU 私有/renderer/通过|不通过` | 存在 |
| GPU 列 | `mem-report.mjs:155` `gpuPrivateGrowthMb` | 存在（#30 要的差值由此出） |
| 隐藏即休 | `app/src-tauri/src/power.rs:16/245-254` `resting()`；`bars.rs:467`、`island.rs:1009/1079`、`main.rs:856`、`pet.rs:294` | 2.0.0 已含 |
| iframe 退场 | `app/src/pet/extension-page.js:17` `PARK_DISCARD_MS=5*60*1000`、`:37` 注入参数、`:128/141` park 与计时 | 2.0.0 已含 |
| **预算原文** | issue 引用的 `红线-禁止屎山十条.md` | **仓库内 glob 无结果**；可用口径散见 `docs/audit-dock-2.0.4.md`、`docs/PERFORMANCE.md`、issue 里的表格 |
| 本机形态 | `installed-plugins.json` 3 插件；catalog 2.2.0 | 满足「装好 + 有插件」 |

### 5.3 计划（执行清单，不改产品代码）

1. **前置阻塞**：拿到 `红线-禁止屎山十条.md` 或由 YG 确认以 `audit-dock-2.0.4.md` / issue 表格为预算口径，逐条登记阈值（dock 私有、GPU 稳态 ≤260MB、30→60min ≤+24MB、句柄等）。
2. 采样脚本参数化：进程清单（host、pet renderer、岛 renderer、插件页 iframe、gpu-process、browser、storage、audio、crashpad、plugin-runtime/backend），间隔 ≤20s，5/30/60min 三档；`mem-report.mjs --at 30,60` 出判定。
3. iframe 专验：T0（关面板）→ T0+3min → T0+5min±30s → T0+6min 四个时间点，按 issue 的 5 个数字断言；重开功能验证。
4. 报数：退场前后 GPU 差值（只报数）；采样期间标注每个点在干什么；CSV 落 `docs/screenshots/` 或 `.work/`（一次性产物）。
5. 收尾：`taskkill` 后 `Get-Process` 查新旧 exe 名残留数，确认只有一个实例。
6. **判定 FAIL 才进入修复子计划**（不在本 plan 内展开）。

### 5.4 风险与未决

- 预算原文缺失是硬阻塞；没有它「逐条 PASS/FAIL」不可信。
- 长跑需 60+ 分钟不跑重活（出图/构建/压测），需 YG 安排窗口。
- 采样本身会轻微扰动内存，需标注。

---

## 6. #30 GPU 常驻 217–246 MB 归因（P2，依赖 #29）

### 6.1 诉求

在 #29 拿到「iframe 退场前后 GPU 差值」后：差值 ≥50MB → 查退场是否真释放合成资源；<50MB → 直接做三组可区分实验（4K@150% 合成表面、`#goo` feGaussianBlur、backplate acrylic/mica），每组两次取样、只测不改默认值；结论回答「合成表面/滤镜/backplate 各占多少、剩多少无法归因」。约束：不许砍功能、不许抬阈值。

### 6.2 完整探测：可切换锚点

| 实验 | 代码锚点 |
| --- | --- |
| a 合成表面 / 缩放 | 主仓 `app/src-tauri/src`（DPI/显示器相关）；前端 `app/src/pet/mica.js`（`mica.set(paper, w, h, mode)`）、`bar.css:38-83` `#mica`、`:52-58 canvas`、`style.css:1292-1317` `#mica`、`:2697-2727` `tb-clear-mica`、`island.js:3906-3922` micaBox / `mica.set` |
| b `#goo` 滤镜 | `app/src/pet/index.html:35` `feGaussianBlur stdDeviation=9`、`style.css:467-511` `#goo` + `filter:url(#goo-filter)`、`island.js:245-247` gooIsland/gooNeck/gooDrop |
| c backplate | `app/src/pet/island.js:1141-1156`（acrylic/plate 说明）、`bridge.js:88`（backplate.rs 注释）、`style.css:2891-2951`（island-acrylic-plate）、`settings.css:1984`、Rust `backplate.rs`（acrylic 实现）；外观预设 `settings.js:571-604`（mica/clear/solid 三组） |
| 采样 | `app/scripts/mem-report.mjs:155` `gpuPrivateGrowthMb`、#29 的 CSV |

### 6.3 计划（实验设计，不改默认值）

1. 取 #29 的 GPU 差值；按 ≥50 / <50 分支记录结论依据。
2. 三组实验各准备**可回滚的临时开关**（不进产品默认值）：
   - a：主屏缩放 100% 或副屏 2560×1600 跑一轮（记录 GPU 私有差、视觉差异）；
   - b：临时 CSS 覆盖关掉 `#goo` 的 `filter`（或 `stdDeviation=0`），两次取样；记录视觉差异；
   - c：外观切纯色 backplate（`settings.js` 的 solid 预设），两次取样；记录视觉差异。
3. 每项记录：取样时间、GPU 私有、进程年龄、当时在干什么、回滚方式（如何恢复默认值）。
4. 结论模板：四栏表（合成表面 / 滤镜 / backplate / 无法归因）+ 无法归因部分的下一步手段（D3D 调试层 / WebView2 tracing），拿不到就写拿不到。
5. 任何「默认值改动建议」单独列出，附视觉对比，交 YG 决定，**不因降数字砍功能、不抬阈值**。

### 6.4 风险与未决

- 依赖 #29；#29 的预算原文缺失会顺延。
- 三组实验互相干扰（缩放会影响 mica 位图尺寸），需固定顺序、每项恢复后再测下一项，并做对照轮。
- WebView2 GPU 进程内存归因本身有不可分部分，结论要允许「无法归因」。

---

## 7. 跨仓归属与排期

| 条目 | 主仓 | 插件仓 | 真机/人工 |
| --- | --- | --- | --- |
| #68 | ✅ 全部 | 目录 `versions` schema（方案 A 时配合） | 测试目录 URL 回归 |
| #69 | ✅ 全部 | — | 视觉截图确认 |
| #70 | ✅ S1/S2/S3 | （2.1.9 已修，无需再动） | ✅ **S0 重启实测（前置）** |
| #71 | issue 归档说明 | ✅ 全部实现 + 发版 | 老数据根验收 |
| #29 | 工具已有 | — | ✅ 60min 长跑 |
| #30 | CSS 实验开关（临时） | — | ✅ 三组实验 |
| 附加 A | ✅ `sources.rs` / `updates.rs` 接入 + 工作区 `AGENTS.md` §八常驻 | 目录 `versions` schema（Gitee 发版后配合） | ✅ 免费渠道实测矩阵 |

**建议批次**
- 批 1（可立即开工，互不冲突）：#68（P0）+ #71（插件仓，独立发版）。
- 批 2：#69（前端，与 #68 的 `failureHtml` 有同文件接触，排在同批或之后避免冲突）；#70 的 S0 实测并行。
- 批 2'：**附加 A** 与 #68 共用版本候选/摘要口径，紧随 #68 落地；`AGENTS.md` §八已先写好路径与约定，随同批提交带上（见 §七提交约定）。
- 批 3：#29 → #30（严格串行）。

## 8. 验收总表（进入实现后逐条勾）

| # | 验收命令 / 证据 | 通过条件 |
| --- | --- | --- |
| 68 | `cargo test plugin_store`（新增回退/探活/摘要夹具）；测试目录 URL 真机安装 | 三源 404 自动装上一可用版本、摘要仍校验、面板含 URL/status/phase、notice 写明回退 |
| 69 | CDP 量尺寸 + 事件注入 | 展开区 ≤120px、间距 ≤40px、标题 17px/600、弹窗锁滚动且恢复 scrollTop |
| 70 | 重启后 `plugin-failures.log` / `recent.log` / `run-state.json` | 无新增失败；若有，记录含 phase/attempt/elapsed；开机任务 XML 与 exe 路径一致 |
| 71 | 老数据根升级后 4 个文件 + `preinstall.test.mjs` 新用例 | meoo spec / 包 / provider 三者齐；状态文件可信；不重复不误删 |
| 29 | `mem-sample.ps1` + `mem-report.mjs --at 30,60` + CSV | 4 条逐条给时间戳与数字；iframe 时间点断言全过 |
| 30 | 三组实验取样表 | 四栏归因结论 + 无法归因部分的下一步手段 |
| 附加 A | 来源矩阵实测表 + `cargo test sources` / `updates` 单测 | 每类资源写出候选/超时/摘要/失败回退；Gitee 接入后同版摘要冲突不装；默认空配置不变；`AGENTS.md` §八路径与仓库实际一致 |

## 9. PLAN 阶段声明

- 本文只写到 `docs/plan-issues-2026-11-04.md` 一份新文档 + 工作区 `AGENTS.md` 的 §七提交约定与 §八 Gitee 双仓路径；**未修改任何实现代码、未改 catalog、未提交 git、未发布任何版本**。
- 所有「方案」均为待批准计划；其中 #70 明确要求先做 S0 真机复核再决定是否改码，#29 有「预算原文缺失」前置阻塞，#68 的 S1-A 需要插件仓配合。
- §10 是 2026-11-04 后补的附加条目（Gitee 双仓 + 国内镜像下载历史欠账），同属本 PLAN，未批准前同样不动实现代码。
- 按工作区 `AGENTS.md` §七：本文件与 `AGENTS.md` 的改动**随下一次代码提交一起带上**，不单独落 commit，提交信息注明包含 `AGENTS.md`。
- ⚠️ **`AGENTS.md` 位于工作区根 `FISH-Dock-1104/`，该目录本身不是 git 仓库**（两个 git 仓库是其下的 `FISH-Dock/` 与 `FISH-Dock-Plugins/`），所以它不会被 `git add` 自动带上。落地方式：本文件在仓库内完整收录了 §八的路径表、配置键表与提交约定（见 §10.2/§10.5），**后续提交以本文件为载体把这些约定带进版本历史**；工作区根的 `AGENTS.md` 仍是给后续会话看的主文件，提交信息注明包含 `AGENTS.md`。
- 待 YG 回复「按此计划实施 / 调整第 X 条」后，再按批次进入实现、测试、发版与 issue 回填。

---

## 10. 附加条目 A：Gitee 双仓与国内镜像下载（历史欠账，纳入本批）

> 无 issue 号，来自 YG 2026-11-04 的追加要求：「把 gitee 双仓路径补入本工作区 agent.md，之后提交也要带着」+「dock 内之前没做的国内镜像下载也可以做入了」。
> 本节与工作区 `AGENTS.md` §八互为索引：§八写**常驻路径/配置/提交约定**，本节写**实现计划与验收**。

### 10.1 范围

1. 记录 Gitee 双仓路径并常驻到工作区 `AGENTS.md`（已完成，见 §八）；
2. 把 2.0.4 遗留的「国内镜像下载」欠账正式纳入本批计划（本节），与 #68 的版本候选/摘要框架共用口径；
3. 明确哪些能立刻接、哪些只能标缺口：**插件仓 Gitee 目前没有发行版本**，不能把它写进插件下载链。

### 10.2 完整探测

**双仓实测**（2026-11-04，node TLS 直连 + Gitee 匿名 API；20:0x 与 21:2x 两次探测，结论以 t5 的 `domestic-mirror-matrix-2026-11-04.md` §4 为准）：

| 仓库 | Gitee 路径 | 探测结果 | 结论 |
| --- | --- | --- | --- |
| Dock 宿主 | `https://gitee.com/Fish-under-sea/FISH-Dock` | 仓可见；`releases` API 返回 `[]`；`/releases` 页面 HTML 里 `releases/download`、`releases/tag` 命中 0；`releases/tag/v2.0.4` 与 `releases/download/v2.0.4/{Setup,Update,*.sha256,SHA256SUMS.txt}`、`raw/main/SHA256SUMS.txt` **全部 404**；**tags 13 个（v1.0.0…v2.0.4）** | **有代码、无发行附件**→ 不能作下载源；接上前置动作：在 Gitee 建发行版本并上传与 GitHub 同名的四个附件 + `SHA256SUMS.txt` |
| 官方插件 | `https://gitee.com/Fish-under-sea/FISH-DOCK-Plugins`（大小写两种写法均试） | 20:0x 匿名 API 曾 200 + tags 5 个；21:2x 复测 `api/v5/repos`、`/releases`、`/tags` 全 **404**、`raw` **403**（浏览器 UA 亦 403） | **可见性冲突（私有仓表现），需人工网页确认**；无论如何无发行版本 → **不接下载链** |

**10.2.1 实测推翻的三条旧前提（写进报告，不得沿用旧结论）**

1. **「Gitee 主仓可作资源下载」不成立**：只有 tags，没有 Release 附件，所有具体附件 URL 404。`url.gitee_repo` 保持默认空，`updateFrom=gitee` 配了也拿不到字节。
2. **「插件仓 Gitee 匿名可读」存疑**：同日两次探测结果相反（200 → 404/403）。文档按「可见性待人工确认」记录（工作区 `AGENTS.md` §8.1 已同步改写）。
3. **`catalog/stable.json` 已写 2.2.0，但 npm 两源没有 2.2.0**：npm 官方 latest 2.1.5、npmmirror latest 2.1.5、第三方 2.1.6，2.2.0 tarball 404，只有 GitHub Release 有 v2.2.0 —— 这正是 #68「三源里两源必失败」的真实形态，t1 的回退逻辑因此有实际用武之地；需补发 npm 包才能让三源都通。

**主仓已有的镜像/择源机制**（不是从零做）：

| 能力 | 代码 | 默认值 |
| --- | --- | --- |
| 配置键 `url.raw_mirror` / `url.gh_proxy` / `url.gitee_repo` / `url.pet_mirror` | `config.rs:96-101`（四个环境变量）、`:255`（Key 表）、`:408`（getter）、`docs/config-keys.md:25` | 全空 = 不加候选；`GITEE_REPO` 空即不启用（`config.rs:710-713` 有断言） |
| 镜像候选 `picks(url)` | `sources.rs:142-173`：jsDelivr raw（**开箱即用的结构化改写**，`:146-148`）、raw 前缀镜像、npm 镜像、Release 加速前缀、宠物镜像 | jsDelivr 无条件；其余「配了才启用」 |
| 一次下载的顺序 `candidates(url)` | `sources.rs:175-194`：原地址 + 镜像，按 `mirror.prefer`（`auto`/`domestic`/`overseas`）排 | `auto` = 原地址优先、镜像兜底 |
| 共用 deadline / 失败回退 | `sources.rs:430/456/476`、`plugin_store.rs:102-107`（`download` 走 `sources::fetch_bytes_with`） | 一个请求总 deadline 跨所有候选，不逐候选重置 |
| 更新器 Gitee 候选 | `updates.rs:303-313` `gitee_found`（读 tag + `.sha256`）、`:461-475` `endpoints()`、`:507` 分派、`:580-601` 摘要冲突不装、`:1042-1075` 降级与「未配置」提示 | `config::gitee_repo()` 空 = 不启用 |
| 公告 Gitee 源 | `updates.rs:64/779-849`（`FISH_DOCK_NOTES_GITEE`，未配置时 `Skipped{why:"未配置"}`） | 空 |

**历史欠账原文**（这批要求的出处）：

- `docs/plan-dock-2.0.4.md:300-311` §7.2 映射表：`api.github.com/.../releases/latest → Gitee API（需 url.gitee_repo）配了才启用`、`github.com/.../releases/download/… → {url.gh_proxy} 默认空`、`codex-pets.net → {url.pet_mirror} 默认空`、**「Windhawk / LHM 没有已确认的免费国内镜像」**、`:309` 明确「不得伪造镜像」；
- `docs/plan-dock-2.0.4.md:324-329` §7.3.2：`raw CDN 仅是候选，不保证国内可达`；**`:329`「无 Gitee、pet mirror 或 Release 加速配置时，尚未满足『每类资源默认国内外双源』；先调研现有免费渠道的真实覆盖、字节一致与可用性，再列实测来源矩阵。未找到时明确标记缺口，不能把默认空前缀算作已完成，也不要求用户部署服务」**；
- `docs/release-2.0.4.md:93`：**「免费国内镜像缺口：pet、Release、Windhawk、LHM 均无已确认的免费国内镜像；Gitee 通道需自行配置」**；
- `docs/plan-dock-2.0.4.md:490`：`gitee-release.ps1` / `app/relay/*` 里指向上游 `zhoupengjie/Wakuwaku` 的活动操作要退役或参数化，**默认禁止向上游仓库发布/开 issue**；
- `docs/plan-dock-2.0.4.md:313-322` §7.3/7.3.1：各网络出口清单（`fetch.rs:139`、`plugin_store.rs:101-102`、`updates.rs:1399`、`windhawk_collection.rs:52`、`lhm_setup.rs:937/950`、`sensors.rs:977-1009,1049`）与「宠物预览不经 Rust 择源层」的坑。

### 10.3 根因判定

欠账不是「没有代码」，而是三件没做完：

1. **没有实测来源矩阵**：所有镜像前缀默认空，`mirror.prefer=domestic` 在没有候选时等于空转；文档自己要求「先调研真实覆盖、字节一致」，这一步一直没做；
2. **Gitee 通道默认不启用且未绑定到本仓**：`url.gitee_repo` 空；`gitee-release.ps1` 默认目标仍是上游仓库，向本仓 Gitee 同步发行的链路没有收口；
3. **插件仓 Gitee 无发行版本**：`catalog/stable.json` 的 `url` 仍全部指向 GitHub Release；把 Gitee 路径写进插件三源会直接制造 #68 那种「目录抢跑」故障。

### 10.4 实现计划（主仓为主，5 步）

**S1 免费渠道实测矩阵（先做，不改产品默认值）**
- 逐项测：Gitee 主仓的 tag + `releases/download/<tag>/<exe>.sha256` 与 exe 实际地址（参照 `updates.rs:303-313` 的拼法与 `fake-release.js:121-135` 的 302 行为）、jsDelivr raw（`cdn.jsdelivr.net/gh/...`）、npmmirror（已有）、gh-proxy 前缀、`FISH_DOCK_PET_MIRROR` 候选、Windhawk / LHM 原地址。
- 每项记录：**URL 形状、是否匿名可达、与 GitHub Release 的 `sha256` 字节是否一致、要不要令牌、实测速率、区域顺序位置、失败回退行为**；不一致或无免费渠道的写「缺口」，**不编数字、不伪造镜像**。
- 产出 `docs/domestic-mirror-matrix-2026-11-04.md`（或并入本节附表），作为 `plan-dock-2.0.4.md §7.3.2` 那条欠账的收口证据。

**S2 主仓 Gitee 资源接入（配了才启用）**
- `updates.rs:461-475` 的 `endpoints()` 在 `config::gitee_repo()` 有值时加入 `https://gitee.com/Fish-under-sea/FISH-Dock`；沿用现有 `gitee_found`（tag + `.sha256`）与 `merge`（`:580-601` 同版摘要冲突不装、一侧超时标降级）；`updateFrom=gitee` 从此真有效（`plan-dock-2.0.4.md:336` 的待办）。
- `sources.rs::picks` 增加「Gitee 附件」候选映射（仅当 `url.gitee_repo` 已配置且目标资源在 Gitee 有同版附件时），并复用 `candidates()` 的区域排序与共用 deadline；`gh_proxy` 白名单与 `secure_url` 判据（`sources.rs:198+`）不变。
- `scripts/gitee-release.ps1`：按 `plan-dock-2.0.4.md:490` 把默认 `zhoupengjie/Wakuwaku` 退役/参数化到本仓 `Fish-under-sea/FISH-Dock`，令牌仍走 `GITEE_TOKEN` 环境变量；**不带令牌运行时只校验不写入**（沿用脚本既有约定）。

**S3 插件仓：不接入下载链**
- 插件仓 Gitee 目前 `releases=0`：**不写进 `catalog/stable.json`，不加入 `plugin_store.rs:253-255` 的三源**。路径只作为代码同步/后续发版入口常驻在 `AGENTS.md` §八。
- 若将来在 Gitee 发插件版：按 #68 §1.4 S1 的「版本候选 + 摘要可信链」作为**第四来源**接入（候选摘要必须来自该 Gitee 发行的 `SHA256SUMS.txt` 或目录历史条目），不直接塞进现有三源；`catalog` 的 `url` 权威不变。

**S4 其余资源与硬约束**
- pet 预览按 `plan-dock-2.0.4.md:317-322` 接入共享择源（避免前端裸发原站图片请求，**不放宽 CSP / 下载域名白名单**）。
- Windhawk / LHM：没有已确认免费镜像就保持「原地址 + 系统代理」，在矩阵里标缺口；`sensors.rs:977-995` 的固定摘要与提权进程来源约束不变（`plan-dock-2.0.4.md:326`）。
- **默认空配置不变、不要求用户部署服务**；`mirror.prefer` 行为不变（无候选时 `domestic` 自然回落原地址）。

**S5 文档与提交**
- `AGENTS.md` §八已写好双仓路径、配置键表、欠账出处与「不写进 catalog/三源」的硬约束；`docs/config-keys.md` 补一行「Gitee 双仓路径与默认值口径」。
- 按 `AGENTS.md` §七：`AGENTS.md` 与本计划文档**随下一次代码提交一起带上**，提交信息注明包含 `AGENTS.md`；plan 阶段不单独落 commit。

### 10.5 验收映射

| 检查 | 命令 / 证据 |
| --- | --- |
| 双仓路径真实 | `GET https://gitee.com/api/v5/repos/<owner>/<repo>` → 200 且 `full_name` 与 `AGENTS.md` §八一致 |
| 来源矩阵 | `docs/domestic-mirror-matrix-2026-11-04.md`：每类资源一行（候选/超时/摘要/失败回退/字节一致结论），缺口如实标注 |
| Gitee 更新通道 | `cargo test updates`：`gitee_repo` 配置后 `endpoints()` 含 Gitee；`updateFrom=gitee` 生效；同版摘要冲突返回 `mismatch` 不装；未配置时设置页显示「未配置」 |
| 择源不变形 | `cargo test sources`：`auto/domestic/overseas` 顺序、原地址永远在候选里、镜像失败回落、一个 deadline 跨候选（`sources.rs:642` 既有断言） |
| 插件链不受污染 | `catalog/stable.json` 三字段仍指向 GitHub Release；`plugin_store` 三源仍是 npm 官方 / npmmirror / GitHub；单测断言 Gitee 路径未出现在三源 URL 里 |
| 摘要可信 | 回退/镜像装上的 `.fish-plugin` 与 `catalog` 的 `sha256` 逐字节一致；Gitee exe 与 `<exe>.sha256` 一致 |
| 文档常驻 | `AGENTS.md` §八与 §七提交约定存在；提交记录包含 `AGENTS.md`（⚠️ 工作区根不是 git 仓库，`AGENTS.md` 本身不进 git；由本文件在仓库内收录其全部路径/配置/约定，见 §9 的说明） |
| 提交信息 | 下一次代码提交的 message 里写明包含 `AGENTS.md`，且 `git show --stat` 能看到本计划文档 |

### 10.6 风险与未决

- Gitee 匿名 API 有速率限制（`DEVELOPMENT.md:609` 记过 60 次/小时 403）：更新检查继续沿用「读 tag 而不是 API」的现有做法，矩阵实测也别高频打 API。
- `gitee-release.ps1` 默认目标是上游仓库（`plan-dock-2.0.4.md:490` 的 T-P 待办），参数化时必须有归属校验，防止误传上游。
- 主仓 Gitee `releases` API 返回 0 与 `/releases` 页面 HTML 含发行版字样不一致：S1 必须人工确认「附件到底在哪、按什么 URL 拼」，**确认前不接进产品默认路径**。
- 插件仓 Gitee 若之后发版，摘要来源与 `catalog` 权威的关系要在方案评审时定（推荐维持 catalog 单一权威，Gitee 只作镜像候选）。
- 本条与 #68 的 `failureHtml` / `plugin_store` 可能改同一文件，排期上紧随 #68 之后，避免冲突。

---

## 11. 实施与验收记录（2026-11-04 合并收口）

本轮按 §0–§10 施工，五条线（#68 / #69 / #70 / #71 / 附加 A）并行、分目录，交叉验证后合并。

### 11.1 各条落地状态

| 条目 | 状态 | 关键改动与验收证据 |
| --- | --- | --- |
| #68 下载回退 | **已落地** | 版本候选（目录历史 → 内置 `KNOWN_GOOD` 12 条）+ HEAD 探活（只对 404/410 定论，403 照旧下载）+ 按来源降级 + `SourceFailure` 补 `url/httpStatus/phase/version` + `kind` 增 `catalog/mismatch`；`InstallNotice.fellBack/catalogLatest` 回退提示。验收：`plugin_store` 22、`plugin_failure` 8、`plugin_install` 29、`plugin_api` 2 全绿；审查链 t6→t18→t19 PASS |
| #69 卡面与浮层 | **已落地** | 展开区 105px（≤120）、日志↔卸载 9px、菜单行 32px、标题 17px/600、行内留白 10px、路径与入口合并一行；滚动锁 `UNLOCKED_SCROLLERS` 三放行区 + 引用计数 + scrollTop 还原；`fellBack` 走 `.note.ok` 成功态。验收：前端夹具 466 passed；审查链 t7→t20→t22 PASS |
| #70 自启失败 | **已落地（代码侧）** | `Failure` 增 `phase/attempt/elapsed_ms`（旧日志可解析）+ `log_finish` 的 ready/stopped 也落盘 + 手动启用 3s 退避重试（**锁外退避**，退避前后各检 `closed/stopping`）+ 注入静态量加锁与 RAII 守卫。验收：`plugin_runtime` 19、`plugin_start` 20 全绿；审查链 t9→t12→t13→t14→t15 PASS + t21 竞态修复 |
| #71 预装登记 | **已落地（插件仓）** | marker 升级 schema 2 带 `baseline`，按基线差集增量补登记（差集空则零写盘）；`registeredFrom` 让状态文件说真话；旧格式/坏 JSON 容忍。验收：`preinstall.test.mjs` 26/26；审查 t8 PASS |
| 附加 A 镜像 | **已落地（配了才启用）** | `config.rs:409` `PLUGIN_RELEASE_BASE`；`sources.rs` 增 `gitee_parts/gitee_asset/picks` 末位候选；`updates.rs` `endpoints()` 保持“配了才启用”+ `allowed()`→`in_repo()` 前缀收窄到边界字符；新增实测矩阵。验收：`sources` 27、`updates` 35（并行连跑 20 次全绿）；审查链 t10→t16→t17 PASS |
| #29 / #30 | **未实施** | 需 60 分钟真机长跑；且 issue 引用的预算原文在仓库内不存在，判定无依据。保持 OPEN |

### 11.2 合并验收（captain 实测）

- 整 crate `cargo check --offline --bin FISH-Dock` → exit 0。
- 全量 `cargo test --offline --bin FISH-Dock` **并行连跑 3 次**：每次 `876 passed; 2 failed; 23 ignored`，失败恒为 `lhm_setup::tests::{a_folder_made_with_her_dacl_is_closed_to_the_user, this_machine_is_looked_at}`（沙箱 DACL `Os{code:5}` 与 Task Scheduler `0x80070003`，既有环境失败）。
- 前端 `node --check` 三文件 exit 0；前端夹具 466 passed（≥ 基线 462）。
- `node app/scripts/check-hardcoded.mjs` **exit 0**（合计 128/128/0、`HTTP_URL 70/70/0`）；`check-contrast.mjs` 无新增违规。
- 插件仓 `node --test preinstall.test.mjs` → 26/26。
- 改动面：主仓 15 文件 +2083/−175，插件仓 3 文件；`catalog/stable.json` 未被改动。

### 11.3 反假绿（本轮教训，已固化）

- t10 发现 t5 自报 `-- updates` 35/0、实测 33/2（并行间歇 ≈60%）；根因是两把测试锁不互斥 + 环境键未清 → t16 统一成一把锁 + Drop 守卫，复验 20 次全绿。
- t16/t17 发现 `#70` 三条用例全量并行 5/5 红、单线程全绿 → t21 加 `MANUAL_ENABLE_TEST_LOCK` + RAII 守卫，复验 3×876/2。
- 结论：**各线自报的 passed 数一律不采信**，合并时只认自己跑出的输出，且必须并行连跑 ≥3 次。

### 11.4 残留（写入 release 说明与 issue 回帖）

1. `exe_name()` 仍拼历史名 `wakuwaku-v<ver>-windows-x64.exe` → **便携版**更新入口 `can_install()` 恒 false（安装版走 `installer_names`，不受影响）。
2. `catalog/stable.json` 已写 2.2.0，但 npm 官方源与 npmmirror 均无 2.2.0 tarball（只有 GitHub Release 有）→ 需补发 npm 包，三源才都通。
3. Gitee 主仓有 tags 无 Release 附件；插件仓 Gitee 可见性两次探测结果相反（待人工确认）。
4. pet 图库 DNS 污染级不可达，无任何已确认镜像；Windhawk / LHM 无免费国内镜像（矩阵 G1–G9）。
5. `.bad` 在整套 CSS 里没有独立颜色规则 → 历史“红色失败”其实不红（独立条目）。
6. `hop()` 同站点域逐跳评估，比旧版略宽（限 GitHub 自有域 + 三重校验）；探活最坏 18 次 HEAD 无整轮总 deadline；`KNOWN_GOOD` 靠人工纪律维护（建议进发版清单）。
7. 并行 repair 会让共享工作树编译中断 —— 同树并行时其他线的 verify 会整段不可用，建议错峰或单开 worktree。
8. 真机未验：`getBoundingClientRect` / 触控惯性 / `#70` 重启自启实测 / 深浅主题 `.note.ok` 实拍。
