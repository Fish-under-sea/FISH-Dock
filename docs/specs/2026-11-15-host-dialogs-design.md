# 宿主能力：插件选择文件夹（`dialogs.chooseFolder`，2.3.0）

> **2.3.4 状态：已实现**（2.3.0 计划并入 2.3.4）。规格按此落地，唯一差异：DLG-2 要求 COM 失败 → `DIALOG_OPERATION_FAILED`，验证支补 `paper::pick_reason` 把「取消（None）」与「COM 失败（Err）」分开（见 evidence/verify-a）。标题版本号以 2.3.4 为准。
>
> 状态：设计待评审，尚未实现。
> 需求来源：插件仓 `FISH-DOCK-Plugins` 的交接文档 `docs/handoff/2026-11-15-magpie-web-folders.md`（P1）。该文件在插件仓，不入本仓；宿主相关的契约已抄全，不依赖读者去翻外部目录。
> 该交接里的 P0（把 magpie 网页模式拒绝的「打开文件夹」接力到资源管理器）完全在插件侧，不需要宿主改动；只有 P1「选文件夹」需要一个宿主原生能力。
> 本文只定宿主侧契约、实现与验收，不改插件仓代码。

## 一、目标与非目标

**目标**：插件后端（Node 进程）能在用户面前弹出 Windows 的文件夹选择框，拿到用户选中的绝对路径；取消时拿到空值，不报错。

**非目标**：

- 不给页面（`sdk/browser`）开放对话框；页面侧需要另一套模态与权限设计，目前没有需求。
- 不做「选择文件」「多选」「按路径打开文件夹」；这些会扩大权限面。
- 不给插件窗口当 owner（`parent` 只接受 `'main'`）；以后有需求再加句柄。
- 不追求 macOS / Linux 的同等能力；宿主是 Windows 优先，其他平台明确报「不支持」。

## 二、为什么需要它

Dock 用 `magpie web --no-open` 起 magpie，管理页永远走网页宿主。网页宿主按设计拒绝「打开文件夹 / 选文件夹」，插件侧的 P0 已经能让「打开文件夹」走资源管理器；「选文件夹」没有路径可接力，必须由宿主提供原生选择框。插件侧在宿主能力缺席时会退回「spawn 一个 STA 的 PowerShell `FolderBrowserDialog`」，但对话框没有 owner、焦点可能跑到窗口后面，只能算兜底。

## 三、对外契约

### 3.1 清单能力

```json
{
  "capabilities": ["storage", "dialogs"]
}
```

- 新能力名 `dialogs`，与 `storage`、`windows`、`file-drop`、`dock` 平级，**不捆绑**。
- 清单校验的允许列表在 [`plugin_manifest.rs:110`](../../app/src-tauri/src/plugin_manifest.rs#L110)，加 `dialogs`；能力总数上限不变（16）。
- 没声明时：SDK 本地拒绝（`插件未声明 dialogs 能力`），宿主再拒绝一次。
- 要用新能力的插件必须发新版本、在清单里加 `dialogs`；旧版插件不会自动获得，也不需要迁移。

### 3.2 Node SDK

```js
// activate(context) 里
const { dir } = await context.dialogs.chooseFolder({ title: '选择备份文件夹' })
if (dir) await context.storage.write('config', 'backup-dir.json', JSON.stringify({ dir }))
```

| 项 | 值 |
| --- | --- |
| 方法 | `context.dialogs.chooseFolder(options?)` |
| 参数 | `{ title?: string, parent?: 'main' }`；未知字段、`title` 非字符串 / 空串 / 超过 80 字 / 含控制字符、`parent` 不是 `'main'`，一律本地拒绝，不发请求 |
| 返回 | `Promise<{ dir: string \| null }>`；用户取消时 `dir: null`，不抛错 |
| 路径 | 原样返回 Windows 文件系统路径；不折叠 `~`、不做大小写或分隔符规范化 |
| 超时 | **不设 SDK 超时**（见 §4.3）；会话关闭时拒绝 |
| 错误 | 能力缺失、`DIALOG_BUSY`、`DIALOG_OPERATION_FAILED`、`DIALOG_UNSUPPORTED` |

页面 SDK（`FishDock.dialogs`）**不提供**同名方法；页面里只能通过插件后端的命令间接使用。

### 3.3 宿主协议

| 项 | 值 |
| --- | --- |
| 方法 | `host.dialogs.chooseFolder` |
| 参数 | 同上，`parent` 缺省为 `'main'` |
| 成功 | `{"dir":"C:\\Users\\…\\Backups"}` 或 `{"dir":null}` |
| 失败 | `{"error":{"code":"DIALOG_OPERATION_FAILED","message":"…"}}`；忙时 `DIALOG_BUSY`；非 Windows `DIALOG_UNSUPPORTED` |
| 身份 | 与 `host.windows.*` 相同：后端会话代次 + 插件清单能力；不接受插件自报 owner |

## 四、宿主实现

### 4.1 先修路由：`host.dock.*` 现在根本到不了窗口模块

[`plugin_process.rs:136`](../../app/src-tauri/src/plugin_process.rs#L136) 只用 `host.windows.` / `host.fileDrop.` 判断是否走 `plugin_window::backend`，`host.dock.*` 与将来的 `host.dialogs.*` 都会落进 `storage_call`，回 `HOST_OPERATION_FAILED`（宿主操作失败，请检查存储类别与声明能力）。这正是 #88 里 P3 的症状。

把判断抽成一个可单测的纯函数，而不是继续加 `starts_with`：

```rust
enum HostCall { Window, Dialog, Storage }

fn host_call(method: &str) -> HostCall {
    if method.starts_with("host.windows.") || method.starts_with("host.fileDrop.") || method.starts_with("host.dock.") {
        HostCall::Window
    } else if method.starts_with("host.dialogs.") {
        HostCall::Dialog
    } else {
        HostCall::Storage
    }
}
```

回调线程按 `HostCall` 分派；错误映射也按域走（窗口错误用 `plugin_window::failure`，对话框错误用 `plugin_dialog::failure`，存储错误保持原文案）。这一条既修 #88 P3，也是 `dialogs` 能落地的前提。

### 4.2 对话框

新增 `app/src-tauri/src/plugin_dialog.rs`：

```
validate(context, plugin, generation, params) -> Result<Title, String>  // 同步：代次、能力、参数、互斥
choose(title, reply)                                                    // 工作线程：弹框，写回响应
```

1. **校验**：代次新鲜；插件已安装；`require_capability(row, "dialogs", …)`；参数按 §3.2 校验。校验失败立即回错误，不弹框。
2. **互斥**：全局同一时间只允许一个对话框（`AtomicBool` + guard）。第二个请求回 `DIALOG_BUSY`，不排队。对话框是模态的用户动作，排队只会让后来的调用等一个已过期的意图。
3. **执行线程**：单独 `std::thread`，不占插件回调线程（见 §4.3）。线程里：
   - owner：`crate::island::hwnd(&crate::shared(app_handle()?))`；为 0 时退回无 owner；
   - 复用 [`paper.rs:583`](../../app/src-tauri/src/paper.rs#L583) 的 `pick(true, title, "", owner)`：它已经在 STA 上初始化 COM、用 `IFileOpenDialog` + `FOS_PICKFOLDERS` + `FOS_FORCEFILESYSTEM`、设置标题与 owner；
   - 成功 → `{"dir": path}`；取消（`pick` 回 `None`）→ `{"dir": null}`；COM 失败 → `DIALOG_OPERATION_FAILED`。
4. **标题**：插件给了就用（1–80 字，去控制字符）；没给时用宿主 i18n 的新键 `dialog.chooseFolder`（中文「选择一个文件夹」，英文「Pick a folder」），加在 [`i18n.rs`](../../app/src-tauri/src/i18n.rs)。
5. **日志**：只记 `dialog: <plugin> chooseFolder shown` / `chosen` / `cancelled`，**不记完整路径**（与 file-drop 的日志口径一致）。
6. **平台**：`#[cfg(not(windows))]` 直接回 `DIALOG_UNSUPPORTED`；不复用 `paper` 的非 Windows 空实现去冒充「取消」。

**代码复用说明**：`paper::pick` 目前同时服务壁纸（选图片 / 选文件夹）。直接复用可以避免复制 120 行 COM 代码；代价是 `plugin_dialog` 依赖 `paper` 的模块名。若实现时觉得边界不清，可以把 `pick` 与 COM 助手整体移到 `pick.rs`，`paper` 与 `plugin_dialog` 都用它——这是重构，不是本次必须；两种做法都要保持 `paper` 现有行为不变。

### 4.3 超时：不能让 SDK 的 5 秒把对话框判死

SDK 的宿主调用默认 5 秒、冷态最多 20 秒（[`sdk/node/plugin.mjs:38`](../../sdk/node/plugin.mjs#L38)）。用户选文件夹经常超过 5 秒，如果走普通请求，对话框还开着，Promise 已经失败。

所以：

- 宿主**不在回调线程里阻塞**等用户。收到 `host.dialogs.chooseFolder` 后立刻把请求转给工作线程，回调线程继续服务同一插件的其他调用。
- 回调线程对 `Dialog` 分派**不写通用响应**（响应由工作线程写），否则同一个请求会收到两帧；这是分派函数必须显式处理的一条。
- 工作线程在用户关闭对话框后，用同一个请求 `id` 写回响应帧（`SyncSender` 可克隆）。宿主不设超时；用户不关，框就一直开着。
- SDK 对这一个方法使用**无超时**的 pending（`hostFor` 增加内部选项 `noTimeout`），收到响应或会话关闭才 settle。
- 会话在对话框开着时被停用 / 卸载 / 换版：工作线程结束时写回的队列已经关闭，忽略即可；新会话不受影响，互斥标志在 `guard` 析构时释放。

这一条是本次设计里最容易做错的地方：**照普通 `await` 直接实现，功能在用户手上 5 秒就会失败**。

## 五、安全

- **能力隔离**：只有声明 `dialogs` 的后端能调用；页面拿不到；跨插件句柄 / 身份伪造无从谈起（没有句柄）。
- **不按插件给的路径打开任何东西**：对话框只返回用户选中的那一个目录；插件不能借此探测任意路径。与 file-drop 的边界一致：用户主动选择的结果可以交给插件。
- **互斥**：一次一个，恶意插件不能刷出一堆模态框。
- **owner 与模态**：挂在主岛窗口上，避免对话框跑到后台；主岛不可见（隐藏 / 全屏让开）时仍可弹，但 owner 可能为 0，日志记一行。
- **不放开文件选择**：只 `FOS_PICKFOLDERS`，用户不能借这个入口把任意文件路径交给插件。
- **SDK 本地拒绝 + 宿主复核**：两层都查能力与参数，任一层缺失都失败。

## 六、验收

### 6.1 单元测试

| 测点 | 期望 |
| --- | --- |
| `host_call("host.dock.rect")` / `host.dialogs.chooseFolder` / `host.storage.read` | `Window` / `Dialog` / `Storage` |
| 清单能力 `["storage","dialogs"]` | 通过校验 |
| 清单能力 `["dialogues"]` | 报「当前宿主不支持的能力」 |
| 未声明 `dialogs` 时调用 | 回能力错误，不弹框 |
| `title` 为空、81 字、含 `\n`；`parent` 为 `'window'`；未知字段 | 本地拒绝 / 宿主拒绝 |
| 第二个请求在第一个未结束时到达 | `DIALOG_BUSY` |
| SDK 侧延迟响应（模拟 6 秒） | Promise 仍 resolve，不被 5 秒超时打断 |
| SDK 侧会话关闭 | Promise reject，无悬挂 |

对话框本身不能在自动测试里真弹；用一个可注入的 opener 替身（测试里记录参数、返回固定路径或 `None`）覆盖「成功 / 取消 / 失败」三条路径，真机验收另列。

### 6.2 手工验收（Windows，必须有桌面）

1. 插件声明 `dialogs`，调用 `chooseFolder({title:'选择备份文件夹'})`：弹出 Windows 文件夹框，标题正确，owner 是主岛，不出现在任务栏 / Alt+Tab 之外。
2. 取消：返回 `{dir:null}`，插件不报错。
3. 选中含空格与中文的路径：原样返回。
4. 未声明 `dialogs` 的插件：得到明确错误，不弹框。
5. 两个插件同时调用：第二个得到 `DIALOG_BUSY`，第一个不受影响。
6. 对话框开着时退出 Dock：进程正常退出，不留悬挂线程；下次启动无残留。
7. magpie 插件（P1 接入后）在网页模式里点「Change…」能选目录，且选完写回的是选中的路径。

## 七、文档

- [`docs/guides/plugin-windows-2.2.0.md`](../guides/plugin-windows-2.2.0.md)：能力表加 `dialogs`，Node SDK 表加 `context.dialogs.chooseFolder`，补一段「选文件夹」的示例与超时说明。
- `sdk/node/plugin.mjs` 的注释与 `sdk/browser` 的说明（注明页面侧不提供）。
- 发行说明 2.3.0 的「插件宿主能力」一节。
- 插件仓交接文档的 P1 落点（在插件仓发版说明里引用宿主版本）。

## 八、版本与回滚

- 宿主能力是新增 API，按 minor 走：**2.3.0**。
- 回滚：删掉 `host.dialogs.*` 路由与 SDK 方法即回到 2.2.3 行为；没有数据迁移、没有持久状态。
- 旧插件不受影响：没有 `dialogs` 能力就不会走到新代码。

## 九、开放问题

1. 页面侧是否也要 `FishDock.dialogs.chooseFolder`？本版不做；若做，需要页面模态、能力与插件窗口 owner 的完整设计。
2. `parent` 是否接受插件窗口句柄？本版只接受 `'main'`；插件窗口作为 owner 需要句柄归属与代次校验。
3. 要不要给「选择文件」同一条能力？本版明确不做；文件路径交给插件比目录更敏感。
4. `paper::pick` 是否抽成 `pick.rs`？实现时按代码整洁度决定，行为不变是硬约束。
