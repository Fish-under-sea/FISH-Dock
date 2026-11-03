// 插件页 SDK 的消息契约（T-N / A14，t25 回归）：
// ready 只发一次**且在脚本执行时立刻发**、ready 后当场量一次、同一尺寸不重复发、
// 观察回调在同一帧里合并、解析完再补一次测量。
//
// 为什么「立刻发」是契约而不是实现细节：宿主只有收到 fish:ready 才向下投
// fish:theme；插件自己的经典脚本挂在 body 里，**在 DOMContentLoaded 之前**就
// 跑完了。把 ready 推到 DOMContentLoaded，插件初始化时就拿不到主题
// （真实浏览器 extension-browser 报过 evidence.theme undefined）。
//
// 这里锁协议与时序（不碰浏览器）；真实沙盒/布局仍由真实浏览器验收。
const { test } = require('node:test')
const assert = require('node:assert/strict')
const fs = require('node:fs')
const path = require('node:path')
const vm = require('node:vm')

const TICKET = 'a'.repeat(64)

// 判定一个 promise 在短时间内是「拒绝 / 已送出等回复 / 还挂着」。
// 用短超时而不是 `await assert.rejects(...)`：删掉手势闸门时调用会变成「等宿主回复」
// （夹具里永远不来），直接 await 会把整个测试进程挂住；这里会**快速失败**。
function settleState(promise, ms = 100) {
  return new Promise(resolve => {
    let done = false
    const timer = setTimeout(() => { if (!done) { done = true; resolve('pending') } }, ms)
    promise.then(() => { if (!done) { done = true; clearTimeout(timer); resolve('resolved') } },
                 () => { if (!done) { done = true; clearTimeout(timer); resolve('rejected') } })
  })
}

function boot({ height = 400, readyState = 'complete', bodyAtParse = true } = {}) {
  const posted = [], frames = [], observers = []
  const state = { height, body: bodyAtParse ? { getBoundingClientRect: () => ({ height: state.height }) } : null }
  // #67：可控时钟（外链速率限制看 Date.now）与可控 userActivation（程序化打开的手势门槛）。
  const clock = { now: 1_000_000 }
  const activation = { api: true, active: false }
  const doc = {
    readyState,
    documentElement: { dataset: {}, style: { setProperty() {} } },
    body: state.body,
    querySelector: () => state.body,
    getElementById: () => null,
    addEventListener(type, fn) {
      if (type === 'DOMContentLoaded') doc.domReady = fn
      if (type === 'click') doc.click = fn
    },
  }
  class FakeResizeObserver {
    constructor(fn) { this.fn = fn; observers.push(this) }
    observe(target) { this.target = target }
  }
  const parent = { postMessage: row => posted.push(row) }
  const win = { addEventListener(type, fn) { win.listener = fn }, removeEventListener() {} }
  const navigator = activation.api ? { userActivation: { get isActive() { return activation.active } } } : {}
  vm.runInNewContext(fs.readFileSync(path.join(__dirname, '../../sdk/browser/bridge.js'), 'utf8'), {
    window: win,
    document: doc,
    parent,
    location: { pathname: `/${TICKET}/ui/index.html` },
    ResizeObserver: FakeResizeObserver,
    requestAnimationFrame: fn => { frames.push(fn) },
    navigator,
    Date: { now: () => clock.now },
    URL,
    // 桥的业务请求用 setTimeout 做 20 秒超时；夹具里给个不真正计时的桩
    // （不提供会让 call() 直接在 executor 里抛 ReferenceError，消息也就发不出去）。
    setTimeout: fn => ({ fn }),
    clearTimeout: () => {},
  })
  // 伪造一条页面里的 <a> 与其点击目标（closest 只认 a[href]）。
  const link = (href, extra = {}) => {
    const anchor = { getAttribute: name => (name === 'href' ? href : extra[name] ?? null) }
    return { target: { closest: selector => (selector === 'a[href]' ? anchor : null) }, anchor }
  }
  return {
    posted, observers, win, parent,
    types: () => posted.map(row => row.type),
    layouts: () => posted.filter(row => row.type === 'fish:layout'),
    readies: () => posted.filter(row => row.type === 'fish:ready'),
    setHeight: value => { state.height = value },
    setBody: value => { state.body = value; doc.body = value },
    fireFrame: () => { const run = frames.splice(0); for (const fn of run) fn() },
    resize: () => observers.at(-1)?.fn(),
    domReady: () => doc.domReady?.(),
    // 模拟宿主收到 ready 后投回来的主题消息（与 extension-page.js 的 theme() 同形）。
    theme: mode => win.listener({ source: parent, data: { type: 'fish:theme', ticket: TICKET, title: 'T', mode, colors: { '--dock-bg': '#fff' } } }),
    dockTheme: () => doc.documentElement.dataset.dockTheme,
    // #67 helpers
    link,
    advance: ms => { clock.now += ms },
    setActivation: active => { activation.active = active },
    dropActivationApi: () => Object.defineProperty(navigator, 'userActivation', { value: undefined, configurable: true }),
    click: (href, overrides = {}) => {
      let prevented = 0
      const fake = { isTrusted: true, target: link(href).target, preventDefault: () => { prevented++ }, ...overrides }
      doc.click?.(fake)
      return prevented
    },
    opens: () => posted.filter(row => row.type === 'fish:call' && row.method === 'dock.open_external'),
  }
}

test('ready 在脚本执行时立刻发（不等 DOMContentLoaded），且只有一条', () => {
  const bridge = boot({ readyState: 'loading', bodyAtParse: false })
  assert.equal(bridge.readies().length, 1, '解析期就必须发 ready：插件自己的脚本再跑之前宿主已拿到')
  assert.equal(bridge.layouts().length, 0, '此刻 main 还没出现，不发 layout')
})

test('宿主收到 ready 就投主题：早于 DOMContentLoaded 的初始化代码能读到 dockTheme', () => {
  const bridge = boot({ readyState: 'loading', bodyAtParse: false })
  bridge.theme('light')                       // 宿主对 ready 的回应
  assert.equal(bridge.dockTheme(), 'light', '这就是 evidence.theme 读到的值')
  bridge.theme('dark')
  assert.equal(bridge.dockTheme(), 'dark', '主题可以再改')
})

test('DOMContentLoaded 只补测量与观察器，ready 不重复发', () => {
  const bridge = boot({ readyState: 'loading', bodyAtParse: false })
  bridge.setBody({ getBoundingClientRect: () => ({ height: 600 }) })
  bridge.domReady()
  assert.equal(bridge.readies().length, 1, 'ready 始终只有一条')
  assert.deepEqual(bridge.layouts().map(row => [row.height, row.layout]), [[600, 'single']], '解析完补一次真实高度')
  assert.equal(bridge.observers.length, 1, '观察器只挂一次')
})

test('文档一开始就绪时：一条 ready + 一条当场量到的 layout', () => {
  const bridge = boot({ height: 400 })
  assert.deepEqual(bridge.types(), ['fish:ready', 'fish:layout'], 'ready 之后立刻量一次')
  const [layout] = bridge.layouts()
  assert.equal(layout.height, 400)
  assert.equal(layout.layout, 'single')
  assert.equal(layout.ticket, TICKET, '消息要带本页票据')
})

test('同一尺寸不再重复发：观察器连抖几次也只有一条', () => {
  const bridge = boot({ height: 400 })
  bridge.resize(); bridge.fireFrame()
  bridge.resize(); bridge.fireFrame()
  assert.equal(bridge.layouts().length, 1, '尺寸没变就不发')
})

test('同一帧里多次观察回调合并成一次测量与一条消息', () => {
  const bridge = boot({ height: 400 })
  bridge.setHeight(500)
  bridge.resize(); bridge.resize(); bridge.resize()
  assert.equal(bridge.layouts().length, 1, '帧内合并：还没到帧就不发')
  bridge.fireFrame()
  assert.equal(bridge.layouts().length, 2, '一帧只补一条')
  assert.equal(bridge.layouts().at(-1).height, 500)
})

test('布局宽窄切换会重新上报，非法值忽略', () => {
  const bridge = boot({ height: 400 })
  bridge.win.FishDock.setLayout('double')
  bridge.fireFrame()
  assert.equal(bridge.layouts().at(-1).layout, 'double')
  const before = bridge.layouts().length
  bridge.win.FishDock.setLayout('triple')
  bridge.fireFrame()
  assert.equal(bridge.layouts().length, before, '非 single/double 不接受')
})

test('高度为 0 时先不发 layout，出现高度后补发', () => {
  const bridge = boot({ height: 0 })
  assert.deepEqual(bridge.types(), ['fish:ready'], '只报 ready')
  bridge.setHeight(260)
  bridge.resize(); bridge.fireFrame()
  assert.equal(bridge.layouts().length, 1)
  assert.equal(bridge.layouts()[0].height, 260)
})

// ── #67：插件页外链走宿主受控通道 ─────────────────────────────────────────
// iframe 沙箱只有 allow-scripts（不加 allow-popups/allow-top-navigation），页面里的
// <a href>、target=_blank 自己都开不了浏览器。桥只把**用户真点了**的 http/https
// 链接交给宿主；危险协议不拦不打开；脚本自动触发需要用户手势 + 2 秒节流。
test('真实点击 https 链接：拦掉默认导航并交给宿主打开', () => {
  const bridge = boot()
  const prevented = bridge.click('https://example.com/docs')
  assert.equal(prevented, 1, '可信点击要被 preventDefault')
  const [open] = bridge.opens()
  assert.ok(open, '必须发出 dock.open_external')
  assert.equal(open.params.url, 'https://example.com/docs')
  assert.equal(open.ticket, TICKET, '带本页票据才能过宿主校验')
})

test('target=_blank 的链接同样走宿主（沙箱里本来就开不了新窗口）', () => {
  const bridge = boot()
  bridge.click('https://example.com/new')
  assert.equal(bridge.opens()[0].params.url, 'https://example.com/new')
})

test('非可信点击不打开：脚本 dispatch 的假点击一律忽略', () => {
  const bridge = boot()
  const prevented = bridge.click('https://example.com/x', { isTrusted: false })
  assert.equal(prevented, 0, '不拦默认行为')
  assert.equal(bridge.opens().length, 0, '不发给宿主')
})

test('危险协议与畸形地址：不拦、不打开（file/javascript/data/vbscript/userinfo/NUL）', () => {
  const bridge = boot()
  for (const href of ['file:///C:/Windows/notepad.exe', 'javascript:alert(1)', 'data:text/html,x', 'vbscript:msgbox(1)', 'https://user:pw@example.com/', 'https://example.com/\0', 'mailto:a@b.c', '#anchor', '']) {
    assert.equal(bridge.click(href), 0, `${href} 不该被拦`)
  }
  assert.equal(bridge.opens().length, 0, '都不该发给宿主')
})

test('程序化 openExternal：没有用户手势就拒绝（且不发消息），绝不静默放宽', async () => {
  const bridge = boot()
  assert.equal(await settleState(bridge.win.FishDock.openExternal('https://example.com/x')), 'rejected', '无手势必须当场拒绝')
  assert.equal(bridge.opens().length, 0, '被拒绝就不能发出 fish:call')
  bridge.setActivation(true)
  // 手势满足时消息会发出，但要等宿主回 fish:result 才 settle —— 这里断言「已送出、等回复」。
  assert.equal(await settleState(bridge.win.FishDock.openExternal('https://example.com/x')), 'pending', '有手势时应当发出并等宿主回复')
  assert.equal(bridge.opens()[0].params.url, 'https://example.com/x')
})

test('不支持 userActivation 的环境：程序化一律拒绝（只信真实点击）', async () => {
  const bridge = boot()
  bridge.dropActivationApi()
  assert.equal(await settleState(bridge.win.FishDock.openExternal('https://example.com/x')), 'rejected')
  assert.equal(bridge.opens().length, 0)
  bridge.click('https://example.com/y')
  assert.equal(bridge.opens().length, 1, '真实点击这条路不依赖 userActivation')
})

// t50 F2：节流按 URL 去重（连点同一个链接只开一次），不同链接不该被误伤。
test('同一链接 2 秒内只开一次；连续点两个不同链接都能打开', () => {
  const bridge = boot()
  bridge.click('https://example.com/a')
  bridge.click('https://example.com/a')
  assert.equal(bridge.opens().length, 1, '同一 URL 连点只开一次')
  bridge.click('https://example.com/b')
  assert.equal(bridge.opens().length, 2, '不同链接不受同一链接的节流影响')
  assert.equal(bridge.opens()[1].params.url, 'https://example.com/b')
  bridge.advance(1_999)
  bridge.click('https://example.com/a')
  assert.equal(bridge.opens().length, 2, '同一个链接仍在 2 秒内')
  bridge.advance(1)
  bridge.click('https://example.com/a')
  assert.equal(bridge.opens().length, 3, '满足 2 秒后同一链接可再开')
})

// t50 F1：对外的 call() 拒绝保留方法——否则脚本能绕过手势与节流直接开浏览器。
test('对外 call 拒绝 dock.open_external：无手势/有手势都不发消息', async () => {
  const bridge = boot()
  assert.equal(await settleState(bridge.win.FishDock.call('dock.open_external', { url: 'https://example.com/x' })), 'rejected')
  assert.equal(bridge.opens().length, 0, '被拒绝就不能发出 fish:call')
  bridge.setActivation(true)
  assert.equal(await settleState(bridge.win.FishDock.call('dock.open_external', { url: 'https://example.com/x' })), 'rejected', '有手势也只能走 openExternal')
  assert.equal(bridge.opens().length, 0)
  // 其它方法仍可正常调用（只禁这一条，不影响 dock.autostart 这类既有能力）。
  bridge.win.FishDock.call('dock.autostart', { autostart: true }).catch(() => {})
  assert.equal(bridge.posted.filter(row => row.type === 'fish:call' && row.method === 'dock.autostart').length, 1, '保留名单只挡 open_external')
})

// t50 F3：scheme 归一后判定——大写 HTTPS:// 也要识别，并按归一后的地址发出。
test('大写 HTTPS:// 的链接同样被识别，发出的地址是归一化后的', () => {
  const bridge = boot()
  bridge.click('HTTPS://Example.COM/Path?Q=1')
  const [open] = bridge.opens()
  assert.ok(open, '大写 scheme 也要识别')
  assert.equal(open.params.url, 'https://example.com/Path?Q=1', '发出的是归一化地址')
})

test('openExternal 只接受 http/https；FishDock 仍冻结且保留原接口', async () => {
  const bridge = boot()
  bridge.setActivation(true)
  await assert.rejects(bridge.win.FishDock.openExternal('file:///C:/x'), /只支持 http\/https/)
  assert.equal(Object.isFrozen(bridge.win.FishDock), true)
  assert.equal(typeof bridge.win.FishDock.call, 'function')
  assert.equal(typeof bridge.win.FishDock.setLayout, 'function')
})