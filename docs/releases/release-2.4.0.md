# FISH Dock 2.4.0

2.4.0 为玻璃模式下的快捷开关带来动态 RGB 流光描边与星光特效，并对 Antigravity Hook 的模式识别、静默效率审批、人工审批覆盖范围与对话流同步体验进行了全面升级。

只发布 Setup 完整安装包，与 2.2.1 起一致。

## 新增与改进

- **玻璃态快捷开关视觉升级：** 修复玻璃模式且控件不透明度调至 0% 时，已启用的「提示音」「灵动岛」等快捷开关图标发黑看不清的问题；已启用态改为深邃星云透底，叠加 360° 旋转动态 RGB 流光描边、错落星芒闪烁特效与亮白发光图标，并适配 `prefers-reduced-motion` 静态降级。
- **Antigravity 极速模式识别：** 新增会话级 `~/.gemini/<product>/annotations/<conversationId>.pbtxt` 预设读取（优先级：会话 > 项目 > 全局），彻底解决在对话栏切至「极速（`AGENT_PERMISSION_PRESET_TURBO`）」后仍有审批弹上 Dock 的问题；点击「本会话极速放行」会立即对当前待审调用返回 `auto_approve`。
- **Antigravity 效率模式零提示音：** 将运行中的 Dock 服务端收口为 `PreToolUse` 唯一决策真相源，效率模式下自动放行的安全命令与工作区内读写，以及极速透传或 Dock 不可见退回，均不再触发 `waiting` 状态与提示音。
- **Antigravity 人工审批全覆盖与并行队列：** 补齐 `AGENT_PERMISSION_PRESET_REQUEST_REVIEW` 判定；关闭效率模式的人工审查下，工作区内 `write_file`/`edit_file`/`notebook_edit`、全部命令（含 `send_command_input`）、`call_mcp_tool`、`search_web`、`browser_*` 与越界目录/检索操作均送上 Dock；并将 `agy` 纳入并行队列且优先按 `toolCall.id` 匹配，避免并行调用互相挤出误放行。
- **Antigravity 桌面端 / CLI 识别与原生双向同步：**
  - 按 `transcriptPath` / `artifactDirectoryPath` 精准区分桌面端与 `antigravity-cli/`，桌面端左下角显示「去 Antigravity 处理」，CLI 才显示「去终端处理」；
  - 点击「去 Antigravity 处理」（或免打扰/超时退回）显式回传 `ask`（权限）或 `allow`（提问），立即唤出 Antigravity 原生交互卡片；
  - 在 Dock 上完成的提问选择、批准/拒绝与本会话极速放行，会在下一次 `PreInvocation` 通过官方 `metadata.user_markdown` 内联同步渲染进 Antigravity 对话流（`systemMessage: ""`，零额外模型 Token）。
- **文档边界收口：** 插件接口与液态玻璃 Hook 文档统一迁往 [FISH-DOCK-Plugins](https://github.com/Fish-under-sea/FISH-DOCK-Plugins)，主仓 README 与文档索引同步指向插件仓。

## 测试

- Rust 全量单元测试 **1061 passed / 0 failed**（含会话级极速预设、人工审批全工具覆盖、并行队列、`agySurface` 识别与 `PreInvocation` 同步注入新增用例）。
- 前端与真实 Chromium 回归测试、脚本测试、`check-hardcoded.mjs`（0 未清零）、`check-contrast.mjs`（0 违规）、`check-readme.mjs` 全部通过。

## 下载

| 安装包 | 适用场景 |
| --- | --- |
| `FISH-Dock-Setup-2.4.0.exe` | 首次安装、修复安装、旧版覆盖升级，含 Node.js |

2.2.1 起只提供 Setup，不再提供 Update 精简包。
