# FISH Dock 2.2.2

2.2.2 补齐外部原生窗口 Hook 的能力（[#87](https://github.com/Fish-under-sea/FISH-Dock/issues/87)）。**主岛的外观、布局与动画没有任何改动**：这一版动的是插件自己的那个外部窗口，以及宿主只读地暴露主岛几何。

只发布 Setup 完整安装包，与 2.2.1 一致。

## 新增

### 主岛几何与状态（C1/C2/C3/E3）

```js
const rect = await FishDock.dock.rect()
// { window: {x,y,width,height},          // 主岛窗口矩形（物理像素）
//   capsule: {x,y,width,height} | null,  // 可见胶囊矩形；尺寸未知时 null
//   state: { view, stowed, hidden, fullscreen },
//   scale: 1.5 }

const off = FishDock.dock.onEvent(event => {
  if (event.type === 'geometry') console.log(event.rect)
  if (event.type === 'pointer') console.log(event.action) // enter | leave
})
```

- **两个矩形分开给**：主岛窗口铺满工作区宽（屏幕多宽它就多宽），可见胶囊只有 240/300/380。只给一个都不够用，所以两个都给。
- `state.view` 取 `compact` / `expanded` / `settings` / `ask`，另有 `stowed`（滑出）、`hidden`（全局隐藏）、`fullscreen`（全屏避让）。
- 需要新能力 `dock`（与 `windows` 分开授权，只要几何的插件不必声明开窗能力）。
- **C3**：锚定窗口现在跟随胶囊尺寸变化重排。之前重排挂在主岛窗口的 `Moved`/`Resized` 上，而那个矩形在胶囊变长变短时并不改变，所以从不触发。

### 宽度跟随主岛（C4）

```js
await FishDock.windows.open({ key:'bar', page:'main', width:'dock', maxWidth:600 })
```

- `width:'dock'` 跟随胶囊当前宽度；`maxWidth` 是可增长到的上限。
- `maxWidth` 小于胶囊宽度时按胶囊算——语义是「可增长到」，不是「缩到比胶囊还窄」。
- 不写 `maxWidth` 就是恒定等宽。

### 锚点对齐（B2）

```js
await FishDock.windows.open({ key:'ball', page:'main', width:36, height:36,
  anchor:'dock-left', anchorBase:'capsule', align:'right-center' })
```

- `align` 指定窗口的哪条边/角贴住锚点，八种取值：`right-top`/`right-center`/`right-bottom`/`left-top`/`left-center`/`left-bottom`/`top-center`/`bottom-center`。
- `anchorBase` 指定锚点相对主岛**窗口**还是可见**胶囊**。
- **两者缺省都保持 2.2.1 的行为**：不写这两项，位置与上一版逐像素一致。

### 更小窗口（B1）

窗口下限从 160×80 降到 **32×32**，36 px 的悬浮球现在可以直接做成窗口。

### 窗口层级（E6）

```js
await FishDock.windows.open({ key:'bar', page:'main', zOrder:'above-dock' })
```

`above-dock` 让插件窗口置顶并排在主岛之上；缺省 `normal` 是普通顶层窗口。

### 拖入落点坐标（E1）

`drop` 事件的载荷多了 `position`：

```js
{ event:'fileDrop', scope, handle, phase:'drop', paths,
  position: { screen: {x,y}, client: {x,y} } }   // client 相对窗口；主 Dock 订阅相对胶囊
```

### 窗口焦点事件（E2）

`windows.onEvent` 新增 `focus` / `blur`。注意这是**本窗口**的焦点变化，不是「点了窗口外面」——独立原生窗口收不到窗口外的指针事件，需要「点外面收起」时只能用 `blur` 近似。

## 按需启用：不影响没用插件的用户

新增的页面上报（视图形态、指针）**默认全关**。宿主只有在真的有插件订阅 `dock.onEvent` 时才通知主岛页面开始上报，回到零订阅立即停止。

因此：

| 情况 | 新增代码是否执行 | 外观 | 性能 |
| --- | --- | --- | --- |
| 不安装任何插件 | 否 | 不变 | 不变 |
| 安装但未启用 | 否 | 不变 | 不变 |
| 启用但没订阅 `dock.onEvent` | 否 | 不变 | 不变 |
| 启用且订阅了 | 是 | 仅插件窗口变化 | 仅在订阅期间 |

## 兼容性

- 新增字段全部可选，缺省行为与 2.2.1 一致。
- 已有的 `anchor` 三值、尺寸范围、事件载荷字段都没有改语义，只增不删。
- 旧插件（含 `jassdingk.filemgr` 1.0.13）行为不变。

## 本次未实现

以下几项仍**没有**，已记录在 `docs/developers/native-window-hook.md` 的「已知缺口」里：

- **B3 drag-out**：把窗口内文件拖回资源管理器。需要 OLE `IDataObject`/`IDropSource` 源侧实现，工作量最大。
- **B4 透明区点击穿透**：`hitTest`/`hitRects` 未做，透明像素仍拦截鼠标。
- **D2 打开内部插件页**：`openPage(pageId)` 未做。
- **E4 动画期间逐帧胶囊矩形**：滑出是页面内 CSS transform，宿主在 `stowed` 切换时推送一次状态，不做逐帧跟随。
- **E7 跨窗口动画**：不支持，建议改为窗口内动画。

## 测试

- `cargo test`：924 passed / 1 failed / 24 ignored。
- 唯一失败是 `lhm_setup::tests::a_folder_made_with_her_dacl_is_closed_to_the_user`，属于沙箱环境限制（DACL 相关，本轮未改动该模块），不是产品缺陷。
- 新增 7 条验收单测，覆盖：32×32 下限、`width:'dock'` 与 `maxWidth` 语义、`align` 八种组合、缺省位置与 2.2.1 一致、胶囊基准与回落、快照两矩形分离。

## 下载

| 安装包 | 适用场景 |
| --- | --- |
| `FISH-Dock-Setup-2.2.2.exe` | 首次安装、修复安装、旧版覆盖升级，含 Node.js |

2.2.1 起只提供 Setup，不再提供 Update 精简包；更新器探针只认 `FISH-Dock-Update-<ver>.exe.sha256`，所以 2.2.2 不走应用内更新通道。