# FISH Dock 文档索引

产品介绍、下载与当前版本以 [中文 README](../README.md) / [English README](../README.en.md) 为准。这里记录开发与维护知识，不再放在 `app/` 源码树中。

## 按任务查找

| 任务 | 入口 |
| --- | --- |
| 开发与架构 | [开发说明](guides/DEVELOPMENT.md) |
| 安装、验包与构建 | [安装版构建](guides/installer-build.md) |
| 已安装版本的运行、迁移与排障 | [运维说明](guides/operations.md) |
| 配置与环境变量 | [配置键](guides/config-keys.md) |
| 插件原生窗口 | [宿主接口](guides/plugin-windows-2.2.0.md) |
| 国内网络来源 | [镜像实测矩阵](guides/domestic-mirror-matrix-2026-11-04.md) |
| Agent 开工与本机环境 | [Agent 文档](agents/README.md)、仓库根 [AGENTS.md](../AGENTS.md) |
| 样式和视觉 | [设计说明](design/style.md)、[当前场景预览](design/previews/README.md)、[预览工具](../tools/preview/README.md) |

## 目录边界

- `releases/`：已发布版本的历史说明，保留当时事实，不按当前版本批量改写。
- `plans/`：历史决策与审计证据；旧路径是当时上下文，不代表当前目录。
- `specs/`：规格；`design/`：设计说明；`design/prototypes/`：设计原型；`design/screenshots/`：历史设计实图；`design/previews/`：当前前端场景预览。
- `agents/local/`：按用户名加载的环境档案。仅记录路径和环境，不记录密钥正文。

## 仓库与工具

`app/` 只保留前端、Rust 宿主、构建脚本和测试。`integrations/` 是 Agent 与邮件集成，`examples/` 是可读示例，`pets/`、`art/`、`wallpapers/` 保留素材与授权说明，`relay/` 独立维护中转代码。

从仓库根运行：

```powershell
node tools/docs/check-readme.mjs
node tools/docs/render-readme.mjs
node tools/docs/render-sections.mjs
node tools/preview/publish.js build .work/preview
& ./tools/preview/render.ps1 -Out .work/preview-shots -Only scene-* -Modes dark,light
```

README 图由 HTML 模板渲染，不手改 PNG；预览使用真实前端和合成场景，不代表真机集成验收。命令参数、全部场景与对比工具见 [预览说明](../tools/preview/README.md)。

一次性构建产物、临时验收与交接材料留在忽略的 `.work/`，不进入 Git；长期知识应提炼进对应指南、规格或测试。
