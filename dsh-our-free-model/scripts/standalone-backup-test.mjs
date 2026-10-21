import assert from 'node:assert/strict'
import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import { test } from 'node:test'
import { startStandalone } from '../packages/standalone/service.mjs'
import { createCredentials } from '../packages/standalone/channels/credentials.mjs'
import { unzipSync, strFromU8 } from '../packages/standalone/vendor/fflate/fflate.mjs'

test('OFM export requires local authenticated explicit confirmation and only contains OFM migration files', async () => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'ofm-export-'))
  const dir = path.join(root, 'ofm')
  fs.mkdirSync(dir)
  const outsideSecret = 'outside-host-secret-fixture'
  fs.writeFileSync(path.join(root, '.credentials.yaml'), outsideSecret)
  fs.writeFileSync(path.join(dir, 'debug.log'), 'must-not-export-debug-log')
  fs.writeFileSync(path.join(dir, 'eac-user.json'), JSON.stringify({ token: 'eac-fixture-token', login: 'fixture', savedAt: 123 }))
  let service
  const call = (key, body, headers = {}, method = 'POST') => fetch(`${service.url}/api/management/backup`, {
    method, headers: { connection: 'close', authorization: `Bearer ${key}`, 'content-type': 'application/json', ...headers },
    body: method === 'GET' ? undefined : JSON.stringify(body),
  })
  try {
    service = await startStandalone({ dataDir: dir, port: 0, refresh: false, logger: { warn() {} } })
    const key = JSON.parse(fs.readFileSync(service.keyFile, 'utf8')).forwardKey
    assert.equal((await call('', { confirm: true })).status, 401)
    assert.equal((await call(key, { confirm: true }, { origin: 'https://foreign.invalid' })).status, 403)
    assert.equal((await call(key, undefined, {}, 'GET')).status, 404)
    assert.equal((await call(key, {})).status, 400)
    assert.equal((await call(key, { confirm: true, path: root })).status, 400)
    const response = await call(key, { confirm: true })
    assert.equal(response.status, 200)
    assert.equal(response.headers.get('content-type'), 'application/zip')
    assert.equal(response.headers.get('cache-control'), 'no-store')
    assert.match(response.headers.get('content-disposition'), /^attachment; filename="OFM-backup-[0-9T-]+\.zip"$/)
    const files = unzipSync(new Uint8Array(await response.arrayBuffer()))
    assert.deepEqual(Object.keys(files).sort(), ['README.md', 'channel-credentials.json', 'channel-pack/accounts-backup.json', 'channel-pack/permanent-locks.json', 'channel-pack/state.json', 'eac-user.json', 'manifest.json', 'settings.json'])
    const settings = JSON.parse(strFromU8(files['settings.json']))
    assert.equal(settings.forwardKey, key)
    assert.equal(settings.standaloneLanEnabled, false)
    assert.equal(JSON.parse(strFromU8(files['eac-user.json'])).token, 'eac-fixture-token')
    assert.deepEqual(JSON.parse(strFromU8(files['channel-credentials.json'])), {})
    const manifest = JSON.parse(strFromU8(files['manifest.json']))
    assert.equal(manifest.format, 'fish-dock/ofm-backup')
    assert.equal(manifest.version, 1)
    assert.equal(manifest.encrypted, false)
    assert.equal(manifest.files.length, 7)
    const all = Object.values(files).map(strFromU8).join('\n')
    assert.ok(!all.includes(outsideSecret))
    assert.ok(!all.includes('must-not-export-debug-log'))
    assert.ok(!all.includes(root), 'do not include local absolute paths')
    assert.match(strFromU8(files['README.md']), /明文/)
    assert.match(strFromU8(files['README.md']), /FISH Dock/)
    const second = unzipSync(new Uint8Array(await (await call(key, { confirm: true })).arrayBuffer()))
    assert.deepEqual(second['README.md'], files['README.md'], 'migration instructions must be a fixed template')
    assert.ok(!fs.readdirSync(dir).some(name => name.endsWith('.zip')), 'no plaintext ZIP persists on the server')
    const credential = JSON.stringify({ accessToken: 'synthetic-migration-token' })
    const seed = { format: 'dsh-codearts-auth/backup', version: 1, exportedAt: new Date().toISOString(),
      accounts: [{ id: 'buddy-migrate', provider: 'buddy', nickname: '迁移替身', enabled: false, refreshable: false, createdAt: 1, credentialRef: 'BUDDY_MIGRATE' }],
      credentials: { BUDDY_MIGRATE: credential }, disabledModels: { buddy: { 'fixture-disabled': true } }, permanentLocks: { buddy: true },
    }
    const imported = await service.channels.rpc({ method: 'backup.import', payload: { payload: seed } })
    assert.equal(imported.ok, true)
    const saved = unzipSync(new Uint8Array(await (await call(key, { confirm: true })).arrayBuffer()))
    assert.equal(JSON.parse(strFromU8(saved['channel-credentials.json'])).BUDDY_MIGRATE, credential)
    assert.deepEqual(JSON.parse(strFromU8(saved['channel-pack/permanent-locks.json'])).locks, { buddy: true })
    const target = path.join(root, 'restored')
    fs.mkdirSync(target)
    for (const [name, bytes] of Object.entries(saved)) {
      if (name === 'README.md' || name === 'manifest.json') continue
      const destination = path.join(target, name)
      fs.mkdirSync(path.dirname(destination), { recursive: true })
      fs.writeFileSync(destination, bytes)
    }
    const store = createCredentials(target)
    assert.equal((await store.resolve('BUDDY_MIGRATE')).value, credential)
    store.dispose()
    const restored = await startStandalone({ dataDir: target, port: 0, refresh: false, logger: { warn() {} } })
    try {
      const accounts = await restored.channels.rpc({ method: 'account.list', payload: { provider: 'buddy' } })
      assert.equal(accounts.value.accounts[0].nickname, '迁移替身')
      assert.equal(accounts.value.accounts[0].enabled, false)
      const lock = await restored.channels.rpc({ method: 'credits.permanentLock', payload: { provider: 'buddy' } })
      assert.equal(lock.value.locked, true)
      const again = await restored.channels.rpc({ method: 'backup.export', payload: {} })
      assert.deepEqual(again.value.payload.disabledModels, seed.disabledModels)
    } finally { await restored.close() }
  } finally {
    await service?.close()
    assert.ok(fs.realpathSync(root).startsWith(fs.realpathSync(os.tmpdir()) + path.sep))
    fs.rmSync(root, { recursive: true, force: true })
  }
})
