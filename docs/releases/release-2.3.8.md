# FISH Dock 2.3.8

2.3.8 修复了 Antigravity 在非系统盘或无 8.3 短路径的含空格目录（如 `H:\FISH Dock`）下无法连接的问题，并同步完善了等待状态、URL 空格路径解码、危险命令词边界匹配与提问超时上限。**主岛的外观、布局与动画没有任何改动。**

只发布 Setup 完整安装包，与 2.2.1 起一致。

## 新增与修复

- **支持非系统盘无 8.3 短路径的含空格安装目录：**
  - `connection.rs` 的 `agy_program()` 在 `GetShortPathNameW` 无法取得短路径（如关闭 8.3 短名的非系统盘或 exFAT 分区）时，回退输出 `call "<exe>"`（仅当路径含 `"`、`%` 或换行符时拒绝），已实测 `cmd /c` 下带空格与括号路径可正常保留 stdin/stdout 执行。
  - `i18n.js` 同步修正不支持路径提示中的示例路径为 `D:\FISH-Dock`。
- **完善 Antigravity 审批等待状态、路径解码与命令匹配：**
  - `events_agy.rs` 的 `mood_of()` 补齐 `PermissionRequest` 映射，让需要确认的操作在灵动岛上正确切为 `waiting` 等待态。
  - `events_agy.rs` 的 `norm_path()` 补齐完整百分号解码（`decode_percent`），修复 `file:///h%3A/FISH%20Dock` 等含 `%20` 空格编码的工作区 URI 匹配失败问题。
  - `events_agy.rs` 的 `is_risky_command()` 改为按 `;`、`&`、`|`、换行分段后匹配命令动词与词边界，避免 `echo model`、`confirm`、`--format json` 等安全参数被误判为危险命令。
  - `server.rs` 为 `agent == "agy"` 补齐 `AGY_ASK_MS`（290s）请求等待上限，避免提问/审批长等待超过 `agy` hook 300s 超时窗口。

## 测试

- **JavaScript 测试**：`node --test test/*.test.js test/*.test.mjs` 本机实跑 **508 passed, 0 failed**。
- **Rust 单元测试**：`cargo test --manifest-path src-tauri/Cargo.toml -j 1` 本机实跑 **1036 passed, 0 failed, 24 ignored**。

## 下载

| 安装包 | 适用场景 |
| --- | --- |
| `FISH-Dock-Setup-2.3.8.exe` | 首次安装、修复安装、旧版覆盖升级，含 Node.js |

2.2.1 起只提供 Setup，不再提供 Update 精简包。
