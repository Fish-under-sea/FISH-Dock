// What a press on the island does while something is open in it that way
// (upstream 2d5ba793, island-state.ts pressOn): narrowed to nudge.js so this
// can be required without the whole page.
const assert = require('node:assert/strict')
const { test } = require('node:test')

const { pressOn } = require('../src/pet/nudge')

const nudge = (until = 5000, extra = {}) => ({ until, clip: 'x', text: null, widget: null, ...extra })

test('a press on it while something is open in it: what was kept for a look is no reason to close it', () => {
  // Opened by itself (an ending, a hello set to open): the press puts it away (#45).
  assert.equal(pressOn(nudge(5000)), 'close')
  // What its words point to opens, whichever way it came.
  const letter = { open: { tab: 'mail', account: 'a', uid: 7 } }
  assert.equal(pressOn(nudge(5000, { widget: letter })), 'there')
  assert.equal(pressOn(nudge(5000, { widget: letter, kept: true })), 'there')
  // Kept for the next look and shown as the person opened it (her hello after
  // a start, on the first hover): a press as any other (the settings).
  assert.equal(pressOn(nudge(5000, { kept: true })), 'press')
  assert.equal(pressOn(nudge(5000, { kept: true, widget: { open: {} } })), 'press')
  assert.equal(pressOn(null), 'close')
})
