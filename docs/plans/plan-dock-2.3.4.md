# FISH Dock 2.3.4：全修全做落地计划（集成版）

> 日期：2026-11-15/16。基线：2.2.3（`0626a19dddf13eae3d2610686b22d7921fae29a2`）。
> 本计划**替代** `plan-dock-2.3.0.md`：2.3.0 计划把 #88 其余项、#89、上游候选与「不移植」产品级项列为延期/排除；2.3.4 按用户批准「全修全做」重排，上述裁剪一律作废。
> 契约与机器可读清单：`.work/2.3.4/contract-2.3.4.md`、`.work/2.3.4/requirements.json`（rev.1，56 条）。
> 本版由三支独立开发 + 三支独立验证修复后，在集成分支 `fish-234-integration` 语义合并；最终离发布还差 README/版本/出包（SETTLE）与 captain 最终提交发布。

## 一、范围（与 2.3.0 的差异）

2.3.0 计划原「非目标」：`#88` 其余 5 处宿主补丁与 Hook 缺口、`#89`、上游 0.4.x 产品级项、页面侧对话框。2.3.4 全部纳入，按下表处置：

| 维 | 类别 | 处置 |
| --- | --- | --- |
| 安全 | 本地端口拒绝网页请求（C1–C6） | 实现（SEC-1/2/3），移植上游 `e143563e` |
| 插件能力 | `dialogs.chooseFolder` 宿主能力 | 实现（DLG-1…5） |
| 插件仓对接 | magpie 网页模式「打开/选文件夹」 | 插件仓独立历史实现（HO-P0/P1/D） |
| #88 | 6 处宿主补丁 + Hook 层 10 项 | 全部实现（I88-*）；H9 见 DEC-1；H-D1 文档对齐 |
| #89 | Windows 11 任务视图图标 | 实现（I89-1，移植 pri.rs，Win10 回退字形） |
| 上游 §3.2 | #115 / 卡住还任务栏 / 托盘 / 首点 / Esc / 抓栈 | 全部实现（UP-3.2-*） |
| 上游 §3.3 | 顶栏+dock、嵌入任务栏、任意显示器、日历、HTML 邮件、外部审批、CI | 实现（DEC-1…7、DEC-9）；整页 TS（DEC-8）本轮不实施，页面保持 JS，转 2.4.0（issue 跟踪） |
| 上游 §3.4 | 等价实现 | 提供本仓代码+测试证据（UP-3.4-*） |

## 二、三分支与集成

- A（`fish-234-verify-a`，最终 `fb8abab`）：安全 + dialogs + handoff + host.dock 路由 + UPSTREAM/文档。
- B（`fish-234-verify-b`，最终 `f71b389`）：#88 全项六补丁与十项 Hook 能力。
- C（`fish-234-verify-c`，最终 `7a58e52`）：#89 + 上游 §3.2/§3.3/§3.4。
- 集成：`fish-234-integration`，基线 `0626a19`，依次 `merge --no-ff` A、B、C 三支最终 SHA；冲突逐个语义解决（见 `plan-dock-2.3.4` 第三节与集成 evidence）。
- 插件仓：magpie 改动在 `FISH-DOCK-Plugins` 独立历史（`feat/2.3.4-plugin-handoff` / 验证分支），不并入宿主仓。

## 三、验收矩阵映射

`requirements.json` 56 条每项都映射到最终代码与测试：
- 集成分支 `git diff 0626a19..HEAD` 覆盖全部 feature 项；
- design 项（DEC-1…7、9）在分支实现为 `plugin_window.rs` 覆盖窗（DEC-1，B 支）/`topbar.rs`/`embed.rs`/`home.rs`/`calendar.rs`/`askapi.rs`/`mail/sanitize.rs`/`.github/workflows/ci.yml` 并各有测试；DEC-8 整页 TS 本轮不实施（页面保持 JS），转 2.4.0 由 issue 跟踪；
- equivalence 项（UP-3.4-*、DEBUGEVAL、DOCKFLICKER）在 `evidence/verify-c/upstream-exclusions-c.md` 有本仓代码+测试证据；
- 真机项在 `evidence/` 逐项标「未测（环境）」与条件，不冒充。

## 四、测试

全量在集成根执行（命令见任务 verify）：
1. `cargo test --locked --no-default-features --manifest-path app/src-tauri/Cargo.toml -- --test-threads=1`
2. `node --test --test-isolation=none "app/test/*.test.js" "app/test/*.test.mjs" "app/scripts/*.test.mjs" "sdk/node/*.test.mjs"`
3. `git diff --check`

已知环境性失败（沿用 2.2.3 口径，逐项核实非本版引入）：`lhm_setup::tests::a_folder_made_with_her_dacl_is_closed_to_the_user`（DACL，基线同样失败）；Chromium/`spawn` 类浏览器与子进程用例（沙箱禁管道 stdio，基线同样失败）。集成 evidence 记录原始输出与基线对照。

## 五、出包与发布（后续阶段，不在本计划执行）

- 只出 Setup：版本位 2.3.4（tauri.conf.json = Cargo.toml = Cargo.lock = plugin.json），README 用 `render-readme.mjs` 重渲，新增 `docs/releases/release-2.3.4.md`，`prepare-installer.mjs` 的 `writeManifest` 生成清单，`build-installer.ps1` 出包，7z + sha256 校验。
- 推送、打 tag、GitHub/Gitee 发布必须用户明确确认后由 captain 统一执行。本地构建与提交不等于授权发布。

## 六、产品级决策（DEC-1…7、9 实现；DEC-8 转 2.4.0）

> 依据 `.work/2.3.4/decisions-captain.md` 与用户最终裁定：DEC-1…7、DEC-9 判为 A（实现）并已落地；DEC-8（整页 TypeScript 化）经用户拍板**撤销本轮实施**，页面保持 JS，转 2.4.0（issue 跟踪）。

- DEC-1（#88 H9 跨窗口 goo 融合动画）：**方案 A**——宿主在拖放落点创建顶层透明覆盖窗（`WS_EX_LAYERED | WS_EX_TRANSPARENT | WS_EX_TOOLWINDOW`，不抢焦点/不进任务栏），用 SVG/CSS 滤镜回放「液滴→颈→球」吸收与抖动后把落点交给目标插件窗口并销毁覆盖窗；B 支已实现、t6 验证（DPI/多屏、快速连拖串行、插件窗销毁/主岛隐藏取消、失败不阻断 drop）。禁止以「跨窗口无法实现」降级为窗内动画，窗内简化仅作失败回退。
- DEC-2 顶栏+dock、DEC-3 嵌入任务栏、DEC-4 任意显示器、DEC-5 日历/正在播放、DEC-6 HTML 邮件（安全默认）、DEC-7 外部审批 `POST /ask`（loopback+默认关+审计）、DEC-9 CI：已在 C 支实现为真实可跑并各有测试。
- DEC-8（整页 TypeScript 化）：**本轮不实施**，页面保持 JS；转 2.4.0（issue 跟踪），不作为 2.3.4 的交付项，也不阻塞 2.3.4 发布。
