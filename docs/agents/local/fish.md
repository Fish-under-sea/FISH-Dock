# 本机环境档案 · Fish

> 机器：Fish（`$env:USERNAME`），用户目录 `C:\Users\Fish`。
> 本文件只写环境事实；规则以仓库根 `AGENTS.md` 为准。凭据只写位置，不写内容。

## 一、工作区与仓库

| 项 | 值 |
| --- | --- |
| 本仓路径 | `D:\Fish-code\DSH-NEXT\插件\插件维护\FISH-Dock-1109\FISH-Dock` |
| 同级仓库（插件仓）路径 | `D:\Fish-code\DSH-NEXT\插件\插件维护\FISH-Dock-1109\FISH-Dock-Plugins` |
| 一次性产物目录 | 同级 `.work\`（如 `.work\installer-resources`） |
| 可复用脚本索引 | `D:\Fish-code\DSH\可复用脚本与经验\README.md` |

## 二、工具链

| 工具 | 路径或版本 | 备注 |
| --- | --- | --- |
| PowerShell | PowerShell 7（`pwsh`） | 默认命令行 |
| Node | 24.14（`node`） | 打包与校验另见插件仓档案的 Node 22 说明 |
| Rust / cargo | 随仓构建 | 双包脚本自设 `CARGO_NET_OFFLINE=true` |
| 7-Zip | `C:\Program Files\7-Zip\7z.exe` | 工具栏位那份不存在，别用 |
| 双包构建脚本 | `D:\Fish-code\DSH\可复用脚本与经验\FISH Dock\build-dual-package.ps1` | 默认 `-Dock` / `-Resources` 已指向 1109 工作区 |
| 下载脚本 | 同上目录的 `fetch-file.mjs`、`fetch-asset-resume.mjs` | 本机 schannel 坏，走 Node TLS |
| README 渲染 | 本仓 `tools/docs/render-readme.mjs` | 实际遇到沙箱拒绝后申请单次授权 |

## 三、环境变量

| 变量 | 建议值 | 原因 |
| --- | --- | --- |
| `npm_config_cache` | `<工作区>\.work\npm-cache` | 默认缓存在用户目录，沙箱不给写 |
| `HTTP_PROXY` / `HTTPS_PROXY` | 用前先 `Get-NetTCPConnection -State Listen` 确认 | Clash 停内核后会残留，造成「以为开了代理」 |
| `GITEE_TOKEN` | 运行 `gitee-release.ps1` 前临时读入 | 见下节令牌位置 |

## 四、凭据位置（只写位置，不写内容）

| 凭据 | 文件路径 | 备注 |
| --- | --- | --- |
| Gitee 令牌 | `C:\Users\Fish\.gitee-token` | 临时读入 `$env:GITEE_TOKEN`；不得入库、写入 remote 或提交记录 |
| GitHub 认证 | `gh auth token` 现取 | 拼临时 URL 推送，用完即弃 |

## 五、本机特有现象

| 现象 | 处置 |
| --- | --- |
| HTTPS 报 `schannel: AcquireCredentialsHandle failed: SEC_E_NO_CREDENTIALS` | 本机 schannel 客户端凭证不可用；所有下载走 Node TLS 脚本 |
| `git push` 报 `TLS connect error: error:0A000126 ... unexpected eof` | 加 `-c http.sslBackend=openssl -c http.version=HTTP/1.1` |
| `gh` 报 `fatal: not a git repository` | 一律显式 `--repo <owner>/<repo>` |
| 跨出工作区写 `D:\Fish-code\DSH\...` 被拒 | 需要 `danger-full-access` 单次授权，并在说明里写清目的 |

## 六、最近一次核对

- 日期：2026-11-12
- 核对人：Fish
