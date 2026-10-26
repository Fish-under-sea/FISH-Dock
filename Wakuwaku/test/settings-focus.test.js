const assert = require('node:assert/strict')
const fs = require('node:fs')
const path = require('node:path')
const vm = require('node:vm')
const { test } = require('node:test')

const source = fs.readFileSync(path.join(__dirname, '../src/pet/settings.js'), 'utf8')
const lifecycle = source.slice(source.indexOf('  function open(next)'), source.indexOf('  async function loadGallery('))
function page() {
  const calls = []
  const replies = []
  const snapshot = () => ({ settings: { settingsPin: false, settingsTop: false } })
  const context = vm.createContext({
    calls, replies, console: { log() {}, error() {} }, Date, CSS: { escape: x => x },
    setInterval: () => 1, clearInterval() {}, clearTimeout() {},
    document: { hasFocus: () => true, activeElement: null },
    layer: { focus() {}, contains: () => false },
    Models: { park() {} }, Clash: { park() {} }, draw: () => calls.push('draw'), relayout() {}, showPart() {}, loadGallery() {},
    cardShows: () => false, openLetter() {}, syncFullscreenPrompt() {},
    window: { pet: {
      keyboard() {}, settingsClosed() {}, settings: {
        get: () => new Promise(resolve => replies.push(() => resolve(snapshot()))),
        focus: on => { calls.push(on); return Promise.resolve(snapshot()) },
        set: patch => Promise.resolve({ settings: patch }),
      },
    } },
  })
  vm.runInContext(`let tab = 'now', snap = null, isOpen = false, openedAt = 0, clockTimer = 0;
    let modelFocus = false, openRevision = 0, welcomeOpen, partToShow, letterToOpen, thumbTimers = [], agentOpen, othersOpen, holdCustom;
    const PART_OF = {}, TABS = ['now', 'models', 'mail'], tick = () => {};
    ${lifecycle}
    this.controls = { open, close, setTab, patch };`, context)
  return { ...context.controls, calls, replies }
}
test('进入 OFM 默认锁定置顶，重绘和重复打开不覆盖用户手动选择', () => {
  const p = page()
  p.open('models')
  p.open('models')
  assert.deepEqual(p.calls, [true])
})
test('切到其他顶级页或关闭时关闭 OFM 锁定置顶', () => {
  const p = page()
  p.open('models'); p.setTab('mail'); p.setTab('models'); p.close()
  assert.deepEqual(p.calls.filter(x => typeof x === 'boolean'), [true, false, true, false])
})
test('OFM 快速关闭后的迟到快照不再重绘或恢复焦点', async () => {
  const p = page()
  p.open('models'); p.close(); p.replies[0]()
  await Promise.resolve()
  assert.equal(p.calls.includes('draw'), false)
})
test('其他标签页打开关闭不改变窗口锁定置顶偏好', () => {
  const p = page()
  p.open('mail'); p.close()
  assert.deepEqual(p.calls, [])
})
