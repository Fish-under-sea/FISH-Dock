# FISH Dock 安装版构建与验收记录

日期：2026-10-21。此记录对应安装版（安装时展开资源、日常直接使用安装资源）。

> 当前版本为 FISH Dock 1.0.1。下方 1.0.0、0.3.4 和 0.3.3 数据均为历史证据，不代表本版实机验收。内部应用标识与主程序名保留兼容；身份迁移规则见本版更新记录。

## 0. 当前版本：FISH Dock 1.0.1

本版完成统一 `.fish-dock` 身份目录、旧资料复制保留、邮件 CurrentUser DPAPI、原生完整身份 ZIP 导入导出、可编辑的系统 Downloads 默认路径、LHM 可见提权入口，以及选择性 OFM v2.5.7 修复。具体范围见 [1.0.1 更新记录](release-1.0.1.md)。

| 当前产物 | 实际结果 |
| --- | --- |
| 安装包 | `FISH-Dock-Setup-1.0.1.exe`，257,596,233 字节，约 245.7 MiB |
| 安装包 SHA-256 | `2A5F972ACDE6C471FBB098450615777A1EB56188219E022C80E68BB09DAC9970` |
| 主程序 | `stage/DSH-FISH.exe`，13,950,976 字节 |
| 主程序 SHA-256 | `6F21FFB3DE79D9F99D6F82292AEAFD9CAE1B13694D24536E7076CFB9DD0CC634` |
| 版本及签名 | 两个 PE 的 ProductName 均为 FISH Dock，ProductVersion 为 1.0.1；Authenticode 状态 NotSigned |
| 安装资源 | 清单内 2,202 个资源文件，含清单共 2,203 个；展开 813,636,053 字节 |
| 构建 | Release 及 NSIS 完整构建退出码 0；没有运行安装器或覆盖用户安装 |
| 默认路径 | 新安装 `%LOCALAPPDATA%\Programs\FISH Dock`；升级沿用登记位置；原图标及内部应用标识不变 |

最终验证：

- 身份定向 21 项通过；品牌、原生身份 UI、模型页及真实 NSIS 定向 28 项通过。最后完整桌面 JavaScript 为 279 项：278 通过、1 个既有 `test/wdesk.test.js:133` 用量文案断言失败。
- 完整 Rust 473 项：459 通过、2 个既有环境失败、12 忽略。失败为 LHM ACL 权限拒绝和 Task Scheduler 根目录 `0x80070003`；没有为全绿削弱断言。
- CodeArts 本机签名传输 12 项、多上下文 RPC 7 项通过；严格测速独立进程 9／11／2 项通过。渠道适配器离线检查 4 项通过，管理状态／HTTP／LAN／备份合计 7 项通过。
- 实际 Edge 在 1440×1100 和 390×844 验证设置保存、页面交互及替身请求；无横向溢出与 JavaScript 异常。两次调用使用内存替身，不代表真实账号或原生桌面系统鼠标验收。
- 封装 Node v24.14.0 在空 PATH、NODE_PATH、NODE_OPTIONS 下校验 2,202 个资源；页面、管理鉴权、LAN 默认关闭及同端口模式、OFM 专项 ZIP、固定 Markdown及管理状态持久化通过，没有使用真实身份资料。
- 构建验收阶段没有执行真实资料迁移、实机安装、原生导入导出对话框实操、实际 UAC、跨设备邮件／DPAPI 联调或真实渠道请求。未签名，不承诺 SmartScreen 提示消失。

升级前从托盘完全退出旧版，并保管好已有旧资料备份。完整身份 ZIP 含明文凭据，应按密码文件保管，不上传 GitHub、工单或公开截图。

## 0.0 历史版本：FISH Dock 1.0.0

本版选择性应用 Wakuwaku 0.3.4 的功能修复和 OFM 异常 Token 计量修复，新增默认关闭的同端口 LAN 推理与独立密钥、仅 OFM 的配置凭据迁移 ZIP。关于页及中英文首页已更新；来源和范围见 [1.0.0 更新记录](release-1.0.0.md)。没有引入广告、启用上游更新下载或崩溃上传，也没有推送或发布 GitHub。

| 当前产物 | 实际结果 |
| --- | --- |
| 安装包 | `FISH-Dock-Setup-1.0.0.exe`，257,551,880 字节，约 245.6 MiB |
| 安装包 SHA-256 | `3FEA513807B99E0E0FCCC8179C249DE0CE4B7DD93AC7E053B645CE312F46356A` |
| 主程序 | `stage/DSH-FISH.exe`，13,719,040 字节 |
| 主程序 SHA-256 | `F3887F47EAE5D0E8AEB1BBA66799A377F334A8D50711CF98716CBD2D53B569E5` |
| 版本与签名 | 主程序和安装器 ProductName 均为 `FISH Dock`，ProductVersion 均为 `1.0.0`；Authenticode 状态 `NotSigned` |
| 默认路径与兼容 | 新安装 `%LOCALAPPDATA%\Programs\FISH Dock`；升级沿用登记位置；旧数据目录和原始图标不变 |
| 构建 | Release 主程序编译成功，NSIS 完整构建退出码 0；未运行安装器或覆盖用户安装 |
| 封装验证 | 内置 Node v24.14.0，空 PATH / NODE_PATH / NODE_OPTIONS 下启动封装 OFM；本机页面、LAN 默认关闭及同端口模式、迁移 ZIP、固定 README 与 fflate 许可均验证通过 |

本轮回归与边界：

- Rust 全量 448 项：434 通过、2 失败、12 忽略。失败为既有 `lhm_setup` ACL 权限拒绝与任务计划根目录 `0x80070003` 环境限制；不是全量通过。
- 最终桌面 JavaScript 与安装脚本 275 项：274 通过、1 失败。失败仍为既有 `test/wdesk.test.js:133` 用量文案断言；没有回退已有 Token 显示。
- 同端口 LAN、OFM ZIP 鉴权与合成账号恢复、界面接线通过。异常 Token 计量用本地 SSE 替身在独立 Node 进程通过；不可与提前导入适配器的测试共用环境模块缓存，已加防意外出网断言。
- 真实 Edge 的 1440×1100 与 390×844 页面布局、同端口地址、查看／轮换密钥和 ZIP 下载通过。使用隔离替身账号及 DOM 驱动，不代表系统级鼠标、两台物理设备 LAN、真实上游账号或邮件联调通过。
- 两个源码项目 `git diff --check` 通过。原始 ICO 和 PNG SHA-256 分别仍为 `5130FE9D4E381DE4692446AA08729AD2DE59CCA443F7D40367FBCDFF41FBFED0`、`5688F69DE350B8C75E0832E7B5928C680A1B7FAA14ECD0259306AFBD5C4178B9`。
- 没有运行 1.0.0 实际安装或测量启动速度。升级前从托盘完全退出旧版，避免 WebView2 DLL 被占用。本版未签名，不保证消除 SmartScreen；备份 ZIP 未加密，应按密码文件保管。

## 0.1 历史版本：FISH Dock 0.3.4（默认目录修正版）

全新安装的默认程序目录改为 `%LOCALAPPDATA%\Programs\FISH Dock`。已有安装仍读取 `InstallLocation` 用于覆盖升级，避免更改默认值后另装一份；用户可在安装页面自行选择其他目录。账号、设置与 WebView 用户数据目录不变，没有自动搬迁已安装程序。

本轮只重建安装器，复用下面记录的原图标主程序和同一套运行时。回归先验证旧目录断言失败，再修改安装脚本；安装器与品牌测试合计 13 项全部通过，包含真实离线 NSIS 编译。不改用自绘图标，不启停用户程序，不部署到真实安装目录。

| 当前产物 | 本轮实际结果 |
| --- | --- |
| 安装包 | `FISH-Dock-Setup-0.3.4-new-path.exe`，257,471,292 字节，包含 2,197 个资源文件 |
| 安装包 SHA-256 | `E02FCE07C26656DC6251687E7F300CC774C70F506E2F266C1F4B3BE2DC3EBDC3` |
| 主程序 SHA-256 | `95F89BC0A8DB68277A261B3D1A084984A353900B05E3ABC3B12BA700A70A6630`，与原图标修正版相同 |
| 默认目录 | 全新安装使用 `%LOCALAPPDATA%\Programs\FISH Dock`；覆盖安装沿用已有 `InstallLocation` |
| 版本及签名 | ProductName 为 `FISH Dock`，ProductVersion 为 `0.3.4`；仍未数字签名 |
| 验证范围 | NSIS 完整构建退出码为 0，生成脚本默认路径及升级登记已核对；未运行真实安装，也不宣称已迁移旧安装目录 |

### 原图标修正记录

上一轮按用户要求撤换自绘鱼形图标，直接使用之前提供的蓝发角色图标；没有重新绘制、改写原素材、新建团队、推送仓库或覆盖真实用户安装。此前的 `FISH-Dock-Setup-0.3.4.exe` 是自绘图标版，不应使用；下面带 `original-icon` 后缀的是原图标修正时的历史产物，本轮新包保留同一原图标。

| 项目 | 本次实际结果 |
| --- | --- |
| 安装包 | `FISH-Dock-Setup-0.3.4-original-icon.exe`，257,489,311 字节，约 245.6 MiB |
| 安装包 SHA-256 | `CF2DCC1572ACD05CE7C97A191048070B1AB874BEFF2FDD3FD63FD900D4CB8D16` |
| 主程序 SHA-256 | `95F89BC0A8DB68277A261B3D1A084984A353900B05E3ABC3B12BA700A70A6630` |
| 二进制版本信息 | 主程序和安装器的 ProductName 均为 `FISH Dock`，ProductVersion 均为 `0.3.4` |
| 原始 ICO SHA-256 | `5130FE9D4E381DE4692446AA08729AD2DE59CCA443F7D40367FBCDFF41FBFED0` |
| 原始 PNG SHA-256 | `5688F69DE350B8C75E0832E7B5928C680A1B7FAA14ECD0259306AFBD5C4178B9` |
| 原图一致性 | ICO、PNG 与改名前保存的审查副本摘要相同；主程序和安装器均包含 ICO 原始的全部 10 个尺寸图像帧；提取的 32 px 图标与原图 RGBA 像素完全一致 |
| 图标入口 | 应用、托盘、通知、安装器、卸载器、中英文 README 直接引用用户素材；通知改用新的图标缓存文件名，避免沿用自绘图标缓存 |
| 数字签名 | 未申请证书，不承诺消除 SmartScreen 提示 |

本次实现：Windows 全新配置默认任务栏；后续沿用保存的任务栏或灵动岛模式、边缘、岛位置和显示器，显示器失效时回到主屏。两种模式在启动时分别播放入场，灵动岛不先闪出任务栏；减少动态效果时短淡入。品牌、应用图标、安装与卸载图标、中英文 README、项目入口统一到 FISH Dock 和自有 GitHub 仓库。旧数据目录、应用标识和内部主程序文件名保留兼容；未启用原项目的更新下载或崩溃上传服务。

本次测试结果：

- 品牌、安装器、启动动画三个测试文件合计 18 项，全部通过，包括真实离线 NSIS 编译。
- 全量 JavaScript：274 项，273 通过，1 失败。失败仍是既有 `test/wdesk.test.js:133` 文案断言：期望「每小时的 token 暂无数据」，实际界面已有「今天的 token 1.7B token · 输出 3.4M」。未修改这项断言，也未回退用户已有的用量组件改动。
- 0.3.4 功能改版时已跑过全量 Rust：432 项，419 通过、2 失败、11 忽略。两项失败仍为第 7 节记录的 `lhm_setup` 沙箱权限和任务计划服务限制。本轮仅修改图标引用及通知图标缓存文件名，没有重复运行该全量套件；已重新编译 Release 主程序，不宣称全量通过。
- Release 主程序编译成功，NSIS 完整安装包生成成功，`git diff --check` 通过。

验证边界：本次没有向真实用户环境安装或启动新版，没有取得真实启动动画、跨显示器恢复或模型页鼠标交互的实机验收。隔离 Edge 无头预览退出码为 `-2147483645`，未生成截图，不将其算作视觉验证通过。旧版管理员安装后可正常运行是用户反馈，不是新版测试结果。安装速度与日常界面可用时间仍没有新版实测值。

## 1. 交付物

`FISH-Dock-Setup-<版本>.exe` 是唯一需要分发的文件。安装时把主程序、Node.js、固定版 WebView2、本地模型服务与许可原文展开到用户选的安装目录；卸载入口在「设置 → 应用」和开始菜单里。

| 项目 | 值 |
| --- | --- |
| 平台 | Windows 10/11 x64 |
| 安装方式 | 按当前用户安装，可改安装位置，全新安装默认 `%LOCALAPPDATA%\Programs\FISH Dock`；已有安装沿用登记的原目录 |
| Node.js | 24.14.0，官方完整 Windows x64 发行目录 |
| WebView2 | 155.0.4283.62，官方固定版 x64 完整目录 |
| 安装资源 | 2,196 个文件，展开后 813,504,286 字节（约 776 MiB） |
| 主程序 | feature `installed`，不内嵌运行时归档 |

本机实测分两轮，两轮的数字不要混用。

**历史产物（0.3.3，t8 修复轮，2026-10-20，Windows x64）**：安装包 257,303,324 字节（245.4 MiB），SHA-256 `6308D795CE93CF07EC23A2920DC4B381065761659F668B58D82DD7D8510D3904`，文件版本 0.3.3，包内 2,197 个文件；主程序 `DSH-FISH.exe` 13,667,840 字节 / SHA-256 `92CFC69BEA10675D8EC2F92922CB0695FD97A103A3EC9D19CFD2028804993B6F`。静默安装（`/S /D=<目录>`）到工作区内的目录：退出码 0，落盘 2,199 个自有文件（2,196 个资源文件 + `manifest.json` + 主程序 + `uninstall.exe`），加上安装前就放在那里的 `neighbour.txt`，目录内共 2,200 个文件；静默卸载（`uninstall.exe /S`）：退出码 0，约 5 s 内安装目录只剩 `neighbour.txt`——主程序、`uninstall.exe`、`resources` 及其 496 个子目录全部清空，而隔离的 home 目录里 `data/account.json`（内容 `{"keep":true}`）与 `webview/Cookies` 都还在。这一轮**没有测启动耗时**。

**上一轮（t3 验收轮，安装包是 2026-10-20 01:43 构建的 257,167,816 字节 / SHA-256 `804F1F8F36CA97A540D00A32B2AB4FA064D0A43117A8B0922A4CD46F4E353DB8`，主程序 13,675,520 字节）**：静默安装 22.42 s，落盘 2,199 个文件 / 827,676,854 字节 / 496 个目录（同样是 2,196 个清单文件 + `manifest.json` + 主程序 + `uninstall.exe`；测试用的 `我的笔记.txt` 是安装之后才放进去的，不在这个计数里）；静默卸载 0.56 s。启动只测到**失败路径**：首次启动 +4.9 s 到许可对话框、+15.9 s 以「无法设置内置浏览器的沙箱读取权限：exit code: 5」退出，此后两次启动 1.34–1.38 s 到达同一个失败框。这些都是「走到失败框」的时间，不是界面可用时间，也不能用来推断日常路径做了多少工作。

**启动计时口径**：早前记录里的「首次启动全量校验耗时 83.4 s」与「日常启动 0.44 s 内走完校验判断」在现有证据里找不到对应的原始日志和计时终点，**已撤回**，不再作为任何结论或宣传口径。日常快路径目前只有源码与单元测试证据，没有任何运行证明，原因见第 7 节。体积与哈希会随版本变化，当前版本以第 0 节的实际构建记录为准。

## 2. 安装后的布局

```text
<安装目录>\DSH-FISH.exe
<安装目录>\uninstall.exe
<安装目录>\resources\manifest.json
<安装目录>\resources\{node,webview2,ofm,licenses}\...
```

用户数据不在安装目录里，安装与卸载都不碰：

```text
%LOCALAPPDATA%\DSH-FISH\data       设置、宠物、桌面布局、日志
%LOCALAPPDATA%\DSH-FISH\webview    WebView2 用户数据
%LOCALAPPDATA%\DSH-FISH\state      首次校验摘要、许可确认
%USERPROFILE%\.dsh-fish            OFM 账号、密钥与用量
```

## 3. 安装时与日常启动各做什么

安装时（NSIS，一次）：

1. 把主程序与 `resources` 下清单里的每个文件摆进安装目录（`File`/`File /r`，不经过运行时解压）。
2. 对 `resources\webview2` 做一次只读授权：`icacls ... /grant *S-1-15-2-1:(OI)(CI)(RX) *S-1-15-2-2:(OI)(CI)(RX) /T`。失败不中止安装，但会在安装日志里记下行，首次启动会再试一次并把失败讲清楚——没有这份授权，固定版 WebView2 建不出界面（只会报 "灾难性故障"）。
3. 建开始菜单与桌面快捷方式，写卸载注册表项与 `EstimatedSize`。

日常启动（主程序）：

1. 读 `resources\manifest.json`，把它的 SHA-256 当验证钥匙。
2. 与 `state\verified` 记住的一致时只检查 5 个必需文件存在且大小一致。标记里同时记着清单摘要和**规范化后的安装资源目录**（canonicalize、去尾分隔符、按 Windows 的比较方式统一大小写与斜杠）：只比摘要不够，同一份清单装到两个目录时，第二次也该按新目录重新校验并重新授权一次。
3. 摘要或目录任一不一致时（首次启动、换了新安装、或把安装目录搬走/复制到别处）对清单里每个文件做一次大小 + SHA-256 校验，失败点名损坏文件并提示重新运行安装程序；校验通过后做那一次只读授权，再记住摘要与目录。授权被拒则不记标记（下次启动会重试），并停下来说清修复方式（安装到有权限的目录后重跑安装程序）。

上面的第 2 步（只 stat 5 个必需文件、不解压、不哈希 776 MiB、不递归授权）是代码结构上的结论：`prepare()` 里没有任何解压调用，`check_required` 只做 `symlink_metadata` + 长度比较。它由源码和单元测试保证（`runtime::first_start_verifies_everything_then_daily_starts_only_look_at_required_files` 等 15 项），**本环境没有取得该分支的运行证据**——`state\verified` 从未写入成功，程序从未走到 `first=false`，原因见第 7 节。因此这里不给出任何日常启动耗时。

## 4. 卸载

默认卸载只删安装器自己放下的东西：主程序、`resources` 下清单里的文件、卸载器、两个快捷方式、开始菜单文件夹、卸载注册表项，以及指向本次安装的 `Wakuwaku` 开机自启动值。脚本里没有 `RMDir /r`，也不提 `data`、`webview`、`.dsh-fish`——即使有人把程序装进一个已有文件的目录，卸载也不会连带删掉不属于本程序的东西。重新安装后沿用原来的用户数据。

开机自启动的计划任务要单独说：任务名是 `Wakuwaku at logon-<用户 SID>`，SID 只有程序知道，NSIS 拼不出来。所以卸载器在删除主程序**之前**先同步跑一次 `"$INSTDIR\DSH-FISH.exe" --wakuwaku-uninstall-cleanup`，由程序自己用 `logon::xml` 读出那条任务、比对它指向的正是本安装目录里的这个 exe，才 `logon::remove`。清理完（或确认那条任务不属于本安装）就早退，不建端口、不准备运行时、不建窗口——卸载过程绝不会顺手把主程序拉起来。别的安装目录里的另一份 DSH-FISH、别的程序的任务都不动；清理失败也不中断卸载（只记一行，主程序删掉之后那条任务已经启动不了任何东西）。这个入口在 `main.rs` 里排在端口监听、`runtime::prepare()`、detach 与窗口创建之前。

目录清理按真实目录树逐层 `RMDir`（非递归），不是只看清单里的文件路径：Node 发行包里有一套只有目录、没有文件的空层级（`node/node_modules/npm/tap-snapshots/…`），只按清单推导就会在卸载后留下空壳文件夹。空目录不进清单，所以清单仍与「装进去的文件」一一对应。

## 5. 构建

命令从工作区根目录运行，需要 Windows 自带的 `tar.exe`、`expand.exe`、Node.js、Rust 与 MSVC。Cargo 使用已下载依赖，保持 `--locked --offline`。

```powershell
$env:RUSTUP_HOME = Join-Path $PWD '.tools/rustup'
$env:CARGO_HOME  = Join-Path $PWD '.tools/cargo'
$env:PATH = (Join-Path $PWD '.tools/cargo/bin') + ';C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools\VC\Tools\MSVC\14.44.35207\bin\Hostx64\x64;' + $env:PATH
& 'FISH本地模型工作台/Wakuwaku/scripts/build-installer.ps1' `
  -Downloads (Join-Path $PWD '.tools/offline-downloads') `
  -Resources (Join-Path $PWD '.tools/installer-resources') `
  -Node      (Join-Path $PWD '.tools/offline-downloads/node/node-v24.14.0-win-x64/node.exe') `
  -Cargo     (Join-Path $PWD '.tools/cargo/bin/cargo.exe') `
  -Makensis  (Join-Path $PWD '.tools/nsis-3.13/makensis.exe')
```

`-Resources` 指向已准备好的资源目录（含 `manifest.json` 与 `node/webview2/ofm/licenses`）；目录不存在时脚本先用 `scripts/prepare-installer.mjs` 从 `-Downloads` 里的官方下载件现场准备（先核对 `scripts/runtime-sources.json` 里的三条 SHA-256）。输出默认写到 `FISH本地模型工作台/dist/installer/FISH-Dock-Setup-<版本>.exe`。

构建链分三段，各自可单独跑：

| 段 | 命令 | 产物 |
| --- | --- | --- |
| 准备安装资源 | `node scripts/prepare-installer.mjs --downloads <dir> --out <new-dir>` | `<new-dir>/resources/` + `manifest.json` |
| 编译主程序 | `cargo build --locked --offline --release --features installed` | `src-tauri/target/release/wakuwaku.exe` |
| 生成安装包 | `node scripts/installer.mjs --app <exe> --resources <dir> --version <v> --out <setup.exe> --makensis <makensis.exe>` | `FISH-Dock-Setup-<v>.exe` |

当前机器工具链的 release LTO 曾异常退出，构建入口沿用已验证的替代：`CARGO_PROFILE_RELEASE_LTO=false`、`CARGO_PROFILE_RELEASE_CODEGEN_UNITS=16`。

NSIS 3.13 不是系统组件；本工作区把官方 `nsis-3.13.zip`（来自 `tauri-apps/binary-releases`，SHA-256 与 GitHub release digest 一致）解在 `.tools/nsis-3.13/`，构建入口默认从这里取 `makensis.exe`。

## 6. 测试

```powershell
node --experimental-test-isolation=none --test "FISH本地模型工作台/Wakuwaku/scripts/prepare-single-exe.test.mjs"
node --experimental-test-isolation=none --test "FISH本地模型工作台/Wakuwaku/scripts/prepare-installer.test.mjs"
node --experimental-test-isolation=none --test "FISH本地模型工作台/Wakuwaku/scripts/installer.test.mjs"
& ".tools/cargo/bin/cargo.exe" test --locked --offline --manifest-path "FISH本地模型工作台/Wakuwaku/src-tauri/Cargo.toml" runtime::
& ".tools/cargo/bin/cargo.exe" test --locked --offline --features installed --manifest-path "FISH本地模型工作台/Wakuwaku/src-tauri/Cargo.toml" runtime::
```

`--experimental-test-isolation=none` 是沙箱限制：默认的测试进程隔离在 Node 里再起子进程时被拒（`Error: spawn EPERM`，`errno: -4048`，`syscall: 'spawn'`，来自 `node:internal/test_runner/runner`），测试文件根本跑不起来。也可以用环境变量代替命令行开关，这样契约里写的原命令能原样跑通：

```powershell
$env:NODE_OPTIONS = '--experimental-test-isolation=none'
node --test "FISH本地模型工作台/Wakuwaku/scripts/prepare-single-exe.test.mjs"
```

安装器测试里调用 `makensis` 必须用 `stdio: 'inherit'`，带管道同样会被拒。这两条都不影响交互式终端里的正常运行。

`runtime::` 覆盖（15 项）：首次启动全量校验并授权一次、日常启动只查必需文件且不再授权、首次校验点名损坏文件、**同一个 home 换一套安装资源目录时重新全量校验并按新目录重新授权（同目录仍不重复）**、**换目录那次授权被拒时不记标记、下次启动重试**、换版本重新校验而用户数据原样保留、安装资源缺失时提示重新安装而不回落外部依赖、清单不安全或外来时拒绝、拒绝许可则不记住任何东西、home 目录被占时不动它、WebView2 授权参数只读且只针对运行时目录、授权被拒时停下并说清修复方式且不记摘要、许可确认只对同一份许可生效、旧数据复制且新数据优先、安装资源就地使用。

`connection::rules::own_task_tests` 覆盖（6 项）：本安装自己的任务要删、别的安装目录/别的程序的任务不动、同一程序换个写法（大小写与斜杠）仍算自己、不是她写的任务名不碰、任务不存在时什么都不做、没有自己程序路径时（源码树里跑）绝不删任何人的任务。

`installer.test.mjs` 覆盖（8 项）：清单里每个文件都进安装包、可选安装位置与快捷方式与卸载登记、卸载只删自有文件且不递归且不提用户数据、**卸载前先让本安装的程序清登录任务且排在删除主程序之前**、资源里的空目录也要清掉否则卸载留下空壳文件夹、安装时一次性只读授权、清单缺失或混入清单外文件时拒绝、生成的脚本能被离线 makensis 真编译。

## 7. 未完成与限制

两轮各自验到了什么，按轮次分开记：

- **t8 修复轮**（当前产物）：源码、构建入口、端到端真实构建、静默安装与静默卸载。安装后自己放下的文件是 2,199 个（2,196 个资源文件 + `manifest.json` + 主程序 + `uninstall.exe`）；连同安装前就在目录里的 `neighbour.txt`，目录内计 2,200 个文件。卸载后只剩 `neighbour.txt`。
- **t3 验收轮**（上一版安装包）：静默安装 22.42 s、2,199 个文件 / 827,676,854 字节 / 496 个目录（与资源清单逐项一致），静默卸载 0.56 s，用户数据 5 项逐项比对无差异。

**仍未验证**（全部是环境权限所致，不是实现缺陷，也不代表已通过）：

1. 真实 GUI：首次启动在 `icacls /grant` 处失败即退出，Tauri 窗口与模型界面从未建立，没有任何「启动后界面可用」的运行证据。
2. 日常快路径的真实运行（`first=false` → `check_required`）：`state/verified` 在本环境从未写入成功，程序从未走到该分支。该行为目前**只有源码与单元测试证据**（`runtime::first_start_verifies_everything_then_daily_starts_only_look_at_required_files` 等）。
3. 开始菜单 / 桌面快捷方式、「设置 → 应用」里的 HKCU 卸载项：沙箱拒绝写 `HKCU\Software\Microsoft\Windows\CurrentVersion\Uninstall`、`%APPDATA%\Microsoft\Windows\Start Menu` 与桌面，安装阶段就没生成，因此这两项在卸载后「无残留」也不能反证卸载逻辑（脚本内容由 `installer.test.mjs` 静态覆盖）。
4. 真实任务计划下的登录任务归属清理：沙箱拿不到任务计划服务根目录（见末段），只验到「早退、什么都不碰」这一半。
5. 模型 iframe 的真实鼠标导航 / 滚动 / 输入 / 切换页 / 收起再开 / 桌面穿透：界面从未建立，无对象可点。这一项**不属于本安装版任务，且尚未通过**——t5 复核因缺真实鼠标证据判 `needs_revision`，判决未改。
6. 默认安装目录 `%LOCALAPPDATA%\Programs\DSH-FISH` 下的真实安装：该路径写入在本环境被拒。

另一个本环境特有的限制：沙箱里 `icacls /grant` 对任何目录都返回 `Access is denied`（退出码 5），连自己刚建的目录也一样。因此安装时的只读授权会失败、首次启动也会失败——程序此时按设计停下并给出提示（"无法设置内置浏览器的沙箱读取权限：exit code: 5……请把 DSH-FISH 安装到你有权限写入的目录后重试"），而不是继续启动再崩在 WebView2 的 "灾难性故障" 上。真实用户在 `%LOCALAPPDATA%\Programs` 下安装时目录归本人所有，授权应当成功；这一点尚未被任何一次实测确认。

未附 Authenticode 签名，Windows SmartScreen 仍可能提示。固定版 WebView2 不自动获得 Evergreen 更新，安全修补需要重新打包发布安装包。

卸载清理入口在本沙箱里只能验到「早退、什么都不碰」这一半：直接跑 `DSH-FISH.exe --wakuwaku-uninstall-cleanup` 时，`%LOCALAPPDATA%\DSH-FISH` 与 `%LOCALAPPDATA%\DSH-FISH\webview` 都没有被创建（说明它确实没走到运行时准备、端口监听与建窗口），13 ms 内返回；同时 `logon::xml` 在沙箱里连任务计划服务的根目录都拿不到（`the Task Scheduler's root folder: 0x80070003`），于是它按设计打一行 errno 到 stderr、以退出码 1 结束，卸载器记一行后照常删文件。同一原因让 `lhm_setup::tests::this_machine_is_looked_at`（`lhm_setup.rs:2386` 断言 `logon::xml(TASK)` 可读）与 `lhm_setup::tests::a_folder_made_with_her_dacl_is_closed_to_the_user`（`lhm_setup.rs:2373` 删不掉用管理员 DACL 建的目录）在本沙箱里失败——这两项与本次改动无关，属于沙箱限制，需要在不受沙箱约束的会话里复跑。真正「任务指向本安装才删、指向别的安装不删」的行为由 `connection::rules::own_task_tests`（6 项，纯逻辑，不碰真实任务计划）覆盖。
