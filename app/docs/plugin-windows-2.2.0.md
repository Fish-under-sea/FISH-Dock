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

`fileDrop` 的 `scope` 为 `dock` 或 `window`。`dock` 的 `handle` 是订阅句柄，`window` 的 `handle` 是窗口句柄。`phase` 为 `enter`、`leave`、`drop`，只有 `drop` 携带 `paths`。载荷**不含落点坐标**（issue #87 E1）。

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

### 7.1 锚点算的是主岛窗口，不是胶囊

`dock-left` 与 `dock-bottom` 的基准是**主岛窗口矩形**（`placement()`，`plugin_window.rs:585-620`），
不是页面上可见的胶囊。两者差别很大：

| 量 | 值 | 来源 |
| --- | --- | --- |
| 主岛窗口宽（非任务栏形态） | 整个工作区宽 | `island.rs:214` `size_for` 返回 `(area.w / sf, ISLAND.1)` |
| 主岛窗口高 | 132 逻辑像素 | `island.rs:31` `ISLAND = (760.0, 132.0)` |
| 主岛窗口位置 | 工作区左上角 | `island.rs:330-339` |
| 胶囊顶部内缩 | 8 逻辑像素 | `island.rs:51` `TOP = 8.0` |
| 胶囊高 | 36 | 页面 `#island-row` |

由此：

- `dock-left` 按 `dx - width - offsetX` 算出的原始 X 是负值，**随后被工作区约束夹回工作区左缘**（`placement()` 末尾的 `clamp`，`:615-618`）。实际效果是**贴在工作区最左边**，与胶囊位置无关。
- `dock-left` 的 `offsetX` 符号与直觉相反：正值继续往左（被夹住，看不出变化），**负值才把窗口往右推**。1920 宽工作区、窗口 160 宽时，`offsetX: -200` 得到 `x = 40`。
- `dock-bottom` 落在**主岛窗口底边**（默认 y=132）下方 `offsetY`，距胶囊底边（y≈44）还有约 88 逻辑像素；水平方向 `x = dx + offsetX`，`dx` 同样在工作区最左。

垂直方向两者都按 132 高的主岛窗口居中，不是按 36 高的胶囊居中：窗口高 80 时 y 起点为 26，而胶囊在 y 8–44。

所以「窗口右缘贴胶囊左缘」用现有 `anchor` 表达不出来。这不是 `offset` 调参问题：
基准点在工作区边缘，且算出的位置还会被夹回去（issue #87 B2、C1）。

### 7.2 `anchorChanged` 的触发条件

重排由主岛的 `Moved`、`Resized`、`ScaleFactorChanged` 三类窗口事件驱动
（`island.rs:691-692` → `plugin_window::dock_changed()`，`plugin_window.rs:1256-1267`），
外框真的变了才重排，变了才回 `anchorChanged`（`:645-657`）。

**胶囊变长/变短、展开/收起、唤醒滑出都不触发。** 原因是这些变化全在页面内部：
主岛窗口宽度恒为工作区宽（见 7.1），胶囊长度变化不改变窗口矩形。收起滑出同样是
页面内 CSS `transform`（`style.css:2991` `.isl-stowed.home-island #island-row`），
宿主窗口不移动。

这条是实现与直觉最不一致的地方，容易被误判成 bug，验收时按此口径核对
（issue #87 C3、E4）。

## 8. 文件拖入隔离

- **没有订阅就不捕获**：没有任何插件申请拖入时，宿主不启用拖入捕获，主岛点击与收起照常。
- 主 Dock 订阅是**独占**的：已被别的活动插件占用时明确报冲突（错误文案里带占用者 ID），
  不广播路径、不静默改绑。同一个插件重复订阅返回原句柄。
- 外部窗口只把拖入交给**那个窗口的所有者**，窗口关闭/隐藏/换版后命中区立刻撤掉。
- `enter`/`leave` 不含路径；只有真实 `drop` 才交付路径，且只送目标插件当前代次。
- 日志只记数量与目标，**不打印完整文件路径**。
- 每批最多 4096 个文件，并对 UTF-16 路径总量保守预留 JSON 转义与协议帧空间；超过限制整批拒绝，不截断后冒充完整拖入。原生读取在分配缓冲区前检查同一预算。
- 后端输入队列有界（32 帧），过载或通道关闭时不等待主线程，拖入交付返回失败并记录不含路径的日志。当前没有可靠重试或持久化；生命周期与 `enter`/`leave` 通知也可能因过载丢失，插件可用 `windows.list()` 重新核对窗口。进程退出后的通知属于尽力交付。

### 8.1 命中区包含插件窗口

主岛窗口是透明 WebView2，拖动时系统命中落到渲染面而没有 OLE drop target，
所以拖入由独立的 plain 窗口 `dropwin.rs` 接收（该文件头注释记录了 2026-11-06 的实测过程）。

命中区由两部分组成（`dropwin.rs:83-89` `live_zones`）：

1. 主 Dock 订阅者对应的胶囊区域（`DRAG_RECT`，`island.rs` poll 按 260 ms 按住判定写入）；
2. 每个已订阅窗口自己的矩形（`plugin_window::drop_rects()`，`plugin_window.rs:1048-1056`）。

落点归属按**真实原生 z 序**取最上层（`dropwin.rs:64-77`）：插件窗口盖在胶囊上时归那个窗口，
被其他应用遮住时不截获、也不转给别人。`dropwin.rs:407` 与 `:413-419` 的测试钉住这两条。

2.2.0 删除了旧版挂在 pet 小圆旁的 36 px 子岛命中区（`island.rs:1206-1207` 有注释），
不再有虚构矩形兜底：页面没报过胶囊尺寸时，主 Dock 命中区不存在（`closed_rect` 返回 `None`）。

## 9. 错误

- 后端窗口调用失败回 `{ code: "WINDOW_OPERATION_FAILED", message }`；`message` 过长或
  含控制字符时退回一句通用说明。
- 页面侧的业务错误仍按既有约定编码「<文案>（错误码 CODE）」，`FishDock` 解出
  `error.code`；参数不合格（如句柄为空、选项是未知字段）在本地就拒绝，不发请求。

## 10. 已知缺口（issue #87 逐条核对）

下表是 2.2.1 的实际能力边界，按 [issue #87](https://github.com/Fish-under-sea/FISH-Dock/issues/87) 的编号排列。
「结论」一列区分**实现缺失**（规范写了、代码没有）与**规范未提供**（需要新增）。

| 编号 | 结论 | 现状与依据 |
| --- | --- | --- |
| A1 SDK 缺 `onEvent` | 与实现不符 | `sdk/node/plugin.mjs:79`、`:91` 均有；2.2.0 起就有。旧行号对应 2.1.9 的 SDK |
| A2 强制 `events` 能力 | 与实现不符 | `:140` 的检查只作用于 `plugin.event`；窗口与拖入走 `plugin.windowEvent`（`:145-158`），按 `windows`/`file-drop` 放行 |
| A3 载荷缺 `scope`/`handle` | 与实现不符 | `plugin_window.rs:1074`/`:1082`/`:1089`/`:1121`/`:1139` 五个载荷都带 `scope` 与 `handle`，`enter`/`leave` 已实现 |
| A4 插件窗口不是 drop target | 与实现不符 | `dropwin.rs:83-89` 命中区含 `drop_rects()`；`:407`、`:413-419` 测试钉住层级与遮挡 |
| B1 更小窗口 / `fitContent` | 实现缺失 | `plugin_window.rs:34-37`：宽 ≥160、高 ≥80 |
| B2 锚点对齐语义 | 实现缺失 | 见 §7.1 |
| B3 drag-out | 规范未提供 | 无接口 |
| B4 透明区点击穿透 | 实现缺失 | `transparent` 只改窗口样式（`:532-533`），无逐像素命中 |
| B5 内容驱动高度 | 部分 | `update(handle,{height})` 可用，无 `fitContent` |
| C1 读主岛几何 | 规范未提供 | 无 `dock.rect()` |
| C2 订阅主岛几何/状态 | 规范未提供 | 事件表只有四个事件 |
| C3 `dock-left` 随主岛重排 | 机制在但不生效 | 见 §7.2 |
| C4 `width:'dock'` | 规范未提供 | 宽度只能固定值 |
| C5 主岛交互 → 子岛收缩 | 规范未提供 | 无主岛 hover/click 事件 |
| D1/D2 打开内部插件页 | 规范未提供 | 页面桥白名单见 `plugin_page.rs:80-110` |
| E1 落点坐标 | 规范未提供 | `drop` 载荷无 `position` |
| E2 失焦/点外部 | 规范未提供 | `window_event`（`:629-643`）不向上发焦点事件 |
| E3 完整状态枚举 | 规范未提供 | 依赖 C1/C2 |
| E4 动画期间胶囊矩形 | 规范未提供 | 见 §7.2 |
| E5 长条等宽 | 与 C4 同一项 | 合并处理 |
| E6 room 预留与 z-order | 部分 | 主岛 room 是页面→宿主（`island.rs:79`、`:1313-1321`），插件无法申报；无 `zOrder` |
| E7 跨窗口动画 | 规范未提供 | 无全屏动画层 |

A 组四条不是「实现缺失」，核对基准不同：A1/A2 引用的行号属于 2.1.9 的 SDK，
A3/A4 引用的 `island_panel.rs` 在本仓任何版本都不存在（`forward_drop_all` 也是 0 命中），
实际代码在 `plugin_window.rs` 与 `dropwin.rs`。

## 11. 未在本轮验证的部分

- 真实多显示器 / 混合 DPI 下的锚点漂移、透明浮窗的点击穿透外观；
- 托盘菜单项在任务栏/托盘两种形态下的实际渲染；
- 与第三方文件管理插件的真实端到端联调（本轮不升级官方插件）。
  这些只能在装有 2.2.0 的实机上观察，静态检查与单元测试不能替代。