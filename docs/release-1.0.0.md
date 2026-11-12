# FISH Dock 1.0.0

日期：2026-10-20。面向 Windows x64 的二次开发集成版，源自 Wakuwaku 与 dsh-our-free-model，不是两者的官方发行版。此说明可作为本项目 GitHub Release 的正文基础；本次只准备本地材料，没有推送或发布。

## 本版内容

### OFM 同端口局域网访问

- 入口：模型工作台「API 接入 → 局域网访问」。默认关闭，开关和独立 LAN Key 持久保存。
- 本机管理继续只监听环回地址；推理中继绑定当前 IPv4 网卡地址，使用同一个端口号。不是把整个管理监听改为 `0.0.0.0`。
- 页面列出成功绑定的地址，支持独立查看、复制和轮换 LAN Key。旧 LAN Key 在轮换后立即失效，本机 Key 不能用于 LAN。
- 只提供推理相关 API，不开放管理页面、渠道管理、账号备份或 Web 资源。
- 地址缺失或端口冲突时明确报错，不选择随机备用端口；多地址绑定中途失败会关闭已创建的中继，保留本机管理可用。
- 不自动修改防火墙、路由器或公网映射。HTTP 仅适合可信局域网；网卡变化后关闭再开启以重新绑定。

### 仅 OFM 的迁移 ZIP

- 入口：模型工作台「服务设置 → OFM 迁移备份」。下载前明确确认明文凭据风险。
- 导出 OFM 设置、账号索引、关联凭据、模型禁用状态和永久锁定状态，以及已有 EAC 用户授权。
- 固定 `README.txt` 说明目标目录、退出程序、备份目标、按路径替换、不可合并与恢复后检查事项。没有自动导入功能。
- 不导出 DSH 凭据库、邮件、聊天、桌面、浏览器、调用统计或日志。管理与导出仅在本机提供，不经过 LAN 中继。
- 只从固定白名单读文件，拒绝符号链接与客户端路径参数，限制大小与并发。ZIP 在内存生成，不在服务端留下明文 ZIP。
- `manifest.json` 提供各文件摘要，不代表数字签名或加密。备份必须视同密码文件保管；不要上传到 GitHub。

### 选择性跟进上游

本版不是直接合并上游 HEAD，而是在已有桌面集成、原图标、独立运行和数据兼容补丁之上应用功能修改。

| 来源 | 固定范围／提交 | 集成内容 |
| --- | --- | --- |
| [Wakuwaku](https://github.com/zhoupengjie/Wakuwaku/compare/f1441cbbcf6801d18c27a1d241fd4e5475086e3b...896f6ba29f98c1c310a5a7c137c3881af5078d66) | 0.3.3 基础至 0.3.4 功能范围 | 网络代理、Codex、Gmail 和任务栏冲突处理；保留本发行版更新及隐私边界 |
| [Codex 后台 hooks](https://github.com/zhoupengjie/Wakuwaku/commit/8da3f62e36d231448c9b2877d40c16acfa4223d5) | 0.148 起的后台 hooks 能力 | 减少等待，旧客户端保留前台兼容；停止与中断仍在前台 |
| [Codex 信任读取](https://github.com/zhoupengjie/Wakuwaku/commit/821569805a6970e2d4a1832f28f1588573058827) | 读取自身配置中的已信任记录 | 重启后不凭「本次还没收到事件」重复要求信任，不改写 Codex 信任记录 |
| [任务栏冲突回退](https://github.com/zhoupengjie/Wakuwaku/commit/549adb1cb4ac5897e8651e3a304718abc0e3b540) | 10 秒滑动窗口内 8 次争抢 | 回退并保存灵动岛模式，减少与第三方任务栏工具反复争抢 |
| [网络代理](https://github.com/zhoupengjie/Wakuwaku/commit/1d8a329299821ee5cc2cf51ffca0398a35af9aba) | 跟随系统／手动 HTTP 或 SOCKS5／不用 | 设置及连通性测试；不宣称覆盖 OFM 自身全部渠道 |
| [Gmail 提示](https://github.com/zhoupengjie/Wakuwaku/commit/c52bc898dd7e23efe93bca5a9beea2d67d5dafb7) | 授权步骤与错误说明 | 应用专用密码、账号对应的 Google 页面和登录失败分类 |
| [OFM 异常 Token 修复](https://github.com/Ebony-Vinyl/dsh-our-free-model/commit/f7a02f15bb3375b605f6c13bcf05a2de3d589afd) | 选择性应用流与适配器的计量补丁 | 限定 `kilo-auto/free`，保留有效输出与推理计量，不将超窗口异常输入伪装成精确总量 |

未引入 OFM 新广告、赞助弹窗、群二维码或其插件发行号。本产品版本为 **FISH Dock 1.0.0**，不等于 OFM 插件版本。没有恢复原作者更新下载、崩溃上传或替换依赖的真实认证网址。

## 保持兼容

- 用户原始蓝发角色图标保持字节不变；安装器与卸载器继续使用该图标。
- 新安装默认 `%LOCALAPPDATA%\Programs\FISH Dock`，升级仍沿用登记位置。安装时展开依赖，日常不重复解压整个运行时。
- 内部 `DSH-FISH.exe`、应用标识、卸载登记及 `%LOCALAPPDATA%\DSH-FISH`、`%USERPROFILE%\.dsh-fish` 数据路径不变。
- 保留任务栏／灵动岛恢复、屏幕回退、粒子启动和减少动态效果支持。关于页与中英文首页说明来源与新增能力。
- 卸载保留用户数据，仅删除自己的文件与已确认属于本安装的自启动任务。

## 本轮验证

| 范围 | 结果与边界 |
| --- | --- |
| 桌面完整 Rust 测试 | 448 项：434 通过、2 失败、12 忽略；失败仍为 `lhm_setup` 的沙箱 ACL 拒绝与任务计划根目录 `0x80070003`，不是全绿 |
| 桌面前端及品牌／安装器回归 | 最终 275 项：274 通过、1 个既有用量文案断言失败；未回退已有用量组件以迎合旧断言 |
| 同端口 LAN | 替身网卡与本机 HTTP 实测：独立密钥、仅推理、持久开关、轮换、关闭及占用失败；未用两台物理设备验证可达性 |
| OFM 迁移 | 鉴权、同源、显式确认和白名单检查；替身账号导出后在新目录启动，验证凭据、账号、模型禁用与永久锁定状态可恢复 |
| 异常计量 | 本地 SSE 替身单独进程通过；该测试用导入时环境变量，不能与提前导入适配器的测试共用模块缓存，已增加防出网断言 |
| 浏览器交互 | 真实 Edge、实际构建页面、本机替身服务；1440×1100 与 390×844 布局、同端口地址、查看／轮换密钥、ZIP 下载通过，未捕获脚本异常 |
| 资源封装 | 缺失的备份模块、固定说明和 fflate 许可已列入打包白名单；封装服务导入检查通过 |

浏览器验证使用的是 DOM 驱动与隔离数据，不能冒充桌面原生窗口的系统鼠标验证、真实邮件或付费模型联调。没有覆盖用户现有安装；本版也没有真实启动速度数字。最终安装包大小、摘要和构建结果见 [构建记录](installer-build.md)。

## 安装与已知限制

先从托盘完全退出旧版，再运行 `FISH-Dock-Setup-1.0.0.exe`。覆盖安装沿用原位置和数据，不要在旧版占用 WebView2 DLL 时忽略写入错误。

本版仍未 Authenticode 签名，不保证消除 SmartScreen。原图标及宠物资源不自动适用代码的 MIT 许可，公开再分发前需确认素材授权。账号可能需要重新登录，迁移不转移设备绑定、授权资格或防火墙规则。

---

## English Summary

FISH Dock 1.0.0 is a derivative integration of Wakuwaku and dsh-our-free-model, not an official upstream release. It adds default-off, persistent, same-port-number LAN inference with an independent key, while keeping management local. LAN binding failures are explicit; no fallback port, firewall rule, public exposure, or advertisement is added.

The confirmed, unencrypted OFM-only ZIP includes settings, account indexes and associated credentials, existing EAC authorization, hashes, and fixed restore instructions. It excludes DSH's credential vault, mail, chats, layouts, browser profiles, statistics, and logs. Treat the ZIP as a password file. Restore is manual replacement, not automatic import or merging.

Selected upstream fixes cover proxy controls, taskbar-conflict fallback, Codex trust/background hooks, Gmail guidance, and inflated `kilo-auto/free` multimodal usage. Original icons, saved docking preferences, install-once resources, and legacy data paths remain. The installer is unsigned.

Verification is bounded: Rust 434 passed / 2 environment failures / 12 ignored; the final desktop frontend and installer run had 274 passed and one existing wording mismatch (275 total). Loopback integration, synthetic-account restore, and real Edge desktop/mobile DOM-driven interactions and ZIP download passed. This is not proof of native system-mouse handling, cross-device LAN reachability, live provider authorization, or new startup performance. Nothing has been pushed or published.
