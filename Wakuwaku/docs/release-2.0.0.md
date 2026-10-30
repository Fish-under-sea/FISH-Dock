# FISH Dock 2.0.0

宿主升到 2.0.0：所有版本位对齐，旧发行名按「沿用 / 迁移 / 清理」三档收尾，用户
数据目录迁到 `FISH Dock` 之后旧目录直接删除。宿主与插件同批发布，四个官方插件
也全部 2.0.0。

## 版本锚点

一处改了别处没跟上，是这次最容易被测试抓住的地方。2.0.0 起下面这些位置写的是
同一个版本号，`updates::tests::one_version_everywhere` 盯着前两项：

| 位置 | 作用 |
| --- | --- |
| `src-tauri/tauri.conf.json` → `version` | Tauri 元数据，安装器 `ProductVersion` 取自这里 |
| `src-tauri/Cargo.toml` → `package.version` | 宿主二进制版本（`CARGO_PKG_VERSION`） |
| `src-tauri/Cargo.lock` → `name = "wakuwaku"` | 锁文件里的自身条目，随 `Cargo.toml` 同步 |
| `integrations/claude-code/.claude-plugin/plugin.json` → `version` | Claude Code 集成清单，同一条测试比对 |
| `README.md` / `README.en.md` / `Wakuwaku/README.md` / `Wakuwaku/README.en.md` | 版本徽标、下载与更新记录链接 |
| `tools/check-readme.mjs` | 不再自带版本字面量，改读 `tauri.conf.json` 再校验四份文档 |

## 双包

`v2.0.0` 照旧发布两个包，校验值在同一条 Release 里：

| 安装包 | 使用场景 |
| --- | --- |
| `FISH-Dock-Setup-2.0.0.exe` | 首次安装、基础资源缺失或修复安装，含完整离线运行时 |
| `FISH-Dock-Update-2.0.0.exe` | 已有安装且基础资源完整时只下载变更文件；缺资源会提示改用完整包 |

## 旧名三档

改名的风险不在「改」，在「改错档」：动到用来匹配的名字，升级后的程序就认不出
自己写过的东西，安装器也清不掉旧值。所以按「动了会不会丢用户数据、会不会让升级
找不到已登记的安装位置」分成三档。#t16 + #t18 之后，原来的「沿用不动」一档已经
不存在：用户看得见的名字全部换成 `FISH Dock`，旧名只留在「读取/识别」这一侧。

### 已改名（写出的一律是新名）

- 主程序文件名 `FISH-Dock.exe`（Cargo 包名 `fish-dock`，`[[bin]] name = "FISH-Dock"`）；
- 应用标识 `com.fish.dock`，通知 AUMID 同步（`notify.rs` 的 `APP_ID`）；
- 任务名 `FISH Dock at logon-<SID>`、传感器任务 `FISH Dock sensors-<SID>`；
- 命令行开关一律写 `--fish-dock-*`（12 个：ensure-running / codex-hook / agy-hook /
  zcode-hook / dsh-hook / reasonix-hook / out-of-job / hw-dump / lhm / sensors /
  sensors-setup / sensors-remove，另加 at-login 与 uninstall-cleanup）；
- 传感器助手 `FISH-Dock-sensors.exe`，助手目录 `%ProgramFiles%\FISH Dock\sensors`；
- 用户数据目录 `%LOCALAPPDATA%\FISH Dock`、卸载表登记名 `FISH Dock`、Run 值名 `FISH Dock`。

### 仍兼容读取（旧名只读不写，至少留一个版本）

- 旧 exe 名 `DSH-FISH.exe`、`wakuwaku.exe`、`wakuwaku-sensors.exe`：安装器与更新器按旧名
  识别并删除（`LEGACY_EXE_NAMES` / `EXE_NAMES`），有清单的安装也会补扫；
- 旧开关 `--wakuwaku-*` 与更早的 `--claude-pets-ensure-running`：`src/flags.rs` 的别名表
  在进程入口归一成新名；旧 hooks/旧任务里的写法仍能跑，`connection.rs` 在修复/连接时把
  它们改写成新名（`is_ours` 认新旧两套）；
- 旧任务名 `Wakuwaku at logon-<SID>` / `Wakuwaku sensors-<SID>`、旧 Run 值 `Wakuwaku`、
  旧卸载表登记 `DSH-FISH` / `Wakuwaku`：只认、修正与删除，不再写；
- 旧占位符 `__WAKUWAKU_PORT/EXE/DATA__`：omp 扩展模板与新名并存、两套都填。

### 明确不改（外部身份，不是本产品的名字）

- Claude Code 插件 id `wakuwaku@wakuwaku` 与其 marketplace 命令：已发布到插件市场，
  改了等于让所有人的已装插件失效；
- `integrations/` 下的文件名（`wakuwaku.js` / `wakuwaku.ts`）与 omp 的 `extensions/wakuwaku.ts`：
  其他 agent 按文件名加载，改名会让它们找不到；
- 事件上报里的 `from=wakuwaku` 埋点：远端统计与崩溃去重按它归属，属于对外协议；
- 上游许可文件名 `licenses/Wakuwaku.txt`：这是许可方的名字，不是我们的。

### 迁移：用户看得见的旧名 → `FISH Dock`

- 任务计划里给人看的作者与说明（`logon.rs` 的 `task_xml` / `DESCRIPTION`）；
- 用户数据目录 `%LOCALAPPDATA%\DSH-FISH` → `%LOCALAPPDATA%\FISH Dock`（见下一节）。

任务名本身（`Wakuwaku at logon-<SID>`）不改：它是用来匹配的，不是给人看的。

### 清理：旧发行名留下的登记与文件，安装时按归属删掉

- 旧主程序 `$INSTDIR\wakuwaku.exe`：安装段就删，不等卸载（删旧建新，不留旧 exe）；
- 指向旧 exe 的 Run 值，与它在 `StartupApproved\Run` 里的同名标记：只按
  「本安装目录 + 我们自己的旧文件名」精确匹配整条值文本，别的程序写的同名值不动；
  指向当前主程序的那条是用户的选择，保留；
- 旧卸载表登记项（`Wakuwaku`、`FISH Dock`）：登记位置就是本目录、或显示名是我们
  自己用过的旧名，才整键删除；
- 卸载表里那条带旧发行名的 `ProductName`：2.0.0 起只用 `DisplayName` +
  `DisplayVersion`。

卸载段再兜一遍底：Run 值、同名标记、旧 exe、旧登记项都按同一套归属判定清理，
不递归删目录、不碰用户数据。旧版回滚点不再保留 —— 只留当前版本。

## 用户数据目录迁完即删

安装器带上 `--migrate-home` 才会动用户数据目录；不带这个参数（包括开发版直接跑
`install-update.cjs`）一个目录都不碰。迁移顺序是「整树盘点 → 逐个复制 → 逐个核对
SHA-256 → 两份清单对齐 → 才删旧目录」，所以：

- 迁完：旧目录直接删除，不留回滚副本；
- 没迁完（新目录已存在、复制中断、摘要不符、旧目录不在）：旧目录原样保留，程序
  照常可用，安装器只提示「完全退出后重跑一次安装程序」；
- 旧目录被 WebView2 占用删不掉：数据已经在新目录里核对过，不算迁移失败，旧目录
  留下并记一行，交给用户手动清；
- 树里有链接或非普通文件：拒绝迁移，不跟着链接把树外的东西算进来。

安装器读 `home-migration-failed` 标记必须排在删除暂存目录之前 —— 失败分支同样会
把暂存目录整份删掉，读晚了就什么也读不到。

## 兼容性

本版是 **breaking**：只支持 Dock ≥ 2.0.0 与同批发布的插件 2.0.0。插件目录条目结构
从 `author` + `maintainer` 改成 `authors[]` + `note`，旧 Dock 读不出新目录，插件页
会是空的；1.x 装 2.0.0 插件同理。升级时请整套一起换。

## 验证

- `node scripts/installer.test.mjs`：15/15，含「离线 makensis 真编出安装包」（makensis
  在本仓的中文路径下写不出安装包，测试把 NSIS 工具树暂存到 ASCII 临时目录再编）。
- `node scripts/installer-upgrade.test.mjs`：9/9，含「NSIS 通过真实 Node 和 PowerShell
  5.1 完成中文路径安装与覆盖」与 #27-D 的迁完删除 / 未迁完不删 / 重试 / 删旧失败 /
  拒绝跟随链接。
- `node scripts/prepare-installer.test.mjs`：6/6。
- `node scripts/branding.test.mjs`：6/6。
- `node tools/check-readme.mjs`：四份文档的版本锚点、相对链接与锚点全部通过。