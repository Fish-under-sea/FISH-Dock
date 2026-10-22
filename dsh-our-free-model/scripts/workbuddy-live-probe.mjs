import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import { createCredentials } from '../packages/standalone/channels/credentials.mjs'
import { createChannelRuntime } from '../packages/standalone/channels/runtime.mjs'

const args = process.argv.slice(2)
const value = key => { const i = args.indexOf(key); return i >= 0 && !args[i + 1]?.startsWith('--') ? args[i + 1] : undefined }
if (!args.includes('--confirm') || !args.includes('--data-dir') || !args.includes('--model')) throw new Error('显式指定 --confirm --data-dir 和 --model；最多发送两次最小真实请求')
const root = path.resolve(value('--data-dir'))
const model = value('--model')
if (!model?.startsWith('workbuddy/')) throw new Error('仅支持显式选择的 WorkBuddy 模型')
const settings = JSON.parse(fs.readFileSync(path.join(root, 'settings.json'), 'utf8'))
const response = await fetch(`${value('--url') ?? 'http://127.0.0.1:18901'}/api/management/channels/rpc`, {
  method: 'POST', headers: { Authorization: `Bearer ${settings.forwardKey}`, 'content-type': 'application/json' },
  body: JSON.stringify({ method: 'account.list', payload: { provider: 'workbuddy' } }), signal: AbortSignal.timeout(15000),
})
if (!response.ok) throw new Error(`活动 Dock 账号查询 HTTP ${response.status}`)
const list = await response.json()
const account = list.value?.accounts?.find(a => a.enabled !== false)
if (!account) throw new Error('没有启用的 WorkBuddy 账号')
const existing = JSON.parse(fs.readFileSync(path.join(root, 'channel-credentials.json'), 'utf8'))
const credential = existing[account.credentialRef]
if (typeof credential !== 'string') throw new Error('所选账号凭据不存在')
const scratch = fs.mkdtempSync(path.join(os.tmpdir(), 'ofm-workbuddy-probe-'))
let runtime
try {
  fs.mkdirSync(path.join(scratch, 'channel-pack'))
  fs.writeFileSync(path.join(scratch, 'channel-pack/state.json'), JSON.stringify({ accounts: [{ ...account, id: 'isolated-probe', credentialRef: 'WORKBUDDY_PROBE', refreshable: false }], disabledModels: {} }), { mode: 0o600 })
  const store = createCredentials(scratch)
  await store.set('WORKBUDDY_PROBE', credential)
  store.dispose()
  runtime = await createChannelRuntime({ dataDir: scratch, logger: { info() {}, warn() {}, error() {} } })
  await runtime.ready
  for (const [kind, system] of [['neutral', 'You are a helpful assistant.'], ['identity', 'You are an AI agent powered by DeepSeek Harness.']]) {
    const started = Date.now()
    try {
      const outcome = await runtime.complete({ model, signal: AbortSignal.timeout(30000), openAi: { messages: [{ role: 'system', content: system }, { role: 'user', content: 'Reply with exactly OK.' }], max_tokens: 16 } })
      console.log(JSON.stringify({ kind, ok: !outcome.error, elapsedMs: Date.now() - started, returnedText: Boolean(outcome.text), businessCode: /11128|11140|429/.exec(String(outcome.error ?? ''))?.[0] }))
      if (outcome.error) break
    } catch (error) {
      console.log(JSON.stringify({ kind, ok: false, elapsedMs: Date.now() - started, http: error.statusCode ?? error.status, businessCode: /11128|11140|429/.exec(error.message)?.[0], cancelled: error.name === 'AbortError' || error.name === 'TimeoutError' }))
      break
    }
  }
} finally {
  await runtime?.close()
  if (path.dirname(scratch) !== os.tmpdir() || !path.basename(scratch).startsWith('ofm-workbuddy-probe-')) throw new Error('临时目录校验失败，保留供人工核对')
  fs.rmSync(scratch, { recursive: true, force: true })
}
