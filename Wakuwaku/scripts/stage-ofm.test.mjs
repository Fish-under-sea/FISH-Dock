import assert from 'node:assert/strict'
import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import { spawnSync } from 'node:child_process'
import { pathToFileURL, fileURLToPath } from 'node:url'
import test from 'node:test'

const root = fileURLToPath(new URL('../', import.meta.url))
const ofm = path.resolve(root, '../dsh-our-free-model')
const required = ['packages/standalone/backup.mjs', 'packages/standalone/backup-readme.txt', 'packages/standalone/vendor/fflate/fflate.mjs', 'packages/standalone/vendor/fflate/LICENSE',
  'packages/standalone/service.mjs', 'packages/standalone/management.mjs', 'packages/standalone/channels/worker.mjs',
  'packages/standalone/channels/validation.mjs', 'packages/standalone/channels/meoo.mjs', 'packages/standalone/channels/raccoon-daily.mjs',
  'packages/standalone/channels/business.mjs', 'packages/standalone/web/app.js', 'packages/standalone/web/app.css', 'packages/standalone/web/assets.json']

test('两套桌面打包入口包含新增渠道与验证修复，资源逐字节一致且服务可以导入', async () => {
  const temporary = fs.mkdtempSync(path.join(os.tmpdir(), 'fish-stage-regression-'))
  try {
    const integrated = path.join(temporary, 'integrated')
    const result = spawnSync(process.execPath, [path.join(root, 'scripts/stage-ofm.mjs'), '--source', ofm, '--out', integrated], { stdio: 'inherit' })
    assert.ifError(result.error)
    assert.equal(result.status, 0)
    const { stageDesktopService } = await import(pathToFileURL(path.join(ofm, 'scripts/prepare-desktop.mjs')))
    const upstream = path.join(temporary, 'upstream')
    stageDesktopService(upstream)
    for (const destination of [integrated, upstream]) {
      for (const relative of required) {
        assert.ok(fs.statSync(path.join(destination, relative)).isFile(), relative)
        assert.deepEqual(fs.readFileSync(path.join(destination, relative)), fs.readFileSync(path.join(ofm, relative)), relative)
      }
      const assets = JSON.parse(fs.readFileSync(path.join(destination, 'packages/standalone/web/assets.json'), 'utf8'))
      for (const name of assets) assert.deepEqual(fs.readFileSync(path.join(destination, 'packages/standalone/web', name)), fs.readFileSync(path.join(ofm, 'packages/standalone/web', name)), name)
      const service = await import(pathToFileURL(path.join(destination, 'packages/standalone/service.mjs')))
      assert.equal(typeof service.startStandalone, 'function')
    }
  } finally {
    assert.ok(path.basename(temporary).startsWith('fish-stage-regression-'))
    fs.rmSync(temporary, { recursive: true, force: true })
  }
})
