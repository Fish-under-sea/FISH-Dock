// The Local models page's console lives in a frame of its own, in a layer of
// its own (#models-host, models.js). That layer is not a child of the island:
// WebView2's own mouseenter / mouseleave of the island never say the pointer
// is on it, and the frame is another origin, so nothing inside it reaches the
// page either. The window lets the mouse through unless the page says the
// pointer is on something of its own — island.rs set_editing / pointer.rs
// look, "solid" rects in page px — so the layer has to be handed over as one
// of those rects, or a real mouse on the console goes to the desktop under
// it: its links, its scroll, its boxes never see a press (the gap this file
// was written for, 2026-10-20).
//
// No DOM here: models.js is run against a stand-in window and document, so
// what is checked is its own bookkeeping — the layer it puts where, what it
// says the window must take, and that the frame it made is the one it keeps.
const assert = require('node:assert/strict')
const fs = require('node:fs')
const path = require('node:path')
const vm = require('node:vm')
const { test } = require('node:test')

const { grow, solid } = require('../src/pet/surfaces')

const PET = path.join(__dirname, '..', 'src', 'pet')

// pointer.rs look: a point in page px on one of the rects (x, y, w, h).
const covered = (rects, x, y) => rects.some(r => x >= r[0] && x <= r[0] + r[2] && y >= r[1] && y <= r[1] + r[3])

// --- A stand-in page, as much of one as models.js uses -------------------------

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
    src: '',
    box: null,
    connected: false,
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
    querySelector(selector) {
      return el.queries[selector] || null
    },
    // A layer put by its styles, as a browser lays it out (models.js writes
    // left / top / width / height; #models-host.away moves it off screen).
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

// The page with its settings open on the Local models page: the slot the
// console is put over, the box the page draws, and the island's side
// (window.Isle) and pointer (window.pet) recorded as they are called.
function page({ open = true, box = { left: 200, top: 100, width: 1120, height: 760 } } = {}) {
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
  const boxEl = node('div')
  boxEl.queries['.s-models-note'] = node('div')
  const queries = { '#island-settings .s-models-slot': slot, '#island-settings .s-models': boxEl }
  // The page's own lookups, as models.js makes them.
  document.querySelector = selector => queries[selector] || null

  const calls = []
  const window = {
    location: { origin: 'http://tauri.localhost' },
    Settings: { isOpen: () => open },
    Isle: {
      sendSolid: () => calls.push(['sendSolid']),
      layerHover: on => calls.push(['layerHover', on]),
    },
    pet: { hover: on => calls.push(['hover', on]) },
    __TAURI__: { core: { invoke: async () => ({ url: 'http://127.0.0.1:1234/console?t=1' }) } },
    addEventListener() {},
  }
  const context = vm.createContext({ window, document, requestAnimationFrame: () => 0, console })
  vm.runInContext(fs.readFileSync(path.join(PET, 'models.js'), 'utf8'), context, { filename: 'models.js' })

  const body = node('div')
  body.queries[':scope > .s-models'] = boxEl
  const layer = () => document.body.children.find(c => c.id === 'models-host') || null
  const frames = () => (layer() ? layer().children.filter(c => c.tagName === 'IFRAME') : [])

  return {
    Models: window.Models,
    calls,
    body,
    slot,
    layer,
    frames,
    frame: () => frames()[0] || null,
    // The layer models.js says is hers, as a plain list of this realm (its own
    // arrays come from the context it is run in).
    solids: () => Array.from(window.Models.solids()),
    holds: node => window.Models.holds(node),
    hovered: () => window.Models.hovered(),
    // What island.js does with the layer: its own elements measured where
    // they are (Surfaces.solid, the edit bar and the picker's alike).
    rects: () => solid(window.Models.solids()),
    enter: () => window.Models.enter(body, key => key),
    park: () => window.Models.park(),
    // The page's own pointer moves (models.js hears them to tell the pointer
    // off the console once it is out of the frame).
    document: () => document,
  }
}

// models.js starts the console's service and makes the frame in a promise:
// let it settle.
const settled = () => new Promise(resolve => setImmediate(resolve))

test('身份文件变更后重新取得管理会话，不复用旧 iframe', async () => {
  const p = page()
  p.enter()
  await settled()
  const old = p.frame()
  p.Models.invalidate?.()
  p.enter()
  await settled()
  assert.notEqual(p.frame(), old)
  assert.equal(p.frames().length, 1)
})

// --- The layer, as the Rust side is told where to take the mouse ----------------

test('shown, the console layer is a solid rect over its own pixels: the window takes the mouse there', async () => {
  const p = page()
  p.enter()
  await settled()
  assert.equal(p.frames().length, 1, 'the console frame made')
  assert.deepEqual(p.solids(), [p.layer()], 'the layer handed over while it shows')
  // island.js grows each part a couple of px and sends it (island.rs
  // set_editing); pointer.rs then keeps the window caught on it.
  const rects = p.rects()
  assert.equal(rects.length, 1)
  assert.ok(covered(rects, 200 + 560, 100 + 380), 'the middle of the console')
  assert.ok(covered(rects, 201, 101), 'just inside its corner')
  assert.ok(covered(rects, 1120 + 200 - 1, 100 + 760 - 1), 'its far corner')
  assert.ok(!covered(rects, 196, 96), 'clear of it: nothing of hers there')
  assert.ok(
    p.calls.some(c => c[0] === 'sendSolid'),
    'the island is told to send the rects as the layer is put',
  )
})

test('parked (another page, or closed) the layer is no rect at all: the desktop keeps the mouse', async () => {
  const p = page()
  p.enter()
  await settled()
  assert.equal(p.solids().length, 1)
  p.park()
  assert.deepEqual(p.solids(), [], 'nothing to take the mouse over')
  assert.deepEqual(p.rects(), [], 'no rect reaches the Rust side')
  assert.ok(p.calls.filter(c => c[0] === 'sendSolid').length >= 2, 'the island is told it went')
})

test('the frame is made once and kept: another page and back reloads nothing', async () => {
  const p = page()
  p.enter()
  await settled()
  const frame = p.frame()
  assert.ok(frame)
  p.park()
  p.enter()
  await settled()
  assert.equal(p.frames().length, 1, 'one frame, not a second')
  assert.equal(p.frame(), frame, 'the same one')
  assert.equal(p.frame().src, 'http://127.0.0.1:1234/console?t=1', 'never built again: its login and its page stay')
})

test('the pointer on the layer is hers: the island hears it and keeps her open', async () => {
  const p = page()
  p.enter()
  await settled()
  p.calls.length = 0
  p.layer().fire('mouseenter')
  assert.deepEqual(p.calls, [['layerHover', true]], 'the pointer on the console, as on the island')
  assert.ok(p.holds(p.layer()), 'the layer is hers')
  assert.ok(p.holds(p.frame()), 'and the frame in it')
  assert.ok(!p.holds(p.body), 'the page under it is not')
  assert.ok(!p.holds(null), 'nothing is not')
  p.calls.length = 0
  p.layer().fire('mouseleave')
  assert.deepEqual(p.calls, [['layerHover', false]], 'and off it, as off her')
})

// --- The wiring the Rust side depends on --------------------------------------

test('the console is not "left" while the pointer is on it: the island keeps its settings', async () => {
  // The layer sits over the page's slot, so WebView2 hit-tests the pointer as
  // off the island the moment it goes on the console: island.js's leave runs,
  // and in the notch that closes the settings — parking the console under the
  // pointer. The layer knows it is hovered; the leave must ask before closing.
  const p = page()
  p.enter()
  await settled()
  assert.equal(p.hovered(), false, 'not hovered until it is')
  p.layer().fire('mouseenter')
  assert.equal(p.hovered(), true, 'the pointer on the console')
  // Onto the frame in it: the frame is another origin and fills the layer, so
  // WebView2 reports the layer as left while the pointer is on the console.
  p.layer().fire('mouseleave', { clientX: 200 + 300, clientY: 100 + 200 })
  assert.equal(p.hovered(), true, 'still on it: the pointer only went into the frame')
  assert.deepEqual(p.calls.filter(c => c[0] === 'layerHover' && c[1] === false), [], 'and it is not said left')
  // Off its box: that is a leave.
  p.layer().fire('mouseleave', { clientX: 200 + 1120 + 40, clientY: 100 + 200 })
  assert.equal(p.hovered(), false, 'off it')
  p.layer().fire('mouseenter')
  p.park()
  assert.equal(p.hovered(), false, 'and never once it is away')

  const source = fs.readFileSync(path.join(PET, 'island.js'), 'utf8')
  const at = source.indexOf("target.addEventListener('mouseleave'")
  assert.ok(at > 0, 'the island leave found')
  const body = source.slice(at, source.indexOf("target.addEventListener('click'", at))
  assert.match(body, /hoverLeft\(\)/, 'the leave is what closes her a beat later')
  const left = source.slice(source.indexOf('function hoverLeft()'), source.indexOf('function hoverEnter()'))
  assert.match(left, /window\.Models\?\.hovered\?\.\(\)/, 'and asks the console before closing')
  assert.ok(
    left.indexOf('window.Models?.hovered?.()') < left.indexOf("window.Settings?.close('left')"),
    'before the settings are closed and the console parked under the pointer',
  )
})

test('out of the frame and onto the page, the console is left: the desktop keeps the mouse', async () => {
  // From the frame to the desktop no leave of the layer's ever comes: it was
  // left when the pointer went into the frame (another origin, filling it),
  // and the frame keeps the pointer until it is off the console altogether —
  // where this page hears the moves again. A move of the page's own, then, is
  // the pointer off the console.
  const p = page()
  p.enter()
  await settled()
  p.layer().fire('mouseenter')
  p.layer().fire('mouseleave', { clientX: 200 + 300, clientY: 100 + 200 })
  assert.equal(p.hovered(), true, 'still on it: only into the frame')
  p.document().fire('pointermove', { clientX: 40, clientY: 700 })
  assert.equal(p.hovered(), false, 'the page has a move of its own: the console is left')
  assert.deepEqual(
    p.calls.filter(c => c[0] === 'layerHover'),
    [['layerHover', true], ['layerHover', false]],
    'and the window is told, so the desktop keeps the mouse there',
  )
})

test('island.js sends the console layer with the picker’s own rects', () => {
  const source = fs.readFileSync(path.join(PET, 'island.js'), 'utf8')
  const at = source.indexOf('function sendSolid()')
  assert.ok(at > 0, 'sendSolid found')
  const body = source.slice(at, source.indexOf('window.pet.editing(on, rects)', at))
  assert.match(body, /window\.Models\?\.solids\?\.\(\)/, 'the models layer is one of the parts sent')
  assert.match(body, /Surfaces\.solid\(/, 'and they are measured by surfaces.js')
})

test('island.js hears the console as hers, and her own enter and leave once she is back', () => {
  const source = fs.readFileSync(path.join(PET, 'island.js'), 'utf8')
  const at = source.indexOf('function layerHover(on)')
  assert.ok(at > 0, 'layerHover found')
  const body = source.slice(at, source.indexOf('const solidWatch', at))
  assert.match(body, /window\.pet\.hover\(true\)/, 'on it, the window takes the mouse')
  assert.match(body, /hoverEnter\(\)/, 'and she opens by itself as when she is entered')
  // Back on her own page the console stands over the settings, inside her box:
  // her own enter has already said so, and her own leave is hers to say.
  assert.match(body, /island\.matches\?\.\(':hover'\)/, 'on her own page, nothing is said of the console')
  assert.ok(
    body.indexOf("island.matches?.(':hover')") < body.indexOf('window.pet.hover(false)'),
    'checked before the pointer is said to have left her',
  )
  assert.match(body, /hoverLeft\(\)/, 'off it, what a leave of hers does')
})

test('the page loads surfaces.js before the scripts that use it', () => {
  const html = fs.readFileSync(path.join(PET, 'index.html'), 'utf8')
  const at = html.indexOf('src="surfaces.js"')
  assert.ok(at > 0, 'surfaces.js loaded')
  assert.ok(at < html.indexOf('src="island.js"'), 'before island.js')
  assert.ok(at < html.indexOf('src="settings.js"'), 'and before settings.js')
  assert.equal(html.includes('src="models.js"'), false, 'legacy OFM page is not bundled into settings')
})

test('a part grown by Surfaces covers its own pixels, and a part not on screen is none', () => {
  const box = { left: 10.4, top: 20.6, width: 100, height: 50 }
  const [rect] = solid([{ isConnected: true, getBoundingClientRect: () => box }])
  assert.deepEqual(rect, grow(box))
  assert.ok(covered([rect], 10.4, 20.6), 'its corner')
  assert.ok(covered([rect], 110.4, 70.6), 'its far corner')
  assert.deepEqual(solid([{ isConnected: false, getBoundingClientRect: () => box }]), [], 'not in the document')
  assert.deepEqual(solid([{ isConnected: true, getBoundingClientRect: () => ({ left: 0, top: 0, width: 0, height: 0 }) }]), [], 'no size')
  assert.deepEqual(solid([]), [], 'none at all')
  assert.deepEqual(solid([null]), [], 'a part that is not there')
})

