// #89：任务视图按钮的两张图标（Windows 11 自己的 SVG）怎么选，纯逻辑，供 bar.js /
// island.js 与 node 单测共用。
//
// 宿主 taskbar_task_view_icons 交出 { light, dark } 两段 **SVG 源字符串**（读自
// Windows 私有 .pri），或 null（Windows 10、资源缺失、读取失败）。按条带的明暗挑：
// 浅色任务栏用字形较深的那张（light），深色任务栏用较浅的那张（dark）。
//
// 拿不到（null / 形状不对）一律回退 U+E7C4 字形：**绝不自绘冒充** Windows 的图标，
// 也不把非字符串（旧宿主把 Vec<u8> 序列化成数字数组的那种）当 SVG 塞进 data URL。
;(function (root) {
  const GLYPH = '\u{e7c4}'

  // 宿主给的两段是否可用：都得是非空字符串，且看起来是 <svg…（挡住数字数组与空串）。
  function usable(icons) {
    if (!icons || typeof icons !== 'object') return null
    const { light, dark } = icons
    if (typeof light !== 'string' || typeof dark !== 'string') return null
    if (!light.trim() || !dark.trim()) return null
    if (!light.includes('<svg') || !dark.includes('<svg')) return null
    return { light, dark }
  }

  // 条带亮 → 用字形较深的那张（light）；条带暗 → 用较浅的那张（dark）。
  // 返回 SVG 源字符串，或 null（用字形）。
  function pick(icons, lightBar) {
    const ok = usable(icons)
    if (!ok) return null
    return lightBar ? ok.light : ok.dark
  }

  // 直接给 CSS 用的 data URL（页面 setProperty('--tview-svg', …)）；没有就是 null。
  function dataUrl(icons, lightBar) {
    const svg = pick(icons, lightBar)
    return svg ? `url("data:image/svg+xml;utf8,${encodeURIComponent(svg)}")` : null
  }

  const api = { GLYPH, usable, pick, dataUrl }
  if (typeof module !== 'undefined' && module.exports) {
    module.exports = api
  } else {
    root.TView = api
  }
})(this)
