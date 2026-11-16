# FISH Dock 2.3.5

2.3.5 是 2.3.4 之后的修复版本，合并 [#93](https://github.com/Fish-under-sea/FISH-Dock/pull/93)：修复 Dock 内部框选或长按拖动后鼠标穿透锁死、无法点击的问题，把插件页桥单次调用上限提升到 2 MiB 并在超限/拒收时立即回传结构化错误，同时处理代码体检发现的提权目录、邮件净化、Tauri IPC 来源校验与若干交互 / 可访问性问题。**主岛的外观、布局与动画没有任何改动。**

只发布 Setup 完整安装包，与 2.2.1 起一致。

## 修复

- **Dock 内部框选或长按拖动后锁死无法点击。** 之前在 Dock 内按下鼠标（文本框选、滑块拖动、长按拖拽）超过 260ms，会被误判成「从外部拖文件进岛」，窗口切到 `WS_EX_TRANSPARENT` 穿透；松手后状态机没有回到可点击，用户必须先点一下桌面才能恢复。
  - 新增「按下起点在已捕获鼠标的 Dock 内部」状态：整个按住期间都按内部操作处理，不再触发外部拖放或穿透；松手即清零。
  - 外部拖放激活时不再绕过 ClickThrough 状态机直接传 `Some(true)`，松手后立即恢复 `Some(false)`，不再停留在 `Mode::Catch`。
  - 页面侧拦截非 `[draggable="true"]` 元素的原生 `dragstart`，避免文本框选触发 WebView2 原生 OLE 拖放吞掉 `pointerup` / `mouseup`。
- **插件页桥超限或拒收时静默丢弃，页面白等 20 秒超时。** 现在单次请求超过 2 MiB、方法名非法/保留、并发超过 16 条或重复请求 ID，都会立即回传带错误码的 `fish:result`（`PAYLOAD_TOO_LARGE` / `INVALID_METHOD` / `INVALID_REQUEST` / `TOO_MANY_REQUESTS`），不再让插件页面干等。
- **邮件 HTML 净化的图片协议与伪自闭合绕过。** 远程图片判定改为大小写无关、去首尾空白，并拦截协议相对 `//`；仅 `cid:` 图片保留 `src`，`file://` 等危险协议一律剥掉（防 Windows SMB/NTLM 泄露）。`<script/>`、`<style/>` 这类非空元素的伪自闭合写法不再绕过内容跳过；`safe_href` 同时拦掉 `//` 协议相对链接。

## 安全

- **提权目录与系统命令路径加固。** PawnIO 下载与解压从 `%TEMP%` 移到管理员独占 ACL 的 `%ProgramFiles%\FISH Dock\sensors\staging`，用完即清，消除预置目录的 TOCTOU 本地提权面；`tar` 与 `schtasks` 改为解析 `%SystemRoot%\System32\*.exe` 绝对路径，不再依赖 PATH。
- **Tauri IPC 来源校验补齐。** `extension_cancel` 与全部 `windhawk_*` 命令补上 `plugin_api::trusted` 校验，只允许本机设置窗口调用。

## 交互与可访问性

- 桌面画板移动磁贴不再整板重建 `<canvas class="wpaper">`，只更新磁贴位置；移除节点时清理 `loadingTimer`。
- 删除确认框补齐 Tab 焦点陷阱，Escape 仍直接取消。
- 邮件菜单的 `data-account` / `data-folder` / `data-uid` 属性补上 `esc()` 转义。
- 扩展卸载进度卡去掉无效的取消按钮（目标已移除）；下载取消按钮防连点并显示「正在取消…」；复制成功给出即时反馈。
- 胶囊面板支持 Escape 关闭；数值刷新时原位更新 `.cp-val`，保持键盘焦点与悬停态。

## 兼容性

- 插件页桥单次调用上限：页面侧 512 KiB → 2 MiB，宿主 JSON 行协议与 Node SDK 1 MiB → 2 MiB。旧插件无需改动，1.2–2 MiB 的请求现在可以通过。
- 超限、非法方法与并发超限从「静默丢弃 + 页面超时」改为「立即返回错误」；依赖 20 秒超时兜底的调用方会更快拿到明确错误。
- 没有删除或修改既有接口语义；主岛外观、布局与动画不变。
- 安装包仍为 Setup-only；更新器自 2.2.0 起只认 Setup 载荷及其摘要。

## 测试

- CI（GitHub Actions）：Rust 单测、Node 页面与 SDK 测试、README 引用与版本一致性三 job 全部通过；合并后的 main 首跑因既有 `plugin_install` 并发护栏在 runner 上超过 30 秒哨兵失败，重跑通过。
- 本轮新增 / 修改的测试：Dock 内部选中/长按拖动与外部拖放后的 ClickThrough 恢复、1.3 MiB 请求放行与超过 2 MiB 拒绝、超限/非法方法即时回传、邮件净化的大小写/协议相对/`file://`/`<script/>` 绕过、系统命令绝对路径。
- 说明：上述 `plugin_install::tests::concurrent_install_uninstall_never_deadlocks` 与本次改动无关（本轮未修改 `plugin_install.rs`），本地复跑 12.08 秒通过；两次 CI 重跑均通过，属慢 runner 上的 30 秒哨兵误报，不是产品缺陷。

## 未测边界（如实记录，不代表通过）

- 真实鼠标在 Dock 内框选、长按拖动，以及松手后立即点击的完整链路，本轮只有单元测试证据，没有真机操作录像或自动化输入证据。
- 真实邮件客户端样本中的远程图片、`file://` 图片、协议相对链接，未做逐封目视核对。
- 提权目录 ACL、`System32\tar.exe` / `schtasks.exe` 在真实安装环境与任务计划服务下的行为：沙箱无法验证管理员 ACL 与任务计划，需要在真机复核。
- 本版未做 Authenticode 签名，SmartScreen 仍可能提示。

## 下载

| 安装包 | 适用场景 |
| --- | --- |
| `FISH-Dock-Setup-2.3.5.exe` | 首次安装、修复安装、旧版覆盖升级，含 Node.js |

2.2.1 起只提供 Setup，不再提供 Update 精简包。