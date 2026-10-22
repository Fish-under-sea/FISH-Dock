<p align="center"><img src="Wakuwaku/src-tauri/icons/dsh-fish.png" width="104" alt="FISH Dock"></p>
<h1 align="center">FISH Dock</h1>
<p align="center">把 AI 会话、模型渠道和桌面工具收进一个工作台。</p>
<p align="center"><strong>v1.0.2</strong> · Windows 10/11 x64 · Rust / Tauri / Node.js / React</p>
<p align="center"><a href="https://github.com/Fish-under-sea/FISH-Dock/releases/tag/v1.0.2">下载安装包</a> · <a href="Wakuwaku/docs/release-1.0.2.md">更新记录</a> · <a href="README.en.md">English</a> · <a href="https://github.com/Fish-under-sea/FISH-Dock/issues">反馈问题</a></p>

> **Meoo 目前仅属于灰度测试，暂时不可使用。** 本版保留实验入口和相关源码，不代表已开放登录、项目绑定或模型调用。请使用其他渠道，后续可用状态以本仓库公告为准。

FISH Dock 将任务栏、灵动岛、AI 会话状态、邮件和模型服务放在同一个桌面入口。仓库包含 **完整 Dock 与内置 OFM 源码**，服务端、React 前端、渠道适配、测试和安装脚本均在同一仓库维护。

## 能做什么

| 桌面工作台 | 内置模型工作台（OFM） |
| --- | --- |
| 任务栏与灵动岛、桌面宠物、Agent 会话状态 | 多渠道账号池、模型清单、用量与请求日志 |
| 邮件聚合、筛选和一键全部已读 | OpenAI 兼容 API、本机密钥、可选局域网推理 |
| 模型页锁定及置顶、其他应用全屏时提示 | 积分领取、余额刷新、凭据续期与故障诊断 |
| 身份管理、导入导出、更新后沿用原数据 | 统一厂商卡片、账号及模型折叠区、渠道备份 |

工作台与代理服务在本机运行，模型请求发送到所选服务商；安装包不含模型权重。渠道资格、额度、支持模型及实际可用性由服务商决定。

## v1.0.2 更新

- **WorkBuddy 恢复**：透明验证区分鉴权与暂时故障，429、超时、5xx 不再持久锁停；保留诊断和手动恢复入口。
- **邮件全部已读**：支持当前邮箱和聚合邮箱，分批确认、保留星标；窄窗口下筛选仍可见，操作有忙态与结果反馈。
- **窗口行为**：进入 OFM 默认锁定并置顶，关闭或离开顶级标签时解除。其他应用全屏时可选择保持，或取消两者；已关置顶但仍锁定时，连续全屏 2 秒后解除锁定。
- **账号与余额**：查询前按账号续期，并发请求合并，故障按账号隔离；新增账号也进入续期调度。
- **每日积分**：商汤增加「一键领取」，总览按厂商折叠核验今日到账与当前余额。实际金额以账单或领取回执为准，证据不足显示待确认。
- **界面统一**：14 个渠道入口统一图标、卡片和「添加账号」文案，保留各渠道已有功能；Qoder 明确区分国际版和国内版。
- **品牌与源码**：使用指定立绘图标，同步完整 Dock / OFM 源码和构建入口。

Meoo 相关界面及授权流程属于实验代码，**本版暂不可使用**。完整变化与验证范围见 [版本说明](Wakuwaku/docs/release-1.0.2.md)。

## 安装与更新

1. 从 [v1.0.2 Release](https://github.com/Fish-under-sea/FISH-Dock/releases/tag/v1.0.2) 下载 `FISH-Dock-Setup-1.0.2.exe` 和同名 SHA-256 文件。
2. 从托盘正常退出旧 FISH Dock，同时退出单独运行的 OFM。
3. 运行安装器，沿用原安装目录直接更新，无需先卸载。新安装默认位于 `%LOCALAPPDATA%\Programs\FISH Dock`。
4. 启动后核对原渠道账号、API Key 和邮件身份，再使用新功能。

安装包包含 Node.js、固定版 WebView2 和 OFM 服务，无需另外安装这些运行时。首次启动校验资源并确认运行时许可，后续直接使用安装目录内资源。安装器尚未进行 Authenticode 签名，Windows 可能显示未知发布者提示。

校验下载文件：

```powershell
Get-FileHash .\FISH-Dock-Setup-1.0.2.exe -Algorithm SHA256
```

请与 Release 附带的 `.exe.sha256` 比对。若覆盖安装提示文件占用，先确认旧程序及其子进程已退出。

## 开始使用

**桌面**：在设置或托盘切换任务栏／灵动岛、边缘、位置和显示器。保存的显示器不可用时回退主屏。

**模型**：打开「本地模型」，为可用渠道添加账号，在「API 接入」查看本机地址与密钥，然后配置兼容客户端。常见地址如下，实际端口以页面为准：

```text
http://127.0.0.1:18901/v1
```

**邮件**：「全部已读」会修改服务器端已读状态。批量操作按固定未读快照执行，新到邮件不并入此前批次；部分失败会报告已经确认的数量。

**每日积分**：商汤先查当日账单，未确认时续期并复查；只有实际到账才显示成功。某些账号仍需在官网完成当日登录，不保证仅续期就触发发放。

### 局域网推理

在「API 接入 → 局域网访问」中开启，复制设备地址和独立 LAN Key 到另一设备。LAN 默认关闭，仅开放推理接口，管理页和备份仍限本机。程序不自动添加防火墙规则；网络为 HTTP，适用于受信任局域网。轮换密钥后需同步更新客户端。

### 身份与备份

完整迁移使用 Dock「通用 → 身份与备份」的完整身份 ZIP，可预览导入内容，并在替换前保留恢复副本。OFM「服务设置」中的迁移包仅包含 OFM，不替换邮件身份。

ZIP 可能包含明文账号令牌和密钥，请按密码文件保管。邮件密码在本机静态保存时使用当前 Windows 用户 DPAPI 加密。

| 数据 | 默认位置 |
| --- | --- |
| OFM 账号、密钥、用量与邮件身份 | `%USERPROFILE%\.fish-dock` |
| Dock 设置、布局、日志 | `%LOCALAPPDATA%\DSH-FISH\data` |
| WebView2 数据 | `%LOCALAPPDATA%\DSH-FISH\webview` |
| 许可和资源校验状态 | `%LOCALAPPDATA%\DSH-FISH\state` |

保留 `FISH_DOCK_DATA`、旧 `DSH_FISH_DATA` 覆盖值。旧 `.dsh-fish` 仅在新目录缺失时复制校验，原件保留；两者同时存在时不自动合并。更新沿用账号 ID、credentialRef 和密钥索引；卸载保留用户数据。

## 源码结构

```text
FISH-Dock/
├─ Wakuwaku/                 # Rust / Tauri 桌面、邮件、身份、窗口、安装器
├─ dsh-our-free-model/        # 完整 OFM 服务、React 前端、渠道、测试及构建脚本
├─ tools/                    # 公开源码配置与文档检查
├─ SOURCE.md                 # 集成范围和发布说明
└─ UPSTREAM.json             # 上游版本、提交及许可来源
```

没有 Git 子模块或指向本机目录的源码链接。运行数据、真实账号、密钥、开发依赖缓存和安装二进制不放入 Git；安装包见 Releases。

## 从源码构建

需要 Windows x64、Rust / MSVC、Node.js 22.19+ 或 24+、NSIS 3。官方运行时版本和摘要锁定在 [runtime-sources.json](Wakuwaku/scripts/runtime-sources.json)。先将其中列出的 Node、WebView2 和许可文件下载到本机缓存目录；离线构建还需先缓存 Cargo 依赖。

```powershell
git clone https://github.com/Fish-under-sea/FISH-Dock.git
Set-Location FISH-Dock
npm --prefix dsh-our-free-model/packages/standalone ci
node dsh-our-free-model/scripts/build-standalone-ui.mjs

# 新建安装资源目录：完整打入本仓库的 OFM 服务
node Wakuwaku/scripts/prepare-installer.mjs --downloads C:/build/runtime-cache --source dsh-our-free-model --out C:/build/fish-resources-102 --version 1.0.2

# 首次联网解析并缓存锁定依赖；后续可使用 --offline
cargo fetch --locked --manifest-path Wakuwaku/src-tauri/Cargo.toml
./Wakuwaku/scripts/build-installer.ps1 -Resources C:/build/fish-resources-102 -Node (Get-Command node).Source -Cargo (Get-Command cargo).Source -Makensis 'C:/Program Files (x86)/NSIS/makensis.exe' -Output C:/build/FISH-Dock-Setup-1.0.2.exe
```

已提交构建后的管理页面，直接打包使用仓库版本；修改前端时请先重建。渠道包修改需按 OFM 自带构建脚本生成对应产物。

### Gemini OAuth 配置

公开源码和此 Release 不携带 Google OAuth 默认客户端参数。使用该渠道前自行配置 `CMDC_PAK_GOOGLE_CLIENT_ID` 和 `CMDC_PAK_GOOGLE_CLIENT_SECRET`，并在相同环境中启动服务；其他渠道不依赖这两项配置。不要把实际值写入 Git 或问题反馈。

### 检查与验证

```powershell
node tools/check-readme.mjs
node --test tools/oauth-publication.test.mjs
node --test Wakuwaku/test/settings-focus.test.js Wakuwaku/test/mail-read-all.test.js Wakuwaku/scripts/branding.test.mjs
node dsh-our-free-model/scripts/workbuddy-validation-test.mjs
node dsh-our-free-model/scripts/meoo-test.mjs
node dsh-our-free-model/scripts/raccoon-daily-test.mjs
cargo test --locked --offline --features installed --manifest-path Wakuwaku/src-tauri/Cargo.toml model_focus -- --test-threads=1
```

自动化覆盖协议、状态机、隔离 RPC 及浏览器渲染。真实 Windows 全屏行为、邮箱服务器、服务商账号授权和跨日到账仍需实机验证；测试通过不代表所有渠道随时可用。具体结果见 [版本说明](Wakuwaku/docs/release-1.0.2.md)。

## 来源与致谢

FISH Dock 是基于 [Wakuwaku](https://github.com/zhoupengjie/Wakuwaku) 和 [dsh-our-free-model](https://github.com/Ebony-Vinyl/dsh-our-free-model) 的集成与二次开发，由 [Fish-under-sea](https://github.com/Fish-under-sea) 维护。

- **Wakuwaku**：Pengjie Zhou / zhoupengjie，桌面、会话及桌面组件核心；[MIT](Wakuwaku/LICENSE)。
- **OFM**：Ebony-Vinyl 及贡献者，模型渠道与服务核心；[MIT](dsh-our-free-model/LICENSE)。
- Node.js、WebView2、fflate 及各组件保留原有许可。角色、宠物和厂商图标不自动适用源码 MIT 许可；Claude 小姐原设计来自 ZipZipPipe，像素二创来自 zhoupengjie。

集成范围见 [SOURCE.md](SOURCE.md) 和 [UPSTREAM.json](UPSTREAM.json)。反馈请到 [Issues](https://github.com/Fish-under-sea/FISH-Dock/issues)，附版本、复现步骤与脱敏日志。
