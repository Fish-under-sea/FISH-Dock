<div align="center">

<p>
  <a href="README.md"><picture><source media="(prefers-color-scheme: dark)" srcset="assets/readme/key-zh-zh-dark.png"><img src="assets/readme/key-zh-zh-light.png" alt="中文" height="40"></picture></a>
  <a href="README.en.md"><picture><source media="(prefers-color-scheme: dark)" srcset="assets/readme/key-en-zh-dark.png"><img src="assets/readme/key-en-zh-light.png" alt="English" height="40"></picture></a>
  <a href="#get-started"><picture><source media="(prefers-color-scheme: dark)" srcset="assets/readme/key-start-zh-dark.png"><img src="assets/readme/key-start-zh-light.png" alt="入门指南" height="40"></picture></a>
</p>

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="assets/readme/hero-zh-dark.png">
  <img src="assets/readme/hero-zh-light.png" alt="FISH Dock：桌面与模型工作台" width="1448">
</picture>

<p>
  <a href="https://github.com/Fish-under-sea/FISH-Dock/releases/tag/v1.1.0"><picture><source media="(prefers-color-scheme: dark)" srcset="assets/readme/stat-version-zh-dark.png"><img src="assets/readme/stat-version-zh-light.png" alt="VERSION：当前版本 1.1.0" width="23%"></picture></a>
  <a href="#features"><picture><source media="(prefers-color-scheme: dark)" srcset="assets/readme/stat-desktop-zh-dark.png"><img src="assets/readme/stat-desktop-zh-light.png" alt="DESKTOP：桌面岛与工作台" width="23%"></picture></a>
  <a href="#workspace"><picture><source media="(prefers-color-scheme: dark)" srcset="assets/readme/stat-mail-zh-dark.png"><img src="assets/readme/stat-mail-zh-light.png" alt="WORKSPACE：邮件与模型工作区" width="23%"></picture></a>
  <a href="https://github.com/Fish-under-sea/FISH-DOCK-Plugins"><picture><source media="(prefers-color-scheme: dark)" srcset="assets/readme/stat-channels-zh-dark.png"><img src="assets/readme/stat-channels-zh-light.png" alt="PLUGINS：四个官方插件" width="23%"></picture></a>
</p>

<p><img src="https://img.shields.io/badge/Windows-10%2F11%20x64-8fd6a0?style=flat-square&logo=windows&logoColor=white&labelColor=5a544a" alt="Windows x64"> <img src="https://img.shields.io/badge/Rust-Tauri-8fd6a0?style=flat-square&logo=rust&logoColor=white&labelColor=5a544a" alt="Rust Tauri"> <img src="https://img.shields.io/badge/Node.js-24.14%2B-8fd6a0?style=flat-square&logo=nodedotjs&logoColor=white&labelColor=5a544a" alt="Node.js"> <img src="https://img.shields.io/badge/License-MIT-8fd6a0?style=flat-square&labelColor=5a544a" alt="MIT License"> <img src="https://img.shields.io/badge/Official%20Plugins-4-ffd84a?style=flat-square&labelColor=5a544a" alt="Four official plugins"></p>
<p><strong>Everything is a Plugin · 一切皆插件</strong><br>一个安静的桌面宿主，把会话、模型和工具收进同一个工作台。</p>

<p>
  <a href="#get-started"><picture><source media="(prefers-color-scheme: dark)" srcset="assets/readme/key-download-zh-dark.png"><img src="assets/readme/key-download-zh-light.png" alt="下载 / Releases" height="40"></picture></a>
  <a href="Wakuwaku/docs/release-1.1.0.md"><picture><source media="(prefers-color-scheme: dark)" srcset="assets/readme/key-release-zh-dark.png"><img src="assets/readme/key-release-zh-light.png" alt="更新记录" height="40"></picture></a>
  <a href="https://github.com/Fish-under-sea/FISH-Dock/issues"><picture><source media="(prefers-color-scheme: dark)" srcset="assets/readme/key-issues-zh-dark.png"><img src="assets/readme/key-issues-zh-light.png" alt="反馈 / Issues" height="40"></picture></a>
</p>

</div>

<h2 id="intro"><picture><source media="(prefers-color-scheme: dark)" srcset="assets/readme/section-intro-dark.png"><img src="assets/readme/section-intro-light.png" alt="INTRO · 简介" width="100%"></picture></h2>

FISH Dock 是 Windows 桌面上的会话与模型工作台。它负责窗口、桌面岛、运行时生命周期、插件页面和本地数据边界；具体业务以插件形式装载，让宿主保持清晰、可替换、可恢复。

桌面能力与模型能力在同一个工作区里协作：从桌面岛查看状态，从工作台管理渠道和模型，再按需安装通知、代理、模型网关和本地模型插件。

<h2 id="features"><picture><source media="(prefers-color-scheme: dark)" srcset="assets/readme/section-features-dark.png"><img src="assets/readme/section-features-light.png" alt="FEATURES · 核心能力" width="100%"></picture></h2>

| 模块 | 说明 |
| --- | --- |
| 桌面岛 | 展示会话状态、任务进度、通知和资源信息。 |
| 模型工作台 | 管理模型渠道、账号、模型目录和 API 配置。 |
| 插件中心 | 校验来源、平台和逐文件 SHA-256；支持检查更新、默认启用、停用、卸载与失败恢复。 |
| 外观工具链 | 随安装包提供 Windhawk 与编译器资源，由外观插件按需调用。 |
| 工作区 | 通过插件接入飞书通知、Clash 代理、Magpie 网关和 OFM 本地模型。 |

<h2 id="workspace"><picture><source media="(prefers-color-scheme: dark)" srcset="assets/readme/section-features-dark.png"><img src="assets/readme/section-features-light.png" alt="WORKSPACE · 工作区" width="100%"></picture></h2>

| 工作区 | 作用 |
| --- | --- |
| 桌面岛 | 查看会话状态、任务进度、通知和资源信息。 |
| 邮件管理 | 聚合收件、筛选、快捷操作和单双栏布局。 |
| 模型工作台 | 管理渠道账号、模型目录和 API 配置。 |
| 插件中心 | 安装官方插件、查看运行状态和恢复旧版本。 |

业务源码、插件包和商店目录由 [FISH-DOCK-Plugins](https://github.com/Fish-under-sea/FISH-DOCK-Plugins) 维护。本仓负责桌面宿主、SDK、安装器、插件管理与 Windhawk。

`1.1.0` 是插件化大更新：应用内确认框统一内化，登录期间的临时锁定与置顶在关闭、取消或到期后释放；全屏结束恢复桌面岛，主动唤醒和待处理审批也可在全屏期间显示。「关于」和插件卡片提供独立更新检查，包体积自动换算为 KB／MB／GB。

<h2 id="get-started"><picture><source media="(prefers-color-scheme: dark)" srcset="assets/readme/section-install-dark.png"><img src="assets/readme/section-install-light.png" alt="INSTALL · 安装使用" width="100%"></picture></h2>

1. 从 [Releases](https://github.com/Fish-under-sea/FISH-Dock/releases/tag/v1.1.0) 下载 `FISH-Dock-Setup-1.1.0.exe`，用同页 `SHA256SUMS.txt` 核对摘要。
2. 安装后从桌面或开始菜单打开 FISH Dock，首次启动会核对运行资源并展示运行时条款。
3. 打开「插件」页，点击「GitHub 登录」。验证码会自动复制，在系统浏览器中粘贴并确认；Dock 自动检查授权结果。
4. 为 [FISH Dock 主仓](https://github.com/Fish-under-sea/FISH-Dock) 点 Star，再点击「检查登录与 Star」。登录和 Star 均验证通过后，官方插件安装入口及对应功能页才会显示。
5. 点击「检查更新」读取官方目录，按需安装 [FISH Dock Official Plugins](https://github.com/Fish-under-sea/FISH-DOCK-Plugins)。新插件安装成功即启用；凭据和服务参数仍需各自配置。

安装包包含 Node.js 24.14.0、固定版 WebView2 155.0.4283.62、Windhawk 1.7.3 及 LLVM-MinGW 编译器，供对应插件按需调用；无需另外安装 Windhawk。安装包当前未做 Authenticode 签名。

<h2 id="data"><picture><source media="(prefers-color-scheme: dark)" srcset="assets/readme/section-intro-dark.png"><img src="assets/readme/section-intro-light.png" alt="DATA · 数据边界" width="100%"></picture></h2>

启动时解析完整数据根：`--data-root` → `FISH_DOCK_HOME` → 当前用户目录下 `.fish-dock`。宿主、插件和凭据都在这个根目录下按命名空间分开保存。

| 归属 | 根内目录 |
| --- | --- |
| 宿主 | `host` |
| 官方插件 | `.fishplugin-<name>` |
| 第三方插件 | `.plugin.<publisher>.<name>` |

每个命名空间继续按 `credentials`、`roster`、`keys`、`config`、`exports`、`backups`、`tmp` 分类。备份可能包含账号和密钥，请作为个人资料保存。

<h2 id="development"><picture><source media="(prefers-color-scheme: dark)" srcset="assets/readme/section-develop-dark.png"><img src="assets/readme/section-develop-light.png" alt="DEVELOP · 开发构建" width="100%"></picture></h2>

需要 Rust MSVC 工具链、Windows SDK 和 Node.js 24。开发版可以通过 `FISH_DOCK_DEV_NODE`、`FISH_DOCK_DEV_PLUGINS` 指定运行时，发行版使用安装资源与插件目录。

```powershell
cargo run --locked --no-default-features --manifest-path Wakuwaku/src-tauri/Cargo.toml
node --test --test-isolation=none 'sdk/node/*.test.mjs' 'Wakuwaku/test/*.test.js'
cargo test --locked --no-default-features --manifest-path Wakuwaku/src-tauri/Cargo.toml -- --test-threads=1
```

插件源码、插件包和目录维护在 [FISH-DOCK-Plugins](https://github.com/Fish-under-sea/FISH-DOCK-Plugins)。

<h2 id="source"><picture><source media="(prefers-color-scheme: dark)" srcset="assets/readme/section-intro-dark.png"><img src="assets/readme/section-intro-light.png" alt="SOURCE · 来源许可" width="100%"></picture></h2>

FISH Dock 基于 [Wakuwaku](https://github.com/zhoupengjie/Wakuwaku) 演进，保留原作者版权与 MIT 许可；历史模型能力源自 [Our Free Model](https://github.com/Ebony-Vinyl/dsh-our-free-model)。原始品牌素材见 [素材记录](assets/readme/SOURCES.md)。Windhawk、Node.js、WebView2 和编译器的原文许可随运行资源交付。

<p align="center"><sub>FISH Dock · Desktop + Model Workspace · Windows x64</sub></p>
