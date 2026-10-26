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
    if (row.error) waiter.reject(new Error(row.error)); else waiter.resolve(row.result)
  })
  function call(method, params = {}) {
    if (pending.size >= 32) return Promise.reject(new Error('插件页面请求过多，请稍后重试'))
    const id = ++sequence
    return new Promise((resolve, reject) => {
      const timer = setTimeout(() => { pending.delete(id); reject(new Error('插件页面请求超时')) }, 20000)
      pending.set(id, { resolve, reject, timer })
      parent.postMessage({ type: 'fish:call', ticket, id, method, params }, '*')
    })
  }
  let layout = 'single'
  function reportLayout() {
    const main = document.querySelector('main') || document.body
    if (!main) return
    const height = Math.ceil(main.getBoundingClientRect().height)
    if (height > 0) parent.postMessage({ type:'fish:layout', ticket, height, layout }, '*')
  }
  function setLayout(value) {
    if (!['single','double'].includes(value)) return
    layout = value
    requestAnimationFrame(reportLayout)
  }
  function observeLayout() {
    parent.postMessage({ type: 'fish:ready', ticket }, '*')
    const main = document.querySelector('main') || document.body
    if (!main || typeof ResizeObserver === 'undefined') return
    new ResizeObserver(reportLayout).observe(main)
  }
  if (document.readyState === 'loading') document.addEventListener('DOMContentLoaded', observeLayout, { once: true }); else observeLayout()
  parent.postMessage({ type: 'fish:ready', ticket }, '*')
  Object.defineProperty(window, 'FishDock', { value: Object.freeze({ call, setLayout }), writable: false, configurable: false })
})()
