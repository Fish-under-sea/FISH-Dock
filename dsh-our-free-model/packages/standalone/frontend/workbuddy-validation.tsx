import { useEffect, useRef, useState } from 'react'
import { CircleAlert, Play, ShieldCheck, Square } from 'lucide-react'
import { Badge, Button, Switch } from './components/ui'
import type { Host, Summary, WorkBuddyValidationState } from './types'
import './sharing.css'

export function WorkBuddyValidation({ summary, host }: { summary: Summary; host: Host }) {
  const enabled = summary.settings.workbuddyValidationEnabled === true
  const status = summary.workbuddyValidation
  const models = summary.catalog.filter(row => row.id.startsWith('workbuddy/') && row.routable)
  const [selected, setSelected] = useState('')
  const model = models.some(row => row.id === selected) ? selected : models[0]?.id ?? ''
  const [saving, setSaving] = useState(false)
  const [running, setRunning] = useState(false)
  const [error, setError] = useState('')
  const [last, setLast] = useState<WorkBuddyValidationState['last']>(null)
  const pending = useRef<AbortController | null>(null)
  const mounted = useRef(true)
  const changing = useRef(false)
  useEffect(() => () => { mounted.current = false; pending.current?.abort() }, [])
  useEffect(() => { if (status?.last) setLast(status.last) }, [status?.last])
  const toggle = async (next: boolean) => {
    if (changing.current || pending.current) return
    if (next && !window.confirm('开启 WorkBuddy 透明验证模式？客户端身份与用户指令保持原样。每次请求仅发送一次，拒绝或失败后锁停；不会自动换号。此操作本身不发送推理请求。')) return
    changing.current = true; setSaving(true); setError('')
    try {
      await host.saveSettings({ workbuddyValidationEnabled: next })
      await host.refresh()
    } catch (reason) { if (mounted.current) setError(reason instanceof Error ? reason.message : '保存失败') }
    finally { changing.current = false; if (mounted.current) setSaving(false) }
  }
  const run = async () => {
    if (!enabled || !model || pending.current || changing.current || status?.stopped) return
    if (!window.confirm(`使用当前已启用的 WorkBuddy 账号，向 ${model} 发送一次验证请求？将产生真实上游调用，遇到拒绝立即停止。`)) return
    const controller = new AbortController(); pending.current = controller; setRunning(true); setError('')
    try {
      const result = await host.validateWorkBuddy(model, controller.signal)
      if (mounted.current && !controller.signal.aborted) setLast(result)
    } catch (reason) {
      if (mounted.current) setError(controller.signal.aborted ? '已取消，验证已停止。' : reason instanceof Error ? reason.message : '验证失败，已停止。')
    } finally {
      pending.current = null
      if (mounted.current) {
        setRunning(false)
        try { await host.refresh() } catch { setError(previous => previous || '状态刷新失败，请重新连接本地服务。') }
      }
    }
  }
  const busy = running || status?.busy === true
  return <section className="sharing-section" aria-labelledby="workbuddy-validation-title">
    <div className="sharing-heading"><ShieldCheck size={21} /><div><h2 id="workbuddy-validation-title">WorkBuddy 透明验证</h2></div>
      <Badge tone={status?.stopped ? 'warning' : enabled ? 'success' : 'neutral'}>{status?.stopped ? '已停止' : enabled ? '已启用' : '已关闭'}</Badge>
      <Switch aria-label="WorkBuddy 透明验证模式" checked={enabled} disabled={saving || busy} onCheckedChange={next => { void toggle(next) }} />
    </div>
    {status?.stopped && <p role="status">验证已锁停。请先确认服务方账号状态，再手动关闭并重新开启。</p>}
    <div className="validation-controls">
      <label htmlFor="workbuddy-validation-model">验证模型</label>
      <select id="workbuddy-validation-model" value={model} onChange={event => setSelected(event.target.value)} disabled={busy || !models.length}>
        {!models.length && <option value="">暂无已启用的 WorkBuddy 模型</option>}
        {models.map(row => <option key={row.id} value={row.id}>{row.name} · {row.id}</option>)}
      </select>
      <Button disabled={!enabled || !summary.settings.enabled || !model || saving || busy || status?.stopped} onClick={() => { void run() }}><Play size={15} />{running ? '验证进行中' : '发送一次验证'}</Button>
      {running && <Button variant="outline" onClick={() => pending.current?.abort()}><Square size={15} />取消</Button>}
    </div>
    {error && <p className="settings-save-error" role="alert"><CircleAlert size={15} />{error}</p>}
    {last && <dl className="validation-result" aria-label="最近验证结果"><dt>HTTP 状态</dt><dd>{last.status}</dd><dt>耗时</dt><dd>{last.durationMs} ms</dd><dt>本地关联号</dt><dd><code>{last.requestId}</code></dd></dl>}
  </section>
}
