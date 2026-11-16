// DEC-2 顶栏 / dock 纯逻辑单测（topbar-ui.js）：窗口展示模型、点击决策（带前台 hwnd）、
// dock 布局居中、顶栏模型。
const assert = require('node:assert/strict')
const { test } = require('node:test')
const { windowItem, clickAction, dockLayout, topbarModel } = require('../src/pet/topbar-ui')

test('windowItem 归一出展示模型，缩略图缺失时降级为空', () => {
  const it = windowItem({ id: 42, title: '  Hello  ', png: 'data:x', min: true })
  assert.equal(it.hwnd, 42)
  assert.equal(it.title, 'Hello')
  assert.equal(it.minimized, true)
  assert.equal(it.icon, 'data:x')
  assert.equal(it.thumb, null, '无缩略图 → null，页面用图标+标题降级')
})

test('clickAction 把窗口带到前台；最小化的标 restore', () => {
  assert.deepEqual(clickAction({ hwnd: 7, minimized: false }), { action: 'focus', hwnd: 7, restore: false })
  assert.deepEqual(clickAction({ hwnd: 7, minimized: true }), { action: 'focus', hwnd: 7, restore: true })
  assert.deepEqual(clickAction({ hwnd: 0 }), { action: 'none' })
  assert.deepEqual(clickAction(null), { action: 'none' })
})

test('dockLayout 等宽图标整体居中', () => {
  const { iconPx, gapPx, xs } = dockLayout(3, 1000, 1, 44, 8)
  assert.equal(iconPx, 44)
  assert.equal(gapPx, 8)
  // 总宽 = 3*44 + 2*8 = 148；左边 = (1000-148)/2 = 426。
  assert.equal(xs[0], 426)
  assert.equal(xs[1], 426 + 52)
  assert.equal(xs[2], 426 + 104)
  // 缩放 2：物理翻倍。
  const d2 = dockLayout(2, 1000, 2, 44, 8)
  assert.equal(d2.iconPx, 88)
})

test('topbarModel 带时钟与项', () => {
  const m = topbarModel([{ id: 1, title: 'A' }, { id: 2, title: 'B' }], '09:41')
  assert.equal(m.clock, '09:41')
  assert.equal(m.items.length, 2)
  assert.deepEqual(m.right, ['quick'])
})
