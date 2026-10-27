# 安装与更新性能优化验收

日期：2026-10-27。本文记录本地测试构建，公开版本及发布状态保持不变。

## 实现

- NSIS 将发行文件解压到所选安装目录内的唯一 `.fish-dock-stage-*` 暂存目录；独立引导 Node 在 NSIS 插件临时目录运行。暂存文件与目标安装处于同一文件系统，校验成功后优先移动，不再整套复制一遍。
- 大文件采用流式 SHA-256。目录祖先检查在一次事务内复用，文件自身仍检查类型和链接。
- 安装目录的 `install-state.json` 记录已验证文件的摘要、大小、设备和文件标识、修改时间、状态变化时间及创建时间。记录同时绑定安装目录和安装清单摘要。文件元数据变化、缓存损坏或缓存缺失时重新哈希。首次安装及实际更新载荷仍验证 SHA-256；跨卷复制后重新验证副本。
- 只有 WebView2 有变更或缺少历史校验记录时，安装器才递归授权。ACL 变化也会修改文件状态时间，因此外部授权或文件系统变化可能使部分缓存失效，下一次重新校验属于预期行为。
- 校验、准备和提交阶段均输出节流的计数与进度。失败沿用事务回滚；更新仅清理已确认归属且摘要匹配的旧文件，不清空用户资料。
- 完整离线包继续保留 Node、固定版 WebView2、Windhawk 引擎及完整编译器。新增精简更新包，仅分发相对基础清单变化的文件，并独立携带引导 Node。基础资源缺失或损坏时，在替换活跃文件前提示使用完整安装包。

## 构建

从 Wakuwaku 目录执行，先按既有构建流程生成 Release 主程序。完整包与精简包应使用不同输出目录。

```powershell
node scripts/installer.mjs --app <DSH-FISH.exe> --resources <resources> --version 1.1.0 --out <full/setup.exe> --makensis <makensis.exe>
node scripts/installer.mjs --app <DSH-FISH.exe> --resources <resources> --version 1.1.0 --out <update/update.exe> --makensis <makensis.exe> --baseline <previous-install-manifest.json>
```

精简更新安装器仍使用完整的目标文件归属清单，卸载表覆盖复用资源。更新包不是首次安装包：目标安装的资源必须满足目标清单。相同版本号的本地测试构建也通过摘要区分，不只比较版本字符串。

## 基准

使用同一台 Windows 机器与同一份真实发行资源，资源总计约 1.30 GiB、10,514 个文件。优化前后分别使用独立目录，采用同一基准脚本记录文件读写量。数据为本地缓存条件下的一轮测量，不是跨机器或下载速度承诺。

| 场景 | 优化前 | 优化后 | 优化后内容读取 |
| --- | ---: | ---: | ---: |
| 首次安装的校验与事务 | 56.237 秒 | 20.007 秒 | 约 1.41 GB |
| 同版重复校验与事务 | 11.396 秒 | 0.433 秒 | 约 6.13 MB 的记录与清单 |
| 只更新主程序 | 12.047 秒 | 0.439 秒 | 约 21.08 MB |

首次安装原先额外复制约 1.41 GB，优化后同卷复制量为 0；使用移动交接。重复场景复用 10,515 项，主程序更新场景只哈希变化的程序。**上述时间不包含 NSIS 解压、结束旧进程、运行时授权及网络下载，不代表点击安装到完成的总时间。**

## 弹窗

确认弹窗与设备验证码弹窗保持现有控件视觉，宽度最多 360 CSS px，高度最多视口的 70%；内容过长在弹窗内部滚动。移除重阴影及整视口暗色遮罩，保留模态焦点、Escape 与焦点回收。

真实 Chromium 使用生产弹窗 DOM、交互代码与样式验证深浅色和 320×480、520×600、1120×760 窗口，并保存截图。该证据验证 Web 页面布局与交互，不等同于原生 Windows 多显示器或缩放实机验收。

## 测试

```powershell
node --test --experimental-test-isolation=none scripts/install-update.test.mjs scripts/install-update-performance.test.mjs scripts/installer.test.mjs scripts/installer-upgrade.test.mjs
node --test --experimental-test-isolation=none test/dialog-browser.test.mjs test/settings-focus.test.js
```

回归覆盖同卷移动、跨卷回退和损坏副本、缓存命中、同大小并恢复 mtime 的损坏检测、精简包缺少基础资源的拦截、事务失败回滚、实际 NSIS → Node → Windows PowerShell 5.1 的中文空格路径首装及重复安装，以及弹窗浏览器交互。

本轮完整前端、SDK、浏览器与安装工具检查 357 项通过。未修改 Rust 源码；此前的两项 LHM 本机环境失败不在此统计中。

最终本地完整包为 326,004,452 字节，精简更新包为 30,842,530 字节，精简约 90.5%。两者归档完整性校验通过，精简包中的更新脚本与当前源码一致，PowerShell 脚本携带 UTF-8 BOM，主程序摘要与本轮 Release 编译产物一致。没有发布或替换日常安装。
