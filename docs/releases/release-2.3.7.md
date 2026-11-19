# FISH Dock 2.3.7

2.3.7 在 2.3.6 之后新增纯 **Antigravity 2.0 桌面端** 的灵动岛审批与选方案支持，同时修复打开灵动岛时后台触发 `agy.exe models` 闪出黑色控制台窗口的问题（[#96](https://github.com/Fish-under-sea/FISH-Dock/issues/96)）。**主岛的外观、布局与动画没有任何改动。**

只发布 Setup 完整安装包，与 2.2.1 起一致。

## 新增与修复

- **纯 Antigravity 2.0 桌面端支持岛上审批（仅「允许 / 拒绝」）：**
  - `connection.rs` 在 Antigravity `hooks.json` 中补齐 `PreToolUse`（`matcher: ""`，超时 300s）；旧版 4 事件配置会自动识别为需要更新（`stale`）。
  - `events_agy.rs` 结合 `~/.gemini/config/config.json` 与对应项目 `projects/<uuid>.json` 中的 `permissionPreset` / `autoExecutionPolicy` / 授权规则，仅在工作区外读写、危险命令、网页请求、MCP 调用或开启 Review 策略确认时上岛弹窗，常规安全读写与安全命令继续自动放行；在 Dock 点击「允许」返回 `auto_approve`、「拒绝」返回 `deny`、超时或关闭返回 `allow` 交回 Antigravity 原生弹窗。
- **支持 `ask_question` 岛上选方案：**
  - `PreToolUse` 拦截 Antigravity 的 `ask_question` 工具并映射为 `AskUserQuestion` 在灵动岛展示；用户选完后以 `{"decision":"deny","reason":"..."}` 将结果回传给模型继续当前轮次，跳过原生弹窗；若超时或关闭则返回 `allow` 交回原生弹窗。
- **修复纯 Antigravity 桌面端钩子断连：**
  - `main.rs` 的 `agy_hook` 移除 `post_and_go` 之前对旧数据目录 `quit-by-choice` 标记的早退检查，避免主机迁入统一数据根后已运行的 Dock 端口收不到 `agy` 会话上报。
- **修复打开灵动岛时后台拉起 `agy.exe models` 闪出黑窗（[#96](https://github.com/Fish-under-sea/FISH-Dock/issues/96)）：**
  - `mail::agent::agy_models` 新增 `should_probe_agy_models` 守卫，仅在设置页打开、已配置邮箱且当前邮箱代理为 `agy` 时才拉取模型列表；平时唤醒或打开灵动岛不再在后台调用 `agy.exe models`。

## 测试

- **JavaScript 测试**：`node --test test/*.test.js test/*.test.mjs` 本机实跑 **508 passed, 0 failed**。
- **Rust 单元测试**：`cargo test --manifest-path src-tauri/Cargo.toml -j 1` 本机实跑 **1035 passed, 0 failed, 24 ignored**（含 `agy` `PreToolUse` 与 `#96` 新增单测 12 项全通过）。

## 下载

| 安装包 | 适用场景 |
| --- | --- |
| `FISH-Dock-Setup-2.3.7.exe` | 首次安装、修复安装、旧版覆盖升级，含 Node.js |

2.2.1 起只提供 Setup，不再提供 Update 精简包。
