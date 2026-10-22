import { parentPort, workerData } from 'node:worker_threads'
import { createCredentials } from './credentials.mjs'
import { createMeooService } from './meoo.mjs'
import { readRaccoonDailyStatus, claimRaccoonDaily } from './raccoon-daily.mjs'
import { attachments } from './images.mjs'
import { toChatMessages, toToolDefs } from '../../../src/messages.js'
import {
  apply, toGenerateOptions, toResponsesGenerateOptions, normalizeReasoningEffort,
  responsesReasoningEffort, normalizeMaxTokens, responsesMaxOutputTokens, markGatewayChannel,
  collectGatewayModels, collectGatewayEffortViews, toOpenAiModels, raccoonHeaders,
} from './business.mjs'

const cleanups = []
const adapters = new Map()
const routes = new Map()
const controllers = new Map()
let closed = false
const send = message => parentPort.postMessage(message)
const logger = Object.fromEntries(['info', 'warn', 'error'].map(level => [level, message => send({ type: 'log', level, message: String(message) })]))
const credentials = createCredentials(workerData.dataDir)
const meoo = createMeooService({ credentials, onChanged: () => send({ type: 'changed' }) })
const ctx = {
  platform: { channelHome: workerData.dataDir }, credentials, logger, attachments,
  connection: { fetch: { register(route) { routes.set(route.path, route) } } },
  get(name) { return this[name] },
  provide(name, value) {
    if (Object.hasOwn(this, name)) throw new Error(`服务重复注册：${name}`)
    this[name] = value
  },
  effect(body) {
    const cleanup = body()
    if (typeof cleanup === 'function') {
      if (closed) void cleanup()
      else cleanups.push(cleanup)
    }
    return cleanup
  },
  inject(names, body) { if (names.every(name => this[name] !== undefined)) return body(this) },
  emit() { send({ type: 'changed' }) },
  llm: {
    registerAdapter(providers, adapter) {
      for (const provider of providers) {
        if (adapters.has(provider)) throw new Error(`渠道重复注册：${provider}`)
        adapters.set(provider, adapter)
      }
    },
    listProviders() { return [...adapters.keys()].map(id => ({ id })) },
    async listModels(provider) { return await adapters.get(provider)?.listModels(provider) ?? [] },
    async resolveModel(provider, model, signal) {
      const adapter = adapters.get(provider)
      if (!adapter) throw Object.assign(new Error('渠道不存在'), { statusCode: 404 })
      return adapter.resolveModel(provider, model, signal)
    },
    async resolveModelInfo(provider, model, signal) { return this.resolveModel(provider, model, signal) },
    async *stream(options) {
      const adapter = adapters.get(options.provider)
      if (!adapter) throw Object.assign(new Error('渠道不存在'), { statusCode: 404 })
      const call = await adapter.prepareCall(options.provider, options.model, options.signal)
      yield* call.stream(options)
    },
  },
}

async function models() {
  const onError = (provider, error) => logger.warn(`${provider} 模型目录读取失败：${error.message}`)
  const groups = (await Promise.all([...adapters.keys()].map(provider => collectGatewayModels({
    listProviders: () => [{ id: provider }],
    listModels: id => ctx.llm.listModels(id),
  }, onError)))).flat()
  const infos = new Map()
  const views = await collectGatewayEffortViews({
    async resolveModelInfo(provider, model) {
      const info = await ctx.llm.resolveModel(provider, model)
      infos.set(`${provider}/${model}`, info)
      return info
    },
  }, groups, onError)
  return [...toOpenAiModels(groups, views).map(row => ({
    ...row, channel: row.owned_by, vision: row.input?.includes('image') === true,
    context_window: row.context_window ?? infos.get(row.id)?.context?.contextWindow,
    max_tokens: row.max_tokens ?? infos.get(row.id)?.defaultMaxTokens,
  })), ...meoo.publicModelRows()]
}

const creditJobs = new Map()
const todayClaims = new Map()
const chinaDay = () => new Intl.DateTimeFormat('en-CA', { timeZone: 'Asia/Shanghai' }).format(new Date())
async function raccoonStatus(entry) {
  const stored = await credentials.resolve(entry.credentialRef)
  const credential = stored && JSON.parse(stored.value)
  if (typeof credential?.access_token !== 'string' || !credential.access_token) throw Object.assign(new Error('商汤账号凭据未就绪，请重新绑定'), { statusCode: 401 })
  const product = ctx.raccoonAuth.product
  return readRaccoonDailyStatus({ base: product.apiBase + product.pointsApiPrefix, headers: raccoonHeaders(credential) })
}
async function claimRaccoonAccounts() {
  const results = []
  for (const entry of ctx.accountPool.listAccountsByProvider('raccoon').filter(a => a.enabled)) {
    let outcome
    try {
      outcome = await claimRaccoonDaily({ readStatus: () => raccoonStatus(entry), refreshSession: () => ctx.raccoonAuth.refreshAccountCredential(entry.credentialRef, ctx.accountPool, entry.id) })
    } catch (error) { outcome = { kind: 'failed', code: error.statusCode ?? -1, message: [401, 403].includes(error.statusCode) ? '账号认证已失效，请重新绑定' : '每日积分领取暂未完成，请稍后重试' } }
    results.push({ accountId: entry.id, nickname: entry.nickname, outcome })
  }
  send({ type: 'changed' })
  return { results, summary: { claimed: results.filter(r => r.outcome.kind === 'claimed').length, totalCredit: results.reduce((sum, r) => sum + (r.outcome.kind === 'claimed' ? r.outcome.credit : 0), 0), failed: results.filter(r => r.outcome.kind === 'failed').length } }
}
async function rpc(call, signal) {
  if (call.method === 'credits.claimAll' && call.payload?.provider === 'raccoon') {
    if (!creditJobs.has('raccoon')) creditJobs.set('raccoon', claimRaccoonAccounts().finally(() => creditJobs.delete('raccoon')))
    return { ok: true, value: await creditJobs.get('raccoon') }
  }
  if (call.method === 'credits.daily.audit') {
    const providers = [...adapters.keys(), 'meoo'].filter(id => id === 'meoo' || ctx.accountPool.listAccountsByProvider(id).length > 0)
    const rows = []
    for (const provider of providers) {
      const [balance, daily] = await Promise.allSettled([
        rpc({ method: provider === 'meoo' ? 'meoo.balances' : 'credits.balances', payload: { provider } }, signal),
        provider === 'raccoon' ? rpc({ method: 'raccoon.daily.status' }, signal) : provider === 'meoo' ? rpc({ method: 'meoo.daily.status' }, signal) : ['buddy', 'minimax'].includes(provider) ? rpc({ method: 'credits.status', payload: { provider } }, signal) : Promise.resolve(null),
      ])
      const balances = balance.status === 'fulfilled' && balance.value?.ok ? balance.value.value.accounts : []
      const statuses = daily.status === 'fulfilled' && daily.value?.ok ? daily.value.value.accounts : []
      const entries = provider === 'meoo' ? (await meoo.rpc('meoo.status')).accounts.map(a => ({ id: a.id, nickname: a.name })) : ctx.accountPool.listAccountsByProvider(provider)
      for (const entry of entries) {
        const b = balances.find(a => a.accountId === entry.id), d = statuses.find(a => a.accountId === entry.id)
        const receipt = todayClaims.get(`${provider}/${entry.id}`)
        const granted = provider === 'raccoon' || provider === 'meoo' ? d?.grantedToday : d?.status?.todayCheckedIn === true ? d.status.todayCredit : receipt?.day === chinaDay() ? receipt.credit : null
        rows.push({ provider, accountId: entry.id, nickname: entry.nickname, balance: Number.isFinite(b?.balance?.total) ? b.balance.total : null, grantedToday: Number.isFinite(granted) && granted >= 0 ? granted : null, unit: provider === 'cline' ? '美元' : provider === 'gemini' ? '配额' : provider === 'zcode' ? 'Token' : '积分', error: b?.error ?? (!b ? '余额查询未完成' : undefined), dailyError: d?.error })
      }
    }
    return { ok: true, value: { day: chinaDay(), accounts: rows } }
  }
  if (call.method === 'raccoon.daily.status') {
    const accounts = []
    for (const entry of ctx.accountPool.listAccountsByProvider('raccoon').filter(a => a.enabled)) {
      const row = { accountId: entry.id, nickname: entry.nickname }
      try {
        const result = await raccoonStatus(entry)
        accounts.push({ ...row, ...result })
      } catch (error) {
        accounts.push({ ...row, state: 'unconfirmed', dailyBalance: null, grantedToday: null, claimSupported: false,
          error: [401, 403].includes(error.statusCode) ? '账号认证已失效，请重新绑定后核验' : '每日积分查询暂未完成，请重试' })
      }
    }
    return { ok: true, value: { accounts } }
  }
  if (call.method.startsWith('meoo.')) return { ok: true, value: await meoo.rpc(call.method, call.payload) }
  const requested = call.method === 'provider.status' && call.payload?.providers
  const vendorCall = Array.isArray(requested) ? { ...call, payload: { ...call.payload, providers: requested.filter(id => id !== 'meoo') } } : call
  const route = routes.get('/api/channel-pack')
  if (!route) throw new Error('渠道管理接口未注册')
  const response = await route.fetch(new Request('http://localhost/api/channel-pack', {
    method: 'POST', headers: { 'content-type': 'application/json' }, signal,
    body: JSON.stringify({ type: 'client-request', rpcId: 'standalone', method: 'channel-pack', payload: vendorCall }),
  }))
  const reply = await response.json()
  if (call.method === 'provider.status' && reply.result?.ok && (!requested || requested.includes('meoo'))) {
    const state = await meoo.rpc('meoo.status')
    reply.result.value.statuses.meoo = { accounts: { total: state.accounts.length, enabled: state.accounts.filter(a => a.projectId).length }, models: { total: meoo.publicModelRows().length, disabled: 0 } }
  }
  if (call.method === 'credits.claimAll' && reply.result?.ok) {
    for (const row of reply.result.value?.results ?? []) {
      if (row.outcome?.kind === 'claimed' && Number.isFinite(row.outcome.credit) && row.outcome.credit > 0) {
        const key = `${call.payload.provider}/${row.accountId}`
        const previous = todayClaims.get(key)
        todayClaims.set(key, { day: chinaDay(), credit: (previous?.day === chinaDay() ? previous.credit : 0) + row.outcome.credit })
      }
    }
  }
  return reply.result
}

/**
 * Project the Anthropic/harness request shape onto the OpenAI body the channel
 * adapters parse. `/v1/messages` hands over harness messages and flat tools,
 * not an `openAi` body; without this the adapter rejects the turn for having no
 * messages and silently loses tools, max_tokens and temperature.
 */
function harnessBody(request) {
  const body = { messages: toChatMessages(Array.isArray(request.messages) ? request.messages : []) }
  if (Array.isArray(request.tools) && request.tools.length > 0) body.tools = toToolDefs(request.tools)
  if (typeof request.maxTokens === 'number') body.max_tokens = request.maxTokens
  if (typeof request.temperature === 'number') body.temperature = request.temperature
  if (typeof request.reasoningEffort === 'string' && request.reasoningEffort !== '') body.reasoning_effort = request.reasoningEffort
  return body
}

async function complete(request, id, signal) {
  if (request.model.startsWith('meoo/')) return meoo.complete({ ...request, signal }, chunk => send({ type: 'chunk', id, chunk }))
  const slash = request.model.indexOf('/')
  const provider = request.model.slice(0, slash)
  const model = request.model.slice(slash + 1)
  // 禁用模型和供应商必须同时影响统一 API，不能只隐藏页面。
  if (!(await ctx.llm.listModels(provider)).some(row => row.id === model)) throw Object.assign(new Error('模型不存在或已停用'), { statusCode: 404 })
  const body = { ...(request.responsesBody ?? request.openAi ?? harnessBody(request)), model: request.model }
  const resolved = await ctx.llm.resolveModel(provider, model, signal)
  const effort = normalizeReasoningEffort(request.responses ? responsesReasoningEffort(body) : body.reasoning_effort, resolved, provider, model,
    notice => logger.info(`${request.model}：思考档位 ${notice.requested} ${notice.applied ? `映射为 ${notice.applied}` : '使用模型默认'}`))
  const requested = request.responses ? responsesMaxOutputTokens(body) : body.max_completion_tokens ?? body.max_tokens
  const maxTokens = typeof requested === 'number' ? normalizeMaxTokens(requested, provider, model) : undefined
  const convert = request.responses ? toResponsesGenerateOptions : toGenerateOptions
  const options = await convert(body, signal, effort, maxTokens, { bridge: attachments }, dropped => logger.warn(`忽略无法表达的工具类型：${dropped.type}`))
  options.sessionId = `standalone:${String(body.user ?? body.conversation ?? 'shared')}`
  for await (const chunk of ctx.llm.stream(markGatewayChannel(options))) {
    if (signal.aborted) throw new Error('请求已取消')
    send({ type: 'chunk', id, chunk })
  }
}

parentPort.on('message', async message => {
  if (message.type === 'cancel') { controllers.get(message.id)?.abort(); return }
  if (message.type === 'close') {
    closed = true
    for (const controller of controllers.values()) controller.abort()
    await meoo.close()
    credentials.dispose()
    for (const cleanup of cleanups.reverse()) { try { await cleanup() } catch {} }
    send({ type: 'closed' })
    return
  }
  const { id, type } = message
  const controller = new AbortController()
  controllers.set(id, controller)
  try {
    if (closed) throw new Error('渠道已停止')
    if (type === 'models') send({ type: 'result', id, value: await models() })
    else if (type === 'rpc') send({ type: 'result', id, value: await rpc(message.call, controller.signal) })
    else if (type === 'complete') { await complete(message.request, id, controller.signal); send({ type: 'result', id }) }
    else throw new Error('未知渠道操作')
  } catch (error) {
    send({ type: 'error', id, message: error.message, statusCode: error.statusCode ?? error.status })
  } finally { controllers.delete(id) }
})

apply(ctx, { disableOpencode: true })
await meoo.ready
send({ type: 'ready', providers: [...adapters.keys(), 'meoo'] })
