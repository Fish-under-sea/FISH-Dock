# FISH Dock 1.2.3

给插件更新收个尾：升完级不再留旧版本目录，正在看的那一页也会换成新票重建；顺手修了主仓两条 issue。

## 插件更新不再留旧版

以前每次装新版，旧的 `packages/<id>/<version>/` 都留在磁盘上：一次升级多几十
MB，旧目录里还带着旧 manifest。现在升级**提交成功之后**才清理，只保留当前激活
版本和最近写入的那一个（回退点），更老的删掉。失败路径不受影响：装不上时旧版
仍然在，回滚照旧。

同时修了页面一侧的漏洞：插件升级后 `digest` 变了，旧 iframe 会被移除、旧票据
会被关闭，但页面宿主还记着"当前就在这一页"，于是下一次进入直接返回、不重建，
用户盯着空白。现在退场的若是当前页，会清掉这一状态，下次进入用**新票据、新
地址**重新建帧。

## 主仓 issue

- **#17 密码框加可视按钮**：邮件表单与 LibreHardwareMonitor 表单的密码行右侧多
  一枚眼睛，点击只切换 `type`，值、光标、表单数据都不动；空值禁用，关闭后一律
  回到隐藏；`aria-pressed` 与中英文案齐备，Tab 顺序是输入框 → 按钮。
- **#16 安装日志乱码**：安装器本身是 Unicode 版，界面文字不乱；乱的是子进程那
  一路——`nsExec::ExecToLog` 按 ANSI/OEM 解释 `node.exe` 的 UTF-8 输出。现在面
  向安装器的输出全部改英文，并统一过 `ascii()` 兜底，同时往安装目录写一份
  UTF-8 BOM 的 `install.log` 存中文细节，卸载时一并删除。

## 配套

插件页的写操作（签到页的「关 / 开」「立即签到」）需要 Magpie 插件 1.2.0 的页面
调用桥才真正生效；本版与此配套发布，安装后到插件页把 Magpie 更新到 1.2.0。

## 验证

- `cargo test --bin wakuwaku`：572 通过 / 2 失败（`lhm_setup` 两项，沙箱内
  `PermissionDenied` 与计划任务根目录 `0x80070003`，环境性）/ 14 ignored。
- `node --test test/extension-page.test.js`：4/4。
- `node --test scripts/installer.test.mjs scripts/install-update.test.mjs
  scripts/install-update-performance.test.mjs`：23/23，含「安装日志只用 ASCII」
  的断言。
