// 确认弹窗居中于 dock 面板内容区（t28）像素证据：
// 贴左 / 居中 / 贴右三种岛锚点，deviceScaleFactor 1 与 1.5 各量一次，
// 断言弹窗中心与岛面板中心偏差 <= 2 CSS px，并把每张截图存到
// FISH-Dock-1029/build/crash-win/dialog-center/。
import test from 'node:test'
import assert from 'node:assert/strict'
import { createServer } from 'node:http'
import { readFile, mkdir, mkdtemp, access, writeFile } from 'node:fs/promises'
import { spawn } from 'node:child_process'
import { inflateSync } from 'node:zlib'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const root = fileURLToPath(new URL('../..', import.meta.url))
const pet = path.join(root, 'app/src/pet')
const shots = path.resolve(root, '..', 'build', 'crash-win', 'dialog-center')
const browser = process.env.FISH_DOCK_TEST_BROWSER || 'C:/Program Files/Google/Chrome/Application/chrome.exe'
const pause = ms => new Promise(r => setTimeout(r, ms))
async function eventually(job, timeout = 15000) {
  const end = Date.now() + timeout
  let last
  while (Date.now() < end) {
    try { const v = await job(); if (v) return v } catch (e) { last = e }
    await pause(80)
  }
  throw last || new Error('等待浏览器行为超时')
}
async function connect(url) {
  const socket = new WebSocket(url); const pending = new Map(); let seq = 0
  await new Promise((resolve, reject) => { socket.addEventListener('open', resolve, { once: true }); socket.addEventListener('error', reject, { once: true }) })
  socket.addEventListener('message', e => { const row = JSON.parse(e.data), p = pending.get(row.id); if (p) { pending.delete(row.id); row.error ? p.reject(Error(JSON.stringify(row.error))) : p.resolve(row.result) } })
  return { socket, call: (method, params = {}) => new Promise((resolve, reject) => { const id = ++seq; pending.set(id, { resolve, reject }); socket.send(JSON.stringify({ id, method, params })) }) }
}
const page = markup => `<!doctype html><meta charset="utf-8">
<link rel="stylesheet" href="/style.css"><link rel="stylesheet" href="/settings.css"><link rel="stylesheet" href="/extensions.css"><link rel="stylesheet" href="/picker.css">
<style>html,body{margin:0;width:100%;height:100%;background:#3a6ea5}#island{transition:none!important}</style>
<body class="island home-island">
  <div id="stage"><div id="panel" hidden></div><div id="bubble" class="hidden"></div><div id="pet"><div id="pet-film"></div></div></div>
  <div id="island-bar"><div id="island-row"><div id="island" data-view="settings"><div id="island-compact"></div><div id="island-expanded"></div><div id="island-her"><div class="sheet"></div></div><div id="island-settings" tabindex="-1">${markup}</div></div></div></div>
  <script src="/dialog-anchor.js"></script>
  <script>
    window.openConfirm = (message, anchor) => {
      document.body.classList.toggle('isl-at-left', anchor === 'left')
      document.body.classList.toggle('isl-at-right', anchor === 'right')
      const dialog = document.querySelector('.s-confirm-dialog')
      dialog.querySelector('#s-confirm-message').textContent = message
      dialog.showModal()
      const island = document.getElementById('island')
      const viewportNow = () => ({ width: innerWidth, height: innerHeight })
      const anchorNow = () => window.DialogAnchor?.anchorOf(document.body.classList) || 'center'
      const placed = window.DialogAnchor?.apply(dialog, island.getBoundingClientRect(), viewportNow(), anchorNow()) || null
      window.DialogAnchor?.watch(dialog, island, viewportNow, anchorNow)
      return placed
    }
    window.measure = () => {
      const island = document.getElementById('island').getBoundingClientRect()
      const dialog = document.querySelector('.s-confirm-dialog').getBoundingClientRect()
      const actions = [...document.querySelectorAll('.s-confirm-actions')].find(node => !node.hidden)
      const ar = actions ? actions.getBoundingClientRect() : null
      const box = document.querySelector('.s-confirm-dialog')
      // #74：内容区边界要扣掉边框与滚动条槽位（scrollbar-gutter: stable both-edges
      // 在两侧各留一个滚动条宽），否则「按钮贴内容区」这类断言会把槽位算成留白。
      const cs = getComputedStyle(box)
      const borderL = parseFloat(cs.borderLeftWidth), borderR = parseFloat(cs.borderRightWidth)
      const gutter = Math.max(0, (box.offsetWidth - box.clientWidth - borderL - borderR) / 2)
      return { island: { left: island.left, top: island.top, width: island.width, height: island.height, right: island.right, bottom: island.bottom },
               dialog: { left: dialog.left, top: dialog.top, width: dialog.width, height: dialog.height, right: dialog.right, bottom: dialog.bottom },
               actions: ar ? { left: ar.left, right: ar.right, width: ar.width } : null,
               paddingRight: parseFloat(cs.paddingRight),
               contentLeft: dialog.left + borderL + gutter + parseFloat(cs.paddingLeft),
               contentRight: dialog.right - borderR - gutter - parseFloat(cs.paddingRight),
               text: box.innerText,
               vw: innerWidth, vh: innerHeight, dsf: devicePixelRatio, anchor: document.body.className }
    }
  </script>
</body>`

test('确认弹窗中心 == dock 面板内容区中心（三种锚点 × 100%/150%）', { timeout: 120000 }, async t => {
  try { await access(browser) } catch { t.skip('设置 FISH_DOCK_TEST_BROWSER 指向已安装 Chromium'); return }
  const source = await readFile(path.join(pet, 'settings.js'), 'utf8')
  const markup = source.match(/<dialog class="s-confirm-dialog"[\s\S]*?<\/dialog>/)[0]
  const files = {}
  for (const css of ['style.css', 'settings.css', 'extensions.css', 'picker.css']) files[css] = await readFile(path.join(pet, css), 'utf8')
  files['dialog-anchor.js'] = await readFile(path.join(pet, 'dialog-anchor.js'), 'utf8')
  const html = page(markup)
  await mkdir(shots, { recursive: true })
  const server = createServer((req, res) => {
    const name = req.url.split('?')[0].slice(1)
    res.setHeader('Content-Type', name.endsWith('.css') ? 'text/css' : name.endsWith('.js') ? 'text/javascript' : 'text/html')
    if (!name) return res.end(html)
    const body = files[name]
    if (!body) { res.statusCode = 404; return res.end('') }
    res.end(body)
  })
  await new Promise(r => server.listen(0, '127.0.0.1', r))
  const results = path.join(root, '.test-results'); await mkdir(results, { recursive: true })
  const profile = await mkdtemp(path.join(results, 'dialog-center-'))
  const child = spawn(browser, ['--headless=new', '--disable-gpu', '--no-first-run', '--no-default-browser-check', '--remote-debugging-port=0', `--user-data-dir=${profile}`, 'about:blank'], { stdio: 'ignore', windowsHide: true })
  let control, pg
  const measured = []
  try {
    const port = await eventually(async () => Number((await readFile(path.join(profile, 'DevToolsActivePort'), 'utf8')).split('\n')[0]))
    control = await connect((await (await fetch(`http://127.0.0.1:${port}/json/version`)).json()).webSocketDebuggerUrl)
    pg = await connect((await (await fetch(`http://127.0.0.1:${port}/json/list`)).json()).find(p => p.type === 'page').webSocketDebuggerUrl)
    const evaluate = async expression => { const got = await pg.call('Runtime.evaluate', { expression, awaitPromise: true, returnByValue: true }); if (got.exceptionDetails) throw new Error(JSON.stringify(got.exceptionDetails)); return got.result.value }
    await pg.call('Page.navigate', { url: `http://127.0.0.1:${server.address().port}/` })
    await eventually(() => evaluate('!!window.openConfirm'))
    for (const dsf of [1, 1.5]) for (const anchor of ['left', 'center', 'right']) {
      await pg.call('Emulation.setDeviceMetricsOverride', { width: 1920, height: 900, deviceScaleFactor: dsf, mobile: false })
      await evaluate(`document.querySelector('.s-confirm-dialog').open && document.querySelector('.s-confirm-dialog').close(); document.getElementById('island').style.width='520px'; document.getElementById('island').style.height='700px'; document.body.className='island home-island'; true`)
      await pause(120)
      await evaluate(`window.openConfirm('安装 Magpie 网关 2.0.1？\\n依赖：FISH Dock >= 2.0.0、Node.js 24（宿主提供）', ${JSON.stringify(anchor)})`)
      await pause(150)
      const m = await evaluate('window.measure()')
      const dx = Math.abs((m.dialog.left + m.dialog.right) / 2 - (m.island.left + m.island.right) / 2)
      const dy = Math.abs((m.dialog.top + m.dialog.bottom) / 2 - (m.island.top + m.island.bottom) / 2)
      const buttonGap = Math.abs((m.actions.left + m.actions.right) / 2 - (m.contentLeft + m.contentRight) / 2)
      assert.ok(buttonGap <= 1, `[${anchor} dsf${dsf}] 按钮组应水平居中于弹窗内容区，差 ${buttonGap.toFixed(2)}px（内容区 ${m.contentLeft.toFixed(1)}..${m.contentRight.toFixed(1)}，按钮 ${m.actions.left.toFixed(1)}..${m.actions.right.toFixed(1)}）`)
      assert.equal((m.text.match(/确认操作/g) || []).length, 1, `[${anchor} dsf${dsf}] 标题「确认操作」在弹窗里应只出现一次：${JSON.stringify(m.text)}`)
      assert.match(m.text, /安装 Magpie 网关 2\.0\.1/, 'fixture 文案应是 Magpie 网关')
      assert.doesNotMatch(m.text, /OFM|本地模型 2\.0\.1/, 'fixture 截图里不许再出现 OFM')
      console.log(`[${anchor} dsf${dsf}] dx=${dx.toFixed(2)} dy=${dy.toFixed(2)} buttonGap=${buttonGap.toFixed(2)}`)
      measured.push({ dsf, anchor, dx, dy, buttonGap, dialog: m.dialog, island: m.island, actions: m.actions, vw: m.vw, vh: m.vh })
      assert.ok(dx <= 2, `[${anchor} dsf${dsf}] 弹窗横向中心偏离面板 ${dx.toFixed(2)}px（>2）：弹窗 ${m.dialog.left.toFixed(1)}..${m.dialog.right.toFixed(1)}，岛 ${m.island.left.toFixed(1)}..${m.island.right.toFixed(1)}`)
      assert.ok(dy <= 2, `[${anchor} dsf${dsf}] 弹窗纵向中心偏离面板 ${dy.toFixed(2)}px（>2）`)
      const shot = await pg.call('Page.captureScreenshot', { format: 'png' })
      const file = path.join(shots, `dialog-center-${anchor}-${dsf === 1 ? '100' : '150'}.png`)
      await writeFile(file, Buffer.from(shot.data, 'base64'))
    }
    // 窄面板（岛 300 → 弹窗 276）也验一次：按钮组必须贴新的内容区右边界，
    // 不能还按 360 的旧宽度算。
    await pg.call('Emulation.setDeviceMetricsOverride', { width: 1920, height: 900, deviceScaleFactor: 1.5, mobile: false })
    await evaluate(`document.querySelector('.s-confirm-dialog').open && document.querySelector('.s-confirm-dialog').close(); document.getElementById('island').style.width='300px'; document.getElementById('island').style.height='700px'; document.body.className='island home-island'; true`)
    await pause(120)
    await evaluate(`window.openConfirm('安装 Magpie 网关 2.0.1？\\n依赖：FISH Dock >= 2.0.0、Node.js 24（宿主提供）', 'center')`)
    await pause(150)
    const narrow = await evaluate('window.measure()')
    const narrowGap = Math.abs((narrow.actions.left + narrow.actions.right) / 2 - (narrow.contentLeft + narrow.contentRight) / 2)
    assert.ok(narrow.dialog.width <= 276, `窄面板弹窗宽度应 276，实际 ${narrow.dialog.width}`)
    assert.ok(narrowGap <= 1, `窄面板按钮组应水平居中于内容区，差 ${narrowGap.toFixed(2)}px`)
    measured.push({ dsf: 1.5, anchor: 'center', width: 'narrow', dx: 0, dy: 0, buttonGap: narrowGap, dialog: narrow.dialog, island: narrow.island, actions: narrow.actions })
    const narrowShot = await pg.call('Page.captureScreenshot', { format: 'png' })
    await writeFile(path.join(shots, 'dialog-center-center-narrow-150.png'), Buffer.from(narrowShot.data, 'base64'))
    await writeFile(path.join(shots, 'dialog-center-measurements.json'), JSON.stringify(measured, null, 2))
    // 文字 110% + 150% 缩放（t9 的 page_sf/zoom 口径）：WebView2 的 page zoom
    // 把 CSS 视口缩到物理尺寸 / 1.1（1920/1.1 × 900/1.1），gBCR 与 innerWidth
    // 仍是同一套 CSS px；居中不许被缩放算歪。
    await pg.call('Emulation.setDeviceMetricsOverride', { width: Math.round(1920 / 1.1), height: Math.round(900 / 1.1), deviceScaleFactor: 1.5, mobile: false })
    await evaluate(`document.querySelector('.s-confirm-dialog').open && document.querySelector('.s-confirm-dialog').close(); document.getElementById('island').style.width='520px'; document.getElementById('island').style.height='700px'; document.body.className='island home-island isl-at-right'; true`)
    await pause(150)
    await evaluate(`window.openConfirm('安装 Magpie 网关 2.0.1？\\n依赖：FISH Dock >= 2.0.0、Node.js 24（宿主提供）', 'right')`)
    await pause(150)
    const t110 = await evaluate('window.measure()')
    const tdx = Math.abs((t110.dialog.left + t110.dialog.right) / 2 - (t110.island.left + t110.island.right) / 2)
    const tdy = Math.abs((t110.dialog.top + t110.dialog.bottom) / 2 - (t110.island.top + t110.island.bottom) / 2)
    console.log(`[text110 right dsf1.5] dx=${tdx.toFixed(2)} dy=${tdy.toFixed(2)} vw=${t110.vw} vh=${t110.vh}`)
    assert.ok(tdx <= 2 && tdy <= 2, `[文字110% right dsf1.5] 弹窗中心偏离 ${tdx.toFixed(2)}/${tdy.toFixed(2)}px`)
    const shot110 = await pg.call('Page.captureScreenshot', { format: 'png' })
    await writeFile(path.join(shots, 'dialog-center-right-text110-150.png'), Buffer.from(shot110.data, 'base64'))
    console.log('dialog-center 实测：' + JSON.stringify(measured.map(m => ({ anchor: m.anchor, dsf: m.dsf, dx: +m.dx.toFixed(2), dy: +m.dy.toFixed(2) }))))
  } finally {
    pg?.socket.close()
    if (control) { await control.call('Browser.close').catch(() => {}); control.socket.close() } else child.kill()
    server.closeAllConnections(); await new Promise(r => server.close(r))
  }
})

// ── #74：视觉居中 ────────────────────────────────────────────────────────────
// 几何中心（上一块测试的 ≤2px）达标不等于「看起来居中」：标题 16px 粗体压在上方、
// 按钮组贴右下、卡片没有投影与压暗时会读成偏上偏右。这里量三件事：
//   1) 内容像素的视觉重心 vs 卡片中心（≤4 CSS px，比几何判据宽 2px）；
//   2) 内容块上沿/下沿到内容框的距离差（≤6 CSS px，光学补偿允许上下不等）；
//   3) 遮罩压暗后的卡片/背景对比度（≥1.2:1，且压暗只发生在设置面板窗口内）。
// 另加滚动条与动效两项：滚动条出现时内容中心不动（`scrollbar-gutter: both-edges`），
// 入场缩放的 transform-origin 在卡片中心，三帧里中心位移 ≤2px。
function decodePng(buf) {
  let pos = 8, width = 0, height = 0, bitDepth = 0, colorType = -1
  const idat = []
  while (pos + 8 <= buf.length) {
    const len = buf.readUInt32BE(pos)
    const type = buf.toString('ascii', pos + 4, pos + 8)
    const data = buf.subarray(pos + 8, pos + 8 + len)
    if (type === 'IHDR') { width = data.readUInt32BE(0); height = data.readUInt32BE(4); bitDepth = data[8]; colorType = data[9] }
    else if (type === 'IDAT') idat.push(data)
    else if (type === 'IEND') break
    pos += 12 + len
  }
  assert.equal(bitDepth, 8, `PNG 位深只支持 8，实际 ${bitDepth}`)
  const bpp = colorType === 6 ? 4 : colorType === 2 ? 3 : 0
  assert.ok(bpp, `PNG colorType 只支持 2/6，实际 ${colorType}`)
  const raw = inflateSync(Buffer.concat(idat))
  const stride = width * bpp
  const out = Buffer.alloc(height * stride)
  let p = 0
  for (let y = 0; y < height; y += 1) {
    const filter = raw[p]
    p += 1
    const line = raw.subarray(p, p + stride)
    p += stride
    const cur = out.subarray(y * stride, (y + 1) * stride)
    const prev = y ? out.subarray((y - 1) * stride, y * stride) : Buffer.alloc(stride)
    for (let x = 0; x < stride; x += 1) {
      const a = x >= bpp ? cur[x - bpp] : 0
      const b = prev[x]
      const c = x >= bpp ? prev[x - bpp] : 0
      let v = line[x]
      if (filter === 1) v += a
      else if (filter === 2) v += b
      else if (filter === 3) v += (a + b) >> 1
      else if (filter === 4) {
        const pa = Math.abs(b - c), pb = Math.abs(a - c), pc = Math.abs(a + b - 2 * c)
        v += pa <= pb && pa <= pc ? a : pb <= pc ? b : c
      }
      cur[x] = v & 0xff
    }
  }
  return { width, height, bpp, data: out }
}
const pixel = (img, x, y) => { const i = (y * img.width + x) * img.bpp; return [img.data[i], img.data[i + 1], img.data[i + 2]] }
const luminance = ([r, g, b]) => {
  const f = v => { const s = v / 255; return s <= 0.04045 ? s / 12.92 : ((s + 0.055) / 1.055) ** 2.4 }
  return 0.2126 * f(r) + 0.7152 * f(g) + 0.0722 * f(b)
}
const contrast = (a, b) => { const [hi, lo] = [luminance(a), luminance(b)].sort((x, y) => y - x); return (hi + 0.05) / (lo + 0.05) }
// 卡片内「内容像素」（与卡片背景差异明显者）的质心；以及内容块自身的上下沿。
// bgRect 是卡片矩形（只用来采背景色，避开圆角与边框），contentRect 是内容块的 CSS 矩形。
// 只在内容块矩形里统计：卡片边缘的抗锯齿、圆角、边框都不是内容，混进来会把质心拽偏。
function contentStats(img, bgRect, contentRect, dsf) {
  const bx0 = Math.round(bgRect.left * dsf), bx1 = Math.round(bgRect.right * dsf)
  const by0 = Math.round(bgRect.top * dsf), by1 = Math.round(bgRect.bottom * dsf)
  const r = Math.round(12 * dsf)
  const counts = new Map()
  const bump = (x, y) => { const k = pixel(img, x, y).join(','); counts.set(k, (counts.get(k) || 0) + 1) }
  for (let x = bx0 + r; x < bx1 - r; x += 1) { bump(x, by0 + 4); bump(x, by1 - 5) }
  for (let y = by0 + r; y < by1 - r; y += 1) { bump(bx0 + 4, y); bump(bx1 - 5, y) }
  const bg = [...counts.entries()].sort((a, b) => b[1] - a[1])[0][0].split(',').map(Number)
  const x0 = Math.round(contentRect.left * dsf), x1 = Math.round(contentRect.right * dsf)
  const y0 = Math.round(contentRect.top * dsf), y1 = Math.round(contentRect.bottom * dsf)
  let sum = 0, sx = 0, sy = 0, top = Infinity, bottom = -Infinity
  for (let y = y0; y < y1; y += 1) {
    for (let x = x0; x < x1; x += 1) {
      const p = pixel(img, x, y)
      if (Math.abs(p[0] - bg[0]) + Math.abs(p[1] - bg[1]) + Math.abs(p[2] - bg[2]) < 36) continue
      sum += 1; sx += x; sy += y
      if (y < top) top = y
      if (y > bottom) bottom = y
    }
  }
  assert.ok(sum > 200, `卡片里几乎没采到内容像素（${sum}），质心不可信`)
  return { bg, count: sum, cx: sx / sum, cy: sy / sum, top, bottom }
}

test('确认弹窗的视觉重心落在卡片中心，遮罩读得出分层（#74）', { timeout: 180000 }, async t => {
  try { await access(browser) } catch { t.skip('设置 FISH_DOCK_TEST_BROWSER 指向已安装 Chromium'); return }
  const source = await readFile(path.join(pet, 'settings.js'), 'utf8')
  const markup = source.match(/<dialog class="s-confirm-dialog"[\s\S]*?<\/dialog>/)[0]
  const files = {}
  for (const css of ['style.css', 'settings.css', 'extensions.css', 'picker.css']) files[css] = await readFile(path.join(pet, css), 'utf8')
  files['dialog-anchor.js'] = await readFile(path.join(pet, 'dialog-anchor.js'), 'utf8')
  const html = page(markup)
  await mkdir(shots, { recursive: true })
  const server = createServer((req, res) => {
    const name = req.url.split('?')[0].slice(1)
    res.setHeader('Content-Type', name.endsWith('.css') ? 'text/css' : name.endsWith('.js') ? 'text/javascript' : 'text/html')
    if (!name) return res.end(html)
    const body = files[name]
    if (!body) { res.statusCode = 404; return res.end('') }
    return res.end(body)
  })
  await new Promise(r => server.listen(0, '127.0.0.1', r))
  const results = path.join(root, '.test-results'); await mkdir(results, { recursive: true })
  const profile = await mkdtemp(path.join(results, 'dialog-visual-'))
  const child = spawn(browser, ['--headless=new', '--disable-gpu', '--no-first-run', '--no-default-browser-check', '--remote-debugging-port=0', `--user-data-dir=${profile}`, 'about:blank'])
  let control, pg
  try {
    const port = await eventually(async () => Number((await readFile(path.join(profile, 'DevToolsActivePort'), 'utf8')).split('\n')[0]))
    control = await connect((await (await fetch(`http://127.0.0.1:${port}/json/version`)).json()).webSocketDebuggerUrl)
    pg = await connect((await (await fetch(`http://127.0.0.1:${port}/json/list`)).json()).find(p => p.type === 'page').webSocketDebuggerUrl)
    const evaluate = async expression => { const got = await pg.call('Runtime.evaluate', { expression, awaitPromise: true, returnByValue: true }); if (got.exceptionDetails) throw new Error(got.exceptionDetails.text + ' ' + (got.exceptionDetails.exception?.description || '')); return got.result.value }
    const shot = async () => Buffer.from((await pg.call('Page.captureScreenshot', { format: 'png' })).data, 'base64')
    await pg.call('Page.navigate', { url: `http://127.0.0.1:${server.address().port}/` })
    await eventually(() => evaluate('!!window.openConfirm'))
    const DSF = 1.5
    await pg.call('Emulation.setDeviceMetricsOverride', { width: 1920, height: 900, deviceScaleFactor: DSF, mobile: false })
    await evaluate(`document.getElementById('island').style.width='520px'; document.getElementById('island').style.height='700px'; document.body.className='island home-island isl-at-center'; true`)
    await pause(120)
    await evaluate(`window.openConfirm('安装 Magpie 网关 2.0.1？\\n依赖：FISH Dock >= 2.0.0、Node.js 24（宿主提供）', 'center')`)
    await pause(300)
    const m = await evaluate(`(() => {
      const box = document.querySelector('.s-confirm-dialog')
      const r = box.getBoundingClientRect()
      const cs = getComputedStyle(box)
      const h2 = box.querySelector('h2').getBoundingClientRect()
      // 只取真正有尺寸的元素：隐藏的 s-device-actions 也有 class s-confirm-actions，
      // 它的 rect 是 0×0（0,0 起），混进来会把内容矩形拉到整屏。
      const parts = [box.querySelector('h2'), box.querySelector('#s-confirm-message'), ...box.querySelectorAll('.s-confirm-actions')]
        .filter(Boolean).map(n => n.getBoundingClientRect()).filter(p => p.width > 0 && p.height > 0)
      const contentRect = { left: Math.min(...parts.map(p => p.left)), top: Math.min(...parts.map(p => p.top)),
                            right: Math.max(...parts.map(p => p.right)), bottom: Math.max(...parts.map(p => p.bottom)) }
      return { dialog: { left: r.left, top: r.top, width: r.width, height: r.height, right: r.right, bottom: r.bottom },
               paddingTop: parseFloat(cs.paddingTop), paddingBottom: parseFloat(cs.paddingBottom),
               paddingLeft: parseFloat(cs.paddingLeft), paddingRight: parseFloat(cs.paddingRight),
               contentRect, contentCenterX: h2.left + h2.width / 2, transformOrigin: cs.transformOrigin, scrollbarGutter: cs.scrollbarGutter || '',
               backdrop: getComputedStyle(box, '::backdrop').backgroundColor }
    })()`)
    assert.match(m.transformOrigin, /^(center|\d+(\.\d+)?px) (center|\d+(\.\d+)?px)$/, `入场缩放的 origin 应在卡片中心：${m.transformOrigin}`)
    assert.match(m.scrollbarGutter, /stable/, '滚动条要占固定槽位，否则出现时内容会偏心')
    assert.ok(m.paddingTop > m.paddingBottom, `光学补偿应上多下少，实际上 ${m.paddingTop} 下 ${m.paddingBottom}`)

    const img = decodePng(await shot())
    const stats = contentStats(img, m.dialog, m.contentRect, DSF)
    const devY = stats.cy / DSF - (m.dialog.top + m.dialog.height / 2)
    const devX = stats.cx / DSF - (m.dialog.left + m.dialog.width / 2)
    const topGap = m.contentRect.top - (m.dialog.top + m.paddingTop)
    const bottomGap = (m.dialog.bottom - m.paddingBottom) - m.contentRect.bottom
    const sideGapL = m.contentRect.left - (m.dialog.left + m.paddingLeft)
    const sideGapR = (m.dialog.right - m.paddingRight) - m.contentRect.right
    console.log(`[visual] devY=${devY.toFixed(2)} devX=${devX.toFixed(2)} topGap=${topGap.toFixed(2)} bottomGap=${bottomGap.toFixed(2)} side=${sideGapL.toFixed(2)}/${sideGapR.toFixed(2)} px=${stats.count}`)
    const contentHeight = m.contentRect.bottom - m.contentRect.top
    const geoDevY = (m.contentRect.top + contentHeight / 2) - (m.dialog.top + m.dialog.height / 2)
    console.log(`[visual] geoDevY=${geoDevY.toFixed(2)} contentHeight=${contentHeight.toFixed(2)}`)
    // 三条合起来才是「视觉居中」，任何一条单独成立都不够：
    //  · 内容块几何中心 == 卡片中心（±4px）；
    //  · 上下可视留白相等（±6px；光学补偿允许故意不等，但不能失衡）；
    //  · 像素质心落在内容块的中间四分之一带内——按钮是实心色块，面积必然把质心往下压，
    //    要它严格居中等于要求按钮不存在，所以阈值按内容块高度取 25%，只保证「不贴边」。
    assert.ok(Math.abs(geoDevY) <= 4, `内容块几何中心纵向偏离卡片中心 ${geoDevY.toFixed(2)}px（>4）`)
    assert.ok(Math.abs(devY) <= Math.max(4, contentHeight * 0.25), `内容视觉重心偏离中心 ${devY.toFixed(2)}px（超出内容块高度的 25%）`)
    // 横向不设质心阈值：按钮组是实心色块，面积主导质心，右移/居中都会留下固有偏置。
    // 能硬性保证的是「内容块没有被推到某一侧」——左右留白必须相等。
    assert.ok(Math.abs(sideGapL - sideGapR) <= 1, `内容块左右留白不等：左 ${sideGapL.toFixed(2)} 右 ${sideGapR.toFixed(2)}px`)
    assert.ok(Math.abs(topGap - bottomGap) <= 6, `上下可视留白差 ${Math.abs(topGap - bottomGap).toFixed(2)}px（>6）：上 ${topGap.toFixed(2)} 下 ${bottomGap.toFixed(2)}`)

    // 分层：遮罩必须保持透明（设置面板窗口铺满工作区宽度，压暗会读成全屏模态）。
    // 投影本身在这个 fixture 的深色岛背景上叠不出层次（黑叠黑），所以像素层只验证
    // 「卡片与紧邻外侧可区分」，投影的存在由 settings-focus.test.js 的 CSS 断言守。
    assert.match(m.backdrop, /rgba?\(0, 0, 0, 0\)|transparent/, `遮罩要保持透明，实际 ${m.backdrop}`)
    const outside = pixel(img, Math.round(m.dialog.left * DSF) - 20, Math.round(m.dialog.top * DSF) - 20)
    const ratio = contrast(outside, stats.bg)
    const dist = (a, b) => Math.abs(a[0] - b[0]) + Math.abs(a[1] - b[1]) + Math.abs(a[2] - b[2])
    console.log(`[visual] outside=${outside.join(',')} card=${stats.bg.join(',')} contrast=${ratio.toFixed(2)}`)
    assert.ok(dist(stats.bg, outside) >= 6, `卡片与外侧背景分不开：卡片 ${stats.bg.join(',')} 外侧 ${outside.join(',')}`)
    assert.ok(ratio >= 1.15, `卡片与背景对比度只有 ${ratio.toFixed(2)}:1，层次读不出来`)

    // 滚动条：换成超长内容触发 overflow，内容横向中心不许动。
    await evaluate(`document.querySelector('.s-confirm-dialog').close(); true`)
    await pause(120)
    await evaluate(`window.openConfirm(${JSON.stringify(Array.from({ length: 30 }, (_, i) => `第 ${i + 1} 行：这一行很长，用来把弹窗撑出纵向滚动条。`).join('\n'))}, 'center')`)
    await pause(300)
    const longM = await evaluate(`(() => { const box = document.querySelector('.s-confirm-dialog'); const r = box.getBoundingClientRect(); const h2 = box.querySelector('h2').getBoundingClientRect(); return { dialog: { left: r.left, top: r.top, width: r.width, height: r.height }, scrolls: box.scrollHeight > box.clientHeight + 1, contentCenterX: h2.left + h2.width / 2 } })()`)
    assert.ok(longM.scrolls, '这组 fixture 应触发弹窗内滚动，否则测不到滚动条偏心')
    const shift = Math.abs(longM.contentCenterX - m.contentCenterX)
    console.log(`[visual] 滚动条前后内容中心位移 ${shift.toFixed(2)}px（短 ${m.contentCenterX.toFixed(1)} → 长 ${longM.contentCenterX.toFixed(1)}）`)
    assert.ok(shift <= 1, `滚动条出现后内容横向中心位移 ${shift.toFixed(2)}px（>1）`)

    // 动效：入场缩放期间卡片中心不许位移。
    await evaluate(`document.querySelector('.s-confirm-dialog').close(); true`)
    await pause(120)
    const centers = []
    await evaluate(`window.openConfirm('安装 Magpie 网关 2.0.1？', 'center'); true`)
    for (const wait of [0, 100, 200]) {
      if (wait) await pause(100)
      centers.push(await evaluate(`(() => { const r = document.querySelector('.s-confirm-dialog').getBoundingClientRect(); return { x: r.left + r.width / 2, y: r.top + r.height / 2 } })()`))
    }
    const driftX = Math.max(...centers.map(c => Math.abs(c.x - centers[2].x)))
    const driftY = Math.max(...centers.map(c => Math.abs(c.y - centers[2].y)))
    console.log(`[visual] 入场三帧中心漂移 ${driftX.toFixed(2)}/${driftY.toFixed(2)}px`)
    assert.ok(driftX <= 2 && driftY <= 2, `入场动效让卡片中心漂了 ${driftX.toFixed(2)}/${driftY.toFixed(2)}px（>2）`)

    await writeFile(path.join(shots, 'dialog-visual-measurements.json'), JSON.stringify({ m, stats, devX, devY, topGap, bottomGap, contrast: ratio, shift, centers }, null, 2))
  } finally {
    pg?.socket.close()
    if (control) { await control.call('Browser.close').catch(() => {}); control.socket.close() } else child.kill()
    server.closeAllConnections(); await new Promise(r => server.close(r))
  }
})
