# 2026-10-28 合入与修复方案：PR #14 / #15、GitHub 认证持久化、配置备份

本文只写方案与依据，不改动代码。每条结论都标了本地实证位置，便于逐条核对。

---

## 0. 摘要

| 事项 | 结论 | 风险 |
| --- | --- | --- |
| PR #14 灵动岛窗口铺满 | 逐行复核通过，改动 2 文件、语义自洽；合入前补三项实测（指针 `near` 范围、`place` 断言、`notch` 路径） | 中 |
| PR #15 连接判断 + 账单分账 | 连接修复正确；账单分账有 1 处必改缺陷（`selAgent` 跨列表重置）、1 处未清理告警、2 处格式问题 | 中高 |
| GitHub 认证掉线 | 根因是「会话过期后被动刷新 + 401 即清凭据 + 无启动续期 + 结构 `deny_unknown_fields`」，不在更新流程 | 高 |
| 配置备份导出/导入 | 导出范围按四类目录硬编码、DPAPI 解密失败即整包失败；导入按插件版本严格相等拒绝 | 高 |

合入顺序：先认证与备份（用户可感知的缺陷），再 #14，最后 #15。三者互不依赖；#14 与 #15 虽同改 `island.js`，实测合并无冲突，可顺序直合。

---

## 1. 远端 PR 现状

`gh pr list` 回读（2026-10-28）：

| PR | 标题 | 作者 | 基线 | 变更 |
| --- | --- | --- | --- | --- |
| #14 | fix(灵动岛): 窗口铺满工作区宽度，中/右锚点切换宽窄页不再抽搐 | elysia395 | `4d46106`（= 当前 `main`） | 2 文件 +32/−11 |
| #15 | fix(连接) + feat(token): 未连接判断与账单按 agent 分账 | elysia395 | `4d46106` | 12 文件 +567/−60 |

#12 已关闭，其内容拆分为 #14 与 #15。#14、#15 均为 `MERGEABLE`，无 draft。

**逐行 diff 复核待网络恢复后补做**：当前 GitHub 出站被本机 Clash 节点故障阻断（`api.github.com`、`codeload.github.com`、`raw.githubusercontent.com` 全部 `ECONNRESET`，控制器命名管道被沙箱拒绝），`gh pr diff` 与 `git fetch` 均失败。上表数据来自故障前成功的 `gh pr view`，含完整正文、文件清单与提交列表。

### 1.1 #14 已核对的本地代码

- `island.rs:205 size_for`：非任务栏落地走 `_ => ISLAND`，宽度固定 760。
- `island.rs:291 fit`：island 分支按 `notch::at_top` 或 `wanted.place(&area, size.0)` 落点。
- `island.rs:162 place`：`Right`/`Center` 的 x 由宽度倒推，#14 描述的抽搐成因成立。
- `island.rs:763-765`：岛内元素偏移同样按 `isl.pos` 与 `isl.size` 计算，窗口铺满后仍自洽（`Center` 得到 `area.x + (area.w − w)/2`）。
- `island.rs:1602-1610`：现有断言写死 `Side::Right.place(&area, 950) == (1610, 0)`，窗口不再按宽度落点后这些断言必须同步改写。
- `island.js:1024 baseRoom / 1026 roomFor`：宽度基准仍是 `BASE`，与 #14 的 `baseRoom/pinRoom` 改动对应。

### 1.2 #14 合入前必须补的三项验证

1. **指针 `near` 范围扩大**（`pointer.rs:114 look`）：`Forward` 态按窗口矩形与 `near` 判定，窗口宽度从 760 变为整个工作区后，鼠标扫过整条都会触发 `Forward`，页面每秒收到指针事件。须实测 CPU 占用与岛外区域是否误拦截。
2. **落点断言与 `closed_rect`**：`place`/`closed_rect`/`pinRoom` 全部依赖 `isl.size.0`；窗口恒宽后，`closed`（收起态）与设置页弹层的位置须逐项复测，含负坐标显示器（`Side::Right.place(&left, 760) == (-760, 48)` 一类用例）。
3. **顶部 `notch` 路径**：`fit` 在 `notch::at_top` 命中时把 `isl.at` 强制为 `Center`；窗口铺满后顶部锚点固定 `(area.x, area.y)`，须确认刘海屏与顶部任务栏两种 `area` 下岛内对齐仍按 `style.css` 的 `isl-at-*` 生效。

### 1.3 #15 已核对的本地代码

- `settings.rs:57`：确认仍是内联五渠道判断（claude / codex / agy / opencode / omp），缺 ZCode、DSH、Reasonix → 只连 ZCode 的用户被反复改回「未连接」。`connection::any_connected` 已存在且已含完整八项，PR 的修法正确且最小。
- `tokens.rs:42 Tokens`：只有 `claude` / `codex` / `agy` 三份账；`tokens.rs:235 words()` 只输出 `total` 与 `output`，无 `agents`。PR 新增 `zcode` / `opencode` / `dsh` 与 `agents` 列表，结构改动成立。
- `Cargo.toml`：当前无 `rusqlite`，也无 `zstd`。PR 带来 `+5` 行依赖与 `Cargo.lock +88`。

### 1.4 #15 合入前需先定范围

1. **`rusqlite` bundled**：走 vendored `sqlite3.c`，离线可构建，release 体积约 +1.5 MB，编译时间增加。须在 `THIRD_PARTY_NOTICES.md` 登记 SQLite（public domain），并确认 NSIS 打包与精简更新包的 payload 计算不受影响。
2. **外部路径读取**：新增读 `~/.zcode/cli/db/db.sqlite`、`~/.local/share/opencode/opencode.db`、`DSH_HOME/sessions/**/*.session.v*.jsonl.zstd`。建议统一约束：只读打开（`SQLITE_OPEN_READ_ONLY`）、单文件上限、总耗时上限、异常静默降级为「无账单」，且不进 UI 主线程。
3. **zstd 多帧**：DSH 日志是多帧首尾相接追加，逐帧解码才完整。实测 PR 未新增 zstd 依赖，复用树内已有的 `ruzstd =0.9.1`（`dsh_text` 里 `StreamingDecoder::new_with_max_window_size`，window 上限 64 MiB）。
4. **`island.js` 冲突**：实测无冲突。`git merge-tree --merge-base=main pr-14 pr-15` 退出码 0，两 PR 改的是不同区段（#14 在 681-700、1020-1045；#15 在 757、5890-5915），可顺序直合。
5. **前端联动**：`words()` 增加 `agents` 后，`wcards.js`、`wdesk.js`、`widgets.js`、`island.js` 与 `i18n.js` 需同时改；PR 正文提到「放进 `vars` 会让页面每秒 TypeError」，说明前端契约脆弱，合入后须跑完整前端测试。实测 PR 已同步改这 5 个文件，并把 `BUILT_SIZES.tokens` 从 `['s','m']` 扩到 `['s','m','l']`。

### 1.5 验收基线（两份 PR 自报，待本地复跑）

- Rust：`plugin_page::tests::page_grants_bind_identity_version_enabled_state_and_lifetime` 在未改动上游同样失败（`Instant` 减时长溢出）。
- JS：`test/about-update.test.js` 两例失败，因 `settings.js` 存为 CRLF 而测试按 `\n\n` 切分。

这两处须在合入 PR 之前修好，否则无法区分「PR 引入的失败」与「既有失败」。建议单独提交：修 `Instant` 溢出用例、统一 `settings.js` 行尾。

### 1.6 逐行复核结论

**#14（`island.rs` + `island.js`）**

- `size_for`：`_ => ISLAND` 改为 `(area.w as f64 / sf, ISLAND.1)` —— 窗口物理宽恒等于工作区宽，高度仍为 132 CSS。`room` 分支 `w.max(base.0)` 仍取 `area.w` 为下限，宽度不会因页面请求变窄。
- `fit`：`wanted.place(&area, size.0)` 改为 `(area.x, area.y)`；`Side::place` 失去生产调用点，PR 加了 `#[allow(dead_code)]`，测试断言（`island.rs:1602-1610`）仍在，编译与测试不受影响。
- `island.js` 三处宽度基准全部换成 `innerWidth`：`pinRoom:691`、`baseRoom:1024`、`roomFor:1039`；`BASE.width` 只剩定义处（`:147`），无残留引用。
- `roomFor` 改为「只按高度增长」，返回 `{ width: innerWidth, height }`；`BASE.height` 仍是高度下限。
- 结论：逻辑自洽，可合入。剩下的是行为实测（§1.2 三项）。

**#15（12 文件）**

正确且已核实的部分：

- `settings.rs` 改用 `connection::any_connected(sh.port)`，与启动判定同一份八项列表。
- `tokens.rs` 新增 `zcode` / `opencode` / `dsh` 三份账与 `by_agent()`；`words()` 签名改 `&Tokens`，全仓只有 `main.rs:2012` 一处调用，已同步。
- `zcode_bill` / `opencode_bill` 均 `SQLITE_OPEN_READ_ONLY`，SQL 带当天与 `status='completed'` 过滤；`opencode_bill` 依赖 `json_extract`，bundled SQLite 默认启用 JSON1。
- `dsh_bill` 按 mtime 过滤当天日志、逐帧解码、`MAX_READ = 64 MiB` 上限、帧无进展即退出，不会死循环。
- `update()` 内 `if !cfg!(test)` 跳过真实数据库，测试不读本机账单。
- 重命名完整：`todayCycle` → `cycleAgents`、`todayAnimClass` → `swipeAnimClass`，`pr-15` 分支上已无旧名引用。

必须处理的问题：

1. **`selAgent` 跨列表重置（逻辑缺陷，必改）**：`wcards.js:120` 只用一个 `selAgent`，`agentsOf()` 在列表不含它时执行 `selAgent = list[0].id`（`:128`）。今天卡与 token 卡的 agent 集合不同时（典型：今天有 dsh、token 无 dsh 账单），滑到 dsh 后下一帧 token 卡渲染会把它重置回最忙的 agent，两块来回跳，与 PR 声称的「两块同步」相反。修法二选一：按 kind 分开保存（`sel = { today, tokens }`），或翻页只在两列表交集内进行。
2. **未使用变量（告警）**：`settings.rs` 删掉判断后，`let conn = ...` 与 `let omp = ...` 不再被使用（`settings.rs:54-55`）。`omp_hooks_status` 的副作用需保留时，应改写成 `let _ = connection::omp_hooks_status(sh.port);` 或直接删变量名。
3. **格式**：`tokens.rs` 测试里 `let opencode = dir.join("opencode.sqlite");        let db = ...` 两语句同行；`test/wcards.test.js` 里 `test('a curve, a ring, bars: each to its own scale', () => {  assert.equal(...)` 同样两行挤一行。合入前跑 `cargo fmt` 与前端格式化。
4. **测试有效性**：`dsh_bills_each_answers_usage` 手抄了生产解析逻辑再断言，生产 `dsh_bill` 改动不会让它失败。应抽出纯函数（如 `dsh_sum(&str, day_start)`）供生产与测试共用。
5. **性能待测**：`dsh_bill` 每次 `update()` 全量读 + 解压当天全部日志；单文件有 64 MiB 上限，但多文件无总量上限，且无「文件未变则不重读」的缓存。需实测刷新周期下的 CPU 与内存。
6. **外部读取边界**：新增读 `~/.zcode`、`~/.local/share/opencode`、`DSH_HOME/sessions`。属「越出插件命名空间读用户目录」，需在产品说明里写清，并保证只读、异常静默。

---

## 2. GitHub 认证持久化

### 2.1 现场证据

- 凭据文件 `C:\Users\Fish\.fish-dock\host\keys\github-access.dpapi`（566 B，当前用户 DPAPI），解密后字段齐全：`token`、`refresh_token`、`client_id=Ov23lisbBQRqCOm57AC6`、`login=Fish-under-sea`、`user_id=138899692`、`star_at=star_checked_at=1793159418974`（2026-10-27 20:30）、`expires_at=1793188218511`（2026-10-28 10:30，已过期）、`refresh_expires_at=1808884217511`（2027-04-26）。
- 结论：**凭据本身没有丢**，refresh 仍有效半年；掉的是「可用的 access」，且前端把它呈现成「需要重新登录」。
- `github_access.rs` 现状：无启动续期（`main.rs` 未调用 `github_access`），`invalidate` 在被调 401 且刷新失败时清空全部字段（`github_access.rs:123-127`），`Account` 带 `#[serde(deny_unknown_fields)]`（`:12`）。

### 2.2 根因（四条，按影响排序）

1. **被动刷新**：只有 `require()` / `check()` 触发 `refresh_with`。启动后到用户点插件之间，access 一直过期；期间任何一次 401 都会走 `invalidate` 清凭据。
2. **401 即清空**：`authorized_get` 在「刷新后仍 401」时无条件 `invalidate`。代理抖动、GitHub 边缘节点临时 401、限流都会命中，用户视角就是「重启后掉登录」。
3. **结构严格**：`deny_unknown_fields` 使任何字段增删都让旧文件解析失败，`load` 返回 `Err`，前端显示未登录。这直接威胁「更新新版本凭据不变」。
4. **Star 回执 24 小时**：`receipt_valid` 只认 24 小时，`extension-panel.js:22` 的 `unlocked` 要求 `authenticated && verified`，于是每次开机、每次过期都要联网复核；离线即「锁住官方插件」，用户读成「掉认证」。

### 2.3 方案

**路线 A（推荐，一次性根治）**：GitHub OAuth App 关闭 *Expire user authorization tokens*。
- 关闭后新发的 access token 长期有效，`refresh_token` 不再产生，`session_live` 恒真。
- 代价：需仓库所有者操作一次，且用户须重新授权一次以取得新令牌；旧令牌仍按原期限过期（代码已能续期，过渡期无感）。
- 代码侧保留 refresh 逻辑，兼容尚未重新授权的账号。

**路线 B（不依赖 GitHub 设置，代码内做）**：
1. **结构宽松化 + 版本号**：`Account` 增加 `schema_version`，去掉 `deny_unknown_fields`，字段缺失走默认。解析失败时把原文件另存 `github-access.dpapi.bak` 并保留登录名，不做静默清空。
2. **失效分级**：只有 GitHub 明确回 `bad_refresh_token` / `expired_token` / `invalid_grant` 才清凭据；401 / 403 / 429 / 5xx / 网络错误一律保留会话，标记 `stale`，返回可重试错误。
3. **启动续期**：`setup` 后起一次性后台任务，`needs_refresh` 为真则刷新并落盘；失败只记日志，不改前端登录态。
4. **登录态与 Star 解耦**：`authenticated` 只由 `session_live` 决定；`verified` 只由 Star 决定。UI 文案分三态：「未登录」「已登录 · Star 待复核（离线保留）」「已登录 · Star 已验证」。
5. **Star 回执放宽**：有效期 24 小时 → 7 天，且离线时沿用上次回执（标注「离线复核待联网」），官方插件不因离线被锁。
6. **凭据随配置备份迁移**：导出把 `github-access.dpapi` 记为 `github-session` 便携编码（复用 `mail_secrets` 的 `id → 明文` 结构），导入时用当前用户 DPAPI 重新加密；`client_id` 不一致时保留登录名并提示重新授权续期，而不是清空。

### 2.4 验收

- 单测：结构迁移（旧 JSON 缺字段/多字段均可读）、401 后凭据仍在、`bad_refresh_token` 才清空、离线时 `authenticated=true` 且 `requiresLogin=false`。
- 结构测试：`startup_refresh_rotates_and_persists`（模拟 transport）。
- 人工：断网启动 → 状态为「已登录 · Star 待复核」；联网后自动恢复「已验证」，全程无需重新登录。
- 跨版本：安装 1.2.0 → 1.3.0 更新后直接沿用凭据（数据根 `host/keys` 不随安装变化，`storage_version::change` 只改 README 与 `storage.json`，已核实）。

---

## 3. 配置备份导出/导入

### 3.1 现场证据

- 实际配置仓：`host` 2.32 MiB / 12 文件，`.fishplugin-magpie` 15.72 MiB / 418 文件，`.fishplugin-clash` 0.03 MiB / 5 文件；按 `included()` 规则统计总量约 18.1 MiB，**远低于 128 MiB 上限**，导出不会因体积失败。
- 凭据实况：`host/keys/github-access.dpapi` 与 `.fishplugin-magpie/keys/web-key` 是标准 DPAPI 头（`1,0,0,0,d0,8c,9d,df,...`）；`.fishplugin-magpie/credentials/magpie/*.json`（含 `plugin-auth.json` 72 KB、`logins.json`）是**明文 JSON**；`host/credentials/mail-secrets.dpapi` 走 `mail-secrets` 专用编码。
- `host/exports` 目录当前只有 `README.md`，没有历史 ZIP。

### 3.2 导出侧根因

1. **范围按四类硬编码**：`config_backup.rs:48 included()` 只保留 `credentials` / `roster` / `keys` / `config` 四类，host 再加 `plugins` / `pets` / `mail-draft.json` / `today.json` / `icons.json` / `USER-NOTES.md`。`host/mail-store`（收件箱 `inbox.json` 832 KB、已发送）**完全不在范围内** → 用户看到的「花名册在、邮件没了」。
2. **DPAPI 解密失败即整包失败**：`export()` 遇到 `dpapi()` 命中的文件直接 `unprotect(bytes)?`（`:113-114`）。第三方插件若用 `LocalMachine` 作用域或带 entropy，解密失败会让整个导出中断，用户只得到一句错误。
3. **运行态垃圾混入**：`credentials/magpie-runtime/data` 下包含 `trash/sessions/**`、`routing/*.jsonl`、`usage.jsonl`、`models.json`（5.9 MB）、`*.bak` 等，全部计入导出 → 备份臃肿且含无关内容。
4. **`missingCredentials` 硬编码 0**：`identity_api.rs:77` 直接回 0，导出结果永远报「无缺失」。
5. **导出成功信息不足**：只回 `path` / `bytes` / `sha256`，不报命名空间与文件数，用户无从判断是否「导全了」。

### 3.3 导入侧根因

1. **插件版本必须完全相等**：`config_backup.rs:134` 要求 `existing.plugin_version == owner.plugin_version`，插件升级（如 clash 1.0.0 → 1.0.2）后导入旧备份直接拒绝，报「请先安装对应版本」。这是「导入有问题」的最高频原因。
2. **目标目录无归属即拒绝**：`claimed` 为空且目录非空时回「目标插件目录已有未归属资料」。
3. **host 归属被旧版本覆盖**：备份里的 `host/storage.json` 会被写回目标，宿主 `plugin_version` 倒退，随后 `Context::open` 再触发一次版本迁移。
4. **失败信息笼统**：`activate()` 失败只回「已恢复导入前整个配置仓」，不指认具体文件与原因。
5. **整仓替换语义未提示**：`names = previous ∪ files`，目标机新增的同类文件会被删除，预览页只说「持久文件：N」。

### 3.4 方案

**导出**
- 范围改为「插件清单声明 + 宿主固定集合」：
  - 宿主增加 `mail-store`（邮件本体，可选开关，默认开）、`icon-cache` 之外的持久目录；
  - 插件侧改读 `plugin.json` 的持久目录声明，未声明时退回四类白名单。
- 明确排除：`trash/`、`routing/`、`usage*.jsonl`、`models.json`、`*.bak`、`tmp/`、`node_modules/`、运行时二进制。
- 凭据分级：
  - 标准 DPAPI 可解 → `dpapi` 编码（现状保留）；
  - 解不开 → `opaque` 编码，原样字节写入并在 README 标注「目标机需重新登录」；
  - GitHub 会话 → 新增 `github-session` 编码。
- 导出结果回传 `owners`、文件数、总字节、各类编码计数；`missingCredentials` 改为真实统计。

**导入**
- 版本检查放宽为「`data_schema` 必须相等，`plugin_version` 允许不等」，不等时按 schema 兼容写入并在结果里列出「版本已从 X 迁到 Y」。
- 目标目录非空但备份声明了归属 → 允许接管，需二次确认。
- 跳过 `host/storage.json` 的版本字段覆盖，保留目标机宿主版本。
- 回滚错误带上「文件 + 原因」，并按命名空间分组。
- 预览页显示「将写入 N、删除 M、跳过 K」，与导入结果对照。

**UI**
- `identity-panel.js` 的说明文案改为按实际范围生成（含邮件是否包含、需重新登录的凭据数）。

### 3.5 验收

- 新增用例：
  - 跨插件版本导入成功且提示版本变化；
  - 不可解 DPAPI 仍能导出（`opaque`），导入后不破坏原文件；
  - `mail-store` 在导出包内、导入后收件箱一致；
  - 导入失败回滚后错误信息含文件名；
  - 预览的「写入/删除」计数与实际 journal 行数一致。
- 回归：现有 7 个 `config_backup` 用例（整仓往返、激活失败回滚、新命名空间回滚、中断恢复、windhawk、magpie 便携数据、篡改拒绝）全绿。

---

## 4. 实施顺序

1. **基线修复**（独立提交）：`plugin_page` 的 `Instant` 溢出用例、`settings.js` 行尾统一 → 测试回到全绿，作为后续所有改动的判据。
2. **认证持久化**（路线 B 先行，路线 A 由仓库所有者操作 GitHub 侧）。
3. **备份导出/导入**。
4. **合入 #14**，补三项验证与断言改写。
5. **合入 #15**：先修 §1.6 的 `selAgent` 缺陷、未使用变量与格式，再定 `rusqlite` 与外部读取范围的说明，最后顺序直合（实测无冲突）。
6. 版本号与文档：`1.2.1`（修复类）或并入下一个特性版本，由用户定。

每条改动独立提交，提交信息按 `<type>(<scope>): <中文简述>`。

---

## 5. 已拍板（2026-10-28）

1. **PR #14**：接受「窗口恒为工作区全宽」，合入后补指针 `near` 与收起态实测。
2. **PR #15**：接受 `rusqlite` 与读取 ZCode / OpenCode / DSH 本地数据库。
3. **备份范围**：**不带邮件正文**，只带认证凭据与花名册（`host/mail-store` 明确排除）。
4. **GitHub OAuth App**：关闭 *Expire user authorization tokens*（需仓库所有者在网页端操作一次）；同时保留轮换刷新令牌兜底。
5. **认证语义**：认证一次长期有效；安装/更新到新版本凭据不变；每次启动复核身份与主仓 Star。

---

## 6. 执行记录（2026-10-28）

已合入并验证：

| 提交 | 内容 | 验证 |
| --- | --- | --- |
| `fix(认证)` | `Account` 加 `schema_version`、去 `deny_unknown_fields`；读不懂时另存 `.bak` 而非删除；Star 回执 24 小时 → 7 天；新增 `resume` / `resume_with` 与 `main.rs` 启动复核 | Rust 22 项 GitHub 用例全过，含新增 4 项 |
| `fix(备份)` | 运行态与可重下产物统一排除；解不开的 DPAPI 走 `opaque` 原样带走；导入只要求 schema 相等；跳过 `host/storage.json`；`missingCredentials` 回真实值 | `config_backup` 11 项全过，含新增 4 项 |
| `test(身份)` | 受限令牌下 `private_directory` 只建目录、不收 DACL | 全量 563 通过 |
| 合入 #14 | 灵动岛窗口铺满工作区 | `island::` 8 项、JS `placement/placing/anchor/capsule/flow` 全过 |
| 合入 #15 | 连接判断 + 账单分账 | 合入后补 `selAgent` 不互相重置的回归用例 |

合入 #15 时修掉它自带的三个问题：`wcards.js` 的 `selAgent` 会在另一张卡的列表里被重置（新增回归用例覆盖）、`settings.rs` 遗留两个未使用变量、`tokens.rs` 与 `wcards.test.js` 各有一处两行挤一行；并把 `dsh_bill` 的解析抽成 `dsh_sum`，让单测与生产共用同一份。

**测试环境**：本机 SChannel 在沙箱内取不到客户端凭据（`cargo`/`curl` 报 `SEC_E_NO_CREDENTIALS`），依赖改用 `.tools/vendor-cargo.mjs` 由 Node 取回、展开成 directory source 后离线构建；`%TEMP%` 指到工作区内。全量 Rust 结果 **563 通过 / 2 失败 / 14 忽略**，两处失败是既有的 `lhm_setup` 环境用例（ACL 与 Task Scheduler 根目录），与本次改动无关。

---

## 7. 环境记录（本次操作）

- GitHub 出站：`api.github.com` / `codeload.github.com` / `raw.githubusercontent.com` 经本机 Clash（`127.0.0.1:7897`）全部 `ECONNRESET`，直连同样失败；DNS 仍返回 fake-ip `198.18.0.9`，而 `0.0.0.0/0` 默认路由指向 `198.18.0.2`（metric 0），TUN 转发侧已不可用。
- 控制器 `\\.\pipe\verge-mihomo` 被沙箱拒绝（`Access to the path is denied`），无法从会话内切换节点。
- 恢复方式（任选其一，均需本机操作）：重启 Clash Verge 并切换可用节点；或关闭 TUN 后删除 `0.0.0.0/0 → 198.18.0.2` 残留路由；或重启本机。
- 网络已恢复：`gh pr diff 14` / `gh pr diff 15` 已取回（`.tools/pr-review-20261028/`），分支引用 `pr-14` / `pr-15` 已抓取，合并冲突实测通过（见 §1）。下次网络再次中断时，上述三份产物在本地可用。