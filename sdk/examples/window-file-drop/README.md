# 合成示例：主 Dock 拖入 → 插件外部窗口

这是一个**第三方（合成）插件**的最小完整例子，不是官方插件，也不会被装进发行版。
它演示 2.2.0 的窗口 Hook 与定向拖入怎么用：

1. 后端在 `activate` 里显式订阅主 Dock 的拖入（`fileDrop.subscribe({ scope: 'dock' })`）；
2. 宿主只把**真实 drop** 的路径交给这个插件（enter/leave 不带路径）；
3. 插件收到路径后更新自己的业务状态，并按需 `windows.open()` 打开自己的外部窗口；
4. 页面侧用 `FishDock.windows.onEvent` / `FishDock.fileDrop.onEvent` 收事件、用
   `FishDock.windows.close()` 真正关闭窗口；
5. 用户关窗后宿主发 `closed`，插件不再持有句柄。

## 目录

| 文件 | 作用 |
| --- | --- |
| `plugin.json` | 插件清单：`capabilities: ["storage","windows","file-drop"]`、页面 `panel` |
| `backend.mjs` | 后端 SDK 用法（Node） |
| `panel.html` | 插件自己的界面（宿主只给窗口容器，不画业务 UI） |
| `harness.mjs` | 不需要 Dock 的可运行闭环：在进程内扮演宿主跑完整协议 |

## 跑一遍

```powershell
node sdk/examples/window-file-drop/harness.mjs
```

期望输出：

```
示例通过：握手 → 激活 → 订阅主 Dock 拖入 → 收到 2 份路径 → 开窗 → 用户关闭 → 句柄释放
```

`harness.mjs` 用的是 `sdk/node/plugin.mjs` 本体，不是替身：它按真实协议回答
`host.fileDrop.subscribe` / `host.windows.open` / `host.windows.update` /
`host.windows.close`，并投递 `plugin.windowEvent`。真机上这套协议由 Dock 宿主实现。

## 装进真正的 Dock（可选）

这是示例，不提供 `.fish-plugin` 发行包。要在真机上试：把 `plugin.json` 的
`files` 摘要重算（改过任何文件都要重算）、按 `docs/plugin-windows-2.2.0.md`
的说明打成插件包，再从「第三方插件」页安装。宿主会校验包摘要、能力与页面登记，
任何一项不符都会在开窗前就拒绝。

## 备注

- 示例里的日志只记数量，不打印完整路径（宿主同样如此）。
- `capabilities` 少写 `windows` 或 `file-drop`，对应 API 会在本地就拒绝，
  宿主侧也会再拒绝一次——两道门是故意的。