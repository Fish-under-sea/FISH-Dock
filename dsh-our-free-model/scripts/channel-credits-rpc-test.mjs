import assert from 'node:assert/strict'

/** 使用实际已打包 RPC，只提供账号池 ref，禁止真实网络与旧单凭据回退。 */
export async function verifyChannelCreditsRpc({ ctx, call, creds }) {
  const originalFetch = globalThis.fetch
  const providers = ['buddy', 'workbuddy', 'qoder', 'qodercn', 'loomy', 'minimax', 'lobsterai', 'zcode', 'gemini']
  let quotaRequests = 0, quotaFailed = true
  const originalResolve = ctx.credentials.resolve
  const resolvedRefs = []
  ctx.credentials.resolve = async ref => { resolvedRefs.push(ref); return originalResolve(ref) }
  try {
    for (const provider of providers) {
      const ref = `FIXTURE_CREDITS_${provider.toUpperCase()}`
      creds.set(ref, { value: JSON.stringify({
        access_token: 'fixture-credits-token', token_type: 'Bearer', user_id: 'fixture-user',
        zcode_jwt: 'fixture-credits-jwt', device_mid: 'fixture-device', account_label: '隔离账号',
      }) })
      await ctx.accountPool.addAccount({
        id: `fixture-credits-${provider}`, provider, nickname: '隔离账号',
        credentialRef: ref, enabled: true, refreshable: false, createdAt: 1,
      })
    }
    globalThis.fetch = async input => {
      const pathname = new URL(String(input)).pathname
      let body
      if (pathname.includes('get-user-resource')) {
        body = { code: 0, data: { Response: { Data: { Accounts: [{
          PackageName: 'fixture', CycleCapacityRemain: 123, Status: 1,
        }] } } } }
      } else if (pathname.includes('/me/usage')) {
        body = { qoderUsage: { userQuota: { total: 123, remaining: 123, used: 0, unit: 'credits' } } }
      } else if (pathname.includes('/points/records')) {
        body = { code: '000000', data: { balance: 123, dailyBalance: 0, availableBalance: 123 } }
      } else if (pathname.includes('/credit/details')) {
        body = { base_resp: { status_code: 0 }, total_count: 1, details: [{ remaining_amount: '123.00' }] }
      } else if (pathname.includes('/profile-summary')) {
        body = { code: 0, success: true, data: { totalCreditsRemaining: 123, creditItems: [] } }
      } else if (pathname.endsWith(':retrieveUserQuotaSummary')) {
        quotaRequests++
        if (quotaFailed) return new Response('本机配额查询失败', { status: 502 })
        body = { groups: [{ buckets: [
          { bucketId: 'gemini-5h', window: '5h', remainingFraction: 0.8, resetTime: new Date(Date.now() + 3600000).toISOString() },
          { bucketId: 'gemini-weekly', window: 'weekly', remainingFraction: 0.4, resetTime: new Date(Date.now() + 86400000).toISOString() },
        ] }] }
      } else throw new Error(`未预期的替身端点：${pathname}`)
      return Response.json(body)
    }
    for (const provider of providers.filter(id => id !== 'zcode' && id !== 'gemini')) {
      resolvedRefs.length = 0
      const result = await call('credits.balances', { provider })
      assert.equal(result.ok, true)
      assert.equal(result.value.accounts[0].balance?.total, 123, `${provider} 的账号池凭据应能查询余额`)
      assert.ok(resolvedRefs.includes(`FIXTURE_CREDITS_${provider.toUpperCase()}`))
    }
    assert.equal((await ctx.zcodeAuth.current()).zcode_jwt, 'fixture-credits-jwt')
    assert.equal((await ctx.zcodeAuth.status()).configured, true)
    creds.delete('FIXTURE_CREDITS_QODER')
    const missing = await call('credits.balances', { provider: 'qoder' })
    assert.equal(missing.value.accounts[0].balance, null)
    assert.equal(missing.value.accounts[0].error, '凭据未配置', '缺失凭据必须返回原因，不能伪造零余额')
    const unavailableQuota = await call('credits.balances', { provider: 'gemini' })
    assert.equal(unavailableQuota.value.accounts[0].balance, null)
    quotaFailed = false
    const cachedQuota = await call('credits.balances', { provider: 'gemini' })
    assert.equal(cachedQuota.value.accounts[0].balance, null)
    assert.equal(quotaRequests, 1, '自动查询保留原有缓存')
    const freshQuota = await call('credits.balances', { provider: 'gemini', force: true })
    assert.equal(quotaRequests, 2, '手动重试必须真正查询上游替身')
    assert.equal(freshQuota.value.accounts[0].balance.total, 60)
    const geminiRefresh = ctx.geminiAuth.refreshAccountCredential
    try {
      quotaFailed = true
      await call('credits.balances', { provider: 'gemini', force: true })
      await ctx.accountPool.updateAccount('fixture-credits-gemini', { refreshable: true, expiresAt: Date.now() - 1000 })
      ctx.geminiAuth.refreshAccountCredential = async (ref, pool, id) => {
        quotaFailed = false
        await pool.updateAccount(id, { expiresAt: Date.now() + 86400000 })
      }
      const recovered = await call('credits.balances', { provider: 'gemini' })
      assert.equal(recovered.value.accounts[0].balance?.total, 60, '续期成功必须绕过旧的鉴权失败缓存')
      assert.equal(quotaRequests, 4)
    } finally { ctx.geminiAuth.refreshAccountCredential = geminiRefresh }
    await verifyBalanceRefresh({ ctx, call, creds })
    await verifyRefreshDispatch({ ctx, call })
  } finally {
    ctx.credentials.resolve = originalResolve
    globalThis.fetch = originalFetch
    for (const provider of providers) {
      await ctx.accountPool.removeAccount(`fixture-credits-${provider}`)
      creds.delete(`FIXTURE_CREDITS_${provider.toUpperCase()}`)
    }
  }
}

async function verifyBalanceRefresh({ ctx, call, creds }) {
  const service = ctx.workbuddyAuth
  const saved = service.refreshAccountCredential
  const expired = 'fixture-balance-expired', failed = 'fixture-balance-failed'
  const before = Date.now() - 1000, after = Date.now() + 86400000
  const refreshes = []
  try {
    for (const id of [expired, failed]) {
      creds.set(id, { value: JSON.stringify({ access_token: 'expired-fixture', refresh_token: 'refresh-fixture' }) })
      await ctx.accountPool.addAccount({ id, provider: 'workbuddy', credentialRef: id, nickname: id,
        expiresAt: before, refreshable: true, enabled: false, createdAt: 1 })
    }
    service.refreshAccountCredential = async (ref, pool, id) => {
      refreshes.push(id)
      if (id === failed) throw new Error('续期网络暂时断开，请重试')
      creds.set(ref, { value: JSON.stringify({ access_token: 'fresh-fixture', refresh_token: 'rotated-fixture' }) })
      await pool.updateAccount(id, { expiresAt: after })
    }
    const result = await call('credits.balances', { provider: 'workbuddy', force: true })
    assert.equal(result.ok, true)
    assert.deepEqual(refreshes, [expired, failed], '到期账号在读余额前按自己的 ref 续期，停用不影响续期')
    assert.equal(result.value.accounts.find(row => row.accountId === expired).balance?.total, 123)
    assert.match(result.value.accounts.find(row => row.accountId === failed).error, /续期网络暂时断开/)
    assert.equal(result.value.accounts.find(row => row.accountId === failed).balance, null)
    assert.equal(ctx.accountPool.listAccountsByProvider('workbuddy').find(row => row.id === failed).refreshable, true, '网络故障不废弃账号')
    assert.equal(result.value.accounts.find(row => row.accountId === 'fixture-credits-workbuddy').balance.total, 123, '单账号故障不影响其他账号')
    refreshes.length = 0
    await call('credits.balances', { provider: 'workbuddy' })
    assert.deepEqual(refreshes, [failed], '已经续期的账号不重复消耗 refresh_token')
    await ctx.accountPool.removeAccount(failed)
    await ctx.accountPool.updateAccount(expired, { expiresAt: before })
    refreshes.length = 0
    let release
    const gate = new Promise(resolve => { release = resolve })
    service.refreshAccountCredential = async (ref, pool, id) => {
      refreshes.push(id)
      await gate
      await pool.updateAccount(id, { expiresAt: after })
    }
    const first = call('credits.balances', { provider: 'workbuddy' })
    const second = call('credits.balances', { provider: 'workbuddy' })
    await new Promise(resolve => setImmediate(resolve))
    const observed = [...refreshes]
    release()
    await Promise.all([first, second])
    assert.deepEqual(observed, [expired], '同账号的并发余额请求共享一次在途续期')
  } finally {
    service.refreshAccountCredential = saved
    for (const id of [expired, failed]) { await ctx.accountPool.removeAccount(id); creds.delete(id) }
  }
  console.log('ok  余额读取前续期：按账号 ref、停用账号、失败隔离、网络故障保留认证、有效令牌不重复续期')
}

async function verifyRefreshDispatch({ ctx, call }) {
  const providers = [
    'codearts', 'buddy', 'workbuddy', 'lobsterai', 'qoder', 'qodercn', 'trae',
    'cline', 'loomy', 'raccoon', 'minimax', 'zcode', 'gemini',
  ]
  for (const provider of providers) {
    const service = ctx[`${provider}Auth`]
    assert.ok(service, `${provider} auth service is mounted`)
    const saved = service.refreshAccountCredential
    const accountId = `fixture-refresh-${provider}`, ref = `FIXTURE_REFRESH_${provider.toUpperCase()}`
    const calls = []
    try {
      await ctx.accountPool.addAccount({
        id: accountId, provider, nickname: 'offline-refresh', credentialRef: ref,
        enabled: false, refreshable: true, createdAt: 1,
      })
      service.refreshAccountCredential = async (...args) => { calls.push(args) }
      const result = await call('account.refresh', { accountId })
      assert.deepEqual(result, { ok: true, value: { success: true } }, provider)
      assert.equal(calls.length, 1, provider)
      assert.deepEqual(calls[0], [ref, ctx.accountPool, accountId], `${provider}: exact ref, pool and id`)
      service.refreshAccountCredential = async () => { throw new Error('offline refresh rejected') }
      const rejected = await call('account.refresh', { accountId })
      assert.equal(rejected.ok, false, `${provider}: refresh failure is not success`)
      assert.match(rejected.error.message, /offline refresh rejected/)
    } finally {
      service.refreshAccountCredential = saved
      await ctx.accountPool.removeAccount(accountId)
    }
  }
  console.log('ok  refresh RPC: all 13 providers pass exact ref/pool/id; disabled accounts refresh; failures return ok:false')
}
