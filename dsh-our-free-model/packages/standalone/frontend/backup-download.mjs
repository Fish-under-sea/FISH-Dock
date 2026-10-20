export async function downloadBackup({ unauthorized, isCurrent = () => true, fetcher = fetch, doc = document, urls = URL }) {
  const response = await fetcher('/api/management/backup', {
    method: 'POST', credentials: 'same-origin', cache: 'no-store',
    headers: { 'content-type': 'application/json' }, body: JSON.stringify({ confirm: true }),
    signal: AbortSignal.timeout(60000),
  })
  if (!response.ok) {
    const value = await response.json().catch(() => ({}))
    if (isCurrent() && response.status === 401) unauthorized()
    throw new Error(value.error ?? `导出失败（${response.status}）`)
  }
  if (!isCurrent()) throw new Error('管理会话已变化，请重新操作。')
  if (!response.headers.get('content-type')?.startsWith('application/zip')) throw new Error('备份响应格式错误')
  const blob = await response.blob()
  if (!isCurrent()) throw new Error('管理会话已变化，请重新操作。')
  const name = response.headers.get('content-disposition')?.match(/filename="(OFM-backup-[A-Za-z0-9_-]+\.zip)"/)?.[1] ?? 'OFM-backup.zip'
  const url = urls.createObjectURL(blob)
  const link = doc.createElement('a')
  link.href = url; link.download = name
  try { doc.body.appendChild(link); link.click() }
  finally { link.remove(); setTimeout(() => urls.revokeObjectURL(url), 1000) }
}
