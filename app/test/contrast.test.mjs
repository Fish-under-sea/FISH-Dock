// 字色审计（t36）：把 scripts/check-contrast.mjs 当成测试跑 —— 违规数必须为 0。
// 为什么用脚本而不是肉眼：这套 CSS 有 260+ 条文字声明、两套主题，靠截图看不出
// "3.45:1 差一点点"这类问题；这里钉住 baseline，谁再往样式里塞暗色字/浅色底的
// 组合，测试立刻红。
import assert from 'node:assert/strict'
import { execFileSync } from 'node:child_process'
import fs from 'node:fs'
import path from 'node:path'
import { test } from 'node:test'
import { fileURLToPath } from 'node:url'

const here = path.dirname(fileURLToPath(import.meta.url))
const script = path.join(here, '../scripts/check-contrast.mjs')

function audit() {
  try {
    return JSON.parse(execFileSync(process.execPath, [script, '--json'], { encoding: 'utf8' }))
  } catch (error) {
    // 有违规时脚本 exit 1，但 stdout 里仍是完整 JSON —— 照样解析出来给人看。
    const stdout = String(error.stdout || '')
    if (!stdout.trim()) throw error
    return JSON.parse(stdout)
  }
}

test('字色对比度：settings.css + extensions.css 两套主题下违规为 0', () => {
  const report = audit()
  assert.ok(report.summary.checked > 200, `审到的文字声明太少了（${report.summary.checked}），脚本可能没解析到样式`)
  const lines = report.violations.slice(0, 12).map(v => `${v.file}:${v.line} ${v.theme} ${v.prop}=${v.color} on ${v.bgColor} ${v.ratio} ${v.verdict}`)
  assert.deepEqual(report.violations, [], `仍有 ${report.violations.length} 条违规：\n${lines.join('\n')}`)
  assert.equal(report.summary.violations, 0)
})

test('字色对比度：暗色白字 / 浅色深字的方向错为 0', () => {
  const report = audit()
  assert.equal(report.summary.direction, 0, '有文字颜色方向跟主题相反')
})

test('字色对比度：白名单逐条写理由，不许用 TODO 糊', () => {
  const source = fs.readFileSync(script, 'utf8')
  const from = source.indexOf('const WHITELIST')
  const block = source.slice(from, source.indexOf('])', from))
  assert.match(block, /岛外观预览/, '白名单要有具体理由')
  assert.doesNotMatch(block, /todo|暂时|先放着|以后再说/i, '白名单不许用 TODO 糊过去')
})

// t38：作用域 token（--wc-*）纳入展开；剩下解析不到的必须逐条注明来源，不许静默。
test('字色对比度：--wc-* 这类作用域 token 已纳入展开，不再是 UNRESOLVED', () => {
  const report = audit()
  const names = report.summary.uncoveredScope.map(u => u.token)
  assert.equal(names.includes('--wc-ok'), false, '--wc-ok 应从作用域池解析出来')
  assert.equal(names.includes('--wc-warn'), false, '--wc-warn 应从作用域池解析出来')
})

test('字色对比度：每个未覆盖 token 都要注明来源（运行时注入到哪个文件）', () => {
  const report = audit()
  for (const item of report.summary.uncoveredScope) {
    assert.ok(item.at && /^src\/pet\/.+:\d+$/.test(item.at), `${item.token} 没注明来源（${item.at || '空'}）`)
  }
})

