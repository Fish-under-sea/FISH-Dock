# 国内镜像实测来源矩阵（2026-11-04）

> 本文件收口两条历史欠账：`docs/plans/plan-dock-2.0.4.md:329` §7.3.2 的「先调研现有免费渠道的真实覆盖、字节一致与可用性，再列实测来源矩阵；未找到时明确标记缺口，不能把默认空前缀算作已完成，也不要求用户部署服务」，以及 `docs/releases/release-2.0.4.md:93` 的「免费国内镜像缺口：pet、Release、Windhawk、LHM 均无已确认的免费国内镜像」。
>
> 它是附加条目 A 的 **S1** 产物（`docs/plans/plan-issues-2026-11-04.md` §10.4），S2 / S4 的代码落点见 §7。与工作区 `AGENTS.md` §八互为索引：那边写常驻路径与配置键，这边写实测证据。
>
> **一句话结论**：raw 文件与插件包（npm 双源）已有**实测字节一致**的免费国内渠道；GitHub Release 资产（Dock 更新 exe、Windhawk、LHM）**没有官方国内镜像**，只有第三方公开前缀加速可用（实测字节一致，但属第三方服务，默认不内置）；pet 图库在本机**DNS 污染级不可达且无任何已确认镜像**；**Gitee 双仓目前一个附件都没有**，配了也拿不到字节。

---

## 1. 实测口径

| 项 | 值 |
| --- | --- |
| 时间 | 2026-11-04 20:05–21:00（Asia/Shanghai） |
| 网络 | 中国大陆家用宽带，**未开系统代理**（Clash 内核未监听，全程直连） |
| 客户端 | node v24.14.0 的 `fetch`（undici / OpenSSL）。本机 schannel 客户端凭证不可用（`SEC_E_NO_CREDENTIALS`，工作区 `AGENTS.md` §三），curl / git 的 HTTPS 结果不可采信，**全部走 node TLS** |
| 请求方式 | 手动跟重定向（`redirect: 'manual'`，最多 6 跳，与 `sources.rs::get_with_ways`、`updates.rs::get_way_budget` 同口径），单次超时 25 s（整包下载 180 s），记录每一跳的 status / location / content-length、总耗时、正文 sha256 |
| 「匿名可达」判据 | 不带任何令牌、不设任何环境变量、不部署任何服务，直连或经**公开**前缀服务能拿到 200 |
| 「字节一致」判据 | sha256 与 GitHub Release / 官方清单（`catalog/stable.json`、`app/scripts/runtime-sources.json`、GitHub API 的 `digest` 字段）给的值**逐位相同**；体积过大无法整包下载时退化为 `content-length` 一致，并在结论里如实写明「未做整包摘要比对」 |
| 探针脚本 | 一次性产物，不进库：`FISH-Dock-1104/.work/mirror-probe/probe.mjs`（meta / bytes / npm / extra 四趟）、`probe2.mjs`（npm 版本可见性 + github 直连稳定性）、`probe3.mjs`（Gitee 发行页语义 + Windhawk / LHM 整包摘要 + pet DNS 归因）、`probe4.mjs`（插件 `.fish-plugin` 整包摘要）；原始结果同目录 `result-meta.json` / `result-bytes.json` / `result-npm.json` / `result-extra.json` / `result-final.json` / `result-plugin.json` |

**铁律（与 `plan-dock-2.0.4.md:309` 一致）**：不编造镜像地址、不伪造摘要、不改 CSP、不放宽下载域名白名单、不要求用户自建服务。找不到的写「缺口」，测不通的写实测状态码与归因，不拿默认空前缀冒充已完成。

---

## 2. 结论速览（每类资源一行）

| 资源 | 候选 URL 形状 | 匿名可达 | 与 GitHub 的 sha256 / 字节一致 | 需令牌 | 实测结论 |
| --- | --- | --- | --- | --- | --- |
| **Dock 更新 exe** | ① `https://api.github.com/repos/Fish-under-sea/FISH-Dock/releases/latest`（元数据）<br>② `https://github.com/Fish-under-sea/FISH-Dock/releases/download/v<ver>/FISH-Dock-{Setup,Update}-<ver>.exe`(+`.sha256`)<br>③ `{url.gh_proxy}/` + ②<br>④ `https://gitee.com/Fish-under-sea/FISH-Dock/releases/download/v<ver>/<同上文件名>` | ① 是（200，635 ms）<br>② **间歇**（同一 URL 6/6 次拿到 302，但整包 GET 多轮里出现 `UND_ERR_CONNECT_TIMEOUT` / `ECONNRESET`）<br>③ 是（`ghfast.top` 200 / 1.1 s，`gh-proxy.com` 200 / 1.9 s，`content-length` 326680015 B 与 GitHub API 的 `size` 完全相同）<br>④ **否**（全 404） | ② 的 `.sha256` / `SHA256SUMS.txt` 与 API `digest` 一致（`f6cbaf00…` / `7f5cf5ba…`）；③ 只比对到 `content-length` 一致，**未做 326 MB 整包摘要比对**；④ 无字节可比 | 全不需要 | **官方国内镜像＝缺口**。GitHub 直连间歇可用；第三方前缀加速实测可用且尺寸一致，但属第三方服务，**默认不内置**（`url.gh_proxy` 保持空，由用户自行配置）；Gitee 侧无发行版本，**接入前必须人工确认附件地址，本次确认结果＝地址不存在**（§4.1） |
| **插件 `.fish-plugin`** | ① `https://registry.npmjs.org/@fish-under-sea/fish-dock-plugin-<slug>/-/fish-dock-plugin-<slug>-<ver>.tgz`<br>② 同形状换 `https://registry.npmmirror.com`<br>③ `https://github.com/Fish-under-sea/FISH-DOCK-Plugins/releases/download/v<ver>/<slug>.fish-plugin`<br>④ Gitee 插件仓（`https://gitee.com/Fish-under-sea/FISH-DOCK-Plugins/...`） | ① 是（200，小包 0.4 s；**16 MB / 33 MB 大包出现 20 s 超时**）<br>② 是（200，小包 0.17–0.19 s，大包 1.7 / 3.4 / 1.2 s，**6 次里 6 次成功**）<br>③ 是（200，36368 B / 1.1–1.4 s，落地 `release-assets.githubusercontent.com`，2 跳）<br>④ **否**（仓库匿名 API 404、raw 403） | ①↔② **tarball 的 sha1 与 sha256、字节数三项全同**（feishu 2.1.4 / 2.1.5、clash 2.1.5、magpie 2.1.5 四组）；③ 整包 sha256 `39aeda0afb7cc945dcfb7ea0eacb969672f10d2d69b6c6410fe73faef59f0bd8`、36368 B，**与 `catalog/stable.json` 的 feishu 条目逐位一致**；④ 无字节可比 | 全不需要 | **已有实测字节一致的免费国内渠道**：npmmirror（②）覆盖全部三个官方插件包且比 npm 官方更快更稳。**三源不变**（npm 官方 / npmmirror / GitHub Release），Gitee 不接入（§4.2）。⚠️ 顺带实测到 `catalog/stable.json` 写 2.2.0，而 npm 与 npmmirror 上**都没有 2.2.0**（feishu/clash 最新 2.1.5、magpie 最新 2.1.6），tarball 404 —— 属 #68 的「目录抢跑」，见 §9 |
| **raw 文件**（catalog / 清单 / 文档） | ① `https://raw.githubusercontent.com/<o>/<r>/<ref>/<path>`<br>② `https://cdn.jsdelivr.net/gh/<o>/<r>@<ref>/<path>`<br>③ `https://fastly.jsdelivr.net/gh/...`、`https://gcore.jsdelivr.net/gh/...`<br>④ `{url.raw_mirror}/` + ①<br>⑤ `https://gitee.com/<o>/<r>/raw/<ref>/<path>` | ① 是（200，0.7–0.8 s）<br>② 是（200，0.55–1.0 s）<br>③ 是（fastly 200 / 0.57 s，gcore 200 / 0.76 s）<br>④ 未配置＝无此候选<br>⑤ **主仓 200**（`README.md` 20339 B，2 跳）；**插件仓 403** | ②③ 与 ① **sha256 与字节数完全相同**：`catalog/stable.json` 三家都是 1793 B / `179b9813fa8d…`；⑤ 主仓 README 无 GitHub 对照件，未做比对 | 全不需要（Gitee raw 匿名即可，但对**不可匿名访问的仓库**返回 403） | **已有实测字节一致的免费国内渠道**：jsDelivr 的三个域名全部可用且字节一致，`sources.rs::picks` 已开箱内置 ②（无需配置）。④ 保持默认空。⑤ 只对**用户自己配置的那个 Gitee 仓**有意义，本批**没有**把 raw 映射接进 `picks`（会把 catalog 的取源改指到另一个仓，风险大于收益） |
| **pet 图库** | ① `https://codex-pets.net/api/pets?page=<n>&pageSize=<n>&sort=<s>`<br>② `https://codex-pets.net/assets/pets/v/<n>/<id>/<file>.webp`<br>③ `{url.pet_mirror}` + ①② 的后半段 | ①② **否**：`fetch failed`，cause `UND_ERR_CONNECT_TIMEOUT`，多轮尝试地址分别是 `2a03:2880:f10d:83:face:b00c:0:25de` / `2a03:2880:f127:83:face:b00c:0:25de` / `2a03:2880:f12a:83:face:b00c:0:25de` / `104.244.46.208` / `199.96.62.75` / `154.85.102.32`，全部 10 s 超时<br>③ 未配置＝无此候选 | 无法比对（一个字节都没拿到） | 不需要 | **缺口（最严重的一类）**：不是「慢」，是**DNS 污染级不可达**。归因证据：阿里 DoH（`dns.alidns.com/resolve`）给 A＝`108.160.172.204`（Dropbox 段）、AAAA＝`2a03:2880:f129:83:face:b00c:0:25de`（Facebook 段），与本机解析给出的 Twitter / Facebook 段一样都是**与业务无关的知名段**；Google DoH（8.8.8.8）本身也连不上。**没有任何已确认的免费国内镜像**，`url.pet_mirror` 保持默认空，不编地址 |
| **Windhawk** | ① `https://github.com/ramensoftware/windhawk/releases/download/v1.7.3/windhawk_setup_offline.exe`<br>② `{url.gh_proxy}/` + ①<br>③ `https://windhawk.net/`（官网，非下载源）<br>④ `https://cdn.jsdelivr.net/gh/ramensoftware/windhawk@v1.7.3/<仓库内文件>` | ① **间歇**（一轮 `UND_ERR_CONNECT_TIMEOUT` 10.6 s，另一轮 200 / 1.1 s）<br>② 是（`ghfast.top` 200）<br>③ 是（200，1.9–3.6 s）<br>④ 只托管**仓库内文件**（`README.md` 301→200，858 B），**不托管 Release 附件** | ② 整包下载 148917720 B，sha256 `4d93016570f982326eebdfc9068e924ff21f6448534ea32672bb4c1d52a8193b`，**与 `app/scripts/runtime-sources.json` 的期望值逐位一致**（耗时 64 s）；① 未做整包比对（同轮直连超时） | 全不需要 | **官方国内镜像＝缺口**（与 `release-2.0.4.md:93` 的判断一致，本次实测确认仍然成立）。第三方前缀加速**实测字节一致**，可作为用户自行配置的 `url.gh_proxy` 值；jsDelivr 结构性不覆盖 Release 附件，不能当 Windhawk 镜像。产品默认不变：原地址 + 系统代理 |
| **LHM**（LibreHardwareMonitor） | ① `https://api.github.com/repos/LibreHardwareMonitor/LibreHardwareMonitor/releases/latest`（元数据）<br>② `https://github.com/LibreHardwareMonitor/LibreHardwareMonitor/releases/download/v0.9.6/LibreHardwareMonitor.zip`<br>③ `{url.gh_proxy}/` + ② | ① 是（200，0.59 s，10904 B）<br>② **否**（本轮 `UND_ERR_CONNECT_TIMEOUT` 10.7 s）<br>③ 是（`ghfast.top` 200 / 1.5 s，`content-length` 6632626 B 与 API 的 `size` 完全相同） | GitHub API 本次**给了** `digest`：`LibreHardwareMonitor.zip`＝`sha256:086d9f1b5a99e643edc2cfaaac16051685b551e4c5ac0b32a57c58c0e529c001`、`LibreHardwareMonitor.NET.10.zip`（8889121 B）＝`sha256:29739c4959b01b348fddad87664066634bcfd4f46e9bf41e4e916c318bcfdb99`。③ 只比对到 `content-length` 一致，**未做 6.6 MB 整包摘要比对** | 全不需要 | **官方国内镜像＝缺口**。`lhm_setup.rs` 的固定摘要校验与提权进程来源约束不变（`plan-dock-2.0.4.md:326`）；镜像只能是用户自配的 `url.gh_proxy`，且字节仍要过既有摘要校验才落地 |

> 表中「间歇」＝同一 URL 在本次实测的多轮里既有 200 也有 `UND_ERR_CONNECT_TIMEOUT` / `ECONNRESET`，**不是稳定不可达**，也不是稳定可达。这正是「多候选 + 失败回落 + 共用一次 deadline」存在的理由。

---

## 3. 逐类实测明细

### 3.1 Dock 更新 exe

GitHub 官方发行 `v2.0.4` 的资产清单（`GET https://api.github.com/repos/Fish-under-sea/FISH-Dock/releases/latest` → 200，18038 B，635 ms；`tag_name=v2.0.4`、`draft=false`、`prerelease=false`）：

| 资产 | 字节 | GitHub API 给的 `digest` |
| --- | --- | --- |
| `FISH-Dock-Setup-2.0.4.exe` | 326680015 | `sha256:f6cbaf001db7b4acebc9b8c237d3da0312c4a27cc81c1362c37803ac80b80544` |
| `FISH-Dock-Setup-2.0.4.exe.sha256` | 93 | `sha256:ac8bdbcddd97006b6d94fcb5ba578566a2e2a20767ad34c3571359258ac2434d` |
| `FISH-Dock-Update-2.0.4.exe` | 31648638 | `sha256:7f5cf5bab5b8d8ac4bb0495cfb888480a82e604a5deaa7cb44e78f712c1e7eea` |
| `FISH-Dock-Update-2.0.4.exe.sha256` | 94 | `sha256:19712d04432a2a2ed70061192c18a1e61d96008ac2361705464828cfda716801` |
| `SHA256SUMS.txt` | 187 | `sha256:da3ce7ef031d0c6dfbcbc3578b6c0020de5ab124d4f022ffa318b2a0c2eb69ec` |

`SHA256SUMS.txt` 实下正文（200，187 B，1396 ms）与上表 `digest` 一致：

```text
f6cbaf001db7b4acebc9b8c237d3da0312c4a27cc81c1362c37803ac80b80544  FISH-Dock-Setup-2.0.4.exe
7f5cf5bab5b8d8ac4bb0495cfb888480a82e604a5deaa7cb44e78f712c1e7eea  FISH-Dock-Update-2.0.4.exe
```

直连与前缀加速的实测：

| 请求 | 结果 |
| --- | --- |
| `github.com/.../FISH-Dock-Setup-2.0.4.exe` 取头（bytes 趟） | 200，2 跳，1644 ms |
| 同上（probe3 趟） | **FAIL** `UND_ERR_CONNECT_TIMEOUT`（`github.com:443`，10 s） |
| `github.com/.../FISH-Dock-Setup-2.0.4.exe.sha256`（bytes 趟） | 200，93 B，sha256 前 12 位 `ac8bdbcddd97` ＝ API 的 `digest`，1313 ms |
| 同上（probe3 趟） | **FAIL** `ECONNRESET`，19276 ms |
| `github.com/.../FISH-Dock-Update-2.0.4.exe` 取头 | **FAIL** `UND_ERR_CONNECT_TIMEOUT`，10680 ms |
| `ghfast.top/` + Setup.exe 取头 | **200**，`content-length` 326680015（＝ API 的 `size`），1107–1219 ms，1 跳 |
| `gh-proxy.com/` + Setup.exe 取头 | **200**，`content-length` 326680015，1251–1935 ms，1 跳 |
| `gitee.com/Fish-under-sea/FISH-Dock/releases/download/v2.0.4/*` | **全 404**（详见 §4.1） |

同一 URL 的 302 稳定性（只取头，连打 6 次）：`github.com/.../releases/download/v2.2.0/feishu.fish-plugin` → 302 六次全中（563 / 101 / 95 / 90 / 93 / 98 ms）；`.../windhawk_setup_offline.exe` → 302 六次全中（364 / 90 / 92 / 94 / 90 / 99 ms）。**说明 DNS 与 TCP 握手层多数时候是通的，卡的是随后到 `release-assets.githubusercontent.com` 的整包传输**——所以「GitHub 直连」要按间歇可用记，不能按不可用记。

### 3.2 插件 `.fish-plugin`

npm 侧的版本可见性（`GET https://registry.npmjs.org/@fish-under-sea/fish-dock-plugin-<slug>`，200）：

| 包 | npm 官方 `dist-tags.latest` | npmmirror `latest` | 是否含 `catalog` 写的 2.2.0 |
| --- | --- | --- | --- |
| `@fish-under-sea/fish-dock-plugin-feishu` | 2.1.5 | 2.1.5 | **否** |
| `@fish-under-sea/fish-dock-plugin-clash` | 2.1.5 | 2.1.5 | **否** |
| `@fish-under-sea/fish-dock-plugin-magpie` | 2.1.6 | 2.1.6 | **否** |

两家的版本列表逐条相同（顺序不同，npmmirror 的同步顺序与 npm 不一致，但集合一致）。按 2.2.0 拼出的 tarball 两家都是 **404**（npmmirror 是 302 → 404）。

tarball 的字节一致性（同一版本、两家对拉，比 sha1 / sha256 / 字节数）：

| 包@版本 | npm 官方 | npmmirror | 一致 |
| --- | --- | --- | --- |
| `feishu@2.1.4` | 200，36421 B，sha1 `83d909b3de55…`，425 ms | 200，36421 B，sha1 `83d909b3de55…`，173 ms | ✅ 三项全同 |
| `feishu@2.1.5` | 200，36423 B，sha1 `1497c8f48796…` / sha256 `49d499312b54…`，419 ms | 200，36423 B，同 sha，185 ms | ✅ |
| `clash@2.1.4` | **FAIL** 20 s 超时（16 MB 级） | 200，16898326 B，sha1 `c341cebdda86…`，1739 ms | ⚠️ 官方源超时，镜像成功 |
| `clash@2.1.5` | 200，16898545 B，sha1 `dec42445fa16…` / sha256 `bbbaf91cd7cc…`，4409 ms | 200，16898545 B，同 sha，3448 ms | ✅ |
| `magpie@2.1.5` | 200，33045443 B，sha1 `a4ac0fea0ce3…` / sha256 `d9e658fb7b92…`，1680 ms | 200，33045443 B，同 sha，11284 ms | ✅ |
| `magpie@2.1.6` | **FAIL** 20 s 超时（33 MB 级） | 200，33219992 B，sha1 `0b75a2c69556…` / sha256 `3726aec99202…`，1185 ms | ⚠️ 官方源超时，镜像成功 |

> sha 值按探针输出记前 12 位十六进制；「三项全同」指字节数、sha1、sha256 都相同。

GitHub Release 侧的整包比对（`catalog/stable.json` 的 feishu 条目：`sha256=39aeda0afb7cc945dcfb7ea0eacb969672f10d2d69b6c6410fe73faef59f0bd8`、`bytes=36368`）：

| 请求 | 结果 | 与 catalog |
| --- | --- | --- |
| `github.com/Fish-under-sea/FISH-DOCK-Plugins/releases/download/v2.2.0/feishu.fish-plugin` 第 1 次 | 200，36368 B，sha256 `39aeda0afb7c…`，1373 ms，落地 `release-assets.githubusercontent.com`（2 跳） | ✅ 逐位一致 |
| 同上 第 2 次 | 200，36368 B，同 sha256，1099 ms | ✅ |
| `ghfast.top/` + 同上 | 200，36368 B，同 sha256，1797 ms（1 跳，落地主机就是 `ghfast.top`） | ✅ |
| `gh-proxy.com/` + 同上 | 200，36368 B，同 sha256，1166 ms（1 跳） | ✅ |
| `api.github.com/repos/Fish-under-sea/FISH-DOCK-Plugins/releases?per_page=5` | 200，62298 B（GitHub 侧发行列表存在） | — |

**结论**：插件包的三源里，npmmirror 与 GitHub Release 都实测拿到了与 `catalog` 逐位一致的字节；npm 官方在小包上正常、在 16 MB / 33 MB 级大包上会超时。三源顺序与摘要权威（`catalog/stable.json`）都不动。

### 3.3 raw 文件

以 `catalog/stable.json` 为样本（同一份内容、四条渠道对拉）：

| 渠道 | 结果 | 与 `raw.githubusercontent.com` |
| --- | --- | --- |
| `https://raw.githubusercontent.com/Fish-under-sea/FISH-DOCK-Plugins/main/catalog/stable.json` | 200，1793 B，sha256 `179b9813fa8d…`，708–809 ms | 基准 |
| `https://cdn.jsdelivr.net/gh/Fish-under-sea/FISH-DOCK-Plugins@main/catalog/stable.json` | 200，1793 B，sha256 `179b9813fa8d…`，549–1037 ms | ✅ 完全一致 |
| `https://fastly.jsdelivr.net/gh/...` | 200，1793 B，同 sha256，570–840 ms | ✅ |
| `https://gcore.jsdelivr.net/gh/...` | 200，1793 B，同 sha256，758–3131 ms | ✅ |
| `https://gitee.com/Fish-under-sea/FISH-DOCK-Plugins/raw/main/catalog/stable.json` | **403**（换浏览器 UA 仍 403） | 拿不到字节 |
| `https://gitee.com/Fish-under-sea/FISH-Dock/raw/main/README.md`（对照：主仓） | 200，20339 B，477 ms，2 跳 | 无 GitHub 对照件 |
| `https://gitee.com/Fish-under-sea/FISH-Dock/raw/master/README.md` | **404**（主仓默认分支是 `main`） | — |

DNS 层：`raw.githubusercontent.com` 解析到 `185.199.108–111.133` 与 `2606:50c0:800x::154`（GitHub Pages 段，未被污染）；`cdn.jsdelivr.net` 解析到 `151.101.x.229`（Fastly）与 `2a04:4e42::485`；`gitee.com` 解析到 `180.76.198.225` / `180.76.199.13` / `180.76.198.77`（百度段，国内直连）。

**结论**：raw 类是本次唯一**开箱即有已确认免费国内镜像**的资源（jsDelivr，且 `sources.rs::picks` 已无条件内置 `cdn.jsdelivr.net/gh` 这一条）。`url.raw_mirror` 保持默认空——没有找到比 jsDelivr 更值得内置的第二家，也不编造。Gitee raw 可用但只对**用户自己配的那个仓**有意义，本批没有接进 `picks`。

### 3.4 pet 图库

| 请求 | 结果 |
| --- | --- |
| `https://codex-pets.net/api/pets?page=1&pageSize=1&sort=popular`（3 轮） | 全 **FAIL** `UND_ERR_CONNECT_TIMEOUT`；尝试地址依次为 `2a03:2880:f10d:83:face:b00c:0:25de:443` + `104.244.46.208:443`、`2a03:2880:f127:83:face:b00c:0:25de:443` + `199.96.62.75:443`、`2a03:2880:f12a:83:face:b00c:0:25de:443` + `154.85.102.32:443` |
| `https://codex-pets.net/` | **FAIL** `UND_ERR_CONNECT_TIMEOUT`，10632 ms |
| DoH 对照：`https://dns.alidns.com/resolve?name=codex-pets.net&type=A` | 200，`Status=0`，`Answer=[{name:"codex-pets.net.", TTL:199, type:1, data:"108.160.172.204"}]` |
| DoH 对照：同上 `type=AAAA` | 200，`Status=0`，`Answer=[{... type:28, data:"2a03:2880:f129:83:face:b00c:0:25de"}]` |
| DoH 对照：`https://dns.google/resolve?...` | **FAIL** `UND_ERR_CONNECT_TIMEOUT`（`8.8.8.8:443` / `2001:4860:4860::8844:443` 均不通） |

**归因**：每次解析给出的地址都不同，且全部落在与业务无关的知名段——`2a03:2880::/32` 是 Facebook、`face:b00c` 是 Facebook 的经典污染标记、`104.244.46.208` / `199.96.62.75` 是 Twitter、`108.160.172.204` 是 Dropbox、`154.85.102.32` 是境外 VPS 段。**连阿里 DoH 也返回同一批污染地址**，说明污染发生在权威解析层而不是本机 resolver。结论：`codex-pets.net` 在本网络环境下**不可达，且不是「慢」而是「黑洞」**。

**缺口**：没有已确认的免费国内镜像。`url.pet_mirror` 保持默认空；不编造地址，不改 CSP，也不要求用户自建反代。`plan-dock-2.0.4.md:317-322` 提到的「pet 预览接入共享择源层」在前端（`app/src/pet/`，本批 B 线范围），本条只负责把缺口如实记在这里。

### 3.5 Windhawk

| 请求 | 结果 |
| --- | --- |
| `https://github.com/ramensoftware/windhawk/releases/download/v1.7.3/windhawk_setup_offline.exe` 取头（第 1 轮） | **FAIL** `UND_ERR_CONNECT_TIMEOUT`，10702 ms |
| 同上（复测轮） | 200，2 跳，1119 ms |
| 同上（第 3 轮） | **FAIL** `UND_ERR_CONNECT_TIMEOUT`，10577 ms |
| `https://ghfast.top/` + 同上，**整包下载** | **200**，148917720 B，sha256 `4d93016570f982326eebdfc9068e924ff21f6448534ea32672bb4c1d52a8193b`，64351 ms |
| `https://windhawk.net/`（官网） | 200（1911 ms / 3603 ms） |
| `https://cdn.jsdelivr.net/gh/ramensoftware/windhawk@v1.7.3/README.md` | 200（301 → 200），858 B，1867 ms |

整包 sha256 与 `app/scripts/runtime-sources.json` 的 `windhawk.sha256` **逐位一致**：

```text
期望 4d93016570f982326eebdfc9068e924ff21f6448534ea32672bb4c1d52a8193b
实得 4d93016570f982326eebdfc9068e924ff21f6448534ea32672bb4c1d52a8193b
```

**结论**：Windhawk 的 GitHub Release 附件**没有官方国内镜像**；jsDelivr 只托管仓库内文件（`README.md` 能拿到，`releases/download/` 的附件拿不到），**结构上不能当 Windhawk 的镜像**。第三方前缀加速实测能拿到字节一致的整包，但它是第三方服务：默认不内置（`url.gh_proxy` 保持空），只作为用户可自行配置的值记在这里。产品行为不变：原地址 + 系统代理 + 既有摘要校验。

### 3.6 LHM

| 请求 | 结果 |
| --- | --- |
| `https://api.github.com/repos/LibreHardwareMonitor/LibreHardwareMonitor/releases/latest` | 200，10904 B，583–594 ms，`tag_name=v0.9.6`，2 个资产 |
| 资产 `LibreHardwareMonitor.zip` | 6632626 B，GitHub 给了 `digest=sha256:086d9f1b5a99e643edc2cfaaac16051685b551e4c5ac0b32a57c58c0e529c001` |
| 资产 `LibreHardwareMonitor.NET.10.zip` | 8889121 B，`digest=sha256:29739c4959b01b348fddad87664066634bcfd4f46e9bf41e4e916c318bcfdb99` |
| `github.com/.../releases/download/v0.9.6/LibreHardwareMonitor.zip` 取头 | **FAIL** `UND_ERR_CONNECT_TIMEOUT`，10657 ms |
| `https://ghfast.top/` + 同上 取头 | **200**，`content-length` 6632626 B（＝ API 的 `size`），1463 ms |

**结论**：与 Windhawk 同类——**官方国内镜像＝缺口**；第三方前缀加速可达且尺寸一致，但**未做 6.6 MB 整包摘要比对**（如实标注，不拿尺寸一致冒充摘要一致）。`lhm_setup.rs` 的固定摘要校验与提权进程来源约束（`sensors.rs:977-995`、`plan-dock-2.0.4.md:326`）不变：任何镜像来的字节都要先过既有摘要才落地。

---

## 4. Gitee 双仓的实测状态

### 4.1 主仓 `https://gitee.com/Fish-under-sea/FISH-Dock`

| 请求 | 结果 |
| --- | --- |
| `GET https://gitee.com/api/v5/repos/Fish-under-sea/FISH-Dock` | **200**，7116 B，232 ms（仓库匿名可见） |
| `GET .../api/v5/repos/Fish-under-sea/FISH-Dock/releases?per_page=5` | **200，正文 2 字节＝`[]`，即 0 条发行版本** |
| `GET .../api/v5/repos/Fish-under-sea/FISH-Dock/tags` | 200，3235 B |
| `GET https://gitee.com/Fish-under-sea/FISH-Dock.git/info/refs?service=git-upload-pack` | 200，2323 B，103–181 ms；tags **13 个**：`v1.0.0` `v1.0.1` `v1.0.2` `v1.1.0` `v1.2.0` `v1.2.1` `v1.2.2` `v1.2.3` `v2.0.0` `v2.0.1` `v2.0.2` `v2.0.3` `v2.0.4`（最新＝`v2.0.4`） |
| `GET https://gitee.com/Fish-under-sea/FISH-Dock/releases`（HTML 页面） | 200，51771 B（HTML 正文 49046 B），694 ms；页面里 `releases/download/` 命中 **0** 条、`releases/tag/` 命中 **0** 条，含「暂无发行版」类字样 |
| `GET .../releases/tag/v2.0.4` | **404** |
| `GET .../releases/download/v2.0.4/FISH-Dock-Setup-2.0.4.exe` | **404** |
| `GET .../releases/download/v2.0.4/FISH-Dock-Update-2.0.4.exe` | **404** |
| `GET .../releases/download/v2.0.4/FISH-Dock-Setup-2.0.4.exe.sha256` | **404** |
| `GET .../releases/download/v2.0.4/SHA256SUMS.txt` | **404** |
| `GET .../releases/download/v2.0.4/wakuwaku-v2.0.4-windows-x64.exe.sha256`（`updates.rs::exe_name` 的历史拼法） | **404** |
| `GET .../raw/main/SHA256SUMS.txt`（仓库内文件） | **404**（仓库里没有这个文件） |
| `GET .../raw/main/README.md`（对照） | 200，20339 B，477 ms，2 跳 |

**这就是 `plan-issues-2026-11-04.md` §10.6 要求「S1 必须人工确认附件到底在哪、按什么 URL 拼」的确认结果**：

> **Gitee 主仓有 tags、没有发行版本、没有任何附件。** `releases` API 返回 `[]` 与 `/releases` 页面 HTML 含「暂无发行版」字样是一致的（此前记录的「HTML 含发行版字样」不成立：页面里 `releases/download` 与 `releases/tag` 命中都是 0 条）。`updates.rs::gitee_found` 的拼法（`<repo>releases/download/<tag>/<exe>` 与它的 `.sha256`）与 `install_installer` 的拼法（`<repo>/releases/download/v<ver>/FISH-Dock-{Update,Setup}-<ver>.exe`）**当前都拿不到字节，全部 404**。

因此：

1. `url.gitee_repo` **保持默认空**，产品默认路径不接 Gitee；
2. 本批 S2 把「配了才启用」的两条候选接好并用单测锁住（§7），**配了也常落空**，由 `sources::candidates` 的失败回落与 `install_installer` 的摘要比对兜住——不会因为多一条 404 候选而装错东西；
3. 真要把 Gitee 变成可用的国内下载源，前置动作是**在 Gitee 上创建发行版本并上传与 GitHub 同名的四个附件 + `SHA256SUMS.txt`**，然后重跑本矩阵的 §4.1 表；在此之前不改任何默认值。

### 4.2 插件仓 `https://gitee.com/Fish-under-sea/FISH-DOCK-Plugins`（⚠️ 与 `AGENTS.md` §八的记录冲突）

| 请求 | 结果 |
| --- | --- |
| `GET https://gitee.com/api/v5/repos/Fish-under-sea/FISH-DOCK-Plugins` | **404** |
| `GET https://gitee.com/api/v5/repos/Fish-under-sea/FISH-Dock-Plugins`（另一种大小写写法） | **404** |
| `GET .../api/v5/repos/Fish-under-sea/FISH-DOCK-Plugins/releases?per_page=5` | **404** |
| `GET .../api/v5/repos/Fish-under-sea/FISH-DOCK-Plugins/tags` | **404** |
| `GET https://gitee.com/Fish-under-sea/FISH-DOCK-Plugins/raw/main/catalog/stable.json` | **403**（换浏览器 UA 仍 403） |

**冲突点**：工作区 `AGENTS.md` §八与 `plan-issues-2026-11-04.md` §10.2 都记「插件仓 API 200、`full_name=Fish-under-sea/FISH-DOCK-Plugins`、tags 5 个、releases 返回 0」。本次实测**仓库级 API 直接 404**，raw 403。Gitee 对**私有仓**的匿名 API 正是返回 404（隐藏存在性）、raw 返回 403，与实测吻合。

**判定**：按「live runtime behavior 优先于 persisted state」的证据顺位，以本次实测为准——**插件仓 Gitee 当前不是公开可匿名访问的**（转为私有、改名或删除，三者之一）。需要人工在 Gitee 网页端确认仓库可见性；`AGENTS.md` §八的插件仓行需要随之修订。

**对本批的影响：无。** 插件仓本来就不接入下载链（S3 明确不做）：`catalog/stable.json` 的 `url` 仍全部指向 GitHub Release，`plugin_store.rs` 的三源仍是 npm 官方 / npmmirror / GitHub Release，`sources.rs::picks` 新增的 Gitee 附件映射**只认同名项目**，插件仓的资产（另一个 repo）永远不会被改指到 Dock 的 Gitee 仓——这条有单测锁住（§7）。

---

## 5. 缺口清单（无法确认的渠道与原因）

| # | 资源 / 渠道 | 状态 | 原因（实测归因） |
| --- | --- | --- | --- |
| G1 | pet 图库 `codex-pets.net` 的任何国内镜像 | **缺口** | 站点本身 DNS 污染级不可达（阿里 DoH 与本机解析都返回 Facebook / Twitter / Dropbox 段地址，全超时）。站点都拿不到字节，无从比对任何镜像；也没找到任何公开镜像站。**不编造地址** |
| G2 | Windhawk 的官方国内镜像 | **缺口** | 上游只在 GitHub Release 发行；jsDelivr 结构性不托管 Release 附件（实测只能拿到仓库内 `README.md`）；Gitee 无对应镜像仓。只有第三方前缀加速可用（实测字节一致），属第三方服务，不内置 |
| G3 | LHM 的官方国内镜像 | **缺口** | 同 G2。另外**未做 6.6 MB 整包摘要比对**，只比对到 `content-length` 与 GitHub API 的 `size` 一致 |
| G4 | Dock 更新 exe 的官方国内镜像 | **缺口** | Gitee 主仓无发行版本、无附件（§4.1，全 404）；GitHub 直连间歇可用；只有第三方前缀加速，且 326 MB 整包**未做摘要比对**（只比对到尺寸一致） |
| G5 | Gitee 插件仓的匿名可达性 | **无法确认，需人工核对** | 仓库级 API 404 + raw 403，与 `AGENTS.md` §八记录的「200」冲突；判定为私有 / 改名 / 删除之一，需在 Gitee 网页端确认（§4.2） |
| G6 | `url.raw_mirror` 的第二家 raw 前缀镜像 | **未内置** | jsDelivr 三个域名已实测字节一致并开箱内置；没有找到比它更值得内置的第二家，因此 `url.raw_mirror` 保持默认空，**不把默认空前缀算作已完成** |
| G7 | `mirror.ghproxy.com` | **已死** | DNS 解析到 `2a03:2880:f127:283:face:b00c:0:25de` / `67.15.129.210`，连接 10 s 超时；两轮都失败。不建议作为 `url.gh_proxy` 的值 |
| G8 | Gitee raw 作为 catalog 的镜像 | **不接入** | 插件仓 raw 403；即便可达，把 catalog 取源改指到 Gitee 会让「目录权威」多一个事实源，与 `catalog/stable.json` 单一权威冲突。风险大于收益，本批不做 |
| G9 | Google DoH（`dns.google`）作为污染对照的第二证据源 | **不可用** | `8.8.8.8:443` 与 `2001:4860:4860::8844:443` 均连接超时。污染归因只拿到阿里 DoH 一个对照源，如实标注 |

---

## 6. 公开前缀加速服务的实测取舍（`url.gh_proxy` 的候选值）

这些是**第三方公开服务**，不是本项目的镜像，也不是官方渠道。列在这里只为「用户想自己配 `url.gh_proxy` 时有一份实测参考」；**产品默认值保持空，不内置任何一家**。

| 前缀 | 匿名可达 | 实测表现 | 字节一致 | 建议 |
| --- | --- | --- | --- | --- |
| `https://ghfast.top/` | 是 | Release 资产取头 3 次 200（799–1190 ms）；Dock Setup.exe 取头 200 / 1107–1219 ms；插件 `.fish-plugin` 整包 200 / 1797 ms 且 sha256 与 catalog 一致；**Windhawk 整包 148917720 B / 64 s，sha256 与 `runtime-sources.json` 逐位一致**；LHM zip 取头 200 / 1463 ms | ✅ 已验证两处（插件包、Windhawk） | 本次实测里最稳的一家 |
| `https://gh-proxy.com/` | 是 | 取头 3 次 200（764–1529 ms）；Dock Setup.exe 取头 200 / 1251–1935 ms；插件 `.fish-plugin` 整包 200 / 1166 ms 且 sha256 与 catalog 一致 | ✅ 已验证一处（插件包） | 可用 |
| `https://ghproxy.net/` | 是 | 取头 3 次 200，但耗时 7771–12474 ms | 未比对 | 能用但慢，不建议 |
| `https://mirror.ghproxy.com/` | **否** | 两轮都 `UND_ERR_CONNECT_TIMEOUT`；DNS 落在污染段（G7） | — | 不要用 |

**为什么默认不内置**：第三方前缀服务会看到用户请求的完整目标地址，且随时可能停服、限速或替换字节。项目的既有防线是「摘要不符就不装」（`updates.rs` 的 `digest` 比对、`plugin_store` 的 catalog `sha256`），字节被换也装不上，但**默认把用户的下载路径交给第三方**不是本项目该替用户做的决定。所以：候选映射代码就位（配了才启用），默认值保持空，选择权留给用户。

---

## 7. 代码落点（S2 / S4）

| 落点 | 文件与位置 | 做了什么 |
| --- | --- | --- |
| S2 · 择源层 | `app/src-tauri/src/sources.rs`：`gitee_parts()` / `gitee_asset()`（`prefixed` 之后）、`picks()` 末尾追加「Gitee 附件」候选 | `url.gitee_repo` 配了才产生候选，且**只映射同一个项目**：GitHub 的 `owner/repo` 必须与配置的 Gitee 仓库同名（大小写不敏感，Gitee 会规范化仓名）。产物形状 `<gitee_repo>/releases/download/<tag>/<file>`，只认 `<tag>/<file>` 一层。`candidates()` 的排序语义、共用 deadline、`secure_url` 判据**都没动**；新候选追加在既有候选之后，默认空配置下候选表逐条与 2.0.4 现状一致 |
| S2 · 更新器 | `app/src-tauri/src/updates.rs`：`endpoints()`（生产分支） | `config::gitee_repo()` 有值时把 `(Source::Gitee, <配置的仓库>)` 追加在 GitHub 之后；空 / 空白＝不启用。区域顺序仍由 `Release::ordered` / `gitee_first` 决定，`endpoints()` 不掺和。本批**没有改 `endpoints()` 的行为**（2.0.4 已是这个形状），补的是把它锁住的单测 |
| S4 · 缺口注释 | `app/src-tauri/src/sources.rs` 模块头文档 | 写明 pet / Windhawk / LHM 无已确认免费镜像、Gitee 侧只有 tags 没有附件、不伪造镜像、不改 CSP / 下载域名白名单，并指回本文件 |
| S4 · 白名单**收窄**（顺带修掉的真实缺陷） | `app/src-tauri/src/updates.rs`：`allowed()` 拆出 `in_repo()` | 原先是纯前缀比较，而 `config::gitee_repo()` 会去掉尾部 `/`，于是配了 `…/FISH-Dock` 之后 `…/FISH-DOCK-Plugins`（插件仓，同名前缀）也被判成「在放行范围内」——用户配一个仓，白名单却覆盖了两个仓。改成「前缀 + 边界字符（`/`、`?`、`#`）」：只**收窄**不放宽，仓库地址到此为止（无边界）仍不放行，`GITHUB_REPO` 自带尾部 `/` 的既有放行结果逐条不变（既有 `allowed` 用例全绿） |
| S4 · 白名单不放宽（断言） | `app/src-tauri/src/updates.rs` 单测 `the_gitee_candidate_stays_inside_the_same_allowance` | 断言 `allowed()` 对 Gitee 候选与对原地址同一口径（前缀就是用户自己配的 `gitee_repo`）；没配时任何 `gitee.com` 地址都进不来；插件仓（另一个 repo）既不进候选也不进白名单 |
| S5 · 配置键文档 | `docs/guides/config-keys.md` 的 `url.gitee_repo` 行 | 补上「同时驱动 `sources::picks` 的 Gitee 附件候选（只映射同名项目）」与本文件的指引 |

**新增单测**（`cargo test -- sources` / `-- updates`）：

| 用例 | 锁住什么 |
| --- | --- |
| `sources::tests::the_gitee_attachment_candidate_needs_the_repo_configured` | 默认无候选；配了才有；`auto` / `domestic` / `overseas` 三种排序下原地址永远在；候选地址仍过 `secure_url` |
| `sources::tests::the_gitee_candidate_never_repoints_another_project` | 插件仓、LHM、Windhawk、PawnIO 的 Release 资产**都不会**被改指到 Dock 的 Gitee 仓；插件 npm 双源不多出 Gitee 这一条 |
| `sources::tests::a_malformed_gitee_repo_yields_no_candidate` | 配置写坏（非 `https://gitee.com/<owner>/<repo>` 形态、带子路径、空 owner）＝没配；非 Release 资产形状（tag 页 / API / 缺文件名 / 嵌套路径）不映射；仓名大小写与尾部斜杠按 Gitee 规范化容忍 |
| `sources::tests::default_candidates_are_locked_without_any_mirror_key` | **四个镜像键全空时**，六类无映射地址只有「原地址」一条、两类有映射地址（jsDelivr / npm 镜像）逐条与现状一致；`domestic` 只改顺序不增不减 |
| `sources::tests::the_gitee_attachment_takes_part_in_the_same_walk` | Gitee 候选参与同一次 walk：原地址失败它接手；两个都失败两条原因都留；**共用一次 deadline**（多一个候选不会多一份预算） |
| `updates::tests::the_gitee_endpoint_appears_only_when_the_repo_is_configured` | 默认只有 GitHub 一条且地址是 `config::update_latest()`；配了才多 Gitee 一条且地址就是用户配的那个；空串 / 空白＝未配置 |
| `updates::tests::the_gitee_candidate_stays_inside_the_same_allowance` | AEGIS：白名单不放宽（见上表） |
| `updates::tests::the_allowance_stops_at_the_repository_boundary` | 放行边界：配的那个仓内的 tag / 资产 / `.sha256` 路径（含大小写变体）都放行；同名前缀的别的仓（`FISH-DOCK-Plugins`、`FISH-Dock-Plugins`、`FISH-DockX`，Gitee 侧与 GitHub 侧都算）、仓库裸地址、主机名伪装、别的账号一律不放行；没配 Gitee 时 Gitee 侧一条都不放行 |
| `updates::tests::both_installer_names_get_the_gitee_candidate` | `installer_names` 的两个真实资产名（`FISH-Dock-Update-<v>.exe` / `FISH-Dock-Setup-<v>.exe`）与它们的 `.sha256` 都能换源到 Gitee |

顺带修掉一个测试基础设施缺陷：`sources.rs` 的 `with_env` 原先在闭包 panic 时**不还原环境变量**，本批实测中一条用例失败后把 `FISH_DOCK_MIRROR_PREFER=domestic` 泄漏给同进程的其他用例，连累四条用例误报。改成 Drop 守卫（panic 也还原），并把镜像类环境变量的测试锁提为 `pub(crate) static MIRROR_ENV_LOCK`，供 `updates` 的用例共用，避免跨模块并行串扰。

**明确没做**：S3（插件仓不接入下载链）、`catalog/stable.json` 未动、`plugin_store.rs` 未动（#68 download-fix 在改）、`config.rs` 未动、CSP 与下载域名白名单未动、任何默认值未动、不发版。

**验证输出**（2026-11-04 21:15–21:17，`CARGO_TARGET_DIR` 指到工作区 `.work/rust-target`——本机 `src-tauri/` 目录在沙箱下不可创建子目录，`target/` 建不出来）：

```text
cargo test --offline --manifest-path FISH-Dock-1104/FISH-Dock/app/src-tauri/Cargo.toml -- sources
test result: ok. 27 passed; 0 failed; 0 ignored; 0 measured; 869 filtered out; finished in 0.37s

cargo test --offline --manifest-path FISH-Dock-1104/FISH-Dock/app/src-tauri/Cargo.toml -- updates
test result: ok. 35 passed; 0 failed; 3 ignored; 0 measured; 858 filtered out; finished in 2.06s
```

全量 `cargo test`（同一次改动之后）：`871 passed; 2 failed; 23 ignored`。两条失败都在 `lhm_setup::tests`，与本条无关，是**沙箱环境**造成的既有失败：`a_folder_made_with_her_dacl_is_closed_to_the_user` 在 `lhm_setup.rs:2404` 拿 `Os { code: 5, PermissionDenied }`（设置 DACL 需要非受限令牌），`this_machine_is_looked_at` 在 `:2417` 拿 `the Task Scheduler's root folder: 0x80070003`（计划任务 COM 在沙箱里不可用）。工作区 `AGENTS.md` §三记的是同一类限制。


---

## 8. 复现方法

```powershell
# 探针（一次性产物，在 .work 下，不进库）
node "FISH-Dock-1104/.work/mirror-probe/probe.mjs" meta    # 只取响应头与重定向链
node "FISH-Dock-1104/.work/mirror-probe/probe.mjs" extra   # DNS 归因 + Gitee 仓名/raw 语义 + 失败项复测
node "FISH-Dock-1104/.work/mirror-probe/probe.mjs" npm     # npm / npmmirror tarball 内档摘要
node "FISH-Dock-1104/.work/mirror-probe/probe.mjs" bytes   # 整包下载与 sha256（含 Windhawk 148 MB，慢）
node "FISH-Dock-1104/.work/mirror-probe/probe2.mjs"        # npm 版本可见性 + github 直连稳定性量化
node "FISH-Dock-1104/.work/mirror-probe/probe3.mjs"        # Gitee 发行页语义 + Windhawk/LHM 摘要 + pet DoH 对照
node "FISH-Dock-1104/.work/mirror-probe/probe4.mjs"        # 插件 .fish-plugin 整包摘要与 catalog 比对

# 单测（本机 src-tauri 目录不可写，target 目录走工作区 .work）
$env:CARGO_TARGET_DIR = "<工作区>/.work/rust-target"
cargo test --offline --manifest-path FISH-Dock-1104/FISH-Dock/app/src-tauri/Cargo.toml -- sources
cargo test --offline --manifest-path FISH-Dock-1104/FISH-Dock/app/src-tauri/Cargo.toml -- updates
```

重跑本矩阵时请注意：

1. **别高频打 Gitee 匿名 API**（`DEVELOPMENT.md:609` 记过 60 次/小时 403）；本批全程只打了 6 次 Gitee API，tag 列表走 `info/refs` 而不是 API（与 `updates.rs::gitee_found` 同口径）。
2. `github.com` 直连是**间歇**的，单次失败不能当「不可达」记；至少重复 3 次并记 cause（`UND_ERR_CONNECT_TIMEOUT` / `ECONNRESET` 要分开写）。
3. Gitee 匿名 API 对**私有仓**返回 404 而不是 403，raw 返回 403——判「仓库是否公开可匿名访问」要两条一起看。

---

## 9. 顺带实测到的两个上游问题（不在本条修复范围，需另立条目）

1. **`catalog/stable.json` 抢跑到 2.2.0，npm 上没有 2.2.0**（§3.2）：三个官方插件的 npm `latest` 分别是 2.1.5 / 2.1.5 / 2.1.6，按 catalog 的 2.2.0 拼出的 tarball 在 npm 官方与 npmmirror 都是 404；只有 GitHub Release 侧有 `v2.2.0` 的 `.fish-plugin`（实测字节与 catalog 一致）。这正是 #68 描述的「三源里两源必失败」，属 A 线（`plugin_store` 版本候选 + 回退）范围，本条只提供实测证据，不改 `plugin_store.rs`。
2. **`updates.rs::exe_name()` 拼的是历史资产名 `wakuwaku-v<ver>-windows-x64.exe`，当前发行里没有这个资产**（实测 `github.com/.../releases/download/v2.0.4/wakuwaku-v2.0.4-windows-x64.exe` → **404**；真实资产名是 `FISH-Dock-Setup-2.0.4.exe` / `FISH-Dock-Update-2.0.4.exe`）。顺着代码读的影响面：
   - `github_release()`（`updates.rs:276-288`）按 `exe_name` 在 `assets` 里找条目 → 找不到 → 返回的 `Found.exe` / `Found.digest` 都是 `None`、`size=0`；
   - `gitee_found()`（`:306-314`）用同一个名字去取 `<exe>.sha256` 当「这条发行是否完整」的门 → 这道门在当前命名下永远打不开（Gitee 侧本来也全 404，见 §4.1）；
   - `Release::can_install()`（`:136-138`）要求 `digest.is_some() && 任一 Found.exe.is_some()` → 两侧都拿不到 digest 时为 false，于是**便携版**的 `view().canInstall` 为 false，更新入口根本不出现；即便出现，`install()` 的便携分支（`:1249` 起）也会在 `r.digest` 处报 `noDigest`；
   - **安装版不受影响**：`install()` 在 `current_mode().installed` 时直接走 `install_installer()`（`:1139`），用的是 `installer_names()`（`FISH-Dock-{Update,Setup}-<ver>.exe`，`:1082-1084`）+ `asset_url()` 自己拼地址 + `<url>.sha256` 取摘要，这条路的名字是对的（实测这两个资产与它们的 `.sha256`、`SHA256SUMS.txt` 在 GitHub 上都是 200）。

   改 `exe_name` 会改更新行为（并牵动 `live_places` / `fake_places` 两条 `#[ignore]` 用例与 `test/fake-release.js` 的夹具命名），超出本条 in-scope，留给 captain 排期。

