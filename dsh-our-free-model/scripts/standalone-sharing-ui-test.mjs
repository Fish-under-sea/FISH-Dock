import assert from 'node:assert/strict'
import fs from 'node:fs'
import test from 'node:test'

const source = name => fs.readFileSync(new URL(`../packages/standalone/frontend/${name}`, import.meta.url), 'utf8')
test('LAN 和备份控件接入真实宿主，不混用本机密钥，不以成功响应代替文件下载', () => {
  const app = source('app.tsx'), connection = source('connection.tsx'), settings = source('settings.tsx')
  const lan = source('lan.tsx'), backup = source('backup.tsx'), download = source('backup-download.mjs')
  assert.match(connection, /<LanAccess\b/)
  assert.match(settings, /<BackupExport\b/)
  assert.match(lan, /host\.getLanKey\(/)
  assert.match(lan, /host\.rotateLanKey\(/)
  assert.match(lan, /standaloneLanEnabled/)
  assert.match(lan, /\.running/)
  assert.match(lan, /\.error/)
  assert.doesNotMatch(lan, /host\.getApiKey\(/)
  assert.match(backup, /window\.confirm\(/)
  assert.match(backup, /明文/)
  assert.match(backup, /host\.exportBackup\(/)
  assert.match(app, /getLanKey:.*['"]\/lan\/key['"]/)
  assert.match(app, /rotateLanKey:.*['"]\/lan\/key\/rotate['"]/)
  assert.match(app, /exportBackup:/)
  assert.match(download, /response\.blob\(/)
  assert.match(download, /createObjectURL\(/)
  assert.match(download, /revokeObjectURL\(/)
  assert.match(download, /response\.status === 401/)
})
