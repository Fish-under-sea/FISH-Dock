// 设置页五个页签的分节顺序，逐条对桌面规格（C:\Users\Fish\Desktop\新建 文本文档.txt
// 的仓库抄本：FISH-Dock-1029\规格-设置页排序.txt，36 行、5 个页签 24 节）。
//
// 两种断言分工：
//  · 通用页能单独渲染（它只调别的面包函数），所以看真的渲染输出；
//  · Agent / 外观 / 邮件 / 插件 四页的分节嵌在大函数里，依赖几十个闭包，单独渲染要
//    桩一堆无关东西 —— 这些页的渲染顺序就是模板串里插值的顺序，所以按源码里标记
//    出现的先后断言（标记都取自真的 i18n 键与函数名，改名即红）。
const assert = require('node:assert/strict')
const fs = require('node:fs')
const path = require('node:path')
const vm = require('node:vm')
const { test } = require('node:test')

const source = fs.readFileSync(path.join(__dirname, '../src/pet/settings.js'), 'utf8')
// 关于页那一段的起止（about-update.test.js 用的是同一对标记）。
const ABOUT_FROM = '  const NOTES_TIMEOUT_MS ='
const ABOUT_TO = '  // Her updates (updates.rs)'
const SPEC_FILE = path.join(__dirname, '../../../规格-设置页排序.txt')
const spec = fs.readFileSync(SPEC_FILE, 'utf8')

// 规格里那五行页签标题（「1. 通用（now）」…）的 key，按行序 —— 页签顺序由它说了算。
function specTabs(text) {
  return [...text.matchAll(/^\d+\.\s*\S+[（(](\w+)[)）]/gm)].map(m => m[1])
}

// 取 settings.js 里一段源码（从 from 到下一个 to 之前），断言两段都存在。
function slice(from, to) {
  const start = source.indexOf(from)
  const end = source.indexOf(to, start)
  assert.ok(start > 0, `settings.js 里找不到「${from}」`)
  assert.ok(end > start, `settings.js 里「${to}」不在「${from}」之后`)
  return source.slice(start, end)
}

// marks 必须在 text 里按顺序出现（可以不相邻，但不能倒）。
function inOrder(label, text, marks) {
  let cursor = 0
  for (const mark of marks) {
    const at = text.indexOf(mark, cursor)
    assert.ok(at >= 0, `${label}：按规格该有「${mark}」，且排在前面那些之后`)
    cursor = at + mark.length
  }
}

// 通用页真的渲染一遍：它只调 pageNow / pageQuiet / pageNet / pageUpdate /
// identityPanel.renderBackup / pageCrash / aboutHTML，全换成记号，留下来的就是这一页的节序。
function generalBody() {
  return vm.runInNewContext(`${slice('  function pageGeneral()', '  const PAGES =')};pageGeneral()`, {
    snap: { version: '2.0.0', loginAtStart: true, settings: { lang: 'zh' } },
    pageNow: () => '«2-快捷开关»',
    pageQuiet: () => '«3-安静»',
    pageNet: () => '«5-网络代理»',
    pageUpdate: () => '«6-软件更新»',
    pageCrash: () => '«X-崩溃报告»',
    aboutHTML: () => '«7-关于»',
    identityPanel: { renderBackup: () => '«X-配置与备份»' },
    part: (id, html) => `‹${id}›${html}`,
    sec: x => `[${x}]`,
    T: k => k,
    esc: x => String(x),
    row: () => '',
    sw: () => '',
    seg: () => '',
  })
}

test('规格 1–5：五个页签的顺序就是 通用 → Agent → 外观 → 邮件 → 插件', () => {
  const tabs = specTabs(spec)
  assert.deepEqual(tabs, ['now', 'agents', 'look', 'mail', 'widgets'], '规格文件里的页签顺序')
  const sandbox = { window: {} }
  vm.runInNewContext(fs.readFileSync(path.join(__dirname, '../src/pet/extension-tabs.js'), 'utf8'), sandbox)
  // 从 vm 里取出来的数组是另一个 realm 的 Array，展开成本地的再比。
  assert.deepEqual([...sandbox.window.ExtensionTabs.CORE], tabs, '默认页签顺序与规格不一致')
})

test('规格 1–7：通用页渲染出来的节序（配置与备份在软件更新之后、关于之前；崩溃报告排在关于之后）', () => {
  inOrder('通用页', generalBody(), ['«2-快捷开关»', '«3-安静»', 's.startLang', '«5-网络代理»', '«6-软件更新»', '«X-配置与备份»', '«7-关于»', '«X-崩溃报告»'])
})

test('用户 2026-10-30：配置与备份搬到通用页，插件页不许再渲染它一遍', () => {
  const body = generalBody()
  assert.match(body, /‹backup›«X-配置与备份»/, '通用页带 part(backup)')
  assert.equal(body.indexOf('«X-配置与备份»') < body.indexOf('«7-关于»'), true, '在关于之前')
  assert.equal(body.indexOf('«X-配置与备份»') > body.indexOf('«6-软件更新»'), true, '在软件更新之后')
  // 插件页（pageWidgets）现在只剩 extension-panel 的身份卡，没有第二份备份面板。
  const widgets = slice('  function pageWidgets()', '  // What went wrong with a mail account')
  assert.equal(widgets.includes('identityPanel'), false, '插件页不再挂载 identityPanel（重复渲染）')
  // 旧挂载点（widgets 上的 identityPanel.mount）也删干净了：全仓只剩通用页这一处。
  const mounts = [...source.matchAll(/identityPanel\.mount\(/g)]
  assert.equal(mounts.length, 1, `identityPanel.mount 只剩一处（实测 ${mounts.length}）`)
  assert.match(source, /if \(tab === 'now'\) identityPanel\.mount\(body\)/, '挂载点在通用页')
  // 旧页名跳转仍能落到该去的地方：backup → now，identity → widgets。
  assert.match(source, /backup: 'now'/, 'PART_OF.backup')
  assert.match(source, /identity: 'widgets'/, 'PART_OF.identity')
})

test('规格 2–7：通用页规格之外的两节 —— 配置与备份排在关于之前（用户点名要的位置），崩溃报告排在关于之后', () => {
  const body = generalBody()
  assert.equal(body.indexOf('«X-崩溃报告»') > body.indexOf('«7-关于»'), true, '崩溃报告在关于之后（不隔断 1–7）')
  assert.match(body, /‹crash›/, '崩溃报告仍带 part(crash)（旧页名跳转靠它）')
  // 「关于」仍是规格 1–7 的最后一节：它后面只允许崩溃报告，不允许再塞别的。
  const afterAbout = body.slice(body.indexOf('«7-关于»') + '«7-关于»'.length)
  assert.equal(afterAbout.includes('«X-配置与备份»'), false, '关于之后没有配置与备份')
})

test('规格 8–15：Agent 页 已找到 → 会话 → 闲置 → 通知 → 提示 → 其它 → 写入说明', () => {
  const body = slice('  function pageAgents()', '  function pageWriteNotes()')
  inOrder('Agent 页', body, [
    'ag.found', // 9 已找到的 Agent
    "part('sessions'", // 10 会话列表
    "part('alerts'", // 11–13 打包在提醒区里，各自顺序见下
    "part('idle'", // 11 闲置的会话
    "part('notify'", // 12 通知
    "part('prompts'", // 13 提示
    '${others}', // 14 其它（'others' 这个变量在 return 之前算好，这里看插值位置）
    "part('files'", // 15 写入说明
  ])
})

test('规格 11/13/34：闲置分钟、等你回答秒数、轮流显示秒数的候选值与规格一致', () => {
  assert.match(source, /const IDLE_HIDES = \[15, 30, 60, 120, 0\]/, '规格 11：15 / 30 / 60 / 120 分钟、一直显示（0）')
  assert.match(source, /const WAITS = \[30, 60, 120, 290\]/, '规格 13：30 / 60 / 120 / 290 秒')
  assert.match(source, /const SPINS = \[0, 5, 8, 15\]/, '规格 34：关（0）/ 5 / 8 / 15')
})

test('规格 16–24：外观页 显示方式 → 内置（任务栏 → 灵动岛 → Windows）→ 标签页排序 → 她 → 宠物与图库', () => {
  const look = slice('  function pageLook()', '  function pageWindows()')
  inOrder('外观页', look, [
    'home.display', // 17 显示方式（灵动岛 / 任务栏二选一卡片）
    's.builtin', // 18 内置（分组）
    's.taskbarEdge', // 19 任务栏参数
    'islandPosRow()', // 20 灵动岛参数
    'pageWindows()', // 21 Windows（含壁纸与 Windhawk）
    'pageTabOrder()', // 22 标签页排序
    's.her', // 23 她
    'pagePets()', // 24 宠物与图库
  ])
  // 19 的五样、20 的四样、23 的四样、24 的三样，逐个在各自片段里对序。
  inOrder('任务栏参数', look, ['s.taskbarEdge', 'displaysRow()', 's.taskbarMaterial', 's.clearWhen', 's.taskbarButtons'])
  inOrder('灵动岛参数', look, ['islandPosRow()', 's.islandWidth', 'hideUnderRow()', 's.oledShift'])
  inOrder('她', look, ['menu.size', 's.details', 's.walk', 's.look'])
  const pets = slice('  function pagePets()', '  // --- Her island')
  inOrder('宠物与图库', pets, ['home.installed', 's.download', '${more}'])
  const windows = slice('  function pageWindows()', '  let pickingPaper = false')
  inOrder('Windows', windows, ['s.winMode', 's.winAccent', 'pagePaper()', 'windhawkPanel.render()'])
})

test('规格 25–29：邮件页 邮箱 → 本地保留 → 邮件代理 → 收件箱', () => {
  const mail = slice('  function pageMail()', '  // The accounts: each with its switch')
  inOrder('邮件页', mail, ['accountsHTML()', 'inboxHTML(id)'])
  const accounts = slice('  function accountsHTML()', '  // How much is kept whole')
  inOrder('邮箱一节', accounts, ['mail.section', 'offlineHTML()', 'proxyHTML()'])
})

test('规格 30–36：插件页 身份 → 插件扩展页签 → 插件和小工具 → 显示 → 在终端里用 → 自己写一个', () => {
  const widgets = slice('  function pageWidgets()', '  // What went wrong with a mail account')
  inOrder('插件页', widgets, [
    'extensionPanel.render()', // 31 身份（GitHub 登录 / Star / 设备验证，extension-panel 的身份卡）
    'w.section', // 33 插件和小工具
    'w.show', // 34 显示
    'w.terminal', // 35 在终端里用
    'w.write', // 36 自己写一个
  ])
  // 32 插件扩展页签也在这块卡里：render() 之后紧跟 w.section，中间不再插备份面板。
  assert.match(widgets, /extensionPanel\.render\(\)\}\$\{sec\(T\('w\.section'\)/)
})

test('规格 2/3/5：快捷开关四个的顺序、安静两条、网络代理手动 + 测试', () => {
  const now = slice('  function pageNow()', '  function pagePets()')
  inOrder('快捷开关', now, ["'show'", "'dnd'", "'sound'", "'home'"])
  const quiet = slice('  function pageQuiet()', '  // The plugins page as rows')
  inOrder('安静', quiet, ['menu.dnd', 'settings.hideInFullscreen'])
  const net = slice('  function pageNet()', '  // What the Test found')
  inOrder('网络代理', net, ['WAY_MODES.netProxy', 'data-net-test', 'manualRow'])
})

test('规格 6/7：软件更新一节给四样，关于只给版本行与说明', () => {
  const update = slice('  function pageUpdate()', '  // Her crash reports')
  inOrder('软件更新', update, ['s.updateVersion', 'data-update="check"', 's.updateAuto', 's.updateFrom'])
  const about = slice(ABOUT_FROM, ABOUT_TO)
  // 关于一节：品牌图 → 版本行（版本号 + 现在检查 + 项目主页）→ 说明文案，没有别的更新控件。
  inOrder('关于', about, ['about-mark', 's.version', 'data-update="check"', 'data-site="repo"', 'class="slogan"'])
  assert.doesNotMatch(about, /data-update="update"/, '关于里不许再有「应用内更新」（与软件更新一节重叠）')
})
