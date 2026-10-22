<div align="center">
<p>
<a href="README.md"><picture><source media="(prefers-color-scheme: dark)" srcset="assets/readme/key-zh-zh-dark.png"><img src="assets/readme/key-zh-zh-light.png" width="74" alt="中文"></picture></a>
<a href="README.en.md"><picture><source media="(prefers-color-scheme: dark)" srcset="assets/readme/key-en-zh-dark.png"><img src="assets/readme/key-en-zh-light.png" width="108" alt="English"></picture></a>
<a href="#开始使用"><picture><source media="(prefers-color-scheme: dark)" srcset="assets/readme/key-start-zh-dark.png"><img src="assets/readme/key-start-zh-light.png" width="126" alt="入门指南"></picture></a>
</p>
<h1><picture><source media="(prefers-color-scheme: dark)" srcset="assets/readme/hero-zh-dark.png"><img src="assets/readme/hero-zh-light.png" width="1440" alt="FISH Dock：AI 会话有处停靠，模型连接集中管理。原图标与用户指定透明挂件。"></picture></h1>
<p>
<a href="Wakuwaku/docs/release-1.0.2.md"><picture><source media="(prefers-color-scheme: dark)" srcset="assets/readme/stat-version-zh-dark.png"><img src="assets/readme/stat-version-zh-light.png" width="23%" alt="当前版本 v1.0.2"></picture></a>
<a href="#开始使用"><picture><source media="(prefers-color-scheme: dark)" srcset="assets/readme/stat-desktop-zh-dark.png"><img src="assets/readme/stat-desktop-zh-light.png" width="23%" alt="任务栏与灵动岛两种停靠模式"></picture></a>
<a href="#邮件管理"><picture><source media="(prefers-color-scheme: dark)" srcset="assets/readme/stat-mail-zh-dark.png"><img src="assets/readme/stat-mail-zh-light.png" width="23%" alt="邮件管理：聚合收件箱，一键全部已读"></picture></a>
<a href="#渠道接入"><picture><source media="(prefers-color-scheme: dark)" srcset="assets/readme/stat-channels-zh-dark.png"><img src="assets/readme/stat-channels-zh-light.png" width="23%" alt="已接入 14 个渠道入口，其中 Meoo 灰度测试、暂不可使用"></picture></a>
</p>
<p>
<a href="https://github.com/Fish-under-sea/FISH-Dock/releases/tag/v1.0.2"><picture><source media="(prefers-color-scheme: dark)" srcset="assets/readme/key-download-zh-dark.png"><img src="assets/readme/key-download-zh-light.png" width="168" alt="下载与 Releases，未发布时无安装包"></picture></a>
<a href="Wakuwaku/docs/release-1.0.2.md"><picture><source media="(prefers-color-scheme: dark)" srcset="assets/readme/key-release-zh-dark.png"><img src="assets/readme/key-release-zh-light.png" width="112" alt="更新记录"></picture></a>
<a href="https://github.com/Fish-under-sea/FISH-Dock/issues"><picture><source media="(prefers-color-scheme: dark)" srcset="assets/readme/key-issues-zh-dark.png"><img src="assets/readme/key-issues-zh-light.png" width="160" alt="问题反馈"></picture></a>
</p>
</div>

FISH Dock **1.0.2** · Windows x64 · MIT 代码许可。README 排版与素材来源见 [说明](assets/readme/SOURCES.md)。

> **这是二次开发集成，不是上游官方发行版。** FISH Dock 基于 [Wakuwaku](https://github.com/zhoupengjie/Wakuwaku) 的桌面与 AI 会话体验，集成 [Our Free Model / dsh-our-free-model](https://github.com/Ebony-Vinyl/dsh-our-free-model) 的模型渠道与 API 工作台。桌面核心与模型服务核心来自上游，原作者、版权和许可证见「来源与致谢」。

> **Meoo 目前仅属于灰度测试，暂时不可使用。** 本版保留实验界面和源码，暂未开放可用的登录、项目绑定及模型调用。

本仓库包含完整 Dock 与 OFM 源码：Rust / Tauri 桌面、Node.js 服务、React 前端、渠道适配、测试及构建脚本。

## 一个工作台，两组能力

| 桌面停靠，来自 Wakuwaku | 模型接入，来自 OFM |
| --- | --- |
| 任务栏、灵动岛、宠物与 AI 会话状态 | 账号渠道、模型清单与 OpenAI 兼容 API |
| Agent 接入、插件、邮件与桌面组件 | 调用用量、本机 API Key 与独立 LAN Key |
| 保存模式、位置与屏幕，每次启动播放入场 | 按需启动独立服务，退出时回收自己的进程 |

**「本地」不等于本机离线推理。** 工作台、代理和数据在本机运行；模型请求仍发送到所选渠道的服务商。资格、额度、计费和可用性由服务商决定。本项目不承诺永久免费、无限额度或绕过服务限制。

## 1.0.2 更新

- **WorkBuddy 恢复**：限流、超时等暂时故障不再持久锁停透明验证，保留诊断和手动恢复入口。
- **邮件全部已读**：支持当前与聚合邮箱，保留其他标志并报告已确认进度；窄窗口下筛选保持可见。
- **窗口行为**：进入 OFM 默认锁定并置顶，离开或关闭时解除；其他应用全屏时询问保持或取消两者。已关置顶但仍锁定时，连续全屏 2 秒后解除锁定。
- **账号与积分**：按账号续期、恢复余额查询；商汤一键领取和按厂商折叠的今日核验均以实际到账证据为准，不写死金额。
- **厂商界面统一**：14 个渠道入口采用统一卡片、图标及账号／模型折叠区，保留各渠道已有功能；Qoder 区分国际版与国内版。
- **兼容与完整源码**：保留 1.0.1 身份迁移、完整身份 ZIP、CodeArts 专用传输和严格模型测试结果；公开完整 Dock 与 OFM 源码，沿用指定原始图标。

**Meoo 仍处于灰度测试，暂不可使用。** 完整变化、验证证据和实机边界见 [1.0.2 更新记录](Wakuwaku/docs/release-1.0.2.md)。

## 安装与更新

适用平台：**Windows 10/11 x64**。系统任务栏集成主要面向 Windows 11；其他系统版本与第三方任务栏软件组合需要单独验证。

1. 从 [本项目 Releases](https://github.com/Fish-under-sea/FISH-Dock/releases/tag/v1.0.2) 下载 `FISH-Dock-Setup-1.0.2.exe`，核对随包提供的 `SHA256SUMS.txt`。
2. 从托盘完全退出旧版及单独运行的 OFM，再运行安装器并选择目录。新安装默认 `%LOCALAPPDATA%\Programs\FISH Dock`；已有安装沿用登记目录覆盖升级，不自动搬迁。
3. 通过桌面或开始菜单启动 **FISH Dock**。完整 Node.js、固定版 WebView2、模型服务和许可随包安装，无需另装运行时。
4. 首次启动需要确认运行时许可并校验资源；后续使用安装目录中的文件，不重复解压整套依赖。

**安装器尚未进行 Authenticode 代码签名。** Windows 可能提示未知发布者或 SmartScreen。改名、换图标、自签名都不能可靠消除信誉提示。仅使用可信来源，核对摘要，不要关闭系统防护、导入不明根证书或绕过组织策略。通常不要求以管理员身份运行。

若出现 `Error writing temporary file. Make sure your temp folder is valid.`，这是 NSIS 执行安装脚本之前的临时目录错误，应检查临时目录、权限、磁盘空间和安全软件日志。覆盖安装出现 DLL 写入失败时，先确认旧版与其子进程已退出，不要忽略写入错误。

## 开始使用

**桌面**：首次默认任务栏；通过设置或托盘切换任务栏／灵动岛、边缘和左中右位置。退出重启保留选择，保存的显示器不可用时回退主屏。减少动态效果时使用短淡入。

**模型**：打开设置中的「本地模型」，添加有权使用的渠道，然后从「API 接入」复制本机地址与密钥。常见地址如下，实际端口以页面为准：

```text
http://127.0.0.1:18901/v1
```

Agent、邮件和第三方插件仍需要各自客户端、账号和权限；安装 FISH Dock 不会授予这些服务的使用资格。第三方登录、下载和图库链接保留真实来源。

### 邮件管理

在「邮件」中绑定自己的邮箱，可查看单个或聚合收件箱，并一键将所选收件箱全部标记为已读。操作显示已确认进度，保留邮件的其他标志。

### 渠道接入

目前展示 **14 个已接入渠道入口**，其中 Meoo 为灰度测试、暂不可使用。其他渠道通过「本地模型 → 添加账号」绑定；登录方式、资格和额度以各服务商为准。

### 局域网访问

1. 打开 OFM「API 接入 → 局域网访问」，在可信网络中启用开关。
2. 复制页面列出的当前设备地址和 **局域网 API Key**。远端客户端不能使用 `127.0.0.1`，也不能使用本机 API Key 代替 LAN Key。
3. 在另一设备配置 OpenAI 兼容客户端。轮换 LAN Key 后旧 Key 立即失效，需同步更新客户端。
4. 不用时关闭开关。地址不可用、端口被占用或网卡变化时，关闭后重新开启以重新绑定；只有成功绑定的地址才会显示。

管理页面仍仅允许本机访问。网络传输为 HTTP，只用于受信任的局域网；不要映射公网。程序不自动创建防火墙规则，也不承诺手机或另一台电脑一定可达，需检查网络隔离及系统规则。相同端口号不等于把本机管理监听开放到所有地址。

### 配置与凭据迁移

在 OFM「服务设置 → OFM 迁移备份」确认后，可下载 **OFM 专项 ZIP**。它包含 OFM 配置、账号索引及关联凭据、现有 EAC 授权，不含邮件身份或调用统计。ZIP 未加密，可能包含账号 Token、本机 API Key 和 LAN Key，应像密码文件一样保管。

完整迁移请从 FISH Dock「通用 → 身份与备份」导出 **完整身份 ZIP**。它包含 OFM 用量与 Token 账本、邮件账号和密码；密码仅在本机静态保存时使用当前 Windows 用户 DPAPI 加密，ZIP 内为明文。导出前会显示系统「下载」目录，路径可以编辑，保存时还可另选位置。不要上传任何备份 ZIP。

导入前先退出独立 OFM；保持 FISH Dock 打开，在「身份与备份」选择 ZIP 并检查预览。程序会暂停自己的模型服务和邮件同步。完整身份包会替换白名单中的 OFM 与邮件文件，并保留替换前副本；旧版 OFM 专项包只替换 OFM 文件，邮件保持不变。导入需要再次确认；启动失败时程序会尝试回滚。旧 `%USERPROFILE%\.dsh-fish` 会迁移到 `%USERPROFILE%\.fish-dock`，原目录保留，不自动合并；两者同时存在时以新目录为准。详见 ZIP 内固定的 `README.md`。

## 数据与卸载

| 内容 | 位置 |
| --- | --- |
| 主程序和内置依赖 | 用户选择的安装目录，新安装默认 `%LOCALAPPDATA%\Programs\FISH Dock` |
| 设置、宠物、布局与日志 | `%LOCALAPPDATA%\DSH-FISH\data` |
| WebView2 用户数据 | `%LOCALAPPDATA%\DSH-FISH\webview` |
| 许可确认及资源校验状态 | `%LOCALAPPDATA%\DSH-FISH\state` |
| OFM 账号、密钥、用量与邮件身份 | `%USERPROFILE%\.fish-dock` |

内部程序名 `DSH-FISH.exe`、应用标识、卸载登记和旧数据路径保留兼容，不代表显示名称未更新。卸载只移除自身安装文件与登记，保留用户数据；只清理确认属于本安装的自启动任务。手动删除数据前应先备份。

## 从源码构建

### Gemini OAuth 客户端配置

公开源码已移除上游内嵌的 Google OAuth 客户端 ID 和 Secret。使用 Gemini OAuth 前，请自行注册并配置 `CMDC_PAK_GOOGLE_CLIENT_ID` 与 `CMDC_PAK_GOOGLE_CLIENT_SECRET`，然后在相同环境中启动程序；不配置时无法完成该渠道的 OAuth 授权。不要把真实值写入源码、提交记录或工单。1.0.2 源码和本次重建的安装包均沿用这项约束；不包含真实 OAuth 客户端秘密。

桌面使用 **Rust + Tauri 2 + HTML/CSS/JavaScript**，模型管理使用 **Node.js + React**，安装器使用 **NSIS 3**。需要 Windows x64、MSVC/Rust、Node.js、NSIS，以及锁定的官方运行时下载件。离线构建要求依赖已缓存，不能忽略锁文件。

```text
FISH-Dock/
  Wakuwaku/              # 桌面源码与此 README
  dsh-our-free-model/     # 集成的服务源码与上游许可
```

先在 `dsh-our-free-model/packages/standalone` 执行 `npm ci`，再在模型服务根目录执行 `node scripts/build-standalone-ui.mjs`。然后在桌面源码目录执行：

```powershell
node scripts/prepare-installer.mjs --downloads <运行时缓存> --source ../dsh-our-free-model --out <新的资源目录> --version 1.0.2
cargo test --locked --offline --features installed --manifest-path src-tauri/Cargo.toml -- --test-threads=1
node --test "test/*.test.js" "scripts/*.test.mjs"
./scripts/build-installer.ps1 -Resources <资源目录> -Cargo <cargo.exe> -Node <node.exe> -Makensis <makensis.exe>
```

版本和官方摘要锁定在 [runtime-sources.json](Wakuwaku/scripts/runtime-sources.json)。测试证据、环境限制和历史包见 [安装版构建记录](Wakuwaku/docs/installer-build.md)。安装包不是模型权重包，也不包含开发工具链。

## 来源与致谢

| 项目 | 原作者／维护者 | 保留的核心能力 | 许可 |
| --- | --- | --- | --- |
| [Wakuwaku](https://github.com/zhoupengjie/Wakuwaku) | Pengjie Zhou / zhoupengjie | 任务栏、灵动岛、宠物、会话与桌面功能 | [MIT](Wakuwaku/LICENSE)，Copyright 2026 Pengjie Zhou |
| [dsh-our-free-model](https://github.com/Ebony-Vinyl/dsh-our-free-model) | Ebony-Vinyl 及贡献者 | 模型渠道、账号、用量与兼容 API | MIT，Copyright 2026 Ebony-Vinyl；发行包保留其 LICENSE 和第三方声明 |

**Fish-under-sea** 负责 FISH Dock 集成与维护。新增集成代码使用 MIT，上游版权声明原样保留。不能将全部功能描述为 FISH Dock 原创，也不应让上游维护者承担本发行版的支持责任。

应用沿用用户提供的蓝发角色原图标，未重新绘制，也不宣称其为原创 MIT 素材。宠物精灵图、应用图标和图库资源不自动适用源码的 MIT 许可；再分发前应核实素材授权。Claude 小姐原角色设计来自 **ZipZipPipe**，像素二创来自 **zhoupengjie**。Node.js、WebView2、fflate 及其他第三方组件保留各自许可。

## 反馈与安全

在 [FISH Dock Issues](https://github.com/Fish-under-sea/FISH-Dock/issues) 提供系统与应用版本、停靠模式、复现步骤和脱敏日志。不要上传真实密钥、完整 Cookies、私人邮件或备份 ZIP。漏洞报告也不要公开附上可用凭据。安装包与校验文件已在上方 v1.0.2 Release 提供。
