# 仓库工具

所有命令从仓库根运行。产品构建脚本继续放在 `app/scripts/`，这里不混放发布载荷、下载缓存或会话交接材料。

| 目录 | 职责 | 入口 |
| --- | --- | --- |
| `docs/` | README 横幅、分节图与链接检查 | `node tools/docs/check-readme.mjs`；`node tools/docs/render-readme.mjs`；`node tools/docs/render-sections.mjs` |
| `preview/` | 真实前端 + Tauri 替身的场景预览、截图与历史版本比较 | [完整说明](preview/README.md) |
| `misc/` | 专项辅助：带鉴权校验的 Agent API 客户端、错误文案截图 | [API 客户端](misc/agent-api-client.mjs)、[错误文案渲染](misc/render-error-copy.mjs) |

## 文档图

README 渲染器从 `app/src-tauri/tauri.conf.json` 读取版本，默认更新同名资产；`README_ASSET_SUFFIX` 可区分生成批次，引用必须与同批 `assets/readme/metadata.json` 一致。更改脚本位置后，不得只迁移渲染器而遗留其根目录解析。

渲染器使用本机 Edge；可用 `EDGE_BINARY` 指定可执行文件。沙箱阻止浏览器或真实子进程时，先核实错误，再为原命令申请单次授权，不删除测试覆盖。

## 预览与回归

```powershell
node tools/preview/publish.js build .work/preview
& ./tools/preview/render.ps1 -Out .work/preview-shots -Only scene-* -Modes dark,light
node --test --test-isolation=none app/test/preview.test.js app/test/agent-api.test.mjs
```

预览场景使用合成数据，不代表真实邮件、Windows 原生窗口或线上模型已完成集成验收。截图与临时构建留在忽略的 `.work/`；选入文档的图应记录来源与验收范围。
