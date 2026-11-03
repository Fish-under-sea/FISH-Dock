// #63 行式动作列表的**真实浏览器**布局取证（t25）：480 / 900 两个视口下量
// 行高、整行宽度、横向溢出、危险色（暗/亮主题）与键盘 Tab 顺序，并存截图。
//
// 为什么单列一个文件：现有 extension-browser.test.mjs 只验消息协议与 iframe 高度，
// 没有视口尺寸/焦点/色值；而这些只能由真实 Chromium 给，静态 CSS 断言（
// extension-style.test.js）不算布局证据。
//
// 页面用的是**真实**的 extension-panel.js 渲染（不是手写镜像标记）、真实的
// settings.css + extensions.css。唯一 fixture 假定：把 #island-settings 宽度
// 放开到 100%（产品里它是固定 520/620），否则 480 视口下量到的是外框而不是面板。
//
// 无浏览器时整条 skip（与 extension-browser 同款判断），不伪造通过。
import test from 'node:test'
import assert from 'node:assert/strict'
import { createServer } from 'node:http'
import { readFile, mkdir, mkdtemp, access, writeFile } from 'node:fs/promises'
import { spawn } from 'node:child_process'
import { fileURLToPath } from 'node:url'
import path from 'node:path'

const root = fileURLToPath(new URL('../..', import.meta.url))
const browser = process.env.FISH_DOCK_TEST_BROWSER || 'C:/Program Files/Google/Chrome/Application/chrome.exe'
const shotDir = path.join(root, '.work/204/desktop/evidence/screenshots')
const pause = ms => new Promise(resolve => setTimeout(resolve, ms))

async function eventually(job, timeout = 12000) {
  const end = Date.now() + timeout
  let last
  while (Date.now() < end) {
    try { const value = await job(); if (value) return value } catch (error) { last = error }
    await pause(80)
  }
  throw last || new Error('等待浏览器行为超时')
}
async function connect(url) {
  const socket = new WebSocket(url), pending = new Map(); let sequence = 0
  await new Promise((resolve, reject) => { socket.addEventListener('open', resolve, { once: true }); socket.addEventListener('error', reject, { once: true }) })
  socket.addEventListener('message', event => { const row = JSON.parse(event.data), waiter = pending.get(row.id); if (waiter) { pending.delete(row.id); row.error ? waiter.reject(new Error(JSON.stringify(row.error))) : waiter.resolve(row.result) } })
  return { socket, call(method, params = {}) { return new Promise((resolve, reject) => { const id = ++sequence; pending.set(id, { resolve, reject }); socket.send(JSON.stringify({ id, method, params })) }) } }
}

// t30：fixture 必须自己声明 UTF-8。原来没有 <meta charset>、响应头也没 charset，
// Chromium 按 windows-1252 猜，中文全乱码——截图里的乱码是 fixture 的编码问题，
// 不是产品渲染问题（修复前的截图留在 evidence/screenshots/before-t30-utf8/）。
const PAGE = `<!doctype html><html><head><meta charset="utf-8">
<link rel="stylesheet" href="/settings.css"><link rel="stylesheet" href="/extensions.css"><link rel="stylesheet" href="/fixture.css">
</head><body><div id="island-settings"><div id="host"></div></div>
<script src="/panel.js"></script><script src="/fixture.js"></script>
</body></html>`

// fixture 只用真实渲染器：喂一份最小的 installed 视图，取 render() 的 HTML。
const FIXTURE = `
const installed = [{ plugin: { manifest: { id: 'fish.magpie', name: 'Magpie', version: '2.5.3', capabilities: ['network'], pages: [], storage: { directory: '.fishplugin-magpie' }, behaviors: { autostart: true } }, enabled: true, digest: 'd1' }, running: true, error: '' }]
window.panel = window.ExtensionPanel.create({
  invoke: async command => {
    if (command === 'extension_view') return { installed, star: { authenticated: true, verified: true, configured: true }, runtime: null, node: null, root: 'X:/fixture', fetch: null }
    if (command === 'extension_catalog') return { cached: true, catalog: { plugins: [] } }
    return {}
  },
  confirm: async () => true,
  lang: () => 'zh',
  redraw() {},
})
window.measure = () => {
  const rows = [...document.querySelectorAll('.extension-more-row')]
  const box = document.querySelector('.extension-more-body')
  const danger = rows.find(row => row.classList.contains('extension-danger'))
  const plain = rows.find(row => !row.classList.contains('extension-danger'))
  const rects = rows.map(row => row.getBoundingClientRect())
  return {
    count: rows.length,
    actions: rows.map(row => row.dataset.extensionAction),
    heights: rects.map(rect => Math.round(rect.height)),
    widths: rects.map(rect => Math.round(rect.width)),
    boxWidth: box ? Math.round(box.getBoundingClientRect().width) : 0,
    dangerColor: danger ? getComputedStyle(danger).color : '',
    plainColor: plain ? getComputedStyle(plain).color : '',
    tabIndexes: rows.map(row => row.tabIndex),
    scrollWidth: document.documentElement.scrollWidth,
    innerWidth: window.innerWidth,
    open: !!document.querySelector('.extension-more')?.open,
  }
}
window.focusFirst = () => { const first = document.querySelector('.extension-more-row'); if (!first) return ''; first.focus(); return document.activeElement?.dataset.extensionAction || '' }
window.activeAction = () => document.activeElement?.dataset.extensionAction || document.activeElement?.tagName || ''
window.setTheme = light => { document.body.classList.toggle('tb-light', !!light); return getComputedStyle(document.querySelector('.extension-more-row.extension-danger') || document.body).color }
;(async () => {
  await window.panel.load()
  document.getElementById('host').innerHTML = window.panel.render()
  const details = document.querySelector('.extension-more')
  if (details) details.open = true
  window.ready = true
})()
`

test('#63 行式动作列表：480/900 视口下行高、整行宽度、无横向溢出、危险色与 Tab 顺序', { timeout: 90000 }, async t => {
  try { await access(browser) } catch { t.skip('设置 FISH_DOCK_TEST_BROWSER 指向已安装 Chromium'); return }
  const files = {}
  for (const name of ['settings.css', 'extensions.css']) files[`/${name}`] = await readFile(path.join(root, 'app/src/pet', name))
  files['/panel.js'] = await readFile(path.join(root, 'app/src/pet/extension-panel.js'))
  const server = createServer((req, res) => {
    // 所有响应都显式带 charset=utf-8（t30）：只靠 <meta charset> 不够稳，
    // 中文文案（面板/按钮/计数）必须是 UTF-8 才不乱码。
    if (req.url === '/') { res.setHeader('Content-Type', 'text/html; charset=utf-8'); res.end(PAGE); return }
    if (req.url === '/fixture.css') {
      // fixture-only 覆盖：产品里这一层是 520/620 固定宽、靠 open 态透明切换；
      // 这里放开宽度并强制可见，才能按视口量面板本身（已在文件头注明）。
      res.setHeader('Content-Type', 'text/css; charset=utf-8')
      res.end('#island-settings{position:static;width:100%;max-width:none;opacity:1;pointer-events:auto}body{margin:0;background:#111}')
      return
    }
    if (req.url === '/fixture.js') { res.setHeader('Content-Type', 'text/javascript; charset=utf-8'); res.end(FIXTURE); return }
    const body = files[req.url]
    if (!body) { res.statusCode = 404; res.end('missing'); return }
    res.setHeader('Content-Type', req.url.endsWith('.js') ? 'text/javascript; charset=utf-8' : 'text/css; charset=utf-8')
    res.end(body)
  })
  await new Promise(resolve => server.listen(0, '127.0.0.1', resolve))
  const base = path.join(root, '.test-results'); await mkdir(base, { recursive: true }); await mkdir(shotDir, { recursive: true })
  const profile = await mkdtemp(path.join(base, 'layout-'))
  const child = spawn(browser, ['--headless=new', '--disable-gpu', '--no-first-run', '--no-default-browser-check', '--remote-debugging-port=0', `--user-data-dir=${profile}`, 'about:blank'], { stdio: 'ignore', windowsHide: true })
  let control, page
  try {
    const port = await eventually(async () => Number((await readFile(path.join(profile, 'DevToolsActivePort'), 'utf8')).split('\n')[0]))
    const version = await (await fetch(`http://127.0.0.1:${port}/json/version`)).json()
    control = await connect(version.webSocketDebuggerUrl)
    const targets = await (await fetch(`http://127.0.0.1:${port}/json/list`)).json()
    page = await connect(targets.find(row => row.type === 'page').webSocketDebuggerUrl)
    const evaluate = async expression => {
      const value = await page.call('Runtime.evaluate', { expression, awaitPromise: true, returnByValue: true })
      if (value.exceptionDetails) throw new Error(JSON.stringify(value.exceptionDetails))
      return value.result.value
    }
    await page.call('Page.navigate', { url: `http://127.0.0.1:${server.address().port}/` })
    await eventually(async () => (await evaluate('window.ready === true')) ? true : false)
    await eventually(async () => (await evaluate('document.querySelectorAll(".extension-more-row").length')) > 0 ? true : false)

    const shut = async (width, theme) => {
      await evaluate(`window.setTheme(${theme === 'light'})`)
      const measured = await evaluate('window.measure()')
      const capture = await page.call('Page.captureScreenshot', { format: 'png' })
      const file = path.join(shotDir, `more-${width}-${theme}.png`)
      await writeFile(file, Buffer.from(capture.data, 'base64'))
      console.log(`[${width}/${theme}]`, JSON.stringify(measured), '→', path.relative(root, file))
      return measured
    }

    for (const width of [480, 900]) {
      await page.call('Emulation.setDeviceMetricsOverride', { width, height: 640, deviceScaleFactor: 1, mobile: false })
      await pause(120)
      const dark = await shut(width, 'dark')
      assert.equal(dark.count, 2, `${width}: 两行动作（更多里只有日志与卸载）`)
      assert.deepEqual(dark.actions, ['logs', 'uninstall'], `${width}: 日志在上、卸载在下`)
      // t30：fixture 编码回归守卫——中文必须按 UTF-8 渲染成「卸载」，
      // 不是 windows-1252 猜出来的乱码。截图乱码就是这里坏的。
      assert.equal(await evaluate(`document.querySelector('.extension-more-row.extension-danger')?.textContent`), '卸载', `${width}: 中文按 UTF-8 渲染（不是乱码）`)
      for (const height of dark.heights) assert.ok(height >= 32, `${width}: 每行 ≥32px（实测 ${dark.heights.join('/')}）`)
      for (const rowWidth of dark.widths) assert.ok(rowWidth >= dark.boxWidth - 1, `${width}: 整行可点（行 ${rowWidth} vs 容器 ${dark.boxWidth}）`)
      assert.ok(dark.scrollWidth <= dark.innerWidth + 1, `${width}: 无横向溢出（scrollWidth ${dark.scrollWidth} ≤ ${dark.innerWidth}）`)
      assert.equal(dark.dangerColor, 'rgb(255, 107, 122)', `${width}: 暗色危险色常态就是 --bad-ink（实测 ${dark.dangerColor}）`)
      assert.notEqual(dark.plainColor, dark.dangerColor, `${width}: 卸载与「打开插件日志」不同色`)
      for (const tabIndex of dark.tabIndexes) assert.ok(tabIndex >= 0, `${width}: 两行都在 tab 序列里`)
      assert.equal(await evaluate('window.focusFirst()'), 'logs', `${width}: 第一行可聚焦`)
      await page.call('Input.dispatchKeyEvent', { type: 'keyDown', key: 'Tab', code: 'Tab', windowsVirtualKeyCode: 9, nativeVirtualKeyCode: 9 })
      await page.call('Input.dispatchKeyEvent', { type: 'keyUp', key: 'Tab', code: 'Tab', windowsVirtualKeyCode: 9, nativeVirtualKeyCode: 9 })
      await pause(80)
      assert.equal(await evaluate('window.activeAction()'), 'uninstall', `${width}: Tab 从日志走到卸载`)
      const light = await shut(width, 'light')
      assert.equal(light.dangerColor, 'rgb(196, 43, 60)', `${width}: 亮色危险色用深档 --bad-ink（实测 ${light.dangerColor}）`)
    }

    // 产品壳约束（只记录，不是组件断言）：真实设置层是固定 520/620 宽，fixture 的
    // 100% 只是为了让组件在视口里量得准。把宽度还原成 520px 后再量一次，把「固定壳
    // 在 480 视口会不会横向溢出」如实写进日志 —— 这条不能靠 fixture 的 100% 洗掉。
    await page.call('Emulation.setDeviceMetricsOverride', { width: 480, height: 640, deviceScaleFactor: 1, mobile: false })
    const shell = await evaluate(`(() => {
      const layer = document.getElementById('island-settings')
      const before = layer.style.width
      layer.style.width = '520px'
      const row = document.querySelector('.extension-more-row')
      const out = {
        layerWidth: Math.round(layer.getBoundingClientRect().width),
        rowWidth: row ? Math.round(row.getBoundingClientRect().width) : 0,
        documentScrollWidth: document.documentElement.scrollWidth,
        innerWidth: window.innerWidth,
      }
      layer.style.width = before
      return out
    })()`)
    console.log('[480/shell-520]', JSON.stringify(shell))
    assert.equal(shell.layerWidth, 520, '固定壳宽度可复现（报告用）')
    assert.ok(shell.rowWidth > 0, '固定壳宽度下动作行仍可量到（报告用，不代表视口无溢出）')
    if (shell.documentScrollWidth > shell.innerWidth + 1) {
      console.log(`[480/shell-520] WARN 产品壳固定 ${shell.layerWidth}px 在 ${shell.innerWidth} 视口会横向溢出（文档 scrollWidth ${shell.documentScrollWidth}）；属产品壳约束，需产品侧决策，不在桌面分支改宽度`)
    }
  } finally {
    page?.socket.close()
    if (control) { await control.call('Browser.close').catch(() => {}); control.socket.close() } else child.kill()
    server.closeAllConnections(); await new Promise(resolve => server.close(resolve))
  }
})