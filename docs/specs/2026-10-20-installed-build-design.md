# DSH-FISH 安装版设计

日期：2026-10-20。状态：用户已批准安装版方案，实现与静态验证完成；最终安装包真机安装、快捷方式与卸载验收留给 acceptance 成员统一执行，详见 [构建与验收记录](../guides/installer-build.md)。

## 1. 目标与边界

交付物是一个自包含安装包 `DSH-FISH-Setup-<版本>.exe`。安装时把主程序、Node.js、固定版 WebView2 与本地模型服务一次展开到用户选的目录，之后日常启动直接使用安装目录里的这些文件：不解压、不对整树（约 776 MiB）做完整性哈希、不递归重设权限。安装器提供安装位置选择、开始菜单与桌面快捷方式、控制面板可见的卸载入口。

不并行维护便携单文件（自解压）版本。上一版单 EXE 在每次冷启动时把内嵌归档释放到 `%LOCALAPPDATA%\DSH-FISH\runtime\<摘要>`，并在每次启动复核整树摘要；本方案把这两件事都移到安装时做一次。

「自包含」限定为启动依赖齐全、不联网下载运行环境。调用在线模型、账号登录、获取在线模型目录仍需网络；不承诺离线推理，不内置模型权重或账号。

## 2. 安装布局

```text
<安装目录>\DSH-FISH.exe        主程序（feature installed）
<安装目录>\resources\node\      官方 Node.js 24 x64 完整目录
<安装目录>\resources\webview2\  官方 WebView2 固定版 x64 完整目录
<安装目录>\resources\ofm\       本地模型服务运行文件
<安装目录>\resources\licenses\  许可原文与运行时来源清单
<安装目录>\resources\manifest.json   文件清单（schema 2）
<安装目录>\uninstall.exe        卸载入口
```

默认安装目录 `%LOCALAPPDATA%\Programs\DSH-FISH`，用户可改；安装位置记在 `HKCU\Software\Microsoft\Windows\CurrentVersion\Uninstall\DSH-FISH` 的 `InstallLocation`，重装时默认回到原位置。

主程序用 `current_exe().parent()/resources` 定位安装资源；`DSH_FISH_RESOURCES` 只用于测试与显式覆盖。安装版不从 PATH 找 Node，也不回落到旁置外部依赖：资源缺失或损坏时报错并提示重新运行安装程序。

## 3. 启动时做什么

1. 读 `resources/manifest.json`（schema 2）当清单用。**清单本身不再算摘要**：换一份清单不再等价于换一套资源。
2. 判据是 `%LOCALAPPDATA%\FISH Dock\state\verified` 里**逐文件的 size+mtime 指纹**，加上规范化后的资源目录。标记是一份 JSON：`{ "schema": …, "resources": …, "files": { "<path>": { "bytes": …, "mtime": … } } }`。逐文件比较全等即通过（只 stat、一个字节都不进哈希）；不一致（首次启动、换了新安装、目录搬走或复制到别处、或其中几个文件变过）时只对变化的那几个文件重算 SHA-256，首次启动与换版本是整棵树，并在失败时点名损坏的文件。
3. 校验通过后确认 WebView2 固定版许可（与之前相同的确认窗口，同意后记住）；WebView2 沙箱只读授权改为**按需**——判据认为缺权限才做一次（`icacls`，仅两个 SID、只读、仅 `resources\webview2`），不再是「首次启动一次性递归授权」。
4. 全部成功后才写标记，且是原子替换（同目录临时文件 → `storage::replace`，不 truncate 目标文件）；失败只删临时文件、旧记录原样保留，下次启动重新来过。

日常启动因此没有解压、没有整棵树的哈希、没有递归授权；变化文件的哈希由后台线程补算，失败点名损坏文件。代价是安装目录里的资源被人为改动后，要等后台那次校验完成才会发现（首次启动与换版本时会发现）。

## 4. 数据位置与卸载

| 数据 | 位置 | 安装/卸载是否触碰 |
| --- | --- | --- |
| 设置、宠物、桌面布局、日志 | `%LOCALAPPDATA%\DSH-FISH\data` | 否 |
| WebView2 用户数据 | `%LOCALAPPDATA%\DSH-FISH\webview` | 否 |
| 首次校验标记、许可确认 | `%LOCALAPPDATA%\DSH-FISH\state` | 否 |
| OFM 账号、密钥与用量 | `%USERPROFILE%\.dsh-fish` | 否 |
| 主程序与运行时 | 安装目录 | 是 |

README 里描述的用户数据位置（`%LOCALAPPDATA%\DSH-FISH` 与 `%USERPROFILE%\.dsh-fish`）与便携版一致，安装、重装、升级都不迁移不清空。旧版便携 EXE 旁存在 `wakuwaku-data` 且新数据目录不存在时，首次启动复制并核对后才启用新目录，不删除旧目录。

默认卸载只删安装器自己放下的东西：主程序、`resources` 下清单里的每个文件、卸载器、两个快捷方式、开始菜单文件夹、卸载注册表项，以及指向本次安装的 `Wakuwaku` 开机自启动值。卸载不做 `RMDir /r`，也不在脚本里提到 `data`、`webview`、`.dsh-fish`——用户把程序装进别的目录（比如某个已有文件夹）时，递归删除会连带删掉不属于本程序的文件。

## 5. 资源来源与安全

Node.js 与 WebView2 固定版只用官方发布件，版本与 SHA-256 锁在 [运行时来源清单](../../app/scripts/runtime-sources.json)；构建时先核对摘要再解包，不信任之前的解压目录。固定版 WebView2 不依赖系统安装，也不自动获得 Evergreen 更新；安全修补靠重新打包发布安装包。安装版禁止安装上游 Wakuwaku 的在线更新，避免被不含安装资源的程序覆盖。

不关闭 Windows 安全功能，不绕过 UAC、SmartScreen 或杀毒检查。安装到用户目录、按当前用户安装，不需要管理员权限。

## 6. 修改范围与非目标

改 `src-tauri/src/runtime.rs`（安装资源路径、首次校验、状态记住、许可与授权）、`oflm.rs`（只用安装目录里的 Node 与服务）、`data.rs`（用户数据位置）、`build.rs` 与 `Cargo.toml`（feature `installed` 取代 `single-exe`）、`scripts/prepare-installer.mjs`（准备安装资源）、`scripts/installer.mjs`（生成 NSIS 脚本并调用 makensis）、`scripts/build-installer.ps1`（构建入口）。

不重做前端与宠物界面，不改 agent 统计与业务逻辑，不做在线更新平台、不做自动下载模型权重、不做代码签名。

## 7. 验收标准

1. 交付一个自包含安装包，含主程序、Node、固定版 WebView2、本地模型服务；安装时可改安装位置，建立开始菜单与桌面快捷方式，注册卸载入口。
2. 已安装程序用明确的安装资源路径，不需要系统 Node 或另装依赖；日常启动不解压、不整树哈希、不递归授权。
3. 账号、密钥、用量、设置与 WebView 用户数据沿用现有位置；默认卸载只删安装资源。
4. 新增与变更行为先红后绿；安装资源缺失时明确提示修复或重装，不静默退回外部依赖。
