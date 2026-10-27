const assert = require('node:assert/strict')
const { test } = require('node:test')

const WCards = require('../src/pet/wcards')

const dark = { lang: 'zh', light: false, detailed: true, width: 266, hour: 14, now: new Date(2026, 9, 16, 14, 30).getTime() }
const light = { ...dark, light: true }
const en = { ...dark, lang: 'en' }

const weather = {
  id: 'weather',
  plugin: 'weather',
  label: 'Dresden · 阴',
  value: '11°',
  color: '#ffb340',
  detail: {
    sizes: ['s', 'm', 'l'],
    extra: '8°~16°',
    series: { points: [10, 11, 13, 15, 16, 14, 11], unit: '°' },
    rows: [
      { kind: 'hour', when: '14时', icon: 'cloud', value: '11°' },
      { kind: 'hour', when: '16时', icon: 'sun', value: '15°' },
      { kind: 'day', when: '明天', icon: 'rain', value: '9°~14°', name: '小雨' },
    ],
  },
}
const monitor = {
  id: 'monitor',
  builtIn: true,
  label: { key: 'widget.monitor' },
  value: { parts: [{ icon: 'cpu', text: '17%' }, { icon: 'memory', text: '44%' }, { icon: 'down', text: '7.4K' }, { icon: 'up', text: '337K' }] },
  detail: {
    sizes: ['s', 'm', 'l'],
    series: { points: [12, 17, 20, 17], unit: '%', hi: 100, lo: 0 },
    rows: [
      { kind: 'cpu', value: '17%', text: '3.5 GHz', name: '16', unit: '32' },
      { kind: 'memory', value: '44%', text: '14.1 / 32 GB' },
      { kind: 'net', value: '7.4K', text: '337K', points: [7400, 8000] },
      { kind: 'uptime', value: '273600' },
    ],
  },
}
const stock = {
  id: 'stock-aapl',
  plugin: 'stock',
  label: '苹果',
  value: '230.40 ▲0.80%',
  color: '#34d27b',
  detail: { sizes: ['s', 'm', 'l'], series: { points: [228, 229, 230.4], hi: 231, lo: 227 }, rows: [{ code: 'AAPL', name: '苹果', price: '230.40', change: '+0.80%', value: '48.1M' }] },
}
const inbox = {
  id: 'inbox-a',
  from: 'mail',
  label: 'Anna：周五见',
  value: { key: 'mail.unread', vars: { n: '13' } },
  private: true,
  detail: { sizes: ['s', 'm', 'l'], rows: [{ from: 'Anna', subject: '周五见', at: new Date(2026, 9, 16, 10, 42).getTime() }] },
}

test('each kind its colour, dark and light; a script its own, deepened on light', () => {
  assert.equal(WCards.kindOf(weather), 'weather')
  assert.equal(WCards.kindOf(stock), 'stock')
  assert.equal(WCards.kindOf(inbox), 'mail')
  assert.equal(WCards.kindOf({ id: 'ci-main', plugin: 'ci' }), 'generic')
  assert.equal(WCards.colorOf(weather, dark), '#8cc8ff')
  assert.equal(WCards.colorOf(weather, light), '#1a78c2')
  assert.equal(WCards.colorOf(monitor, dark), '#34d27b')
  assert.equal(WCards.colorOf(stock, light), '#a46a00')
  assert.equal(WCards.colorOf({ id: 'x', color: '#5e9bff' }, dark), '#5e9bff')
  assert.notEqual(WCards.colorOf({ id: 'x', color: '#ffd60a' }, light), '#ffd60a')
  // Ups and downs by the colour the script gave, the one place that decides
  // (stock.ps1: green up and red down; -RedUp the other way round).
  assert.equal(WCards.trendOf(stock), 'green')
  assert.equal(WCards.trendOf({ color: '#ff5c6c' }), 'red')
  assert.equal(WCards.trendOf({ color: '#34d27b' }), 'green')
})

test("Windows' own glyphs: the weather's by its sky now, the rest by kind; her face for today", () => {
  assert.equal(WCards.glyphOf(weather), 'E753')
  assert.equal(WCards.glyphOf({ ...weather, detail: { rows: [{ kind: 'hour', icon: 'sun' }] } }), 'E706')
  assert.equal(WCards.glyphOf({ ...weather, detail: { rows: [{ kind: 'hour', icon: 'rain' }] } }), 'EB42')
  assert.equal(WCards.glyphOf(monitor), 'E950')
  assert.equal(WCards.glyphOf(stock), 'E9D9')
  assert.equal(WCards.glyphOf(inbox), 'E715')
  assert.equal(WCards.glyphOf({ id: 'pomodoro' }), 'E916')
  assert.equal(WCards.glyphOf({ id: 'countdown' }), 'E787')
  assert.match(WCards.glyphHTML({ id: 'today' }), /class="wher/)
  assert.match(WCards.glyphHTML({ id: 'ci-main', icon: 'check' }), /<svg/)
})

test('on the taskbar: small its glyph and value, medium its value over its name; a private line hidden', () => {
  assert.match(WCards.faceHTML(weather, 'S', dark), /&#xE753;.*<b class="twv">11°<\/b>/)
  const m = WCards.faceHTML(weather, 'M', dark)
  assert.match(m, /<b class="twv">11° 阴<\/b><i>Dresden<\/i>/)
  // The monitor: CPU over memory and the network down.
  assert.match(WCards.faceHTML(monitor, 'M', dark), /CPU 17%<\/b><i>内存 44% · ↓ 7\.4K<\/i>/)
  // A stock: its change in the down colour (its script's red), light deeper.
  assert.match(WCards.faceHTML(stock, 'M', dark), /苹果 230\.40<\/b><i style="color:#34d27b">\+0\.80%<\/i>/)
  assert.match(WCards.faceHTML(stock, 'M', light), /color:#0f7b3f/)
  // Mail: its count over the letter, "···" while the specifics may not show.
  assert.match(WCards.faceHTML(inbox, 'M', dark), /<i>Anna：周五见<\/i>/)
  assert.match(WCards.faceHTML(inbox, 'M', { ...dark, detailed: false }), /<i>···<\/i>/)
  // Nothing yet: greyed, its name; loading, a shimmer; stopped, amber.
  assert.match(WCards.emptyFaceHTML({ type: 'plugin', of: 'weather' }, '天气', 'M', 'none', dark), /暂无数据.*天气/)
  assert.match(WCards.emptyFaceHTML({ type: 'plugin', of: 'weather' }, '天气', 'M', 'loading', dark), /wshim/)
  assert.match(WCards.emptyFaceHTML({ type: 'plugin', of: 'weather' }, '天气', 'S', 'error', dark), /werr/)
})

test('in the open island: a row small or medium, a curve where it has one, a › where it has a card', () => {
  const s = WCards.rowHTML(weather, 'S', dark, { card: true })
  assert.doesNotMatch(s, /wspark|wtwo/)
  assert.match(s, /class="wchev"/)
  const m = WCards.rowHTML(weather, 'M', dark, { card: true })
  assert.match(m, /<span class="wtwo"><span class="wl">天气<\/span><span class="wsec">阴 · 8°~16° · Dresden<\/span><\/span>/)
  // The monitor small: its three readings in words; a stock in its trend's colour.
  assert.match(WCards.rowHTML(monitor, 'S', dark), /系统监控<\/span><span class="wv" style="color:#34d27b">17% · 44% · ↓7\.4K/)
  assert.match(WCards.rowHTML(stock, 'M', dark), /<span class="wsec">AAPL · \+0\.80%<\/span>.*color:#34d27b">230\.40</)
  // Today: its turns and time, its approvals and the tokens widget's.
  const today = { id: 'today', value: { key: 'widget.todayValue', vars: { turns: '197', time: '2:01', approvals: '3' } } }
  assert.match(WCards.rowHTML(today, 'M', { ...dark, all: [{ id: 'tokens', value: { vars: { total: '1.7B' } } }] }), /Claude 今天<\/span><span class="wsec">批准 3 次 · 1\.7B token<\/span>.*>197 轮 · 2:01</)
  // A pomodoro medium: a small ring of the time left.
  assert.match(WCards.rowHTML({ id: 'pomodoro', label: '专注', value: '18:42', detail: { sizes: ['s', 'm'], series: { points: [750], hi: 1500 } } }, 'M', dark), /class="wring"/)
  assert.match(m, /<svg class="wspark" width="56" height="16"/)
  assert.match(WCards.rowHTML({ id: 'ci-main', label: 'main', value: '✓' }, 'S', dark), /wchev none/)
  // A private one's name blurred while the specifics may not show.
  assert.match(WCards.rowHTML(inbox, 'S', { ...dark, detailed: false }), /<span class="wpriv hid">Anna：周五见<\/span>/)
})

test("a card: the weather's curve and hours, the monitor's lines (none read: no data), a stock's day", () => {
  const w = WCards.cardHTML(weather, dark)
  assert.match(w, /<svg class="wspark" width="266" height="40"/)
  assert.match(w, /14时<i style="color:#8cc8ff">&#xE753;<\/i><b>11°<\/b>/)
  assert.match(w, /明天 9°~14° 小雨/)
  const mon = WCards.cardHTML(monitor, dark)
  assert.match(mon, /16 核 32 线程 · 3\.5 GHz/)
  assert.match(mon, /内存<\/span><span class="cd">14\.1 \/ 32 GB<\/span><span class="cv">44%/)
  // No GPU nor drive read: greyed, no data.
  assert.match(mon, /cline off"><span class="cn">GPU<\/span><span class="wnone">暂无数据/)
  assert.match(mon, /cline off"><span class="cn">磁盘/)
  assert.match(mon, /运行 3 天 4 小时/)
  const st = WCards.cardHTML(stock, dark, [stock, { id: 'stock-msft', plugin: 'stock', label: 'MSFT', value: '420.00 ▼1.30%', color: '#ff5c6c' }])
  assert.match(st, /AAPL · 苹果/)
  // Up green, down red, as the script's colours say.
  assert.match(st, /<span class="cchip" style="--c:#34d27b">\+0\.80%<\/span>/)
  assert.match(st, /MSFT<\/span><span class="cv">420\.00<\/span><span class="cchip" style="--c:#ff6b6b">/)
  assert.match(st, /成交 48\.1M/)
  assert.match(st, /最高 231 · 最低 227/)
  assert.match(st, /class="off"[^>]*>1周/)
  assert.match(st, /MSFT<\/span><span class="cv">420\.00/)
  // Mail: who, when, what about; private, blurred.
  const mail = WCards.cardHTML(inbox, { ...dark, detailed: false })
  assert.match(mail, /<b><span class="wpriv hid">Anna<\/span><\/b><em>10:42<\/em>/)
  assert.ok(WCards.hasCard(weather) && WCards.hasCard(monitor) && !WCards.hasCard({ id: 'pomodoro', detail: { sizes: ['s', 'm'] } }))
})

test("today's card: its numbers, the turns by the hour up to now lit, the tokens' (the tokens widget's)", () => {
  const today = { id: 'today', builtIn: true, label: { key: 'widget.today' }, value: { key: 'widget.todayValue', vars: { turns: '197', time: '2:01', approvals: '3' } }, detail: { sizes: ['s', 'm', 'l'], series: { points: [0, 0, 0, 0, 0, 0, 0, 0, 0, 2, 5, 9, 14, 11, 18, 0, 0, 0, 0, 0, 0, 0, 0, 0] } } }
  const tokens = { id: 'tokens', builtIn: true, value: { key: 'widget.tokensValue', vars: { total: '1.7B', output: '3.4M' } } }
  const card = WCards.cardHTML(today, dark, [today, tokens])
  assert.match(card, /<b>197<\/b><span>轮<\/span>/)
  assert.match(card, /<b>1\.7B<\/b><span>token<\/span>/)
  // Twelve hour slots at least (the design session): from the first turn's
  // hour (9), twelve back from now (14): 3 to 14; the hours with turns bars,
  // the hour it is lit, the others a 2 px line; 18 px at most each.
  const rects = card.match(/<rect [^>]*>/g) || []
  assert.equal(rects.length, 12)
  assert.equal(rects.filter(r => /height="2"/.test(r)).length, 6)
  assert.ok(rects.every(r => Number(/width="([\d.]+)"/.exec(r)[1]) <= 18))
  assert.match(card, /fill="#f08a62"\/><\/svg>/)
  assert.match(card, /3 时.*14 时/)
  // Not counted yet: greyed.
  assert.match(card, /每小时的 token<\/span><span class="wnone">暂无数据/)
})

// The bills by agent: the widget shows the one it swipes to, its card lists
// them all (the one shown lit), and the today card's token cell is that same
// agent's bill, not the day's sum.
test("today's card and the token widget turn to the same agent (its own bill)", () => {
  const byAgent = [
    { id: 'claude', who: 'Claude', total: '850K', output: '40K' },
    { id: 'zcode', who: 'ZCode', total: '60K', output: '12K' },
    { id: 'codex', who: 'GPT', total: '300K', output: '30K' },
  ]
  const tokens = { id: 'tokens', builtIn: true, value: { key: 'widget.tokensValue', vars: { total: '1.2M', output: '85K' }, agents: byAgent }, detail: { sizes: ['s', 'm', 'l'] } }
  const today = { id: 'today', builtIn: true, value: { key: 'widget.todayValue', vars: { turns: '9', time: '0:40', approvals: '1' }, agents: [{ id: 'claude', who: 'Claude', turns: '5', time: '0:30', workedMs: 1800000, approvals: '1', hours: [] }, { id: 'zcode', who: 'ZCode', turns: '4', time: '0:10', workedMs: 600000, approvals: '0', hours: [] }] }, detail: { sizes: ['s', 'm', 'l'], series: { points: new Array(24).fill(0) } } }
  const all = [today, tokens]
  // The busiest first: both widgets start on Claude.
  assert.equal(WCards.titleOf(tokens, dark), 'Claude 的 token')
  assert.equal(WCards.titleOf(today, dark), 'Claude 今天')
  // Its row: the agent's own bill, not the sum over all of them.
  const row = WCards.rowHTML(tokens, 'M', { ...dark, all })
  assert.match(row, /850K/)
  assert.ok(!/1\.2M/.test(row), 'the sum is not the agent it shows')
  // Its card lists every bill, the one shown lit.
  const card = WCards.cardHTML(tokens, dark, all)
  assert.match(card, /Claude.*850K/)
  assert.match(card, /GPT.*300K/)
  assert.match(card, /ZCode.*60K/)
  assert.match(card, /class="cline on"/)
  // The today card's token cell: Claude's 850K, not the 1.2M sum.
  const dayCard = WCards.cardHTML(today, dark, all)
  assert.match(dayCard, /<b>850K<\/b><span>token<\/span>/)
  assert.ok(!/1\.2M/.test(dayCard))
  // A swipe on either goes to the same agent (and both follow).
  assert.equal(WCards.cycleAgents('tokens', 1), 'zcode')
  assert.equal(WCards.titleOf(tokens, dark), 'ZCode 的 token')
  assert.equal(WCards.titleOf(today, dark), 'ZCode 今天')
  assert.match(WCards.cardHTML(today, dark, all), /<b>60K<\/b><span>token<\/span>/)
  assert.match(WCards.cardHTML(tokens, dark, all), /class="cline on"[\s\S]*?ZCode/)
  // Back the other way wraps: zcode is second of three, one back is Claude.
  assert.equal(WCards.cycleAgents('today', -1), 'claude')
  // No bills for an agent: the sum, as it was (a script's words, old data).
  const plain = { id: 'tokens', builtIn: true, value: { key: 'widget.tokensValue', vars: { total: '1.7B', output: '3.4M' } } }
  assert.match(WCards.rowHTML(plain, 'M', { ...dark, all: [plain] }), /1\.7B/)
  assert.match(WCards.cardHTML(plain, dark, [plain]), /1\.7B/)
  // An agent that spent nothing (dsh, omp, reasonix: no bill at all) is never
  // shown another agent's sum as its own: no token figure, the gap left empty.
  const dshToday = { id: 'today', builtIn: true, value: { key: 'widget.todayValue', vars: { turns: '1', time: '0:06', approvals: '0' }, agents: [{ id: 'dsh', who: 'DeepSeek', turns: '1', time: '0:06', workedMs: 755474, approvals: '0', hours: [] }] }, detail: { sizes: ['s', 'm', 'l'], series: { points: new Array(24).fill(0) } } }
  const dshRow = WCards.rowHTML(dshToday, 'M', { ...dark, all: [dshToday, tokens] })
  assert.match(dshRow, /DeepSeek 今天/)
  assert.ok(!/1\.2M/.test(dshRow), 'never another agent\'s sum')
  assert.ok(!/850K/.test(dshRow), 'and not the busiest agent\'s bill either')
  const dshCard = WCards.cardHTML(dshToday, dark, [dshToday, tokens])
  assert.ok(!/1\.2M/.test(dshCard), 'the today card leaves the token cell empty too')
  assert.match(dshCard, /<b>—<\/b><span>token<\/span>/)
})

test('a curve, a ring, bars: each to its own scale', () => {  assert.equal(WCards.spark([5], 56, 16, '#fff'), '')
  const svg = WCards.spark([0, 10], 100, 20, '#fff', { pad: 0, fill: false, dot: false })
  assert.match(svg, /points="0,20 100,0"/)
  // A scale wider than the points (0 to 100 for a share).
  assert.match(WCards.spark([50, 50], 100, 20, '#fff', { pad: 0, fill: false, dot: false, lo: 0, hi: 100 }), /points="0,10 100,10"/)
  assert.match(WCards.ring(0.5, 20, 2, '#f00', '#333'), /stroke-dasharray="28\.3 56\.5"/)
  assert.match(WCards.bars([1, 2], 10, 10, '#f00', '#333', 1), /fill="#333"\/><rect [^>]*fill="#f00"/)
})

test("when a letter came, how long the machine has been up, in her language", () => {
  const at = (d, h, m) => new Date(2026, 9, d, h, m).getTime()
  assert.equal(WCards.whenOf(at(16, 9, 5), dark), '09:05')
  assert.equal(WCards.whenOf(at(15, 23, 0), dark), '昨天')
  assert.equal(WCards.whenOf(at(13, 8, 0), en), 'Tue')
  assert.equal(WCards.whenOf(at(2, 8, 0), en), '10/2')
  assert.equal(WCards.whenOf(undefined, dark), '')
  assert.equal(WCards.upFor(273_600, dark), '3 天 4 小时')
  assert.equal(WCards.upFor(4_000, en), '1 h 6 min')
})

test("a stack's line: its glyph in its colour's block, its name, its number and what it counts", () => {
  const row = WCards.stackRowHTML(monitor, '系统监控', dark)
  assert.match(row, /<span class="sgl" style="--c:#34d27b">/)
  assert.match(row, /<b class="sname">系统监控<\/b><span class="sval">17%<small>CPU<\/small><\/span><span class="ssay">CPU 17% · 内存 44%<\/span>/)
  const mail = WCards.stackRowHTML(inbox, '邮件', { ...dark, detailed: false })
  assert.match(mail, /13<small>封未读<\/small>/)
  assert.match(mail, /wpriv hid/)
  assert.equal(WCards.titleOf(monitor, dark), '系统监控')
  assert.equal(WCards.titleOf(stock, dark), '苹果')
  assert.equal(WCards.titleOf(inbox, en), 'Mail')
})

test("a stock's rise takes the colour its script sent, red up or green up, on every surface", () => {
  // Red up (Chinese markets', the Plugins page's 红涨绿跌): the rise red.
  const redUp = { ...stock, color: '#ff5c6c' }
  assert.match(WCards.faceHTML(redUp, 'M', dark), /<i style="color:#ff6b6b">\+0\.80%<\/i>/)
  assert.match(WCards.rowHTML(redUp, 'M', dark), /color:#ff6b6b">230\.40</)
  assert.match(WCards.stackRowHTML(redUp, '苹果', dark), /<small style="color:#ff6b6b">\+0\.80%<\/small>/)
  assert.match(WCards.cardHTML(redUp, dark), /<span class="cchip" style="--c:#ff6b6b">\+0\.80%<\/span>/)
  // Green up: the rise green, as the design draws it.
  assert.match(WCards.faceHTML(stock, 'M', light), /<i style="color:#0f7b3f">\+0\.80%<\/i>/)
  assert.match(WCards.stackRowHTML(stock, '苹果', dark), /<small style="color:#34d27b">/)
  assert.match(WCards.cardHTML(stock, dark), /<span class="cchip" style="--c:#34d27b">\+0\.80%<\/span>/)
})
