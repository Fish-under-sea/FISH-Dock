# 本地端口接受网页请求（Origin / Host 未校验）：问题整理、证据与修复设计

> **2.3.4 状态：已修复并验证**（SEC-1/2/3）。目标版本以 2.3.4 为准；实现为 `server.rs` 的 `turned_away`/`is_page_origin`/`is_local_host`，原始 TCP 复验见 evidence/verify-a。
>
> 状态：已复现（外部测试机，FISH Dock 2.2.3 / `0626a19`），**修复尚未实施**。
> 目标版本：2.3.0 的安全项；若 2.3.0 延期，可把本项单独切成 2.2.4 先发。
> 上游：`e143563e`（首次随 v0.4.3 发布）修的是同一段代码。复现包写的「v0.4.2 已修」不准确，见第八节。
> 本文按仓库 issue 模板的顺序组织，可直接作为 issue 正文；模板填不了的侧边栏六项见第十二节。

## 一、结论

FISH Dock 的本地 HTTP 端口只绑 `127.0.0.1`，但除 `/v1/*` 外**不校验 `Origin`，也不校验 `Host`**。任意一个打开着的网页都能用「简单跨源请求」（`POST` + `text/plain`，不触发 CORS 预检）让它退出、弹出伪造的审批、注入会话与小组件；通过 DNS rebinding 还能读到 `/health` 的回复。开着 `FISH_DOCK_DEBUG=1` 时，网页还能调 `/debug/eval` 在页面里执行代码。

| 项 | 内容 |
| --- | --- |
| 受影响版本 | 按代码看 1.0.0 起（复现包口径）；本轮只实测 2.2.3（`main` 的 `0626a19`），其余版本未逐版核对 |
| 代码位置 | [`app/src-tauri/src/server.rs:108`](../../app/src-tauri/src/server.rs#L108) 的 `serve()` 除 `GET /v1/` 外把每个请求直接交给 `handle()`；`/v1/*` 在 [`agent_hub.rs:782`](../../app/src-tauri/src/agent_hub.rs#L782) 里有自己的 Host / Origin 校验（`sessions` / `events` 另有 Bearer） |
| 触发条件 | 默认配置即可触发退出、伪造审批、注入状态与小组件；代码执行需要 `FISH_DOCK_DEBUG=1`（默认关） |
| 严重性 | 默认配置：中（本地、无需账号、可静默退出与伪造提示）；调试配置：高（页面上下文可调宿主命令，等同代码执行） |
| 修复 | 照上游 `e143563e` 在 `serve()` 里、`handle()` 之前拒绝网页请求；`/v1/*` 保持自己的校验 |

严重性不套用 CVSS 数字：默认配置下没有直接的文件读写或凭据泄露，影响集中在拒绝服务、伪造提示与状态污染；调试模式的影响取决于当事人是否主动打开排障开关。结论是**按安全缺陷处理，尽快随 2.3.0 发布**，但不必按「默认配置远程代码执行」宣传。

## 二、网页能触发的接口

`handle()` 的路由在 [`server.rs:190`](../../app/src-tauri/src/server.rs#L190)，以下接口都没有来源校验：

| 接口 | 网页能做什么 | 前提 | 说明 |
| --- | --- | --- | --- |
| `POST /hook` | 伪造任意 agent 事件；`PermissionRequest` 会让岛上出现一条**内容由网页决定的审批** | 无 | 正文任意 JSON；`read_json` 不看 `Content-Type` |
| `POST /state` | 注入或伪造会话状态 | 无 | 走 `Shared::apply` |
| `POST /widget` | 往岛上推插件条（文字、图标、颜色可控） | 无 | 最多 32 条，字段有校验，但内容可自定 |
| `POST /come-home` | 让她重新出现 | 无 | 对应「再次启动」流程 |
| `POST /after-update` | 伪造「新版已起来」 | 无 | 只登记 `pid` / `version`，见 [`updates.rs:1430`](../../app/src-tauri/src/updates.rs#L1430)；破坏更新交接的判断，不等于直接结束进程 |
| `POST /quit` | 让她退出（任务栏模式会交还任务栏） | 无 | `?wake=1` 时之后会话还能把她拉起 |
| `GET /health` | 通过 DNS rebinding 读到会话状态；调试模式下还有窗口矩形与整份设置 | 需要伪造 `Host` 且让浏览器把域名解析到本机 | 会话名、项目、状态可读 |
| `POST /debug/eval` | 在她的页面里执行任意表达式 | `FISH_DOCK_DEBUG=1` | 页面能调全部 Tauri 命令（`withGlobalTauri` 为真、CSP 为 `null`） |
| `POST /hw` | 不受影响 | 需要传感器助手留下的 token | 已有校验，见 [`server.rs:365`](../../app/src-tauri/src/server.rs#L365) |

`/v1/*` 不在上表：它对所有路径先做 Host / Origin 校验（只接受本机同端口来源），`/v1/sessions` 与 `/v1/events` 另有 Bearer 校验（[`agent_hub.rs:856`](../../app/src-tauri/src/agent_hub.rs#L856)）。`/v1` 的修复不需要动。

## 三、为什么浏览器自带的缓解不够

较新的 Chromium 内核浏览器有 Local Network Access 检查：被当作公网网站的页面访问本机地址时会被拦下或弹窗询问。复现包的 C3 实测确认这层检查有效，但以下情况不受它保护：

- 本机 `file://` 打开的 HTML（邮件附件、下载下来的网页），见 C5；
- 本机上其他端口的页面（开发服务器、其他本地应用的网页），见 C4；
- 用户在弹窗里点了允许的网站；
- 没有这项检查的浏览器或版本。

所以服务端自己拒绝网页请求才是与浏览器无关的边界。上游的修复也基于同一判断。

## 四、复现证据

复现包（`fish-dock-port-repro.zip`）内容：

| 文件 | 作用 |
| --- | --- |
| `README.md` | 问题、接口、复现步骤、参考修复 |
| `run-repro.ps1` | 隔离数据根、两次启动、C1–C6、清理计划任务与通知登记 |
| `evil.html` | 模拟其他来源的网页，只发简单跨源请求 |
| `serve.mjs` / `seed-config.mjs` | 托管测试页；写入岛模式、跳过引导、关闭更新检查 |
| `results/winbuild-2026-11-14.txt` | 实测输出（SID 已替换为 `<SID>`） |

实测环境：Windows NT 10.0.26300（SSH、session 0、没有桌面），Edge 157.0.4341.46，Node 26.13.1；`FISH-Dock.exe` 由 `0626a19` 以 release 编译。

| 编号 | 做了什么 | 结果 |
| --- | --- | --- |
| C1 | `curl.exe` 用 `Origin: https://evil.example`、`Content-Type: text/plain` 发 `POST /widget` | **VULNERABLE**：200 |
| C2 | `curl.exe` 用 `Host: evil.example` 读 `GET /health` | **VULNERABLE**：200 |
| C3 | 无头 Edge 把测试页当作公网网站（Local Network Access 生效） | not reproduced：浏览器拦下，会话数 0 → 0 |
| C4 | 无头 Edge 打开本机另一端口（`http://127.0.0.1:8099`）的测试页 | **VULNERABLE**：`/widget` 送达，`/hook` 挂起等待回答；会话 0 → 1，最新为伪造的 `repro-web-local` |
| C5 | 无头 Edge 以 `file://` 打开测试页（`Origin: null`） | **VULNERABLE**：会话 1 → 2，最新为伪造的 `repro-web-file` |
| C6 | 本机测试页 `POST /quit` | **VULNERABLE**：FISH Dock 以退出码 0 退出，日志 `quitting (POST /quit), by choice` |

证据的使用边界：

- C4、C5 的「伪造审批」在测试机上确实挂起等待回答，说明 `PermissionRequest` 进入了面板路径；测试机没有桌面会话，画面本身未被观察。
- `/debug/eval` **没有实测**；复现包只按代码判断。本文把它列为受影响项，依据是 [`server.rs:373`](../../app/src-tauri/src/server.rs#L373) 的路由与页面能力，不把「未测」写成「已测」。
- 本轮没有在本机重跑复现包：它需要一个已编译的 2.2.3 exe，会短暂接管任务栏，并改写计划任务与通知登记。复现包的输出与代码审读已足够定位；修复后按第九节重跑。
- 复现包里的数据全部是合成的，不含令牌、Cookie、账号或真实日志。

## 五、根因

1. [`serve()`](../../app/src-tauri/src/server.rs#L108) 的循环在 `spawn_named` 里逐条取请求，直接 `handle(&sh, req, is_debug)`，中间没有任何来源判断。
2. `handle()` 只看「方法 + 路径」（[`server.rs:190`](../../app/src-tauri/src/server.rs#L190)），`/hook`、`/state`、`/widget`、`/quit` 都不需要令牌或来源证明。
3. `/v1/*` 的校验在 `agent_hub::handle` 内部（[`agent_hub.rs:856`](../../app/src-tauri/src/agent_hub.rs#L856)），与端口上的其他路径无关；`serve()` 只对 `GET /v1/` 做了分流。
4. 浏览器把 `POST + Content-Type: text/plain` 当「简单请求」，不预检就把请求送到服务端；页面读不到回复（CORS），但请求已经执行。`read_json` 不检查 `Content-Type`，所以 JSON 正文照样能解析。
5. `GET /health` 不带 `Origin`，同源 GET 本来就不带；要让网页读到回复，需要 DNS rebinding 把攻击者域名解析到 `127.0.0.1`，而服务端不看 `Host`，于是同源成立。
6. 只绑 `127.0.0.1` 只挡住局域网，不挡本机浏览器里的页面。

## 六、修复设计（照上游 `e143563e`）

### 6.1 判定规则

在 `serve()` 里、`handle()` 之前判断；`/v1` 与 `/v1/*` 跳过，交给 `agent_hub` 自己的校验。

| 请求 | 判定 |
| --- | --- |
| `Origin` 是 `http:`、`https:`、`file:` 开头，或等于 `null`（不区分大小写，允许前后空白） | 403 |
| `Host` 存在，且去掉可选端口（只认纯数字端口）后不是 `127.0.0.1` / `localhost` / `[::1]` | 403 |
| 没有 `Origin`，且没有 `Host` 或 `Host` 是本机 | 照常处理 |
| `Origin` 是 `moz-extension://` 等扩展来源 | 照常处理 |
| `/v1` 或 `/v1/*` | 跳过，交给 `agent_hub` |

不带 `Origin` 的客户端（Claude Code 的 axios HTTP hook、Node 的 `fetch` / `http`、`curl`、PowerShell 5.1 / 7、她自己的 `ureq` 转发）都照常工作；Thunderbird 扩展的 `moz-extension://` 也照常。正则与端口细节按上游的写法，不额外发明规则。

### 6.2 代码落点

我们的 `server.rs` 目前没有 `header` 与 `is_local_host`（`agent_hub.rs` 有一份私有的 `header`，但 `host_ok` 的语义是「精确端口」，不适合直接复用）。按上游的做法在 `server.rs` 里加私有助手：

```rust
// 请求头，有就取第一个。
fn header<'a>(req: &'a Request, name: &'static str) -> Option<&'a str> {
    req.headers().iter().find(|h| h.field.equiv(name)).map(|h| h.value.as_str())
}

// Host 是否指向本机（后面可以带一个纯数字端口）。
fn is_local_host(host: &str) -> bool {
    let name = host.rsplit_once(':').filter(|(_, port)| !port.is_empty() && port.bytes().all(|b| b.is_ascii_digit())).map_or(host, |(name, _)| name);
    matches!(name.to_ascii_lowercase().as_str(), "127.0.0.1" | "localhost" | "[::1]")
}

// 网页发来的 Origin：站点的、文件的，或不透明来源（"null"）。
fn is_page_origin(origin: &str) -> bool {
    let origin = origin.trim().to_ascii_lowercase();
    origin == "null" || ["http:", "https:", "file:"].iter().any(|scheme| origin.starts_with(scheme))
}

// 网页的请求：返回写进日志的理由；该服务的返回 None。
fn turned_away(req: &Request) -> Option<String> {
    let path = req.url().split('?').next().unwrap_or("");
    if path == "/v1" || path.starts_with("/v1/") {
        return None;
    }
    let (host, origin) = (header(req, "Host"), header(req, "Origin"));
    let from = match (origin, host) {
        (Some(origin), _) if is_page_origin(origin) => format!("the web page {origin}"),
        (_, Some(host)) if !is_local_host(host) => format!("a page by the name {host}"),
        _ => return None,
    };
    Some(format!("{} {path} from {from}", req.method().as_str()))
}
```

`serve()` 的循环改为：

```rust
crate::spawn_named("server", move || {
    // 每种被拒的请求只记一次，最多 8 种：开着的网页可以每秒发一次。
    let mut refused = Vec::new();
    for req in server.incoming_requests() {
        if let Some(why) = turned_away(&req) {
            if first_time(&mut refused, &why) {
                sh.log(&format!("http: {why}, turned away (a web page's)"));
            }
            reply(req, 403, json!({ "error": "not for web pages" }));
            continue;
        }
        handle(&sh, req, is_debug);
    }
});
```

要点：

- **在读正文之前拒绝**。`reply` 直接回 403，不调用 `read_json`；被拒的请求体没读完时，同一条连接上的下一个请求仍要正常工作（上游用 `tiny_http` 的原始请求钉过这一条，第九节照搬）。
- `first_time` 已有（[`server.rs:100`](../../app/src-tauri/src/server.rs#L100)），上限就是 8 种，不另立去重表。
- 日志格式与上游一致，便于两边对日志：`http: POST /quit from the web page https://…, turned away (a web page's)`。
- `header` 与 `agent_hub` 的私有版本重复十几行；`host_ok` 要求「恰好是本次监听端口」，`is_local_host` 允许任意纯数字端口，两者语义不同，不为省几行强行合并。若以后有第三个调用者，再抽公共模块。
- 403 正文是 `{"error":"not for web pages"}`，与上游一致；不返回更多信息。

### 6.3 边界情况

| 情况 | 结果 |
| --- | --- |
| `Host: 127.0.0.1:47213`、`localhost:47213`、`[::1]:47213` | 放行 |
| `Host: evil.example:47213`、`localhost.evil:47213`、`127.0.0.1.evil` | 403 |
| `Host` 没有端口、没有 Host（HTTP/1.0 脚本） | 放行（没有 Host 的不是浏览器） |
| `Origin: null`、`NULL`、带前导空格、`file://` | 403 |
| `Origin: moz-extension://…`、`chrome-extension://…` | 放行（扩展本来就拥有更高权限） |
| 同一请求同时带网页 `Origin` 和本机 `Host` | 403（Origin 优先判断） |
| 重复的 `Host` 头 | 取第一个；`agent_hub` 对 `/v1` 另有「只许一个」的更严校验。本地端口不做请求走私防护，不额外扩张范围 |

## 七、调用方盘点

| 调用方 | 带 `Origin`？ | 修复后 |
| --- | --- | --- |
| Claude Code 的 HTTP hook（axios） | 不带 | 照常 |
| Codex / Antigravity / ZCode / Reasonix / DSH 的 hook | 经本程序自己的 `ureq` 或命令转发，不带 | 照常 |
| Node 的 `fetch` / `http`、`curl`、PowerShell 的 `Invoke-RestMethod` | 不带 | 照常 |
| Thunderbird 扩展（`integrations/thunderbird`） | `moz-extension://…` | 照常 |
| 本程序自己的页面 | 走 Tauri IPC，不访问这个端口 | 不受影响 |
| `/v1/*` 的客户端 | 可能有同端口本机 `Origin` | 仍由 `agent_hub` 判定，不变 |
| 浏览器里的网页、`file://` 页面 | 有 | 403（目的） |

设置页「插件示例」里的 `Invoke-RestMethod … /widget` 不带 `Origin`，修复后仍可直接复制使用。

## 八、与上游的关系（含版本更正）

- 上游提交：`e143563e4363c6c16bd290499bde2ce778f64f80`，标题「Her local port turns away web pages' requests (403), all but /v1's」。
- 该提交属于 PR [#116](https://github.com/zhoupengjie/Wakuwaku/pull/116)（「本地端口拒绝来自网页的请求；修好偶尔挂的栈测试；加上 CI」）。
- **版本更正**：`e143563e` 的父提交是 `387a1ec1…`，也就是 `v0.4.2` 的 tag 提交；它本身是 v0.4.2 之后的第一个提交，**首次随 v0.4.3 发布**（2026-11-13）。复现包 README 写的「上游 v0.4.2 修复（`e143563`）」不准确，本仓文档按 v0.4.3 记录。
- 上游同期改了 `docs/DEVELOPMENT.md` 与 `docs/WIDGETS.md` 的说明；本仓对应改 [`DEVELOPMENT.md` 的「HTTP 接口」](../guides/DEVELOPMENT.md) 与 [`docs/specs/widgets.md`](../specs/widgets.md)（见发布计划 T4）。
- 上游的回归测试用 `tiny_http` 收发原始请求，覆盖：六种网页 Origin、三种非网页 Origin、六条放行请求、五条被拒请求、`/v1` 不归它管、以及「被拒的 POST 正文没读完，同一条连接上的下一个请求照常」。本仓 `server.rs` 已有同样的原始 TCP 测试写法（[`server.rs:490`](../../app/src-tauri/src/server.rs#L490)），照搬即可。

## 九、验收

### 9.1 单元测试（`server.rs` 的 `mod tests`）

| 用例 | 期望 |
| --- | --- |
| `is_page_origin`：`null`、`NULL`、`" https://evil.example"`、`http://127.0.0.1:47213`、`http://localhost:5173`、`file://` | 全部为真 |
| `is_page_origin`：空串、`moz-extension://…`、`chrome-extension://…` | 全部为假 |
| 未带 Origin 的 axios 风格 `POST /hook`、`Host: localhost` 的 `POST /widget`、Thunderbird 的 `POST /widget`、无 Host 的 HTTP/1.0 `POST /widget`、`GET /health` | 放行（200） |
| 带恶意 Origin 的 `GET /v1/info` | `turned_away` 返回 `None`（不归它管）；实际响应由 `agent_hub` 判 403 |
| 带站点 Origin 的 `POST /quit`、`Origin: null` 的 `POST /debug/eval`、`Origin: http://localhost:5173` 的 `POST /hook`、`Origin: file://` 的 `POST /state`、`Host: evil.example:47213` 的 `GET /health` | 403，且 `turned_away` 的理由逐字匹配 |
| 一条被拒的 `POST /quit`（正文未读）后面紧跟 `GET /health`，同一条连接 | 先 403、后 200 |

### 9.2 复现包回归

修复后重新执行 `run-repro.ps1`：

- C1、C2 应为 403（脚本会标 `not reproduced`）；
- C4、C5 不再出现 `repro-web-local` / `repro-web-file` 会话；
- C6 中 FISH Dock 不退出；
- C3 仍由浏览器拦下，与修复无关。

### 9.3 命令

```powershell
cargo test --locked --no-default-features --manifest-path app/src-tauri/Cargo.toml -- --test-threads=1
node --test --test-isolation=none 'app/test/*.test.js' 'app/test/*.test.mjs' 'app/scripts/*.test.mjs' 'sdk/node/*.test.mjs'
```

Rust 测试若遇到沙箱权限错误，保留断言并按仓库约定申请单次授权；未跑通不能记通过。

## 十、披露与发布

- 上游 v0.4.3 的发行说明已公开写着「网页不能再往她的本地端口发请求」，本问题**没有保密期**；本仓 issue 可以直接公开，不必走私有安全通告。
- 不申请 CVE：本仓不是 CNA，且漏洞已由上游公开修复。需要时可在 GitHub 开 Security Advisory（仓库有该功能），但不是发布前提。
- 感谢提供复现包的外部研究者；在 issue 与发行说明里以「提供复现包的研究者」称呼，不替对方署名。
- 仓库当前没有 `security` 类型标签，`CONTRIBUTING.md` 的类型表也没有安全类；建议在 2.3.0 发布时补一个 `security` 标签并写清用法（不影响本次修复）。
- 发行说明只写影响与修复，不放可直接复制的攻击页；复现细节留在本文件与复现包里。

## 十一、遗留与不再范围

- 本机其他程序仍可调用这些接口：端口没有鉴权，这是既有设计；`/hw` 已有 token，其余接口不为「防本机程序」加认证。修复边界只到「网页」。
- 浏览器扩展来源继续放行：扩展本身能做的事远多于这个端口，拒绝它没有安全收益，反而会破坏 Thunderbird 集成。
- 调试模式的 `/debug/eval` 仍能在页面里执行代码；修复后只有本机程序能触发，不再是网页。排障开关不应在生产环境常开，文档已如此写。
- 不做限流、不做 CSRF token：本地脚本的正常用法不能要求先拿令牌；来源校验已覆盖网页场景。
- `/v1/*` 的 `origin_ok` 与本次修复的 `is_page_origin` 是两套语义（前者只认本机同端口来源），不合并。

## 十二、issue 侧边栏（模板填不了，提交后补）

| 字段 | 值 |
| --- | --- |
| 标题 | `[安全] 本地端口接受网页请求：可退出、伪造审批、注入会话（Origin / Host 未校验，2.2.3 已复现）` |
| Labels | `bug`（建议同时新增 `security`） |
| Milestone | `2.3.0` |
| Assignees | `Fish-under-sea` |
| Projects | 无（本仓未启用） |
| Relationships | 与上游 `zhoupengjie/Wakuwaku` 的 `e143563e`（v0.4.3）同源；与 #88 / #89 无依赖 |
| Development | 未开工 |

模板正文的顺序对应：现象（第一、二节）→ 复现步骤（第四节）→ 期望（第六节）→ 证据（第四、五节）→ 修复与验收（第六、九节）。
