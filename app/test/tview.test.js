// #89 任务视图图标选择的纯逻辑单测（tview.js）：浅/深条带各挑哪张、资源缺失与
// 形状不对（旧宿主把字节数组当 SVG）一律回退字形、data URL 是真 SVG 源。
const assert = require('node:assert/strict')
const { test } = require('node:test')
const { GLYPH, usable, pick, dataUrl } = require('../src/pet/tview')

const LIGHT = '<svg viewBox="0 0 24 24"><path d="M1,7H17"/></svg>'
const DARK = '<svg viewBox="0 0 24 24"><path d="M7,1H23"/></svg>'

test('浅色条带挑字形较深的那张，深色条带挑较浅的那张', () => {
  const icons = { light: LIGHT, dark: DARK }
  assert.equal(pick(icons, true), LIGHT, '浅色任务栏 → light（字形较深）')
  assert.equal(pick(icons, false), DARK, '深色任务栏 → dark（字形较浅）')
})

test('资源缺失（Windows 10 / 读不到）回退 U+E7C4 字形', () => {
  assert.equal(pick(null, true), null)
  assert.equal(pick(null, false), null)
  assert.equal(dataUrl(null, true), null)
  assert.equal(GLYPH, '\u{e7c4}')
})

test('形状不对不当 SVG：旧宿主把字节数组序列化成数字数组', () => {
  // Vec<u8> 走 serde_json 会变成 [60,115,…]：encodeURIComponent 会拼出假 data URL。
  const bytes = { light: [60, 115, 118, 103], dark: [60, 115, 118, 103] }
  assert.equal(usable(bytes), null, '数字数组不是 SVG 源')
  assert.equal(pick(bytes, true), null)
  assert.equal(dataUrl(bytes, true), null)
  // 空串、缺字段、非对象也都不算。
  assert.equal(usable({ light: '', dark: DARK }), null)
  assert.equal(usable({ light: LIGHT }), null)
  assert.equal(usable('nope'), null)
})

test('data URL 里是真 SVG 源（encodeURIComponent 过）', () => {
  const url = dataUrl({ light: LIGHT, dark: DARK }, true)
  assert.ok(url.startsWith('url("data:image/svg+xml;utf8,'), url)
  assert.ok(url.includes(encodeURIComponent(LIGHT)), '整段 SVG 都编进去')
  assert.ok(!url.includes('<svg'), '尖括号已编码')
})
