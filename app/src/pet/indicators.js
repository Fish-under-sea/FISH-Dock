// The taskbar's clock and the Wi-Fi signal, as Windows' own show them.
//
// Loaded by index.html (the island) and bar.html (the other displays' strips)
// before island.js and bar.js. Pure functions only, so
// app/test/taskbar-indicators.test.js can run them in Node without a page.
(function (global) {
  // HH:mm, or HH:mm:ss while Windows' "Show seconds in system clock" is on
  // (taskbar.rs show_seconds_in_system_clock, sent with the look as seconds;
  // island.js / bar.js draw it).
  function clockTime(date, lang, seconds) {
    const locale = lang === 'zh' ? 'zh-CN' : undefined
    const options = seconds
      ? { hour: '2-digit', minute: '2-digit', second: '2-digit' }
      : { hour: '2-digit', minute: '2-digit' }
    return date.toLocaleTimeString(locale, options)
  }

  // Windows' Wi-Fi bars: 0 to 5, or null when the strength is unknown (the
  // host keeps null for "not Wi-Fi / not connected / not readable", so the
  // page never draws a full signal it was not told about).
  function signalBars(value) {
    return Number.isInteger(value) && value >= 0 && value <= 5 ? value : null
  }

  global.FishIndicators = { clockTime, signalBars }
})(globalThis)