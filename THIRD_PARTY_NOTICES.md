# 第三方组件声明（Third-Party Notices）

FISH Dock 本体以 MIT 许可发布。本文件列出它分发或依赖的第三方组件及其许可义务（v4 设计 §9.4）。仅当安装包实际捆绑了下述二进制/源码时，对应义务随该安装包生效。

## 1. mihomo 内核（Clash 页的本地内核，用户主动下载）

- 来源：`MetaCubeX/mihomo`，`Meta` 分支 release（例如 `mihomo-windows-amd64-v1.19.x.zip`）。
- 许可：**GPL-3.0**。注意：GitHub 仓库 API 的 `license` 字段显示 MIT，那是默认分支 `main` 上一个无关项目——内核在 `Meta` 分支，以该分支根目录的 `LICENSE`（GPL-3.0）为准。
- 获取方式：版本与来源 URL 记录在 `<数据目录>/clash/core/SOURCE.json`（内核经页面下载时写入）。
- 义务：随分发附 GPL-3.0 全文，并注明源码获取地址（上表 URL + 版本号）。
- dock 与内核的交互方式：以独立进程运行、经其 HTTP 控制器与本机回环通信，不链接其代码。

## 2. zashboard（Clash 页内嵌控制台，用户主动下载）

- 来源：`Zephyruso/zashboard`，官方产物 `dist-no-fonts.zip`。
- 许可：**MIT**（保留其版权与许可声明；产物包内自带）。
- 交互方式：静态文件由 mihomo 的 `external-ui` 同源托管，dock 仅在 iframe 中展示。

## 3. ZenDesktop 的 9 个 Windhawk mod（一键部署时下载，尚未实现）

- 来源：ZenDesktop 仓库根目录 7 个与 `optional/` 2 个 `local@*.wh.cpp`。
- 许可：**GPL-3.0**；其中 `windows-animations`（ReDrag）为 **MIT**；5 个基于 m417z 的 Windows 11 *Styler（GPL-3.0），需一并署名。
- 义务：随分发附 GPL-3.0 全文、仓库出处与各 mod 作者署名。此功能落地时本节需展开为逐 mod 清单。

## 4. Windhawk（ZenDesktop mod 的宿主；dock 不分发）

- 来源：`ramensoftware/windhawk`。
- 许可：**GPL-3.0**。dock 不分发、不链接 Windhawk 本体，仅在 Zen 页提供其官网链接（windhawk.net）与本地安装的探测。无随包义务。

## 5. 其他随包组件

- Node.js 运行时、WebView2 Fixed Version 运行时：随安装包分发，其许可见安装资源内的 `licenses/`（`NOTICE.txt`、`WebView2-Fixed-Version.html`）。
- `Wakuwaku`（上游桌面端）与 `dsh-our-free-model`（上游模型服务）：许可见各自目录的 `LICENSE`（UPSTREAM.json 有来源与提交记录）。
