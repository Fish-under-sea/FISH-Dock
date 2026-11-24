# FISH Dock 2.4.2

2.4.2 修复液态玻璃模式下插件设置页 `iframe` 因 `color-scheme` 失配出现 `#121212` 黑底与边角裁切的问题（[#99](https://github.com/Fish-under-sea/FISH-Dock/issues/99)），并加固进程退出、应用内更新交接与 Windows 系统关机期间的 WebView2 退出事件抑制及崩溃报告记录逻辑（[#100](https://github.com/Fish-under-sea/FISH-Dock/issues/100)）。

只发布 Setup 完整安装包，与 2.2.1 起一致。

## 修复内容

- **液态玻璃插件页 `iframe` 透明穿透与裁角修复（[#99](https://github.com/Fish-under-sea/FISH-Dock/issues/99)）：**
  - 为宿主根文档（`index.html`、`style.css`）、设置层（`settings.css`）及插件页主／服务 `iframe`（`extensions.css`、`extension-page.js`、`island.js`、`settings.js`）按宿主模式同步声明 `color-scheme`：`dark` 与 `glass` 模式固定为 `dark`，`light` 模式固定为 `light`，不随系统 `prefers-color-scheme` 漂移，消除 Chromium / WebView2 因父子 `color-scheme` 失配强行铺设不透明 `#121212` 黑底的问题；
  - 针对 `.models .s-body:has(iframe)` 解除 `14px` 的 `clip-path` 裁切与圆角裁剪，避免插件页顶部工具栏边角被裁切，同时保留普通设置页与插件离线卡原有的 `inset(0 round 14px)` 裁切效果；
  - 新增真实无头 Chrome（CDP）像素回归测试（`app/test/extension-color-scheme.test.mjs`）与 `extension-page.test.js` 单测，覆盖系统浅／深主题与宿主 `glass` / `dark` / `light` 及 `dock-glass + tb-light` 混合态透明穿透、裁角与顶层透明窗口断言。
- **退出、更新与系统关机期 WebView2 事件抑制及崩溃记录加固（[#100](https://github.com/Fish-under-sea/FISH-Dock/issues/100)）：**
  - 暴露进程退出状态 `is_quitting()`（含 `RunEvent::Exit` 入口置位）、应用内更新重启交接状态 `updates::is_restarting()`（仅 `stage == Some("restart")`）以及 Windows 会话关机检测 `system_shutting_down()`（`GetSystemMetrics(SM_SHUTTINGDOWN)`）；
  - 在 `crash::webview_failed` 收到 WebView2 异常退出事件时，若处于退出、更新重启交接或系统关机阶段，统一抑制崩溃报告写入、自愈重启与重载并记录忽略原因，避免与安装器停进程或系统关机产生竞态；正常运行时仍保持 `0x40010004` 不报崩溃、真实崩溃（如 `0xC0000005`）照常上报与自愈恢复；
  - 修复 `webview_failed` 的 10 秒去重窗口（`LAST`）先前被非崩溃事件刷新时间戳、导致紧随其后的真实崩溃被误吞的问题；
  - 修复 `crash::issue_body` 仅解析整数 `code` 导致字符串格式退出码（如 `browser-exited/unexpected/0xC0000005`）未写入预填 Issue 正文的问题，并补充 4 条 Rust 回归单测。

## 测试

- `node tools/docs/check-readme.mjs`：79 份文档、本地引用与锚点校验通过（`version: 2.4.2`）。
- `node app/scripts/check-hardcoded.mjs`：0 未清零（120/120 白名单）。
- `node app/scripts/check-contrast.mjs`：462 条文字声明，0 违规。
- 前端与插件集成回归（`app/test/` 59 个测试文件，`node --test --test-isolation=none`）：512 passed，0 failed，0 skipped（含无头 Chrome CDP 像素透明穿透回归 `extension-color-scheme.test.mjs`）。
- 脚本与 SDK 回归（`app/scripts/*.test.mjs` 与 `sdk/node/plugin.test.mjs`）：112 passed，0 failed，6 skipped（跳过项为隔离机原生 NSIS 测试）。
- Rust 全量单元测试（`cargo test --locked --no-default-features -- --test-threads=1`）：1065 passed，0 failed，24 ignored（安装版特性 `--features installed crash::tests` 21 passed，0 failed）。

## 下载

| 安装包 | 适用场景 |
| --- | --- |
| `FISH-Dock-Setup-2.4.2.exe` | 首次安装、修复安装、旧版覆盖升级，含 Node.js |

2.2.1 起只提供 Setup，不再提供 Update 精简包。
