// The Clash page's kernel lives in a frame in a layer of its own (#clash-host,
// clash.js), with the dock's own bar above it: the system proxy switch and the
// kernel's start and stop, neither of which zashboard can do. The layer is
// handed to the window as a solid rect and parked off screen exactly as
// models.js's is — this file checks that bookkeeping with the same stand-in
// page (no DOM: what is checked is what clash.js says and where it puts it).
const assert = require('node:assert/strict')
const fs = require('node:fs')
const path = require('node:path')
const vm = require('node:vm')
const { test } = require('node:test')

const { solid } = require('../src/pet/surfaces')

const PET = path.join(__dirname, '..', 'src', 'pet')

// pointer.rs look: a point in page px on one of the rects (x, y, w, h).
const covered = (rects, x, y) => rects.some(r => x >= r[0] && x <= r[0] + r[2] && y >= r[1] && y <= r[1] + r[3])

function node(tag) {
  const el = {
    tagName: String(tag).toUpperCase(),
    children: [],
    parent: null,
    id: '',
    className: '',
    style: {},
    dataset: {},
    hidden: false,
    innerHTML: '',
    textContent: '',
    src: '',
    box: null,
    connected: true,
    listeners: {},
    queries: {},
    appendChild(child) {
      child.parent = el
      el.children.push(child)
      return child
    },
    remove() {
      const at = el.parent ? el.parent.children.indexOf(el) : -1
      if (at >= 0) el.parent.children.splice(at, 1)
      el.parent = null
    },
    addEventListener(type, fn) {
      ;(el.listeners[type] ||= []).push(fn)
    },
    fire(type, event = {}) {
      for (const fn of el.listeners[type] || []) fn({ type, relatedTarget: null, ...event })
    },
    setAttribute() {},
    // clash.js 会在运行时查自己动态建出来的子元素（动作条按钮），替身要会沿树找。
    querySelector(selector) {
      if (el.queries[selector]) return el.queries[selector]
      const scoped = selector.startsWith(':scope > .') && selector.slice(10)
      if (scoped) return el.children.find(c => c.className.split(/\s+/).includes(scoped)) || null
      const byClass = selector.startsWith('.') && selector.slice(1)
      const wantData = selector.startsWith('[data-') ? selector.slice(6, -1) : null
      const camel = wantData && wantData.replace(/-(\w)/g, (_, c) => c.toUpperCase())
      for (const child of el.children) {
        if (byClass && child.className.split(/\s+/).includes(byClass)) return child
        if (camel && child.dataset[camel]) return child
        const found = child.querySelector(selector)
        if (found) return found
      }
      return null
    },
    getBoundingClientRect() {
      if (el.box) return el.box
      const px = value => parseFloat(value) || 0
      return { left: px(el.style.left), top: px(el.style.top), width: px(el.style.width), height: px(el.style.height) }
    },
  }
  el.classList = {
    add(name) {
      if (!el.classList.contains(name)) el.className = `${el.className} ${name}`.trim()
    },
    remove(name) {
      el.className = el.className
        .split(/\s+/)
        .filter(c => c && c !== name)
        .join(' ')
    },
    contains: name => el.className.split(/\s+/).includes(name),
  }
  Object.defineProperty(el, 'isConnected', {
    get: () => {
      let at = el
      while (at.parent) at = at.parent
      return at.connected === true
    },
  })
  return el
}

// The page with the settings open on the Clash page. `kernel: 'none'` makes
// clash_ensure fail, to see the failed state and its retry.
function page({ open = true, box = { left: 200, top: 100, width: 1120, height: 760 }, kernel = 'ready' } = {}) {
  const document = {
    body: node('body'),
    createElement: tag => node(tag),
    querySelector: () => null,
    addEventListener: (type, fn) => (document.listeners[type] ||= []).push(fn),
    listeners: {},
    fire: (type, event = {}) => {
      for (const fn of document.listeners[type] || []) fn({ type, ...event })
    },
  }
  document.body.connected = true
  const slot = node('div')
  slot.box = box
  const queries = { '#island-settings .s-clash-slot': slot }
  // '#island-settings .x' 的后半段才是类名：先查预置，再沿 body 子树找动态建出来的。
  document.querySelector = selector => queries[selector] || document.body.querySelector(selector.replace(/^#island-settings\s+/, '')) || null

  const calls = []
  const window = {
    location: { origin: 'http://tauri.localhost' },
    Settings: { isOpen: () => open },
    Isle: {
      sendSolid: () => calls.push(['sendSolid']),
      layerHover: on => calls.push(['layerHover', on]),
    },
    __TAURI__: {
      core: {
        invoke: async (command, args) => {
          calls.push(['invoke', command, args])
          if (command === 'clash_ensure' && kernel === 'none') throw { code: 'KERNEL_MISSING', message: '未找到 mihomo 内核' }
          if (command === 'clash_sysproxy' && args?.enable === false) return { on: false, mixed: 7899 }
          if (command === 'clash_ensure') return { url: 'http://127.0.0.1:9099/ui/?secret=s&disableUpgradeCore=1', version: 'v1.19.33', mixed: 7899, controller: 9099 }
          if (command === 'clash_state') return { running: kernel === 'ready', version: 'v1.19.33', mixed: 7899, controller: 9099, sysproxy: false }
          if (command === 'clash_sysproxy') return { on: args?.enable !== false, mixed: 7899 }
          return null
        },
      },
    },
    addEventListener() {},
  }
  const context = vm.createContext({ window, document, requestAnimationFrame: () => 0, console })
  vm.runInContext(fs.readFileSync(path.join(PET, 'clash.js'), 'utf8'), context, { filename: 'clash.js' })

  const body = node('div')
  document.body.appendChild(body)
  const layer = () => document.body.children.find(c => c.id === 'clash-host') || null
  const frames = () => (layer() ? layer().children.filter(c => c.tagName === 'IFRAME') : [])
  const bar = () => (layer() ? layer().children.find(c => c.className.includes('s-clash-bar')) : null)
  // 点动作条上的一个按钮：真点击会冒泡到 stage 的委托，这里照它的判法给一个 target。
  const clickBar = selector => {
    const target = { closest: sel => (sel === selector ? { dataset: {} } : null) }
    layer().fire('click', { target })
  }
  // 点 note 里的按钮（重试/启动）：note 的委托在 ensureBox 里。
  const clickNote = selector => {
    // note 的重试/启动按钮是 paint 时 innerHTML 写出来的，替身不解析它：
    // 冒泡到 note 委托的 target 用判法等价物代替（clash.js 只认 closest）。
    const note = document.querySelector('#island-settings .s-clash')?.querySelector('.s-clash-note')
    assert.ok(note, 'note 在')
    const target = { closest: sel => (sel === selector ? { dataset: {} } : null) }
    note.fire('click', { target })
  }

  return {
    Clash: window.Clash,
    calls,
    body,
    layer,
    frames,
    frame: () => frames()[0] || null,
    bar,
    clickBar,
    clickNote,
    note: () => document.querySelector('#island-settings .s-clash')?.querySelector('.s-clash-note') || null,
    solids: () => Array.from(window.Clash.solids()),
    rects: () => solid(window.Clash.solids()),
    document: () => document,
    enter: () => window.Clash.enter(body, key => key),
    park: () => window.Clash.park(),
  }
}

const settled = () => new Promise(resolve => setImmediate(resolve))

test('进入即拉内核并建同源 frame，层以实心矩形交给窗口', async () => {
  const p = page()
  p.enter()
  await settled()
  console.log('DEBUG calls:', JSON.stringify(p.calls))
  console.log('DEBUG layer:', !!p.layer(), 'frames:', p.frames().length, 'solids:', p.solids().length, 'away:', p.layer() && p.layer().classList.contains('away'))
  assert.equal(p.frames().length, 1, '面板的 frame 建了一个')
  assert.ok(p.frame(), 'frame 对象在')
  assert.ok(p.frame().src.includes('disableUpgradeCore=1'), '面板禁用自己的内核升级')
  assert.ok(p.frame().src.includes('secret=s'), 'secret 走参数')
  assert.equal(p.solids().length, 1, '实心矩形一个')
  assert.equal(p.solids()[0], p.layer(), '实心矩形就是宿主层')
  const rects = p.rects()
  assert.equal(rects.length, 1, '交给 Rust 的矩形一个')
  assert.ok(covered(rects, 200 + 560, 100 + 380), '面板正中')
  assert.ok(covered(rects, 201, 101), '左上角内侧')
  assert.ok(covered(rects, 1120 + 200 - 1, 100 + 760 - 1), '右下角内侧')
  assert.ok(!covered(rects, 196, 96), '层外不接管')
  assert.ok(p.calls.some(c => c[0] === 'invoke' && c[1] === 'clash_ensure'))
})

test('动作条有系统代理与启停两个按钮，点系统代理走命令并回写状态', async () => {
  const p = page()
  p.enter()
  await settled()
  const bar = p.bar()
  assert.ok(bar, '动作条存在')
  const sys = bar.children.find(c => c.dataset.clashSysproxy)
  const toggle = bar.children.find(c => c.dataset.clashToggle)
  assert.ok(sys, '系统代理按钮在')
  assert.ok(toggle, '启停按钮在')
  p.clickBar('[data-clash-sysproxy]')
  await settled()
  assert.ok(
    p.calls.some(c => c[0] === 'invoke' && c[1] === 'clash_sysproxy' && c[2]?.enable === true),
    '点了开关就走 clash_sysproxy(true)',
  )
  assert.ok(String(sys.textContent).includes('·'), '回写后的状态标点')
})

test('park 之后层不再是实心矩形，回到页面又回来', async () => {
  const p = page()
  p.enter()
  await settled()
  assert.equal(p.solids().length, 1)
  p.park()
  assert.deepEqual(p.solids(), [])
  p.enter()
  await settled()
  assert.equal(p.solids().length, 1)
})

test('停止内核走 clash_stop，停在已停态且不自动拉起；再点启动才回来', async () => {
  const p = page()
  p.enter()
  await settled()
  p.clickBar('[data-clash-toggle]')
  await settled()
  assert.ok(p.calls.some(c => c[0] === 'invoke' && c[1] === 'clash_stop'), '点了停止就走 clash_stop')
  assert.ok(p.note().innerHTML.includes('data-clash-retry'), '已停态给出启动按钮')
  const framesBefore = p.frames().length
  p.enter()
  await settled()
  assert.equal(p.frames().length, framesBefore, '已停态不自动拉起')
  p.clickNote('[data-clash-retry]')
  await settled()
  assert.equal(p.frames().length, framesBefore + 1, '点了启动，frame 回来')
})

test('失败时给出原因与重试，重试会再走一遍 clash_ensure', async () => {
  const p = page({ kernel: 'none' })
  p.enter()
  await settled()
  console.log('DEBUG5 kids:', JSON.stringify(p.document().body.children.map(c => c.id + '/' + c.className)))
  console.log('DEBUG5 note:', !!p.note(), '| calls:', JSON.stringify(p.calls.map(c => c[1] || c[0])))
  assert.ok(p.note().innerHTML.includes('data-clash-retry'), '失败态给出重试按钮')
  assert.ok(p.note().innerHTML.includes('未找到 mihomo 内核'), '失败原因可见')
  const attempts = p.calls.filter(c => c[0] === 'invoke' && c[1] === 'clash_ensure').length
  p.clickNote('[data-clash-retry]')
  await settled()
  const after = p.calls.filter(c => c[0] === 'invoke' && c[1] === 'clash_ensure').length
  assert.equal(after, attempts + 1, '重试再走一遍 clash_ensure')
})
