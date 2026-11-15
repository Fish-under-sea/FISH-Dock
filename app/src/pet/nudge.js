// What a press on the island does while something is open in it that way
// (a nudge), not on a session's or a widget's row (upstream 2d5ba793,
// island-state.ts pressOn). Loaded by the island's page before island.js,
// and required by the tests.
//
//   there  a widget's words point somewhere (a letter, the updates): there
//   close  it opened by itself, the press is to put it away (#45)
//   press  what was kept for the next look (a hello after a start, a plugin's
//          card) and shown as the person opened the island: not open by
//          itself, so the press does what any other does (the settings)
;(function (root) {
  function pressOn(nudge) {
    if (nudge?.widget?.open?.tab) return 'there'
    return nudge?.kept ? 'press' : 'close'
  }

  const api = { pressOn }
  if (typeof module !== 'undefined' && module.exports) {
    module.exports = api
  } else {
    root.Nudge = api
  }
})(this)
