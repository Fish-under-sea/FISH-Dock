// Esc 关掉权限询问（上游 bf6f0adb）：面板持有提问时，Esc 把提问交还终端
// （不替它答），与「去终端处理」按钮同一个动作；未武装（刚弹出 600ms 内）或
// 无提问时不动。以 role=island 起页，注入 pet:asks 与 pet:update。
const assert = require('node:assert/strict')
const fs = require('node:fs')
const path = require('node:path')
const vm = require('node:vm')
const { test } = require('node:test')

const panelSrc = fs.readFileSync(path.join(__dirname, '../src/pet/panel.js'), 'utf8')

// A tiny DOM stand-in: panel element with hidden + replaceChildren + querySelector.
function fakePanel() {
  const buttons = []
  return {
    hidden: true,
    replaceChildren() {
      buttons.length = 0
    },
    append(...kids) {
      for (const k of kids) if (k && k.tag === 'button') buttons.push(k)
    },
    querySelector(sel) {
      if (sel === 'button:not([disabled])') return buttons.find(b => !b.disabled) || null
      return null
    },
    querySelectorAll() {
      return buttons
    },
    // The tests add an armed (enabled) button when the prompt is up.
    _arm() {
      buttons.push({ tag: 'button', disabled: false })
    },
    addEventListener() {},
    offsetWidth: 100,
    offsetHeight: 40,
  }
}

function load() {
  const listeners = []
  const answered = []
  const dismissed = []
  const jumped = []
  const panel = fakePanel()
  const documentStub = {
    getElementById: id => (id === 'panel' ? panel : null),
    createElement: () => ({ append() {}, setAttribute() {}, addEventListener() {} }),
    createTextNode: t => t,
    addEventListener: (type, cb, capture) => listeners.push({ type, cb, capture }),
    documentElement: { style: { setProperty() {} } },
    body: { classList: { toggle() {} } },
    activeElement: null,
    hasFocus: () => true,
  }
  const ctx = {
    URLSearchParams,
    location: { search: '?role=island' },
    console: { log() {}, error() {} },
    setTimeout: () => 0,
    clearTimeout() {},
    document: documentStub,
    ResizeObserver: undefined,
    requestAnimationFrame: cb => cb(),
    window: {
      I18n: { t: () => 'x' },
      pet: {
        onShift() {},
        onUpdate() {},
        onHotkey() {},
        onAsks(cb) {
          this._asks = cb
        },
        onPeek() {},
        answer: (id, choice) => answered.push([id, choice]),
        dismiss: id => dismissed.push(id),
        jump: id => jumped.push(id),
        keyboard() {},
        panel() {},
        hover() {},
      },
      addEventListener() {},
    },
  }
  ctx.window.pet.window = ctx.window
  vm.createContext(ctx)
  vm.runInContext(panelSrc, ctx)
  // The panel's onAsks handler is registered after render(); find it.
  const onAsks = ctx.window.pet._asks
  return { ctx, listeners, answered, dismissed, jumped, panel, onAsks }
}

const permission = { id: 7, kind: 'permission', from: 'claude', session: 's1' }

test('Esc 把权限询问交还终端（不替它答）', () => {
  // Drive render() through onAsks on a fresh page to set shownId.
  const page = load()
  page.onAsks([permission])
  // The page's panel is hidden false after render with an ask; arm a button.
  page.panel.hidden = false
  page.panel._arm()
  const key = page.listeners.find(l => l.type === 'keydown')
  assert.ok(key, 'keydown 监听已注册')
  let prevented = false
  let stopped = false
  key.cb({ key: 'Escape', isComposing: false, preventDefault: () => (prevented = true), stopPropagation: () => (stopped = true) })
  assert.deepEqual(page.dismissed, [7], 'Esc 交还终端（pet.dismiss）')
  assert.deepEqual(page.answered, [], '不替它回答')
  assert.ok(prevented && stopped, '吞掉这次 Esc，设置页不会也关掉')
})

test('未武装或无提问时 Esc 不动', () => {
  const page = load()
  // No ask yet.
  const key = page.listeners.find(l => l.type === 'keydown')
  let prevented = false
  key.cb({ key: 'Escape', preventDefault: () => (prevented = true), stopPropagation() {} })
  assert.deepEqual(page.dismissed, [], '没有提问：不交还')
  assert.equal(prevented, false)
})
