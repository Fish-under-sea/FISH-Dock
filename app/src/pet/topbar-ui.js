// DEC-2：顶栏 / dock 的纯逻辑（无 DOM），供 topbar.js 与 node 单测共用。
//
//   - windowItem(win)：把一个「运行中的窗口」（taskbar.rs windows_now 的一项）变成顶栏缩略项
//     / dock 图标的展示模型：标题、缩略图（有则用，无则降级为图标+标题）、进程。
//   - clickAction(item)：点击缩略项/dock 图标要做什么——把该窗口带到前台（SetForegroundWindow），
//     最小化的先还原。这是可测的决策函数。
//   - dockLayout(count, mon, sf)：dock 的图标布局（等宽居中），供页面定位每个图标。
//   - topbarModel(windows, clock)：顶栏要展示的数据（左时间、中项、右入口）。
;(function (root) {
  // 一个运行窗口的展示模型（缩略图不可用时降级：thumb=null，用 icon+title）。
  // 认两种形状：宿主归一后的 { hwnd, title, minimized, icon, thumb }，以及 taskbar 原始
  // 窗口条目 { id, title, png, min }。
  function windowItem(win) {
    const w = win || {}
    const title = String(w.title || w.name || '').trim()
    const hwnd = Number(w.hwnd ?? w.id ?? 0)
    return {
      hwnd,
      title: title || String(w.name || ''),
      app: String(w.app || w.exe || w.name || ''),
      minimized: w.minimized === true || w.min === true,
      // 缩略图：宿主给了就用（data URL / asset），否则 null，页面降级为图标+标题。
      thumb: w.thumb || w.thumbnail || null,
      icon: w.icon || w.png || null,
    }
  }

  // 点击缩略项/图标：{ action: 'focus', hwnd, restore } —— restore 表示最小化要先还原。
  // 无有效 hwnd 则 { action: 'none' }。
  function clickAction(item) {
    const hwnd = Number(item && item.hwnd ? item.hwnd : 0)
    if (!hwnd) return { action: 'none' }
    return { action: 'focus', hwnd, restore: item.minimized === true }
  }

  // dock 图标布局：等宽 + 间距，整体居中在 mon 宽内。返回每个图标的 x（相对 mon 左）+ 格宽。
  function dockLayout(count, monW, sf, icon = 44, gap = 8) {
    const iconPx = Math.round(icon * sf)
    const gapPx = Math.round(gap * sf)
    const n = Math.max(0, count | 0)
    const totalW = n === 0 ? 0 : n * iconPx + (n - 1) * gapPx
    const start = Math.round((monW - totalW) / 2)
    const xs = []
    for (let i = 0; i < n; i++) xs.push(start + i * (iconPx + gapPx))
    return { iconPx, gapPx, start, xs }
  }

  // 顶栏模型：左时间、中项（缩略项）、右入口。clock 是已格式化的 "HH:mm"。
  function topbarModel(windows, clock) {
    const items = (Array.isArray(windows) ? windows : []).map(windowItem)
    return { clock: String(clock || ''), items, right: ['quick'] }
  }

  const api = { windowItem, clickAction, dockLayout, topbarModel }
  if (typeof module !== 'undefined' && module.exports) {
    module.exports = api
  } else {
    root.TopbarUI = api
  }
})(this)
