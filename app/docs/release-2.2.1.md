# FISH Dock 2.2.1

2.2.1 是修复版，只发布 Setup 完整安装包。2.2.0 宿主在应用内更新时会下载并校验这个 Setup；更早的宿主请先升级到 2.2.0，或直接下载本版 Setup 覆盖安装。

## 修复

- **启动时不再因 WebView2 正在更新而崩溃（[#85](https://github.com/Fish-under-sea/FISH-Dock/issues/85)）。** Tauri 只在启动那一刻询问一次 WebView2，问不到就直接崩溃。现场是 EdgeUpdate 正把运行时从 156 换成 157：注册表还写着 156，旧版本目录已被换走。现在启动前最多等约 20 秒；运行时确实不可用时，弹出明确提示后退出，不再崩溃。
- **清除旧版留下的失效 WebView2 目录变量（#85）。** 2.1.8 及更早会把 `WEBVIEW2_BROWSER_EXECUTABLE_FOLDER` 指向随包的 `resources/webview2`，升级交接时这个变量可能被新进程继承，而新版已不带这个目录。变量指向的目录里没有 `msedgewebview2.exe` 时，现在直接清掉，改用系统 Evergreen；指向有效目录（用户或企业自己固定的运行时）则保留。
- **目录说明缺失时仍能完成换版本（[#86](https://github.com/Fish-under-sea/FISH-Dock/issues/86)）。** 插件清理 `tmp` 时会删掉宿主写在那里的 `README.md`，之后插件更新会在提交阶段报「缺少旧版本目录说明」。宿主自身升级也走同一段代码。现在缺失的说明按「原本不存在」记入事务日志并重建；2.2.0 写下、未完成的旧事务日志仍能读入。Magpie 插件侧的清理也已改为跳过宿主文件，随下一个插件版本发布。
- **托盘「关闭插件窗口」只通知一次 `closed`（[#83](https://github.com/Fish-under-sea/FISH-Dock/issues/83)）。** 之前插件会收到两次。

## 测试环境

- 实时进程祖先断言和任务计划根目录读取两条测试的失败（[#80](https://github.com/Fish-under-sea/FISH-Dock/issues/80)、[#81](https://github.com/Fish-under-sea/FISH-Dock/issues/81)）不是产品缺陷。在带 Low 强制完整性标签的目录编译时，测试程序以低完整性运行：进程快照看不到中完整性的父进程，任务计划根目录也返回 `0x80070003`。Dock 以中完整性运行，不受影响。测试在低完整性下只断言与环境无关的部分；以中完整性运行时完整断言照常执行，并已通过。
- 历史上一次 `0xC0000374` 堆损坏（[#82](https://github.com/Fish-under-sea/FISH-Dock/issues/82)）此后多次全量测试均未复现，也没有崩溃转储，按无法复现关闭；再次出现时请附上转储。

## 下载

| 安装包 | 适用场景 |
| --- | --- |
| `FISH-Dock-Setup-2.2.1.exe` | 首次安装、修复安装、2.2.0 应用内更新，以及旧版覆盖升级；包含 Node.js，使用系统 Evergreen WebView2 |

EXE 附同名 `.sha256`；GitHub 另附 `SHA256SUMS.txt`。安装包通过 7-Zip 完整性检查，包内资源清单为 FISH Dock 2.2.1、Node.js 24.14.0、1843 个文件，不含固定版 WebView2 或 Windhawk。安装包当前未做 Authenticode 签名。

| 包 | 字节数 | SHA-256 |
| --- | ---: | --- |
| Setup | 32887436 | `1d3e19261c33c3ab49b4a9cc432e74964986b360d7e8d8f2e9396b192176fb9d` |

## 验证

- Rust installed 全量：918 通过、1 失败、24 忽略；default 全量：917 通过、1 失败、24 忽略。两种模式唯一的失败都是 DACL 夹具测试，原因同样是低完整性测试进程无法在中完整性的 `%TEMP%` 下建目录；把同一测试程序放到无标签目录、以中完整性运行时通过。两轮均未出现 #82 的堆损坏。
- 本次修改的目标测试 29 项通过；Magpie 运行时测试 14 通过、0 失败、3 跳过。
- 原生窗口与安装升级矩阵已由维护者人工验收（[#84](https://github.com/Fish-under-sea/FISH-Dock/issues/84)）。
