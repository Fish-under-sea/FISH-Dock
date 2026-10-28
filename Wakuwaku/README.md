<p align="center"><img src="src-tauri/icons/dsh-fish.png" width="112" alt="FISH Dock 图标"></p>
<h1 align="center">FISH Dock</h1>
<p align="center">AI 会话有处停靠，模型连接集中管理。</p>
<p align="center"><strong>1.2.2</strong> · Windows x64 · MIT 代码许可</p>
<p align="center"><strong>简体中文</strong> · <a href="README.en.md">English</a> · <a href="https://github.com/Fish-under-sea/FISH-Dock/releases/tag/v1.2.2">下载</a> · <a href="https://github.com/Fish-under-sea/FISH-Dock/issues">反馈</a> · <a href="docs/release-1.2.2.md">更新记录</a></p>

> **这是二次开发集成，不是上游官方发行版。** FISH Dock 基于 [Wakuwaku](https://github.com/zhoupengjie/Wakuwaku) 的桌面与 AI 会话体验，集成 [Our Free Model / dsh-our-free-model](https://github.com/Ebony-Vinyl/dsh-our-free-model) 的模型渠道与 API 工作台。桌面核心与模型服务核心来自上游，原作者、版权和许可证见「来源与致谢」。

> **业务插件独立分发。** OFM 等插件的可用渠道和灰度状态见 [官方插件仓](https://github.com/Fish-under-sea/FISH-DOCK-Plugins)，本安装包不预装业务服务。

本仓库包含完整桌面宿主、插件 SDK、统一存储、运行时管理、安装器与测试。旧 OFM 服务副本已移出主仓，业务实现和发行包在官方插件仓维护。

## 一个工作台，两组能力

| 桌面停靠，来自 Wakuwaku | 模型接入，来自 OFM |
| --- | --- |
| 任务栏、灵动岛、宠物与 AI 会话状态 | 账号渠道、模型清单与 OpenAI 兼容 API |
| Agent 接入、插件、邮件与桌面组件 | 调用用量、本机 API Key 与独立 LAN Key |
| 保存模式、位置与屏幕，每次启动播放入场 | 按需启动独立服务，退出时回收自己的进程 |

**「本地」不等于本机离线推理。** 工作台、代理和数据在本机运行；模型请求仍发送到所选渠道的服务商。资格、额度、计费和可用性由服务商决定。本项目不承诺永久免费、无限额度或绕过服务限制。

## 1.2.2 更新

**1.2.2** 修好插件页的「自启动」开关：1.2.1 用的 `host.autostart` 会被页面桥按设计丢弃（`host.` 前缀留给插件后端），开关因此点不动；宿主侧接口改为 `dock.autostart`，插件页同步更新后即时生效，旧宿主上自动隐藏。1.2.1 的认证持久化、备份范围、PR #14 灵动岛落点、PR #15 账单分账与第三方插件都保留。见 [v1.2.2 Release](https://github.com/Fish-under-sea/FISH-Dock/releases/tag/v1.2.2)。

- 正式版检查更新：启动后及每天检查，支持手动检查；API 受限时回退官方 Release 页面，通过完整包或精简包升级。
- 应用内标语：「Everything is a Plugin · 一切皆插件」，宿主与独立业务插件职责清晰。
- 安装与更新：目标卷暂存、受管旧文件清理、重复校验复用、进度显示与失败回滚；提供完整包和精简更新包。
- 整仓配置备份：包含宿主设置、布局、插件注册、各命名空间的持久配置、凭据和名册；邮件正文不入包，预览后恢复并保留原副本，插件版本升级过也能恢复。
- 登录与窗口：验证码在紧凑的 Dock 内弹窗中显示；临时锁定可手动解除，灵动岛位置随验证结束恢复。
- 依赖发现：显示 Node 的实际来源、路径和版本；无效覆盖值可恢复到默认运行时。

继续保留以下能力：

- 独立插件平台：飞书、Clash、Magpie、OFM 按需安装，安装完成默认启用。
- GitHub 设备登录与主仓 Star 验证；自动复制设备码、轮询授权结果，按实际验证结果显示官方入口。
- 内化应用确认弹窗；移除旧自动锁定逻辑，验证期间只保留临时焦点状态。
- 修复全屏退出恢复、主动唤醒及审批显示；保存手动置顶与锁定偏好。
- 插件中心和「关于」支持检查更新，目录含 GitHub API 回退，包大小自动换算单位。
- 内置 Windhawk 与编译器；保留原生插件管理页、标签拖动及单双栏布局。

完整变化和验证边界见 [1.2.2 更新记录](docs/release-1.2.2.md)。

## 安装与更新

适用平台：**Windows 10/11 x64**。系统任务栏集成主要面向 Windows 11；其他系统版本与第三方任务栏软件组合需要单独验证。

1. 从 [本项目 Releases](https://github.com/Fish-under-sea/FISH-Dock/releases/tag/v1.2.2) 选择已发布版本的安装包并核对同版本 `SHA256SUMS.txt`。`FISH-Dock-Setup-1.2.2.exe` 用于首装或修复；`FISH-Dock-Update-1.2.2.exe` 适用于基础资源完整的已有 `1.1.0` 或此前 `1.2.2` 安装。
2. 从托盘完全退出旧版及单独运行的 OFM，再运行安装器并选择目录。新安装默认 `%LOCALAPPDATA%\Programs\FISH Dock`；已有安装沿用登记目录覆盖升级，不自动搬迁。
3. 通过桌面或开始菜单启动 **FISH Dock**。Node.js、固定版 WebView2、Windhawk 与编译器随包安装；业务插件另行下载。
4. 首次启动需要确认运行时许可并校验资源；后续使用安装目录中的文件，不重复解压整套依赖。

**安装器尚未进行 Authenticode 代码签名。** Windows 可能提示未知发布者或 SmartScreen。改名、换图标、自签名都不能可靠消除信誉提示。仅使用可信来源，核对摘要，不要关闭系统防护、导入不明根证书或绕过组织策略。通常不要求以管理员身份运行。

若出现 `Error writing temporary file. Make sure your temp folder is valid.`，这是 NSIS 执行安装脚本之前的临时目录错误，应检查临时目录、权限、磁盘空间和安全软件日志。覆盖安装出现 DLL 写入失败时，先确认旧版与其子进程已退出，不要忽略写入错误。

## 开始使用

**桌面**：首次默认任务栏；通过设置或托盘切换任务栏／灵动岛、边缘和左中右位置。退出重启保留选择，保存的显示器不可用时回退主屏。减少动态效果时使用短淡入。

**插件与模型**：在「插件」页完成 GitHub 登录与主仓 Star 验证后安装插件。Dock 内弹窗显示设备码及真实复制结果；网页确认后自动检查，等待期间可手动解除临时锁定或取消验证。新插件默认启用，模型服务仍需按页面配置并启动。API 地址和端口以插件页面为准，例如：

```text
http://127.0.0.1:18901/v1
```

Agent、邮件和第三方插件仍需要各自客户端、账号和权限；安装 FISH Dock 不会授予这些服务的使用资格。第三方登录、下载和图库链接保留真实来源。

### 局域网访问

1. 打开 OFM「API 接入 → 局域网访问」，在可信网络中启用开关。
2. 复制页面列出的当前设备地址和 **局域网 API Key**。远端客户端不能使用 `127.0.0.1`，也不能使用本机 API Key 代替 LAN Key。
3. 在另一设备配置 OpenAI 兼容客户端。轮换 LAN Key 后旧 Key 立即失效，需同步更新客户端。
4. 不用时关闭开关。地址不可用、端口被占用或网卡变化时，关闭后重新开启以重新绑定；只有成功绑定的地址才会显示。

管理页面仍仅允许本机访问。网络传输为 HTTP，只用于受信任的局域网；不要映射公网。程序不自动创建防火墙规则，也不承诺手机或另一台电脑一定可达，需检查网络隔离及系统规则。相同端口号不等于把本机管理监听开放到所有地址。

### 配置与凭据迁移

OFM「服务设置 → OFM 迁移备份」生成的 **OFM 专项 ZIP** 不含邮件和调用统计。它未加密，可能含本机 API Key、LAN Key、渠道凭据及 EAC 授权。

整仓迁移使用「通用 → FISH Dock 配置与备份」。ZIP 包含宿主设置、布局、插件注册与各命名空间的持久配置、凭据、名册和密钥；**邮件正文（`host/mail-store`）不入包**，账号名册与登录凭据照常带上。排除运行态记录（`trash`、`routing`、用量日志、模型与市场缓存）、缓存、临时文件、程序包及历史备份。默认导出目录为配置仓下的 `host/exports`，可修改。导出的 ZIP 含明文凭据，导入后重新进行本机加密；当前用户解不开的凭据原样带走，导入后在该机器重新登录一次即可，请按密码文件保管。

导入前退出独立 OFM，保持 FISH Dock 打开；程序会暂停自己的模型服务与邮件同步，检查预览后再次确认。整仓包按预览范围恢复并保留恢复前副本；旧备份继续按原格式范围处理，含 OFM 资料的旧包使用对应插件的迁移入口。失败时程序尝试回滚。旧 `%USERPROFILE%\.dsh-fish` 会复制迁移到 `%USERPROFILE%\.fish-dock`，原目录不删除；已有新目录始终优先，不自动合并。固定说明随 ZIP 以 `README.md` 提供。

## 数据与卸载

| 内容 | 位置 |
| --- | --- |
| 主程序和内置依赖 | 用户选择的安装目录，新安装默认 `%LOCALAPPDATA%\Programs\FISH Dock` |
| 设置、宠物、布局与日志 | 数据根下 `host` 命名空间 |
| WebView2 用户数据 | 数据根下 `host/webview` |
| 许可确认及资源校验状态 | 系统用户运行时状态目录 |
| 插件配置、凭据与身份 | 数据根下 `.fishplugin-*` 或 `.plugin.<publisher>.<name>`，按分类隔离 |

内部程序名 `DSH-FISH.exe`、应用标识、卸载登记和旧数据路径保留兼容，不代表显示名称未更新。卸载只移除自身安装文件与登记，保留用户数据；只清理确认属于本安装的自启动任务。手动删除数据前应先备份。

## 从源码构建

### GitHub 与插件配置

GitHub 登录使用公开 OAuth Client ID 与设备授权流程，令牌由宿主 DPAPI 加密保存，不交给插件。**认证一次即长期有效**：凭据存放在配置仓 `host/keys`，安装或更新到新版本都不改动它；每次启动（含更新后）自动复核一次个人身份与主仓 Star，离线只停在「待复核」，不会清除登录。OAuth 应用关闭令牌过期后签发的令牌本身不再到期，另有轮换刷新令牌兜底。可通过 `FISH_DOCK_GITHUB_CLIENT_ID` 指定自有应用。模型渠道的 OAuth 配置在对应插件仓维护。

桌面使用 **Rust + Tauri 2 + HTML/CSS/JavaScript**，模型管理使用 **Node.js + React**，安装器使用 **NSIS 3**。需要 Windows x64、MSVC/Rust、Node.js、NSIS，以及锁定的官方运行时下载件。离线构建要求依赖已缓存，不能忽略锁文件。

```text
FISH-Dock/
  Wakuwaku/              # 桌面源码与此 README
  sdk/                   # Node 后台和浏览器插件桥接 SDK
  licenses/              # 保留的历史组件许可证
```

在桌面源码目录执行；构建宿主不再需要旧 OFM 源码树：

```powershell
node scripts/prepare-installer.mjs --downloads <运行时缓存> --out <新的资源目录> --version 1.2.2
cargo test --locked --offline --features installed --manifest-path src-tauri/Cargo.toml -- --test-threads=1
node --test "test/*.test.js" "scripts/*.test.mjs"
./scripts/build-installer.ps1 -Resources <资源目录> -Cargo <cargo.exe> -Node <node.exe> -Makensis <makensis.exe>
```

版本和官方摘要锁定在 [runtime-sources.json](scripts/runtime-sources.json)。测试证据、环境限制和历史包见 [安装版构建记录](docs/installer-build.md)。安装包不含模型权重，也不含开发 Dock 所需的 Rust／MSVC 环境；内置 Windhawk 的 LLVM-MinGW 编译器用于模组编译。

## 来源与致谢

| 项目 | 原作者／维护者 | 保留的核心能力 | 许可 |
| --- | --- | --- | --- |
| [Wakuwaku](https://github.com/zhoupengjie/Wakuwaku) | Pengjie Zhou / zhoupengjie | 任务栏、灵动岛、宠物、会话与桌面功能 | [MIT](LICENSE)，Copyright 2026 Pengjie Zhou |
| [dsh-our-free-model](https://github.com/Ebony-Vinyl/dsh-our-free-model) | Ebony-Vinyl 及贡献者 | 模型渠道、账号、用量与兼容 API | 历史模型集成；MIT，Copyright 2026 Ebony-Vinyl；[许可原文](../licenses/Our-Free-Model.txt)，业务代码见插件仓 |

**Fish-under-sea** 负责 FISH Dock 集成与维护。新增集成代码使用 MIT，上游版权声明原样保留。不能将全部功能描述为 FISH Dock 原创，也不应让上游维护者承担本发行版的支持责任。

应用沿用用户提供的蓝发角色原图标，未重新绘制，也不宣称其为原创 MIT 素材。宠物精灵图、应用图标和图库资源不自动适用源码的 MIT 许可；再分发前应核实素材授权。Claude 小姐原角色设计来自 **ZipZipPipe**，像素二创来自 **zhoupengjie**。Node.js、WebView2、fflate 及其他第三方组件保留各自许可。

## 反馈与安全

在 [FISH Dock Issues](https://github.com/Fish-under-sea/FISH-Dock/issues) 提供系统与应用版本、停靠模式、复现步骤和脱敏日志。不要上传真实密钥、完整 Cookies、私人邮件或备份 ZIP。漏洞报告也不要公开附上可用凭据。1.2.2 双包及校验文件见上方正式 Release。
