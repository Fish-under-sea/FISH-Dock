import assert from 'node:assert/strict'
import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import { spawnSync } from 'node:child_process'
import { pathToFileURL, fileURLToPath } from 'node:url'
import test from 'node:test'

const root = fileURLToPath(new URL('../', import.meta.url))
const ofm = path.resolve(root, '../dsh-our-free-model')
const required = ['packages/standalone/backup.mjs', 'packages/standalone/backup-readme.txt', 'packages/standalone/vendor/fflate/fflate.mjs', 'packages/standalone/vendor/fflate/LICENSE']

test('两套桌面打包入口都包含 ZIP 备份及其许可，打包后的服务可以导入', async () => {
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
      for (const relative of required) assert.ok(fs.statSync(path.join(destination, relative)).isFile(), relative)
      const service = await import(pathToFileURL(path.join(destination, 'packages/standalone/service.mjs')))
      assert.equal(typeof service.startStandalone, 'function')
    }
  } finally {
    assert.ok(path.basename(temporary).startsWith('fish-stage-regression-'))
    fs.rmSync(temporary, { recursive: true, force: true })
  }
})
