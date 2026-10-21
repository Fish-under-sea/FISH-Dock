import { randomUUID } from 'node:crypto'

const failure = (statusCode, message) => Object.assign(new Error(message), { statusCode })
const safeStatus = value => Number.isInteger(value) && value >= 400 && value <= 599 ? value : 502

/** 只改变验证调度和诊断，不修改请求正文、客户端身份或请求头。 */
export function createWorkBuddyValidation({ settings, complete }) {
  let busy = false
  let stopped = settings.get().workbuddyValidationStopped === true
  let enabled = settings.get().workbuddyValidationEnabled === true
  const persist = patch => {
    settings.update(patch)
    settings.flush()
    if (settings.writeFailed) throw failure(500, '验证状态未能保存；已在当前进程停止，请检查配置目录权限。')
  }
  return {
    state: () => ({
      enabled: settings.get().workbuddyValidationEnabled === true,
      stopped: stopped || settings.get().workbuddyValidationStopped === true,
      busy,
      last: settings.get().workbuddyValidationLast ?? null,
    }),
    settingsChanged() {
      const next = settings.get().workbuddyValidationEnabled === true
      if (!enabled && next && settings.get().workbuddyValidationStopped !== true) stopped = false
      enabled = next
    },
    async complete(request, onChunk) {
      if (!request.model?.startsWith('workbuddy/') || settings.get().workbuddyValidationEnabled !== true) return complete(request, onChunk)
      if (stopped || settings.get().workbuddyValidationStopped === true) throw failure(409, 'WorkBuddy 验证已停止。请先确认服务方的账号状态，再手动关闭并重新开启验证模式。')
      if (busy) throw failure(429, '已有一个 WorkBuddy 验证请求在执行；未发送本次请求。')
      busy = true
      const started = Date.now()
      const requestId = randomUUID()
      const record = status => ({ requestId, status, durationMs: Math.max(0, Date.now() - started), at: new Date().toISOString() })
      try {
        const result = await complete(request, onChunk)
        if (result?.error) throw failure(safeStatus(result.error?.statusCode ?? result.error?.status), '验证未成功')
        persist({ workbuddyValidationLast: record(200) })
        return result
      } catch (error) {
        stopped = true
        const status = safeStatus(error?.statusCode ?? error?.status)
        persist({ workbuddyValidationStopped: true, workbuddyValidationLast: record(status) })
        throw failure(status, `WorkBuddy 验证停止（HTTP ${status}，本地关联号 ${requestId}）。未重试或切换账号。`)
      } finally { busy = false }
    },
  }
}
