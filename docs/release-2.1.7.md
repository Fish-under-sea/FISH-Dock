# FISH Dock 2.1.7

**更新包重出**：2.1.0 到 2.1.6 的用户点「应用内更新」都能一次升上来，包体仍然是精简差分。

## 为什么有这一版

2.1.6 的更新包以 **2.1.5** 为基线，只装了「2.1.5 → 2.1.6」的 10 个文件。2.1.0 / 2.1.1 的安装目录里，`resources/licenses/runtime-sources.json` 还是 #73 之前那份（含 Windhawk 条目），摘要与 2.1.6 不同。安装器的校验顺序是：

```text
场景里的文件与清单摘要一致 → 复用，不下载
不一致 → 必须在更新包的载荷里，否则报「精简更新包所需基础资源缺失」并回滚
```

于是 2.1.0 / 2.1.1 的用户会看到：

```text
Installation update failed: ?resources/licenses/runtime-sources.json
```

中文被 NSIS 的 OEM 代码页过滤成了问号，但失败点就是那个文件。

## 修法：更新包基线改到 2.1.0

不是改安装器逻辑，而是把差分的起点前移到**最老的受支持版本**：

| 版本 | 与 2.1.7 的差异 |
| --- | --- |
| 2.1.0 / 2.1.1 | `FISH-Dock.exe`、`resources/manifest.json`、`resources/licenses/runtime-sources.json` |
| 2.1.2 – 2.1.6 | 只有前两个（`runtime-sources.json` 已与 2.1.7 一致，现场文件直接复用） |

实测 2.1.0 → 2.1.7 的差分只有 **3 个文件 / 16.4 MB**，其中主程序 16.0 MB、资源清单 0.4 MB、许可来源记录 1.3 KB。更新包因此仍是 29.7 MB 这个量级，没有为了兼容旧版本把包撑大。

- 基线清单入库：`app/scripts/baselines/install-manifest-2.1.0.json`（2.1.0 的归属清单，去掉已下线的 `resources/windhawk/` 8413 条）。
- `installer.test.mjs` 加了一条自检：这份基线必须是 2.1.0、必须含 `runtime-sources.json`、不得含 windhawk 路径。
- 构建命令里的 `-Baseline` 一律指向它，不再随版本滚动。

## 顺手修掉的三处宿主并发问题

这一版对 2.1.6 的回调线程做了一次专项复审，三条会把「忙」放大成「坏」的路径一起修：

1. **回调队列无界**：插件狂发回调时宿主持续涨内存（每帧上限 1 MB）。改成 32 帧的有界队列 + 阻塞发送，背压回插件的 stdout，最坏 32 MB/插件。
2. **迟到回执被当成下一条的回答**：一次 15 秒超时之后，编号串位会让下一条请求报「ID 不匹配」，插件被当成坏实例杀掉、之后每条请求都错一帧。现在 `request` 丢掉不属于自己编号的帧继续等。
3. **停用前先过一次 2 秒探针**：插件正在跑慢命令时探针会超时，`stop()` 因此跳过 `plugin.deactivate` 直接 kill。clash 的系统代理租约就留在注册表里指向已经死掉的端口——用户看到的是整机断网，要等下次 Dock 启动才自愈。现在只要进程还活着就发告别。

## 验证

- 用 2.1.0 的真实归属清单算出 payload：`FISH-Dock.exe`、`resources/manifest.json`、`resources/licenses/runtime-sources.json` 三项，全部落在包里。
- `install-update.cjs` 的升级用例在合成目录上重跑（2.1.0 的三项旧摘要 + 2.1.7 清单），校验、备份、替换、回滚四条路径都通过。
- 两包 `7z t` 通过，`<exe>.sha256` 与实际字节一致，包内 `resources/manifest.json` 为 `FISH Dock / 2.1.7`。

## 已知限制

- 2.0.x 的安装目录结构与归属清单跨越了改名与 Windhawk 下线，仍然只用 Setup 覆盖安装；更新包只承诺 2.1.0 及以上。
- 便携版入口仍无载荷（发行不提供便携 exe），安装版不受影响。
- 插件侧的自启写法（准备阶段进重试、`view()` 只读一次）仍在插件仓 [#45](https://github.com/Fish-under-sea/FISH-DOCK-Plugins/issues/45) 跟踪。
