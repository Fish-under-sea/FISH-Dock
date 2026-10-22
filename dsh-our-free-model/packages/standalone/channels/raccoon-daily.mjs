const calendar = new Intl.DateTimeFormat('en-CA', { timeZone: 'Asia/Shanghai', year: 'numeric', month: '2-digit', day: '2-digit' })
const failure = (statusCode, message) => Object.assign(new Error(message), { statusCode })

/** 每日积分由服务端发放；续期后只按当日账单确认，不预填固定奖励。 */
export async function claimRaccoonDaily({ readStatus, refreshSession }) {
  const before = await readStatus()
  if (before.state === 'confirmed') return { kind: 'already-claimed', credit: 0, grantedToday: before.grantedToday }
  await refreshSession()
  const after = await readStatus()
  if (after.state === 'confirmed' && after.grantedToday > 0) return { kind: 'claimed', credit: after.grantedToday }
  return { kind: 'failed', code: -1, actionRequired: true, message: '今日积分暂未到账，请打开商汤官网完成今日登录后重试', actionUrl: 'https://raccoon.sensetime.com/' }
}

export async function readRaccoonDailyStatus({ base, headers, fetcher = fetch, now = new Date() }) {
  async function get(path) {
    const response = await fetcher(`${base}/${path}`, { method: 'GET', headers, signal: AbortSignal.timeout(15000) })
    if (!response.ok) throw failure(response.status, `每日积分查询失败（HTTP ${response.status}）`)
    const body = await response.json()
    if (body.code !== 0 || !body.data) throw failure(502, '每日积分查询未返回有效数据')
    return body.data
  }
  const [balance, billing] = await Promise.allSettled([get('balance'), get('bills?paging.limit=100&paging.offset=0')])
  if (balance.status === 'rejected') throw balance.reason
  if (billing.status === 'rejected' && [401, 403].includes(billing.reason?.statusCode)) throw billing.reason
  const daily = balance.value.daily_points
  const dailyBalance = typeof daily === 'number' && Number.isFinite(daily) && daily >= 0 ? daily : null
  const today = calendar.format(now)
  const records = billing.status === 'fulfilled' && Array.isArray(billing.value.items) ? billing.value.items : null
  const matched = (records ?? []).filter(row => {
    if (row?.biz_type !== 'daily_grant' || typeof row.points !== 'number' || !Number.isFinite(row.points) || row.points <= 0) return false
    if (typeof row.created_at !== 'string' || !/(?:Z|[+-]\d{2}:\d{2})$/i.test(row.created_at)) return false
    const date = new Date(row.created_at)
    return Number.isFinite(date.valueOf()) && calendar.format(date) === today
  })
  return {
    state: matched.length ? 'confirmed' : 'unconfirmed', day: today,
    dailyBalance, grantedToday: matched.length ? matched.reduce((sum, row) => sum + row.points, 0) : null,
    billingAvailable: records !== null, recordsChecked: records?.length ?? 0,
    claimSupported: false,
  }
}
