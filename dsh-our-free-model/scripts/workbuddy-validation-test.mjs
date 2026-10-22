import assert from 'node:assert/strict'
import { test } from 'node:test'

const moduleArgument = process.argv.find(value => value.startsWith('--module='))
const { createWorkBuddyValidation } = await import(moduleArgument?.slice(9) ?? '../packages/standalone/channels/validation.mjs')
const request = () => ({ model: 'workbuddy/example', openAi: { messages: [{ role: 'user', content: 'OK' }] } })
function store(initial = {}) {
  let value = { workbuddyValidationEnabled: true, ...initial }
  return { get: () => value, update: patch => { value = { ...value, ...patch } }, flush() {}, writeFailed: false }
}

for (const status of [408, 429, 500, 502, 503, 504]) {
  test(`HTTP ${status} 不永久锁停，后续请求可恢复`, async () => {
    const settings = store()
    let calls = 0
    const validation = createWorkBuddyValidation({ settings, complete: async () => {
      if (++calls === 1) throw Object.assign(new Error('temporary'), { statusCode: status, code: 'UPSTREAM' })
      return { text: 'OK' }
    } })
    await assert.rejects(validation.complete(request()), error => error.statusCode === status)
    assert.equal(validation.state().stopped, false)
    assert.equal(settings.get().workbuddyValidationStopped, false)
    assert.deepEqual(await validation.complete(request()), { text: 'OK' })
    assert.equal(calls, 2)
  })
}

test('旧版 429 锁停在启动时迁移，保留历史诊断', async () => {
  const last = { status: 429, requestId: 'old', at: '2026-10-22T00:00:00.000Z' }
  const settings = store({ workbuddyValidationStopped: true, workbuddyValidationLast: last })
  const validation = createWorkBuddyValidation({ settings, complete: async () => ({ text: 'OK' }) })
  assert.equal(validation.state().stopped, false)
  assert.deepEqual(validation.state().last, last)
  assert.equal((await validation.complete(request())).text, 'OK')
})

for (const status of [401, 403]) {
  test(`HTTP ${status} 保留鉴权锁停，显式关闭后重新开启可恢复`, async () => {
    const settings = store()
    let calls = 0
    const validation = createWorkBuddyValidation({ settings, complete: async () => {
      if (++calls === 1) throw Object.assign(new Error('authentication'), { statusCode: status })
      return { text: 'OK' }
    } })
    await assert.rejects(validation.complete(request()))
    assert.equal(validation.state().stopped, true)
    await assert.rejects(validation.complete(request()), error => error.statusCode === 409)
    assert.equal(calls, 1)
    settings.update({ workbuddyValidationEnabled: false }); validation.settingsChanged()
    settings.update({ workbuddyValidationEnabled: true, workbuddyValidationStopped: false }); validation.settingsChanged()
    assert.equal((await validation.complete(request())).text, 'OK')
  })
}

test('取消保留原始异常，不写入账号锁停', async () => {
  const cancelled = new DOMException('cancelled', 'AbortError')
  const settings = store()
  const validation = createWorkBuddyValidation({ settings, complete: async () => { throw cancelled } })
  await assert.rejects(validation.complete(request()), error => error === cancelled)
  assert.equal(validation.state().stopped, false)
})

test('诊断落盘失败不把已成功的推理改成失败', async () => {
  const settings = store()
  settings.writeFailed = true
  const validation = createWorkBuddyValidation({ settings, complete: async () => ({ text: 'OK' }) })
  assert.equal((await validation.complete(request())).text, 'OK')
  assert.equal(validation.state().stopped, false)
})

test('非 WorkBuddy 与关闭验证时原样传递请求和回调', async () => {
  const settings = store({ workbuddyValidationStopped: true })
  const input = { ...request(), model: 'other/example' }
  const callback = () => {}
  const validation = createWorkBuddyValidation({ settings, complete: async (value, onChunk) => {
    assert.equal(value, input); assert.equal(onChunk, callback)
    return { text: 'OK' }
  } })
  assert.equal((await validation.complete(input, callback)).text, 'OK')
})

test('流内错误也按真实 HTTP 状态记录，不误认为成功', async () => {
  const settings = store()
  const validation = createWorkBuddyValidation({ settings, complete: async () => ({ error: { statusCode: 429, code: 'RATE_LIMIT' } }) })
  await assert.rejects(validation.complete(request()), error => error.statusCode === 429)
  assert.equal(validation.state().stopped, false)
  assert.equal(validation.state().last.status, 429)
})

test('重复验证不重复调用上游，忙状态在完成后释放', async () => {
  const settings = store()
  let finish
  let calls = 0
  const validation = createWorkBuddyValidation({ settings, complete: () => { calls++; return new Promise(resolve => { finish = resolve }) } })
  const first = validation.complete(request())
  await assert.rejects(validation.complete(request()), error => error.statusCode === 429)
  assert.equal(calls, 1)
  assert.equal(validation.state().stopped, false)
  finish({ text: 'OK' }); await first
  assert.equal(validation.state().busy, false)
})
