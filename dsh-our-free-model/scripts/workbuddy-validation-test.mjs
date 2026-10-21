import assert from 'node:assert/strict'
import { test } from 'node:test'
import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import { createWorkBuddyValidation } from '../packages/standalone/channels/validation.mjs'
import { createRuntimeStores } from '../src/core/runtime.js'

const request = { model: 'workbuddy/deepseek-v4.1-flash', openAi: { messages: [{ role: 'system', content: 'User identity: DeepSeek Harness.' }, { role: 'user', content: 'PRIVATE_PROMPT' }] } }
const fixture = () => {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'ofm-validation-'))
  const stores = createRuntimeStores({ dataDir: dir, logger: { warn() {} } })
  return { ...stores, dir, close() { stores.dispose(); if (!fs.realpathSync(dir).startsWith(fs.realpathSync(os.tmpdir()) + path.sep)) throw Error('unexpected fixture'); fs.rmSync(dir, { recursive: true, force: true }) } }
}

test('开启后的成功调用保持请求原样，诊断只记录本地关联号和状态', async () => {
  const f = fixture()
  try {
    f.settings.update({ workbuddyValidationEnabled: true })
    let calls = 0
    const guard = createWorkBuddyValidation({ settings: f.settings, complete: async value => { calls++; assert.strictEqual(value, request); return { text: 'PRIVATE_REPLY' } } })
    await guard.complete(request)
    assert.equal(calls, 1)
    const state = guard.state()
    assert.equal(state.enabled, true); assert.equal(state.stopped, false)
    assert.equal(state.last.status, 200); assert.match(state.last.requestId, /^[\da-f-]{36}$/)
    assert.ok(state.last.durationMs >= 0)
    assert.doesNotMatch(JSON.stringify(state), /PRIVATE|User identity|Harness/)
  } finally { f.close() }
})

test('遇到拒绝立即锁停，重复调用和重启均不再发送，错误不泄漏响应正文', async () => {
  const f = fixture()
  try {
    f.settings.update({ workbuddyValidationEnabled: true })
    let calls = 0
    const fail = async () => { calls++; throw Object.assign(new Error('PRIVATE_PROMPT PRIVATE_REPLY'), { statusCode: 403 }) }
    const guard = createWorkBuddyValidation({ settings: f.settings, complete: fail })
    await assert.rejects(guard.complete(request), error => error.statusCode === 403 && !/PRIVATE/.test(error.message))
    await assert.rejects(guard.complete(request), /已停止/)
    assert.equal(calls, 1)
    assert.equal(guard.state().stopped, true)
    const restarted = createWorkBuddyValidation({ settings: f.settings, complete: fail })
    await assert.rejects(restarted.complete(request), /已停止/)
    assert.equal(calls, 1)
    assert.equal(JSON.parse(fs.readFileSync(path.join(f.dir, 'settings.json'), 'utf8')).workbuddyValidationStopped, true)
  } finally { f.close() }
})

test('验证中的并行请求不发出，开关关闭和其他渠道保持原有流程', async () => {
  const f = fixture()
  try {
    f.settings.update({ workbuddyValidationEnabled: true })
    let finish, calls = 0
    const guard = createWorkBuddyValidation({ settings: f.settings, complete: async () => { calls++; return new Promise(resolve => { finish = resolve; setTimeout(() => resolve({ text: 'OK' }), 200) }) } })
    const first = guard.complete(request)
    await assert.rejects(guard.complete(request), error => error.statusCode === 429)
    assert.equal(calls, 1)
    finish({ text: 'OK' }); await first
    let raw = 0
    const ordinary = createWorkBuddyValidation({ settings: f.settings, complete: async () => { raw++; return { text: 'OK' } } })
    await ordinary.complete({ model: 'buddy/fixture' })
    f.settings.update({ workbuddyValidationEnabled: false, workbuddyValidationStopped: true })
    await ordinary.complete(request)
    assert.equal(raw, 2)
  } finally { f.close() }
})
