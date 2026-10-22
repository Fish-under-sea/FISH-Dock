import http from 'node:http'
import { createHash, randomBytes, randomUUID, timingSafeEqual } from 'node:crypto'
import { readSse } from '../../../src/http.js'
import { readStream, finishReason } from '../../../src/stream.js'
import { fromOpenAiMessages } from '../../../src/core/completion.js'
import { toChatMessages, toToolDefs } from '../../../src/messages.js'

const ORIGIN = 'https://meoo.com'
const AI = 'https://api.meoo.host/meoo-ai/compatible-mode/v1'
const CLIENT = 'meoo_app_tdWlRbpvAxN1EjmZ8XICxdTh'
const REF = 'MEOO_ACCOUNTS'
const failure = (statusCode, message) => Object.assign(new Error(message), { statusCode })
const digest = value => createHash('sha256').update(value).digest()
const same = (a, b) => timingSafeEqual(digest(a), digest(b))
const validId = value => typeof value === 'string' && /^[A-Za-z0-9_.:-]{1,200}$/.test(value)

/** OAuth 与项目密钥分开保存；仅公开账号元数据。 */
export function createMeooService({ credentials, fetcher = fetch, onChanged = () => {} }) {
  let accounts = []
  let closed = false
  const flows = new Map()
  const refreshes = new Map()
  const ready = (async () => {
    const stored = await credentials.resolve(REF)
    if (stored?.value) {
      const loaded = JSON.parse(stored.value)
      if (!Array.isArray(loaded)) throw failure(500, 'Meoo 账号数据格式错误')
      accounts = loaded.filter(a => validId(a.id) && typeof a.accessToken === 'string')
    }
  })()
  async function save() {
    if (closed) throw failure(503, 'Meoo 服务已停止')
    await credentials.set(REF, JSON.stringify(accounts))
    onChanged()
  }
  function account(id) {
    const found = accounts.find(a => a.id === id)
    if (!found) throw failure(404, 'Meoo 账号不存在')
    return found
  }
  function view() {
    return { accounts: accounts.map(a => ({ id: a.id, name: a.name, projectId: a.projectId, projectName: a.projectName, models: a.models ?? [], expiresAt: a.expiresAt })) }
  }
  async function token(parameters) {
    const response = await fetcher(`${ORIGIN}/oauth/token`, {
      method: 'POST', headers: { 'Content-Type': 'application/x-www-form-urlencoded', Accept: 'application/json' },
      body: new URLSearchParams({ ...parameters, client_id: CLIENT }), signal: AbortSignal.timeout(15000),
    })
    const data = await response.json()
    if (!response.ok) throw failure(response.status, `Meoo 登录令牌请求失败（HTTP ${response.status}），请重新绑定`)
    if (typeof data.access_token !== 'string' || !data.access_token || typeof data.refresh_token !== 'string' || !data.refresh_token || data.token_type !== 'Bearer' || !Number.isFinite(Number(data.expires_in)) || Number(data.expires_in) <= 0 || typeof data.openid !== 'string' || typeof data.tenant_id !== 'string') throw failure(502, 'Meoo 登录令牌响应格式错误')
    return { accessToken: data.access_token, refreshToken: data.refresh_token, expiresAt: Date.now() + Number(data.expires_in) * 1000, openid: data.openid, tenantId: data.tenant_id }
  }
  async function refresh(a) {
    if (refreshes.has(a.id)) return refreshes.get(a.id)
    const pending = (async () => {
      const next = await token({ grant_type: 'refresh_token', refresh_token: a.refreshToken })
      if (accounts.includes(a)) { Object.assign(a, next); await save() }
      else throw failure(404, 'Meoo 账号已删除')
    })().finally(() => refreshes.delete(a.id))
    refreshes.set(a.id, pending)
    return pending
  }
  async function api(a, path, retried = false) {
    if (a.expiresAt < Date.now() + 30000) await refresh(a)
    const response = await fetcher(`${ORIGIN}${path}`, { method: 'GET', headers: { Authorization: `Bearer ${a.accessToken}`, 'X-Meoo-Client': 'cli', Accept: 'application/json' }, signal: AbortSignal.timeout(30000) })
    if (response.status === 401 && !retried) { await refresh(a); return api(a, path, true) }
    if (!response.ok) throw failure(response.status, `Meoo 项目请求失败（HTTP ${response.status}）`)
    const data = await response.json()
    if (data.success === false || !data.data) throw failure(502, 'Meoo 未返回可用的项目权限')
    return data.data
  }
  async function projects(a) {
    const data = await api(a, '/open/v1/cli-compat/projects?page_size=100&scope=mine')
    return (Array.isArray(data.list) ? data.list : []).filter(p => validId(p.url_id)).map(p => ({ id: p.url_id, name: typeof p.name === 'string' ? p.name : p.url_id }))
  }
  async function projectKey(a, projectId) {
    const data = await api(a, `/open/v1/cli-compat/projects/${encodeURIComponent(projectId)}/cloud/environment`)
    if (data.enabled !== true || typeof data.meooProjectApiKey !== 'string' || !data.meooProjectApiKey) throw failure(409, '所选项目尚未提供模型服务权限，请在 Meoo 官网检查该现有项目')
    return data.meooProjectApiKey
  }
  async function stopFlow(flow) {
    clearTimeout(flow.timer)
    await flow.starting?.catch(() => {})
    if (!flow.server.listening) return
    flow.server.closeAllConnections()
    await new Promise(resolve => flow.server.close(resolve))
  }
  async function startLogin() {
    if ([...flows.values()].some(f => f.status === 'pending' || f.status === 'exchanging')) throw failure(409, '已有 Meoo 登录等待完成，请先取消或返回授权页面')
    for (const [id, f] of flows) if (f.status !== 'pending' && f.status !== 'exchanging') flows.delete(id)
    const state = randomBytes(32).toString('base64url')
    const verifier = randomBytes(32).toString('base64url')
    const flow = { id: randomUUID(), status: 'pending' }
    const server = http.createServer(async (req, res) => {
      const url = new URL(req.url, 'http://127.0.0.1')
      res.setHeader('content-type', 'text/plain; charset=utf-8')
      res.setHeader('cache-control', 'no-store')
      res.setHeader('referrer-policy', 'no-referrer')
      res.setHeader('content-security-policy', "default-src 'none'; frame-ancestors 'none'")
      if (req.method !== 'GET' || url.pathname !== '/oauth/callback') { res.writeHead(404); res.end(); return }
      if (!same(url.searchParams.get('state') ?? '', state)) { res.writeHead(400); res.end('授权请求校验失败'); return }
      if (flow.status !== 'pending') { res.writeHead(409); res.end('该授权请求已处理'); return }
      if (url.searchParams.has('error')) { flow.status = 'cancelled'; clearTimeout(flow.timer); res.writeHead(400); res.end('授权已取消'); server.close(); return }
      const code = url.searchParams.get('code')
      if (!code || code.length > 4096) { res.writeHead(400); res.end('缺少有效授权码'); return }
      flow.status = 'exchanging'
      try {
        const value = await token({ grant_type: 'authorization_code', code, redirect_uri: flow.redirectUri, code_verifier: verifier })
        if (closed || flow.status !== 'exchanging') throw failure(409, '登录请求已取消')
        let a = accounts.find(item => item.openid === value.openid && item.tenantId === value.tenantId)
        if (a) Object.assign(a, value)
        else { a = { id: randomUUID(), name: `Meoo ${accounts.length + 1}`, ...value }; accounts.push(a) }
        await save()
        flow.accountId = a.id
        flow.status = 'success'
        res.writeHead(200); res.end('Meoo 账号已绑定，请返回 Fish Dock 选择已有项目。')
      } catch (error) { flow.status = 'failed'; flow.error = 'Meoo 授权未完成，请重试登录'; res.writeHead(error.statusCode ?? 502); res.end(flow.error) }
      finally { clearTimeout(flow.timer); server.close() }
    })
    flow.server = server
    flow.starting = new Promise((resolve, reject) => { server.once('error', reject); server.listen(0, '127.0.0.1', resolve) })
    flows.set(flow.id, flow)
    try {
      await flow.starting
      if (closed) throw failure(503, 'Meoo 服务已停止')
      if (flow.status !== 'pending') throw failure(409, '登录请求已取消')
      flow.redirectUri = `http://127.0.0.1:${server.address().port}/oauth/callback`
      const url = new URL('/oauth/authorize', ORIGIN)
      for (const [key, value] of Object.entries({ client_id: CLIENT, redirect_uri: flow.redirectUri, response_type: 'code', scope: 'cli.compat', state, code_challenge: digest(verifier).toString('base64url'), code_challenge_method: 'S256' })) url.searchParams.set(key, value)
      flow.timer = setTimeout(() => { flow.status = 'expired'; void stopFlow(flow) }, 5 * 60000)
      flow.timer.unref?.()
      return { flowId: flow.id, loginUrl: url.toString() }
    } catch (error) {
      flows.delete(flow.id)
      await stopFlow(flow)
      throw error
    }
  }
  const publicModelRows = () => [...new Set(accounts.filter(a => a.projectKey).flatMap(a => a.models ?? []))].map(id => ({ id: `meoo/${id}`, object: 'model', owned_by: 'meoo', channel: 'meoo' }))
  async function rpc(method, payload = {}) {
    await ready
    if (closed) throw failure(503, 'Meoo 服务已停止')
    switch (method) {
      case 'meoo.status': return view()
      case 'meoo.balances': {
        const rows = []
        for (const a of [...accounts]) {
          try {
            const data = await api(a, '/api/mc/entitlement/get')
            const total = data.userPointInfo?.account?.availableBalance
            if (typeof total !== 'number' || !Number.isFinite(total) || total < 0) throw failure(502, 'Meoo 余额响应未返回有效积分')
            rows.push({ accountId: a.id, nickname: a.name, balance: { total } })
          } catch (error) { rows.push({ accountId: a.id, nickname: a.name, balance: null, error: error.message }) }
        }
        return { accounts: rows }
      }
      case 'meoo.daily.status': {
        const day = new Intl.DateTimeFormat('en-CA', { timeZone: 'Asia/Shanghai' }).format(new Date())
        return { accounts: accounts.map(a => ({ accountId: a.id, nickname: a.name, day, grantedToday: a.lastGrant?.day === day ? a.lastGrant.credit : null })) }
      }
      case 'meoo.claimAll': {
        const results = []
        for (const a of [...accounts]) {
          try {
            const data = await api(a, '/api/mc/entitlement/init')
            if (data.initialized !== true || typeof data.dailyLoginGranted !== 'boolean') throw failure(502, 'Meoo 未确认每日积分领取结果')
            const credit = data.dailyLoginGrantAmount
            if (data.dailyLoginGranted && !(typeof credit === 'number' && Number.isFinite(credit) && credit > 0)) throw failure(502, 'Meoo 未返回实际领取金额')
            if (!accounts.includes(a)) throw failure(404, 'Meoo 账号已删除')
            if (data.dailyLoginGranted) {
              a.lastGrant = { day: new Intl.DateTimeFormat('en-CA', { timeZone: 'Asia/Shanghai' }).format(new Date()), credit, at: Date.now() }
              await save()
            }
            results.push({ accountId: a.id, nickname: a.name, outcome: data.dailyLoginGranted ? { kind: 'claimed', credit } : { kind: 'already-claimed', credit: 0 } })
          } catch (error) { results.push({ accountId: a.id, nickname: a.name, outcome: { kind: 'failed', message: error.message, code: error.statusCode ?? -1 } }) }
        }
        return { results, summary: { claimed: results.filter(r => r.outcome.kind === 'claimed').length, totalCredit: results.reduce((sum, r) => sum + (r.outcome.kind === 'claimed' ? r.outcome.credit : 0), 0), failed: results.filter(r => r.outcome.kind === 'failed').length } }
      }
      case 'meoo.refresh': {
        for (const a of [...accounts]) {
          if (payload.accountId && payload.accountId !== a.id) continue
          await refresh(a)
          if (a.projectId) await rpc('meoo.bind', { accountId: a.id, projectId: a.projectId })
        }
        return view()
      }
      case 'meoo.login.start': return startLogin()
      case 'meoo.login.poll': {
        const f = flows.get(payload.flowId)
        if (!f) throw failure(404, 'Meoo 登录会话已结束')
        return { status: f.status, accountId: f.accountId, error: f.error }
      }
      case 'meoo.login.cancel': {
        const f = flows.get(payload.flowId)
        if (f && ['pending', 'exchanging'].includes(f.status)) { f.status = 'cancelled'; await stopFlow(f) }
        return { cancelled: true }
      }
      case 'meoo.projects': return { projects: await projects(account(payload.accountId)) }
      case 'meoo.bind': {
        const a = account(payload.accountId)
        const project = (await projects(a)).find(p => p.id === payload.projectId)
        if (!project) throw failure(400, '请选择该账号已有的项目')
        const key = await projectKey(a, project.id)
        const response = await fetcher(`${AI}/models`, { headers: { Authorization: `Bearer ${key}` }, signal: AbortSignal.timeout(30000) })
        if (!response.ok) throw failure(response.status, `Meoo 模型目录尚未取得（HTTP ${response.status}）`)
        const data = await response.json()
        const models = (Array.isArray(data.data) ? data.data : []).map(m => m.id).filter(id => typeof id === 'string' && /^[A-Za-z0-9_./:-]{1,200}$/.test(id))
        if (!models.length) throw failure(409, '所选项目未返回可用模型')
        if (!accounts.includes(a)) throw failure(404, 'Meoo 账号已删除')
        Object.assign(a, { projectId: project.id, projectName: project.name, projectKey: key, models })
        await save()
        return view()
      }
      case 'meoo.remove': account(payload.accountId); accounts = accounts.filter(a => a.id !== payload.accountId); await save(); return view()
      default: throw failure(400, '未知 Meoo 操作')
    }
  }
  async function complete(request, onChunk = () => {}) {
    await ready
    if (closed) throw failure(503, 'Meoo 服务已停止')
    request.signal?.throwIfAborted()
    const model = request.model.slice('meoo/'.length)
    const a = accounts.find(item => item.projectKey && item.models?.includes(model))
    if (!a) throw failure(404, 'Meoo 模型尚未绑定或已移除')
    const responseBody = request.responsesBody ?? {}
    const body = request.responses ? { ...responseBody, messages: toChatMessages(fromOpenAiMessages(responseBody, true, request.model)), tools: responseBody.tools?.filter(t => t.type === 'function').map(t => ({ type: 'function', function: { name: t.name, description: t.description, parameters: t.parameters } })), max_tokens: responseBody.max_output_tokens } : request.openAi ?? { messages: toChatMessages(request.messages ?? []), tools: request.tools?.length ? toToolDefs(request.tools) : undefined, max_tokens: request.maxTokens, temperature: request.temperature }
    const wire = { ...body, model, stream: true, stream_options: { include_usage: true } }
    delete wire.input; delete wire.instructions; delete wire.max_output_tokens
    const lifetime = new AbortController()
    const signal = AbortSignal.any([lifetime.signal, request.signal ?? new AbortController().signal, AbortSignal.timeout(300000)])
    let response
    try {
      for (let attempt = 0; attempt < 2; attempt++) {
        response = await fetcher(`${AI}/chat/completions`, { method: 'POST', headers: { Authorization: `Bearer ${a.projectKey}`, 'Content-Type': 'application/json', Accept: 'text/event-stream' }, body: JSON.stringify(wire), signal })
        if (response.status !== 401 || attempt === 1) break
        await response.body?.cancel()
        const key = await projectKey(a, a.projectId)
        if (!accounts.includes(a)) throw failure(404, 'Meoo 账号已删除')
        a.projectKey = key; await save()
      }
      if (!response.ok) { await response.body?.cancel(); throw failure(response.status, `Meoo 模型请求失败（HTTP ${response.status}）`) }
      const payloads = new ReadableStream({ start(controller) { void readSse(response.body, data => controller.enqueue(data), signal).then(() => controller.close(), error => controller.error(error)) }, cancel() { lifetime.abort() } })
      const stream = readStream(payloads, 'chat', new Map())
      let result
      while (true) {
        const next = await stream.next()
        if (next.done) { result = next.value; break }
        onChunk(next.value)
      }
      if (!result.sawFinish) throw failure(502, 'Meoo 响应在结束标记前中断，未自动重发')
      if (result.sawUsage) onChunk({ type: 'usage', usage: result.usage })
      onChunk({ type: 'finish', reason: finishReason(result.finish) })
    } finally { lifetime.abort() }
  }
  async function close() { closed = true; await Promise.all([...flows.values()].map(f => { if (f.status === 'pending' || f.status === 'exchanging') f.status = 'cancelled'; return stopFlow(f) })); flows.clear() }
  return { ready, rpc, publicModelRows, complete, close }
}
