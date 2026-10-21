# 源码快照说明

本仓库发布 FISH Dock 1.0.1 的双项目集成源码，不是上游未修改镜像，也不冒充官方发行版。两份源码放在同一级目录，避免只有桌面代码而缺少实际 OFM 集成。

- `Wakuwaku/`：桌面、原生窗口、模型工作台嵌入、安装器及其测试。
- `dsh-our-free-model/`：独立模型服务、React 管理页面、同端口 LAN 与迁移备份实现及测试。
- `UPSTREAM.json`：记录上游仓库、原始基础提交和选择性修复范围。原始版权及第三方许可保留在各自目录。
- 根目录中英文 README：展示页副本，已调整相对链接；子项目 README 同时保留。

安装器的官方运行时下载件、编译缓存、开发依赖、安装包、用户账号数据和密钥不放入 Git。构建步骤见 [中文首页](README.md#从源码构建) 和 [完整构建记录](Wakuwaku/docs/installer-build.md)。依赖版本锁定在源码中；需要 Windows 构建工具链和对应缓存。

源码测试保留必要的合成样本，不使用真实用户凭据。上游的自动部署／签名／发布工作流没有启用，防止本仓库推送或打标签后误连原作者基础设施。标签本身不代表 GitHub Release 或安装包上传。

## 公开源码的 OAuth 配置

本发布副本移除了上游渠道包的硬编码 Google OAuth 客户端 ID 和 Secret，保留 `CMDC_PAK_GOOGLE_CLIENT_ID`、`CMDC_PAK_GOOGLE_CLIENT_SECRET` 环境变量。三份源码的脱敏及配置读取回归通过 3 项，见 `tools/oauth-publication.test.mjs`。首次提交不继承此前被 GitHub 拦截的本地历史，也未绕过推送保护。1.0.1 安装包由本发布源码重新构建；相关 OAuth 配置需自行提供。源码回归不代表真实账号联调通过。

## 已知验证限制

- 最终桌面 JavaScript 与安装脚本：279 项，278 通过、1 个既有用量文案断言失败。
- Rust 全量：473 项，459 通过、2 个 Windows 沙箱环境失败、12 忽略。
- LAN、OFM-only ZIP、合成账号恢复和真实 Edge 的 DOM 交互通过；未证明原生系统鼠标、两台物理设备之间的 LAN 或真实上游账号联调。
- 安装器未 Authenticode 签名，素材授权不自动适用源码 MIT 许可。

详细范围及固定上游链接见 [1.0.1 版本说明](Wakuwaku/docs/release-1.0.1.md)。
