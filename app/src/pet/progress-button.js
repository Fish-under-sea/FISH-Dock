// 安装/更新的进度按钮——原样移植 https://codepen.io/faustdp/pen/yXypKM 的 SVG 与时间轴。
//
// 为什么单独一个文件：笔者的实现是 GSAP Timeline + Snap 路径解析，本仓不引第三方库，
// 所以这里用浏览器原生能力等价实现：
//   · SVG 路径变形（btn / arrow 的 d 变化）→ SMIL <animate attributeName="d">，同结构路径可用
//   · 位移、缩放、透明度、颜色 → Web Animations API（element.animate）
//   · 圆环进度（subCircle 的 drawSVG）→ stroke-dasharray / dashoffset，由进度直接驱动
//   · 绕圈的小点（dot 沿 bezier 走 + r 变大）→ 按角度算 cx/cy，无需路径库
//
// 对外的三个动作与 pen 一致：open（点击开始）→ 进度推进 → complete（收尾）。
// 与笔者的差别只有一处：笔者的 downloadTime 是 Math.random()，这里用真实进度。
(function () {
  const SVG = `
<svg viewBox="0 0 100 50" class="pb-svg" fill="none" aria-hidden="true">
  <circle cx="20" cy="35" r="8.5" stroke="currentColor" stroke-width="1" class="mainCircle"></circle>
  <circle cx="20" cy="35" r="8.05" stroke="currentColor" stroke-width=".9" fill="url(#pb-gradient)" class="mainCircleFill"></circle>
  <rect x="17.5" y="32.5" width="5" height="5" stroke="none" fill="currentColor" class="rect"></rect>
  <path d="M20,39 l3.5,-3.5 l0,0 M20,39 l-3.5,-3.5 l0,0 M20,39 l0,-7.5" stroke="currentColor" stroke-linecap="round" stroke-width=".8" class="arrow"></path>
  <text x="55" y="36.5" fill="currentColor" text-anchor="middle" font-size="5.5" letter-spacing=".2" class="text"></text>
  <path d="M50,25 h30 a10,10 0 0,1 10,10 a10,10 0 0,1 -10,10 s-30,0 -60,0 a10,10 0 0,1 -10,-10 a10,10 0 0,1 10,-10 h30" stroke="currentColor" stroke-width=".7" fill="transparent" class="btn"></path>
  <circle cx="20" cy="35" r="7.9" fill="currentColor" fill-opacity="0" stroke="currentColor" stroke-width="1.6" stroke-opacity="0" class="subCircle"></circle>
  <circle cx="50" cy="26" r="0" fill="currentColor" class="dot"></circle>
  <linearGradient id="pb-gradient" x1="0%" y1="0%" x2="0%" y2="100%">
    <stop offset="98%" class="gradient" stop-color="transparent"></stop>
    <stop offset="98%" class="gradient" stop-color="#02fc86"></stop>
  </linearGradient>
</svg>`

  // pen 里的三处路径（按钮药丸的三种形态、箭头的两种形态）
  const BTN_WIDE = 'M50,25 h30 a10,10 0 0,1 10,10 a10,10 0 0,1 -10,10 s-30,0 -60,0 a10,10 0 0,1 -10,-10 a10,10 0 0,1 10,-10 h30'
  const BTN_TIGHT = 'M50,25 h0 a10,10 0 0,1 10,10 a10,10 0 0,1 -10,10 s0,0 0,0  a10,10 0 0,1 -10,-10 a10,10 0 0,1 10,-10 h0'
  const BTN_BACK = 'M50,25 h20 a10,10 0 0,1 10,10 a10,10 0 0,1 -10,10 s-20,0 -40,0 a10,10 0 0,1 -10,-10 a10,10 0 0,1 10,-10 h20'
  const ARROW_DOWN = 'M20,39 l3.5,-3.5 l0,0 M20,39 l-3.5,-3.5 l0,0 M20,39 l0,-7.5'
  const ARROW_CROSS = 'M20,39 l3.5,-3.5 l-3.5,-3.5 M20,39 l-3.5,-3.5 l3.5,-3.5 M20,39 l0,0'

  const CIRC = 2 * Math.PI * 9.07      // pen 里 subCircle 收尾半径 9.07
  const EASE_OUT = 'cubic-bezier(.33,.9,.4,1)'
  const EASE_SNAP = 'cubic-bezier(.22,1.1,.3,1)'
  const EASE_BACK = 'cubic-bezier(.34,1.56,.64,1)'

  // SMIL：给某个元素加一条一次性的 d 变形，返回启动函数
  function morph(el, to, dur) {
    const old = el.querySelector('animate')
    if (old) old.remove()
    const a = document.createElementNS('http://www.w3.org/2000/svg', 'animate')
    a.setAttribute('attributeName', 'd')
    a.setAttribute('to', to)
    a.setAttribute('dur', dur + 's')
    a.setAttribute('fill', 'freeze')
    a.setAttribute('begin', 'indefinite')
    el.appendChild(a)
    return () => { try { a.beginElement() } catch (_) {} }
  }

  function create(host, options) {
    const opts = options || {}
    host.innerHTML = SVG
    const svg = host.querySelector('svg')
    const $ = sel => svg.querySelector(sel)
    const nodes = {
      main: $('.mainCircle'), mainFill: $('.mainCircleFill'), rect: $('.rect'),
      arrow: $('.arrow'), text: $('.text'), btn: $('.btn'),
      sub: $('.subCircle'), dot: $('.dot'),
    }
    let opened = 0          // 0 = 药丸，1 = 小圆
    let raf = 0
    let closed = false

    // 初始态：与 pen 的 HTML 一致（小方块先转 45°）
    // SVG 元素用 CSS transform 时必须显式给 transform-box，否则按 view-box 原点旋转，
    // 方块会飘到圆外（用户 2026-11-01 截图里那颗独立的小方块）。
    for (const el of [nodes.rect, nodes.main, nodes.mainFill, nodes.sub, nodes.arrow]) {
      el.style.transformBox = 'fill-box'
      el.style.transformOrigin = 'center'
    }
    nodes.rect.style.transform = 'rotate(45deg)'
    nodes.sub.style.strokeDasharray = `${CIRC.toFixed(2)} ${CIRC.toFixed(2)}`
    nodes.sub.style.strokeDashoffset = CIRC.toFixed(2)
    nodes.text.textContent = opts.label || 'install'

    // pen 第 27 行与第 32 行：主圆、填充、方块、箭头 **以及 subCircle** 都右移 30
    const group = [nodes.main, nodes.mainFill, nodes.rect, nodes.arrow, nodes.sub]

    function squeeze() {
      if (opened) return
      opened = 1
      host.classList.add('pb-open')
      // ① 箭头下沉 ② 标签缩小淡出 ③ 药丸收成小圆 ④ 圆环/方块一起右移
      nodes.arrow.animate([{ transform: 'translateY(0)' }, { transform: 'translateY(2.5px)' }],
        { duration: 350, easing: EASE_SNAP, fill: 'forwards' })
      nodes.text.animate([{ transform: 'scale(1)', opacity: 1 }, { transform: 'scale(.77)', opacity: 0 }],
        { duration: 300, easing: EASE_SNAP, fill: 'forwards' })
      morph(nodes.btn, BTN_TIGHT, 0.7)()
      nodes.arrow.animate([{ opacity: 1 }, { opacity: 0 }], { duration: 300, delay: 200, fill: 'forwards' })
      for (const el of group) {
        el.animate([{ transform: 'translateX(0)' }, { transform: 'translateX(30px)' }],
          { duration: 700, easing: EASE_OUT, fill: 'forwards' })
      }
      nodes.rect.animate([{ transform: 'rotate(45deg)', fillOpacity: 1 }, { transform: 'rotate(270deg)', fillOpacity: 1 }],
        { duration: 700, easing: EASE_OUT, fill: 'forwards' })
      // 收成小圆后把圆环交给进度驱动
      nodes.sub.setAttribute('r', '9.07')   // pen 第 32 行：套在圆外面的那一圈
      nodes.sub.style.opacity = '1'
    }

    // 进度：0..100。圆环与小点都按它画（pen 里是 drawSVG + dot 沿圆周移动）
    function setProgress(pct) {
      if (!opened) squeeze()
      const p = Math.max(0, Math.min(100, pct))
      nodes.sub.style.strokeDashoffset = (CIRC * (1 - p / 100)).toFixed(2)
      if (opened && !raf) {
        // 小点绕圈：角度从 -90° 起，随进度转一圈；r 从 1.2 长到 2.7
        const spin = () => {
          const ang = (-90 + p * 3.6) * Math.PI / 180
          nodes.dot.setAttribute('cx', (50 + 9.07 * Math.cos(ang)).toFixed(2))
          nodes.dot.setAttribute('cy', (35 + 9.07 * Math.sin(ang)).toFixed(2))
          nodes.dot.setAttribute('r', (1.2 + 1.5 * (p / 100)).toFixed(2))
          nodes.dot.style.opacity = p > 0 ? '1' : '0'
          raf = 0
        }
        spin()
      }
      // 完成：变绿 + 小点弹跳 + 药丸拉回 + 标签弹入
      if (p >= 100) complete()
    }

    function complete() {
      if (closed) return
      closed = true
      host.classList.add('pb-done')
      nodes.btn.style.stroke = '#02fc86'
      nodes.dot.animate([
        { transform: 'translateY(0) scale(1)', opacity: 1 },
        { transform: 'translateY(-22px) scale(.5)', opacity: 1, offset: 0.4 },
        { transform: 'translateY(-22px) scaleY(.6)', opacity: 1, offset: 0.6 },
        { transform: 'translateY(22px) scale(.4)', opacity: 0 },
      ], { duration: 800, easing: 'ease-out', fill: 'forwards' })
      for (const el of [nodes.main, nodes.mainFill, nodes.sub, nodes.rect, nodes.arrow]) {
        el.animate([{ opacity: 1 }, { opacity: 0 }], { duration: 330, delay: 160, fill: 'forwards' })
      }
      morph(nodes.btn, BTN_BACK, 0.4)()
      nodes.text.textContent = opts.doneLabel || 'done'
      nodes.text.animate([
        { opacity: 0, transform: 'translateY(7px) scale(.9)' },
        { opacity: 1, transform: 'translateY(0) scale(1)' },
      ], { duration: 450, delay: 360, easing: EASE_BACK, fill: 'forwards' })
    }

    function reset() {
      closed = false
      opened = 0
      host.classList.remove('pb-open', 'pb-done')
      nodes.btn.style.stroke = ''
      morph(nodes.btn, BTN_WIDE, 0.001)()
      nodes.text.textContent = opts.label || 'install'
      nodes.text.style.opacity = '1'
      nodes.text.style.transform = 'none'
      nodes.sub.style.strokeDashoffset = CIRC.toFixed(2)
      nodes.dot.setAttribute('r', '0')
      for (const el of [nodes.main, nodes.mainFill, nodes.sub, nodes.rect, nodes.arrow]) el.style.opacity = '1'
      for (const el of group) el.style.transform = 'none'
    }

    svg.addEventListener('click', () => { if (opts.onCancel) opts.onCancel() })

    return { setProgress, complete, reset, svg }
  }

  window.ProgressButton = { create }
})()