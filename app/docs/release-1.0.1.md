# FISH Dock 1.0.1 更新记录

本版对应 [Issue #1](https://github.com/Fish-under-sea/FISH-Dock/issues/1)，在 1.0.0 基础上增加统一身份目录、完整身份迁移和硬件监控提权窗口修复，并选择性接入 OFM 的功能修复。

## 身份目录与邮件

- 默认将 OFM 账号、凭据、配置、用量、Token 账本及邮件身份保存到 `%USERPROFILE%\.fish-dock`。
- 新目录不存在时，从旧 `%USERPROFILE%\.dsh-fish` 复制已知身份文件，校验后启用；旧目录保留。两个目录同时存在时以新目录为准，不自动合并或双写。
- `FISH_DOCK_DATA` 可指定绝对路径；旧 `DSH_FISH_DATA` 在本版保留兼容并给出弃用提示。单独运行的 OFM 命令行仍保留自己的路径规则。
- 邮件账号从旧桌面配置读取一次，密码按对应账号 ID 从 Windows 凭据管理器读取。本地新邮件密码库使用当前 Windows 用户 DPAPI 加密；旧凭据不删除。缺少密码的账号会标记需要重新授权，不自动连接。
- 桌面布局、宠物、WebView2 数据、LHM 专用凭据以及 DSH 自身的凭据目录保持原有边界。

## 完整身份 ZIP

入口为「通用 → 身份与备份」。导出路径默认为系统「下载」目录，可在设置字段中编辑，保存对话框也可另选位置。

完整身份包包含 OFM 白名单文件、用量与账本、邮件账号及用于迁移的邮件密码。**ZIP 未加密，邮件密码及其他密钥在包内为明文，应按密码文件保管，不能上传 GitHub 或工单。** 本机 DPAPI 密文不会直接跨设备复制；原生导入会在目标 Windows 用户下重新加密。账号 Token 仍可能因服务商策略、设备绑定或过期而需要重新授权。

导入流程提供只读预览、数量及缺失项提示，并要求再次确认。程序暂停自身模型服务与邮件同步，保留替换前副本，按白名单事务替换；启动失败会尝试恢复导入前状态。外部独立 OFM 应先退出。取消对话框不会写入 ZIP，已有文件不会被静默覆盖。

OFM 页面中的专项备份继续只包含 OFM 配置、账号索引、关联凭据和已有 EAC 授权，不包含邮件或调用统计。原生导入兼容该旧格式，并保持邮件身份不变。新专项包附带固定 `README.md`，旧 `README.txt` 包仍可读取。

导入器限制完整包为 32 MiB、最多 128 个文件、单文件最多 16 MiB；旧 OFM 包保留 16 MiB 上限。路径、文件类型、重复名称、压缩比、CRC、清单摘要及数据结构都会校验。备份不包含 DSH 凭据、浏览器资料、日志或程序资源。

## 硬件监控与渠道修复

- 请求 LHM 管理员操作前，显示、聚焦普通桌面设置窗口并临时置顶；结束后恢复原有置顶策略。窗口无法显示时不发起提权。托盘新增硬件监控设置入口，并区分拒绝和等待未完成。不覆盖 Windows 安全桌面。
- 选择性接入 OFM `v2.5.7` 的 CodeArts 专用签名传输、上下文隔离与取消、严格测速、手动结果持久化和探测竞争顺序修复。没有整体合并该版本，不接入广告或上游更新器。

固定上游标签提交为 `d857f8ecb6236608dd97f8f2aa81beacc4994a8d`，功能合并来源为 `761babf6508e2c87ce6a4c86e9a5f98708d785c2`；原始基线和选择范围保存在 [UPSTREAM.json](../../UPSTREAM.json)。

## 验证与限制

- 身份路径、邮件 DPAPI、事务中断恢复、旧 OFM 包保留邮件、恶意 ZIP 拒绝、取消保存和激活失败恢复均使用临时合成数据验证。
- 新身份界面、模型页失效重建、品牌和真实离线 NSIS 编译的定向 JavaScript 回归通过。
- CodeArts 使用本机 TLS／代理与合成密钥夹具，签名传输 12 项、多上下文 RPC 7 项通过；没有访问真实上游账号。
- 严格测速分别在独立 Node 进程执行：单元 9 项、协议集成 11 项、结果竞争 2 项通过。渠道拒绝处理的离线检查 4 项通过。
- 完整 Rust 回归为 473 项：459 通过、2 个既有环境失败、12 忽略。失败来自 LHM ACL 权限拒绝和任务计划根目录 `0x80070003`。
- 完整 JavaScript 回归为 279 项：278 通过、1 个既有用量组件文案断言失败；没有为全绿回退真实 Token 显示。隔离源码的真实 NSIS 编译使用明确路径并通过。
- 管理接口、状态持久化、LAN 和备份回归合计 7 项通过；渠道请求与错误处理检查 4 项通过。实际 Edge 在 1440×1100 与 390×844 下验证当前页面交互，无横向溢出、无 JavaScript 异常；请求使用内存替身，不代表真实账号结果。
- 封装自带 Node 在空 `PATH`、`NODE_PATH`、`NODE_OPTIONS` 下校验 2,202 个资源文件，页面、管理鉴权、LAN 默认关闭、OFM 专项 ZIP、固定 Markdown及管理状态持久化通过。没有读取真实身份资料。
- 本轮未覆盖真实用户安装、原生导入导出对话框实操、实际 UAC、安全桌面、跨设备 DPAPI／邮件联调或真实渠道风控结果。安装包仍未签名。

最终产物摘要与封装冒烟结果见 [安装版构建记录](installer-build.md)。升级前从托盘完全退出旧版，并保管好已有备份。

---

## English

FISH Dock 1.0.1 addresses [Issue #1](https://github.com/Fish-under-sea/FISH-Dock/issues/1).

- OFM identity, usage and mail identity now share `%USERPROFILE%\.fish-dock`. Migration copies and verifies the allowlisted legacy `.dsh-fish` files and retains the originals. An existing new directory takes precedence; there is no automatic merge.
- Mail secrets use current-user DPAPI at rest. Legacy Windows credentials remain. Missing secrets require reauthorization. Desktop layout, pets, WebView data, LHM credentials and DSH's own vault remain separate.
- General → Identity and backup offers native export/import. The editable default destination comes from the system Downloads folder. Preview and explicit confirmation precede replacement; prior files are retained for recovery. The plaintext ZIP includes OFM usage/ledger and mail secrets, and must never be uploaded. Import encrypts mail secrets for the destination user; provider tokens may still require reauthorization.
- Legacy OFM-only ZIPs remain supported without changing mail identity. New OFM-only exports contain a fixed Markdown migration guide and exclude mail and usage history.
- LHM elevation receives a visible, focused settings owner, a tray entry and clearer declined/unfinished results. This does not draw over the Windows secure desktop.
- Selected OFM v2.5.7 signed-transport and strict model-test fixes were integrated from the pinned sources above, without a full version merge, ads or upstream updater.

Validation uses synthetic identity data, local TLS/proxy fixtures and isolated Node processes. The final full Rust run is 459 passed, 2 existing environment failures and 12 ignored out of 473; JavaScript is 278 passed and 1 existing usage-copy assertion failure out of 279. Management-state/HTTP and LAN/backup regressions pass all 7 checks, with 4 separate channel adapter checks. Isolated Edge exercised the real built UI at 1440×1100 and 390×844 with local request substitutes, no horizontal overflow and no JavaScript exceptions. Bundled Node verified all 2,202 resources and the packaged service with empty dependency-search environment variables. Native dialogs, real UAC, cross-machine mail/DPAPI migration and live account behavior were not exercised. The installer is unsigned; see the build record for final fingerprints.
