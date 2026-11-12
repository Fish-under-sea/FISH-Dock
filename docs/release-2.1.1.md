# FISH Dock 2.1.1

> 定稿：2026-11-05。宿主 **2.1.1**，兼容插件 2.x（官方目录当前 2.2.0：Clash / Magpie / 飞书）。
> 补丁版，两件事：**第三方插件识别排除官方三个插件（#72）**，以及**把自动检查更新里的 Gitee 拉取恢复**。

## 一、第三方插件排除官方三个（#72）

**现象**：官方三个插件在 npm 上也有合规包名——`@fish-under-sea/fish-dock-plugin-feishu`、`…-clash`、`…-magpie`（`plugins/tools/build-npm-packages.mjs` 发布的名字）。第三方页的默认发现词就是 `fish-dock-plugin`，npm 检索因此把这三个官方包排在最前面，当作「可安装的第三方插件」列出来。它们**不是**第三方：身份由官方目录授予（`Source::OfficialCatalog` + 主仓 Star），从第三方通道装进去会被登记成第三方身份，装完也和商店条目对不上。

**改动（只动 Dock，插件仓 `catalog/stable.json` 一字未动）**：

- **官方身份只剩一份定义**：`plugin_store::OFFICIAL_IDS`（`fish.feishu` / `fish.clash` / `fish.magpie`）。目录校验与第三方页排除都读它，不再各抄一份数组；官方 npm 包名也仍由 `plugin_store::npm_package` 构造。
- **判据只认官方那份包名**：`plugin_thirdparty::official_package` 比对 `@fish-under-sea/fish-dock-plugin-<slug>`，大小写无关。无 scope 的 `fish-dock-plugin-magpie`、`@acme/fish-dock-plugin-magpie` 都不是官方包，照常按第三方处理——依据是「官方身份」，不是「名字里带 magpie」。
- **检索两路都排除**：精确包名元数据查（①）与 `/-/v1/search`（②）命中官方包时**不展示**，并计入新字段 `official`；`hidden` 仍是「不符合 `fish-dock-plugin-*` 命名」的口径，两者不混在一个数字里。命令返回体 `search_json` 与前端夹具 `app/test/fixtures/thirdparty-search.json` 同步新增 `official`。
- **安装口零网络拒绝**：`resolve_via` 在命名判据之后、任何请求之前拦下官方包，返回明确理由（``…` 是官方插件：请到「官方插件」页安装或更新``），不再出现「装了但起不来」或「装成第三方身份」。
- **界面说出来，不静默消失**：第三方页提示「本页另有 N 个官方插件的 npm 包：官方插件请到「官方插件」页安装或更新」，规则行也补一句「官方插件不在本页」。

## 二、自动检查更新恢复 Gitee 拉取

**背景**：2.1.0 时 Gitee 通道是「配了才启用、默认空」，理由是当时主仓 Gitee 只有 tags、没有任何发行附件。现在那边**已经有发行资产**，实测（2026-11-05，匿名 API）：

- `api/v5/repos/Fish-under-sea/FISH-Dock/releases` 返回 `v2.1.0`，附件 `FISH-Dock-Update-2.1.0.exe` 与 `FISH-Dock-Update-2.1.0.exe.sha256`；
- 那份 `.sha256` 是 `5513ee3a5605a67e256fc23ecb68625ae92a0c0c18fe1f1a18e97b9fadd84406  FISH-Dock-Update-2.1.0.exe`，与 GitHub v2.1.0 的 `assets[].digest`（`sha256:5513ee…4406`）**逐字相同**。

**改动**：

- `config::GITEE_REPO_DEFAULT = https://gitee.com/Fish-under-sea/FISH-Dock`，`url.gitee_repo` 改为**默认启用**；`off` / `none` / `0`（大小写不敏感）是明确的关闭写法——空串在这套 getter 里的既定语义是「没覆盖，用默认」，不能兼作开关。
- 设置页「自动检查更新 · 从哪里下载」不再显示「Gitee 未配置」，`自动 / Gitee / GitHub` 三个选项都在（`giteeConfigured` 由 `gitee_repo()` 是否为空决定，不变）。
- **探针名错位修复**（`AGENTS.md` §8.1 记的上游缺陷①）：检查器一直按 `wakuwaku-v<ver>-windows-x64.exe`（便携 exe 的历史名）去发行里找资产与 `<exe>.sha256`，而 2.0.x 起的发行只有 `FISH-Dock-Setup-<ver>.exe` / `FISH-Dock-Update-<ver>.exe`——所以 Gitee 侧连 `.sha256` 都探不到，整条通道等于没接。新增 `updates::update_asset_name`（`FISH-Dock-Update-<ver>.exe`，即应用内更新真正先下载的那份），Gitee 探针改按它取摘要。
- **两侧比的是同一个文件**：`github_release` 的 `digest` 也改取安装器载荷（Update 包）的摘要——它是 Gitee 侧唯一放上去的那份，两边摘要因此可以直接比对；两边不一致时仍按 2.0.4 的口径判 `conflict`、不装。发行里没有安装器载荷（例如只有便携 exe 的历史版本）时摘要为 `None`，**不拿便携 exe 的摘要去比**，避免造出假冲突。
- `Found.exe`（便携载荷）语义没变：Gitee 不放便携 exe，所以恒为 `None`；不拿安装器顶替「换掉自己的 exe」。
- **只认完整的 Gitee 发行**：探不到 `FISH-Dock-Update-<ver>.exe.sha256` 就返回「那一侧没有」，不用半份发行（只传了 Setup 的那种）宣布有新版本，也不制造跨来源摘要冲突。Gitee 发布仍需「Update exe + 它的 `.sha256`」两件齐全。
- 放行范围：`allowed()` 仍是「前缀 + 边界字符」；默认 `gitee_repo` 非空只意味着**同一个 repo** 的 Gitee 资产可下载，插件仓（`FISH-DOCK-Plugins`）与别人的仓照旧拒绝。

## 三、版本位

- `app/src-tauri/tauri.conf.json` 与 `app/src-tauri/Cargo.toml`：**2.1.1**（同版）。
- `app/integrations/claude-code/.claude-plugin/plugin.json`：**2.1.1**。它此前停在 **2.0.4**，而 `one_version_everywhere` 要求三处一致——2.1.0 那版漏了这一处，本版把这条测试修回绿。
- `AGENTS.md` §八 与 `docs/config-keys.md` 同步（Gitee 现状从「有 tags 无附件」改为「有 v2.1.0 发行资产、默认启用」）。
- README 的版本卡与发行段由 `tools/render-readme.mjs` 从 `tauri.conf.json` 渲染，出包时统一重渲染，不手改 PNG。

## 四、测试

- `plugin_thirdparty::tests::the_three_official_plugins_are_never_third_party`（新增）：判据只认官方包名、大小写无关；发现词检索把官方三个全部排除且 `official = 3`、`hidden` 只数真正命名不合规的包；安装口命中官方包时 `asked == 0`（零网络）且理由指回官方商店。
- `plugin_api::tests::the_search_command_carries_every_field_the_ui_reads`：命令返回体与前端夹具字段一一对应，新增 `official`。
- `config::tests::mirror_prefixes_default_to_empty_and_prefer_whitelists_or_falls_back`：`gitee_repo()` 默认等于 `GITEE_REPO_DEFAULT`；`off/ none / 0` 关掉；空白＝没覆盖。
- `sources::tests`：`the_gitee_attachment_candidate_is_on_by_default_and_off_switches_it_away`、`a_malformed_gitee_repo_yields_no_candidate`、`default_candidates_are_locked_with_the_gitee_channel_off` 按新默认重钉。
- `updates::tests`：`the_gitee_endpoint_is_on_by_default_and_switches_off_explicitly`、`the_gitee_candidate_stays_inside_the_same_allowance`、`the_allowance_stops_at_the_repository_boundary`、`github_read`（摘要取安装器载荷、便携资产不掺和）。
- 前端（`node --test --test-isolation=none app/test/extension-panel.test.js`，75 passed）：`被排除的官方插件数量单独提示，并指回官方插件页`、`官方插件与命名不合规同时出现时，两句提示都在`。

## 五、安装包

| 安装包 | 使用场景 |
| --- | --- |
| `FISH-Dock-Setup-2.1.1.exe` | 首次安装、基础资源缺失或修复安装，包含完整离线运行时。 |
| `FISH-Dock-Update-2.1.1.exe` | 已有 **2.1.0** 且基础资源完整时的精简更新，只含相对 2.1.0 的变更；缺失或损坏时提示改用完整包。 |

> 精简包只对 2.1.0 有效（差分包按基线生成，跨版本会缺文件）。

## 六、已知限制

- **便携版仍无应用内更新载荷**：发行不提供便携 exe，`Found.exe` 保持 `None`；应用内更新只对安装版生效（装的是发行里的安装器）。
- **Gitee 侧必须传 Update 包**：只传 `FISH-Dock-Setup-*.exe` 时 Gitee 不参与「有新版本」的判断（见第二节「只认完整的 Gitee 发行」）。`app/scripts/gitee-release.ps1` 默认只传 Setup，要恢复完整的国内更新链，发布时需再跑一次并传 Update 包和它的 `.sha256`。
- 官方插件仍只以 `catalog/stable.json` + npm 双源 + GitHub Release 为来源；本次改动**没有**把 Gitee 接进插件三源。