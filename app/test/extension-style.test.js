// 插件卡动作区与检索框的**静态**样式契约（T-C / #63）。
//
// 这里只证明 CSS 规则写对了（行式列表、32px 行高、发丝线、危险色常态、
// 输入框省略号），**不**证明真实排版：480/900 视口的行高、裁切与视觉差
// 必须由真实浏览器/WebView2 测量与截图验收（本机沙盒不能起 Chromium，
// 见 .work/204/desktop/handoff.md 的未验事项）。两类证据分开记账。
const { test } = require('node:test')
const assert = require('node:assert/strict')
const fs = require('node:fs')
const path = require('node:path')

const css = fs.readFileSync(path.join(__dirname, '../src/pet/extensions.css'), 'utf8').replace(/\/\*[\s\S]*?\*\//g, '')
// 大括号配平地切块：@media 包裹的规则也不会让选择器串行（简单的正则做不到）。
function blocks(source) {
  const out = []
  let at = 0
  while (at < source.length) {
    const open = source.indexOf('{', at)
    if (open < 0) break
    let head = source.slice(at, open)
    const close = head.lastIndexOf('}')
    if (close >= 0) head = head.slice(close + 1)
    let depth = 1, j = open + 1
    while (j < source.length && depth > 0) { if (source[j] === '{') depth += 1; else if (source[j] === '}') depth -= 1; j += 1 }
    out.push({ selector: head.trim(), body: source.slice(open + 1, j - 1) })
    at = j
  }
  return out
}
const parsed = blocks(css)
const matches = selector => parsed.filter(row => row.selector.split(',').map(part => part.trim()).includes(selector))
// 取某个选择器的最后一条声明块（同选择器可能被后置规则覆盖，最后一条赢）。
function rule(selector) {
  const found = matches(selector)
  assert.ok(found.length, `找不到规则：${selector}`)
  return found.at(-1).body
}
const value = (body, prop) => (body.match(new RegExp(`(?:^|;)\\s*${prop}\\s*:\\s*([^;]+)`)) || [])[1]?.trim()
const rules = selector => matches(selector)
const decls = (selector, prop) => rules(selector).map(row => value(row.body, prop)).filter(Boolean)

test('「更多」是行式动作列表：纵向排列、无间隙，不再是两颗挤在一行', () => {
  const body = rule('#island-settings .extension-more-body')
  assert.equal(value(body, 'flex-direction'), 'column', '纵向一列')
  assert.equal(value(body, 'gap'), '0', '行与行之间靠发丝线分隔，不靠 gap')
  assert.notEqual(value(body, 'flex-wrap'), 'wrap', '不再允许换行成两颗并排')
})

test('每行整行可点、32px 高：宽度撑满、文字左对齐、最小高度不塌陷', () => {
  const row = rule('#island-settings .extension-more-row')
  assert.equal(value(row, 'width'), '100%', '整行可点')
  assert.equal(value(row, 'min-height'), '32px', '≤32px 的点击目标（与功能页入口同档）')
  assert.equal(value(row, 'text-align'), 'left', '文字左对齐，不是居中胶囊')
  assert.equal(value(row, 'cursor'), 'pointer')
  assert.equal(value(row, 'background'), 'transparent', '平时无底色，悬停才有')
})

test('第二行起用 1px 发丝线隔 8px；禁用态仍然保持高度与可读色', () => {
  const sep = rule('#island-settings .extension-more-row + .extension-more-row')
  assert.equal(value(sep, 'border-top'), '1px solid var(--g3)', '发丝线用现有 --g3')
  assert.equal(value(sep, 'margin-top'), '8px', '隔 8px')
  const disabled = rule('#island-settings .extension-more-row:disabled')
  assert.equal(value(disabled, 'cursor'), 'default')
  assert.ok(value(disabled, 'color'), '禁用态给次级色，不消失')
})

test('卸载常态就是危险色：不再只在 hover/focus 才变红', () => {
  const danger = rule('#island-settings .extension-more-row.extension-danger')
  assert.equal(value(danger, 'color'), 'var(--bad-ink)', '常态危险色')
  // 旧口径（.extension-more-body .extension-danger 只在 hover/focus 上色）必须消失。
  assert.equal(rules('#island-settings .extension-more-body .extension-danger:hover:not(:disabled)').length, 0, '旧 hover-only 规则已删')
  const onDanger = decls('#island-settings .extension-more-row.extension-danger:hover:not(:disabled)', 'background')
  assert.ok(onDanger.some(bg => /color-mix\(in srgb, var\(--bad-ink\) 8%/.test(bg)), '悬停只补极淡底色：' + JSON.stringify(onDanger))
})

test('检索框长包名不撑破字段：输入框带省略号', () => {
  const input = rule('#island-settings .extension-search-input')
  assert.equal(value(input, 'text-overflow'), 'ellipsis')
  assert.equal(value(input, 'min-width'), '0', '可收缩，不被长包名顶宽')
})
// t26：真实浏览器量到 28px —— `.extension-panel .pbtn { min-height: 28px }`（特异性
// 1,2,0）压过 `.extension-more-row`（1,1,0）。静态测试要能拦住这种「改回来了又被
// 别人的级联压下去」的情况，所以这里按级联规则判定：最后生效的那条 min-height 必须
// 是 32px，且它的选择器优先级不低于通用按钮规则；高度必须交还内容（不吃 .pbtn.sm 的 24px）。
test('级联不许把动作行压回 28px：生效的 min-height 是 32 且优先级够', () => {
  const specificity = selector => {
    const ids = (selector.match(/#/g) || []).length
    const classes = (selector.match(/\.[A-Za-z0-9_-]+/g) || []).length
    return ids * 100 + classes
  }
  const matched = parsed
    .map((row, index) => ({ ...row, index }))
    .filter(row => row.selector.split(',').map(part => part.trim()).some(part => part.includes('.extension-more-row')))
    .map(row => ({ ...row, minHeight: value(row.body, 'min-height'), height: value(row.body, 'height') }))
    .filter(row => row.minHeight)
  assert.ok(matched.length, '动作行必须显式声明 min-height')
  const winner = matched.at(-1)                       // 同档特异性：文档顺序靠后的赢
  assert.equal(winner.minHeight, '32px', `最后生效的 min-height 应是 32px（实测 ${winner.minHeight}）`)
  assert.equal(winner.height, 'auto', '高度交给内容 + min-height，不再吃 .pbtn.sm 的固定 24px')
  const pbtn = parsed
    .map((row, index) => ({ ...row, index }))
    .filter(row => row.selector.split(',').map(part => part.trim()).includes('#island-settings .extension-panel .pbtn'))
  assert.ok(pbtn.length, '通用按钮规则还在（修法不是删掉它）')
  const pbtnRule = pbtn.at(-1)
  assert.equal(value(pbtnRule.body, 'min-height'), '28px', '别的按钮仍按 28px')
  assert.ok(specificity(winner.selector) >= specificity(pbtnRule.selector), `行规则优先级要 ≥ 通用按钮规则（${specificity(winner.selector)} vs ${specificity(pbtnRule.selector)}）`)
  assert.ok(winner.index > pbtnRule.index, '行规则必须写在通用按钮规则之后，否则同档也压不住')
})
