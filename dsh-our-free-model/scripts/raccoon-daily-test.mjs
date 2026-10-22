import test from 'node:test'
import assert from 'node:assert/strict'
import { readRaccoonDailyStatus } from '../packages/standalone/channels/raccoon-daily.mjs'

const now = new Date('2026-10-22T16:30:00+08:00')
function fixture({ daily = 0, items = [], balanceStatus = 200, billStatus = 200 } = {}) {
  const calls = []
  return { calls, async fetcher(url, init) {
    calls.push({ url, init })
    const bills = url.includes('/bills?')
    const status = bills ? billStatus : balanceStatus
    return Response.json({ code: status === 200 ? 0 : status, data: bills ? { items } : { daily_points: daily } }, { status })
  } }
}
const read = f => readRaccoonDailyStatus({ base: 'https://xiaohuanxiong.com/api/web/points/v1', headers: { Authorization: 'Bearer fixture-secret' }, fetcher: f.fetcher, now })

test('每日核验只按今日 daily_grant 账单确认发放，不混入一次性奖励', async () => {
  const f = fixture({ daily: 17, items: [
    { id: 1, biz_type: 'reward_grant', points: 3000, created_at: '2026-10-22T01:00:00+08:00' },
    { id: 2, biz_type: 'daily_grant', points: 300, created_at: '2026-10-22T00:00:01+08:00' },
    { id: 3, biz_type: 'daily_grant', points: 300, created_at: '2026-10-21T23:59:59+08:00' },
  ] })
  const result = await read(f)
  assert.equal(result.state, 'confirmed')
  assert.equal(result.grantedToday, 300)
  assert.equal(result.dailyBalance, 17)
  assert.equal(result.claimSupported, false)
  assert.equal(f.calls.length, 2)
  assert.ok(f.calls.every(c => c.init.method === 'GET'))
  assert.doesNotMatch(JSON.stringify(result), /fixture-secret|Authorization|created_at/)
})

test('余额为零或近期未找到今日账单，保持未确认而不冒充领取成功或未领取', async () => {
  const f = fixture()
  const result = await read(f)
  assert.equal(result.state, 'unconfirmed')
  assert.equal(result.dailyBalance, 0)
  assert.equal(result.grantedToday, null)
  assert.equal(result.claimSupported, false)
})

test('今日账单按 Asia/Shanghai 跨日判断，拒绝缺时间与负积分记录', async () => {
  const f = fixture({ daily: 3, items: [
    { biz_type: 'daily_grant', points: 300, created_at: '2026-10-21T16:01:00Z' },
    { biz_type: 'daily_grant', points: 500, created_at: '2026-10-21T15:59:00Z' },
    { biz_type: 'daily_grant', points: 900 },
    { biz_type: 'daily_grant', points: -100, created_at: '2026-10-22T01:00:00+08:00' },
  ] })
  const result = await read(f)
  assert.equal(result.grantedToday, 300)
})

test('账单故障保留已读余额但不推断今日领取状态', async () => {
  const f = fixture({ daily: 29, billStatus: 503 })
  const result = await read(f)
  assert.equal(result.state, 'unconfirmed')
  assert.equal(result.dailyBalance, 29)
  assert.equal(result.grantedToday, null)
  assert.equal(result.billingAvailable, false)
})

test('鉴权失败不伪造余额，查询不访问任何领取接口', async () => {
  const f = fixture({ balanceStatus: 401, billStatus: 401 })
  await assert.rejects(read(f), { statusCode: 401 })
  assert.ok(f.calls.every(c => !c.url.includes('/grant') && c.init.method === 'GET'))
})
