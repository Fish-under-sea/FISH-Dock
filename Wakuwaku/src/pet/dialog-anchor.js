// 确认弹窗的位置（t2）。
//
// 根因：`<dialog>` 的 `showModal()` 把它提升到 top layer，浏览器给的是
// `position: fixed; inset: 0; margin: auto; width/height: fit-content` ——
// 于是它永远落在**视口正中**。岛的窗口铺满整个工作区宽度（island.rs
// size_for），所以视口正中就是**屏幕正中**，与岛在哪一侧无关。这就是
// 「弹窗不在 dock 内部、锁在屏幕中央」两次没改掉的来源：改的是弹窗的
// 宽高与遮罩（settings.css 里的 360px / 70vh / transparent），没有一处
// 碰它的位置，位置一直是 UA 的 auto 居中。
//
// 这里按岛的矩形摆：横向贴岛的锚点一侧（左/中/右），纵向在岛内居中；
// 宽高都夹在岛内并留出内衬，长内容在弹窗里滚动。top layer 里 fixed 的
// 包含块是视口，所以入参用岛的 getBoundingClientRect 与视口尺寸；显示器
// 与 DIP 缩放都随岛窗口走，不必另算（rect 是 CSS px，视口也是 CSS px）。
//
// 只放算术，方便单测（island 在左/中/右、屏幕边缘、多显示器的负坐标、
// DIP 缩放下视口的 CSS px）。
;(function (root) {
  // 弹窗与岛边之间留的内衬（与弹窗自身 16px padding 同量级）。
  const INSET = 12
  // 弹窗宽度上限：桌面端读起来舒适的那一档（settings.css 的 360 同值）。
  const MAX_WIDTH = 360
  // 高度上限占视口的比例（settings.css 的 70vh 同值），再被岛高夹一次。
  const MAX_HEIGHT_RATIO = 0.7
  // 岛太矮时（设置还没长开）允许弹窗至少这么高：此时它从岛顶向下延伸，
  // 仍与岛同一侧对齐，但不缩到读不出字。
  const MIN_HEIGHT = 220

  // 弹窗在岛内的宽高上限：岛比上限窄/矮时以岛为准，再被视口夹一次。
  function limits(island, viewport, inset = INSET) {
    const across = Math.min(MAX_WIDTH, island.width - 2 * inset, viewport.width - 2 * inset)
    const down = Math.min(viewport.height * MAX_HEIGHT_RATIO, island.height - 2 * inset, viewport.height - 2 * inset)
    return { width: Math.max(1, across), height: Math.max(1, down) }
  }

  // 弹窗的高度：内容要多少给多少，但不低于 MIN_HEIGHT（否则字读不出来），
  // 也不超过岛与视口给的上限。岛矮到连 MIN_HEIGHT 都放不下时，取视口允许
  // 的那一档（此时它从岛顶向下延伸，见 place）。
  function heightFor(room, viewport, measured, inset = INSET) {
    const wanted = typeof measured === 'number' && measured > 0 ? measured : room.height
    const floor = Math.min(MIN_HEIGHT, viewport.height - 2 * inset)
    return Math.max(1, Math.min(Math.max(wanted, floor), room.height))
  }

  // 弹窗的框（视口坐标）：横向贴岛的锚点一侧，纵向在岛内居中；
  // 一律夹在视口内，免得岛被摆到屏幕边上时弹窗跑出去。
  // height: 弹窗实际需要的高度（没量过就按上限）。
  function place({ island, viewport, anchor = 'center', inset = INSET, height }) {
    const room = limits(island, viewport, inset)
    const tall = heightFor(room, viewport, height, inset)
    // right/bottom 自己算：只有 left/top/width/height 的对象（手造的、或某个
    // 只给了这几项的调用方）也摆得对，不会算出 NaN。
    const right = typeof island.right === 'number' ? island.right : island.left + island.width
    const wanted = anchor === 'left' ? island.left + inset : anchor === 'right' ? right - inset - room.width : island.left + (island.width - room.width) / 2
    const left = Math.max(inset, Math.min(wanted, viewport.width - room.width - inset))
    // 岛内居中；放不进岛（岛矮）就贴着岛顶向下延伸，至少和岛同一侧对齐。
    const centred = island.top + (island.height - tall) / 2
    const top = Math.max(inset, Math.min(tall > island.height - 2 * inset ? island.top + inset : centred, viewport.height - tall - inset))
    return { left, top, width: room.width, height: tall }
  }

  // 岛在窗口里的哪一侧（island.js placeAnchor 的 body 类）：确认弹窗跟它。
  function anchorOf(classList) {
    const has = name => !!classList?.contains?.(name)
    if (has('isl-at-left')) return 'left'
    if (has('isl-at-right')) return 'right'
    return 'center'
  }

  // 把框写进 dialog（top layer 里 fixed，所以 left/top 就是视口坐标）。
  // 宽度先定、量一次真实高度、再摆纵向位置：内容多寡决定它多高。
  // 量到的是被 max-height 夹过的高度，也就是「最多要这么高」，正好是上限。
  function apply(dialog, island, viewport, anchor = 'center') {
    const room = limits(island, viewport)
    dialog.style.position = 'fixed'
    // UA 的 margin: auto 就是居中的那一步，必须显式清掉。
    dialog.style.margin = '0'
    dialog.style.width = `${room.width}px`
    dialog.style.maxHeight = `${room.height}px`
    // 刚写完宽度就取矩形：getBoundingClientRect 会先强制一次布局，
    // 拿到的就是这次宽度下的真实高度，不是上一次的。
    const measured = dialog.getBoundingClientRect?.().height
    const rect = place({ island, viewport, anchor, height: measured })
    dialog.style.left = `${rect.left}px`
    dialog.style.top = `${rect.top}px`
    dialog.style.maxHeight = `${rect.height}px`
    return rect
  }

  // 岛开着弹窗时自己还在动（长高、锚点换侧、窗口被摆到别的显示器）：弹窗跟着
  // 重摆。只在弹窗开着的时候观察，关掉就停——不常驻一个 ResizeObserver。
  // 岛没真的变形就不重摆：回调里读布局会再触发一次，白算。
  // 三种来源各看一路，缺一路就会「攒丢」：
  //   • ResizeObserver：岛变大变小（长高）。
  //   • body 的 class：只换锚点侧（isl-at-left/right），尺寸不变时 RO 不报。
  //   • window resize：视口变了（窗口被摆到别的显示器、DIP 缩放），上限要重算。
  // 「隐藏期 IsVisible=false 让 RO 停摆、唤出后补一次」这一路由调用方接：
  // settings.js 在 island:look-here / settings:changed / pet:update 时再 seat 一次。
  // 返回 stop()（也由 dialog 的 close 事件自动调）。
  function watch(dialog, island, viewportOf, anchorOfNow) {
    let observer = null
    let nodes = null
    let seen = ''
    const reseat = () => {
      if (!dialog.open) return stop()
      const box = island.getBoundingClientRect()
      const viewport = viewportOf()
      const seenNow = [box.left, box.top, box.width, box.height, viewport.width, viewport.height].map(Math.round).join()
      if (seenNow === seen) return
      seen = seenNow
      apply(dialog, box, viewport, anchorOfNow())
    }
    const onResize = () => reseat()
    const stop = () => {
      observer?.disconnect()
      nodes?.disconnect()
      window.removeEventListener('resize', onResize)
      observer = null
      nodes = null
      seen = ''
    }
    if (!dialog || !island) return stop
    observer = new ResizeObserver(reseat)
    observer.observe(island)
    if (typeof MutationObserver === 'function') {
      nodes = new MutationObserver(reseat)
      nodes.observe(document.body, { attributes: true, attributeFilter: ['class'] })
    }
    window.addEventListener('resize', onResize)
    dialog.addEventListener('close', stop, { once: true })
    return stop
  }

  const api = { INSET, MAX_WIDTH, MAX_HEIGHT_RATIO, MIN_HEIGHT, limits, heightFor, place, anchorOf, apply, watch }

  if (typeof module !== 'undefined' && module.exports) {
    module.exports = api
  } else {
    root.DialogAnchor = api
  }
})(this)
