// 关闭设置时，落在面板上的确认提问一并作废（上游 bf6f0adb：Esc 能关、重开不回来）。
//
// 上游的 island-state.ts 把这类提问收在 approveFor 里；本仓的设置页没有 approveFor，
// 它自己的确认框状态是 confirmActive（正在显示的那一个）与 confirmQueue（排队的）。
// 取消按钮、Esc、点背景、✕ 都走 finishConfirm（同一条「作废」路径），关闭设置时也
// 一并作废——不这样，重开设置会又冒出上一次没答完的提问。
//
// 纯逻辑，便于单测：只认传进来的两个东西，不碰页面别处的状态。
//   finishActive() -> bool   真的作废了正在显示的那一个（没有则 false）
//   queue: [{ resolve }]     排队中的，逐个 resolve(false)（不答，当作否）
;(function (root) {
  // 作废全部：返回作废的条数（正在显示的 + 排队中的）。不打断正在干活的那一个
  // 由调用方决定（finishActive 自己判断；排队的一律作否）。
  function closeCleared(state) {
    if (!state) return 0
    let n = 0
    if (typeof state.finishActive === 'function' && state.finishActive()) n += 1
    const queue = Array.isArray(state.queue) ? state.queue : []
    while (queue.length) {
      const item = queue.shift()
      try {
        item?.resolve?.(false)
      } catch (error) {
        // 一个失败的 resolve 不该挡住其余的清场。
      }
      n += 1
    }
    return n
  }

  const api = { closeCleared }
  if (typeof module !== 'undefined' && module.exports) {
    module.exports = api
  } else {
    root.SettingsAsked = api
  }
})(this)
