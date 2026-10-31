// 密码旁边的显示按钮（#17）：按下只是把输入框的 type 翻成 text，输入的值、
// 焦点和表单都不动；没内容时按钮不可用；关掉设置再打开又是隐藏的。
// 页面里的实现是 IIFE 里的几个函数，这里按标记切出来在 vm 里跑（同
// settings-focus），配一个够用的假 DOM：要的只有 parentElement.querySelector
// 和按钮的几个属性。
const assert = require('node:assert/strict')
const fs = require('node:fs')
const path = require('node:path')
const vm = require('node:vm')
const { test } = require('node:test')
const { t } = require('../src/shared/i18n')

const source = fs.readFileSync(path.join(__dirname, '../src/pet/settings.js'), 'utf8')
const styles = fs.readFileSync(path.join(__dirname, '../src/pet/settings.css'), 'utf8')

// 这次改动新增的全部代码：画眼睛、拼密码框、翻 type、跟着内容走。按函数自身的
// 边界切，免得把中间那几百行表单拼进来。
function block() {
  const at = marker => {
    const index = source.indexOf(marker)
    assert.ok(index > 0, `标记没找到：${marker}`)
    return index
  }
  const from = (marker, end) => {
    const start = at(marker)
    const stop = typeof end === 'function' ? end(start) : at(end)
    assert.ok(stop > start, `切不出这一段：${marker}`)
    return source.slice(start, stop)
  }
  // 一个函数到下一个函数开始的地方。
  const nextFunction = start => source.indexOf('\n  function ', start + 1) + 1
  return [
    from('  const eyeGlyph = shown', '  function passwordBox('),
    from('  function passwordBox(', nextFunction),
    from('  function paintEye(', nextFunction),
    from('  function syncEye(', () => at("  layer.addEventListener('input', e => {")),
  ].join('')
}

function page() {
  const eyeShown = new Set()
  const context = vm.createContext({
    eyeShown,
    T: key => t('zh', key),
    esc: text => String(text).replace(/[&<>"]/g, c => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;' })[c]),
  })
  vm.runInContext(`${block()}
    this.passwordBox = passwordBox;
    this.paintEye = paintEye;
    this.syncEye = syncEye;`, context)
  return { ...context, eyeShown }
}

// 一个密码框：输入框、它的按钮，和按钮之外看不见的那个 span。
function box(page, value, id = 'mail') {
  const input = { value, type: 'password', dataset: { eyeFor: id } }
  const attrs = { 'aria-pressed': 'false', 'aria-label': '' }
  const eye = {
    disabled: !value,
    title: '',
    innerHTML: '',
    selected: '',
    classList: { on: false, toggle(name, value) { this.on = value } },
    get pressed() { return attrs['aria-pressed'] },
    get label() { return attrs['aria-label'] },
    getAttribute: name => attrs[name] ?? null,
    setAttribute(name, value) { attrs[name] = value },
    querySelector(selector) { this.selected = selector; return this },
  }
  const wrap = { querySelector: selector => (selector.includes('data-eye') ? eye : null) }
  input.parentElement = wrap
  return { input, eye }
}

test('密码框默认是隐藏的，眼睛只在有内容时可用', () => {
  const p = page()
  const empty = p.passwordBox('mail', 'data-mf="password"', '')
  assert.match(empty, /type="password"/, '默认隐藏')
  assert.match(empty, /data-eye="mail"/, '按钮指出自己管哪个框')
  assert.match(empty, /data-eye-for="mail"/, '框也标出自己归哪个按钮')
  assert.match(empty, /aria-pressed="false"/)
  assert.match(empty, /aria-label="显示密码"/)
  assert.match(empty, /<button[^>]*disabled/, '空的时候按不了')
  assert.doesNotMatch(empty, /<input[^>]*disabled/, '空的密码框还得能打字')

  const filled = p.passwordBox('mail', 'data-mf="password"', 'hunter2')
  assert.match(filled, /type="password"/)
  assert.doesNotMatch(filled, /disabled/, '有内容了就能按')
  assert.match(filled, /value="hunter2"/)
})

test('按下只是翻开 type：值不动、焦点不动、表单照旧', () => {
  const p = page()
  const { input, eye } = box(p, 'hunter2')
  p.paintEye(input, true)
  assert.equal(input.type, 'text')
  assert.equal(input.value, 'hunter2', '显示不等于交出：值原样留着')
  assert.equal(eye.pressed, 'true')
  assert.equal(eye.label, '隐藏密码', '按过之后说的是再按一次做什么')
  assert.equal(eye.title, '隐藏密码')
  assert.equal(eye.classList.on, true)
  assert.ok(p.eyeShown.has('mail'))
  assert.match(eye.innerHTML, /<svg/, '眼睛照旧画着')

  p.paintEye(input, false)
  assert.equal(input.type, 'password')
  assert.equal(eye.pressed, 'false')
  assert.equal(eye.label, '显示密码')
  assert.equal(eye.classList.on, false)
  assert.equal(p.eyeShown.has('mail'), false, '再按回去就不记得了')
})

test('翻开的框再画一遍仍是翻开的，另一个框各论各的', () => {
  const p = page()
  const { input } = box(p, 'hunter2')
  p.paintEye(input, true)
  const again = p.passwordBox('mail', 'data-mf="password"', 'hunter2')
  assert.match(again, /type="text"/, '页面重画不该把它盖回去')
  assert.match(again, /aria-pressed="true"/)
  assert.match(again, /aria-label="隐藏密码"/)
  assert.equal(p.passwordBox('lhmPass', 'data-mon="lhmPass"', 'x').includes('type="text"'), false, '另一个框各论各的')
})

test('框被清空时眼睛跟着失效并回到隐藏，粘贴进去也认得', () => {
  const p = page()
  const { input, eye } = box(p, 'hunter2')
  p.paintEye(input, true)
  input.value = ''
  p.syncEye(input)
  assert.equal(eye.disabled, true, '没内容按不到了')
  assert.equal(input.type, 'password', '清空之后不该还露着')
  assert.equal(eye.pressed, 'false')
  input.value = 'pasted'
  p.syncEye(input)
  assert.equal(eye.disabled, false, '粘贴进来又有内容了')
  assert.equal(input.type, 'password', '重新有内容不等于重新显示')
})

test('按钮在页面上有位置：键盘能到、看得见按没按', () => {
  for (const rule of ['#island-settings .pw .eye', '#island-settings .pw .eye:focus-visible', '#island-settings .pw .eye:disabled']) {
    assert.ok(styles.includes(rule), `样式里缺 ${rule}`)
  }
  assert.match(styles, /#island-settings \.pw \{[^}]*position: relative/, '眼睛叠在输入框右端')
  assert.match(styles, /#island-settings \.pw \.in \{[^}]*padding-right: 36px/, '右边留出眼睛的位置，字不会被压在按钮底下')
  // 关闭设置时清掉，再打开是隐藏的（不记忆状态）。
  const close = source.slice(source.indexOf('  function close(why'), source.indexOf('  function setTab'))
  assert.match(close, /eyeShown\.clear\(\)/)
  // 这份状态在页面上真的声明过：上面的 vm 自己造了一个，漏了看不出来。
  assert.match(source, /^  const eyeShown = new Set\(\)$/m, '页面上要声明 eyeShown，否则一画就 ReferenceError')
})
