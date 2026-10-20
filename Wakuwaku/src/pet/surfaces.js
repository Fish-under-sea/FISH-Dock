// The rects the page hands the Rust side as "solid": page px (x, y, w, h)
// where the window takes the mouse whatever the page says of the pointer
// (island.rs set_editing, pointer.rs look). island.js sends the edit bar and
// whatever picker.js has open; models.js's console layer (#models-host) is
// one of them too — it is not a child of the island, so WebView2's own
// mouseenter / mouseleave never say the pointer is on it, and its frame is
// another origin, so nothing inside it reaches this page either. Without the
// rect the window lets a real press on the console through to the desktop
// under it (2026-10-20). Loaded by the page before island.js and models.js,
// and required by the tests.
;(function (root) {
  // A couple of px more all round: caught, WebView2 hit-tests the pointer a
  // pixel or so from where the Rust side read it, so a press on the very edge
  // is not let through (the same margin the edit bar has had since #53).
  const EDGE_PX = 2

  // A box ({ left, top, width, height }) as the rect sent, or null for one
  // with nothing to take (not laid out, not on screen).
  function grow(box) {
    if (!box || !(box.width > 0) || !(box.height > 0)) return null
    return [Math.floor(box.left) - EDGE_PX, Math.floor(box.top) - EDGE_PX, Math.ceil(box.width) + 2 * EDGE_PX, Math.ceil(box.height) + 2 * EDGE_PX]
  }

  // The elements given (an element, or nothing at all), as the rects to send:
  // those in the document with a size, in the order given.
  function solid(nodes) {
    const out = []
    for (const node of nodes || []) {
      if (!node || !node.isConnected) continue
      const rect = grow(node.getBoundingClientRect())
      if (rect) out.push(rect)
    }
    return out
  }

  const api = { grow, solid, EDGE_PX }

  if (typeof module !== 'undefined' && module.exports) {
    module.exports = api
  } else {
    root.Surfaces = api
  }
})(this)
