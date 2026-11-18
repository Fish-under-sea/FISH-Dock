# FISH Dock 2.3.6

2.3.6 是 2.3.5 之后的修复版本，合并来自 [@jassdingk](https://github.com/jassdingk) 的 [#95](https://github.com/Fish-under-sea/FISH-Dock/pull/95)（解决 [#94](https://github.com/Fish-under-sea/FISH-Dock/issues/94)）并补充宿主侧收口加固：打通插件外部原生窗口从 `build()` 建窗、`show_without_activating()` 首显绘制、透明无边框消框到 `dock.reserve` / `release` 主岛占位避让的完整链路。**主岛的外观、布局与动画没有任何改动。**

只发布 Setup 完整安装包，与 2.2.1 起一致。

## 修复

- **`windows.open` 建窗不再在主线程死锁超时。** 原 `build()` 先通过 `on_main` 切到主线程，再在闭包内派生子线程调用 `WebviewWindowBuilder::build()` 并让主线程 `recv_timeout(6s)` 等待；但 Tauri v2 的 `build()` 内部本身会把建窗请求投递回主线程事件循环，主线程同时被 `recv_timeout` 阻塞，导致每次开窗必超时失败（`closed reason=failed`）。
  - 现改为在调用方后台线程直接调用 `WebviewWindowBuilder::build()`，由 Tauri 正常投递主线程建窗。
  - 宿主侧补充主线程防呆守卫（若在 `MAIN` 线程误调 `build()` 立即报错返回，杜绝永久卡死），并将阴影改为 `.shadow(!snapshot.transparent)`，关闭透明窗无边框阴影的同时保留非透明系统窗阴影。
- **隐藏建窗首次显示不再只出透明空壳。** 用 `.visible(false)` 创建的 Tauri 窗口，其底层 WebView2 controller 同样处于隐藏状态；之前仅靠 `SetWindowPos(SWP_SHOWWINDOW)` 只显示了 Win32 壳而未唤醒 controller 绘制。现改为先调 `win.show()` 触发 controller 可见并绘制内容，再调 `SetWindowPos(SWP_NOSIZE | SWP_NOMOVE | SWP_NOACTIVATE | SWP_SHOWWINDOW)`，最后仅在前台窗口不是自身时将焦点归还原前台窗口。
- **透明外部窗口消除 8 px 非客户区白框与默认白底。** `build()` 在 `snapshot.transparent` 分支显式设置 `win.set_background_color(Some(Color(0, 0, 0, 0)))` 并关闭 `.shadow(false)`（消除 `WM_NCCALCSIZE` 非客户区内嵌边距导致的 `outer > client` 外框），同时在 `strip_chrome()` 中先经 `GetAncestor(hwnd, GA_ROOT)` 确保作用于顶层宿主 HWND 后再剥除系统标题栏/边框样式。
- **补齐主岛占位方法转发。** `plugin_window::page()` 与 `plugin_window::backend()` 现已转发 `dock.dock.reserve` / `dock.dock.release` 与 `host.dock.reserve` / `host.dock.release` 至 `plugin_dock`（此前 `plugin_dock` 已有实现但窗口分发层漏放行，调用报「插件窗口方法无效」）。
- **修复零参释放与退订时的占位归属与主岛同步通知。**
  - `plugin_dock::release_op()` 无 `handle` 参数清空、`unsubscribe()` 退订及 `forget()` 停用清理现统一复用 `clear_plugin_reservations()`：带连字符的 `reserve-<plugin>-` 前缀避免误伤同前缀其他插件，同时记录自定义 `handle` 的所属插件并在实际清掉占位时主动推送 `island:reserve`，确保主岛立即收回避让高度；
  - 显式传入自定义 `handle` 时校验证明归属一致，防止跨插件覆盖或误删。

## 兼容性

- 与 2.3.4 / 2.3.5 外部窗口 Hook 契约完全兼容，不改动现有方法名、字段与事件结构。
- 之前依赖官方外部窗口 Hook 并受阻于建窗超时、首显空白、透明白框/白底或 `dock.reserve` 报「插件窗口方法无效」的插件无需改动即可恢复。
- 自定义 `handle` 占位现严格绑定提交它的插件 ID，其他插件不得同名覆盖或越权释放。
- 安装包仍为 Setup-only；自 2.2.0 起更新器只认 Setup 载荷及其摘要。

## 测试

- **Rust 单元测试**：`cargo test --locked --no-default-features --manifest-path app/src-tauri/Cargo.toml -- --test-threads=1` 本机实跑 **1030 项通过、0 项失败、24 项 `#[ignore]` 跳过**，包含新增的 `plugin_window::tests::dock_reserve_and_release_route_for_page_and_backend`（覆盖页面与后端桥 `dock.dock.reserve` / `release` 与 `host.dock.reserve` / `release` 转发）及 `plugin_dock::tests::release_op_clears_only_calling_plugins_reservations`（覆盖零参按归属清理与防跨插件误删）。
- **Node 页面与 SDK 测试**：`node --test --test-isolation=none "app/test/*.test.js" "app/test/*.test.mjs" "app/scripts/*.test.mjs" "sdk/node/*.test.mjs"` 本机实跑 **626 项用例中 620 项通过、0 项失败、6 项按约定跳过**（均标注需隔离机器的 `FISH_DOCK_ISOLATED_INSTALL_TESTS=1` 原生 NSIS 安装测试）。
- **文档与版本一致性**：`node tools/docs/check-readme.mjs` 核验所有文档、锚点、README 卡片资源与 `tauri.conf.json` 2.3.6 版本一致。

## 未测边界（如实记录，不代表通过）

- 真实图形桌面下外部 WebView2 透明窗口的首显绘制、像素边缘与前台焦点归还链路：本轮打包端未在真机桌面逐项录制交互回归；该链路在真实 Windows 11 x86_64（DPI 96 / scale 1）环境上的实机验证结论来自 [#95](https://github.com/Fish-under-sea/FISH-Dock/pull/95) 贡献者 `@jassdingk` 的原始测试报告（合入后新增的 `.shadow(!snapshot.transparent)`、主线程守卫与零参占位通知尚未在真机桌面逐一截图复验）。
- 默认标记为 `FISH_DOCK_ISOLATED_INSTALL_TESTS=1` 的 6 项 NSIS 原生安装器测试与 24 项 Rust `#[ignore]` 集成/环境用例未在本次发版前执行。
- 本版未做 Authenticode 签名，Windows SmartScreen 仍可能提示。

## 下载

| 安装包 | 适用场景 |
| --- | --- |
| `FISH-Dock-Setup-2.3.6.exe` | 首次安装、修复安装、旧版覆盖升级，含 Node.js |

2.2.1 起只提供 Setup，不再提供 Update 精简包。
