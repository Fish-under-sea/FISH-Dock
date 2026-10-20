import { useState } from 'react'
import { Download, ShieldAlert } from 'lucide-react'
import { Button } from './components/ui'
import type { Host } from './types'
import './sharing.css'

export function BackupExport({ host }: { host: Host }) {
  const [busy, setBusy] = useState(false), [message, setMessage] = useState(''), [error, setError] = useState('')
  const download = async () => {
    if (busy || !window.confirm('导出的 ZIP 含明文 OFM 账号凭据、授权令牌和 API Key，可被持有者直接使用。确认仅保存在可信位置？')) return
    setBusy(true); setMessage(''); setError('')
    try { await host.exportBackup(); setMessage('备份已交给浏览器下载，请确认文件已保存。') }
    catch (reason) { setError(reason instanceof Error ? reason.message : '导出失败，请重试。') }
    finally { setBusy(false) }
  }
  return <section className="sharing-section" aria-labelledby="backup-title">
    <div className="sharing-heading"><Download size={19} /><div><h2 id="backup-title">OFM 迁移备份</h2><p>账号、配置与独立密钥</p></div></div>
    <p>ZIP 含明文凭据和固定迁移说明。不包含 DSH 配置、邮件凭据、聊天记录或桌面布局。</p>
    <Button variant="outline" busy={busy} onClick={() => void download()}><Download size={15} />{busy ? '正在生成备份' : '导出 OFM ZIP'}</Button>
    {message && <p role="status">{message}</p>}
    {error && <p className="connection-error" role="alert"><ShieldAlert size={16} />{error}</p>}
  </section>
}
