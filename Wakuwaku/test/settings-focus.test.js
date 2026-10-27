const assert = require('node:assert/strict')
const fs = require('node:fs')
const path = require('node:path')
const vm = require('node:vm')
const { test } = require('node:test')

const styles = fs.readFileSync(path.join(__dirname, '../src/pet/settings.css'), 'utf8')
test('确认弹窗尺寸紧凑，遮罩不把整个 Dock 背景压暗', () => {
  for (const selector of ['s-confirm-dialog', 's-fullscreen-dialog']) {
    const rule = styles.match(new RegExp(`#island-settings \\.${selector} \\{([^}]+)\\}`))?.[1] || ''
    assert.match(rule, /width:\s*min\(360px, calc\(100vw - 40px\)\)/)
    assert.match(rule, /max-height:\s*min\(70vh, calc\(100vh - 32px\)\)/)
    assert.match(rule, /box-shadow:\s*none/)
    assert.match(styles, new RegExp(`#island-settings \\.${selector}::backdrop \\{ background: transparent; \\}`))
  }
})
test('设置键盘处理不依赖已移除的全屏弹窗变量', () => {
  const handler = source.slice(source.indexOf("  document.addEventListener('keydown'"), source.indexOf('  window.pet.onIdentityChanged'))
  const listeners = []
  const context = { document: { addEventListener: (_, cb) => listeners.push(cb) }, isOpen: true, menuEl: { hidden: true }, close: why => { context.closed = why }, confirmDialog: { open: false } }
  vm.runInNewContext(handler, context)
  listeners[0]({ key: 'Escape', preventDefault() {} })
  assert.equal(context.closed, 'Esc')
})

const source = fs.readFileSync(path.join(__dirname, '../src/pet/settings.js'), 'utf8')
const lifecycle = source.slice(source.indexOf('  function open(next)'), source.indexOf('  async function loadGallery('))
function page() {
  const calls = []
  const saved = {}
  const replies = []
  const snapshot = () => ({ settings: { settingsPin: false, settingsTop: false, ...saved } })
  const context = vm.createContext({
    calls, replies, console: { log() {}, error() {} }, Date, CSS: { escape: x => x },
    setInterval: () => 1, clearInterval() {}, clearTimeout() {},
    document: { hasFocus: () => true, activeElement: null },
    layer: { focus() {}, contains: () => false },
    extensionPage: { park() {} },
    extensionPanel: { pages:()=>[{key:'fish.ofm/main'}], load:async()=>{} },
    draw: () => calls.push('draw'), relayout() {}, showPart() {}, loadGallery() {},
    cardShows: () => false, openLetter() {}, syncFullscreenPrompt() {},
    window: { pet: {
      keyboard() {}, settingsClosed() {}, settings: {
        get: () => new Promise(resolve => replies.push(() => resolve(snapshot()))),
        focus: on => { calls.push(on); return Promise.resolve(snapshot()) },
        set: patch => { Object.assign(saved, patch); return Promise.resolve(snapshot()) },
      },
    } },
  })
  vm.runInContext(`let tab = 'now', snap = null, isOpen = false, openedAt = 0, clockTimer = 0;
    let modelFocus = false, openRevision = 0, confirmActive = null, confirmQueue = [], welcomeOpen, partToShow, letterToOpen, thumbTimers = [], agentOpen, othersOpen, holdCustom;
    const PART_OF = {}, TABS = ['now', 'fish.ofm/main', 'mail'], tick = () => {};
    ${lifecycle}
    this.controls = { open, close, setTab, patch, currentTab: () => tab };`, context)
  return { ...context.controls, calls, replies, saved }
}
test('关闭时记住当前标签并保留窗口偏好', () => {
  const p = page()
  p.open('mail'); p.close()
  assert.equal(p.saved.settingsLastTab, 'mail')
  assert.equal(p.saved.settingsPin, undefined)
  assert.equal(p.saved.settingsTop, undefined)
})
test('重开时恢复上次页面，迟到恢复不覆盖新选择', async () => {
  const p = page()
  p.saved.settingsLastTab = 'mail'
  p.open(); p.replies.shift()()
  await new Promise(resolve => setImmediate(resolve))
  assert.equal(p.currentTab(), 'mail')
  p.close(); p.open(); p.setTab('now'); p.replies.shift()()
  await new Promise(resolve => setImmediate(resolve))
  assert.equal(p.currentTab(), 'now')
})
test('进入 OFM 和重复打开均保留手动锁定置顶选择', () => {
  const p = page()
  p.open('fish.ofm/main')
  p.open('fish.ofm/main')
  assert.deepEqual(p.calls, [])
})
test('切换插件页和关闭重开均不覆盖窗口状态', () => {
  const p = page()
  p.open('fish.ofm/main'); p.setTab('mail'); p.setTab('fish.ofm/main'); p.close(); p.open('fish.ofm/main')
  assert.deepEqual(p.calls.filter(x => typeof x === 'boolean'), [])
})
test('OFM 快速关闭后的迟到快照不再重绘或恢复焦点', async () => {
  const p = page()
  p.open('fish.ofm/main'); p.close(); p.replies[0]()
  await Promise.resolve()
  assert.equal(p.calls.includes('draw'), false)
})
test('其他标签页打开关闭不改变窗口锁定置顶偏好', () => {
  const p = page()
  p.open('mail'); p.close()
  assert.deepEqual(p.calls, [])
})
