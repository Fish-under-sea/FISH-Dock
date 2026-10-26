# FISH Dock 1.1.0 源码与发行范围

本仓库维护桌面宿主与独立插件平台，不使用子模块或本机目录链接：

- `Wakuwaku/`：桌面窗口、审批与邮件、插件管理、统一存储、GitHub 设备登录、全屏恢复、Windhawk、安装器和测试。
- `sdk/`：Node 后台协议和浏览器桥接 SDK。
- `licenses/`：从旧模型服务副本保留的许可证；版权归属不因移除冗余代码而改变。
- `UPSTREAM.json`：原始基础提交、选择性移植范围和许可证来源。

飞书、Clash、Magpie、OFM 等业务插件在 [FISH-DOCK-Plugins](https://github.com/Fish-under-sea/FISH-DOCK-Plugins) 独立维护。主仓已移除废弃的 `dsh-our-free-model/` 副本、旧 OFM 暂存脚本和未编译的 OFM 宿主模块。现有身份导入兼容、邮件与 SDK 继续保留。

不纳入 Git 的内容：真实账号与邮件、API Key、Cookies、身份 ZIP、日志、运行状态、开发依赖、工具链缓存、官方下载运行时和生成的安装器。安装器通过 GitHub Release 单独分发。必要的合成协议样本和原始许可证保留。

GitHub 登录使用公开 OAuth Client ID 和设备授权流程；宿主加密保存令牌，不向插件传递。公开 Client ID 属于应用标识，不是客户端密钥。自建应用可通过 `FISH_DOCK_GITHUB_CLIENT_ID` 配置。模型渠道的授权配置在对应业务插件仓维护。

[构建方法](README.md#development) · [1.1.0 更新记录](Wakuwaku/docs/release-1.1.0.md)。
