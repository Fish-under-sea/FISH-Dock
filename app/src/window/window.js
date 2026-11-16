// 插件外部窗口的宿主容器（2.2.0 §5）。只做三件事：
//   ① 把一个**已授权**的插件页面装进沙箱 iframe（票据由宿主在建窗时签发）；
//   ② 把页面桥（fish:call / fish:ready / fish:escape）转给 Rust，把结果与宿主事件转回页面；
//   ③ 页面上报 ready 之前窗口是隐藏的；Escape 走宿主出口真正销毁窗口。
// 这里不画任何业务 UI，也不认识任何插件 ID：票据才是身份。
;(function () {
  const core = window.__TAURI__ && window.__TAURI__.core
  if (!core || !core.invoke) return
  const invoke = core.invoke
  const frame = document.getElementById('page')
  const note = document.getElementById('note')
  let ticket = ''
  let ready = false
  let generation = 0
  const MAX_CALL_BYTES = 2 * 1024 * 1024
  const pending = new Set()

  function validateCall(row) {
    if (!row || row.type !== 'fish:call' || !Number.isSafeInteger(row.id) || row.id < 1) return '插件页面请求编号无效（错误码 INVALID_REQUEST）'
    if (typeof row.method !== 'string' || !/^[a-zA-Z0-9_.-]{1,100}$/.test(row.method) || /^(host|plugin)\./.test(row.method)) return '插件页面请求方法无效（错误码 INVALID_METHOD）'
    let length = 0
    try {
      length = JSON.stringify(row).length
    } catch {
      return '插件页面请求无法序列化为 JSON（错误码 INVALID_PAYLOAD）'
    }
    if (length > MAX_CALL_BYTES) return `插件页面单次请求超过上限（${length} > ${MAX_CALL_BYTES} 字符，上限 2 MiB）（错误码 PAYLOAD_TOO_LARGE）`
    return ''
  }

  function accepts(row) {
    return validateCall(row) === ''
  }

  function say(text) {
    if (!note) return
    note.textContent = text || ''
  }

  async function start() {
    let grant
    try {
      grant = await invoke('plugin_window_attach')
    } catch (error) {
      say(String((error && error.message) || error))
      return
    }
    if (!grant || !grant.url || !grant.ticket) {
      say('插件窗口页面不可用')
      return
    }
    ticket = grant.ticket
    generation = grant.generation
    pending.clear()
    document.title = grant.title || '插件窗口'
    // 透明浮窗：窗口自己透出桌面；带原生标题栏的普通窗口给一块中性底色。
    document.documentElement.classList.toggle('opaque', grant.transparent !== true)
    frame.title = grant.title || '插件窗口'
    frame.src = grant.url
  }

  function toPage(row) {
    try {
      frame.contentWindow && frame.contentWindow.postMessage(row, '*')
    } catch {
      /* 页面还没就绪/已销毁：丢掉这一条，宿主那边自己的超时会收尾 */
    }
  }

  window.addEventListener('message', async event => {
    const row = event.data
    // 只认这个 iframe、沙箱来源（opaque 的 'null'）和本窗口票据发来的消息。
    if (!frame.contentWindow || event.source !== frame.contentWindow || event.origin !== 'null') return
    if (!row || row.ticket !== ticket) return
    if (row.type === 'fish:ready') {
      if (ready) return
      ready = true
      say('')
      try {
        await invoke('plugin_window_ready')
      } catch (error) {
        say(String((error && error.message) || error))
      }
      return
    }
    if (row.type === 'fish:escape') {
      void invoke('plugin_window_close').catch(() => {})
      return
    }
    if (row.type !== 'fish:call') return
    const validId = Number.isSafeInteger(row.id) && row.id >= 1
    const rejected = validateCall(row)
    if (rejected || !accepts(row)) {
      if (validId) toPage({ type: 'fish:result', ticket, id: row.id, error: rejected })
      return
    }
    if (pending.has(row.id)) return
    if (pending.size >= 16) {
      toPage({ type: 'fish:result', ticket, id: row.id, error: '插件页面并发请求过多（上限 16）（错误码 TOO_MANY_REQUESTS）' })
      return
    }
    pending.add(row.id)
    let answer
    try {
      answer = { result: await invoke('plugin_window_call', { method: row.method, params: row.params || {} }) }
    } catch (error) {
      answer = { error: String((error && error.message) || error) }
    } finally {
      pending.delete(row.id)
    }
    toPage({ type: 'fish:result', ticket, id: row.id, ...answer })
  })

  // 宿主事件（ready/closed/anchorChanged/fileDrop/dock）交给插件页。2.3.4（#88 B3）：
  // 用 `...payload` 全量转发，保留 dock 的 type/action/rect/x/y 等字段（旧代码只挑固定
  // 几个字段，把 dock 事件内容全丢了）；投递类型与 extension-page.js 一致。
  try {
    if (window.__TAURI__ && window.__TAURI__.event && window.__TAURI__.event.listen) {
      window.__TAURI__.event.listen('plugin:window-event', event => {
        const payload = event && event.payload
        if (!payload || !ticket || payload.generation !== generation) return
        toPage({ type: 'fish:window-event', ticket, ...payload })
      })
    }
  } catch {
    /* 事件桥不可用时页面仍能通过 request/response 工作 */
  }

  // Escape：焦点在容器自己（标题栏、空白处）时也要能真正关掉透明浮窗。
  document.addEventListener('keydown', event => {
    if (event.key === 'Escape') void invoke('plugin_window_close').catch(() => {})
  })

  void start()
  window.FishContainer = { start }
})()