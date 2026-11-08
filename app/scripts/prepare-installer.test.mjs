// 安装资源清单与构建期准备：先红后绿，覆盖安装版关键约束。
import assert from 'node:assert/strict'
import { test } from 'node:test'
import { mkdtemp, mkdir, writeFile, readFile, rm, stat } from 'node:fs/promises'
import os from 'node:os'
import path from 'node:path'
import { hashFile, inventory, writeManifest, NOTICE_TEXT, REQUIRED, MANIFEST_SCHEMA, PRODUCT, productVersion } from './prepare-installer.mjs'

async function fakeResources() {
  const root = await mkdtemp(path.join(os.tmpdir(), 'fish-res-'))
  for (const name of REQUIRED) {
    await mkdir(path.dirname(path.join(root, name)), { recursive: true })
    await writeFile(path.join(root, name), `payload for ${name}\n`)
  }
  return root
}

// 2.1.9：固定版 WebView2 与它的许可不再随包，清单也不再写 webview2_version。
const OPTIONS = { version: '0.3.3', nodeVersion: '24.14.0' }

test('资源清单记录每个文件的字节与 SHA-256，且不含清单自身', async () => {
  const root = await fakeResources()
  try {
    const manifest = await writeManifest(root, OPTIONS)
    assert.equal(manifest.schema, MANIFEST_SCHEMA)
    assert.equal(manifest.product, PRODUCT)
    assert.equal(manifest.architecture, 'x86_64')
    assert.deepEqual(manifest.files.map(f => f.path), [...manifest.files.map(f => f.path)].sort())
    assert.equal(manifest.files.some(f => f.path === 'manifest.json'), false, '清单不能把自己写进文件表')
    assert.equal(manifest.files.length, REQUIRED.length)
    for (const record of manifest.files) {
      assert.equal(record.bytes, (await stat(path.join(root, record.path))).size)
      assert.equal(record.sha256, await hashFile(path.join(root, record.path)))
    }
    const written = JSON.parse(await readFile(path.join(root, 'manifest.json'), 'utf8'))
    assert.deepEqual(written, manifest)
  } finally { await rm(root, { recursive: true, force: true }) }
})

test('缺少必需资源时准备清单直接失败，并点名缺哪个文件', async () => {
  const root = await fakeResources()
  try {
    await rm(path.join(root, 'node/node.exe'))
    await assert.rejects(writeManifest(root, OPTIONS), /node\/node\.exe/)
    await assert.rejects(stat(path.join(root, 'manifest.json')), /ENOENT/, '失败时不能留下半份清单')
  } finally { await rm(root, { recursive: true, force: true }) }
})

test('清单已存在时拒绝覆盖，避免把两次构建混在一起', async () => {
  const root = await fakeResources()
  try {
    await writeManifest(root, OPTIONS)
    await assert.rejects(writeManifest(root, OPTIONS), /EEXIST/)
  } finally { await rm(root, { recursive: true, force: true }) }
})

test('清单缺少产品版本时拒绝生成', async () => {
  const root = await fakeResources()
  try {
    await assert.rejects(writeManifest(root, { version: '', nodeVersion: '24.14.0' }), /version/)
  } finally { await rm(root, { recursive: true, force: true }) }
})

test('资源清单位于安装资源目录，与程序读取路径一致', async () => {
  const root = await fakeResources()
  try {
    await writeManifest(root, OPTIONS)
    const names = (await inventory(root)).map(f => f.path)
    assert.ok(names.includes('manifest.json'), '清单必须和资源放在同一目录')
    for (const required of REQUIRED) assert.ok(names.includes(required), required)
  } finally { await rm(root, { recursive: true, force: true }) }
})

// 2.1.9（#78）：Setup 前置检测系统 Evergreen 运行时，安装包不再携带固定版 WebView2
// 与它的许可原文；清单也随之不再写 webview2_version（宿主 runtime.rs 的 Manifest
// 把它当可选字段，2.1.8 及以前的旧清单仍能解析）。
test('清单不再写 webview2_version，也不再把固定版运行时列为必需资源', async () => {
  const root = await fakeResources()
  try {
    const manifest = await writeManifest(root, OPTIONS)
    assert.deepEqual(Object.keys(manifest).sort(),
      ['architecture', 'files', 'node_version', 'product', 'schema', 'version'],
      '清单键集合变了就要同步 runtime.rs 的 Manifest 结构（2.1.9 起 webview2_version 是可选的旧字段）')
    assert.equal('webview2_version' in manifest, false, '不再随包固定版 WebView2，清单也不再写它')
    assert.equal(manifest.files.some(file => file.path.startsWith('webview2/')), false)
    assert.equal(REQUIRED.some(name => name.startsWith('webview2/')), false, '必需资源里不许再有 webview2/*')
  } finally { await rm(root, { recursive: true, force: true }) }
})

test('NOTICE 不再引用固定版 WebView2 与它的许可原文，但要说清运行时由系统提供', () => {
  assert.doesNotMatch(NOTICE_TEXT, /Fixed Version|WebView2-Fixed-Version|SmartScreen|fixed=true/i)
  assert.match(NOTICE_TEXT, /Node\.js/, 'Node 的许可入口要留着')
  assert.match(NOTICE_TEXT, /Wakuwaku/, '上游许可原文要留着')
  assert.match(NOTICE_TEXT, /Evergreen/, '要说明 WebView2 由系统提供，用户才知道缺它时去装什么')
})

test('产品版本取自 tauri.conf.json，安装包版本不会另行漂移', async () => {
  const config = JSON.parse(await readFile(new URL('../src-tauri/tauri.conf.json', import.meta.url), 'utf8'))
  assert.equal(await productVersion(), config.version)
  const cargo = await readFile(new URL('../src-tauri/Cargo.toml', import.meta.url), 'utf8')
  assert.match(cargo, new RegExp(`^version = "${config.version.replace(/\./g, '\\.')}"$`, 'm'), 'Cargo.toml 与 tauri.conf.json 的版本必须一致')
})
