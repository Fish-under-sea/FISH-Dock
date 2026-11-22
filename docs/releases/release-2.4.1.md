# FISH Dock 2.4.1

2.4.1 修复邮件页双栏模式下顶部工具栏与徽章未随液态玻璃／壁纸模式透明化的问题（PR #98），并同步精简 README 更新记录与文档库 Agent 环境档案。

只发布 Setup 完整安装包，与 2.2.1 起一致。

## 修复与文档调整

- **邮件双栏液态玻璃与壁纸磨砂对齐（[#98](https://github.com/Fish-under-sea/FISH-Dock/pull/98)）：**
  - 为 `body.dock-glass #island-settings .pane-l .mail-tools` 补充半透明玻璃染色背景、`backdrop-filter` 模糊与底部分割线，解决吸顶工具栏使用不透明 `var(--g1)` 覆盖收件箱卡片玻璃材质与内描边的问题；
  - 将邮件页附件/状态标签（`.chip`）及会话题数徽章（`.letter .cnt`）纳入液态玻璃控件规则；
  - 将 `#island-settings .pane-l .mail-tools` 同步纳入深色与浅色选配背景图时的轻度半透明磨砂规则。
- **文档清理与环境档案同步：**
  - 精简中英文 `README` 更新记录栏，固定仅展示最近 3 个版本概要；
  - 按 `skill-audit` 口径清理 `AGENTS.md` 与 `docs/agents/README.md` 中的历史叙事与负向表述，并刷新 `docs/agents/local/fish.md` 本机实测工具链、凭据位置及与全局 `GEMINI.md` 的引用去重。

## 测试

- `node tools/docs/check-readme.mjs`：77 份文档与引用校验通过。
- `node app/scripts/check-hardcoded.mjs`：0 未清零（120/120 白名单）。
- `node app/scripts/check-contrast.mjs`：462 条文字声明，0 违规。
- 前端回归与 Rust 全量测试均在发版前重跑验证通过。

## 下载

| 安装包 | 适用场景 |
| --- | --- |
| `FISH-Dock-Setup-2.4.1.exe` | 首次安装、修复安装、旧版覆盖升级，含 Node.js |

2.2.1 起只提供 Setup，不再提供 Update 精简包。
