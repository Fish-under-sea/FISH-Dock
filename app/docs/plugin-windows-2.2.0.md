# 插件外部原生窗口与定向拖入（2.2.0）

状态：2.2.0 实现草稿，尚未完成独立审查与发行。对应已批准规格 `app/docs/specs/2026-11-08-dock-2.2.0-design.md` §1–7。

## 1. 这是什么

Dock 允许插件**显式**申请一个处于主岛之外的原生窗口：Dock 进程创建独立的
Tauri/WebView2 顶层窗口（独立 HWND），插件提供里面的 HTML/CSS/JS，Dock 只负责：

- 承载与隔离（宿主最小容器页 + 沙箱 iframe，插件拿不到 Tauri 管理命令）；
- 受控通信（页面票据 / 后端会话，宿主侧统一做能力、摘要、代次、句柄、配额检查）；
- 生命周期回收（真正销毁 WebView，不留隐藏 renderer）与定向文件拖入。

三件**不是**本轮范围：宿主不提供任何业务 UI（旧的文件暂存子岛已删除）、不承载
任意原生程序、不放开任意网页导航（远端服务页仍走既有受控 gateway 规则）。

## 2. 清单声明

```json
{
  "capabilities": ["storage", "windows", "file-drop"],
  "pages": [{ "id": "panel", "title": "我的暂存", "entry": "panel.html" }]
}
```

| 能力 | 作用 |
| --- | --- |
| `windows` | 允许创建/控制本插件的外部窗口 |
| `file-drop` | 允许订阅文件拖入并接收路径 |

两种能力都独立于 `storage`，少一个就在入口被拒绝（SDK 本地拒绝 + 宿主再拒绝）。
窗口的 `page` 必须是清单 `pages[].id`；资源必须在本插件 `files` 里并通过摘要检查。

## 3. 浏览器 SDK

页面里用宿主注入的 `FishDock`（`sdk/browser/bridge.js`）。设置面板里的插件页与
外部窗口里的插件页是**同一套接口**。

```js
// 开窗（页面 ready 后才返回句柄；同一 key 重复调用复用同一个实例）
const handle = await FishDock.windows.open({
  key: 'panel',            // 本插件内的复用键：小写字母/数字/-/_，≤64
  page: 'panel',           // 已登记的 pages[].id
  title: '我的暂存',        // 可选，1–80 字
  width: 420, height: 260, // 逻辑像素；宽 160–2400、高 80–1600
  anchor: 'dock-bottom',   // dock-left | dock-bottom | screen
  offsetX: 0, offsetY: 8,  // 逻辑像素，±20000
  transparent: false,      // true：无原生标题栏的透明浮窗（Escape/托盘仍可关）
})
await FishDock.windows.update(handle, { height: 300, anchor: 'dock-left' })
const rows = await FishDock.windows.list()
await FishDock.windows.close(handle)   // 真正销毁；重复关闭幂等

const off = FishDock.windows.onEvent(event => { /* ready | closed | anchorChanged | fileDrop */ })
off()

// 文件拖入：scope 'dock'（主 Dock，独占）或 'window' + handle（本插件自己的窗口）
const sub = await FishDock.fileDrop.subscribe({ scope: 'dock' })
const offDrop = FishDock.fileDrop.onEvent(event => {
  if (event.phase === 'drop') console.log(event.paths)   // 只有真实 drop 才带路径
})
await FishDock.fileDrop.unsubscribe(sub)
```

| 页面方法 | 宿主协议 |
| --- | --- |
| `windows.open(options)` | `dock.windows.open` |
| `windows.update(handle, options)` | `dock.windows.update` |
| `windows.close(handle)` | `dock.windows.close` |
| `windows.list()` | `dock.windows.list` |
| `fileDrop.subscribe(options)` | `dock.fileDrop.subscribe` |
| `fileDrop.unsubscribe(handle)` | `dock.fileDrop.unsubscribe` |

## 4. Node SDK

后端（`sdk/node/plugin.mjs`）在 `activate`/命令参数里拿到 `context`，能力与协议方法一一对应：

```js
import { createPlugin } from './sdk/node/plugin.mjs' // 按插件内实际 SDK 路径调整

export default createPlugin({
  id: 'acme.notes',
  async activate(context) {
    await context.fileDrop.subscribe({ scope: 'dock' })
    context.windows.onEvent(event => { /* 与页面侧同形 */ })
    context.fileDrop.onEvent(async event => {
      if (event.phase !== 'drop') return
      const handle = await context.windows.open({ key: 'panel', page: 'panel' })
      await context.windows.update(handle, { height: 300 })
    })
  },
  commands: { /* … */ },
})
```

| 后端方法 | 宿主协议 |
| --- | --- |
| `context.windows.open(options)` | `host.windows.open` |
| `context.windows.update(handle, options)` | `host.windows.update` |
| `context.windows.close(handle)` | `host.windows.close` |
| `context.windows.list()` | `host.windows.list` |
| `context.fileDrop.subscribe(options)` | `host.fileDrop.subscribe` |
| `context.fileDrop.unsubscribe(handle)` | `host.fileDrop.unsubscribe` |

宿主把窗口事件作为**单向通知**发 `plugin.windowEvent`（不要求 `events` 能力，
按 `windows` / `file-drop` 分派）。

完整可运行例子：`sdk/examples/window-file-drop/`
（`node sdk/examples/window-file-drop/harness.mjs` 在不需要 Dock 的情况下跑完整协议）。

## 5. 事件与关闭原因

| 事件 | 载荷 | 说明 |
| --- | --- | --- |
| `ready` | `handle` | 页面已就绪、窗口开始显示（之前一直隐藏） |
| `closed` | `handle`、`reason` | `user`（关闭按钮/Escape/托盘）、`plugin`（插件 close）、`deactivated`、`uninstalled`、`upgraded`、`crashed`、`exit` |
| `anchorChanged` | `handle`、`x`/`y`/`width`/`height`（物理像素） | 位置/尺寸或 DPI 变化 |
| `fileDrop` | `handle`、`scope`、`phase`、`paths`（仅 `drop`） | `enter`/`leave` 不带路径 |

窗口**真正销毁**的出口：原生关闭按钮、宿主 Escape（含透明浮窗）、托盘「关闭插件窗口」、
插件自己的 `close`，以及停用/卸载/升级/崩溃/宿主退出。关闭后不会自动重建小球；
再次显示 Dock 也不会复活已关闭的窗口——只有插件再次显式 `open` 才会重开。

## 6. 权限与所有权

- 身份只从**可信来源**推导：页面侧来自页面票据（票据绑定承载标签），后端侧来自
  宿主拉起的那个实例会话与其代次。接口不接受插件自报 `owner`/`pluginId`。
- 句柄是不透明字符串，不带身份信息；跨插件句柄一律拒绝，重复关闭幂等。
- 记录包含插件 ID、包摘要与实例代次：旧代次不能创建或控制新实例的窗口；创建与
  加载之间被停用/换版的窗口在加载时（`attach`）当场关掉。
- 上限：每插件最多 4 个、全局最多 16 个窗口；页面票据最多 32 张。
- 创建失败、超时（默认 8 s 未 ready）或创建期间被停用：名额与票据当场释放，
  不留空白窗口，迟到的创建结果不会再出现。

## 7. 显示、锚点与焦点

- 页面 ready 之前窗口隐藏；显示用 `SWP_NOACTIVATE`，不抢当前主界面焦点，
  用户点进来时仍能正常获焦。
- 锚点：`dock-left` 贴在 Dock 左侧、`dock-bottom` 贴在 Dock 下方（跟随主岛移动、
  换屏与 DPI 变化重排）；`screen` 用 `offsetX/offsetY` 作为逻辑屏幕坐标。
- 所有位置都会被限制在当前显示器的工作区内，不通过扩大主岛来容纳窗口。
- Dock 全局隐藏或主岛全屏避让时，附着于 Dock 的窗口随之隐藏并释放拖入命中；
  恢复时只恢复那批本来还开着的窗口（用户自己关掉的不复活）。`screen` 窗口
  随 Dock 全局隐藏，但不跟随主岛普通展开/收起。

## 8. 文件拖入隔离

- **没有订阅就不捕获**：没有任何插件申请拖入时，宿主不启用拖入捕获，主岛点击与收起照常。
- 主 Dock 订阅是**独占**的：已被别的活动插件占用时明确报冲突（错误文案里带占用者 ID），
  不广播路径、不静默改绑。同一个插件重复订阅返回原句柄。
- 外部窗口只把拖入交给**那个窗口的所有者**，窗口关闭/隐藏/换版后命中区立刻撤掉。
- `enter`/`leave` 不含路径；只有真实 `drop` 才交付路径，且只送目标插件当前代次。
- 日志只记数量与目标，**不打印完整文件路径**。
- 每批最多 4096 个文件，并对 UTF-16 路径总量保守预留 JSON 转义与协议帧空间；超过限制整批拒绝，不截断后冒充完整拖入。原生读取在分配缓冲区前检查同一预算。
- 后端输入队列有界（32 帧），过载或通道关闭时不等待主线程，拖入交付返回失败并记录不含路径的日志。当前没有可靠重试或持久化；生命周期与 hover 通知也可能因过载丢失，插件可用 `windows.list()` 重新核对窗口。进程退出后的通知属于尽力交付。

## 9. 错误

- 后端窗口调用失败回 `{ code: "WINDOW_OPERATION_FAILED", message }`；`message` 过长或
  含控制字符时退回一句通用说明。
- 页面侧的业务错误仍按既有约定编码「<文案>（错误码 CODE）」，`FishDock` 解出
  `error.code`；参数不合格（如句柄为空、选项是未知字段）在本地就拒绝，不发请求。

## 10. 未在本轮验证的部分

- 真实多显示器 / 混合 DPI 下的锚点漂移、透明浮窗的点击穿透外观；
- 托盘菜单项在任务栏/托盘两种形态下的实际渲染；
- 与第三方文件管理插件的真实端到端联调（本轮不升级官方插件）。
  这些只能在装有 2.2.0 的实机上观察，静态检查与单元测试不能替代。