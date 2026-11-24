# FISH Dock 宿主仓 · Agent 必读

> 本文件是本仓的**跨机器口径**：任何人克隆本仓都适用，只写「改这里一定会踩到的坑」。
> 本机路径、工具链与凭据位置记录在 `docs/agents/local/<用户名>.md`（见第零节）。
> 提交 issue / PR 的规则在 `.github/CONTRIBUTING.md`。

## 零、先认出当前用户，再读你的本机覆盖

本仓把「跨机器规则」与「本机事实」分成两层：本文件是前者，`docs/agents/local/<用户名>.md` 是后者，**两者都入库**，每个作者维护自己那份。

开工前按顺序做：

1. 取当前用户名：PowerShell 里 `$env:USERNAME`（同时可看 `$env:USERPROFILE` 确认用户目录）。
2. 转成小写、空格与非 ASCII 字符替换为 `-`，得到文件名，读 `docs/agents/local/<该名>.md`。
   - 本机 `USERNAME=Fish` → 读 `docs/agents/local/fish.md`。
3. 文件不存在：复制 `docs/agents/local/_template.md` 为 `<你的名字>.md`，填完随你的一次正常提交一起入库，**不要只留在本地**。
4. 两层冲突时的优先级：**路径、工具链、凭据、缓存位置以本机覆盖为准；规则、契约、发布口径以本文件为准。**
5. 拿不准用户名的规范写法，看 `docs/agents/README.md`。

仅读取与当前 `$env:USERNAME` 对应的那份个人档案。

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
| `.github/` | issue 模板、`CONTRIBUTING.md`（提交规则唯一出处）与工作流（`ci.yml`、`sync-plugin-packages.yml`） |

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
- **资源清单** `resources/manifest.json`：`product` 必须是 `FISH Dock`，`version` 必须等于 `app/src-tauri/tauri.conf.json`；统一通过 `app/scripts/prepare-installer.mjs` 的 `writeManifest` 生成，确保 7 项必填键（`schema` / `product` / `version` / `architecture` / `node_version` / `webview2_version` / `files`）与 `runtime.rs` 及 `installer.test.mjs` 回归测试一致。
- **插件摘要**以插件仓的 `catalog/stable.json` 为准。

## 四、出包与发布

- 当前统一发布单包 Setup（构建入口 `app/scripts/build-installer.ps1`）；应用内更新直接下载并校验 Setup 载荷及其 `.sha256` 摘要。
- 资源由 `app/scripts/prepare-installer.mjs` 从已锁定摘要的 Node 包准备，清单通过 `writeManifest` 生成；Setup 启动时检测系统 Evergreen WebView2。
- 从仓库根运行 `pwsh app/scripts/build-installer.ps1 -Resources <含 manifest.json 的资源目录>`；实际工具参数见脚本与 `docs/guides/installer-build.md`。复用现有资源时，先复制到 `.work/` 临时副本再修改。
- 出包后用 7-Zip 验证完整性，抽取清单与 NOTICE 核对，并校验 `FISH-Dock-Setup-<ver>.exe.sha256`。
- 推送、发布与部署须单独获得用户确认后执行。
- **同步官方插件到本仓 Packages 栏**：插件仓发布新版本且 npm 源到位后，在 GitHub Actions 触发 `Sync Plugin Packages`（或运行 `gh workflow run sync-plugin-packages.yml --repo Fish-under-sea/FISH-Dock`，单包可加 `-f only=<feishu|clash|magpie>`），工作流会按插件仓 `catalog/stable.json` 校验 SHA-256 并将新版同步挂到本仓 GitHub Packages。

## 五、版本位与文档同步

- 版本唯一权威：`app/src-tauri/tauri.conf.json` 与 `app/src-tauri/Cargo.toml`（必须同版）。
- README 的版本卡与头图由 `tools/docs/render-readme.mjs` 从 `tauri.conf.json` 自动渲染生成；更新后用 `read_image` 核对版本号。
- 发版同步更新 `README.md`、`README.en.md` 的当前版本 / Release 链接 / 包名，并新增 `docs/releases/release-<ver>.md`。

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
- 镜像文档须按实测结果逐项记录已验证的国内外可用源；尚无可用镜像的资源如实标注为待补齐。
- 引用 Gitee 发行状态前，用 Node TLS 拉取 `https://gitee.com/api/v5/repos/<owner>/<repo>/releases` 实时探测（接口按 id 升序返回，末条为最新）。

## 八、提交与交付

- 命令行默认 PowerShell 7。
- 提交用中文 conventional（`chore(release): …` / `fix(plugin): …` / `docs(...): …`），一个提交只做一件事。
- 仅提交源码与长期文档；`.work/`、`target/`、`dist/`、`*.exe` 留在本地工作区。
- 清理临时产物时，仅显式指定本次在 `.work/` 下自建的路径逐项删除。
- 改动本文件或 `docs/agents/` 下的内容时，随对应功能或环境更新提交一起入库。
- 短期交接与临时构建/验收产物保存在 `.work/`；长期知识沉淀至对应指南、规格或回归测试后随功能提交入库。
