// /v1 本地 API 的客户端（T-B）。给本机的脚本/工具用：
//
//   1. 读宿主的本地 token（`<数据目录>/agent-api/token`，用户私有）；
//   2. 用一个新随机 nonce 向 `GET /v1/info?nonce=…` 要 proof，
//      自己算 HMAC-SHA256(token, nonce) 并比对；
//   3. 对上了才发 Bearer。对不上、或 info 里没有 proof：一个带 Bearer 的请求都不发。
//
// token 只在请求头里，不进 URL、不进日志、不进错误正文。这个机制只证明「拿着 token」，
// 不宣称能防同一个用户下读得到 token 的进程。
import { createHmac, randomBytes } from 'node:crypto'
import { readFile } from 'node:fs/promises'
import { join } from 'node:path'

// 一个 nonce：16 字节随机，hex（服务端只收 1..128 位的 alnum/-/_/.）。
export function newNonce() {
  return randomBytes(16).toString('hex')
}

export function proofOf(token, nonce) {
  return createHmac('sha256', token).update(nonce).digest('hex')
}

// 读 token；坏掉（空、太短、不是十六进制）就是错的，不猜、不换。
export async function readToken(dataDir) {
  const path = join(dataDir, 'agent-api', 'token')
  let text
  try {
    text = (await readFile(path, 'utf8')).trim()
  } catch (error) {
    throw new Error(`无法读取 agent-api token（${path}）：${error.code || error.message}`)
  }
  if (text.length < 32 || !/^[0-9a-f]+$/i.test(text)) throw new Error(`agent-api token 损坏（${path}）`)
  return text
}

// 连上 /v1：验过 proof 才返回一个会带 Bearer 的客户端。
// `fetchImpl` 方便夹具注入（默认用全局 fetch）。
export async function connect({ base, token, fetchImpl = globalThis.fetch } = {}) {
  if (!base) throw new Error('缺少 base（http://127.0.0.1:<端口>）')
  if (!token) throw new Error('缺少 token')
  const nonce = newNonce()
  const url = `${base}/v1/info?nonce=${encodeURIComponent(nonce)}`
  const res = await fetchImpl(url, { headers: { accept: 'application/json' }, cache: 'no-store' })
  if (!res.ok) throw new Error(`/v1/info 失败：HTTP ${res.status}`)
  const info = await res.json()
  const want = proofOf(token, nonce)
  if (typeof info.proof !== 'string' || info.proof.toLowerCase() !== want) {
    // 到这里为止没有发过 Bearer，之后也不会发。
    throw new Error('proof 校验失败：不发送 Bearer')
  }
  const authFetch = (path, options = {}) =>
    fetchImpl(`${base}${path}`, {
      ...options,
      headers: { accept: 'application/json', ...(options.headers || {}), authorization: `Bearer ${token}` },
    })
  return {
    info,
    sessions: async (query = '') => {
      const suffix = query ? `?${query.replace(/^\?/, '')}` : ''
      const answer = await authFetch(`/v1/sessions${suffix}`)
      if (!answer.ok) throw new Error(`/v1/sessions 失败：HTTP ${answer.status}`)
      return answer.json() // { epoch, seq, sessions }
    },
    // SSE：把每个事件的 data 解析好交出来。abort 一个 AbortController 就停。
    events: async function* (options = {}) {
      const query = options.since ? `?since=${encodeURIComponent(options.since)}` : ''
      const answer = await authFetch(`/v1/events${query}`, { headers: { accept: 'text/event-stream', ...(options.headers || {}) }, signal: options.signal })
      if (!answer.ok) throw new Error(`/v1/events 失败：HTTP ${answer.status}`)
      const reader = answer.body.getReader()
      const decoder = new TextDecoder()
      let buffer = ''
      for (;;) {
        const { done, value } = await reader.read()
        if (done) return
        buffer += decoder.decode(value, { stream: true })
        for (;;) {
          const end = buffer.indexOf('\n\n')
          if (end < 0) break
          const frame = buffer.slice(0, end)
          buffer = buffer.slice(end + 2)
          const lines = frame.split('\n')
          const event = lines.find(line => line.startsWith('event: '))?.slice(7) || 'message'
          const data = lines.filter(line => line.startsWith('data: ')).map(line => line.slice(6)).join('\n')
          if (!data) continue // 心跳（": ping"）
          yield { event, id: lines.find(line => line.startsWith('id: '))?.slice(4) || '', data: JSON.parse(data) }
        }
      }
    },
  }
}
