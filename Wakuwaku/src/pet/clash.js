// The Clash page: a mihomo kernel of the dock's own, with its zashboard in a
// frame (clash.rs is the kernel's business; this file is only the glass, after
// models.js). Above the frame sits a bar of the dock's own: the kernel cannot
// set the Windows system proxy and neither can zashboard, so the switch that
// puts the machine's traffic through the kernel lives here, with the kernel's
// start and stop.
//
// The layer is made once and parked off screen rather than moved or hidden —
// models.js says why (a moved frame is built again, a hidden one is throttled).
(function () {
  const invoke = (command, args) => window.__TAURI__?.core?.invoke?.(command, args)

  // idle → starting → ready → stopped (by hand), or failed (with the reason).
  let state = 'idle'
  let generation = 0
  let failure = null
  // What the kernel says of itself and of the system proxy, for the bar.
  let info = { version: '', mixed: 0, controller: 0, sysproxy: false }
  let frame = null
  let stage = null
  let placed = ''
  let subValue = ''
  let T = key => key

  const escaped = value =>
    String(value ?? '').replace(/[&<>"']/g, c => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' })[c])

  function layer() {
    if (!stage) {
      stage = document.createElement('div')
      stage.id = 'clash-host'
      stage.className = 'away'
      document.body.appendChild(stage)
      // 动作条：dock 自己的系统代理开关与内核的启停（zashboard 都做不了）。
      // 点击走 stage 上的事件委托：按钮的 innerHTML 每次重画，委托不丢。
      const bar = document.createElement('div')
      bar.className = 's-clash-bar'
      const what = document.createElement('span')
      what.className = 's-clash-what'
      const grow = document.createElement('span')
      grow.className = 'grow'
      const sys = document.createElement('button')
      sys.className = 'pbtn'
      sys.dataset.clashSysproxy = '1'
      const toggle = document.createElement('button')
      toggle.className = 'pbtn'
      toggle.dataset.clashToggle = '1'
      // 订阅 URL（v4 §3.3 补充）：节点走内核的 proxy-provider，由内核下载与
      // 定时更新；面板自己的「导入订阅」是全量替换配置，会把 dock 的控制
      // 字段顶掉，所以订阅从 dock 这条路进。
      const subLabel = document.createElement('span')
      subLabel.className = 's-clash-sub-label'
      const sub = document.createElement('input')
      sub.className = 's-clash-sub'
      sub.type = 'url'
      sub.placeholder = T('s.clashSubHint')
      const subSave = document.createElement('button')
      subSave.className = 'pbtn'
      subSave.dataset.clashSubSave = '1'
      bar.appendChild(what)
      bar.appendChild(grow)
      bar.appendChild(subLabel)
      bar.appendChild(sub)
      bar.appendChild(subSave)
      bar.appendChild(sys)
      bar.appendChild(toggle)
      stage.appendChild(bar)
      stage.addEventListener('click', event => {
        if (event.target?.closest?.('[data-clash-sysproxy]')) toggleSysproxy()
        else if (event.target?.closest?.('[data-clash-toggle]')) toggleKernel()
        else if (event.target?.closest?.('[data-clash-sub-save]')) saveSubscription()
      })
      sub.addEventListener('keydown', event => {
        if (event.key === 'Enter') saveSubscription()
      })
      stage.addEventListener('mouseenter', () => {
        onLayer = true
        window.Isle?.layerHover?.(true)
      })
      stage.addEventListener('mouseleave', event => {
        if (!gone(event)) return
        onLayer = false
        window.Isle?.layerHover?.(false)
      })
      document.addEventListener('pointermove', event => {
        if (onLayer && gone(event)) leave()
      }, true)
      window.addEventListener('resize', place)
    }
    return stage
  }

  const showing = () => !!stage && !stage.classList.contains('away')

  function solids() {
    return showing() ? [stage] : []
  }

  function holds(node) {
    if (!node || !showing()) return false
    for (let at = node; at; at = at.parentElement || at.parent) if (at === stage) return true
    return false
  }

  let onLayer = false
  function hovered() {
    if (!showing()) return false
    const hover = stage?.matches?.(':hover')
    return typeof hover === 'boolean' ? hover : onLayer
  }

  function gone(event) {
    const box = stage?.getBoundingClientRect?.()
    const x = event?.clientX
    const y = event?.clientY
    if (typeof x !== 'number' || typeof y !== 'number' || !box || !(box.width > 0)) return true
    return x < box.left || x > box.left + box.width || y < box.top || y > box.top + box.height
  }

  function leave() {
    if (!onLayer) return
    onLayer = false
    window.Isle?.layerHover?.(false)
  }

  function tell() {
    window.Isle?.sendSolid?.()
  }

  function place() {
    if (!stage) return
    const slot = state === 'ready' ? document.querySelector('#island-settings .s-clash-slot') : null
    const box = slot ? slot.getBoundingClientRect() : null
    const open = !!box && box.width > 0 && box.height > 0 && window.Settings?.isOpen?.() !== false
    if (!open) {
      if (!stage.classList.contains('away')) {
        stage.style.visibility = ''
        stage.classList.add('away')
        placed = ''
        leave()
        tell()
      }
      return
    }
    const next = [box.left, box.top, box.width, box.height].map(Math.round).join()
    if (next === placed) return
    placed = next
    stage.classList.remove('away')
    stage.style.left = `${Math.round(box.left)}px`
    stage.style.top = `${Math.round(box.top)}px`
    stage.style.width = `${Math.round(box.width)}px`
    stage.style.height = `${Math.round(box.height)}px`
    tell()
  }

  let following = 0
  let still = 0
  function follow() {
    if (!following) return
    const before = placed
    place()
    still = placed === before ? still + 1 : 0
    following = still > 40 ? 0 : requestAnimationFrame(follow)
  }
  function startFollowing() {
    still = 0
    if (following) return
    following = requestAnimationFrame(follow)
  }

  function readFailure(error) {
    const value = error && typeof error === 'object' ? error : { message: String(error ?? '') }
    return { code: String(value.code ?? 'START_FAILED'), message: String(value.message ?? '') }
  }

  function ensureBox(el) {
    let box = el.querySelector(':scope > .s-clash')
    if (!box) {
      box = document.createElement('div')
      box.className = 's-clash'
      const slot = document.createElement('div')
      slot.className = 's-clash-slot'
      const note = document.createElement('div')
      note.className = 's-clash-note'
      box.appendChild(slot)
      box.appendChild(note)
      el.appendChild(box)
      note.addEventListener('click', event => {
        if (event.target.closest('[data-clash-retry]')) retry()
      })
    }
    box.dataset.state = state
    return box
  }

  // The dock's own bar: what the kernel is, and the two switches zashboard
  // cannot do — the system proxy and the kernel's life.
  function paintBar() {
    if (!stage) return
    const what = stage.querySelector('.s-clash-what')
    const sys = stage.querySelector('[data-clash-sysproxy]')
    const toggle = stage.querySelector('[data-clash-toggle]')
    if (!what || !sys || !toggle) return
    const on = !!info.sysproxy
    const ready = state === 'ready'
    what.textContent = ready
      ? `${T('s.clashKernel')} ${info.version} · ${T('s.clashPort')} ${info.mixed}`
      : state === 'stopped'
        ? T('s.clashStopped')
        : ''
    sys.textContent = T('s.clashSysproxy') + (on ? ' ·' : '')
    sys.disabled = !ready
    toggle.textContent = ready ? T('s.clashStop') : T('s.clashStart')
    toggle.disabled = state === 'starting'
    const sub = stage.querySelector('.s-clash-sub')
    const label = stage.querySelector('.s-clash-sub-label')
    const save = stage.querySelector('[data-clash-sub-save]')
    if (sub && document.activeElement !== sub) sub.value = subValue
    if (label) label.textContent = T('s.clashSubLabel')
    if (save) save.textContent = T('s.clashSubSave')
  }

  function paint(box) {
    if (!box) return
    box.dataset.state = state
    const note = box.querySelector('.s-clash-note')
    if (state === 'ready') {
      note.hidden = true
      note.innerHTML = ''
      return
    }
    note.hidden = false
    if (state === 'starting' || state === 'idle') {
      note.innerHTML = `<div class="s-clash-asking"><span class="s-models-spin"></span>${escaped(T('s.clashStarting'))}</div>`
      return
    }
    if (state === 'stopped') {
      note.innerHTML = `<div class="s-clash-asking"><div class="s-clash-title">${escaped(T('s.clashStopped'))}</div><button class="pbtn wide" data-clash-retry>${escaped(T('s.clashStart'))}</button></div>`
      return
    }
    const detail = failure?.message ? `<div class="s-clash-detail">${escaped(failure.message)}</div>` : ''
    note.innerHTML = `<div class="s-clash-asking"><div class="s-clash-title">${escaped(T('s.clashFailed'))}</div>${detail}<button class="pbtn wide" data-clash-retry>${escaped(T('s.clashRetry'))}</button></div>`
  }

  function makeFrame(url) {
    if (frame && frame.dataset.url === url) return frame
    const next = document.createElement('iframe')
    next.className = 's-clash-frame'
    next.title = 'Clash'
    // 面板与控制器同源（mihomo 的 external-ui），secret 走参数；不让面板
    // 的请求把 dock 的地址带出去。
    next.setAttribute('referrerpolicy', 'no-referrer')
    next.dataset.url = url
    next.src = url
    layer().appendChild(next)
    if (frame) frame.remove()
    frame = next
    paintBar()
    return next
  }

  async function refreshState() {
    try {
      const state_now = await invoke('clash_state')
      info = {
        version: String(state_now?.version ?? ''),
        mixed: Number(state_now?.mixed ?? 0),
        controller: Number(state_now?.controller ?? 0),
        sysproxy: !!state_now?.sysproxy,
      }
      try {
        const settings = await invoke('settings_get')
        subValue = String(settings?.settings?.clashSubscription ?? '')
      } catch {}
    } catch {
      // 内核不在跑：栏里少些字即可，不作为失败。
    }
    paintBar()
  }

  async function start() {
    if (state === 'starting') return
    state = 'starting'
    const attempt = ++generation
    failure = null
    paint(document.querySelector('#island-settings .s-clash'))
    try {
      const ready = await invoke('clash_ensure', { origin: window.location.origin })
      if (attempt !== generation) return
      if (!ready || typeof ready.url !== 'string') throw new Error('clash_ensure 没有给出地址')
      info = { version: String(ready.version ?? ''), mixed: Number(ready.mixed ?? 0), controller: Number(ready.controller ?? 0), sysproxy: info.sysproxy }
      makeFrame(ready.url)
      state = 'ready'
      paint(document.querySelector('#island-settings .s-clash'))
      paintBar()
      place()
      startFollowing()
    } catch (error) {
      if (attempt !== generation) return
      failure = readFailure(error)
      state = 'failed'
      paint(document.querySelector('#island-settings .s-clash'))
      place()
    }
  }

  function retry() {
    state = 'idle'
    start()
  }

  async function toggleKernel() {
    if (state === 'ready') {
      state = 'stopped'
      paint(document.querySelector('#island-settings .s-clash'))
      paintBar()
      try {
        await invoke('clash_stop')
      } catch {
        // 停止失败极少见（进程已死即目的已达）；下次启动会重写一切。
      }
      frame?.remove()
      frame = null
      info = { ...info, version: '', mixed: 0, controller: 0 }
      paintBar()
    } else if (state === 'stopped' || state === 'failed') {
      retry()
    }
  }

  // 订阅保存：进设置，然后重启内核应用（config.yaml 由 clash_ensure 按
  // 新订阅重写；provider 的节点由内核自己下载）。空值 = 清除订阅。
  async function saveSubscription() {
    const sub = stage?.querySelector('.s-clash-sub')
    if (!sub) return
    const url = sub.value.trim()
    try {
      await invoke('settings_set', { patch: { clashSubscription: url } })
      subValue = url
      await invoke('clash_stop')
      state = 'idle'
      start()
    } catch (error) {
      failure = readFailure(error)
      state = 'failed'
      paint(document.querySelector('#island-settings .s-clash'))
    }
  }

  async function toggleSysproxy() {
    if (state !== 'ready') return
    try {
      const done = await invoke('clash_sysproxy', { enable: !info.sysproxy })
      info = { ...info, sysproxy: !!done?.on }
    } catch (error) {
      // 没在跑之类：把原因放进失败位，面板不受影响。
      failure = readFailure(error)
    }
    paintBar()
  }

  function enter(el, translate) {
    if (translate) T = translate
    paint(ensureBox(el))
    if (state === 'ready') paintBar()
    if (state === 'idle') start()
    else {
      place()
      startFollowing()
    }
    if (state === 'ready') refreshState()
  }

  function park() {
    following = 0
    if (!stage) return
    const box = frame ? frame.getBoundingClientRect() : null
    if (box && box.width > 0 && box.height > 0) {
      stage.style.width = `${Math.round(box.width)}px`
      stage.style.height = `${Math.round(box.height)}px`
    }
    stage.classList.add('away')
    placed = ''
    leave()
    tell()
  }

  function invalidate() {
    ++generation
    park()
    frame?.remove()
    frame = null
    state = 'idle'
    failure = null
  }
  window.Clash = { enter, park, solids, holds, hovered, invalidate }
})()
