import { useState } from 'react'
import { Check, Copy, Eye, EyeOff, Network, RotateCcw, ShieldAlert } from 'lucide-react'
import { Button, Switch } from './components/ui'
import type { Host, Summary } from './types'
import './sharing.css'

export function LanAccess({ summary, host }: { summary: Summary; host: Host }) {
  const [key, setKey] = useState(''), [busy, setBusy] = useState(false), [error, setError] = useState(''), [message, setMessage] = useState('')
  const [copied, setCopied] = useState('')
  const lan = summary.lan
  const run = async (action: () => Promise<void>) => {
    if (busy) return
    setBusy(true); setError(''); setMessage('')
    try { await action() } catch (reason) { setError(reason instanceof Error ? reason.message : '操作失败，请重试。') }
    finally { setBusy(false) }
  }
  const copy = (id: string, value: string) => void run(async () => {
    await host.copy(value); setCopied(id)
    window.setTimeout(() => setCopied(current => current === id ? '' : current), 1600)
  })
  const enabled = summary.settings.standaloneLanEnabled === true
  const toggle = (next: boolean) => void run(async () => {
    if (next && !window.confirm('允许局域网设备使用独立密钥调用推理接口？请仅在可信网络开启。Windows 防火墙仍可能需要手动放行。')) return
    await host.saveSettings({ standaloneLanEnabled: next })
    if (!next) setKey('')
    await host.refresh()
  })
  return <section className="sharing-section" aria-labelledby="lan-title">
    <div className="sharing-heading"><Network size={19} /><div><h2 id="lan-title">局域网访问</h2><p>独立推理密钥 · 管理页面仅限本机</p></div>
      <Switch aria-label="启用局域网访问" checked={enabled} disabled={busy} onCheckedChange={toggle} /></div>
    <p role="status">{lan?.running ? '局域网推理已启用' : enabled ? '局域网推理未就绪' : '局域网访问已关闭'}</p>
    {lan?.error && <p className="connection-error" role="alert"><ShieldAlert size={16} />{lan.error}</p>}
    {enabled && <>
      {lan?.running && lan.baseUrls.map((url, index) => <div className="copy-field" key={url}>
        <div className="cx-field"><input aria-label={`局域网地址 ${index + 1}`} readOnly value={url} /></div>
        <Button variant="outline" disabled={busy} onClick={() => copy(url, url)}>{copied === url ? <Check size={14} /> : <Copy size={14} />}复制</Button>
      </div>)}
      {lan?.running && lan.baseUrls.length === 0 && <p className="footnote">未找到可公布的局域网地址，请检查网卡连接。</p>}
      <label className="field-label" htmlFor="lan-key">局域网 API Key</label>
      <div className="copy-field"><div className="cx-field"><input id="lan-key" readOnly type={key ? 'text' : 'password'} value={key} placeholder="点击查看后显示" /></div>
        <Button variant="outline" disabled={busy} onClick={() => void run(async () => setKey(key ? '' : await host.getLanKey()))}>{key ? <EyeOff size={14} /> : <Eye size={14} />}{key ? '隐藏局域网密钥' : '查看局域网密钥'}</Button>
        <Button variant="outline" disabled={busy || !key} onClick={() => copy('lan-key', key)}><Copy size={14} />复制密钥</Button>
      </div>
      <Button variant="outline" disabled={busy} onClick={() => {
        if (!window.confirm('轮换后旧局域网密钥立即失效，本机密钥不受影响。确认继续吗？')) return
        void run(async () => { setKey(await host.rotateLanKey()); setMessage('局域网密钥已轮换，请更新其他设备。') })
      }}><RotateCcw size={14} />轮换局域网密钥</Button>
      <p className="footnote">当前端口由服务提供；不包含管理和备份接口。网络传输使用 HTTP，请勿映射到公网。</p>
    </>}
    {message && <p role="status">{message}</p>}
    {error && <p className="connection-error" role="alert"><ShieldAlert size={16} />{error}</p>}
  </section>
}
