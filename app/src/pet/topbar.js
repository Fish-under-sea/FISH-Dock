// DEC-2：顶栏 / dock 页面脚本。窗口由 topbar.rs 创建（label topbar / dock），按 mode 渲染：
//   mode=topbar：左时间，中缩略项（点击带到前台），右状态入口。
//   mode=dock：底部图标条（运行中窗口，点击切换）。
// 数据走宿主事件 topbar:state（windows 列表 + clock），点击回 topbar_focus(hwnd)。
;(function () {
  const invoke = window.__TAURI__?.core?.invoke || window.__TAURI__?.invoke
  const listen = window.__TAURI__?.event?.listen
  const { t } = window.I18n || { t: k => k }
  const UI = window.TopbarUI
  const params = new URLSearchParams(location.search)
  const mode = params.get('mode') === 'dock' ? 'dock' : 'topbar'
  document.body.dataset.mode = mode

  const bar = document.getElementById('bar')
  const lead = document.getElementById('lead')
  const clockEl = lead?.querySelector('.clock')
  const itemsEl = document.getElementById('items')
  const end = document.getElementById('end')
  if (mode === 'topbar') {
    lead.hidden = false
    end.hidden = false
  }

  let state = { windows: [], clock: '' }

  function render() {
    const model = UI.topbarModel(state.windows, state.clock)
    if (clockEl) clockEl.textContent = model.clock
    itemsEl.replaceChildren()
    for (const it of model.items) {
      const btn = document.createElement('button')
      btn.className = 'item'
      btn.dataset.hwnd = String(it.hwnd)
      btn.title = it.title || it.app
      if (it.thumb) {
        const img = document.createElement('img')
        img.src = it.thumb
        img.alt = ''
        btn.append(img)
        // 缩略图不可用时降级：onerror 换成图标 + 标题。
        img.onerror = () => {
          img.remove()
          btn.append(fallback(it))
        }
      } else {
        btn.append(fallback(it))
      }
      btn.addEventListener('click', () => {
        const act = UI.clickAction(it)
        if (act.action === 'focus') invoke('topbar_focus', { hwnd: act.hwnd, restore: act.restore }).catch(() => {})
      })
      itemsEl.append(btn)
    }
  }

  function fallback(it) {
    const wrap = document.createElement('span')
    wrap.className = 'fb'
    const ic = document.createElement('span')
    ic.className = 'ic'
    ic.textContent = it.icon || '\u{1F5D4}'
    const ti = document.createElement('span')
    ti.className = 'ti'
    ti.textContent = it.title || it.app
    wrap.append(ic, ti)
    return wrap
  }

  function apply(next) {
    state = { windows: next?.windows || [], clock: next?.clock || '' }
    render()
  }

  listen?.('topbar:state', e => apply(e.payload)).catch?.(() => {})
  invoke?.('topbar_ready').catch?.(() => {})
})()
