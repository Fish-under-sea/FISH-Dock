// 标签顺序使用稳定 ID，停用页的历史位置保留在设置中。
;(function () {
  const CORE = ['now','agents','look','widgets','mail']
  const valid = id => typeof id === 'string' && (CORE.includes(id) || /^[a-z0-9]+(?:-[a-z0-9]+)*\.[a-z0-9]+(?:-[a-z0-9]+)*\/[a-z0-9-]{1,64}$/.test(id)) && id.length <= 160
  const clean = values => [...new Set((Array.isArray(values) ? values : []).filter(valid))].slice(0,128)
  function order(saved, pages) {
    const available = new Set([...CORE, ...clean(pages)])
    return [...new Set([...clean(saved), ...available])].filter(id => available.has(id))
  }
  function move(saved, pages, id, offset) {
    const visible = order(saved,pages), from = visible.indexOf(id), to = from + offset
    if (from < 0 || to < 0 || to >= visible.length || ![-1,1].includes(offset)) return clean(saved)
    const all = clean([...clean(saved), ...visible])
    const a = all.indexOf(id), b = all.indexOf(visible[to]); [all[a],all[b]] = [all[b],all[a]]
    return all
  }
  function nearest(current, previous, next) {
    if (next.includes(current)) return current
    const index = previous.indexOf(current)
    for (let distance = 1; distance <= previous.length; distance++) {
      if (next.includes(previous[index + distance])) return previous[index + distance]
      if (next.includes(previous[index - distance])) return previous[index - distance]
    }
    return next[0] || 'now'
  }
  function bindScroll(element) {
    let pointer = null, suppressClick = false
    element.addEventListener('wheel', event => {
      if (event.ctrlKey || Math.abs(event.deltaX) >= Math.abs(event.deltaY) || element.scrollWidth <= element.clientWidth) return
      const before = element.scrollLeft
      element.scrollLeft += event.deltaY * (event.deltaMode === 1 ? 16 : event.deltaMode === 2 ? element.clientWidth : 1)
      if (element.scrollLeft !== before) event.preventDefault()
    }, { passive: false })
    element.addEventListener('pointerdown', event => {
      if (event.button !== 0 || event.isPrimary === false) return
      suppressClick = false
      pointer = { id: event.pointerId, x: event.clientX, y: event.clientY, scroll: element.scrollLeft, dragging: false }
    })
    element.addEventListener('pointermove', event => {
      if (!pointer || pointer.id !== event.pointerId || element.scrollWidth <= element.clientWidth) return
      const dx = event.clientX - pointer.x
      if (!pointer.dragging) {
        if (Math.abs(dx) < 6 || Math.abs(dx) < Math.abs(event.clientY - pointer.y)) return
        pointer.dragging = true
        element.setPointerCapture(event.pointerId)
        element.classList.add('dragging')
      }
      element.scrollLeft = pointer.scroll - dx
      event.preventDefault()
    })
    const finish = event => {
      if (!pointer || pointer.id !== event.pointerId) return
      suppressClick = pointer.dragging
      pointer = null
      element.classList.remove('dragging')
      if (element.hasPointerCapture(event.pointerId)) element.releasePointerCapture(event.pointerId)
    }
    element.addEventListener('pointerup', finish)
    element.addEventListener('pointercancel', finish)
    element.addEventListener('lostpointercapture', finish)
    element.addEventListener('click', event => {
      if (suppressClick && event.detail > 0) { suppressClick = false; event.preventDefault(); event.stopImmediatePropagation() }
    }, true)
  }
  window.ExtensionTabs = { CORE, order, move, nearest, valid, bindScroll }
})()
