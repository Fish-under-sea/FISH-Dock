# FISH Dock 2.1.3

2.1.2 的紧急修复版。

## 为什么有这一版

**2.1.2 装完启动不了**：弹「安装资源清单无法解析：missing field `node_version`」。原因是那一版重算 `resources/manifest.json` 时绕过了 `prepare-installer.mjs` 的 `writeManifest`、手写了一份 JSON，漏掉 `node_version` 与 `webview2_version` 两个**必填**字段——`runtime.rs` 的 `Manifest` 反序列化在启动第一步就失败，Setup 包与 Update 包都中招。

2.1.3 的清单由 `writeManifest` 生成，七个字段齐全；并加了两条回归测试锁住这件事：Rust 侧钉「缺 `node_version` 必须报出字段名」，JS 侧钉「清单键集合固定为七项」。

**已经装了 2.1.2 的机器起不来，应用内更新也进不去，请手动下载 2.1.3 的完整安装包覆盖安装。** 账号与数据不受影响（数据在 `%LOCALAPPDATA%\FISH Dock`，安装器不碰）。

## 这一版改了什么

### 修复 2.1.2 的启动失败

- `resources/manifest.json` 重新由 `writeManifest` 生成：`schema 2 / product FISH Dock / version 2.1.3 / architecture x86_64 / node_version 24.14.0 / webview2_version 155.0.4283.62 / files 2100`。
- 回归测试两条（见上），都在 `cargo test` 与 `node --test` 里跑。

### 清扫旧版留在用户机器上的东西（#73 的收尾）

2.1.2 只做到了「不再随包分发 windhawk」，**没有清用户机器上已经落地的旧文件**。这一版补上：

- 启动时一次性清掉旧内置引擎在数据根的那份：`%LOCALAPPDATA%\FISH Dock\config\windhawk\` 下的 `windhawk.exe`、`windhawk-x64-helper.exe` 与 `windhawk.ini`（ini 里的 `EnginePath` / `CompilerPath` 指向已经不存在的安装资源，留着就是悬空配置）。
  **`config/windhawk/data/` 原样保留**——那是你导入过的 mod 源码与配置。
- 指向内置目录的 `config/windhawk.json`（外观页「已选择的路径」）一并作废，避免状态读取去打开一个不存在的 layout。
- 幂等：引擎进程还占着文件时删不掉，退出后下次启动会再清一次，不影响启动。

### 清掉历史遗留的 `FISH-Dock-2.0.1-run.exe`

2.0.1 的便携启动器一直留在安装目录里没被清过。这一版把它加进 `LEGACY_EXE_NAMES`，安装与卸载都会按旧名删除（与 `DSH-FISH.exe`、`wakuwaku.exe`、`wakuwaku-sensors.exe` 同样处理），指向它的自启动项也一并按归属清掉。

## 包与校验

- `FISH-Dock-Setup-2.1.3.exe`（完整安装包，**2.1.2 起不来就用这个**）
- `FISH-Dock-Update-2.1.3.exe`（精简差分包，**只对 2.1.2 有效**；从 2.1.2 起不来更新，请用 Setup）
- 各自附 `.sha256`；`SHA256SUMS.txt` 列出全部。

## 已知限制

- 2.1.2 的用户必须手动装 Setup；应用内更新在 2.1.2 上跑不起来。
- 便携版入口仍无载荷（发行不提供便携 exe），安装版不受影响。
