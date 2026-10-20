// 安装资源清单与构建期准备：先红后绿，覆盖安装版关键约束。
import assert from 'node:assert/strict'
import { test } from 'node:test'
import { mkdtemp, mkdir, writeFile, readFile, rm, stat } from 'node:fs/promises'
import os from 'node:os'
import path from 'node:path'
import { hashFile, inventory, writeManifest, REQUIRED, MANIFEST_SCHEMA, PRODUCT, productVersion } from './prepare-installer.mjs'

async function fakeResources() {
  const root = await mkdtemp(path.join(os.tmpdir(), 'fish-res-'))
  for (const name of REQUIRED) {
    await mkdir(path.dirname(path.join(root, name)), { recursive: true })
    await writeFile(path.join(root, name), `payload for ${name}\n`)
  }
  await mkdir(path.join(root, 'licenses'), { recursive: true })
  await writeFile(path.join(root, 'licenses/WebView2-Fixed-Version.html'), '<html>terms</html>\n')
  return root
}

test('资源清单记录每个文件的字节与 SHA-256，且不含清单自身', async () => {
  const root = await fakeResources()
  try {
    const manifest = await writeManifest(root, { version: '0.3.3', nodeVersion: '24.14.0', webview2Version: '155.0.4283.62' })
    assert.equal(manifest.schema, MANIFEST_SCHEMA)
    assert.equal(manifest.product, PRODUCT)
    assert.equal(manifest.architecture, 'x86_64')
    assert.deepEqual(manifest.files.map(f => f.path), [...manifest.files.map(f => f.path)].sort())
    assert.equal(manifest.files.some(f => f.path === 'manifest.json'), false, '清单不能把自己写进文件表')
    assert.equal(manifest.files.length, REQUIRED.length + 1)
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
    await assert.rejects(
      writeManifest(root, { version: '0.3.3', nodeVersion: '24.14.0', webview2Version: '155.0.4283.62' }),
      /node\/node\.exe/,
    )
    await assert.rejects(stat(path.join(root, 'manifest.json')), /ENOENT/, '失败时不能留下半份清单')
  } finally { await rm(root, { recursive: true, force: true }) }
})

test('清单已存在时拒绝覆盖，避免把两次构建混在一起', async () => {
  const root = await fakeResources()
  try {
    const options = { version: '0.3.3', nodeVersion: '24.14.0', webview2Version: '155.0.4283.62' }
    await writeManifest(root, options)
    await assert.rejects(writeManifest(root, options), /EEXIST/)
  } finally { await rm(root, { recursive: true, force: true }) }
})

test('清单缺少产品版本时拒绝生成', async () => {
  const root = await fakeResources()
  try {
    await assert.rejects(writeManifest(root, { version: '', nodeVersion: '24.14.0', webview2Version: '155.0.4283.62' }), /version/)
  } finally { await rm(root, { recursive: true, force: true }) }
})

test('资源清单位于安装资源目录，与程序读取路径一致', async () => {
  const root = await fakeResources()
  try {
    await writeManifest(root, { version: '0.3.3', nodeVersion: '24.14.0', webview2Version: '155.0.4283.62' })
    const names = (await inventory(root)).map(f => f.path)
    assert.ok(names.includes('manifest.json'), '清单必须和资源放在同一目录')
    for (const required of REQUIRED) assert.ok(names.includes(required), required)
  } finally { await rm(root, { recursive: true, force: true }) }
})

test('产品版本取自 tauri.conf.json，安装包版本不会另行漂移', async () => {
  const config = JSON.parse(await readFile(new URL('../src-tauri/tauri.conf.json', import.meta.url), 'utf8'))
  assert.equal(await productVersion(), config.version)
  const cargo = await readFile(new URL('../src-tauri/Cargo.toml', import.meta.url), 'utf8')
  assert.match(cargo, new RegExp(`^version = "${config.version.replace(/\./g, '\\.')}"$`, 'm'), 'Cargo.toml 与 tauri.conf.json 的版本必须一致')
})
