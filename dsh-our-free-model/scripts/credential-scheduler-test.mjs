import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { stripTypeScriptTypes } from 'node:module'
import vm from 'node:vm'

const source = readFileSync(new URL('../vendor/channel-pack/src/index.ts', import.meta.url), 'utf8')
const start = source.indexOf('  const REFRESH_INTERVAL_MS =')
const end = source.indexOf('  // 保留旧的 stop scheduler', start)
assert.ok(start >= 0 && end > start, '定位真实的续期调度器，而非复制实现')
const scheduler = stripTypeScriptTypes(source.slice(start, end))

for (const initial of ['empty', 'existing', 'storage-error']) {
  const intervals = [], cleared = [], cleanups = [], refreshed = [], warnings = []
  const auths = Object.fromEntries([
    'service', 'buddy', 'workbuddy', 'lobsterai', 'qoder', 'qoderCn', 'trae', 'cline', 'loomy', 'raccoon', 'minimax', 'zcode', 'gemini',
  ].map(name => [name, { refreshAll: async () => { refreshed.push(name) }, stop() {} }]))
  const sandbox = {
    ...auths, zcodeAdapter: { stop() {} },
    pool: { listAllAccounts: async () => {
      if (initial === 'storage-error') throw new Error('fixture storage temporarily unavailable')
      return initial === 'existing' ? [{ id: 'fixture' }] : []
    } },
    ctx: {
      logger: { warn: value => warnings.push(value) },
      effect: callback => { cleanups.push(callback()) },
    },
    setInterval: (callback, delay) => { const timer = { callback, delay, unref() {} }; intervals.push(timer); return timer },
    clearInterval: timer => { cleared.push(timer) },
  }
  vm.runInNewContext(scheduler, sandbox)
  await new Promise(resolve => setImmediate(resolve))
  assert.equal(intervals.length, 1, `${initial}: 即使首轮没有账号或存储暂时失败也注册续期`)
  assert.equal(intervals[0].delay, 30 * 60 * 1000)
  assert.equal(refreshed.length, initial === 'existing' ? 13 : 0, '空池不发送无意义的启动续期')
  assert.equal(warnings.length, initial === 'storage-error' ? 1 : 0)
  refreshed.length = 0
  await intervals[0].callback()
  await new Promise(resolve => setImmediate(resolve))
  assert.equal(refreshed.length, 13, '稍后登录的账号仍由周期续期处理')
  for (const cleanup of cleanups) cleanup?.()
  assert.equal(cleared.length, 1, '销毁时清理定时器')
  console.log(`ok  凭据续期调度器：${initial}`)
}
