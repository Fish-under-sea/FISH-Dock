// 插件浏览器 SDK：只连接父页面，所有业务调用由票据锁定当前插件。
;(function () {
  const ticket = location.pathname.split('/')[1]
  let sequence = 0
  const pending = new Map()
  window.addEventListener('message', event => {
    const row = event.data
    if (event.source !== parent || !row || row.ticket !== ticket) return
    if (row.type === 'fish:theme') {
      const palette = row.colors || {}
      for (const [name, value] of Object.entries(palette)) if (/^--dock-[a-z-]+$/.test(name) && typeof value === 'string' && /^(#[a-f\d]{3,8}|rgba?\([\d.,%\s]+\))$/i.test(value)) document.documentElement.style.setProperty(name, value)
      document.documentElement.dataset.dockTheme = row.mode === 'light' ? 'light' : 'dark'
      const title = document.getElementById('title')
      if (title && typeof row.title === 'string') title.textContent = row.title
      return
    }
    if (row.type !== 'fish:result') return
    const waiter = pending.get(row.id)
    if (!waiter) return
    pending.delete(row.id); clearTimeout(waiter.timer)
    if (row.error) waiter.reject(failure(row.error)); else waiter.resolve(row.result)
  })
  // ---- 外部原生窗口与定向拖入（2.2.0）-------------------------------------------
  // 宿主只给**本插件**的窗口发事件；页面按事件名自己分辨。
  //   ready / closed / anchorChanged：本插件某个外部窗口的生命周期；
  //   fileDrop：enter / leave（不含路径）与 drop（含 paths，仅本插件收到）。
  const windowListeners = new Set()
  const dropListeners = new Set()
  const dockListeners = new Set()
  function listen(set, listener) {
    if (typeof listener !== 'function') throw new TypeError('事件监听器必须是函数')
    set.add(listener)
    return () => set.delete(listener)
  }
  function dispatch(payload) {
    for (const listener of [...windowListeners]) {
      try { listener(payload) } catch (error) { console.error('FishDock windows listener', error) }
    }
    if (payload.event === 'dock') {
    for (const listener of [...dockListeners]) {
      try { listener(payload) } catch (error) { console.error('FishDock dock listener', error) }
    }
  }
  if (payload.event === 'fileDrop') {
      for (const listener of [...dropListeners]) {
        try { listener(payload) } catch (error) { console.error('FishDock fileDrop listener', error) }
      }
    }
  }
  window.addEventListener('message', event => {
    const row = event.data
    if (event.source !== parent || !row || row.ticket !== ticket) return
    if (row.type === 'fish:window-event') dispatch(row)
  })
  function handle(value, what) {
    if (typeof value !== 'string' || !value) throw new Error(`${what}句柄无效`)
    return value
  }
  const windows = Object.freeze({
    // 页面就绪后返回不透明句柄；同一 key 重复调用复用同一个实例（宿主判定）。
    async open(options = {}) {
      const result = await send('dock.windows.open', options)
      return result && result.handle
    },
    async update(handle, options = {}) {
      return send('dock.windows.update', { ...options, handle: handleOf(handle, '窗口') })
    },
    async close(handle) {
      const result = await send('dock.windows.close', { handle: handleOf(handle, '窗口') })
      return !!(result && result.closed)
    },
    async list() {
      const result = await send('dock.windows.list', {})
      return (result && result.windows) || []
    },
    onEvent: listener => listen(windowListeners, listener),
  })
  const fileDrop = Object.freeze({
    // scope: 'dock'（主 Dock，独占）或 'window' + handle（本插件自己的窗口）。
    async subscribe(options = {}) {
      const result = await send('dock.fileDrop.subscribe', options)
      return result && result.handle
    },
    async unsubscribe(handle) {
      const result = await send('dock.fileDrop.unsubscribe', { handle: handleOf(handle, '拖入订阅') })
      return !!(result && result.unsubscribed)
    },
    onEvent: listener => listen(dropListeners, listener),
  })
  // 2.2.2：主岛几何与状态。rect() 读实时快照；onEvent 订阅后宿主才开始按需
  // 上报（没有任何订阅者时主岛页面不上报，零开销）。
  const dock = Object.freeze({
    async rect() {
      return send('dock.dock.rect', {})
    },
    async onEvent(listener) {
      if (typeof listener !== 'function') throw new Error('事件监听器必须是函数')
      const off = listen(dockListeners, listener)
      await send('dock.dock.subscribe', {})
      return async () => {
        off()
        try { await send('dock.dock.unsubscribe', {}) } catch { /* 页面已关：宿主已清理 */ }
      }
    },
  })
  // 宿主保留 Escape 出口：透明浮窗没有原生关闭按钮时，页面里按 Escape 也要能真正关掉。
  document.addEventListener('keydown', event => {
    if (event.key !== 'Escape') return
    parent.postMessage({ type: 'fish:escape', ticket }, '*')
  })
  function handleOf(value, what) {
    return handle(value, what)
  }
  // 宿主把插件业务错误编码成「<文案>（错误码 CODE）」（见 plugin_protocol.rs 的 business_error）：
  // 这里还原成带 code 的 Error，页面就能按码查表显示专属文案；不是这种形状的提示按原文报。
  const CODE_TAIL = /（错误码 ([A-Z][A-Z0-9_]{0,39})）$/
  function failure(text) {
    const raw = String(text ?? '')
    const hit = CODE_TAIL.exec(raw)
    const error = new Error(hit ? raw.slice(0, hit.index) : raw)
    if (hit) error.code = hit[1]
    return error
  }
  // 页面能调的业务方法。但这一个必须**只**能从「可信点击」或「手势内的 openExternal」发出：
  // 否则脚本直接 FishDock.call('dock.open_external', {url}) 就能绕过手势与节流开浏览器。
  // 注意别一刀切禁 dock.*：dock.autostart 是页面正常在用的（既有设计，不在本次收紧范围）。
  const RESERVED_BY_BRIDGE = new Set(['dock.open_external'])
  function call(method, params = {}) {
    if (RESERVED_BY_BRIDGE.has(method)) return Promise.reject(new Error('该能力只能由点击触发'))
    return send(method, params)
  }
  // 桥内部发送（不对外暴露）：openExternal / 可信点击走这里，绕不过上面的保留拒绝，因为
  // 只有闭包内的这两条路径能拿到它。
  function send(method, params = {}) {
    if (pending.size >= 32) return Promise.reject(new Error('插件页面请求过多，请稍后重试'))
    const id = ++sequence
    return new Promise((resolve, reject) => {
      const timer = setTimeout(() => { pending.delete(id); reject(new Error('插件页面请求超时')) }, 20000)
      pending.set(id, { resolve, reject, timer })
      parent.postMessage({ type: 'fish:call', ticket, id, method, params }, '*')
    })
  }
  let layout = 'single'
  // 已发过的「高度:布局」：ResizeObserver 回调很密，同一个尺寸不重复发给宿主。
  let sentLayout = ''
  let layoutFrame = 0
  function measure() {
    const main = document.querySelector('main') || document.body
    return main ? Math.ceil(main.getBoundingClientRect().height) : 0
  }
  function reportLayout() {
    const height = measure()
    if (!(height > 0)) return
    const key = `${height}:${layout}`
    if (key === sentLayout) return
    sentLayout = key
    parent.postMessage({ type:'fish:layout', ticket, height, layout }, '*')
  }
  // 一帧只量一次、只发一次：观察器的回调成串到来时，别把同一个尺寸发好几遍。
  function scheduleLayout() {
    if (layoutFrame) return
    layoutFrame = 1
    const flush = () => { layoutFrame = 0; reportLayout() }
    if (typeof requestAnimationFrame === 'function') requestAnimationFrame(flush)
    else flush()
  }
  function setLayout(value) {
    if (!['single','double'].includes(value)) return
    layout = value
    scheduleLayout()
  }
  let announced = false, watching = false
  // 观察 main 的尺寸；只挂一次（解析早期 main 可能还没出现，DOMContentLoaded 再试一次）。
  function watch() {
    if (watching) return
    const main = document.querySelector('main') || document.body
    if (!main || typeof ResizeObserver === 'undefined') return
    watching = true
    new ResizeObserver(scheduleLayout).observe(main)
  }
  // ready 只发一次（2.0.3 在文件顶层与 DOMContentLoaded 各发一次，宿主会收到两条），
  // 但**必须在脚本执行时就发**：宿主只有收到 ready 才会向下投 fish:theme，而插件自己的
  // 经典脚本（挂在 body 里）在 DOMContentLoaded 之前就跑完了 —— 把 ready 推迟到
  // DOMContentLoaded 会让插件初始化时读不到主题（真实浏览器 evidence.theme undefined）。
  // 这里保留 2.0.3 的「立刻握手」时序，只去掉重复的那一条。
  function announce() {
    if (announced) return
    announced = true
    parent.postMessage({ type: 'fish:ready', ticket }, '*')
    reportLayout()   // 能测就测；解析早期高度为 0 时静默跳过，DOMContentLoaded 再补
    watch()
  }
  announce()
  // 解析完再补一次测量并挂观察器：ready 不重复发，尺寸这时才可靠。
  const settle = () => { reportLayout(); watch() }
  if (document.readyState === 'loading') document.addEventListener('DOMContentLoaded', settle, { once: true })
  else settle()
  // ---- 外链（#67）------------------------------------------------------------
  // iframe 沙箱只有 allow-scripts，也没有 allow-popups/allow-top-navigation：页面里的
  // <a href>、target=_blank、window.open 自己都打不开浏览器。这里把**用户真点了**的
  // http/https 链接交给宿主（dock.open_external，宿主侧只放行网页地址），
  // 其余协议（file:/javascript:/data:/vbscript: 等）一律不拦、不打开。
  // 只覆盖 `<a>` 点击；window.open 在沙箱里本就不可用，不属于 #67（不强行接管）。
  const WEB_SCHEMES = new Set(['http:', 'https:'])
  // 同一 URL 的 2 秒去重：连点同一个链接只开一次；点**不同**链接照常打开（不误伤用户）。
  const OPEN_GAP_MS = 2000
  const lastOpenedAt = new Map()
  // 与宿主 browser::is_web_url 同一口径：http/https、host 非空、无 userinfo、无 NUL。
  // scheme 先归一（`new URL()` 会把 `HTTPS://` 解析成 `https:`，再显式小写比较），
  // 判定通过时返回规范化后的 href —— 发给宿主的总是归一后的地址。
  function webTarget(url) {
    if (typeof url !== 'string' || url.includes('\0')) return null
    let parsed
    try { parsed = new URL(url) } catch { return null }
    const protocol = String(parsed.protocol).toLowerCase()
    if (!WEB_SCHEMES.has(protocol) || !parsed.hostname || parsed.username || parsed.password) return null
    return parsed.href
  }
  function isWebLink(url) { return webTarget(url) !== null }
  // 程序化调用只有在真实用户手势里才放行：浏览器支持 userActivation 时看 isActive，
  // 不支持的环境按「只信 trusted 点击」处理，绝不静默放宽。
  function inUserGesture() {
    const activation = typeof navigator !== 'undefined' ? navigator && navigator.userActivation : null
    return !!(activation && activation.isActive === true)
  }
  // 真正发给宿主的那一步：只有这条路（以及可信点击）能发出 dock.open_external。
  // 走桥内部的 send()，不复用对外的 call()——call() 会直接拒绝这个保留方法。
  function deliver(url) {
    const target = webTarget(url)
    if (!target) return Promise.reject(new Error('只支持 http/https 链接'))
    const now = Date.now()
    const last = lastOpenedAt.get(target)
    if (last !== undefined && now - last < OPEN_GAP_MS) return Promise.reject(new Error('这个链接刚打开过，请稍后再试'))
    lastOpenedAt.set(target, now)
    return send('dock.open_external', { url: target })
  }
  function openExternal(url) {
    if (!isWebLink(url)) return Promise.reject(new Error('只支持 http/https 链接'))
    if (!inUserGesture()) return Promise.reject(new Error('打开外链需要用户点击'))
    return deliver(String(url))
  }
  // 页面里的链接：可信点击 + 网页地址 → 拦掉默认导航（沙箱里反正也导航不了），改走宿主。
  function onClick(event) {
    if (!event || event.isTrusted !== true) return          // 脚本 dispatch 的假点击不放行
    const target = event.target
    const anchor = target && typeof target.closest === 'function' ? target.closest('a[href]') : null
    if (!anchor) return
    const href = anchor.getAttribute('href')
    if (!isWebLink(href)) return                             // 非 http/https：不拦、不打开
    event.preventDefault()
    deliver(href).catch(() => {})                            // 宿主拒绝时静默：外链失败不该炸页面
  }
  document.addEventListener('click', onClick, true)
  Object.defineProperty(window, 'FishDock', { value: Object.freeze({ call, setLayout, openExternal, windows, fileDrop, dock }), writable: false, configurable: false })
})()
