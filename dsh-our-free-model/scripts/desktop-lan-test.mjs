import assert from 'node:assert/strict'
import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import net from 'node:net'
import { test } from 'node:test'
import { startStandalone } from '../packages/standalone/service.mjs'
import { startLanRelay } from '../src/forward.js'

// Distinct loopback IPs exercise real same-port sockets without exposing the test to the LAN.
const interfaces = () => ({ fixture: [{ address: '127.0.0.2', family: 'IPv4', internal: false }] })
test('desktop LAN uses the current port, separate keys, inference only, persistence and explicit bind failures', async () => {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'ofm-same-port-'))
  let service, blocker
  const started = []
  const startRelay = async options => {
    const relay = await startLanRelay(options)
    started.push(relay)
    return relay
  }
  const options = { dataDir: dir, port: 0, refresh: false, lanSamePort: true, networkInterfaces: interfaces, startRelay, logger: { warn() {} } }
  const saved = () => JSON.parse(fs.readFileSync(service.keyFile, 'utf8'))
  const call = (base, route, key, body) => fetch(base + route, {
    method: body === undefined ? 'GET' : 'POST',
    headers: { connection: 'close', authorization: `Bearer ${key}`, ...body === undefined ? {} : { 'content-type': 'application/json' } },
    body: body === undefined ? undefined : JSON.stringify(body), signal: AbortSignal.timeout(5000),
  })
  try {
    service = await startStandalone(options)
    assert.equal(service.lan.enabled, false)
    const key = saved().forwardKey
    const update = patch => call(service.url, '/api/management/settings', key, patch)
    assert.equal((await update({ standaloneLanEnabled: true })).status, 200)
    assert.equal(service.lan.running, true)
    assert.equal(service.lan.port, service.port, 'desktop LAN must not silently choose another port')
    assert.deepEqual(service.lan.baseUrls, [`http://127.0.0.2:${service.port}/v1`])
    assert.equal(started.at(-1).server.address().address, '127.0.0.2')
    const remote = `http://127.0.0.2:${service.port}`
    let lanKey = saved().standaloneLanKey
    assert.ok(lanKey && lanKey !== key)
    for (const wrong of ['', 'wrong', key]) assert.equal((await call(remote, '/v1/models', wrong)).status, 401)
    assert.equal((await call(remote, '/v1/models', lanKey)).status, 200)
    for (const route of ['/', '/health', '/assets/app.js', '/api/management/summary', '/api/management/key', '/api/management/backup']) {
      assert.equal((await call(remote, route, lanKey)).status, 404, route)
    }
    assert.equal((await call(service.url, '/api/management/summary', lanKey)).status, 401)
    const rotated = await call(service.url, '/api/management/lan/key/rotate', key, { confirm: true })
    const previous = lanKey
    lanKey = (await rotated.json()).key
    assert.equal((await call(remote, '/v1/models', previous)).status, 401)
    assert.equal((await call(remote, '/v1/models', lanKey)).status, 200)
    const running = started.length
    await update({ standaloneLanEnabled: true })
    assert.equal(started.length, running)
    await service.close()
    service = await startStandalone(options)
    assert.equal(service.lan.port, service.port)
    assert.equal(saved().standaloneLanKey, lanKey)
    await update({ standaloneLanEnabled: false })
    assert.equal(service.lan.running, false)
    blocker = net.createServer(socket => socket.destroy())
    await new Promise((resolve, reject) => { blocker.once('error', reject); blocker.listen(service.port, '127.0.0.2', resolve) })
    await update({ standaloneLanEnabled: true })
    assert.equal(service.lan.running, false)
    assert.equal(service.lan.port, null)
    assert.match(service.lan.error, /EADDRINUSE/)
    assert.deepEqual(service.lan.baseUrls, [])
    assert.equal((await call(service.url, '/api/management/summary', key)).status, 200)
    await new Promise(resolve => blocker.close(resolve)); blocker = undefined
    await update({ standaloneLanEnabled: true })
    assert.equal(service.lan.port, service.port)
    await update({ enabled: false })
    assert.equal(service.lan.running, false)
  } finally {
    await service?.close()
    if (blocker) await new Promise(resolve => blocker.close(resolve))
    assert.equal(started.filter(relay => relay.server.listening).length, 0)
    assert.ok(fs.realpathSync(dir).startsWith(fs.realpathSync(os.tmpdir()) + path.sep))
    fs.rmSync(dir, { recursive: true, force: true })
  }
})
