# 本机环境档案 · Fish

> 机器：Fish（`$env:USERNAME`），用户目录 `C:\Users\Fish`。
> 跨机器规则与通用排障见仓库根 [AGENTS.md](../../../AGENTS.md)；第三方工具集根目录、自写可复用脚本根目录、包缓存（`npm-cache` / `pnpm-store`）与 `.work/` 临时产物约定见全局 `C:\Users\Fish\.gemini\config\GEMINI.md`。本文件仅补充 FISH Dock 专属路径、实测入口、凭据位置与本机特有处置。

## 一、工作区与仓库

| 项 | 值 |
| --- | --- |
| 主工作区根 | `D:\Fish-code\AGY` |
| 本仓当前工作副本 | `D:\Fish-code\AGY\.work\release239`（按任务在 `.work\` 下切出） |
| 同级插件仓路径 | `D:\Fish-code\AGY\FISH-DOCK-Plugins` |
| 复用编译缓存目录 | `D:\Fish-code\AGY\.target-fish-dock`（通过 `CARGO_TARGET_DIR` 共享） |
| 离线安装资源复用源 | `C:\Users\Fish\AppData\Local\Programs\FISH Dock\resources`（复用时先复制到 `.work\` 临时目录再更新 `manifest.json` 版本号） |
| 通用工具集 / 脚本根 / `.work/` | 见全局 `C:\Users\Fish\.gemini\config\GEMINI.md`；FISH Dock 专属脚本位于该脚本根下的 `FISH Dock\` 子目录 |

## 二、工具链（本机实测入口）

| 工具 | 路径或版本 | 备注 |
| --- | --- | --- |
| PowerShell | `pwsh` 7.6.6 | 默认命令行 |
| Node | `C:\Program Files\nodejs\node.exe`（v24.14.0，Medium IL） | 本机脚本、测试与打包统一使用此系统安装版 |
| Rust / cargo | 工具集根下 `Rust\1.99.1\`（`rustc 1.99.1`） | 按第三节设置环境变量并启用 `CARGO_NET_OFFLINE=true` |
| NSIS | `C:\Users\Fish\AppData\Local\FishDockShip\nsis-3.13\makensis.exe` | 打包时通过 `build-installer.ps1 -Makensis` 传入 |
| 7-Zip | `C:\Program Files\7-Zip\7z.exe` | 安装包完整性校验与内容核对 |
| 无头浏览器 | `C:\Program Files\Google\Chrome\Application\chrome.exe` | 经 `EDGE_BINARY` 传给 `tools/docs/render-readme.mjs` |
| Gitee 发版与下载脚本 | 脚本根下 `FISH Dock\gitee-publish.cjs`、`fetch-file.mjs`、`fetch-asset-resume.mjs` | 走 Node TLS；发版用法：`node gitee-publish.cjs <版本> <exe路径> <40位commit>` |

## 三、环境变量

| 变量 | 建议值 | 用途 |
| --- | --- | --- |
| `CARGO_HOME` / `RUSTUP_HOME` | 工具集根下 `Rust\1.99.1\cargo` 与 `Rust\1.99.1\rustup`（并把 `$env:CARGO_HOME\bin` 放到 `PATH` 最前） | 覆盖系统默认用户变量，固定指向本机 Rust 1.99.1 工具链 |
| `CARGO_TARGET_DIR` | `D:\Fish-code\AGY\.target-fish-dock` | 跨工作副本共享编译缓存 |
| `CARGO_NET_OFFLINE` | `true` | 离线锁版本编译与测试 |
| `EDGE_BINARY` | `C:/Program Files/Google/Chrome/Application/chrome.exe` | 指定 Chrome 执行 README 渲染与截图 |
| `npm_config_cache` / `pnpm_store` | 见全局 `C:\Users\Fish\.gemini\config\GEMINI.md` | 统一落在工具集缓存目录 |
| `GITEE_TOKEN` | 发版前从凭据文件读入内存 | 脚本结束后清空 |

## 四、凭据位置（仅登记路径）

| 凭据 | 文件路径 | 用法 |
| --- | --- | --- |
| Gitee 令牌 | `C:\Users\Fish\.git-credentials` 中的 `gitee.com` 条目 | 发版前读入内存赋值给 `$env:GITEE_TOKEN`，脚本结束后清空 |
| GitHub 认证 | `gh` CLI 已登录（账号 `Fish-under-sea`） | 需令牌时用 `gh auth token` 现取，用完即弃 |

## 五、本机特有处置

> 跨机器通用排障见仓库根 [AGENTS.md](../../../AGENTS.md) 第六节，Gitee 同树同步流程见第七节。

| 场景 | 做法 |
| --- | --- |
| 运行 `cargo` / `rustc` | 先按第三节显式设置 `CARGO_HOME`、`RUSTUP_HOME` 并前置 `$env:CARGO_HOME\bin` 到 `PATH` |
| 运行 `tools/docs/render-readme.mjs` | 显式设置 `EDGE_BINARY='C:/Program Files/Google/Chrome/Application/chrome.exe'` |

## 六、最近一次核对

- 日期：2026-11-22
- 核对人：Fish
