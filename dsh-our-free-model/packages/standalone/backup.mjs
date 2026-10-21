import fs from 'node:fs'
import path from 'node:path'
import crypto from 'node:crypto'
import { zipSync } from './vendor/fflate/fflate.mjs'
import { SETTINGS_INITIAL } from '../../src/store.js'

const LIMIT = 16 * 1024 * 1024
const README = fs.readFileSync(new URL('./backup-readme.md', import.meta.url))
const settingKeys = [...Object.keys(SETTINGS_INITIAL), 'forwardKey', 'standalonePort', 'standaloneProbe', 'standaloneLanEnabled', 'standaloneLanPort', 'standaloneLanKey']
const jsonBytes = value => Buffer.from(`${JSON.stringify(value, null, 2)}\n`)

// 只读取独立端白名单路径，拒绝符号链接，绝不遍历目录或接收客户端路径。
function optionalJson(root, relative) {
  const base = fs.realpathSync(root)
  const file = path.join(base, relative)
  try {
    let current = base
    for (const component of relative.split('/')) {
      current = path.join(current, component)
      if (fs.lstatSync(current).isSymbolicLink()) throw new Error('backup source is a link')
    }
    const stat = fs.statSync(file)
    if (!stat.isFile() || stat.size > LIMIT || !fs.realpathSync(file).startsWith(base + path.sep)) throw new Error('invalid backup source')
    const value = JSON.parse(fs.readFileSync(file, 'utf8'))
    if (!value || typeof value !== 'object' || Array.isArray(value)) throw new Error('invalid backup document')
    return value
  } catch (error) {
    if (error.code === 'ENOENT') return undefined
    throw new Error('无法安全读取 OFM 迁移文件，请检查数据目录')
  }
}

export async function createOfmBackup({ dataDir, settings, channels, serviceVersion }) {
  const result = await channels.rpc({ method: 'backup.export', payload: {} })
  const payload = result?.value?.payload
  if (result?.ok !== true || payload?.format !== 'dsh-codearts-auth/backup' || payload.version !== 1 || !Array.isArray(payload.accounts) || !payload.credentials || typeof payload.credentials !== 'object') {
    throw new Error('无法取得 OFM 账号快照')
  }
  const preferences = optionalJson(dataDir, 'channel-pack/state.json') ?? {}
  const state = {
    accounts: payload.accounts, disabledModels: payload.disabledModels,
    loomyPermanentLocked: payload.loomyPermanentLocked === true,
    ...typeof preferences.gatewayEnabled === 'boolean' ? { gatewayEnabled: preferences.gatewayEnabled } : {},
    ...Array.isArray(preferences.providerOrder) ? { providerOrder: preferences.providerOrder.filter(value => typeof value === 'string') } : {},
  }
  const files = {
    'README.md': README,
    'settings.json': jsonBytes(Object.fromEntries(settingKeys.filter(key => Object.hasOwn(settings, key)).map(key => [key, settings[key]]))),
    'channel-pack/accounts-backup.json': jsonBytes(payload),
    'channel-pack/state.json': jsonBytes(state),
    'channel-pack/permanent-locks.json': jsonBytes({ schema: 'dsh-codearts-auth/permanent-locks/v1', locks: payload.permanentLocks ?? {} }),
    'channel-credentials.json': jsonBytes(payload.credentials),
  }
  const eac = optionalJson(dataDir, 'eac-user.json')
  if (typeof eac?.token === 'string' && eac.token !== '') files['eac-user.json'] = jsonBytes({
    token: eac.token, login: typeof eac.login === 'string' ? eac.login : '',
    avatar: typeof eac.avatar === 'string' ? eac.avatar : '', savedAt: Number.isFinite(eac.savedAt) ? eac.savedAt : 0,
  })
  const entries = Object.entries(files)
  if (entries.reduce((sum, [, bytes]) => sum + bytes.length, 0) > LIMIT) throw Object.assign(new Error('OFM 备份超过 16 MiB，请减少账号后重试'), { statusCode: 413 })
  const at = new Date()
  files['manifest.json'] = jsonBytes({
    format: 'fish-dock/ofm-backup', version: 1, product: 'FISH Dock', serviceVersion,
    exportedAt: at.toISOString(), encrypted: false,
    missingCredentialCount: Array.isArray(result.value.warnings) ? result.value.warnings.length : 0,
    files: entries.map(([name, bytes]) => ({ path: name, bytes: bytes.length, sha256: crypto.createHash('sha256').update(bytes).digest('hex') })),
  })
  return { bytes: Buffer.from(zipSync(files, { level: 6 })), filename: `OFM-backup-${at.toISOString().replace(/[:.]/g, '-').replace(/Z$/, '')}.zip` }
}
