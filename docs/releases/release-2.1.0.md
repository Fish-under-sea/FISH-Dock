# FISH Dock 2.1.0

> 定稿：2026-11-04。宿主 **2.1.0**，兼容插件 2.x（官方目录当前 2.2.0：Clash / Magpie / 飞书）。
> 本版收口主仓六个 OPEN issue 里的四条 + 一条历史欠账，并补齐 2.0.4 的两处遗留。

## 一、插件下载：三源全灭时按来源回退上一可用版本（#68）

目录里的版本跑在制品前面时（目录写了新版、npm 与 Release 还没有），用户过去只会看到「检查网络或代理后重试」——而网络是好的。

- **版本候选**：优先读官方目录的版本历史条目；目录没有该字段时用编译期内置表 `KNOWN_GOOD`（12 条，摘要取自官方发行，经 GitHub Releases API 的 `assets[].digest` 逐条核验 **12/12 一致**）。比目录更新的内置条目不作回退目标。
- **下载前探活**：对每个来源每个候选做 `HEAD`，只对 **404/410** 定论「制品不存在」；**403 不定论**，照旧发起完整下载（限流或 CDN 拒 HEAD 证明不了缺制品，一次限流不该把可用来源判死）。
- **按来源降级**：`fetch_from` 改为「逐来源 × 逐候选（新→旧）」，某条来源最近可用版本成功即返回，并带出实际装上的 `effective_version`；三源都失败时把各候选的 attempts 一并交给面板。
- **界面保留新版**：目录数据一个字节不改，卡片照常显示最新版；装完后提示「已安装 vX；目录最新 vY 暂不可用」，走**绿色成功态**而不是红色失败。
- **摘要零放宽**：期望摘要只取候选自带的官方目录摘要，装上的 digest 绝不等于自算摘要；错摘要按该来源失败处理并继续下一候选。
- **跳转判据修正**：`hop()` 从「只认两个 GitHub 资产域」放宽到「官方发行资产域 **或** 与当前请求同站点域」。这修掉了一个真实缺陷——npmmirror 的 tarball 的 `HEAD` 与 `GET` **都** 302 到 `cdn.npmmirror.com`，此前一律被拒，于是「三源」实际只有两源可用，而国内用户最需要的恰好是死掉的那条。仍要求 HTTPS、拒绝跨域与端口、拒绝后缀伪装（`npmmirror.com.evil.example` 照样拒）。
- **可诊断性**：结构化下载失败按内容语义记进 `recent.log`（恰好一条，不重复），探活判死补 stderr；失败面板逐源显示 **来源 · 结论 · HTTP 状态码 · 失败阶段 · 可复制 URL**，`title` 保留原始技术串。

## 二、插件卡与浮层（#69）

- 「更多」展开区从约 210px 压到 **105px**（目标 ≤120），日志↔卸载 **9px**（目标 ≤40）；菜单行 32px 整行可点、组内仅一条分隔线、卸载置组末且常态红。
- 目录名与功能页入口**合并成一行**（12px 次级色 + 省略 + 点击复制）。
- 已安装卡标题 **17px/600**，状态点 6px、状态↔版本 8px，行内最大留白 10px。
- **Dock 弹窗锁滚动**：确认弹窗打开时底层列表不再被滚走；进度浮层与失败块在锁定期仍可滚动（`UNLOCKED_SCROLLERS` 具名清单），关闭/取消/Escape/点背景四路都解锁并还原 `scrollTop`。
- 新增「插件卡与浮层视觉规范」注释块（字号/行高/间距/危险色/滚动锁口径），防 #63 式反复。

## 三、自启失败可定位（#70）

预算与重试调度在 2.0.4 已存在（冷态一次尝试共用 60s、整轮 180s、每插件最多 4 次尝试），本版只补**可观测性**与**对称性**：

- `Failure` 增 `phase`（verify/spawn/hello/activate/service）、`attempt`、`elapsedMs`（增字段，旧 `plugin-failures.log` 仍可解析）。
- `log_finish` 的 ready/stopped 也落盘（复用单一写入路径，failed 不重复）。
- **手动启用**首次失败后补一次 3s 退避重试：退避在**锁外**做（不会冻住插件页 3 秒），醒来重取锁并复检 `closed/stopping`/代次。

> ⚠️ 这条的**真机重启自启实测仍待执行**（装本版后重启，看 `plugin-failures.log` 与托盘）。代码侧已闭环，真机结论未取。

## 四、国内镜像与 Gitee（历史欠账）

- `url.gitee_repo` 仍是「配了才启用」，默认空；`updates::endpoints()` 在配置后加入 Gitee 候选。
- `sources::picks` 增 Gitee 附件候选（仅配置时生效），`candidates()` 排序语义与共用 deadline 未改。
- **行为变化（请留意）**：`allowed()` 从纯前缀比较收窄为「前缀 + 边界字符」。原先配 `…/FISH-Dock` 会连带放行 `…/FISH-DOCK-Plugins`（同名前缀），现在只放行边界匹配的地址。**只收窄，不放宽。**
- 新增 `docs/guides/domestic-mirror-matrix-2026-11-04.md`：六类资源逐项实测。结论摘要——raw 文件是唯一开箱即有免费国内镜像的一类（`stable.json` 在 raw.githubusercontent 与 jsDelivr 三域名字节与 sha256 完全相同）；npm 官方与 npmmirror 的 tarball 四项校验全同；**pet 图库 DNS 污染级不可达且无任何已确认镜像**；Windhawk / LHM 无免费国内镜像。缺口如实标注，不编地址、不要求用户自建服务。
- **Gitee 现状（不吹）**：主仓 Gitee 有 13 个 tags 但**没有任何 Release 附件**（`releases` API 返回 `[]`，具体附件 URL 全 404）；插件仓 Gitee 的匿名可见性两次探测结果相反（待人工确认）。因此本版**仍以 GitHub Release 为唯一发行源**，Gitee 通道保持默认关闭。

## 五、安装包

| 安装包 | 使用场景 |
| --- | --- |
| `FISH-Dock-Setup-2.1.0.exe` | 首次安装、基础资源缺失或修复安装，包含完整离线运行时。 |
| `FISH-Dock-Update-2.1.0.exe` | 已有版本且基础资源完整时使用，只含相对 **2.0.4** 的变更；缺失或损坏时提示改用完整包。 |

> **精简更新包只对 2.0.4 有效**：它是差分包，跨版本使用会缺文件。更早的版本请用完整安装包。

## 六、已知限制与残留

1. **便携版更新入口仍不可用**：`updates::exe_name()` 还在拼历史文件名 `wakuwaku-v<ver>-windows-x64.exe`，而真实发行名是 `FISH-Dock-{Setup,Update}-<ver>.exe` → 便携版的 `can_install()` 恒为 false（安装版走 `installer_names`，不受影响）。列为独立条目。
2. **npm 两源仍无新版**：`catalog/stable.json` 已写 2.2.0，但 npm 官方源与 npmmirror 的 latest 仍是 2.1.5/2.1.5/2.1.6、2.2.0 tarball 404，只有 GitHub Release 有。插件更新目前依赖 GitHub 一条腿；回退逻辑正好覆盖这种情况。
3. **真机未验**：卡面尺寸（`getBoundingClientRect`）、触控惯性下的滚动锁、深浅主题的 `.note.ok` 对比度、以及 #70 的重启自启实测。
4. **`.bad` 无独立颜色规则**：整套 CSS 里只有组合选择器带红，所以历史上那些“红色失败”其实不红。属既有观感问题，独立条目处理。
5. **探活无整轮总 deadline**：最坏 3 源 × 6 候选 = 18 次 `HEAD`；单次 fetch 内共用 deadline，整轮候选没有。
6. **`KNOWN_GOOD` 靠人工纪律维护**：表过期后回退能力会静默退化到“只有目录当前版”。
7. `hop()` 的同站点域是**逐跳**评估，比旧版略宽（仍限 GitHub 自有域 + 目录 sha256/size/manifest 三重校验）。

## 七、验证

- 整 crate `cargo check --offline --bin FISH-Dock` → exit 0。
- 全量 `cargo test` **并行连跑 3 次**：每次 `876 passed; 2 failed; 23 ignored`，两条失败恒为 `lhm_setup::tests` 的沙箱 DACL 与 Task Scheduler 环境问题。
- 前端：`node --check` 三文件 exit 0；前端夹具 466 passed（基线 462）。
- `node app/scripts/check-hardcoded.mjs` → **exit 0**（合计 128/128/0、`HTTP_URL 70/70/0`）；对比度无新增违规。
- 插件仓 `node --test preinstall.test.mjs` → 26/26。
- 改动面：主仓 15 文件 +2083/−175；插件仓 3 文件；`catalog/stable.json` 未改动。

## 八、插件侧配套

插件仓同期提交了 **#71 预装登记改按基线差集增量补登记**（`plugins/magpie/preinstall.mjs`）：marker 升级 schema 2 带 `baseline`，老用户升级后能拿到 2.1.4 才进基线的 Meoo；状态文件的 `registered/added` 改为按实际清单计算。**该修复要等插件仓发新版才到用户手里**，本版宿主不包含它。