// 安装/更新的进度按钮。
//
// 形态取自 https://codepen.io/faustdp/pen/yXypKM（faustdp）：
//   药丸 → 箭头下沉、标签缩小 → 收成右侧小圆 → 圆环按进度画、小点绕圈
//   → 完成变绿、弹回药丸、标签弹入。
//
// 前几版一直出问题的原因是「viewBox 与显示尺寸不一致」：pen 用 100×50 的 viewBox
// 显示成 620×310，我照搬坐标却塞进 96×24 的区域，缩放系数一算错，图元就散架、
// 还会漏到卡片外面。所以这里把**坐标系定为 96×24，与 CSS 尺寸 1:1**：所有图元按像素写，
// 不依赖 preserveAspectRatio 或任何缩放。
(function () {
  const W = 96
  const H = 24
  const PAD = 12          // 闭合态圆心 x
  const R = 8.5           // 圆半径
  const GAP = 1.2         // 圆环与圆之间的间隙
  const RING_R = R + GAP
  const RING_C = 2 * Math.PI * RING_R

  const SVG = `
<svg class="pb-svg" width="${W}" height="${H}" viewBox="0 0 ${W} ${H}" fill="none" aria-hidden="true">
  <rect class="pb-track" x="0.5" y="0.5" width="${W - 1}" height="${H - 1}" rx="${(H - 1) / 2}"></rect>
  <circle class="pb-ring" cx="${PAD}" cy="${H / 2}" r="${RING_R}" stroke-dasharray="${RING_C.toFixed(2)}" stroke-dashoffset="${RING_C.toFixed(2)}"></circle>
  <path class="pb-arrow" d="M${PAD} ${H / 2 - 3.4} v6.8 M${PAD - 2.7} ${H / 2 + 0.2} l2.7 2.7 2.7 -2.7"></path>
  <circle class="pb-dot" cx="${PAD}" cy="${H / 2 - RING_R}" r="0"></circle>
  <text class="pb-label" x="${PAD + 9}" y="${H / 2 + 3.4}"></text>
</svg>`

  function create(host, options) {
    const opts = options || {}
    host.innerHTML = SVG
    const svg = host.querySelector('svg')
    const track = svg.querySelector('.pb-track')
    const ring = svg.querySelector('.pb-ring')
    const arrow = svg.querySelector('.pb-arrow')
    const dot = svg.querySelector('.pb-dot')
    const label = svg.querySelector('.pb-label')
    let running = false
    let done = false

    label.textContent = opts.label || '安装'
    ring.style.opacity = '0'

    // 收拢后的圆心：药丸缩成右侧的小圆（宽高都是 H-1）
    const collapsedCx = W - H + (H - 1) / 2
    const cy = H / 2

    // ① 收拢：药丸缩成右侧小圆，箭头与标签淡出，圆环出现
    function squeeze() {
      if (running) return
      running = true
      host.classList.add('pb-running')
      track.setAttribute('x', String(W - H))
      track.setAttribute('width', String(H - 1))
      ring.setAttribute('cx', String(collapsedCx))
      arrow.style.opacity = '0'
      label.style.opacity = '0'
      dot.setAttribute('r', '1.4')
      ring.style.opacity = '1'
    }

    // ② 进度：圆环 dashoffset + 小点绕圈
    function setProgress(pct) {
      const p = Math.max(0, Math.min(100, Number(pct) || 0))
      if (!running) squeeze()
      ring.setAttribute('stroke-dashoffset', (RING_C * (1 - p / 100)).toFixed(2))
      const ang = (-90 + p * 3.6) * Math.PI / 180
      dot.setAttribute('cx', (collapsedCx + RING_R * Math.cos(ang)).toFixed(2))
      dot.setAttribute('cy', (cy + RING_R * Math.sin(ang)).toFixed(2))
      dot.setAttribute('r', (1.2 + 1.2 * (p / 100)).toFixed(2))
      if (p >= 100) complete()
      return p
    }

    // ③ 完成：描边转绿（CSS 负责颜色）、圆环与小点淡出、药丸弹回、标签弹入
    function complete() {
      if (done) return
      done = true
      host.classList.add('pb-done')
      track.setAttribute('x', '0.5')
      track.setAttribute('width', String(W - 1))
      ring.style.opacity = '0'
      dot.style.opacity = '0'
      label.textContent = opts.doneLabel || '完成'
      label.style.opacity = '1'
    }

    function reset() {
      running = false
      done = false
      host.classList.remove('pb-running', 'pb-done')
      track.setAttribute('x', '0.5')
      track.setAttribute('width', String(W - 1))
      ring.setAttribute('stroke-dashoffset', RING_C.toFixed(2))
      ring.style.opacity = '0'
      arrow.style.opacity = '1'
      dot.setAttribute('r', '0')
      dot.style.opacity = '1'
      label.textContent = opts.label || '安装'
      label.style.opacity = '1'
    }

    svg.addEventListener('click', () => { if (opts.onCancel) opts.onCancel() })
    return { setProgress, complete, reset, svg }
  }

  window.ProgressButton = { create, W, H }
})()