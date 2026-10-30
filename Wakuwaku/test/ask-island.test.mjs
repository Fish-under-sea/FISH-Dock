// 审批卡「半开」回归：真实 Chromium 中加载 island.js + panel.js + style.css，
// 以 role=island 起页，注入 pet:update 与 pet:asks（Edit 类），断言批准卡
// 有正文/diff 区、高度非 0、不被岛裁掉、长 diff 在 40vh 内滚动。
// 复现证据（修复前）：.code 区被 96px 卡死 / 无 diff 正文 → 断言失败。
import test from 'node:test'
import assert from 'node:assert/strict'
import { createServer } from 'node:http'
import { readFile, mkdir, mkdtemp, access, writeFile } from 'node:fs/promises'
import { spawn } from 'node:child_process'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const root = fileURLToPath(new URL('../..', import.meta.url))
const browser = process.env.FISH_DOCK_TEST_BROWSER || 'C:/Program Files/Google/Chrome/Application/chrome.exe'
const pause = ms => new Promise(resolve => setTimeout(resolve, ms))

async function eventually(job, timeout = 12000) {
  const end = Date.now() + timeout
  let last
  while (Date.now() < end) {
    try {
      const value = await job()
      if (value) return value
    } catch (error) {
      last = error
    }
    await pause(80)
  }
  throw last || new Error('等待浏览器行为超时')
}

async function connect(url) {
  const socket = new WebSocket(url)
  const pending = new Map()
  let seq = 0
  await new Promise((resolve, reject) => {
    socket.addEventListener('open', resolve, { once: true })
    socket.addEventListener('error', reject, { once: true })
  })
  socket.addEventListener('message', e => {
    const row = JSON.parse(e.data)
    const p = pending.get(row.id)
    if (p) {
      pending.delete(row.id)
      row.error ? p.reject(new Error(JSON.stringify(row.error))) : p.resolve(row.result)
    }
  })
  return { socket, call: (method, params = {}) => new Promise((resolve, reject) => { const id = ++seq; pending.set(id, { resolve, reject }); socket.send(JSON.stringify({ id, method, params })) }) }
}

// 岛页骨架与 index.html 相同；bridge 之前拦截 __TAURI__，收集事件监听。
const page = scripts => `<!doctype html><meta charset="utf-8">
<link rel="stylesheet" href="/style.css"><link rel="stylesheet" href="/settings.css"><link rel="stylesheet" href="/extensions.css"><link rel="stylesheet" href="/picker.css">
<style>html,body{margin:0;width:100%;height:100%;background:#202124}</style>
<body>
  <div id="stage"><div id="panel" hidden></div><div id="bubble" class="hidden"></div><div id="pet"><div id="pet-film"></div></div></div>
  <div id="island-bar"><div id="island-row"><div id="island"><div id="island-compact"></div><div id="island-expanded"></div><div id="island-her"><div class="sheet"></div></div><div id="island-settings" tabindex="-1"></div></div></div></div>
  <div id="goo"><div class="goo-island"></div><div class="goo-neck"></div><div class="goo-drop"></div></div>
  <div id="drop-her"><div class="sheet"></div></div>
  <svg width="0" height="0" style="position:absolute" aria-hidden="true"><filter id="goo-filter"><feGaussianBlur in="SourceGraphic" stdDeviation="9" result="blur"/><feColorMatrix in="blur" mode="matrix" values="1 0 0 0 0  0 1 0 0 0  0 0 1 0 0  0 0 0 22 -10"/></filter></svg>
  <script>
  ;(function(){
    const listeners = {}
    window.__TAURI__ = { core: { invoke: () => Promise.resolve({}) }, event: { listen: (ch, fn) => { (listeners[ch] ||= []).push(fn); return Promise.resolve(() => {}) } } }
    window.__fire = (ch, payload) => (listeners[ch] || []).forEach(fn => fn({ payload }))
  })()
  </script>
  ${scripts.map(s => `<script src="/${s}"></script>`).join('\n')}
</body>`

test('Edit 批准卡有正文/diff、高度非 0 且不被岛裁掉（回归：审批卡半开）', { timeout: 60000 }, async t => {
  try {
    await access(browser)
  } catch {
    t.skip('设置 FISH_DOCK_TEST_BROWSER 指向已安装 Chromium')
    return
  }
  const pet = path.join(root, 'Wakuwaku/src/pet')
  const read = async f => readFile(path.join(pet, f), 'utf8')
  const scripts = ['hovers.js', 'surfaces.js', 'bridge.js', 'i18n.js', 'sprite.js', 'status.js', 'widgets.js', 'deskgrid.js', 'capsule.js', 'placing.js', 'wcards.js', 'agents.js', 'mica.js', 'presses.js', 'aim.js', 'flow.js', 'anchor.js', 'pet.js', 'island.js', 'panel.js']
  const files = {}
  for (const s of scripts) files[s] = s === 'i18n.js' ? await readFile(path.join(root, 'Wakuwaku/src/shared/i18n.js')) : await read(s)
  for (const css of ['style.css', 'settings.css', 'extensions.css', 'picker.css']) files[css] = await read(css)
  const html = page(scripts)

  const server = createServer((req, res) => {
    const name = req.url.split('?')[0].slice(1)
    res.setHeader('Content-Type', name.endsWith('.css') ? 'text/css' : name.endsWith('.js') ? 'text/javascript' : 'text/html')
    if (!name) return res.end(html)
    const body = files[name]
    if (!body) { res.statusCode = 404; return res.end('') }
    res.end(body)
  })
  await new Promise(r => server.listen(0, '127.0.0.1', r))
  const results = path.join(root, '.test-results')
  await mkdir(results, { recursive: true })
  const profile = await mkdtemp(path.join(results, 'ask-island-'))
  const child = spawn(browser, ['--headless=new', '--disable-gpu', '--no-first-run', '--no-default-browser-check', '--remote-debugging-port=0', `--user-data-dir=${profile}`, 'about:blank'], { stdio: 'ignore', windowsHide: true })
  let control
  let pageSocket
  try {
    const port = await eventually(async () => Number((await readFile(path.join(profile, 'DevToolsActivePort'), 'utf8')).split('\n')[0]))
    control = await connect((await (await fetch(`http://127.0.0.1:${port}/json/version`)).json()).webSocketDebuggerUrl)
    pageSocket = await connect((await (await fetch(`http://127.0.0.1:${port}/json/list`)).json()).find(p => p.type === 'page').webSocketDebuggerUrl)
    const evaluate = async expression => {
      const got = await pageSocket.call('Runtime.evaluate', { expression, awaitPromise: true, returnByValue: true })
      if (got.exceptionDetails) throw new Error(JSON.stringify(got.exceptionDetails))
      return got.result.value
    }
    await pageSocket.call('Emulation.setDeviceMetricsOverride', { width: 640, height: 800, deviceScaleFactor: 1, mobile: false })
    await pageSocket.call('Page.navigate', { url: `http://127.0.0.1:${server.address().port}/?role=island` })
    await eventually(() => evaluate('!!window.Island && !!window.__fire'))

    // 她在家（island）且有待批的 Edit：先 pet:update，再 pet:asks。
    await evaluate(`(function(){
      __fire('pet:update', { mood: 'waiting', lang: 'zh', config: { display: 'island', out: false }, sprite: 'data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNk+M9QDwADhgGAWjR9awAAAABJRU5ErkJggg==', islandAt: 'center', quiet: false, fullScreen: false, widgets: [], list: [] })
      return window.Island.isOn()
    })()`)

    // Edit 类 ask 的视图（asks.rs view_of 的形态）：summary 带 diff 摘要（修复后 summarize 产出）。
    const diffLines = Array.from({ length: 24 }, (_, i) => (i % 2 ? `+ 新内容第 ${i} 行，用于撑长 diff 区` : `- 旧内容第 ${i} 行，用于撑长 diff 区`)).join('\n')
    const ask = { id: 1, session: 's', agent: '', from: '', project: 'pet', tool: 'Edit', kind: 'permission', summary: `D:\\work\\pet\\src\\main.rs\n${diffLines}`, always: null, reason: null }
    await evaluate(`(function(){
      window.__snapshot = null
      __fire('pet:asks', ${JSON.stringify([ask])})
      return document.getElementById('panel').hidden
    })()`)

    // ask 视图激活 + 弹簧结束后的最终几何。
    await eventually(() => evaluate(`document.getElementById('island').dataset.view === 'ask' && !document.getElementById('panel').hidden`))
    const geometry = await eventually(async () => {
      const g = await evaluate(`(function(){
        const island = document.getElementById('island'), panel = document.getElementById('panel'), code = panel.querySelector('.code')
        if (!code) return null
        const ir = island.getBoundingClientRect(), pr = panel.getBoundingClientRect(), cr = code.getBoundingClientRect()
        const cs = getComputedStyle(code)
        const row = panel.querySelector('.row')
        return { islandH: ir.height, panelBottom: pr.bottom, islandBottom: ir.bottom, codeTop: cr.top, codeH: cr.height,
          codeMax: cs.maxHeight, codeScrollH: code.scrollHeight, codeClientH: code.clientHeight,
          rowBottom: row ? row.getBoundingClientRect().bottom : null, vh: innerHeight,
          panelText: code.textContent.slice(0, 60), hasDiff: code.textContent.includes('- 旧内容第 2 行') && code.textContent.includes('+ 新内容第 3 行') }
      })()`)
      // 弹簧稳定：等高度不再变化后取值（两次采样一致）。
      if (!g) return null
      await pause(700)
      const again = await evaluate(`Math.round(document.getElementById('island').getBoundingClientRect().height)`)
      return Math.round(g.islandH) === again ? g : null
    }, 15000)

    // ---- 断言：修复目标 ----
    // 1) 正文/diff 区存在且有内容（复现点：当前 summarize 只给路径 → hasDiff 红）。
    assert.ok(geometry.hasDiff, `批准卡应有 diff 正文，实际 code 区文本开头：${geometry.panelText}`)
    // 2) 高度非 0。
    assert.ok(geometry.codeH > 0, 'diff 区高度应为正')
    // 3) 长内容在 40vh 内滚动（修复点：`.code` 原被 96px 固定上限卡死）。
    assert.ok(geometry.codeScrollH > geometry.codeClientH, '长 diff 应产生滚动')
    assert.ok(geometry.codeClientH <= geometry.vh * 0.4 + 1, `diff 区可见高度应不超过 40vh（${geometry.vh * 0.4}），实际 ${geometry.codeClientH}`)
    assert.ok(geometry.codeClientH >= 200, `diff 区不应被 96px 卡死，应在 40vh/220px 上限内展开（当前 ${geometry.codeClientH}）`)
    // 4) 岛不裁卡：面板与按钮行都在岛内。
    assert.ok(geometry.panelBottom <= geometry.islandBottom + 1, `面板底 ${geometry.panelBottom} 应在岛底 ${geometry.islandBottom} 内`)
    assert.ok(geometry.rowBottom !== null && geometry.rowBottom <= geometry.islandBottom + 1, `按钮行底 ${geometry.rowBottom} 应在岛底 ${geometry.islandBottom} 内`)
    // 5) 岛自身不被撑爆屏幕。
    assert.ok(geometry.islandH <= geometry.vh + 1, `岛高 ${geometry.islandH} 应不超过视口 ${geometry.vh}`)
    // 证据留档。
    const shot = await pageSocket.call('Page.captureScreenshot', { format: 'png' })
    await writeFile(path.join(results, 'ask-island-edit.png'), Buffer.from(shot.data, 'base64'))
  } finally {
    pageSocket?.socket.close()
    if (control) {
      await control.call('Browser.close').catch(() => {})
      control.socket.close()
    } else child.kill()
    server.closeAllConnections()
    await new Promise(r => server.close(r))
  }
})
