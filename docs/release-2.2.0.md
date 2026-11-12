# FISH Dock 2.2.0

> 维护者已明确接受下列已知问题并批准快速发行。DS 独立审查原结论为 `needs_revision`，定点修改后未再次独立复审，不宣称全量质检通过。

**2.2.0 是最后一个同时提供 Setup 完整安装包和 Update 精简更新包的版本。2.2.0 之后仅发布 Setup 完整安装包。** 本次保留 Update，供旧宿主过渡到新的更新方式；2.2.0 宿主的后续应用内更新只下载并校验 Setup。

## 插件按需创建 Dock 外部原生窗口

- 第三方插件可通过浏览器或 Node SDK 的 `windows.open()` 请求独立原生窗口。窗口由 Dock 进程创建，拥有独立 HWND；页面与业务界面由插件提供。
- **没有插件主动请求时，不创建、不绘制第二窗口，不预建隐藏 WebView，不显示常驻小球。** 原来硬编码的文件暂存界面与固定插件调用已移除。
- 新增 `windows.open/update/close/list/onEvent` 与 `fileDrop.subscribe/unsubscribe/onEvent`。内部设置页与外部窗口分别承载，能力、包摘要、实例代次、票据、资源和句柄按所有者校验。
- 文件拖入由插件显式订阅。主 Dock 订阅独占，外部窗口只向自身插件交付路径；无订阅时不启用拖入捕获。超过数量或协议预算时整批拒绝，不发送截断路径。
- 用户关闭、Escape、托盘关闭及插件停用、卸载、升级、崩溃时回收窗口和订阅。关闭后恢复 Dock 不会复活该窗口。

开发入口：[插件仓](https://github.com/Fish-under-sea/FISH-DOCK-Plugins)、[开发指南](https://github.com/Fish-under-sea/FISH-DOCK-Plugins/blob/main/docs/developers/README.md)、[原生窗口 Hook](https://github.com/Fish-under-sea/FISH-DOCK-Plugins/blob/main/docs/developers/native-window-hook.md)、[SDK 完整协议示例](https://github.com/Fish-under-sea/FISH-Dock/tree/main/sdk/examples/window-file-drop)。文档与本版源码一同推送。

## 更新与安装

- 2.2.0 宿主只选择完整 Setup，移除 Update → Setup 资产回退以及 Update 包内下载 Setup 的兜底脚本。同一个 Setup 的 GitHub、Gitee 与已配置镜像重试仍保留，下载继续校验 SHA-256。
- 安装仍使用事务，失败回滚并保留用户资料。失败提示区分「未写入」「已回滚」「回滚未完成」；静默安装失败时显示可见错误，成功保持静默。
- Setup 继续要求系统 Evergreen WebView2，补充读取加载器的 ClientState/EBWebView 登记并校验实际运行时文件，覆盖 Clients 登记不完整的情况。缺依赖时提示官方入口并退出，不自动安装依赖。已证实检测缺口，尚不能将其断言为所有现场误判的唯一原因。
- Update 自身不检测或安装系统依赖。系统 Evergreen 应在升级前可用；本次 Update 不会失败后自动下载 Setup，失败时请核对日志并手动使用同版 Setup。

## 下载与迁移

| 安装包 | 适用场景 |
| --- | --- |
| `FISH-Dock-Setup-2.2.0.exe` | 首次安装、修复、基础资源缺失及 2.0.x 覆盖升级；包含 Node.js，使用系统 Evergreen WebView2 |
| `FISH-Dock-Update-2.2.0.exe` | 2.1.0 至 2.1.9 的过渡升级；固定以 2.1.0 安装归属清单生成差分 |

每个 EXE 附同名 `.sha256`；GitHub 另附 `SHA256SUMS.txt`。两包正式构建后均通过 7-Zip 完整性检查。

| 包 | 字节数 | SHA-256 |
| --- | ---: | --- |
| Setup | 32898576 | `f798405fbcb52dc78f450a38e6a9a106e4ccb9b8fa2edcddbf923b6009d7c2bc` |
| Update | 31241074 | `d051b454b73c9349f91b722622de93b5e31b002740da4d4c64e56fb897831e4c` |

停留在 2.2.0 之前、未经过本次过渡版本的旧宿主，遇到后续单包发行时请直接下载该版 Setup 覆盖升级。安装包当前未做 Authenticode 签名。

## 验证状态与已知限制

- 最近 Node 全量：583 通过、0 失败、6 跳过；跳过的原生安装用例未在用户电脑执行。
- 最终 Rust installed：908 通过、2 失败、24 忽略；default：907 通过、2 失败、24 忽略；agent-core 两模式各 13 通过。剩余为实时祖先进程断言和 Task Scheduler 根访问。DACL 夹具及长路径原子替换已通过最终全量。
- 历史一次 Windows 堆损坏未再复现，尚无根因。已建 Issue：[实时祖先进程断言 #80](https://github.com/Fish-under-sea/FISH-Dock/issues/80)、[Task Scheduler 根访问 #81](https://github.com/Fish-under-sea/FISH-Dock/issues/81)、[堆损坏追踪 #82](https://github.com/Fish-under-sea/FISH-Dock/issues/82)。维护者已接受延期处理。
- 真实 HWND、焦点、透明关闭、混合 DPI 和真实文件拖入，以及正式安装升级矩阵未完成隔离机器验收。协议与逻辑测试不能代替这些检查。
- 拖入交付采用 32 帧有界队列，过载明确失败；没有可靠重试或持久化，生命周期通知尽力交付，插件可用 `windows.list()` 复核状态。
- DS 提出的会话保留、显示快照重排、启动隐藏与拖入初始化、已提交清理误报、停用示例和文档已定点修改。最新 Node 局部 22/22 与 SDK 示例通过；Rust 窗口模块 16/17，托盘关闭重复发送 `closed` 事件，跟踪 [#83](https://github.com/Fish-under-sea/FISH-Dock/issues/83)。
- 修复未经过独立复审，原生矩阵及最新安装源码尚未在隔离机器验证，跟踪 [#84](https://github.com/Fish-under-sea/FISH-Dock/issues/84)。维护者已明确批准按已知风险发行，原失败和审查结论保留。
