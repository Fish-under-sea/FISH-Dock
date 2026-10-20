// The Local models page: Our Free Model's own management console, unchanged, in a frame.
//
// The console is a page of its own, served by a Node service on loopback (the
// service is Rust's business: oflm.rs, oflm_ensure). This file is only the glass
// it is seen through, and it keeps that glass: the frame lives in a layer of its
// own (#models-host, fixed, its own place on screen), and the page moves that
// layer over the page's slot instead of moving the frame into the page. Moving a
// frame between parents makes the browser build it again — the console would come
// back at its front page, logged out. So the frame is made once and never moved:
// another page, or a closed settings, only takes the layer off screen.
//
// Not display:none either: hidden that way WebView2 throttles the frame and the
// page inside it sees itself hidden; off screen it keeps running as if looked at.
(function () {
  const invoke = (command, args) => window.__TAURI__?.core?.invoke?.(command, args)

  // idle → starting → ready, or failed (with the service's own reason).
  let state = 'idle'
  let failure = null
  // The frame and the layer it lives in, made once each.
  let frame = null
  let stage = null
  // The box it was put in last, so a still panel costs no writes.
  let placed = ''
  // The translator, kept for repaints after the page is drawn again.
  let T = key => key

  const escaped = value =>
    String(value ?? '').replace(/[&<>"']/g, c => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' })[c])

  // The frame's own layer: fixed, rounded like the page's own box, and always in
  // the document from the moment there is a frame.
  //
  // The layer is not a child of the island, so WebView2's own mouseenter and
  // mouseleave of her never say the pointer is on the console, and the frame
  // is another origin, so nothing inside it reaches this page either. So the
  // pointer on the layer is told from here (island.js layerHover: the window
  // takes the mouse, as it does on her), and the layer goes to the Rust side
  // as one of her solid rects (solids, island.js sendSolid; island.rs
  // set_editing, pointer.rs look): the window keeps the mouse over it
  // whatever the page says, so a real press, wheel or focus on the console is
  // never let through to the desktop under it (2026-10-20).
  function layer() {
    if (!stage) {
      stage = document.createElement('div')
      stage.id = 'models-host'
      stage.className = 'away'
      document.body.appendChild(stage)
      stage.addEventListener('mouseenter', () => {
        onLayer = true
        window.Isle?.layerHover?.(true)
      })
      stage.addEventListener('mouseleave', event => {
        if (!gone(event)) return
        onLayer = false
        window.Isle?.layerHover?.(false)
      })
      // From the frame to the desktop, no leave of the layer's comes at all:
      // the layer was left when the pointer went into the frame (another
      // origin, filling it), and the frame has the pointer until it is off the
      // console altogether, where the page hears its moves again. A move of
      // the page's own, then, is the pointer off the console.
      document.addEventListener('pointermove', event => {
        if (onLayer && gone(event)) leave()
      }, true)
      window.addEventListener('resize', place)
    }
    return stage
  }

  // On screen over the page's slot now, or off screen (another page, or the
  // settings closed).
  const showing = () => !!stage && !stage.classList.contains('away')

  // The layer, as a part of hers the window has to take the mouse over
  // (island.js sendSolid, island.rs set_editing): nothing while it is away.
  function solids() {
    return showing() ? [stage] : []
  }

  // Whether a node is the layer or in it (the frame): the console is hers, so
  // the pointer on it keeps her open and the window takes the mouse there.
  function holds(node) {
    if (!node || !showing()) return false
    for (let at = node; at; at = at.parentElement || at.parent) if (at === stage) return true
    return false
  }

  // Whether the pointer is on the console now. The layer is over the page's
  // slot, not a child of hers, so the moment the pointer goes on it WebView2
  // reports it as having left the island: island.js's leave asks this before
  // closing her (in the notch it would park the console under the pointer).
  // The page's own :hover is the truth, frame and all (a frame's owner is
  // hovered while the pointer is in the frame), and it goes without an event,
  // so a leave lost on the way out never leaves her thinking it is still
  // there; the enter and leave told here are the fallback (a page without
  // :hover to ask).
  let onLayer = false
  function hovered() {
    if (!showing()) return false
    const hover = stage?.matches?.(':hover')
    return typeof hover === 'boolean' ? hover : onLayer
  }

  // The pointer left the layer. Onto the frame in it, though, WebView2 reports
  // the layer as left too — the frame is another origin, and it fills the
  // layer — so a leave whose point is still on the console is no leave.
  function gone(event) {
    const box = stage?.getBoundingClientRect?.()
    const x = event?.clientX
    const y = event?.clientY
    // No point told (the layer taken off screen): it is gone.
    if (typeof x !== 'number' || typeof y !== 'number' || !box || !(box.width > 0)) return true
    return x < box.left || x > box.left + box.width || y < box.top || y > box.top + box.height
  }

  // Taken off screen under the pointer: no leave of WebView2's own comes for
  // it then, so the pointer is told it is off (the window must not keep the
  // mouse over where the console was — the desktop is there now).
  function leave() {
    if (!onLayer) return
    onLayer = false
    window.Isle?.layerHover?.(false)
  }

  // Where the layer is (or that it went) is the Rust side's business: the
  // island sends every part of hers at once (island.js sendSolid).
  function tell() {
    window.Isle?.sendSolid?.()
  }

  // Where the frame goes: over the page's slot while the page shows, and off
  // screen — at the size it has, so the console does not reflow unseen — while
  // another page shows or the settings are closed.
  function place() {
    if (!stage) return
    const slot = state === 'ready' ? document.querySelector('#island-settings .s-models-slot') : null
    const box = slot ? slot.getBoundingClientRect() : null
    const open = !!box && box.width > 0 && box.height > 0 && window.Settings?.isOpen?.() !== false
    if (!open) {
      if (!stage.classList.contains('away')) {
        stage.style.visibility = ''
        stage.classList.add('away')
        placed = ''
        // Taken off screen under the pointer: no leave of WebView2's own comes
        // for it now, and the window must not keep the mouse over where it was.
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

  // The panel slides in, and the page's own box may settle a beat later: follow
  // it while it moves, and let the loop go once it has stood still for a moment.
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

  // The page's own box: the slot the frame is put over, and the note that says
  // what is going on before there is anything to show.
  function ensureBox(el) {
    let box = el.querySelector(':scope > .s-models')
    if (!box) {
      box = document.createElement('div')
      box.className = 's-models'
      box.innerHTML = '<div class="s-models-slot"></div><div class="s-models-note"></div>'
      el.appendChild(box)
      box.querySelector('.s-models-note').addEventListener('click', event => {
        if (event.target.closest('[data-models-retry]')) retry()
      })
    }
    box.dataset.state = state
    return box
  }

  function paint(box) {
    if (!box) return
    box.dataset.state = state
    const note = box.querySelector('.s-models-note')
    if (state === 'ready') {
      note.hidden = true
      note.innerHTML = ''
      return
    }
    note.hidden = false
    if (state === 'starting' || state === 'idle') {
      note.innerHTML = `<div class="s-models-asking"><span class="s-models-spin"></span>${escaped(T('s.modelsStarting'))}</div>`
      return
    }
    const detail = failure?.message ? `<div class="s-models-detail">${escaped(failure.message)}</div>` : ''
    note.innerHTML = `<div class="s-models-asking"><div class="s-models-title">${escaped(T('s.modelsFailed'))}</div>${detail}<button class="pbtn wide" data-models-retry>${escaped(T('s.modelsRetry'))}</button></div>`
  }

  function makeFrame(url) {
    if (frame && frame.dataset.url === url) return frame
    const next = document.createElement('iframe')
    next.className = 's-models-frame'
    next.title = 'Our Free Model'
    // The bootstrap token rides in the fragment; no-referrer keeps the console's
    // own requests from carrying Wakuwaku's URL anywhere.
    next.setAttribute('referrerpolicy', 'no-referrer')
    // The console copies model ids and keys, and downloads files.
    next.setAttribute('allow', 'clipboard-read; clipboard-write')
    next.dataset.url = url
    next.src = url
    layer().appendChild(next)
    if (frame) frame.remove()
    frame = next
    return next
  }

  async function start() {
    if (state === 'starting') return
    state = 'starting'
    failure = null
    paint(document.querySelector('#island-settings .s-models'))
    try {
      // The page's own origin, so only Wakuwaku may show the console in a frame.
      const ready = await invoke('oflm_ensure', { origin: window.location.origin })
      if (!ready || typeof ready.url !== 'string') throw new Error('oflm_ensure 没有给出地址')
      makeFrame(ready.url)
      state = 'ready'
      paint(document.querySelector('#island-settings .s-models'))
      place()
      startFollowing()
    } catch (error) {
      failure = readFailure(error)
      state = 'failed'
      paint(document.querySelector('#island-settings .s-models'))
      place()
    }
  }

  function retry() {
    state = 'idle'
    start()
  }

  // Shown: put the frame over the page (and start the service the first time).
  function enter(el, translate) {
    if (translate) T = translate
    paint(ensureBox(el))
    if (state === 'idle') start()
    else {
      place()
      startFollowing()
    }
  }

  // Another page, or no page: off screen, at the size it has.
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
    // Gone from under the pointer without a leave of its own: the window must
    // not keep the mouse over where the console was (the desktop is there now).
    leave()
    tell()
  }

  window.Models = { enter, park, solids, holds, hovered }
})()

