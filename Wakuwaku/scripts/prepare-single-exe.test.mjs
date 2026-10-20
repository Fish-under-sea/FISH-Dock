// 便携单 EXE 链已退役：分发物只有安装包（安装时展开，日常直接用安装资源）。
// 本文件名保留给既有的验证入口，内容改为「守门」——防止旧的自解压链被悄悄加回来，
// 同时确认安装版的三段构建入口都在。
import assert from 'node:assert/strict'
import { test } from 'node:test'
import { existsSync, readFileSync } from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')

test('便携单 EXE 的构建入口已移除，只保留安装包这一条构建链', () => {
  for (const retired of ['scripts/build-single-exe.ps1', 'scripts/prepare-single-exe.mjs', 'docs/single-exe-build.md', 'docs/specs/2026-10-19-single-exe-design.md']) {
    assert.equal(existsSync(path.join(root, retired)), false, `${retired} 属于退役的便携单 EXE 链，不应保留`)
  }
  for (const kept of ['scripts/prepare-installer.mjs', 'scripts/installer.mjs', 'scripts/build-installer.ps1', 'docs/installer-build.md']) {
    assert.equal(existsSync(path.join(root, kept)), true, `${kept} 是安装版构建入口，必须存在`)
  }
})

test('运行时不再内嵌归档，改为使用安装目录里的资源', () => {
  const runtime = readFileSync(path.join(root, 'src-tauri/src/runtime.rs'), 'utf8')
  assert.doesNotMatch(runtime, /include_bytes!|runtime\.tar|flate2::read::GzDecoder/)
  assert.match(runtime, /DSH_FISH_RESOURCES/, '安装资源路径要能被显式指定')
  assert.match(runtime, /fn resources_directory/)
  const manifest = readFileSync(path.join(root, 'src-tauri/Cargo.toml'), 'utf8')
  assert.doesNotMatch(manifest, /^single-exe\s*=/m, 'feature 已改名为 installed')
  assert.match(manifest, /^installed\s*=\s*\[\]/m)
})

test('构建入口按「准备资源 → 编译 → 打包」三段串起来，且不再要求内嵌归档', () => {
  const build = readFileSync(path.join(root, 'scripts/build-installer.ps1'), 'utf8')
  assert.match(build, /prepare-installer\.mjs/)
  assert.match(build, /--features installed/)
  assert.match(build, /installer\.mjs/)
  assert.doesNotMatch(build, /DSH_FISH_RUNTIME_BUNDLE/)
  const prepare = readFileSync(path.join(root, 'scripts/prepare-installer.mjs'), 'utf8')
  assert.doesNotMatch(prepare, /runtime\.tar\.gz/, '安装资源不压缩成归档，安装器直接摆放文件')
})
