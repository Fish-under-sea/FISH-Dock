const assert = require('node:assert/strict')
const { test } = require('node:test')

const D = require('../src/pet/dialog-anchor')

// 确认弹窗的定位算术（t2）。为什么要这一组：`showModal()` 的 UA 样式是
// `position: fixed; inset: 0; margin: auto` —— 弹窗永远在视口正中；岛的窗口
// 铺满整个工作区宽度，所以视口正中就是屏幕正中。定位逻辑必须自己算，
// 这里把「岛在哪、视口多宽」的算术钉死。

const viewport = (width, height) => ({ width, height })
const island = (left, top, width, height) => ({ left, top, width, height, right: left + width, bottom: top + height })

test('弹窗贴岛的锚点一侧：岛在左、中、右', () => {
  // 岛窗口铺满工作区宽度（1920），岛本体 520 宽，按 island.js 的锚点摆在窗口里。
  const vp = viewport(1920, 740)
  const left = island(8, 8, 520, 700)
  const center = island(700, 8, 520, 700)
  const right = island(1392, 8, 520, 700)
  // 左侧：贴岛左内衬；中间：在岛内居中；右侧：贴岛右内衬。
  assert.equal(D.place({ island: left, viewport: vp, anchor: 'left' }).left, 8 + D.INSET)
  assert.equal(D.place({ island: center, viewport: vp, anchor: 'center' }).left, 700 + (520 - D.MAX_WIDTH) / 2)
  assert.equal(D.place({ island: right, viewport: vp, anchor: 'right' }).left, 1392 + 520 - D.INSET - D.MAX_WIDTH)
  // 三种锚点都不在视口正中，除非岛自己就在中间。
  const mid = r => r.left + r.width / 2
  assert.ok(Math.abs(mid(D.place({ island: left, viewport: vp, anchor: 'left' })) - vp.width / 2) > 100)
  assert.ok(Math.abs(mid(D.place({ island: right, viewport: vp, anchor: 'right' })) - vp.width / 2) > 100)
  assert.ok(Math.abs(mid(D.place({ island: center, viewport: vp, anchor: 'center' })) - vp.width / 2) < 1)
})

test('纵向在岛内居中；岛太矮放不下时贴岛顶向下延伸', () => {
  const vp = viewport(1920, 740)
  // 岛高 700、弹窗 518：居中。
  const tall = D.place({ island: island(8, 8, 520, 700), viewport: vp, anchor: 'left', height: 518 })
  assert.equal(tall.top, 8 + (700 - 518) / 2)
  // 岛高只有 132（设置还没长开）、内容要 518：夹在岛给的上限（132 - 2*12 = 108），
  // 贴着岛顶向内衬。
  const short = D.place({ island: island(8, 8, 520, 132), viewport: vp, anchor: 'left', height: 518 })
  assert.equal(short.height, 108)
  assert.equal(short.top, 8 + D.INSET)
  // 内容矮（一行确认）也至少给 MIN_HEIGHT，免得缩成一条。
  const one = D.place({ island: island(8, 8, 520, 700), viewport: vp, anchor: 'left', height: 40 })
  assert.equal(one.height, D.MIN_HEIGHT)
})

test('屏幕边缘：岛被摆到视口边上时弹窗不跑出视口', () => {
  const vp = viewport(1366, 600)
  // 岛贴着视口右缘（工作区在左，Windows 自己的任务栏占掉左边一条）。
  const flushRight = island(1366 - 400, 8, 400, 560)
  const r = D.place({ island: flushRight, viewport: vp, anchor: 'center' })
  assert.ok(r.left + r.width <= vp.width - D.INSET, `弹窗右缘 ${r.left + r.width} 应在视口内`)
  // 岛贴在视口左缘、锚点是右：同样夹回视口内。
  const flushLeft = island(0, 8, 400, 560)
  const l = D.place({ island: flushLeft, viewport: vp, anchor: 'right' })
  assert.ok(l.left >= D.INSET, `弹窗左缘 ${l.left} 应在视口内`)
  assert.ok(l.left + l.width <= vp.width - D.INSET)
})

test('多显示器：岛的窗口在第二显示器的坐标系里，算术与主屏同一套', () => {
  // 第二显示器在主屏左边时，Windows 给它的原点 x 是负的；但岛的**窗口**
  // 视口坐标仍从 0 开始（getBoundingClientRect 是相对本窗口的），所以这里
  // 换的只是工作区尺寸，不是坐标原点。
  const laptop = viewport(1920, 1080)
  const external = viewport(2560, 1440)
  const at = (vp, w) => D.place({ island: island(vp.width - w - 8, 8, w, vp.height - 40), viewport: vp, anchor: 'right' })
  // 同一个锚点，两套工作区：各自落在自己的窗口内，绝不用主屏的宽度。
  for (const vp of [laptop, external]) {
    const r = at(vp, 520)
    assert.ok(r.left >= D.INSET && r.left + r.width <= vp.width - D.INSET, `${vp.width} 宽的工作区里弹窗应在窗口内`)
    assert.equal(r.left, vp.width - 520 - 8 + 520 - D.INSET - r.width)
  }
  // 视口高度不同：max-height 跟着视口走，不写死 70vh 的绝对值。
  assert.ok(D.place({ island: island(8, 8, 520, 700), viewport: laptop }).height <= laptop.height * D.MAX_HEIGHT_RATIO)
  assert.ok(D.place({ island: island(8, 8, 520, 1400), viewport: external }).height <= external.height * D.MAX_HEIGHT_RATIO)
})

test('DIP 缩放不进入算术：量到的 CSS px 与视口的 CSS px 已经同一单位', () => {
  // devicePixelRatio 变了，getBoundingClientRect 与 innerWidth 一起变，
  // 算出来的 CSS px 框不变；这里显式钉住「同一组输入 → 同一组输出」，
  // 免得以后有人把 dpr 塞进公式里（那会把 175% 的机器算歪）。
  const inputs = { island: island(8, 8, 520, 700), viewport: viewport(1920, 740), anchor: 'left' }
  const first = D.place(inputs)
  for (const dpr of [1, 1.25, 1.5, 1.75, 2]) {
    // dpr 不参与：同样的 CSS px 输入，结果逐字段相同。
    assert.deepEqual({ ...D.place({ ...inputs, dpr }) }, { ...first }, `dpr ${dpr} 不该改变框`)
  }
})

test('岛在窗口里的哪一侧，由 island.js 的锚点类给出', () => {
  const has = (...names) => ({ contains: n => names.includes(n) })
  assert.equal(D.anchorOf(has('isl-at-left')), 'left')
  assert.equal(D.anchorOf(has('isl-at-right')), 'right')
  assert.equal(D.anchorOf(has()), 'center')
  // 两个都在（不该发生）：左优先，与 island.js placeAnchor 的写法一致。
  assert.equal(D.anchorOf(has('isl-at-left', 'isl-at-right')), 'left')
  // 没有 classList（元素还不在文档里）：按中间，不抛。
  assert.equal(D.anchorOf(undefined), 'center')
})

test('上限：宽度取 min(360, 岛宽-2内衬, 视口-2内衬)，高度再夹 70vh', () => {
  const vp = viewport(1920, 740)
  assert.deepEqual(D.limits(island(8, 8, 520, 700), vp), { width: D.MAX_WIDTH, height: 518 })
  // 岛比 360 窄：以岛为准。
  assert.equal(D.limits(island(8, 8, 300, 700), vp).width, 300 - 2 * D.INSET)
  // 视口比 360+2内衬 还窄：以视口为准（400 宽给 376，仍大于 360，故换 340）。
  assert.equal(D.limits(island(0, 0, 2000, 700), viewport(340, 740)).width, 340 - 2 * D.INSET)
  assert.equal(D.MAX_WIDTH, 360)
  assert.equal(D.INSET, 12)
  assert.equal(D.MIN_HEIGHT, 220)
})
