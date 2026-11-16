// CI 工作流（DEC-9，上游 b32e9352 的等价实现）：Windows runner 上跑 Rust/Node/文档检查，
// 失败阻断；不放令牌与发布凭据；不缓存 target/ 与资源包。用源码断言钉住这些口径。
const assert = require('node:assert/strict')
const fs = require('node:fs')
const path = require('node:path')
const { test } = require('node:test')

const file = path.join(__dirname, '../../.github/workflows/ci.yml')
const yml = fs.readFileSync(file, 'utf8')

test('CI 在 Windows runner 上跑 Rust / Node / 文档三类，失败阻断', () => {
  assert.match(yml, /runs-on:\s*windows-latest/, '用 Windows runner')
  // 三个 job
  for (const name of ['rust:', 'node:', 'docs:']) assert.match(yml, new RegExp(`\\n  ${name}`), `有 ${name} job`)
  // 与本地 verify 同一条命令（Rust / Node 全量）
  assert.match(yml, /cargo test --locked --no-default-features --manifest-path app\/src-tauri\/Cargo\.toml -- --test-threads=1/, 'Rust 全量命令')
  assert.match(yml, /node --test --test-isolation=none "app\/test\/\*\.test\.js"/, 'Node 页面命令')
  assert.match(yml, /sdk\/node\/\*\.test\.mjs/, 'Node SDK 命令')
  assert.match(yml, /tools\/docs\/check-readme\.mjs/, 'README 检查')
})

test('CI 不放任何令牌 / 发布凭据，也不做发布动作', () => {
  // 常见的令牌名都不该出现在工作流里。
  for (const token of ['GITEE_TOKEN', 'GH_TOKEN', 'GITHUB_TOKEN:', 'NPM_TOKEN', 'secrets.', 'password', 'api[_-]?key']) {
    assert.doesNotMatch(yml, new RegExp(token, 'i'), `不含 ${token}`)
  }
  // 不做发布：没有 gh release / git push / gitee 发布脚本。
  for (const act of ['gh release', 'git push', 'gitee-release', 'npm publish']) {
    assert.doesNotMatch(yml, new RegExp(act), `不做发布动作：${act}`)
  }
  // 权限只读。
  assert.match(yml, /permissions:\s*\n\s*contents:\s*read/, 'contents: read')
})

test('CI 不缓存 target/ 与资源包（只缓存 cargo registry/git）', () => {
  const cacheBlock = yml.slice(yml.indexOf('actions/cache@'), yml.indexOf('cargo test'))
  assert.doesNotMatch(cacheBlock, /target\/?[\s'"]/, '不缓存 target/')
  assert.match(cacheBlock, /\.cargo\/registry/, '缓存 registry')
  assert.match(cacheBlock, /\.cargo\/git/, '缓存 git 依赖')
})
