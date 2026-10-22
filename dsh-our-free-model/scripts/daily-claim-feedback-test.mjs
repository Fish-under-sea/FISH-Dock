import test from 'node:test'
import assert from 'node:assert/strict'
import { createMeooService } from '../packages/standalone/channels/meoo.mjs'
import * as raccoon from '../packages/standalone/channels/raccoon-daily.mjs'

function meooFixture() {
 const values = new Map([['MEOO_ACCOUNTS', JSON.stringify([{ id: 'a', name: '测试账号', accessToken: 'fixture-access', refreshToken: 'fixture-refresh', expiresAt: Date.now() + 3600000 }])]])
 let granted = false
 const calls = []
 const service = createMeooService({ credentials: { resolve: async k => values.has(k) ? { value: values.get(k) } : undefined, set: async (k, v) => values.set(k, v) }, fetcher: async (url, init) => {
  calls.push({ url, method: init.method })
  if (url.endsWith('/entitlement/get')) return Response.json({ success: true, data: { userPointInfo: { account: { availableBalance: 2400 } } } })
  if (url.endsWith('/entitlement/init')) { const first = !granted; granted = true; return Response.json({ success: true, data: { initialized: true, dailyLoginGranted: first, dailyLoginGrantAmount: first ? 2000 : 0 } }) }
  if (url.endsWith('/oauth/token')) return Response.json({ access_token: 'next', refresh_token: 'next-refresh', token_type: 'Bearer', expires_in: 3600, openid: 'u', tenant_id: 't' })
  throw Error(`unexpected ${url}`)
 } })
 return { service, calls }
}
test('Meoo 刷新余额只查询，领取使用官网初始化端点且重复领取不报新增', async t => {
 const f = meooFixture(); t.after(() => f.service.close())
 const balances = await f.service.rpc('meoo.balances')
 assert.equal(balances.accounts[0].balance.total, 2400)
 assert.ok(f.calls.every(c => !c.url.endsWith('/init')))
 const first = await f.service.rpc('meoo.claimAll')
 assert.equal(first.summary.totalCredit, 2000)
 assert.equal(first.summary.claimed, 1)
 const again = await f.service.rpc('meoo.claimAll')
 assert.equal(again.summary.totalCredit, 0)
 assert.equal(again.results[0].outcome.kind, 'already-claimed')
 assert.ok(f.calls.some(c => c.url === 'https://meoo.com/api/mc/entitlement/init' && c.method === 'GET'))
 assert.doesNotMatch(JSON.stringify(await f.service.rpc('meoo.status')), /fixture-access|fixture-refresh/)
})
test('商汤一键领取依据前后当日账单，已领和未确认不虚报 300 分', async () => {
 assert.equal(typeof raccoon.claimRaccoonDaily, 'function')
 let refreshes = 0, reads = 0
 const result = await raccoon.claimRaccoonDaily({ readStatus: async () => (++reads === 1 ? { state: 'unconfirmed', grantedToday: null } : { state: 'confirmed', grantedToday: 300 }), refreshSession: async () => refreshes++ })
 assert.equal(result.kind, 'claimed'); assert.equal(result.credit, 300); assert.equal(refreshes, 1)
 const already = await raccoon.claimRaccoonDaily({ readStatus: async () => ({ state: 'confirmed', grantedToday: 300 }), refreshSession: async () => { throw Error('不应再续期') } })
 assert.equal(already.kind, 'already-claimed')
 const pending = await raccoon.claimRaccoonDaily({ readStatus: async () => ({ state: 'unconfirmed', grantedToday: null }), refreshSession: async () => {} })
 assert.equal(pending.kind, 'failed'); assert.equal(pending.actionRequired, true)
})
