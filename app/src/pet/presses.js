// What a press on a program's button does by the mouse button and the keys
// held, as on Windows 11's own taskbar (zhoupengjie/Wakuwaku#36): the same
// on her taskbar (island.js) and on hers on other displays (bar.js). Loaded
// by their pages before them, and required by the tests.
;(function (root) {
  // A press (its event's type, click, auxclick or contextmenu, and button)
  // with Shift and Ctrl held or not ({ shift, ctrl }), on a program with
  // this many windows open:
  //   press       a plain one: one not running starts, one window to the
  //               front or down, several their list
  //   launch      another of it started (the middle button; Shift)
  //   admin       another run as administrator (Ctrl and Shift)
  //   next        the next of its windows to the front (Ctrl, several)
  //   menu        its menu (a right press)
  //   windowMenu  its window's own menu, or one for all its windows (Shift
  //               and a right press, while it has any)
  //   null        nothing (a right button's auxclick, say: its menu has it)
  function pressOf(e, keys, open) {
    const { shift = false, ctrl = false } = keys || {}
    if (e.type === 'contextmenu') return shift && open > 0 ? 'windowMenu' : 'menu'
    if (e.button === 1) return 'launch'
    if (e.button !== 0) return null
    if (shift) return ctrl ? 'admin' : 'launch'
    if (ctrl && open > 1) return 'next'
    return 'press'
  }

  // Ctrl and a press on a program with several windows: the next of them to
  // the front, in the order its list shows them (as Windows goes along its
  // own): after the one in front, or after the one last brought so within
  // CYCLED_MS (its mark in front may not have come yet); the first when
  // none of them is.
  const CYCLED_MS = 1500
  function nextWindow(windows, last) {
    if (!windows?.length) return null
    let at = last == null ? -1 : windows.findIndex(w => String(w.id) === String(last))
    if (at < 0) at = windows.findIndex(w => w.front)
    return windows[(at + 1) % windows.length]
  }

  // A file held over a program's button this long (ms): its window to the
  // front, or its windows' list open (a card in it held as long: that one),
  // as on Windows' own.
  const DRAG_HOLD_MS = 500

  const api = { pressOf, nextWindow, CYCLED_MS, DRAG_HOLD_MS }

  if (typeof module !== 'undefined' && module.exports) {
    module.exports = api
  } else {
    root.Presses = api
  }
})(this)
