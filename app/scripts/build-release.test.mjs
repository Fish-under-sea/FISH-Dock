// build-release.ps1 的路径残留扫描，按**行为**验：造一份只含某种编码泄漏的合成产物，
// 真跑脚本，断言命中偏移与非零退出。不是 grep 源码里出现 "Unicode" 字样。
//
// 为什么要有这条用例：t20 审查 F1 发现扫描的 UTF-16LE 分支是空操作——
// pattern 用 `$encoding.GetString($encoding.GetBytes($needle))`（还原成原串），
// 而 haystack 是 Latin1 字节映射，纯 UTF-16LE 的路径永远匹配不到。
// 这条用例在旧实现上会失败（utf16 用例结果 -1），修好后通过。
import assert from 'node:assert/strict'
import { test } from 'node:test'
import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import { spawnSync } from 'node:child_process'
import { fileURLToPath } from 'node:url'

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')
const script = path.join(root, 'scripts/build-release.ps1')
const manifest = path.join(root, 'src-tauri/Cargo.toml')
// 脚本自己从 manifest 推仓库根（app/src-tauri/.. = app，再上一层 = 仓库根），这里保持一致。
const repo = path.resolve(root, '..')
const pwsh = process.env.FISH_DOCK_PWSH || 'pwsh'
const cargo = process.env.FISH_DOCK_CARGO || 'cargo'

const PREFIX = Buffer.from('MZ')
const PAD = Buffer.alloc(16) // 让偏移不是 0，顺便证明报的是真实偏移

function pad(count) { return Buffer.alloc(count, 0x5a) }

// 只有 UTF-16LE 一份，没有同串的 UTF-8：这是旧实现漏检的那一种。
function utf16Only(text) { return Buffer.concat([PREFIX, PAD, Buffer.from(text, 'utf16le'), pad(8)]) }
// 只有 UTF-8 一份。
function utf8Only(text) { return Buffer.concat([PREFIX, PAD, Buffer.from(text, 'utf8'), pad(8)]) }
// 干净产物：只随机填充，不含任何本机路径。
function clean() { return Buffer.concat([PREFIX, PAD, pad(64)]) }

function run(fixture) {
  const base = fs.mkdtempSync(path.join(os.tmpdir(), 'fish-release-scan-'))
  const target = path.join(base, 'target')
  fs.mkdirSync(path.join(target, 'release'), { recursive: true })
  const exe = path.join(target, 'release', 'FISH-Dock.exe')
  fs.writeFileSync(exe, fixture)
  const out = path.join(base, 'out.txt')
  const fd = fs.openSync(out, 'w')
  let result
  try {
    // 用文件句柄收输出：本机沙箱下管道 stdio 可能被拒（EPERM）。
    result = spawnSync(pwsh, [
      '-NoProfile', '-File', script,
      '-Cargo', cargo, '-Manifest', manifest, '-SkipBuild',
    ], {
      env: { ...process.env, CARGO_TARGET_DIR: target },
      stdio: ['ignore', fd, fd],
      windowsHide: true,
      timeout: 120000,
    })
  } finally {
    fs.closeSync(fd)
  }
  const text = fs.readFileSync(out, 'utf8')
  fs.rmSync(base, { recursive: true, force: true })
  return { status: result.status, signal: result.signal, error: result.error, text }
}

function requirePwsh() {
  const probe = spawnSync(pwsh, ['-NoProfile', '-NonInteractive', '-Command', '$PSVersionTable.PSVersion.Major'], { encoding: 'utf8', timeout: 60000 })
  assert.equal(probe.error, undefined, `本机没有可用的 ${pwsh}，无法跑行为测试：${probe.error?.message}`)
  assert.equal(probe.status, 0, `pwsh 启动失败：${probe.stderr}`)
}

test('只以 UTF-16LE 出现的本机路径要被扫出来（命中偏移 + 非零退出）', () => {
  requirePwsh()
  const { status, text } = run(utf16Only(repo))
  const offset = PREFIX.length + PAD.length
  assert.equal(status, 1, `应当失败退出：${text}`)
  assert.match(text, new RegExp(`路径残留：.*偏移 ${offset} 处出现`), `要报出真实偏移 ${offset}：${text}`)
  assert.match(text, /已失败/, text)
})

test('只以 UTF-8 出现的本机路径同样要被扫出来', () => {
  requirePwsh()
  const { status, text } = run(utf8Only(repo))
  const offset = PREFIX.length + PAD.length
  assert.equal(status, 1, `应当失败退出：${text}`)
  assert.match(text, new RegExp(`路径残留：.*偏移 ${offset} 处出现`), text)
})

test('不含本机路径的产物通过扫描', () => {
  requirePwsh()
  const { status, text } = run(clean())
  assert.equal(status, 0, `干净产物不应失败：${text}`)
  assert.match(text, /路径扫描通过/, text)
})