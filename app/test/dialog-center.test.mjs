// 确认弹窗居中于 dock 面板内容区（t28）像素证据：
// 贴左 / 居中 / 贴右三种岛锚点，deviceScaleFactor 1 与 1.5 各量一次，
// 断言弹窗中心与岛面板中心偏差 <= 2 CSS px，并把每张截图存到
// FISH-Dock-1029/build/crash-win/dialog-center/。
import test from 'node:test'
import assert from 'node:assert/strict'
import { createServer } from 'node:http'
import { readFile, mkdir, mkdtemp, access, writeFile } from 'node:fs/promises'
import { spawn } from 'node:child_process'
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
      return { island: { left: island.left, top: island.top, width: island.width, height: island.height, right: island.right, bottom: island.bottom },
               dialog: { left: dialog.left, top: dialog.top, width: dialog.width, height: dialog.height, right: dialog.right, bottom: dialog.bottom },
               actions: ar ? { left: ar.left, right: ar.right, width: ar.width } : null,
               paddingRight: parseFloat(getComputedStyle(box).paddingRight),
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
      const buttonGap = Math.abs(m.dialog.right - m.paddingRight - m.actions.right)
      assert.ok(buttonGap <= 1, `[${anchor} dsf${dsf}] 按钮组右缘应贴弹窗内容区右缘，差 ${buttonGap.toFixed(2)}px（弹窗右 ${m.dialog.right}，padding ${m.paddingRight}，按钮右 ${m.actions.right}）`)
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
    const narrowGap = Math.abs(narrow.dialog.right - narrow.paddingRight - narrow.actions.right)
    assert.ok(narrow.dialog.width <= 276, `窄面板弹窗宽度应 276，实际 ${narrow.dialog.width}`)
    assert.ok(narrowGap <= 1, `窄面板按钮组右缘应贴内容区，差 ${narrowGap.toFixed(2)}px`)
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
