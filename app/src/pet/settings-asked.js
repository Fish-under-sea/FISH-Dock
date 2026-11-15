// The questions asked over the settings panel — an uninstall's, the market's, a
// .zip's steps, new permissions — put away by one path (upstream bf6f0adb): the
// 取消 button and Esc alike, and closing the settings clears them too, so a
// question once put away is not asked again the next time the settings open.
// Pure, so the state shape can be checked without the whole page (the page's
// own state carries more; this is the part the fix turns on).
;(function (root) {
  // Whether a question may be put away now: not while one is at work
  // (uninstalling, installing).
  function mayCancel(state) {
    return !(state?.gone?.busy === true || state?.flow?.step === 'installing')
  }

  // Put every question over the panel away: the uninstall/install in flight
  // (gone), the market's ask (mkAsk), new permissions (approveFor), and a
  // .zip's steps (flow). Cleared only when mayCancel.
  function cancelAsked(state) {
    if (!mayCancel(state)) return false
    state.gone = null
    state.mkAsk = null
    state.approveFor = null
    state.flow = { ...(state.flow || {}), step: 'idle' }
    return true
  }

  // Closing the settings: whatever was asked over it goes with it (the new
  // permissions' ask would otherwise come again as soon as they reopen).
  function closeCleared(state) {
    state.gone = null
    state.mkAsk = null
    state.approveFor = null
    state.recorded = null
    if (state.flow?.step === 'ask' || state.flow?.step === 'refused') state.flow = { ...state.flow, step: 'idle' }
  }

  const api = { mayCancel, cancelAsked, closeCleared }
  if (typeof module !== 'undefined' && module.exports) {
    module.exports = api
  } else {
    root.SettingsAsked = api
  }
})(this)
