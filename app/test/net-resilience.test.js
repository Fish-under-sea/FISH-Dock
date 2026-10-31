// 弱网/无梯子韧性（t4）：四条单测，每条对着一个行为面。
//   超时       —— 关于页公告的 8 秒兜底：loading 超过 8 秒换成「超时 + 重试」。
//   缓存命中   —— 图库离线命中缓存（got.offline）：列表照画，标「离线」，不写红字错误。
//   离线降级   —— 宿主 ok=false（连缓存都没有）：一句人话错误 + 可再点的重试，不是死转。
//   重试退避   —— GitHub 自动复核失败后按 60 秒退避重排，且不会无限连发。
// 全部用 vm 切源码跑（同 about-update / password-eye 的做法），不碰真实网络。
const assert = require('node:assert/strict')
const fs = require('node:fs')
const path = require('node:path')
const vm = require('node:vm')
const { test } = require('node:test')
const { t } = require('../src/shared/i18n')

const source = fs.readFileSync(path.join(__dirname, '../src/pet/settings.js'), 'utf8')

// —— 关于页公告的 8 秒兜底（超时） ————————————————————————————————
// notesKind 是「该画哪一态」的唯一裁判；aboutNotes 是文案。分两段切出来测，
// 不拖整页件（aboutHTML 依赖 aboutHero 等一整页东西）。
// 切片按「下一个 function」收尾，不靠注释文字——注释一改（t30 瘦身就改过），
// 按注释切的片段会切进别的函数，跑出 SyntaxError 而不是断言失败。
const NEXT_FN = from => source.indexOf('\n  function ', from)
const NOTES_KIND_BLOCK = source.slice(source.indexOf('  function notesKind(notes)'), NEXT_FN(source.indexOf('  function notesKind(notes)')))
const NOTES_HTML_BLOCK = source.slice(source.indexOf('  function aboutNotes(notes)'), source.indexOf('\n  // hero：品牌图', source.indexOf('  function aboutNotes(notes)')))
const notesKind = (state, timedOut) => vm.runInNewContext(`${NOTES_KIND_BLOCK}\n;notesKind(notes)`, {
  notesTimedOut: timedOut,
  notes: { state, entries: [] },
  NOTES_HAVE: 'have', NOTES_EMPTY: 'empty', NOTES_FAILED: 'failed', NOTES_LOADING: 'loading',
})
const notesHTML = (state, timedOut, kind) => vm.runInNewContext(`${NOTES_HTML_BLOCK};const notes = { state: ${JSON.stringify(state)}, entries: [] };aboutNotes(notes)`, {
  lang: 'zh', notesTimedOut: timedOut, esc: x => String(x),
  notesKind: () => kind,
  NOTES_HAVE: 'have', NOTES_EMPTY: 'empty', NOTES_FAILED: 'failed', NOTES_LOADING: 'loading',
  noteRow: () => '', notesSkeleton: () => '',
})

test('超时：公告读取超过 8 秒不再永远转，换成「超时 + 重试」', () => {
  assert.equal(notesKind('loading', false), 'loading', '8 秒内还是「正在读取」')
  assert.equal(notesKind('loading', true), 'failed', '过了 8 秒按读不到处理，不再永远转')
  const late = notesHTML('loading', true, 'failed')
  assert.match(late, /超时/, '超时后说出「超时」')
  assert.doesNotMatch(late, /正在读取更新公告/, '不再假装还在读')
  assert.match(late, /data-update="notes"/, '重试按钮在')
  const early = notesHTML('loading', false, 'loading')
  assert.match(early, /正在读取更新公告/, '8 秒内先给「正在读取」')
  assert.match(early, /aria-busy="true"/, '读取中有忙碌标记')
})

// —— 图库（缓存命中 / 离线降级） ————————————————————————————————————
// pagePets 与 loadGallery 都是 IIFE 内的函数：按边界切出来，配最小的假环境。
function galleryPage() {
  const context = vm.createContext({
    gallery: { sort: 'popular', query: '', items: [], page: 0, totalPages: 1, total: 0, loading: false, error: '', offline: false, getting: '', asked: 0 },
    snap: { settings: { pet: '' }, pets: [] },
    fetchNote: { text: '', isError: false },
    fetching: null,
    typedRef: '',
    isLink: () => false,
    thumb: () => '',
    Widgets: { icon: x => x },
    T: (key, vars) => t('zh', key, vars),
    esc: x => String(x).replace(/[&<>"]/g, c => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;' })[c]),
    sec: x => x,
    seg: () => '',
    draw: () => {},
    relayout: () => {},
  })
  const at = marker => source.indexOf(marker)
  const pagePets = source.slice(at('  function pagePets()'), source.indexOf('\n  // --- Her island\'s look', at('  function pagePets()')))
  const loadGallery = source.slice(at('  async function loadGallery('), source.indexOf('\n\n  // The gallery as a search finds it'))
  vm.runInContext(`${pagePets}\n${loadGallery}\nthis.pagePets = pagePets; this.loadGallery = loadGallery;`, context)
  return context
}

test('缓存命中：离线时宿主给 offline=true，列表照画并标「离线」，不当成错误', async () => {
  const page = galleryPage()
  const calls = []
  page.window = { pet: { settings: { gallery: async (...args) => {
    calls.push(args)
    return { ok: true, offline: true, error: '离线，先用上次的图库（连不上：refused）', items: [{ id: 'claude-chan', name: 'Claude', author: 'a', likes: 1, preview: '' }], page: 1, totalPages: 1, total: 1 }
  } } } }
  await page.loadGallery(false)
  assert.equal(calls.length, 1)
  assert.equal(page.gallery.loading, false, '请求结束了，不是永远 loading')
  assert.equal(page.gallery.offline, true, 'offline 状态被记下')
  assert.equal(page.gallery.error, '', '离线缓存不是红字错误')
  const html = page.pagePets()
  assert.match(html, /离线：先显示上次的图库/, '页面上标出「离线/缓存」')
  assert.match(html, /Claude/, '缓存的列表真的画出来了')
  assert.doesNotMatch(html, /加载不了图库/, '不走错误文案')
})

test('离线降级：连缓存都没有时给一句人话错误，且能再次发起加载（可重试）', async () => {
  const page = galleryPage()
  let asked = 0
  page.window = { pet: { settings: { gallery: async () => {
    asked += 1
    if (asked === 1) return { ok: false, error: '连不上：refused' }
    return { ok: true, offline: false, items: [], page: 1, totalPages: 1, total: 0 }
  } } } }
  await page.loadGallery(false)
  const failed = page.pagePets()
  assert.match(failed, /加载不了图库：连不上：refused/, '失败要可见，有具体原因')
  assert.equal(page.gallery.loading, false, '失败了也要停，不许永远 loading')
  assert.equal(asked, 1, '第一次失败后不会自动连发第二次')
  // 再次加载（用户点了重试 / 再切回页面）：第一次失败不落 page 号，重试不被拦。
  await page.loadGallery(false)
  assert.equal(asked, 2, '重试真的发起了第二次请求')
  assert.equal(page.gallery.error, '', '成功后错误不再挂着')
  const recovered = page.pagePets()
  assert.doesNotMatch(recovered, /加载不了图库/, '页面不再显示旧错误')
})

// —— GitHub 自动复核的退避（重试退避） ————————————————————————————————
// 复用 extension-panel 的测试基建：网络失败后 authRetryAt = now + 60s，
// 下一次自动检查排在至少 60 秒之后，而不是立即再来一次（无限重试）。
function makePanel({ initialStar, auth } = {}) {
  const window = {}, timers = new Map(); let sequence = 0, star = { authenticated: true, verified: true, configured: true, ...initialStar }
  vm.runInNewContext(fs.readFileSync(path.join(__dirname, '../src/pet/extension-panel.js'), 'utf8'), { window, Date, setTimeout: (fn, ms) => { const id = ++sequence; timers.set(id, { fn, ms }); return id }, clearTimeout: id => timers.delete(id) })
  const calls = []
  const panel = window.ExtensionPanel.create({ deviceDialog: () => {}, syncAuthFocus: async () => {}, lang: () => 'zh', redraw() {}, confirm: () => true, invoke: async (command, args) => {
    calls.push({ command, args })
    if (command === 'extension_view') return { installed: [], star, runtime: null, node: null, root: 'X:/.fish-dock' }
    if (command === 'extension_auth') { star = await auth(args); return star }
  } })
  return { panel, calls, timers }
}

test('重试退避：自动复核失败后按 ≥60 秒退避排下一次，不无限连发', async () => {
  const { panel, calls, timers } = makePanel({ initialStar: { login: 'fixture-user', needsRefresh: true }, auth: async () => { throw Error('合成网络超时') } })
  await panel.load()
  // scheduleAuth 排的首次自动复核（needsRefresh=true → 1 秒后 check）。
  assert.equal(timers.size, 1, '初始排了一次自动复核')
  const [id, first] = [...timers][0]
  assert.ok(first.ms >= 1000, `首次排程在 1 秒外（实际 ${first.ms}ms）`)
  timers.clear()
  await first.fn()
  assert.ok(calls.some(c => c.command === 'extension_auth' && c.args.action === 'check'), '第一次自动复核已发起')
  assert.match(panel.render(), /登录资料已保留/, '失败时保留界面并说明会重试')
  assert.ok(timers.size >= 1, '失败后按退避又排了一次')
  const backoffs = [...timers.values()].map(x => x.ms)
  assert.ok(backoffs.every(ms => ms >= 59000), `退避间隔必须 ≥60 秒（实际 ${backoffs}ms），不许立即重试`)
})
