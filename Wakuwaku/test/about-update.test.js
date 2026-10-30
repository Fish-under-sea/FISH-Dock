const assert = require('node:assert/strict')
const fs = require('node:fs')
const vm = require('node:vm')
const { test } = require('node:test')
const source = fs.readFileSync(require('node:path').join(__dirname, '../src/pet/settings.js'), 'utf8')
// 关于页这一段：公告四态的常量与判定（notesKind）、三态渲染（aboutNotes）、骨架屏
// （notesSkeleton）、hero（aboutHero）与 aboutHTML 本身。它前面的 noteNotesLoading
// 只排重绘，不进单元测试；它后面的 pageUpdate 另取（updatePanel）。
const ABOUT_FROM = '  const NOTES_TIMEOUT_MS ='
const ABOUT_TO = '  // Her updates (updates.rs)'
function about(update) {
  const start = source.indexOf(ABOUT_FROM)
  const end = source.indexOf(ABOUT_TO, start)
  assert.ok(start > 0 && end > start, '关于页这一段在 settings.js 里')
  return vm.runInNewContext(`${source.slice(start, end)};aboutHTML()`, {
    snap: { version: '2.0.0', update },
    lang: 'zh',
    sec: x => x,
    esc: x => String(x).replace(/</g, '&lt;'),
    T: (k, v) => k + (v?.version || ''),
  })
}
const { t } = require('../src/shared/i18n.js')
function updatePanel(update) {
  const start = source.indexOf('  function pageUpdate()'), end = source.indexOf('  function pageCrash()', start)
  return vm.runInNewContext(`${source.slice(start, end)};pageUpdate()`, {
    snap: { version: '2.0.0', update, settings: { autoUpdateCheck: true } },
    lang: 'zh',
    sec: x => x,
    esc: x => String(x),
    T: (key, vars) => t('zh', key, vars),
    row: (title, note, actions) => `<div>${title}|${note}|${actions}</div>`,
    sw: () => '<switch>',
    seg: (key, options) => `<segments:${key}:${options.map(o => o[0]).join(',')}>`,
    netNow: () => '',
    Date,
  })
}

test('正式安装版：检查与应用内更新（不再有发行页按钮），也不再显示源码构建提示', () => {
  const state = { current: '2.0.0', may: false, mayCheck: true, installed: true, source: false, latest: { newer: true, version: '2.1.0', canInstall: true, installer: true }, checkedAt: Date.now() }
  const html = updatePanel(state)
  assert.doesNotMatch(html, /从源码运行|校验值|下载、校验并重启/)
  assert.match(html, /data-update="check"/)
  assert.match(html, /data-update="update"/)
  assert.match(html, /应用内更新/)
  // 内置更新：不许再有导向发行页 / GitHub 的按钮。
  assert.doesNotMatch(html, /data-update="page"/)
  assert.match(html, /正式安装版/)
})

test('真正源码构建保留源码提示，正式安装版无更新时仍可手动检查且不跳发行页', () => {
  assert.match(updatePanel({ may: false, mayCheck: false, source: true }), /从源码运行/)
  const html = updatePanel({ may: false, mayCheck: true, installed: true, source: false, latest: { newer: false, version: '2.0.0' }, checkedAt: Date.now() })
  assert.match(html, /已经是最新版本/)
  assert.match(html, /data-update="check"/)
  assert.doesNotMatch(html, /从源码运行/)
  assert.doesNotMatch(html, /data-update="page"/)
})

test('软件更新这一节：规格 6 的「从哪里下载（自动 / Gitee / GitHub）」在安装版上也在', () => {
  // 安装版（installed: true）也要能选来源：规格 6 没给安装版豁免。
  const installed = updatePanel({ may: false, mayCheck: true, installed: true, source: false, latest: { newer: false, version: '2.0.0' }, checkedAt: Date.now() })
  assert.match(installed, /<segments:updateFrom:auto,gitee,github>/)
  const portable = updatePanel({ may: true, mayCheck: true, installed: false, latest: null })
  assert.match(portable, /<segments:updateFrom:auto,gitee,github>/)
})

test('关于页双语标语采用一切皆插件并明确宿主与业务插件分工', () => {
  assert.match(t('zh', 'home.intro'), /Everything is a Plugin.*一切皆插件/)
  assert.match(t('en', 'home.intro'), /Everything is a Plugin/)
  assert.match(t('zh', 'home.upstream'), /独立插件/)
})

test('关于页：about-hero 是 .about 的第一个子元素，品牌图（彩蛋按钮）又是 hero 的第一个子元素', () => {
  const html = about({ notes: { state: 'ok', entries: [] } })
  assert.ok(html.startsWith('<div class="about"><div class="about-hero">'), 'hero 是 .about 的第一个 child')
  const heroAt = html.indexOf('<div class="about-hero">')
  const btnAt = html.indexOf('<button type="button" class="about-mark-btn"', heroAt)
  const imgAt = html.indexOf('<img class="about-mark"', btnAt)
  assert.ok(btnAt > heroAt, 'hero 的第一个孩子是那个按钮')
  const afterHeroTag = html.slice(heroAt + '<div class="about-hero">'.length, btnAt)
  assert.doesNotMatch(afterHeroTag, /<(div|span|p|section|b|i|img) /, '品牌图那一格是 hero 的第一位')
  assert.ok(imgAt > btnAt, '图在按钮里')
  assert.equal(/<(div|span|p|section|b|i|img) /.test(html.slice(btnAt + '<button type="button" class="about-mark-btn">'.length, imgAt)), false, '图是按钮的第一个孩子')
  assert.match(html.slice(imgAt, imgAt + 160), /src="about-mark\.png"/)
  // 尺寸与居中不变（72×72 的宽高属性还在图上）。
  assert.match(html.slice(imgAt, imgAt + 160), /width="72" height="72"/)
  // 图片与说明都在 hero 里，hero 在关于标题之前。
  assert.ok(html.indexOf('about-hero') < html.indexOf('settings.about'), 'hero 在关于标题之前')
  assert.match(html, /class="slogan"/)
})

// 品牌图彩蛋（用户 2026-10-31）：键盘可达的真按钮，网址不在前端。
test('品牌图彩蛋：data-site=officialSite 的可点按钮，带 aria-label，且前端不含任何网址', () => {
  const html = about({ notes: { state: 'ok', entries: [] } })
  const btn = html.slice(html.indexOf('<button type="button" class="about-mark-btn"'), html.indexOf('</button>'))
  assert.match(btn, /data-site="officialSite"/, '交给宿主 settings_open_site 的 place')
  assert.match(btn, /aria-label="关于 · 打开官网"/, '无障碍名字')
  assert.match(btn, /type="button"/, '真按钮：Enter / 空格与点击同一条路')
  // 可发现性克制：没有「点击进入官网」这类文字，也没有外链图标。
  assert.doesNotMatch(btn, /点击|打开官网<|↗|external/, '不加提示文字与外链图标')
  // 地址不写死：整个 settings.js 里找不到 h5.metaapp.cn / 任何 http(s) 字面量。
  assert.equal(source.includes('metaapp'), false, 'settings.js 里没有落地页地址')
  assert.equal(/https?:\/\/[a-z0-9.-]+\.[a-z]{2,}/i.test(source.replace(/https?:\/\/(www\.)?w3\.org[^"' ]*/g, '')), false, 'settings.js 不写死任何网址')
  // 处理器还在唯一的 [data-site] 那一条上（不新造命令）。
  assert.match(source, /window\.pet\.settings\.openSite\(site\.dataset\.site/)
})

test('关于页版本行保留：版本号 + 现在检查 + 项目主页；红圈那处重复块已删', () => {
  const html = about({ may: false, checkedAt: Date.now(), latest: { newer: false, version: '2.0.0' }, notes: { state: 'ok', entries: [] } })
  assert.match(html, /s\.version/) // 版本号
  assert.match(html, /data-update="check"/) // 现在检查
  assert.match(html, /data-site="repo"/) // 项目主页
  assert.match(html, /about-version/)
  // 重复的「发行页」按钮、「发现新版本」chip 不再出现。
  assert.doesNotMatch(html, /data-update="page"/)
  assert.doesNotMatch(html, /发现新版本/)
})

test('关于页不再重复「软件更新」那一节的话与按钮（用户红圈处的功能重叠）', () => {
  // 有新版、正在检查中：关于页也不许再出现「应用内更新」按钮与状态话——
  // 那是软件更新那一节的事，两处并存就是功能重叠。
  const html = about({ latest: { newer: true, version: '2.1.0', canInstall: true }, checking: false })
  assert.doesNotMatch(html, /data-update="update"/)
  assert.doesNotMatch(html, /应用内更新/)
  assert.doesNotMatch(html, /s\.updateLatest/)
  assert.doesNotMatch(html, /s\.updateNewer/)
  assert.doesNotMatch(html, /s\.updateChecking/)
  // 检查按钮仍在（规格 7 要「现在检查」），只是不再带一句状态话。
  assert.match(html, /data-update="check"/)
  // 状态话留在软件更新那一节。
  assert.match(updatePanel({ may: false, mayCheck: true, checkedAt: Date.now(), latest: { newer: false, version: '2.0.0' } }), /已经是最新版本/)
})

test('更新公告三态：有内容立刻渲染（缓存优先）', () => {
  const html = about({ notes: { state: 'ok', entries: [{ version: '2.0.0', title: 'FISH Dock 2.0.0', body: '首版', at: '2026-10-29T10:00:00Z' }] } })
  assert.match(html, /class="notes"/)
  assert.match(html, /note-row/)
  assert.match(html, /2\.0\.0/)
  assert.match(html, /首版/)
  assert.doesNotMatch(html, /正在读取/)
  assert.doesNotMatch(html, /notes-skeleton/)
})

test('更新公告三态：缓存先到也立刻渲染，即使宿主这次还在读', () => {
  // 盘上缓存已经灌进快照（state 还是 loading），旧公告照样先画出来，不等网络。
  const html = about({ notes: { state: 'loading', entries: [{ version: '1.9.0', title: '缓存那份' }] } })
  assert.match(html, /note-row/)
  // 头部改版后（2026-10-31）版本号不再单独显示，认标题。
  assert.match(html, /缓存那份/)
  assert.doesNotMatch(html, /正在读取/)
})

test('更新公告三态：读失败但盘上有旧的，旧公告照画，并在下面说清这次没读到', () => {
  const html = about({ notes: { state: 'ok', entries: [{ version: '1.9.0', title: '旧' }], error: { code: 'notes', detail: 'timeout' } } })
  assert.match(html, /note-row/)
  assert.match(html, /旧/, '盘上那份照画（头部改版后不再显示版本号，认标题）')
  assert.match(html, /最新一次没读到/)
  assert.match(html, /timeout/)
  assert.match(html, /data-update="notes"/)
})

test('内置更新不导向 GitHub：更新一节与关于页都没有发行页文案，也没有站外链接', () => {
  const update = updatePanel({ may: false, mayCheck: true, installed: true, latest: { newer: true, version: '2.1.0', canInstall: true, installer: true }, checkedAt: Date.now() })
  assert.doesNotMatch(update, /发行页|github\.com|https?:\/\//i)
  assert.match(update, /应用内更新/)
  const aboutHtml = about({ latest: { newer: true, version: '2.1.0', canInstall: true }, notes: { state: 'ok', entries: [] } })
  assert.doesNotMatch(aboutHtml, /发行页/)
  // 关于页唯一的站外链接是规格 7 的「项目主页」（data-site="repo"，宿主白名单里打开），
  // 它不参与检查 / 下载 / 安装任何一条更新分支。
  assert.match(aboutHtml, /data-site="repo"/)
  const places = [...aboutHtml.matchAll(/data-site="([\w]+)"/g)].map(m => m[1])
  assert.deepEqual(places.sort(), ['officialSite', 'repo'].sort(), `关于页只有「项目主页」与品牌图彩蛋两个 place（实测 ${places}）`)
})

// 一份「新特性 + 修复」两节的正文，用来验瘦身只取前一节。
const MIXED_BODY = ['## 新特性', '- 灵动岛可以贴合刘海', '- 设置页按新的规格排序', '', '## 修复', '- 修了一个崩溃', '- 又修了一个'].join('\n')

test('公告头部（2026-10-31）：版本号那行删了，日期并到标题同一行、字色字号不变', () => {
  const html = about({ notes: { state: 'ok', entries: [{ version: '1.2.3', title: 'FISH Dock 1.2.3', body: MIXED_BODY, at: '2026-10-28T00:00:00Z' }] } })
  // 标题与日期同一行：「FISH Dock 1.2.3 · 2026-10-28」；日期仍用 .d（原来的灰、12px）。
  assert.match(html, /<div class="nt">FISH Dock 1\.2\.3 <span class="d">· 2026-10-28<\/span><\/div>/)
  assert.doesNotMatch(html, /class="nh"/, '旧的「版本 + 日期」头行已删')
  assert.doesNotMatch(html, /class="ver">1\.2\.3/, '版本号不再单独占一行')
  // 标题行内部只允许单空格：标题 + 一个空格 + 「· 日期」。
  const head = html.slice(html.indexOf('<div class="nt">'), html.indexOf('</div>', html.indexOf('<div class="nt">')))
  assert.doesNotMatch(head, / {2}/, '标题行里不许出现双空格')
  assert.equal((head.match(/·/g) || []).length, 1, '只有一个间隔符')
})

test('公告要点（2026-10-31）：**粗体** 与反引号渲染成真的元素，不露裸标记', () => {
  const body = ['## 新特性', '- **#17 密码框加可视按钮**：加了眼睛图标', '- `cargo test --bin wakuwaku`：572 通过'].join('\n')
  const html = about({ notes: { state: 'ok', entries: [{ version: '1.2.3', title: 'FISH Dock 1.2.3', body, at: '' }] } })
  const points = html.slice(html.indexOf('<ul class="notes-points">'), html.indexOf('</ul>'))
  assert.match(points, /<strong>#17 密码框加可视按钮<\/strong>/, '**粗体** 渲染成 strong')
  assert.match(points, /<code>cargo test --bin wakuwaku<\/code>/, '反引号渲染成 code')
  assert.doesNotMatch(points, /\*\*/, '要点里不许露出 **')
  assert.doesNotMatch(points, /`/, '要点里不许露出反引号')
  const full = html.slice(html.indexOf('<div class="nb">'), html.indexOf('</details>'))
  assert.match(full, /<strong>#17 密码框加可视按钮<\/strong>/, '折叠的完整说明同一套规则')
  assert.doesNotMatch(full, /\*\*/)
})

test('公告瘦身：只显示最新一条 release，旧的不再铺开', () => {
  const html = about({ notes: { state: 'ok', source: 'GitHub', at: 1, entries: [
    { version: '1.2.3', title: 'FISH Dock 1.2.3', body: MIXED_BODY, at: '2026-10-20T00:00:00Z' },
    { version: '1.2.2', title: 'FISH Dock 1.2.2', body: '旧的一条', at: '2026-10-10T00:00:00Z' },
  ] } })
  assert.equal((html.match(/class="note-row"/g) || []).length, 1, '只渲染最新一条')
  assert.match(html, /1\.2\.3/)
  assert.doesNotMatch(html, /1\.2\.2/)
  assert.doesNotMatch(html, /旧的一条/)
})

test('公告瘦身：只取「新特性」章节的条目，「修复」那一节不进列表', () => {
  const html = about({ notes: { state: 'ok', entries: [{ version: '1.2.3', title: 'T', body: MIXED_BODY, at: '' }] } })
  assert.match(html, /class="notes-points"/)
  assert.match(html, /灵动岛可以贴合刘海/)
  assert.match(html, /设置页按新的规格排序/)
  const points = html.slice(html.indexOf('<ul class="notes-points">'), html.indexOf('</ul>'))
  assert.equal((points.match(/<li>/g) || []).length, 2, '这一节就两条')
  assert.doesNotMatch(points, /修了一个崩溃/, '修复那一节不在列表里')
})

test('公告瘦身：认不出「新特性」章节时降级取正文前 5 条要点，都不许渲染成空白', () => {
  const six = ['- 甲', '- 乙', '- 丙', '- 丁', '- 戊', '- 己'].join('\n')
  const html = about({ notes: { state: 'ok', entries: [{ version: '1.0.0', title: '', body: six, at: '' }] } })
  assert.equal((html.match(/<li>/g) || []).length, 5, '最多 5 条')
  const points = html.slice(html.indexOf('<ul class="notes-points">'), html.indexOf('</ul>'))
  assert.match(points, /甲/, '列表里有第一条')
  assert.doesNotMatch(points, /己/, '第 6 条不进列表（它只在折叠的完整说明里）')
  assert.match(html, /己/, '完整说明里仍然全在')
  // 纯段落（一条要点都没有）也不能空白：折叠块仍给整段正文。
  const prose = about({ notes: { state: 'ok', entries: [{ version: '0.9.0', title: '', body: '这一版把一切都重写了。', at: '' }] } })
  assert.doesNotMatch(prose, /class="notes-points"/)
  assert.match(prose, /这一版把一切都重写了/)
})

test('公告瘦身：其余内容折在「查看完整说明」里，默认收起、点了才展开', () => {
  const html = about({ notes: { state: 'ok', entries: [{ version: '1.2.3', title: 'T', body: MIXED_BODY, at: '' }] } })
  assert.match(html, /<details class="fold notes-more"><summary>查看完整说明<\/summary>/)
  assert.doesNotMatch(html, /<details[^>]*\bopen\b/, '默认收起（许可那一块也是收起的）')
  assert.match(html, /修了一个崩溃/, '完整说明里还在')
})

test('公告瘦身之后：四态与「来源 · 上次同步」那行都还在', () => {
  const at = Date.UTC(2026, 9, 30, 12, 0)
  const have = about({ notes: { state: 'ok', source: 'GitHub', at, entries: [{ version: '1.2.3', title: 'T', body: MIXED_BODY, at: '' }] } })
  assert.match(have, /来源：GitHub/)
  assert.match(have, /上次同步/)
  assert.match(about({ notes: { state: 'loading', entries: [] } }), /正在读取更新公告/) // 骨架
  assert.match(about({ notes: { state: 'empty', entries: [] } }), /还没有发布过更新公告/) // 还没有
  const late = about({ notes: { state: 'ok', entries: [{ version: '1.2.3', body: MIXED_BODY }], failure: 'timeout' } })
  assert.match(late, /最新一次没读到：timeout/) // 失败仍可重试
  assert.match(late, /data-update="notes"/)
})

test('公告瘦身：1.x 那种没有「新特性」标题的正文，跳过验证/安装章节再取要点', () => {
  const body = ['# FISH Dock 1.2.3', '', '## 插件更新不再留旧版', '- 装完不再留旧版目录', '- 卸载删净定义与配置', '', '## 验证', '- cargo 572 通过', '', '## 安装', '- 双击 exe 即可'].join('\n')
  const html = about({ notes: { state: 'ok', entries: [{ version: '1.2.3', title: '', body, at: '' }] } })
  const points = html.slice(html.indexOf('<ul class="notes-points">'), html.indexOf('</ul>'))
  assert.match(points, /装完不再留旧版目录/)
  assert.match(points, /卸载删净定义与配置/)
  assert.doesNotMatch(points, /572 通过/, '「验证」那一节不算新特性')
  assert.doesNotMatch(points, /双击 exe/, '「安装」那一节不算新特性')
  assert.match(html, /572 通过/, '它们在折叠的完整说明里，一个字都没丢')
})

test('更新公告三态：读取中给骨架屏与重试，不是空白也不永远转', () => {
  const html = about({ notes: { state: 'loading', entries: [] } })
  assert.match(html, /正在读取更新公告/)
  assert.match(html, /aria-busy="true"/)
  assert.match(html, /class="notes-skeleton" aria-hidden="true"/)
  assert.equal((html.match(/class="sk"/g) || []).length, 3, '骨架三行')
  assert.match(html, /data-update="notes"/) // 有重试
})

test('更新公告三态：读不到 / 离线 / 读通了但还没有发行，各有各的话与重试', () => {
  const error = about({ notes: { state: 'error', entries: [], error: { code: 'notes', detail: 'boom' } } })
  assert.match(error, /更新公告没读到/)
  assert.match(error, /boom/)
  assert.match(error, /data-update="notes"/)
  const offline = about({ notes: { state: 'offline', entries: [] } })
  assert.match(offline, /离线/)
  assert.match(offline, /data-update="notes"/)
  // 仓库里还没有发行：不是「加载不出来」，是「还没有」。
  const empty = about({ notes: { state: 'empty', entries: [] } })
  assert.match(empty, /还没有发布过更新公告/)
  assert.doesNotMatch(empty, /离线|没读到/)
  assert.match(empty, /data-update="notes"/)
  // 宿主说 ok 但列表是空的，同样按「还没有」算。
  assert.match(about({ notes: { state: 'ok', entries: [] } }), /还没有发布过更新公告/)
  // 任何输入都不会给出一片空白。
  for (const u of [{ notes: {} }, {}, { notes: { state: 'loading' } }]) {
    const html = about(u)
    assert.ok(/正在读取更新公告|离线|没读到|还没有发布过/.test(html), '总有句话')
    assert.match(html, /data-update="notes"/)
  }
})

// ── 公告多源（t10）：来源可见、上次同步可见、失败是人话 ──────────────────
test('公告多源：有内容时显示来源与上次同步时间', () => {
  const html = about({ notes: { state: 'ok', entries: [{ version: '2.0.0', title: 'FISH Dock 2.0.0' }], source: 'Gitee', at: 1761611000000 } })
  assert.match(html, /来源：Gitee/, '哪条源读到的要说出来')
  assert.match(html, /上次同步/, '上次同步时间要说出来')
  // 没有 source 字段（旧快照）不渲染这一行，也不崩。
  const plain = about({ notes: { state: 'ok', entries: [{ version: '2.0.0' }], at: 1761611000000 } })
  assert.doesNotMatch(plain, /来源：/)
})

test('公告多源：这次没读到但盘上有旧的，人话原因跟在旧公告后面（不是异常串）', () => {
  const html = about({ notes: { state: 'ok', entries: [{ version: '1.9.0' }], failure: 'GitHub：连接超时；Gitee：连不上（refused）' } })
  assert.match(html, /最新一次没读到：GitHub：连接超时；Gitee：连不上（refused）/, 'failure 人话原样带出')
  assert.match(html, /data-update="notes"/, '重试入口还在')
  // 旧字段 error.detail 仍兜底（旧快照兼容）。
  const legacy = about({ notes: { state: 'ok', entries: [{ version: '1.9.0' }], error: { code: 'notes', detail: 'timeout' } } })
  assert.match(legacy, /最新一次没读到：timeout/)
})
