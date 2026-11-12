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
  for (const retired of ['scripts/build-single-exe.ps1', 'scripts/prepare-single-exe.mjs', '../docs/single-exe-build.md', '../docs/specs/2026-10-19-single-exe-design.md']) {
    assert.equal(existsSync(path.join(root, retired)), false, `${retired} 属于退役的便携单 EXE 链，不应保留`)
  }
  for (const kept of ['scripts/prepare-installer.mjs', 'scripts/installer.mjs', 'scripts/build-installer.ps1', '../docs/installer-build.md']) {
    assert.equal(existsSync(path.join(root, kept)), true, `${kept} 是安装版构建入口，必须存在`)
  }
})

test('运行时不再内嵌归档，改为使用安装目录里的资源', () => {
  const runtime = readFileSync(path.join(root, 'src-tauri/src/runtime.rs'), 'utf8')
  assert.doesNotMatch(runtime, /include_bytes!|runtime\.tar|flate2::read::GzDecoder/)
  // 2.0.4 T-Q（t33/F6）：资源目录只有一个解析点——旧断言只看字符串出现过 DSH_FISH_RESOURCES，
  // 管不住「到底有没有接线」。现在钉住 runtime.rs 调用 config::resources_override，
  // 且不再自己读环境变量。
  // #73：原来还有 windhawk_bundle.rs 这个第二解析点，随内置引擎一起删了——
  // 这里反过来钉住它不许回来（再出现就说明内置引擎被悄悄加回发行包）。
  assert.match(runtime, /config::resources_override\(\)/, '安装资源路径必须经 config::resources_override 收口')
  assert.doesNotMatch(runtime, /var_os\(\s*[`"'](?:DSH_FISH|FISH_DOCK)_RESOURCES/, 'runtime.rs 不许再自己读资源环境变量')
  assert.match(runtime, /fn resources_directory/)
  assert.equal(existsSync(path.join(root, 'src-tauri/src/windhawk_bundle.rs')), false, '#73：内置 Windhawk 引擎的模块不许回到源码树')
  const required = runtime.match(/pub const REQUIRED: \[&str; \d+\] = \[([\s\S]*?)\];/)?.[1] || ''
  assert.doesNotMatch(required, /windhawk\//, '#73：启动必需清单里不再有 windhawk/*')
  const manifest = readFileSync(path.join(root, 'src-tauri/Cargo.toml'), 'utf8')
  assert.doesNotMatch(manifest, /^single-exe\s*=/m, 'feature 已改名为 installed')
  assert.match(manifest, /^installed\s*=\s*\[\]/m)
})

test('构建入口按「准备资源 → 编译 → 打包」三段串起来，且不再要求内嵌归档', () => {
  const build = readFileSync(path.join(root, 'scripts/build-installer.ps1'), 'utf8')
  assert.match(build, /prepare-installer\.mjs/)
  // 2.0.4 T-L：编译这一段收进 build-release.ps1（加 --remap-path-prefix 并扫路径残留），
  // 安装器脚本只负责接线，features 由 -Features installed 传进去。
  assert.match(build, /build-release\.ps1/)
  assert.match(build, /-Features installed/)
  assert.match(build, /installer\.mjs/)
  const release = readFileSync(path.join(root, 'scripts/build-release.ps1'), 'utf8')
  assert.match(release, /'--features'/, 'features 由参数拼进 cargo 命令行')
  assert.match(release, /--remap-path-prefix/, '编译时洗掉开发机绝对路径')
  assert.match(release, /UTF16|Unicode/, '构建后按 UTF-8 与 UTF-16LE 两种写法扫残留')
  assert.doesNotMatch(build, /DSH_FISH_RUNTIME_BUNDLE/)
  const prepare = readFileSync(path.join(root, 'scripts/prepare-installer.mjs'), 'utf8')
  assert.doesNotMatch(prepare, /runtime\.tar\.gz/, '安装资源不压缩成归档，安装器直接摆放文件')
})
