// 硬编码自查（scripts/check-hardcoded.mjs）的「存在性」测试 —— 价值是「脚本坏了能红」。
// 背景：改名 Wakuwaku/→app/ 后该脚本曾用 process.cwd() 定位 ROOT，两种 cwd 都说白名单
// 读不到直接崩，却因为没人调用而静默失效（51 个测试全绿也没暴露）。这里钉住三条：
//   ① 退出码 0；② 被扫描文件数 > 0（防再次静默空扫）；③ remaining 为 0。
// 从系统临时目录（与仓库无关的中立 cwd）调用脚本自身，同时证明不再依赖 cwd。
import assert from 'node:assert/strict'
import { execFileSync } from 'node:child_process'
import os from 'node:os'
import path from 'node:path'
import { test } from 'node:test'
import { fileURLToPath } from 'node:url'

const here = path.dirname(fileURLToPath(import.meta.url))
const script = path.join(here, '../scripts/check-hardcoded.mjs')

function audit(cwd) {
  let code = 0
  let stdout = ''
  try {
    stdout = execFileSync(process.execPath, [script, '--json'], { encoding: 'utf8', cwd })
  } catch (error) {
    code = typeof error.status === 'number' ? error.status : 1
    stdout = String(error.stdout || '')
  }
  // 有未清零命中时脚本 exit 1，但 stdout 仍是完整 JSON —— 照样解析出来给人看。
  return { code, report: stdout.trim() ? JSON.parse(stdout) : null }
}

test('硬编码自查：脚本自身可跑、扫描文件数>0、remaining=0', () => {
  const { code, report } = audit(os.tmpdir()) // 中立 cwd，证明不依赖 process.cwd()
  assert.equal(code, 0, `check-hardcoded 退出码 ${code}（应为 0）`)
  assert.ok(report, 'check-hardcoded --json 没有输出')
  assert.ok(report.scanned > 0, `被扫描文件数为 ${report.scanned}，脚本可能没解析到源码（静默空扫）`)
  const left = report.remaining || []
  const lines = left.slice(0, 12).map((r) => `${r.file}:${r.no} ${r.category} ${r.text}`)
  assert.equal(report.totals.remaining, 0, `仍有 ${report.totals.remaining} 条未清零硬编码：\n${lines.join('\n')}`)
  assert.deepEqual(left, [])
})
