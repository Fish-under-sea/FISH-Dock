import assert from 'node:assert/strict'
import { test } from 'node:test'
import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import crypto from 'node:crypto'
import { createRequire } from 'node:module'
const require = createRequire(import.meta.url)
const { update } = require('./install-update.cjs')
const sha = b => crypto.createHash('sha256').update(b).digest('hex')
function fixture(t) {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'fish-update-speed-'))
  t.after(() => fs.rmSync(dir, { recursive: true, force: true }))
  const stage = path.join(dir, 'stage'), root = path.join(dir, '安装 中文')
  fs.mkdirSync(stage); fs.mkdirSync(root)
  return { dir, stage, root }
}
function release(stage, rows, payload) {
  for (const [name, bytes] of Object.entries(rows)) {
    if (payload && !payload.includes(name)) continue
    const file = path.join(stage, name); fs.mkdirSync(path.dirname(file), { recursive: true }); fs.writeFileSync(file, bytes)
  }
  fs.writeFileSync(path.join(stage, 'install-manifest.json'), JSON.stringify({ schema: 1, product: 'FISH Dock', version: '1.1.0', files: Object.entries(rows).map(([path, bytes]) => ({ path, bytes: Buffer.byteLength(bytes), sha256: sha(bytes) })), ...(payload ? { payload } : {}) }))
}
function monitored() {
  const counts = { copies: 0, reads: 0, bytesRead: 0 }
  const io = { ...fs, copyFileSync(...args) { counts.copies++; return fs.copyFileSync(...args) }, readSync(...args) { const n = fs.readSync(...args); counts.reads++; counts.bytesRead += n; return n }, readFileSync(...args) { const data = fs.readFileSync(...args); counts.reads++; counts.bytesRead += data.length; return data } }
  return { io, counts }
}
test('同卷首次安装移动已校验载荷，不复制整套运行时', t => {
  const f = fixture(t); release(f.stage, { 'DSH-FISH.exe': 'program', 'resources/file.bin': 'runtime' })
  const m = monitored(); update(f.stage, f.root, m.io)
  assert.equal(fs.readFileSync(path.join(f.root, 'resources/file.bin'), 'utf8'), 'runtime')
  assert.equal(m.counts.copies, 0)
})
test('同版再次检查复用校验记录，损坏文件即使同大小同 mtime 仍重新校验', t => {
  const f = fixture(t), rows = { 'DSH-FISH.exe': 'program', 'resources/file.bin': 'r'.repeat(1024 * 1024) }
  release(f.stage, rows); update(f.stage, f.root)
  const m = monitored(); update(f.stage, f.root, m.io)
  assert.ok(m.counts.bytesRead < 100000, `重复检查读了 ${m.counts.bytesRead} 字节`)
  const file = path.join(f.root, 'resources/file.bin'), prior = fs.statSync(file)
  fs.writeFileSync(file, 'b'.repeat(1024 * 1024)); fs.utimesSync(file, prior.atime, prior.mtime)
  release(f.stage, rows); update(f.stage, f.root)
  assert.equal(sha(fs.readFileSync(file)), sha(rows['resources/file.bin']))
})
test('跨卷回退复制后仍检查摘要；失败不动旧安装', t => {
  const f = fixture(t); release(f.stage, { 'DSH-FISH.exe': 'new', 'resources/file.bin': 'runtime' })
  fs.writeFileSync(path.join(f.root, 'DSH-FISH.exe'), 'old')
  const io = { ...fs, renameSync(from, to) { if (from.startsWith(f.stage) && to.includes('.fish-install-transaction')) { const e = Error('cross volume'); e.code = 'EXDEV'; throw e } return fs.renameSync(from, to) }, copyFileSync(from, to) { fs.copyFileSync(from, to); if (from.endsWith('file.bin')) fs.writeFileSync(to, 'corrupt') } }
  assert.throws(() => update(f.stage, f.root, io), /校验/)
  assert.equal(fs.readFileSync(path.join(f.root, 'DSH-FISH.exe'), 'utf8'), 'old')
})
test('精简更新包只携带变更文件，基础资源缺失时停止且保留旧程序', t => {
  const f = fixture(t), rows = { 'DSH-FISH.exe': 'old', 'resources/file.bin': 'runtime' }
  release(f.stage, rows); update(f.stage, f.root)
  rows['DSH-FISH.exe'] = 'new'; release(f.stage, rows, ['DSH-FISH.exe'])
  update(f.stage, f.root); assert.equal(fs.readFileSync(path.join(f.root, 'DSH-FISH.exe'), 'utf8'), 'new')
  fs.unlinkSync(path.join(f.root, 'resources/file.bin')); rows['DSH-FISH.exe'] = 'newer'; release(f.stage, rows, ['DSH-FISH.exe'])
  assert.throws(() => update(f.stage, f.root), /完整安装包|基础资源/)
  assert.equal(fs.readFileSync(path.join(f.root, 'DSH-FISH.exe'), 'utf8'), 'new')
})
test('同安装卷受管隐藏暂存目录允许原子接收发行文件', t => {
  const f = fixture(t), stage = path.join(f.root, '.fish-dock-stage-fixture')
  fs.mkdirSync(stage); release(stage, { 'DSH-FISH.exe': 'program', 'resources/runtime.bin': 'runtime' })
  update(stage, f.root)
  assert.equal(fs.readFileSync(path.join(f.root, 'resources/runtime.bin'), 'utf8'), 'runtime')
  assert.ok(!fs.existsSync(stage), '事务提交后隐藏 stage 可清理')
})
test('校验记录损坏时回退摘要验证，丢失或修改的资源得到修复', t => {
  const f = fixture(t), rows = { 'DSH-FISH.exe': 'program', 'resources/runtime.bin': 'original' }
  release(f.stage, rows); update(f.stage, f.root)
  fs.writeFileSync(path.join(f.root, 'install-state.json'), '{invalid')
  fs.writeFileSync(path.join(f.root, 'resources/runtime.bin'), 'modified')
  release(f.stage, rows); const result = update(f.stage, f.root)
  assert.equal(fs.readFileSync(path.join(f.root, 'resources/runtime.bin'), 'utf8'), 'original')
  assert.ok(result.hashedFiles > 0)
})
test('只有首次授权或 WebView2 变化时需要递归授权，其余更新不重复授权', t => {
  const f = fixture(t), rows = { 'DSH-FISH.exe': 'program', 'resources/webview2/runtime.bin': 'browser' }
  release(f.stage, rows); assert.equal(update(f.stage, f.root).needsWebviewAcl, true)
  assert.equal(update(f.stage, f.root).needsWebviewAcl, false)
  rows['DSH-FISH.exe'] = 'new-program'; release(f.stage, rows)
  assert.equal(update(f.stage, f.root).needsWebviewAcl, false)
  rows['resources/webview2/runtime.bin'] = 'new-browser'; release(f.stage, rows)
  assert.equal(update(f.stage, f.root).needsWebviewAcl, true)
})
test('明确显示校验与提交进度，日志数量有上限', t => {
  const f = fixture(t), rows = Object.fromEntries(Array.from({ length: 120 }, (_, i) => [`resources/f${i}`, 'synthetic']))
  release(f.stage, rows); const events = []; update(f.stage, f.root, fs, { report: text => events.push(text) })
  // 这些进的是安装器日志（nsExec::ExecToLog 转出去，按 OEM 代码页解释），
  // 所以是英文（#16）：中文一句不进去。
  assert.ok(events.some(text => text.includes('Checking')))
  assert.ok(events.some(text => text.includes('complete')))
  for (const text of events) assert.doesNotMatch(text, /[^\x00-\x7f]/, `安装器日志里出现了非 ASCII：${text}`)
  assert.ok(events.length < 50)
})
