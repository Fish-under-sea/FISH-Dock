// Where her island sits at the top of the screen (zhoupengjie/Wakuwaku#59):
// the top centre, as ever, or a top corner (the islandPos setting: left,
// center, right). At a corner it keeps as far from the screen's side as from
// its top (island.js TOP, 8 px), and opens downward and toward the screen's
// middle: from the left corner it grows to the right, from the right corner
// to the left. What hangs under it follows it (the picker, the capsule's
// panel, the toasts: picker.css, capsule-panel.js), the ring and the dot are
// laid over it anyway (style.css #flow), and it slides up out of the way of
// a window under it wherever it is.
//
// The page lays the island out in its window by the anchor island.rs placed
// that window at (pet:update's islandAt): the window at the screen's top
// left, top right, or top centre, as wide as ever (760, island.rs ISLAND),
// the island in it at that side. None said (an older island.rs), the
// centre. In the playground, fake-tauri.js says it as island.rs does
// (tools/preview).
//
// Only the sums, so the tests can say them: island.js and the panels use
// them. Loaded by the page before island.js, and required by the tests.
;(function (root) {
  const POSITIONS = ['left', 'center', 'right']
  // From the screen's side at a corner: as far as from its top (island.js TOP).
  const SIDE = 8

  // The setting as kept (settings.rs is_island_pos): anything else, the centre.
  const posOf = v => (POSITIONS.includes(v) ? v : 'center')

  // The anchor the page lays the island by: where island.rs put its window
  // (at), at the top of the screen; on the taskbar, or round a Mac's notch
  // (only ever in the middle), none: the centre.
  function anchorOf({ home, at, notch = false } = {}) {
    if (home !== 'island' || notch) return 'center'
    return posOf(at)
  }

  // Where the island lies at a new size (width, height) from where it lies
  // (r: a DOMRect's left, top, width): its top kept; about its middle at the
  // centre, its anchored side kept at a corner (it grows toward the middle).
  function grown(r, width, height, anchor) {
    const x = anchor === 'left' ? r.left : anchor === 'right' ? r.left + r.width - width : r.left + (r.width - width) / 2
    return { x, y: r.top, width, height }
  }

  // The left of what is w wide hung under the island (r: its box; the
  // capsule's panel): under its middle at the centre, lined up with its
  // anchored side at a corner; kept within the window (vw) by margin.
  function under(r, w, anchor, vw, margin = SIDE) {
    const x = anchor === 'left' ? r.left : anchor === 'right' ? r.left + r.width - w : r.left + r.width / 2 - w / 2
    return Math.max(margin, Math.min(x, vw - w - margin))
  }

  // The window's left on the screen (logical px) for an anchor, as island.rs
  // puts it (fit): the work area's side at a corner, else its middle.
  // What the playground and the gallery place the island's page by.
  function windowLeft(anchor, area, winWidth) {
    const [x, , w] = area
    if (anchor === 'left') return x
    if (anchor === 'right') return x + w - winWidth
    return x + (w - winWidth) / 2
  }

  const api = { POSITIONS, SIDE, posOf, anchorOf, grown, under, windowLeft }

  if (typeof module !== 'undefined' && module.exports) {
    module.exports = api
  } else {
    root.Anchor = api
  }
})(this)
