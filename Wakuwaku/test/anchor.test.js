const assert = require('node:assert/strict')
const { test } = require('node:test')

const A = require('../src/pet/anchor')

// Her island at a top corner (zhoupengjie/Wakuwaku#59): where the page lays
// it out, how it grows, what hangs under it, where its window goes.

test('the side the island is laid out at: where its window was put, at the top of the screen only', () => {
  assert.equal(A.anchorOf({ home: 'island', at: 'left' }), 'left')
  assert.equal(A.anchorOf({ home: 'island', at: 'right' }), 'right')
  assert.equal(A.anchorOf({ home: 'island', at: 'center' }), 'center')
  // Nothing said (island.rs not placing it so yet), or anything else: the centre.
  for (const at of [undefined, null, '', 'centre', 'bottom-left', 1]) assert.equal(A.anchorOf({ home: 'island', at }), 'center', String(at))
  // On the taskbar, and round a Mac's notch: the centre (the notch's place).
  assert.equal(A.anchorOf({ home: 'taskbar', at: 'left' }), 'center')
  assert.equal(A.anchorOf({ home: 'island', at: 'right', notch: true }), 'center')
  assert.deepEqual(A.POSITIONS, ['left', 'center', 'right'])
  assert.equal(A.posOf('right'), 'right')
  assert.equal(A.posOf('top-right'), 'center')
})

test('opening, it grows toward the screen: from its middle at the centre, from its side at a corner', () => {
  const closed = { left: 230, top: 8, width: 300, height: 36 }
  assert.deepEqual(A.grown(closed, 420, 140, 'center'), { x: 170, y: 8, width: 420, height: 140 })
  // From the left corner, to the right: its left edge kept.
  const left = { left: 8, top: 8, width: 300, height: 36 }
  assert.deepEqual(A.grown(left, 420, 140, 'left'), { x: 8, y: 8, width: 420, height: 140 })
  // From the right corner, to the left: its right edge kept (8 px in from the window's 760).
  const right = { left: 452, top: 8, width: 300, height: 36 }
  assert.deepEqual(A.grown(right, 420, 140, 'right'), { x: 332, y: 8, width: 420, height: 140 })
  // Back to its closed size the same way.
  assert.deepEqual(A.grown({ left: 332, top: 8, width: 420, height: 140 }, 300, 36, 'right'), { x: 452, y: 8, width: 300, height: 36 })
})

test('hung under it (the capsule panel): under its middle, or lined up with its side; kept in the window', () => {
  const vw = 760
  assert.equal(A.under({ left: 230, width: 300 }, 340, 'center', vw), 210)
  assert.equal(A.under({ left: 8, width: 300 }, 340, 'left', vw), 8)
  assert.equal(A.under({ left: 452, width: 300 }, 340, 'right', vw), 412)
  // Never off the window's side.
  assert.equal(A.under({ left: 8, width: 120 }, 340, 'center', vw), 8)
  assert.equal(A.under({ left: 700, width: 52 }, 340, 'left', vw), vw - 340 - 8)
  assert.equal(A.SIDE, 8)
})

test("its window on the screen, as island.rs will put it: the work area's side, or its middle", () => {
  const area = [0, 0, 1600, 852]
  assert.equal(A.windowLeft('left', area, 760), 0)
  assert.equal(A.windowLeft('center', area, 760), 420)
  assert.equal(A.windowLeft('right', area, 760), 840)
  // A work area that does not start at the screen's left (Windows' own taskbar on the left).
  assert.equal(A.windowLeft('left', [48, 0, 1552, 900], 760), 48)
  assert.equal(A.windowLeft('right', [48, 0, 1552, 900], 760), 840)
})
