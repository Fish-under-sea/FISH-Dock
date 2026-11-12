# FISH Dock 宿主仓 · Agent 必读

> 本文件是本仓的**跨机器口径**：任何人克隆本仓都适用，只写「改这里一定会踩到的坑」。
> 涉及本机路径、工具链位置、凭据文件的内容**不写在这里**，见第零节。
> 提交 issue / PR 的规则在 `.github/CONTRIBUTING.md`，不与本文件重复。

## 零、先认出当前用户，再读你的本机覆盖

本仓把「跨机器规则」与「本机事实」分成两层：本文件是前者，`docs/agents/local/<用户名>.md` 是后者，**两者都入库**，每个作者维护自己那份。

开工前按顺序做：

1. 取当前用户名：PowerShell 里 `$env:USERNAME`（同时可看 `$env:USERPROFILE` 确认用户目录）。
2. 转成小写、空格与非 ASCII 字符替换为 `-`，得到文件名，读 `docs/agents/local/<该名>.md`。
   - 本机 `USERNAME=Fish` → 读 `docs/agents/local/fish.md`。
3. 文件不存在：复制 `docs/agents/local/_template.md` 为 `<你的名字>.md`，填完随你的一次正常提交一起入库，**不要只留在本地**。
4. 两层冲突时的优先级：**路径、工具链、凭据、缓存位置以本机覆盖为准；规则、契约、发布口径以本文件为准。**
5. 拿不准用户名的规范写法，看 `docs/agents/README.md`。

不要凭猜测套用别人的本机文件：别人的盘符和令牌位置在你的机器上不成立。

## 一、仓库边界与模块位置

| 目录 | 是什么 |
| --- | --- |
| `app/src-tauri/` | Rust 宿主本体：窗口、桌面岛、运行时、插件管理、更新器、安装器运行时 |
| `app/src-tauri/src/` | 按关注点分文件：`config.rs`、`updates.rs`、`sources.rs`、`plugin_store.rs`、`plugins.rs`、`installer` 相关、`crash.rs`、`mail/`、`plugins/` |
| `app/src/pet/` | 前端（宠物与岛上的页面） |
| `app/scripts/` | 构建、安装、发布、渲染、校验脚本（`installer.mjs`、`prepare-installer.mjs`、`gitee-release.ps1`、`build-*.ps1`、各 `*.test.mjs`） |
| `docs/` | 开发说明、计划、发行说明、规格、审计（见第二节） |
| `sdk/` | 对外 SDK（`browser/`、`node/`、`examples/`） |
| `tools/docs/` | README 渲染与引用校验（`render-readme.mjs`、`check-readme.mjs`、`render-sections.mjs`） |
| `tools/preview/`、`tools/misc/` | 真实前端预览与截图；API 客户端与专项截图辅助工具 |
| `integrations/`、`examples/` | Agent / 邮件集成与示例脚本，Rust 编译期嵌入路径从这里取 |
| `pets/`、`art/`、`wallpapers/` | 默认宠物、素材说明与壁纸；素材许可独立于代码 |
| `relay/` | 独立的崩溃报告中转代码，不在源码构建目录内 |
| `.github/` | issue 模板与 `CONTRIBUTING.md`（提交规则唯一出处） |

官方插件本体与商店目录不在本仓，在 `Fish-under-sea/FISH-DOCK-Plugins`。

## 二、文档导航（改之前先读对应那份）

| 你要做的事 | 先读 |
| --- | --- |
| 理解宿主架构、hook、更新、崩溃、岛 | `docs/guides/DEVELOPMENT.md`（内容最全，按小节查） |
| 动安装器 / 出包 | `docs/guides/installer-build.md`、`app/scripts/installer.mjs`、`app/scripts/installer.test.mjs` |
| 动配置键或环境变量 | `docs/guides/config-keys.md` |
| 发版 | `docs/releases/release-<ver>.md`（模板照抄上一版） |
| 查历史决策与欠账 | `docs/plans/plan-*.md`、`docs/plans/audit-*.md` |
| 国内镜像口径 | `docs/guides/domestic-mirror-matrix-2026-11-04.md` |
| 提 issue / PR | `.github/CONTRIBUTING.md` |
| 本机环境 | `docs/agents/local/<用户名>.md` |

## 三、摘要与清单的权威来源

- **宿主自身**：Release 必须附 `<exe>.sha256`，应用内更新从它读 digest；只给 `SHA256SUMS.txt` 会让 Dock 拒绝应用内升级。
- **资源清单** `resources/manifest.json`：`product` 必须是 `FISH Dock`，`version` 必须等于 `app/src-tauri/tauri.conf.json`。
  - `rebuild-manifest.mjs` 会**保留旧 product**，产品改名后不要用它重置清单，改用 `app/scripts/prepare-installer.mjs` 的 `writeManifest`。
  - **清单只能由 `writeManifest` 生成，不要手写 JSON**：2.1.2 手写时漏了 `node_version` / `webview2_version` 两个必填字段，`runtime.rs` 的 `Manifest` 反序列化在启动第一步就失败（「安装资源清单无法解析：missing field `node_version`」），Setup 与 Update 两个包全崩，用户也进不去应用内更新，只能手动重装。
  - 键集合固定 7 项（`schema` / `product` / `version` / `architecture` / `node_version` / `webview2_version` / `files`），改结构要同步 `runtime.rs`；`installer.test.mjs` 与 `runtime.rs` 各有一条回归测试钉住它。
- **插件摘要**以插件仓的 `catalog/stable.json` 为准，本仓不重复定义。

## 四、出包与发布

- 当前仅发布 Setup；2.2.0 是最后的 Setup + Update 双包版本。历史双包脚本不能当作当前默认入口。
- 资源由 `app/scripts/prepare-installer.mjs` 从已下载并锁定摘要的 Node 包准备；不再携带 Windhawk 或固定版 WebView2，Setup 检测系统 Evergreen。清单必须用 `writeManifest` 生成。
- 从仓库根运行 `pwsh app/scripts/build-installer.ps1 -Resources <含 manifest.json 的资源目录>`；实际工具参数见脚本与 `docs/guides/installer-build.md`。复用资源时不得就地修改硬链接。
- 出包后用 7-Zip 验证完整性，抽取清单与 NOTICE 核对，并校验 `FISH-Dock-Setup-<ver>.exe.sha256`。
- 更新器自 2.2.0 起只认 Setup 载荷及其摘要，不再用 Update 作为候选；不要沿用「Setup-only 不能应用内升级」的过期说法。
- 推送、发布与部署必须单独得到用户确认；本地构建或提交不代表授权发布。

## 五、版本位与文档同步

- 版本唯一权威：`app/src-tauri/tauri.conf.json` 与 `app/src-tauri/Cargo.toml`（必须同版）。
- README 的版本卡与头图由 `tools/docs/render-readme.mjs` 渲染（从 `tauri.conf.json` 读取），**禁止手改 PNG**；改完用 `read_image` 确认两个版本号都对。
- 发版要同步：`README.md`、`README.en.md` 的当前版本 / Release 链接 / 包名，并新增 `docs/releases/release-<ver>.md`；历史变更条目保持原样。

## 六、环境坑（跨机器通用的部分）

| 现象 | 真相 | 处置 |
| --- | --- | --- |
| `git push` 报 `TLS connect error: error:0A000126 ... unexpected eof` | git 默认 TLS 通道在部分机器不稳 | 加 `-c http.sslBackend=openssl -c http.version=HTTP/1.1`；别用 `schannel` |
| `git push` 要凭据时 credential helper 起不来 | 沙箱禁命名管道（`sh.exe ... signal pipe, Win32 error 5`） | 用 `gh auth token` 拼**临时 URL** 推送，用完即弃；**不写进 remote/config**，推完 `git fetch` 对齐 |
| 以为开了代理、其实没有 | `HTTP(S)_PROXY` 会在代理内核停掉后残留 | 先 `Get-NetTCPConnection -State Listen` 看端口是否在听，再决定用或清 |
| `gh` 报 `fatal: not a git repository` | `gh` 要在仓库目录执行 | 一律显式 `--repo <owner>/<repo>` |
| Chromium/Edge 无头起不来（`mojo … 拒绝访问 (0x5)`） | 沙箱禁命名管道 | `tools/docs/render-readme.mjs`、截图类脚本在实际被拒后申请原命令单次授权 |
| `node --test` 报 `spawn EPERM` | 沙箱禁管道 stdio | 用 `node --test --test-isolation=none "<glob>"`；真实子进程被拒时保留断言并申请该命令单次授权，未经验证不能记通过 |
| HTTPS 报 `schannel: AcquireCredentialsHandle failed` | 本机 schannel 客户端凭证不可用，与代理、目标站无关 | 下载走 Node TLS（脚本位置见本机覆盖），大文件用断点续传那份 |

测试必须保留真实协议与子进程覆盖，不为适配沙箱删断言；只把能确认的权限错误归类为环境阻塞。

## 七、Gitee 通道与国内镜像

- Gitee 仓：`https://gitee.com/Fish-under-sea/FISH-Dock`，是**国内仓库入口**，发行与 catalog 的权威仍在 GitHub。
- **Gitee tag 指向 Gitee 自有历史提交**：在 Gitee main 上重放 GitHub 本轮提交，核对两边树一致，再推送 Gitee main 和 tag；不直接推 GitHub tag、不强推历史。
- 发布脚本 `app/scripts/gitee-release.ps1` 已参数化且默认指向本仓（`-Owner 'Fish-under-sea' -Repo 'FISH-Dock'`），并有上游保护（遇 `zhoupengjie/Wakuwaku` 直接 `throw`，除非显式 `-AllowUpstream`）。**无 `GITEE_TOKEN` 时脚本只校验不写入**；令牌来源见本机覆盖，令牌内容不得入库、写入 remote 或提交记录。
- 请求 Gitee 返回 TLS `Authentication failed` 时，先用同一令牌经 Node TLS 验证身份；不要把 TLS 握手错误直接判成令牌失效。
- 更新器的镜像机制是「配了才启用」：

  | 配置键 / 环境变量 | 默认 | 作用 |
  | --- | --- | --- |
  | `url.raw_mirror` / `FISH_DOCK_RAW_MIRROR` | 空 | `raw.githubusercontent.com` 前缀镜像；`sources::picks` 另有开箱即用的 jsDelivr 候选 |
  | `url.gh_proxy` / `FISH_DOCK_GH_PROXY` | 空 | GitHub Release 下载加速前缀 |
  | `url.gitee_repo` / `FISH_DOCK_GITEE_REPO` | 主仓 Gitee 镜像（默认启用；`off`/`none`/`0` 关闭） | 更新检查候选 + Gitee 发行资产候选 + `updates::allowed` 放行边界 |
  | `url.pet_mirror` / `FISH_DOCK_PET_MIRROR` | 空 | 宠物图库前缀镜像 |
  | `mirror.prefer` / `FISH_DOCK_MIRROR_PREFER` | `auto` | `auto` 原地址优先、`domestic` 镜像优先、`overseas` 原地址优先 |

- 择源统一入口：`sources::candidates(url)` = 原地址 + `picks(url)` 的镜像候选，**所有候选共用一个请求 deadline**；`plugin_store::download` 也走 `sources::fetch_bytes_with`。
- **历史欠账如实标注**：Windhawk / LHM 没有已确认的免费国内镜像；未配置 Gitee / pet mirror / Release 加速时「每类资源默认国内外双源」不成立。**不能把默认空前缀算作已完成，也不要求用户部署服务**；未找到时写缺口，别伪造镜像。
- 写文档时不要沿用过期的 Gitee 状态：需要引用「Gitee 下载」前，先用 Node TLS 拉 `https://gitee.com/api/v5/repos/<owner>/<repo>/releases` 重跑探测（接口按 id 升序返回，取末条才是最新）。

## 八、提交与交付

- 命令行默认 PowerShell 7。
- 提交用中文 conventional（`chore(release): …` / `fix(plugin): …` / `docs(...): …`），一个提交只做一件事。
- `.work/`、`target/`、`dist/`、`*.exe` 不进库；`.work/` 只放一次性产物。
- **禁用 `git clean -fdx`**，删东西按白名单逐项删。
- 改本文件或 `docs/agents/` 下的内容，随**改动它的那次功能提交**一起带上，不为文档单独开一个空转提交。
- `AGENTS.local.md` 这种只在本机生效、不入库的写法已废弃；本机事实一律写进 `docs/agents/local/<用户名>.md` 并入库。

- 不提交 handoff、会话交接、临时验收与构建报告。短期材料放忽略的 `.work/`；长期知识归入指南、规格或回归测试。
