import { randomUUID } from 'node:crypto'

const failure = (statusCode, message) => Object.assign(new Error(message), { statusCode })
const safeStatus = value => Number.isInteger(value) && value >= 400 && value <= 599 ? value : 502
const transient = status => status === 408 || status === 429 || status >= 500

/** 只记录验证结果，不改变原请求，也不把暂时故障变成永久停用。 */
export function createWorkBuddyValidation({ settings, complete, log = () => {} }) {
  let busy = false
  let stopped = settings.get().workbuddyValidationStopped === true
  let enabled = settings.get().workbuddyValidationEnabled === true
  let last = settings.get().workbuddyValidationLast ?? null
  const persist = patch => {
    try {
      settings.update(patch)
      settings.flush()
      if (settings.writeFailed) log('WorkBuddy 验证诊断暂未落盘；当前请求结果保持不变。')
    } catch {
      log('WorkBuddy 验证诊断保存失败；当前请求结果保持不变。')
    }
  }
  // 仅迁移有明确临时故障证据的旧锁停；鉴权失败和未知原因保持原状态。
  if (stopped && transient(last?.status ?? 0)) {
    stopped = false
    persist({ workbuddyValidationStopped: false })
  }
  return {
    state: () => ({ enabled: settings.get().workbuddyValidationEnabled === true, stopped, busy, last }),
    settingsChanged() {
      const next = settings.get().workbuddyValidationEnabled === true
      if (!enabled && next && settings.get().workbuddyValidationStopped !== true) stopped = false
      enabled = next
    },
    async complete(request, onChunk) {
      if (!request.model?.startsWith('workbuddy/') || settings.get().workbuddyValidationEnabled !== true) return complete(request, onChunk)
      if (stopped) throw failure(409, 'WorkBuddy 鉴权验证已停止，请重新绑定账号，或关闭并重新开启验证模式。')
      if (busy) throw failure(429, '已有一个 WorkBuddy 验证请求在执行；未发送本次请求。')
      busy = true
      const started = Date.now()
      const requestId = randomUUID()
      const record = status => ({ requestId, status, durationMs: Math.max(0, Date.now() - started), at: new Date().toISOString() })
      try {
        const result = await complete(request, onChunk)
        if (result?.error) {
          const detail = result.error
          throw Object.assign(failure(safeStatus(detail?.statusCode ?? detail?.status), 'WorkBuddy 上游返回错误'), {
            code: detail?.code, cause: detail,
          })
        }
        last = record(200)
        persist({ workbuddyValidationStopped: false, workbuddyValidationLast: last })
        return result
      } catch (error) {
        const cancelled = request.signal?.aborted || error?.name === 'AbortError' || error?.code === 'ABORT_ERR'
        const status = cancelled ? 499 : safeStatus(error?.statusCode ?? error?.status)
        stopped = status === 401 || status === 403
        last = record(status)
        persist({ workbuddyValidationStopped: stopped, workbuddyValidationLast: last })
        // 原始错误保留 Retry-After、业务码与取消语义；调度层负责既有退避策略。
        if (error && (typeof error === 'object' || typeof error === 'function')) throw error
        throw failure(status, `WorkBuddy 请求失败（HTTP ${status}，关联号 ${requestId}）`)
      } finally { busy = false }
    },
  }
}
