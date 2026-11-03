// 任务栏指示器（2.0.4 T-G / T-H）：时钟的秒与 Wi-Fi 格数的纯函数。
//
// 页面侧只做这两件「读来的值怎么显示」的事；真正的值来自宿主
// （taskbar.rs：注册表 ShowSecondsInSystemClock、tasks.rs：WLAN 信号质量）。
const test = require('node:test')
const assert = require('node:assert/strict')
const { readFileSync } = require('node:fs')
const vm = require('node:vm')
const path = require('node:path')

function loadIndicators() {
  const file = path.join(__dirname, '..', 'src', 'pet', 'indicators.js')
  const sandbox = {}
  vm.runInNewContext(readFileSync(file, 'utf8'), sandbox, { filename: file })
  return sandbox.FishIndicators
}

test('the clock shows seconds only when Windows does', () => {
  const { clockTime } = loadIndicators()
  const at = new Date(2026, 10, 3, 15, 6, 9)
  assert.equal(clockTime(at, 'zh', false), '15:06')
  assert.equal(clockTime(at, 'zh', true), '15:06:09')
  // A language without a fixed layout only has to carry the seconds.
  const english = clockTime(at, 'en', true)
  assert.match(english, /06/)
  assert.match(english, /09/)
  assert.notEqual(english, clockTime(at, 'en', false))
})

test('an unknown strength stays unknown, and 0 to 5 pass through', () => {
  const { signalBars } = loadIndicators()
  for (const bars of [0, 1, 2, 3, 4, 5]) assert.equal(signalBars(bars), bars)
  for (const unknown of [null, undefined, -1, 6, 2.5, '3', true, NaN]) {
    assert.equal(signalBars(unknown), null, `${unknown} 必须按未知处理`)
  }
})