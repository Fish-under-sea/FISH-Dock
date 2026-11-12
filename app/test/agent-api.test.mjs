// T-B 的客户端协议夹具：/v1 的 proof 与 Bearer 顺序（tools/misc/agent-api-client.mjs）。
//
// 要证明的是：客户端先验 HMAC-SHA256(token, nonce) 的 proof，验不过就一个带 Bearer
// 的请求都不发；token 不进 URL。服务端那一半（Host/Origin、游标、背压、释放）
// 在 src-tauri 的 agent_hub 测试里用真的 HTTP/SSE 验。
import assert from 'node:assert/strict'
import { mkdtemp, mkdir, writeFile, rm } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { createHmac } from 'node:crypto'
import { test } from 'node:test'

import { connect, newNonce, proofOf, readToken } from '../../tools/misc/agent-api-client.mjs'

const TOKEN = 'a'.repeat(63) + 'f' // 64 位 hex

async function tokenDir(tag, token = TOKEN) {
  const dir = await mkdtemp(join(tmpdir(), `fish-dock-agent-client-${tag}-`))
  await mkdir(join(dir, 'agent-api'), { recursive: true })
  await writeFile(join(dir, 'agent-api', 'token'), token)
  return dir
}

test('the nonce is fresh and the proof is HMAC-SHA256 over it', () => {
  const a = newNonce()
  const b = newNonce()
  assert.notEqual(a, b)
  assert.match(a, /^[0-9a-f]{32}$/)
  assert.equal(proofOf('Jefe', 'what do ya want for nothing?'), '5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843')
})

test('the token is read from the private file, and a broken one is an error', async () => {
  const dir = await tokenDir('read')
  assert.equal(await readToken(dir), TOKEN)
  await writeFile(join(dir, 'agent-api', 'token'), 'short')
  await assert.rejects(() => readToken(dir), /损坏/)
  await rm(dir, { recursive: true, force: true })
})

test('proof first: no Bearer is ever sent when the proof does not match', async () => {
  const dir = await tokenDir('proof')
  const token = await readToken(dir)
  const sent = []
  const fetchImpl = async (url, options = {}) => {
    sent.push({ url, headers: { ...(options.headers || {}) } })
    const nonce = new URL(url).searchParams.get('nonce')
    // 服务端算错的 proof（拿另一个 token 算的）：客户端必须拒绝。
    return { ok: true, status: 200, json: async () => ({ app: 'fish-dock', epoch: 1, seq: 0, proof: createHmac('sha256', 'another-token').update(nonce).digest('hex') }) }
  }
  await assert.rejects(() => connect({ base: 'http://127.0.0.1:9', token, fetchImpl }), /proof 校验失败/)
  assert.equal(sent.length, 1, 'only the info request')
  assert.ok(sent.every(call => !('authorization' in call.headers) && !('Authorization' in call.headers)), 'no Bearer at all')
  assert.ok(!sent.some(call => call.url.includes(token)), 'the token never appears in a URL')
  await rm(dir, { recursive: true, force: true })
})

test('after the proof, every call carries Bearer in the header only', async () => {
  const dir = await tokenDir('ok')
  const token = await readToken(dir)
  const sent = []
  const fetchImpl = async (url, options = {}) => {
    sent.push({ url, headers: { ...(options.headers || {}) } })
    if (url.includes('/v1/info')) {
      const nonce = new URL(url).searchParams.get('nonce')
      return { ok: true, status: 200, json: async () => ({ app: 'fish-dock', version: '2.0.4', epoch: 7, seq: 3, proof: proofOf(token, nonce) }) }
    }
    return { ok: true, status: 200, json: async () => ({ epoch: 7, seq: 3, sessions: [{ id: 's1' }] }) }
  }
  const client = await connect({ base: 'http://127.0.0.1:47213', token, fetchImpl })
  assert.equal(client.info.epoch, 7)
  const answer = await client.sessions('agent=codex')
  assert.equal(answer.sessions[0].id, 's1')
  const [info, sessions] = sent
  assert.ok(!('authorization' in info.headers), 'info stays unauthenticated')
  assert.equal(sessions.headers.authorization, `Bearer ${token}`)
  assert.match(sessions.url, /\/v1\/sessions\?agent=codex$/)
  assert.ok(!sessions.url.includes(token), 'the token is never in the URL')
  await rm(dir, { recursive: true, force: true })
})
