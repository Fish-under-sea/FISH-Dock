import test from 'node:test'
import assert from 'node:assert/strict'
import { createMeooService } from '../packages/standalone/channels/meoo.mjs'

const token = () => ({ access_token: 'secret-access', refresh_token: 'secret-refresh', token_type: 'Bearer', expires_in: 3600, scope: 'cli.compat', openid: 'user-1', tenant_id: 'tenant-1' })
function fixture() {
  const values = new Map()
  const requests = []
  let expired = false
  const credentials = { async resolve(key) { return values.has(key) ? { value: values.get(key) } : undefined }, async set(key, value) { values.set(key, value) }, async unset(key) { values.delete(key) } }
  const fetcher = async (url, init = {}) => {
    requests.push({ url: String(url), init })
    const u = new URL(url)
    const json = body => Response.json(body)
    if (u.pathname === '/oauth/token') return json(token())
    if (u.pathname.endsWith('/cloud/environment')) return json({ success: true, data: { enabled: true, meooProjectApiKey: 'secret-project' } })
    if (u.pathname.endsWith('/projects')) return json({ success: true, data: { list: [{ url_id: 'project-1', name: 'existing' }] } })
    if (u.pathname.endsWith('/models')) return json({ data: [{ id: 'verified-model' }] })
    if (u.pathname.endsWith('/chat/completions')) {
      if (expired) return new Response('{}', { status: 401 })
      return new Response('data: {"choices":[{"delta":{"content":"OK"}}]}\r\n\r\ndata: {"choices":[{"delta":{},"finish_reason":"stop"}],"usage":{"prompt_tokens":3,"completion_tokens":1}}\r\n\r\ndata: [DONE]\r\n\r\n', { headers: { 'content-type': 'text/event-stream' } })
    }
    throw new Error(`unexpected endpoint ${u.pathname}`)
  }
  const service = createMeooService({ credentials, fetcher })
  return { service, requests, values, expire() { expired = true } }
}
async function login(f) {
  const flow = await f.service.rpc('meoo.login.start', {})
  const url = new URL(flow.loginUrl)
  const callback = new URL(url.searchParams.get('redirect_uri'))
  callback.searchParams.set('state', url.searchParams.get('state'))
  callback.searchParams.set('code', 'fixture-code')
  const response = await fetch(callback)
  assert.equal(response.status, 200)
  const status = await f.service.rpc('meoo.login.poll', { flowId: flow.flowId })
  assert.equal(status.status, 'success')
  return status.accountId
}

test('Meoo 使用官方 PKCE 登录、校验 state，前端不接触密钥', async t => {
  const f = fixture(); t.after(() => f.service.close())
  const flow = await f.service.rpc('meoo.login.start', {})
  const url = new URL(flow.loginUrl)
  assert.equal(url.origin, 'https://meoo.com')
  assert.equal(url.searchParams.get('code_challenge_method'), 'S256')
  assert.equal(url.searchParams.get('scope'), 'cli.compat')
  const callback = new URL(url.searchParams.get('redirect_uri'))
  callback.searchParams.set('state', 'wrong'); callback.searchParams.set('code', 'bad')
  assert.equal((await fetch(callback)).status, 400)
  assert.equal(f.requests.length, 0)
  callback.searchParams.set('state', url.searchParams.get('state'))
  assert.equal((await fetch(callback)).status, 200)
  const state = await f.service.rpc('meoo.status', {})
  assert.equal(state.accounts.length, 1)
  assert.doesNotMatch(JSON.stringify(state), /secret-|refresh_token|access_token/)
  assert.equal(f.requests[0].init.body.get('grant_type'), 'authorization_code')
  assert.equal(f.service.publicModelRows().length, 0)
})

test('Meoo 并发启动登录只保留一个回调监听器', async t => {
  const f = fixture(); t.after(() => f.service.close())
  const results = await Promise.allSettled([
    f.service.rpc('meoo.login.start', {}),
    f.service.rpc('meoo.login.start', {}),
  ])
  assert.equal(results.filter(r => r.status === 'fulfilled').length, 1)
  const rejected = results.find(r => r.status === 'rejected')
  assert.equal(rejected?.reason.statusCode, 409)
})

test('Meoo 回调监听器启动期间关闭服务不遗留授权会话', async t => {
  const f = fixture(); t.after(() => f.service.close())
  await f.service.ready
  const started = f.service.rpc('meoo.login.start', {}).then(
    value => ({ status: 'fulfilled', value }),
    reason => ({ status: 'rejected', reason }),
  )
  await Promise.resolve()
  await f.service.close()
  const result = await started
  assert.equal(result.status, 'rejected')
  assert.equal(result.reason.statusCode, 503)
  assert.equal(f.requests.length, 0)
})

test('Meoo 仅选现有项目，以项目密钥发现模型，不自动创建或开通云服务', async t => {
  const f = fixture(); t.after(() => f.service.close())
  const accountId = await login(f)
  const { projects } = await f.service.rpc('meoo.projects', { accountId })
  assert.deepEqual(projects, [{ id: 'project-1', name: 'existing' }])
  await f.service.rpc('meoo.bind', { accountId, projectId: 'project-1' })
  assert.deepEqual(f.service.publicModelRows().map(m => m.id), ['meoo/verified-model'])
  const discovery = f.requests.find(r => r.url.endsWith('/models'))
  assert.equal(discovery.init.headers.Authorization, 'Bearer secret-project')
  assert.equal(f.requests.filter(r => r.init.method === 'POST').length, 1)
  await assert.rejects(f.service.rpc('meoo.bind', { accountId, projectId: 'unknown' }), /项目/)
})

test('Meoo 复用 SSE 解析，保持工具声明、usage 和取消信号', async t => {
  const f = fixture(); t.after(() => f.service.close())
  const accountId = await login(f)
  await f.service.rpc('meoo.bind', { accountId, projectId: 'project-1' })
  const chunks = []
  await f.service.complete({ model: 'meoo/verified-model', openAi: { messages: [{ role: 'user', content: 'hello' }], tools: [{ type: 'function', function: { name: 'lookup', parameters: { type: 'object' } } }] } }, c => chunks.push(c))
  assert.ok(chunks.some(c => c.type === 'text-delta' && c.text === 'OK'))
  assert.ok(chunks.some(c => c.type === 'usage' && c.usage.outputTokens === 1))
  const call = f.requests.find(r => r.url.endsWith('/chat/completions'))
  assert.equal(call.init.headers.Authorization, 'Bearer secret-project')
  assert.equal(JSON.parse(call.init.body).tools[0].function.name, 'lookup')
  const abort = new AbortController(); abort.abort()
  const before = f.requests.length
  await assert.rejects(f.service.complete({ model: 'meoo/verified-model', signal: abort.signal, openAi: {} }))
  assert.equal(f.requests.length, before)
})

test('Meoo 401 刷新项目权限后最多重试一次，不循环重发', async t => {
  const f = fixture(); t.after(() => f.service.close())
  const accountId = await login(f)
  await f.service.rpc('meoo.bind', { accountId, projectId: 'project-1' })
  f.expire()
  await assert.rejects(f.service.complete({ model: 'meoo/verified-model', openAi: { messages: [{ role: 'user', content: 'hi' }] } }), { statusCode: 401 })
  assert.equal(f.requests.filter(r => r.url.endsWith('/chat/completions')).length, 2)
})

test('Meoo 服务关闭后推理入口不发送上游请求', async t => {
  const f = fixture(); t.after(() => f.service.close())
  const accountId = await login(f)
  await f.service.rpc('meoo.bind', { accountId, projectId: 'project-1' })
  await f.service.close()
  const before = f.requests.length
  await assert.rejects(f.service.complete({ model: 'meoo/verified-model', openAi: { messages: [{ role: 'user', content: 'hi' }] } }), { statusCode: 503 })
  assert.equal(f.requests.length, before)
})

test('Meoo 重启保留账号，删除后模型立即消失且无凭据导出', async t => {
  const f = fixture(); t.after(() => f.service.close())
  const accountId = await login(f)
  await f.service.rpc('meoo.bind', { accountId, projectId: 'project-1' })
  const second = createMeooService({ credentials: { async resolve(key) { return { value: f.values.get(key) } }, async set(key, value) { f.values.set(key, value) } } })
  t.after(() => second.close())
  assert.equal((await second.rpc('meoo.status', {})).accounts.length, 1)
  await f.service.rpc('meoo.remove', { accountId })
  assert.deepEqual(f.service.publicModelRows(), [])
  assert.doesNotMatch(JSON.stringify(await f.service.rpc('meoo.status', {})), /secret-/)
})
