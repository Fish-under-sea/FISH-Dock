import { useEffect, useRef, useState } from 'react'
import { RefreshCw } from 'lucide-react'
import { Button } from './ui'

type Daily = { accountId: string; nickname: string; state: 'confirmed' | 'unconfirmed'; dailyBalance: number | null; grantedToday: number | null; billingAvailable?: boolean; day?: string; error?: string }
export function RaccoonDaily({ rpc }: { rpc(method: string, payload?: object): Promise<any> }) {
  const [rows, setRows] = useState<Daily[]>()
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState('')
  const alive = useRef(false)
  useEffect(() => { alive.current = true; return () => { alive.current = false } }, [])
  async function refresh() {
    setBusy(true); setError('')
    try {
      const value = await rpc('raccoon.daily.status', {})
      if (alive.current) setRows(value.accounts)
    } catch (failure) { if (alive.current) setError((failure as Error).message) }
    finally { if (alive.current) setBusy(false) }
  }
  return <section aria-label="每日积分" aria-busy={busy}>
    <div className="ofm_row"><h3>每日积分</h3><Button variant="outline" disabled={busy} onClick={() => { void refresh() }}><RefreshCw size={15} />{busy ? '查询中' : '核验今日积分'}</Button></div>
    {rows && <div role="status" aria-live="polite">
      {rows.length === 0 && <p className="ofm_note">暂无已启用的商汤账号</p>}
      {rows.map(row => <div key={row.accountId} className="ofm_row"><strong>{row.nickname}</strong>
        <span>{row.error ?? (row.state === 'confirmed' ? `${row.day} 已确认发放 ${row.grantedToday} 积分` : '近期账单未确认今日发放')}</span>
        {!row.error && <span>每日余额：{row.dailyBalance ?? '未返回'}</span>}
        {!row.error && row.billingAvailable === false && <span>账单暂未取得</span>}
      </div>)}
    </div>}
    {error && <div className="channel-error" role="alert">{error}</div>}
  </section>
}
