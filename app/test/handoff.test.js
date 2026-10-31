const assert = require('node:assert/strict')
const fs = require('node:fs')
const path = require('node:path')
const vm = require('node:vm')
const { test } = require('node:test')

const source = fs.readFileSync(path.join(__dirname, '../src/pet/handoff.js'), 'utf8')
function page({ quiet = false, home = 'island', role = 'island', left = 100 } = {}) {
  const classes = new Set()
  const animations = []
  const canvases = []
  const answers = []
  const timeouts = []
  let handler
  let frameId = 0
  const part = (id, x) => ({
    id, offsetWidth: 240, getClientRects: () => [1],
    getBoundingClientRect: () => ({ left: x, top: 12, width: 240, height: 48 }),
    querySelector: () => null,
    animate: (frames, options) => { animations.push({ id, frames, options }); return { cancel() {} } },
  })
  const island = part('island', left)
  const button = part('start', 400)
  const ctx = { setTransform() {}, clearRect() {}, fillRect() {}, beginPath() {}, arc() {}, fill() {}, createRadialGradient: () => ({ addColorStop() {} }) }
  const document = {
    documentElement: { classList: { add: c => classes.add(c), remove: c => classes.delete(c) } },
    body: { classList: { contains: c => c === `home-${home}` }, append: el => canvases.push(el) },
    querySelectorAll: selector => selector === '#island' ? [island] : [island, button],
    getElementById: id => id === 'island' ? island : null,
    createElement: () => ({ style: {}, getContext: () => ctx, remove() { this.removed = true } }),
  }
  const pet = { onHandoff: fn => { handler = fn }, taskbar: { handoff: what => answers.push(what) } }
  const sandbox = {
    window: { pet }, document, location: { search: `?role=${role}` }, URLSearchParams,
    innerWidth: 800, innerHeight: 600, devicePixelRatio: 1, performance: { now: () => 0 },
    matchMedia: () => ({ matches: quiet }), getComputedStyle: () => ({ color: '#ffffff' }),
    requestAnimationFrame: () => ++frameId, cancelAnimationFrame() {},
    setTimeout: (fn, ms) => { timeouts.push({ fn, ms }); return timeouts.length }, clearTimeout() {},
    MutationObserver: class { observe() {} disconnect() {} }, console,
  }
  vm.runInNewContext(source, sandbox)
  return { animations, canvases, answers, timeouts, classes, send: value => handler(value), handler }
}

test('灵动岛启动沿用粒子入场，只凝聚当前岛而不动画任务栏按钮', async () => {
  const p = page()
  await p.send({ step: 'go', home: 'island' })
  assert.equal(p.classes.has('handoff-wait'), false)
  assert.ok(p.canvases.length > 0, '创建粒子画布')
  assert.ok(p.canvases.every(c => c.style.cssText.includes('pointer-events:none')), '画布不拦截鼠标')
  assert.deepEqual(p.animations.map(a => a.id), ['island'])
  for (const timer of p.timeouts.filter(t => t.ms > 1000 && t.ms < 3000)) timer.fn()
  assert.ok(p.answers.includes('done'))
})

test('减少动态效果时短淡入且立即交还交互，不创建粒子', async () => {
  const p = page({ quiet: true })
  await p.send({ step: 'go', home: 'island' })
  assert.equal(p.classes.has('handoff-wait'), false)
  assert.equal(p.canvases.length, 0)
  assert.deepEqual(p.answers, ['done'])
  assert.equal(p.animations.length, 1)
  assert.ok(p.animations[0].options.duration <= 160)
  assert.equal(p.animations[0].options.delay || 0, 0)
  assert.equal(p.animations[0].frames.at(-1).opacity, 1)
})

test('任务栏每次新启动继续播放原有部件入场', async () => {
  for (let run = 0; run < 2; run++) {
    const p = page({ home: 'taskbar' })
    await p.send({ step: 'go', home: 'taskbar' })
    assert.deepEqual(p.animations.map(a => a.id), ['island', 'start'])
    assert.ok(p.canvases.length > 0)
  }
})

test('其他窗口不注册入场处理，不改变可见性', () => {
  const p = page({ role: 'pet' })
  assert.equal(p.handler, undefined)
  assert.equal(p.classes.size, 0)
})

test('原生窗口就绪后给灵动岛发送入场，不强制改写保存的停靠模式', () => {
  const native = fs.readFileSync(path.join(__dirname, '../src-tauri/src/island.rs'), 'utf8')
  const ready = native.slice(native.indexOf('pub fn ready('), native.indexOf('pub fn apply_visibility('))
  assert.match(ready, /"home":\s*sh.home\(\)/)
  assert.match(ready, /sh.home\(\) == "island"/)
  assert.doesNotMatch(ready, /sh.change\(/)
})
