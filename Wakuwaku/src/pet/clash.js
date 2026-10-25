// The Clash page: a mihomo kernel of the dock's own, with its zashboard in a
// frame (clash.rs is the kernel's business; this file is only the glass, after
// models.js). Above the frame sits a bar of the dock's own: the kernel cannot
// set the Windows system proxy and neither can zashboard, so the switch that
// puts the machine's traffic through the kernel lives here, with the kernel's
// start and stop. Nodes themselves stay on zashboard's own proxy page.
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
  let subDraft = null
  let T = key => key
  let notice = null
  let saving = false
  let savedUrl = ''
  let savedUntil = 0
  let actionBusy = ''
  let action = 0

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
      const switches = document.createElement('div')
      switches.className = 's-clash-switches'
      const sysLabel = document.createElement('label')
      sysLabel.className = 's-clash-switch'
      const sysText = document.createElement('span')
      sysText.textContent = T('s.clashSysproxy')
      const sys = document.createElement('button')
      sys.className = 'sw'
      sys.dataset.clashSysproxy = '1'
      sys.setAttribute('role', 'switch')
      sys.setAttribute('aria-label', T('s.clashSysproxy'))
      sysLabel.appendChild(sysText)
      sysLabel.appendChild(sys)
      const kernelLabel = document.createElement('label')
      kernelLabel.className = 's-clash-switch'
      const kernelText = document.createElement('span')
      kernelText.textContent = T('s.clashKernel')
      const toggle = document.createElement('button')
      toggle.className = 'sw'
      toggle.dataset.clashToggle = '1'
      toggle.setAttribute('role', 'switch')
      toggle.setAttribute('aria-label', T('s.clashKernel'))
      kernelLabel.appendChild(kernelText)
      kernelLabel.appendChild(toggle)
      switches.appendChild(sysLabel)
      switches.appendChild(kernelLabel)
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
      subSave.className = 's-clash-save'
      subSave.dataset.clashSubSave = '1'
      bar.appendChild(what)
      bar.appendChild(grow)
      bar.appendChild(subLabel)
      bar.appendChild(sub)
      bar.appendChild(subSave)
      bar.appendChild(switches)
      stage.appendChild(bar)
      const feedback = document.createElement('div')
      feedback.className = 's-clash-feedback'
      feedback.hidden = true
      feedback.setAttribute('role', 'status')
      feedback.setAttribute('aria-live', 'polite')
      stage.appendChild(feedback)
      stage.addEventListener('click', event => {
        const target = event.target?.closest?.('button') || event.target
        if (target?.dataset?.clashSysproxy) toggleSysproxy()
        else if (target?.dataset?.clashToggle) toggleKernel()
        else if (target?.dataset?.clashSubSave) saveSubscription()
      })
      sub.addEventListener('input', () => {
        subDraft = sub.value
        paintBar()
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
    const running = ready && !actionBusy
    what.textContent = ready
      ? `${T('s.clashKernel')} ${info.version} · ${T('s.clashPort')} ${info.mixed}`
      : state === 'stopped'
        ? T('s.clashStopped')
        : state === 'failed'
          ? T('s.clashFailed')
          : ''
    sys.classList.toggle('on', on)
    sys.setAttribute('aria-checked', String(on))
    sys.setAttribute('aria-label', T('s.clashSysproxy'))
    sys.setAttribute('aria-busy', String(actionBusy === 'sysproxy'))
    sys.disabled = !ready || !!actionBusy || saving
    sys.title = on ? T('s.clashEnabled') : T('s.clashDisabled')
    toggle.classList.toggle('on', running)
    toggle.setAttribute('aria-checked', String(running))
    toggle.setAttribute('aria-label', T('s.clashKernel'))
    toggle.setAttribute('aria-busy', String((!!actionBusy && actionBusy !== 'sysproxy') || state === 'starting'))
    toggle.disabled = !!actionBusy || saving || state === 'starting'
    toggle.title = running ? T('s.clashRunning') : T('s.clashStopped')
    stage.querySelector('.s-clash-switches')?.classList.toggle('busy', !!actionBusy)
    const sub = stage.querySelector('.s-clash-sub')
    const label = stage.querySelector('.s-clash-sub-label')
    const save = stage.querySelector('[data-clash-sub-save]')
    if (sub) {
      if (document.activeElement !== sub) sub.value = subDraft ?? subValue
      sub.setAttribute('aria-label', T('s.clashSubLabel'))
    }
    if (label) label.textContent = T('s.clashSubLabel')
    if (save) {
      const dirty = sub ? sub.value.trim() !== subValue : false
      const saved = !dirty && subValue === savedUrl && Date.now() < savedUntil
      save.textContent = saving ? T('s.clashSaving') : saved ? T('s.clashSaved') : dirty ? T('s.clashSubApply') : T('s.clashSubSave')
      save.disabled = saving || !dirty || !!actionBusy
      save.classList.toggle('saved', saved)
      save.setAttribute('aria-busy', String(saving))
    }
    const feedback = stage.querySelector('.s-clash-feedback')
    if (feedback) {
      feedback.hidden = !notice
      feedback.textContent = notice?.text || ''
      feedback.dataset.kind = notice?.kind || ''
    }
  }

  function showNotice(kind, text) {
    notice = { kind, text: String(text || '') }
    paintBar()
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
      if (state_now?.running === false && state === 'ready') {
        state = 'stopped'
        frame?.remove()
        frame = null
        paint(document.querySelector('#island-settings .s-clash'))
        place()
      }
      try {
        const settings = await invoke('settings_get')
        subValue = String(settings?.settings?.clashSubscription ?? '')
        subDraft = null
        if (!saving && state === 'ready') savedUrl = subValue
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
    if (actionBusy || saving) return
    if (state === 'ready') {
      const attempt = ++action
      actionBusy = 'stopping'
      showNotice('busy', T('s.clashStopping'))
      try {
        await invoke('clash_stop')
        if (attempt !== action) return
        state = 'stopped'
        frame?.remove()
        frame = null
        info = { ...info, version: '', mixed: 0, controller: 0, sysproxy: false }
        notice = null
      } catch (error) {
        if (attempt !== action) return
        showNotice('error', String(error?.message ?? error ?? T('s.clashStopFailed')))
      } finally {
        if (attempt === action) actionBusy = ''
        paintBar()
      }
      paint(document.querySelector('#island-settings .s-clash'))
      place()
    } else if (state === 'stopped' || state === 'failed') {
      retry()
    }
  }

  // 订阅保存：设置快照确实收下 URL 后才重启应用；失败不伪装成成功。
  async function saveSubscription() {
    const sub = stage?.querySelector('.s-clash-sub')
    if (!sub || saving || actionBusy) return
    const url = sub.value.trim()
    subDraft = sub.value
    if (url === subValue) return
    saving = true
    ++generation
    state = 'starting'
    failure = null
    paint(document.querySelector('#island-settings .s-clash'))
    showNotice('busy', T('s.clashSaving'))
    try {
      const settings = await invoke('settings_set', { patch: { clashSubscription: url } })
      if (String(settings?.settings?.clashSubscription ?? '') !== url) throw new Error(T('s.clashSaveRejected'))
      subValue = url
      savedUrl = url
      showNotice('busy', T('s.clashApplying'))
      await invoke('clash_stop')
      state = 'idle'
      await start()
      if (state !== 'ready') throw new Error(failure?.message || T('s.clashFailed'))
      savedUntil = Date.now() + 5000
      showNotice('ok', T('s.clashApplied'))
    } catch (error) {
      if (state !== 'ready' && state !== 'stopped') {
        state = 'failed'
        paint(document.querySelector('#island-settings .s-clash'))
        place()
      }
      showNotice('error', String(error?.message ?? error ?? T('s.clashSaveFailed')))
    } finally {
      saving = false
      paintBar()
    }
  }

  async function toggleSysproxy() {
    if (state !== 'ready' || actionBusy || saving) return
    const attempt = ++action
    const enable = !info.sysproxy
    actionBusy = 'sysproxy'
    paintBar()
    try {
      const done = await invoke('clash_sysproxy', { enable })
      if (attempt !== action) return
      info = { ...info, sysproxy: !!done?.on }
      const from = String(done?.from ?? '')
      const base = T(enable ? 's.clashProxyEnabled' : 's.clashProxyDisabled')
      showNotice('ok', enable && from ? `${T('s.clashTookOver').replace('%s', from)}${base}` : base)
    } catch (error) {
      if (attempt === action) showNotice('error', String(error?.message ?? error ?? ''))
    } finally {
      if (attempt === action) actionBusy = ''
      paintBar()
    }
  }

  function enter(el, translate) {
    if (translate) T = translate
    paint(ensureBox(el))
    paintBar()
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
    ++action
    park()
    frame?.remove()
    frame = null
    state = 'idle'
    failure = null
    actionBusy = ''
    saving = false
    notice = null
  }
  window.Clash = { enter, park, solids, holds, hovered, invalidate }
})()
