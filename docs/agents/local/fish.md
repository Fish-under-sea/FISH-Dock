# 本机环境档案 · Fish

> 机器：Fish（`$env:USERNAME`），用户目录 `C:\Users\Fish`。
> 跨机器规则与通用排障见仓库根 [AGENTS.md](../../../AGENTS.md)；**第三方工具集根目录、自写可复用脚本根目录、包缓存（`npm-cache` / `pnpm-store`）与 `.work/` 临时产物约定已在全局 `C:\Users\Fish\.gemini\config\GEMINI.md` 维护**，本文件不重复展开，仅记录 FISH Dock 相关具体路径、可用入口、凭据位置与本机特有坑点。

## 一、工作区与仓库

| 项 | 值 |
| --- | --- |
| 主工作区根 | `D:\Fish-code\AGY` |
| 本仓当前工作副本 | `D:\Fish-code\AGY\.work\release239`（随工作任务在 `.work\` 下切出，不固定单一旧目录） |
| 同级插件仓路径 | `D:\Fish-code\AGY\FISH-DOCK-Plugins` |
| 复用编译缓存目录 | `D:\Fish-code\AGY\.target-fish-dock`（通过 `CARGO_TARGET_DIR` 共享） |
| 离线安装资源复用源 | `C:\Users\Fish\AppData\Local\Programs\FISH Dock\resources`（复用时须复制到 `.work\` 临时目录并更新 `manifest.json` 版本号，不得就地修改） |
| 可复用工具集 / 脚本与经验 / `.work/` 约定 | 见全局 `C:\Users\Fish\.gemini\config\GEMINI.md`；FISH Dock 专属脚本位于该脚本根下的 `FISH Dock\` 子目录 |

## 二、工具链（本机实测可用入口）

| 工具 | 路径或版本 | 备注 |
| --- | --- | --- |
| PowerShell | `pwsh` 7.6.6 | 默认命令行 |
| Node | `C:\Program Files\nodejs\node.exe`（v24.14.0，Medium IL） | 优先用系统安装版；工具集内的 `Node\24.14.0\node.exe` 为 Low IL，执行会崩溃 |
| Rust / cargo | 工具集根下 `Rust\1.99.1\`（`rustc 1.99.1`） | 需覆盖 shell 环境变量并设 `CARGO_NET_OFFLINE=true`（见第三节） |
| NSIS | `C:\Users\Fish\AppData\Local\FishDockShip\nsis-3.13\makensis.exe` | 打包时通过 `build-installer.ps1 -Makensis` 传入 |
| 7-Zip | `C:\Program Files\7-Zip\7z.exe` | 安装包完整性校验与内容核对 |
| 无头浏览器 | `C:\Program Files\Google\Chrome\Application\chrome.exe` | 经 `EDGE_BINARY` 传给 `tools/docs/render-readme.mjs` |
| Gitee 发版与下载脚本 | 脚本根下 `FISH Dock\gitee-publish.cjs`、`fetch-file.mjs`、`fetch-asset-resume.mjs` | 走 Node TLS，规避本机 schannel 握手失败；发版用法：`node gitee-publish.cjs <版本> <exe路径> <40位commit>` |

## 三、环境变量

| 变量 | 建议值 | 原因 |
| --- | --- | --- |
| `CARGO_HOME` / `RUSTUP_HOME` | 工具集根下 `Rust\1.99.1\cargo` 与 `Rust\1.99.1\rustup`（并把 `$env:CARGO_HOME\bin` 放到 `PATH` 最前） | 系统用户变量仍残留已迁走的旧路径 `D:\Fish-code\DSH\工具\Rust\1.99.1\...` |
| `CARGO_TARGET_DIR` | `D:\Fish-code\AGY\.target-fish-dock` | 避免不同工作副本重复冷编译 |
| `CARGO_NET_OFFLINE` | `true` | 离线锁版本编译与测试 |
| `EDGE_BINARY` | `C:/Program Files/Google/Chrome/Application/chrome.exe` | 本机无头 `msedge.exe` 启动即退出 |
| `npm_config_cache` / `pnpm_store` | 见全局 `C:\Users\Fish\.gemini\config\GEMINI.md` | 统一落在工具集缓存目录 |
| `GITEE_TOKEN` | 运行 `gitee-publish.cjs` / `gitee-release.ps1` 前临时读入内存 | 用完清空；来源见第四节 |

## 四、凭据位置（只写位置，不写内容）

| 凭据 | 文件路径 | 备注 |
| --- | --- | --- |
| Gitee 令牌 | `C:\Users\Fish\.git-credentials` 中的 `gitee.com` 条目 | 旧 `C:\Users\Fish\.gitee-token` 已不存在；仅临时读入内存，严禁打印、入库或写入 remote |
| GitHub 认证 | `gh` CLI 已登录（账号 `Fish-under-sea`） | 需令牌时用 `gh auth token` 现取，用完即弃 |

## 五、本机特有现象

> 跨机器通用坑点（`git push` TLS `-c http.sslBackend=openssl -c http.version=HTTP/1.1`、`gh --repo`、Clash 停止后的代理残留、schannel 报错）见仓库根 [AGENTS.md](../../../AGENTS.md) 第六节，不在此重复。

| 现象 | 处置 |
| --- | --- |
| 直接跑 `cargo` / `rustc` 找不到命令或报无默认 toolchain | 用户环境变量仍指向旧 `D:\Fish-code\DSH\...`；按第三节显式覆盖 `CARGO_HOME`、`RUSTUP_HOME` 与 `PATH` |
| 调用工具集内 `Node\24.14.0\node.exe` 以 `-1073741502` 退出 | 该副本为 Low IL；改用 `C:\Program Files\nodejs\node.exe` |
| `tools/docs/render-readme.mjs` 报 Edge 立即退出 | 显式设置 `EDGE_BINARY='C:/Program Files/Google/Chrome/Application/chrome.exe'` |
| 同步 Gitee `main` 时无法 fast-forward | Gitee `main` 为独立提交链；按 `AGENTS.md` 第七节用 `git commit-tree` 在 Gitee `main` 上重放同树提交并核对 `git diff` 为空，不强推 |

## 六、最近一次核对

- 日期：2026-11-22
- 核对人：Fish
