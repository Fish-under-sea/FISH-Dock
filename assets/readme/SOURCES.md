# README 素材来源

- 排版参考：[Fish-under-sea/dsh-wallpaper-engine](https://github.com/Fish-under-sea/dsh-wallpaper-engine)，复古窗口、散热格栅、像素按钮和状态块。引用源码遵循其 MIT 许可，Copyright 2026 elysia395，原许可完整保留于 `LICENSE-reference.txt`。本仓库重新编排为 FISH Dock 双语内容。
- 像素字体：Jersey 20，Copyright 2023 The Soft Type Project Authors，SIL OFL 1.1；见 `OFL-Jersey20.txt`。中文使用系统 Microsoft YaHei 字体，仅发布排版截图，不复制系统字体。
- 主图：直接引用 `../../Wakuwaku/src-tauri/icons/dsh-fish.png`，即用户提供的蓝发角色抱海豚原图标。未重画、未更换应用图标，原始 SHA-256 为 `5688F69DE350B8C75E0832E7B5928C680A1B7FAA14ECD0259306AFBD5C4178B9`。
- 挂件：用户明确指定的「求佛_transparent.png」，原样复制为 `mascot.png`，SHA-256 为 `8E2BDD1C5B5DDB851DB88B8ED29F9660F0FFE18F73266A89BA8CC03F9360952E`。包含用户原图中的绳子和椅子，没有替换为参考仓库中的角色。
- 角色、挂件、图标并非由本项目创作，不因排版源码的 MIT 许可自动获得素材再分发授权；相关权利仍属于各自作者。用户要求的展示使用不构成对第三方权利的额外保证。

## 重新渲染

在仓库根目录执行 `node tools/render-readme.mjs`。需要 Microsoft Edge；可通过 `EDGE_BINARY` 指定安装路径。脚本只在隔离浏览器中加载本地 HTML、PNG 和字体，检查图片、字体加载和标题边界，再生成 PNG 与 `metadata.json`；不调用生成式图片服务，不发送用户素材。

GitHub README 不执行自定义 CSS，深浅主题使用 `<picture>` 选择本地 PNG。所有正文、安装步骤、风险说明和上游版权仍保留为可检索的 Markdown。
