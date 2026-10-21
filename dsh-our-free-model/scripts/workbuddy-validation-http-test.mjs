import assert from 'node:assert/strict'
import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import { test } from 'node:test'
import { startStandalone } from '../packages/standalone/service.mjs'

test('真实管理接口保存开关，拒绝后锁停且重启保留；仅明确确认后发一次', async () => {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'ofm-validation-http-'))
  let service
  const start = () => startStandalone({ dataDir: dir, port: 0, refresh: false, logger: { warn() {} } })
  const call = (route, body, key = true) => fetch(service.url + '/api/management' + route, {
    method: body === undefined ? 'GET' : 'POST', headers: { connection: 'close', ...key ? { authorization: 'Bearer ' + JSON.parse(fs.readFileSync(service.keyFile)).forwardKey } : {}, ...body === undefined ? {} : { 'content-type': 'application/json' } },
    body: body === undefined ? undefined : JSON.stringify(body), signal: AbortSignal.timeout(5000),
  })
  try {
    service = await start()
    assert.equal((await (await call('/summary')).json()).workbuddyValidation.enabled, false)
    assert.equal((await call('/workbuddy/validate', { model: 'workbuddy/fixture', confirm: true }, false)).status, 401)
    assert.equal((await call('/settings', { workbuddyValidationEnabled: true })).status, 200)
    assert.equal((await call('/settings', { workbuddyValidationStopped: false })).status, 400)
    let calls = 0
    service.channels.complete = async request => { calls++; assert.equal(request.model, 'workbuddy/fixture'); assert.equal(request.openAi.messages[0].content, 'Reply with exactly: OK'); throw Object.assign(new Error('PRIVATE_GATEWAY_BODY'), { statusCode: 403 }) }
    assert.equal((await call('/workbuddy/validate', { model: 'workbuddy/fixture' })).status, 400)
    assert.equal(calls, 0)
    const response = await call('/workbuddy/validate', { model: 'workbuddy/fixture', confirm: true })
    assert.equal(response.status, 403); assert.doesNotMatch(await response.text(), /PRIVATE/)
    assert.equal(calls, 1)
    assert.equal((await call('/workbuddy/validate', { model: 'workbuddy/fixture', confirm: true })).status, 409)
    assert.equal((await call('/settings', { workbuddyValidationEnabled: true })).status, 200)
    assert.equal((await call('/workbuddy/validate', { model: 'workbuddy/fixture', confirm: true })).status, 409)
    assert.equal(calls, 1)
    await service.close(); service = await start()
    assert.equal((await (await call('/summary')).json()).workbuddyValidation.stopped, true)
    await call('/settings', { workbuddyValidationEnabled: false }); await call('/settings', { workbuddyValidationEnabled: true })
    service.channels.complete = async () => { calls++; return { text: 'PRIVATE_REPLY' } }
    const result = await call('/workbuddy/validate', { model: 'workbuddy/fixture', confirm: true })
    assert.equal(result.status, 200); assert.doesNotMatch(await result.text(), /PRIVATE/)
    assert.equal(calls, 2)
    assert.equal((await call('/workbuddy/validate', { model: 'buddy/fixture', confirm: true })).status, 400)
  } finally {
    await service?.close()
    if (!fs.realpathSync(dir).startsWith(fs.realpathSync(os.tmpdir()) + path.sep)) throw Error('unexpected fixture')
    fs.rmSync(dir, { recursive: true, force: true })
  }
})
