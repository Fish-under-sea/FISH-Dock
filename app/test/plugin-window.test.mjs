// 2.2.0 插件外部窗口：宿主容器页、页面桥与「移除硬编码子岛」的回归。
//
// 这些用例钉的是**可观察契约**：
//   · 容器页只加载票据给它的那一页，并把页面桥/事件转发给**这个插件**的帧；
//   · 浏览器 SDK 的 windows / fileDrop 真发协议方法、真按票据收发、Escape 走宿主出口；
//   · 主岛源码与样式里不再有 stash 子岛（没有小球、没有命中区、没有固定插件 ID）。
import test from 'node:test'
import assert from 'node:assert/strict'
import fs from 'node:fs'
import path from 'node:path'
import vm from 'node:vm'
import { fileURLToPath } from 'node:url'

const here = path.dirname(fileURLToPath(import.meta.url))
const root = path.join(here, '..')
const read = (...parts) => fs.readFileSync(path.join(root, ...parts), 'utf8')

// —— 浏览器 SDK（sdk/browser/bridge.js）在最小 vm 里跑起来 ——
const TICKET = 'a'.repeat(64)

function sdk() {
  const posted = []
  const listeners = { message: [], keydown: [] }
  const window = {
    addEventListener(type, fn) { (listeners[type] ||= []).push(fn) },
    removeEventListener(type, fn) { listeners[type] = (listeners[type] || []).filter(x => x !== fn) },
    setTimeout, clearTimeout,
  }
  const document = {
    addEventListener(type, fn) { (listeners[type] ||= []).push(fn) },
    documentElement: { style: { setProperty() {} }, dataset: {} },
    getElementById: () => null,
    querySelector: () => null,
    body: { getBoundingClientRect: () => ({ height: 0 }) },
    readyState: 'complete',
  }
  const parent = { postMessage(row) { posted.push(row) } }
  const context = { window, document, parent, location: { pathname: `/${TICKET}/page.html` }, setTimeout, clearTimeout, console, Map, Set, Object, Error, String, Number, RegExp, JSON, TypeError, Promise }
  vm.createContext(context)
  vm.runInContext(read('..', 'sdk', 'browser', 'bridge.js'), context)
  const api = window.FishDock
  const fromParent = row => { for (const fn of listeners.message) fn({ data: row, source: parent }) }
  const escape = () => { for (const fn of listeners.keydown) fn({ key: 'Escape' }) }
  const answer = (id, result) => fromParent({ type: 'fish:result', ticket: TICKET, id, result })
  // 只看「页面请求」：bridge.js 启动时还会发一条 fish:ready，别混进请求计数。
  const calls = () => posted.filter(row => row.type === 'fish:call')
  return { api, posted, calls, fromParent, answer, escape, listeners }
}

// vm 里造的对象与测试文件里的对象原型不同，比较前先如实转成 JSON 形状。
const plain = value => JSON.parse(JSON.stringify(value))
const until = async (predicate, label) => {
  for (let i = 0; i < 200; i++) { if (predicate()) return; await new Promise(resolve => setImmediate(resolve)) }
  throw new Error(`等待超时：${label}`)
}

test('浏览器 SDK：windows / fileDrop 真发协议方法，句柄由宿主给', async () => {
  const f = sdk()
  assert.equal(typeof f.api.windows.open, 'function')
  assert.equal(typeof f.api.windows.onEvent, 'function')
  assert.equal(typeof f.api.fileDrop.subscribe, 'function')

  const opening = f.api.windows.open({ key: 'panel', page: 'main' })
  await until(() => f.calls().length === 1, 'open 发出')
  const open = f.calls()[0]
  assert.equal(open.method, 'dock.windows.open')
  assert.equal(open.ticket, TICKET)
  assert.deepEqual(plain(open.params), { key: 'panel', page: 'main' })
  f.answer(open.id, { handle: 'h-1', key: 'panel', ready: true })
  assert.equal(await opening, 'h-1', 'open 返回宿主的 opaque handle')

  const closing = f.api.windows.close('h-1')
  await until(() => f.calls().length === 2, 'close 发出')
  const close = f.calls()[1]
  assert.equal(close.method, 'dock.windows.close')
  assert.deepEqual(plain(close.params), { handle: 'h-1' })
  f.answer(close.id, { closed: true })
  assert.equal(await closing, true)

  const subscribing = f.api.fileDrop.subscribe({ scope: 'dock' })
  await until(() => f.calls().length === 3, 'subscribe 发出')
  const subscribe = f.calls()[2]
  assert.equal(subscribe.method, 'dock.fileDrop.subscribe')
  assert.deepEqual(plain(subscribe.params), { scope: 'dock' })
  f.answer(subscribe.id, { handle: 'drop-1', scope: 'dock' })
  assert.equal(await subscribing, 'drop-1')
})

test('浏览器 SDK：空/伪造句柄在本地就拒绝，不发请求', async () => {
  const f = sdk()
  for (const bad of ['', null, 42, undefined]) {
    await assert.rejects(f.api.windows.update(bad, { width: 300 }), /句柄无效/)
    await assert.rejects(f.api.windows.close(bad), /句柄无效/)
    await assert.rejects(f.api.fileDrop.unsubscribe(bad), /句柄无效/)
  }
  assert.equal(f.calls().length, 0, '参数不合格时一条请求都不该发')
})

test('浏览器 SDK：事件只按票据接收并分发给对应监听器', async () => {
  const f = sdk()
  const seen = [], dropped = []
  f.api.windows.onEvent(row => seen.push(row))
  f.api.fileDrop.onEvent(row => dropped.push(row))
  // 别的票据发来的事件必须被忽略。
  f.fromParent({ type: 'fish:window-event', ticket: 'b'.repeat(64), event: 'ready', handle: 'h' })
  assert.equal(seen.length, 0)
  f.fromParent({ type: 'fish:window-event', ticket: TICKET, event: 'ready', handle: 'h-1' })
  assert.equal(seen.length, 1)
  f.fromParent({ type: 'fish:window-event', ticket: TICKET, event: 'fileDrop', phase: 'drop', paths: ['x'] })
  assert.equal(seen.length, 2, '窗口监听器也收到 fileDrop（插件自己分辨）')
  assert.equal(dropped.length, 1)
  assert.deepEqual(dropped[0].paths, ['x'])
})

test('浏览器 SDK：Escape 交给宿主关闭（透明浮窗没有原生关闭按钮）', () => {
  const f = sdk()
  f.escape()
  const escape = f.posted.find(row => row.type === 'fish:escape')
  assert.ok(escape, 'Escape 必须发给父页面（宿主出口）')
  assert.equal(escape.ticket, TICKET)
})

// —— 设置面板里的插件页：窗口事件只转给同一插件的帧 ——
function panel() {
  const calls = []
  const listeners = {}
  const tickets = []
  const element = () => {
    const node = {
      style: {}, dataset: {}, children: [], removed: false, posted: [],
      contentWindow: { postMessage(row) { node.posted.push(row) } },
      classList: { add() {}, remove() {}, toggle() {}, contains() { return false } },
      setAttribute() {}, appendChild(child) { node.children.push(child); return child }, append(...items) { node.children.push(...items) },
      replaceChildren(...items) { node.children = items.slice() }, insertBefore(child) { node.children.unshift(child); return child },
      replaceWith() { node.removed = true }, remove() { node.removed = true }, querySelectorAll() { return [] },
      getBoundingClientRect() { return { height: 10, width: 10 } },
    }
    return node
  }
  const body = element()
  const document = { createElement: element, body, getElementById() { return null }, documentElement: element() }
  const hostWindow = {
    addEventListener(type, fn) { listeners[type] = fn },
    removeEventListener(type, fn) { if (listeners[type] === fn) delete listeners[type] },
    getComputedStyle() { return { getPropertyValue() { return '' } } },
    __TAURI__: { event: { listen: async (name, handler) => { listeners[`tauri:${name}`] = handler; return () => { delete listeners[`tauri:${name}`] } } } },
  }
  const invoke = async command => {
    calls.push(command)
    if (command === 'extension_page') { const ticket = 'c'.repeat(63) + String(calls.length); tickets.push(ticket); return { url: `http://fish-plugin.localhost/${ticket}/page.html`, ticket, generation: 7 } }
    return null
  }
  return { document, hostWindow, invoke, calls, listeners }
}

async function loadPanel() {
  const window = {}
  vm.runInNewContext(read('src', 'pet', 'extension-page.js'), { window, setTimeout, clearTimeout, URL, console })
  return window.ExtensionPage
}

test('设置面板插件页：外部窗口事件只转给同一插件的帧', async () => {
  const ExtensionPage = await loadPanel()
  const f = panel()
  const page = ExtensionPage.create({ invoke: f.invoke, lang: () => 'zh', hostWindow: f.hostWindow, document: f.document })
  const others = f.document.createElement('div'), mine = f.document.createElement('div')
  await page.enter(others, { key: 'fish.clash/main', title: 'Clash', digest: 'd1' })
  await page.enter(mine, { key: 'acme.weather/main', title: '天气', digest: 'd2' })
  await until(() => f.listeners['tauri:plugin:window-event'], '事件桥挂上')
  const frameOf = key => f.document.body.children.find?.(() => false)
  // 从帧表里取那一页的 iframe：enter 之后 root 的 children 只有一个 outer。
  f.listeners['tauri:plugin:window-event']({ payload: { plugin: 'acme.weather', generation: 7, event: 'ready', handle: 'h-1' } })
  // 只有 acme.weather 那一帧收到。
  const frames = [...f.document.body.children, ...[...f.document.body.children]]
  void frames
  const sent = []
  for (const root of [others, mine]) {
    for (const child of root.children) {
      for (const posted of child.posted || []) sent.push(posted)
      for (const grand of child.children || []) for (const posted of grand.posted || []) sent.push(posted)
    }
  }
  const received = sent.filter(row => row && row.type === 'fish:window-event')
  assert.equal(received.length, 1, '只转给同一插件的帧')
  assert.equal(received[0].event, 'ready')
  // 别的插件的事件一条都不转。
  f.listeners['tauri:plugin:window-event']({ payload: { plugin: 'fish.magpie', event: 'ready', handle: 'h-2' } })
  assert.equal(sent.filter(row => row && row.type === 'fish:window-event').length, 1)
  const ownFrame = mine.children[0].children[0]
  const before = ownFrame.posted.length
  f.listeners['tauri:plugin:window-event']({ payload: { plugin: 'acme.weather', generation: 6, event: 'fileDrop', paths: ['synthetic-old-session'] } })
  assert.equal(ownFrame.posted.length, before, '旧会话路径不得交给同 ID 的新页面')
  page.dispose()
})

test('设置面板销毁后迟到注册的监听器立即释放', async () => {
  const ExtensionPage = await loadPanel()
  const f = panel()
  let complete, stopped = 0
  f.hostWindow.__TAURI__.event.listen = () => new Promise(resolve => { complete = resolve })
  const page = ExtensionPage.create({ invoke: f.invoke, lang: () => 'zh', hostWindow: f.hostWindow, document: f.document })
  page.dispose()
  complete(() => { stopped++ })
  await until(() => stopped === 1, '销毁后释放迟到监听器')
  page.dispose()
  assert.equal(stopped, 1, '释放保持幂等')
})

// —— 移除硬编码子岛：源码与样式里不再有它 ——
test('移除宿主硬编码子岛：没有 stashSub、小球、命中区与固定插件 ID', () => {
  const island = read('src', 'pet', 'island.js')
  const css = read('src', 'pet', 'style.css')
  for (const forbidden of ['stashSub', 'StashPanel', 'stash-sub', 'ssub-', 'ssub-chips', 'jassdingk.filemgr']) {
    assert.ok(!island.includes(forbidden), `island.js 不该再有 ${forbidden}`)
    assert.ok(!css.includes(forbidden), `style.css 不该再有 ${forbidden}`)
  }
  assert.ok(!island.includes("'pet:drag-drop'"), '不再监听专用拖入事件（改走通用 fileDrop 订阅）')
  assert.ok(!island.includes('pet:pet-rect'), '不再为子岛小球推送 pet 小圆窗口位置')
  // 主岛自己的布局与一般拖放桥仍在：只删子岛，不砍主岛能力。
  assert.ok(island.includes('window.pet.onFileDrag'), '一般文件拖动预览保留')
})

test('窗口生命周期接线：全局隐藏先于主岛早退，拖入服务独立启动，显示快照驱动重排', () => {
  const island = read('src-tauri', 'src', 'island.rs')
  const apply = island.slice(island.indexOf('pub fn apply_visibility('), island.indexOf('fn over_full('))
  assert.ok(apply.indexOf('set_dock_hidden(!sh.is_visible())') >= 0)
  assert.ok(apply.indexOf('set_dock_hidden(!sh.is_visible())') < apply.indexOf('show_or_hide(sh)'))
  assert.ok(!island.includes('dropwin::start('), '不再依赖主岛创建来启动接收线程')
  const main = read('src-tauri', 'src', 'main.rs')
  assert.ok(main.indexOf('dropwin::start(') > main.indexOf('plugin_window::install('))
  assert.ok(main.includes('previous.primary != screens.primary'))
  assert.ok(main.includes('plugin_window::screens_changed()'))
  const tray = read('src-tauri', 'src', 'tray.rs')
  const close = tray.slice(tray.indexOf('"closePluginWindows" =>'), tray.indexOf('// Quit by the person'))
  assert.ok(close.includes('close_windows("user")'))
  assert.ok(!close.includes('close_all('))
})

test('主岛拖放检测不再用虚构的 500×36 矩形', () => {
  const source = read('src-tauri', 'src', 'island.rs')
  const production = source.split('#[cfg(test)]')[0]
  assert.ok(!production.includes('(500.0, 36.0)'), 'closed_rect 不再用默认虚构尺寸兜底')
})