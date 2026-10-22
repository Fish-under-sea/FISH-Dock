import { useCallback, useEffect, useRef, useState } from 'react'
import { LogIn, RefreshCw, Trash2, Link as LinkIcon } from 'lucide-react'
import { providers } from 'ofm-provider-list'
import { Button } from './ui'

type Rpc = (method: string, payload?: object) => Promise<any>
interface Account { id: string; name: string; projectId?: string; projectName?: string; models: string[]; expiresAt?: number }
interface Project { id: string; name: string }

function MeooLoginDialog({ loginUrl, busy, error, onCancel }: { loginUrl: string; busy: boolean; error: string; onCancel(): void }) {
  const dialog = useRef<HTMLDialogElement>(null)
  useEffect(() => {
    const element = dialog.current!
    element.showModal()
    return () => { element.close() }
  }, [])
  return <dialog ref={dialog} className="ofm_modal ofm-login-dialog" aria-label="登录 Meoo" onCancel={event => { event.preventDefault(); if (!busy) onCancel() }}>
    <div className="ofm_modalhead">登录 Meoo</div>
    <div className="ofm_modalbody ofm_loginbody">
      <div className="ofm_loginstep"><b>1</b><span>在浏览器中完成登录（若没有自动打开，点下面的按钮）。</span></div>
      <div className="ofm_loginstep"><b>2</b><span>登录成功后本页会自动完成，无需复制任何东西。</span></div>
      <code className="ofm_mono" style={{ wordBreak: 'break-all', whiteSpace: 'pre-wrap' }}>{loginUrl}</code>
      <p className="ofm_note ofm_scanline" role="status">已打开登录页，等待授权…（最长等待 10 分钟）</p>
      {error && <p role="alert" className="channel-error">{error}</p>}
    </div>
    <div className="ofm_modalfoot">
      <a className="ofm_btn primary" href={loginUrl} target="_blank" rel="noreferrer noopener">打开登录页</a>
      <Button variant="outline" disabled={busy} onClick={onCancel} autoFocus>取消</Button>
    </div>
  </dialog>
}

export function MeooAccounts({ rpc, onChanged }: { rpc: Rpc; onChanged(): void }) {
  const [accounts, setAccounts] = useState<Account[]>([])
  const [loading, setLoading] = useState(true)
  const [open, setOpen] = useState<'accounts' | 'models'>()
  const provider = providers.find(row => row.id === 'meoo')!
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState('')
  const [message, setMessage] = useState('')
  const [action, setAction] = useState('')
  const [balances, setBalances] = useState<Array<{ accountId: string; balance: { total: number } | null; error?: string }>>([])
  const refreshBalances = async () => {
    const result = await rpc('meoo.balances')
    if (alive.current) setBalances(result.accounts)
  }
  const [flow, setFlow] = useState<{ flowId: string; loginUrl: string }>()
  const [projects, setProjects] = useState<Record<string, Project[]>>({})
  const [chosen, setChosen] = useState<Record<string, string>>({})
  const alive = useRef(false)
  const reload = useCallback(async () => {
    const state = await rpc('meoo.status')
    if (alive.current) { setAccounts(state.accounts); setLoading(false) }
  }, [rpc])
  useEffect(() => {
    alive.current = true
    void reload().catch(f => { if (alive.current) { setError(f.message); setLoading(false) } })
    return () => { alive.current = false }
  }, [reload])
  useEffect(() => {
    if (!flow) return
    let stopped = false
    let timer: ReturnType<typeof setTimeout>
    const poll = async () => {
      try {
        const state = await rpc('meoo.login.poll', { flowId: flow.flowId })
        if (stopped || !alive.current) return
        if (state.status === 'success') { setFlow(undefined); await reload(); onChanged(); return }
        if (['failed', 'cancelled', 'expired'].includes(state.status)) { setFlow(undefined); setError(state.error ?? (state.status === 'expired' ? 'Meoo 授权已超时' : 'Meoo 授权已结束')); return }
        timer = setTimeout(() => { void poll() }, 2000)
      } catch (failure) { if (!stopped && alive.current) { setError((failure as Error).message); timer = setTimeout(() => { void poll() }, 4000) } }
    }
    void poll()
    return () => { stopped = true; clearTimeout(timer) }
  }, [flow, rpc, reload, onChanged])
  const run = async (operation: () => Promise<void>, name = '') => {
    if (busy) return
    setBusy(true); setAction(name); setError(''); setMessage('')
    try { await operation() } catch (failure) { if (alive.current) setError((failure as Error).message) }
    finally { if (alive.current) { setBusy(false); setAction('') } }
  }
  const login = () => {
    const popup = window.open('about:blank', '_blank')
    void run(async () => {
      try {
        const next = await rpc('meoo.login.start')
        if (popup) { popup.opener = null; popup.location.href = next.loginUrl }
        if (alive.current) setFlow(next)
      } catch (failure) { popup?.close(); throw failure }
    })
  }
  const modelIds = [...new Set(accounts.flatMap(account => account.models))]
  const balanceKnown = accounts.length > 0 && accounts.every(account => Number.isFinite(balances.find(row => row.accountId === account.id)?.balance?.total))
  return <article className="ofm_chan ofm_glass" data-state={accounts.length ? 'on' : 'idle'} style={{ '--chan-accent': provider.accent } as React.CSSProperties}>
    <div className="ofm_chanhead">
      <span className="ofm_chanlogo" aria-hidden="true"><img src={provider.logo} alt="" width={38} height={38} /></span>
      <span className="ofm_chanid"><b>{provider.name}</b><span>{provider.org}</span></span>
      <span className="ofm_chanstate"><span className={`ofm_dot ${accounts.length ? 'ok' : ''}`} />{loading ? '读取中' : accounts.length ? '已接入' : '未接入'}</span>
    </div>
    <p className="ofm_note">{provider.note}</p>
    <div className="ofm_chanmeta"><span>账号 <b>{loading ? '…' : `${accounts.length}/${accounts.length}`}</b></span><span>模型 <b>{loading ? '…' : modelIds.length ? `${modelIds.length}/${modelIds.length}` : '—'}</b></span><span>剩余积分 <b>{balanceKnown ? accounts.reduce((total, account) => total + (balances.find(row => row.accountId === account.id)?.balance?.total ?? 0), 0) : '—'}</b></span></div>
    <section aria-label="Meoo 账号管理" aria-busy={loading || busy}>
    <div className="ofm_chanacts">
      <Button onClick={login} disabled={busy || !!flow}><LogIn size={16} />添加账号</Button>
      <Button variant="outline" disabled={busy || loading} onClick={() => void run(reload)}><RefreshCw size={16} />刷新</Button>
      <Button variant="outline" busy={action === 'balance'} disabled={busy || loading || !accounts.length} onClick={() => void run(async () => { await refreshBalances(); if (alive.current) setMessage('余额已刷新') }, 'balance')}><RefreshCw size={16} />{action === 'balance' ? '刷新中…' : '刷新余额'}</Button>
      <Button busy={action === 'claim'} disabled={busy || loading || !accounts.length} onClick={() => void run(async () => {
        const value = await rpc('meoo.claimAll')
        if (alive.current) setMessage(value.results.map((r: any) => `${r.nickname}：${r.outcome.kind === 'claimed' ? `已领取 ${r.outcome.credit} 积分` : r.outcome.kind === 'already-claimed' ? '今日已领取，无新增积分' : r.outcome.message}`).join('；'))
        await refreshBalances(); onChanged()
      }, 'claim')}>{action === 'claim' ? '领取中…' : '一键领取'}</Button>
      <Button variant="outline" busy={action === 'refresh'} disabled={busy || loading || !accounts.length} onClick={() => void run(async () => { await rpc('meoo.refresh'); await reload(); onChanged(); if (alive.current) setMessage('账号续期完成') }, 'refresh')}>{action === 'refresh' ? '续期中…' : '刷新续期'}</Button>
    </div>
    <div className="ofm_chanfolds">
      {(['accounts', 'models'] as const).map(tab => <button key={tab} type="button" className="ofm_foldtoggle" data-open={open === tab} aria-expanded={open === tab} aria-controls={`meoo-${tab}`} onClick={() => setOpen(previous => previous === tab ? undefined : tab)}><svg viewBox="0 0 12 12" aria-hidden="true"><path d="M3 1.5l6 4.5-6 4.5z" /></svg>{tab === 'accounts' ? '账号' : '模型'} ({tab === 'accounts' ? accounts.length : modelIds.length})</button>)}
    </div>
    {flow && <MeooLoginDialog loginUrl={flow.loginUrl} busy={busy} error={error} onCancel={() => { void run(async () => { await rpc('meoo.login.cancel', { flowId: flow.flowId }); if (alive.current) setFlow(undefined) }) }} />}
    {loading ? <p role="status">正在读取账号…</p> : accounts.length === 0 ? <p className="ofm_note">尚未绑定 Meoo 账号</p> : null}
    <div id="meoo-accounts" hidden={open !== 'accounts'} className="ofm_chanfold">{accounts.map(a => <div className="ofm_acct meoo-account" key={a.id}>
      <div className="ofm_row"><h3>{a.name}</h3><span>{a.projectName ?? '尚未选择项目'}</span>
        <Button variant="ghost" title={`删除 ${a.name}`} aria-label={`删除 ${a.name}`} disabled={busy} onClick={() => {
          if (window.confirm(`删除 ${a.name} 的本机登录绑定？`)) void run(async () => { await rpc('meoo.remove', { accountId: a.id }); await reload(); onChanged() })
        }}><Trash2 size={16} /></Button></div>
      <div className="ofm_row">
        <Button variant="outline" disabled={busy} onClick={() => void run(async () => {
          const state = await rpc('meoo.projects', { accountId: a.id })
          if (alive.current) { setProjects(prev => ({ ...prev, [a.id]: state.projects })); setChosen(prev => ({ ...prev, [a.id]: a.projectId ?? state.projects[0]?.id ?? '' })) }
        })}><RefreshCw size={15} />读取已有项目</Button>
        {projects[a.id] && <>
          <select aria-label={`${a.name} 的项目`} value={chosen[a.id] ?? ''} disabled={busy} onChange={event => setChosen(prev => ({ ...prev, [a.id]: event.target.value }))}>
            <option value="">选择已有项目</option>{projects[a.id].map(p => <option key={p.id} value={p.id}>{p.name}</option>)}
          </select>
          <Button disabled={busy || !chosen[a.id]} onClick={() => void run(async () => { await rpc('meoo.bind', { accountId: a.id, projectId: chosen[a.id] }); await reload(); onChanged() })}><LinkIcon size={15} />绑定模型服务</Button>
          {projects[a.id].length === 0 && <span>该账号暂无可选项目</span>}
        </>}
      </div>
      {balances.find(row => row.accountId === a.id) && <p className="ofm_note">{balances.find(row => row.accountId === a.id)?.error ?? `当前余额：${balances.find(row => row.accountId === a.id)?.balance?.total ?? '待确认'} 积分`}</p>}
      {a.models.length > 0 && <p className="ofm_note">{a.models.length} 个模型：{a.models.join('、')}</p>}
    </div>)}</div>
    <div id="meoo-models" hidden={open !== 'models'} className="ofm_chanfold"><p className="ofm_note">{modelIds.length ? `${modelIds.length} 个模型` : '绑定已有项目后显示可用模型'}</p>{modelIds.map(id => <div className="ofm_row" key={id}><code>{id}</code></div>)}</div>
    {message && <p role="status" aria-live="polite" className="ofm_note">{message}</p>}
    {error && <div className="channel-error" role="alert">{error}</div>}
    </section>
  </article>
}
