// 生成插件失败提示的两张深色截图，并量失败块高度（t45 验收 ④）。
// 用真实的 extension-panel.js + settings.css + extensions.css 在无头 Chrome 里渲染，
// 宿主 invoke 换成替身；不发真实安装。产物：build/error-copy/{摘要态,展开态}.png
import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import { createServer } from 'node:http'
import { spawn } from 'node:child_process'
import { fileURLToPath } from 'node:url'
import assert from 'node:assert/strict'

const root = fileURLToPath(new URL('../..', import.meta.url))
const outDir = path.join(root, 'build/error-copy')
fs.mkdirSync(outDir, { recursive: true })
const browser = process.env.FISH_DOCK_TEST_BROWSER || 'C:/Program Files/Google/Chrome/Application/chrome.exe'
const panelSrc = fs.readFileSync(path.join(root, 'app/src/pet/extension-panel.js'), 'utf8')
const settingsCss = fs.readFileSync(path.join(root, 'app/src/pet/settings.css'), 'utf8')
const extCss = fs.readFileSync(path.join(root, 'app/src/pet/extensions.css'), 'utf8')

const failure = {
  fishFailure: true, stage: 'download',
  message: '三个下载来源都没成功，插件没能更新。', hint: '检查网络或代理后重试。',
  sources: [
    { origin: 'npm 官方源', short: '版本与目录不一致', error: 'npm 包里的插件归档大小与官方目录不一致（30859277 ≠ 30957961）' },
    { origin: 'npm 镜像', short: '跳转被拒绝', error: '插件下载跳转不属于官方发行资源服务' },
    { origin: 'GitHub 发行包', short: '连接超时', error: '插件下载中断或超过大小限制' },
  ],
}

const boot = `
const payload = ${JSON.stringify(failure)}
const lang = () => 'zh'
const panel = window.ExtensionPanel.create({ lang, redraw(){}, confirm:()=>true, deviceDialog(){}, syncAuthFocus:async()=>{}, invoke: async (command,args)=>{
  if(command==='extension_view') return {installed:[], star:{verified:true,authenticated:true,configured:true,login:'reviewer'}, runtime:null, node:null, root:'X:/合成/.fish-dock'}
  if(command==='extension_catalog') return {cached:true, catalog:{plugins:[{id:'fish.magpie',name:'Magpie',version:'2.1.0',description:'模型网关',authors:['FISH'],hostApi:1,platform:'windows-x86_64',bytes:4096,dependencies:['Node 24']}]}}
  if(command==='extension_manage') throw JSON.stringify(payload)
  if(command==='extension_progress') return {items:[]}
  return undefined
}})
;(async()=>{ await panel.load(); await panel.store(); await panel.action('install','fish.magpie'); document.getElementById('host').innerHTML = panel.render(); window.__ready = true })().catch(e=>{ window.__err = String(e) })
`

const html = `<!doctype html><html><head><meta charset="utf-8">
<link rel="stylesheet" href="/settings.css"><link rel="stylesheet" href="/extensions.css">
<style>
  html,body{margin:0;background:#1c1c1e}
  /* 让设置面板在截图里正常可见（真实里由 #island[data-view=settings] 打开） */
  #island-settings{opacity:1 !important;pointer-events:auto !important;position:static !important;width:520px;margin:16px}
</style></head><body>
<div id="island-settings" data-r="t45"><div id="host"></div></div>
<script src="/panel.js"></script>
<script src="/boot.js"></script>
</body></html>`

const server = createServer((req, res) => {
  if (req.url === '/') { res.setHeader('Content-Type', 'text/html; charset=utf-8'); return res.end(html) }
  if (req.url === '/settings.css') { res.setHeader('Content-Type', 'text/css'); return res.end(settingsCss) }
  if (req.url === '/extensions.css') { res.setHeader('Content-Type', 'text/css'); return res.end(extCss) }
  if (req.url === '/panel.js') { res.setHeader('Content-Type', 'text/javascript'); return res.end(panelSrc) }
  if (req.url === '/boot.js') { res.setHeader('Content-Type', 'text/javascript'); return res.end(boot) }
  res.statusCode = 404; res.end('missing')
})
await new Promise(r => server.listen(0, '127.0.0.1', r))
const profile = fs.mkdtempSync(path.join(os.tmpdir(), 'fish-error-copy-'))
const child = spawn(browser, ['--headless=new', '--disable-gpu', '--no-first-run', '--no-default-browser-check', '--remote-debugging-port=0', `--user-data-dir=${profile}`, '--force-color-profile=srgb', 'about:blank'], { stdio: 'ignore', windowsHide: true })
const pause = ms => new Promise(r => setTimeout(r, ms))
const until = async (job, ms = 20000) => { const end = Date.now() + ms; let last; while (Date.now() < end) { try { const v = await job(); if (v) return v } catch (e) { last = e }; await pause(80) } throw last || Error('超时') }
let ws, seq = 0
const pending = new Map()
try {
  const port = await until(async () => Number((fs.readFileSync(path.join(profile, 'DevToolsActivePort'), 'utf8')).split('\n')[0]))
  const version = await (await fetch(`http://127.0.0.1:${port}/json/version`)).json()
  ws = new WebSocket(version.webSocketDebuggerUrl)
  await new Promise((res, rej) => { ws.addEventListener('open', res, { once: true }); ws.addEventListener('error', rej, { once: true }) })
  ws.addEventListener('message', ev => { const row = JSON.parse(ev.data), w = pending.get(row.id); if (w) { pending.delete(row.id); row.error ? w.reject(Error(JSON.stringify(row.error))) : w.resolve(row.result) } })
  const call = (method, params = {}) => new Promise((resolve, reject) => { const id = ++seq; pending.set(id, { resolve, reject }); ws.send(JSON.stringify({ id, method, params })) })
  const targets = await (await fetch(`http://127.0.0.1:${port}/json/list`)).json()
  const pageWs = targets.find(t => t.type === 'page').webSocketDebuggerUrl
  // 同一个浏览器另开一个 page 连接
  const page = new WebSocket(pageWs)
  await new Promise((res, rej) => { page.addEventListener('open', res, { once: true }); page.addEventListener('error', rej, { once: true }) })
  const ppending = new Map(); let pseq = 0
  page.addEventListener('message', ev => { const row = JSON.parse(ev.data), w = ppending.get(row.id); if (w) { ppending.delete(row.id); row.error ? w.reject(Error(JSON.stringify(row.error))) : w.resolve(row.result) } })
  const pcall = (method, params = {}) => new Promise((resolve, reject) => { const id = ++pseq; ppending.set(id, { resolve, reject }); page.send(JSON.stringify({ id, method, params })) })
  const evaluate = async expression => { const r = await pcall('Runtime.evaluate', { expression, returnByValue: true, awaitPromise: true }); if (r.exceptionDetails) throw Error(JSON.stringify(r.exceptionDetails)); return r.result.value }
  await pcall('Page.enable'); await pcall('Runtime.enable')
  await pcall('Emulation.setDeviceMetricsOverride', { width: 640, height: 640, deviceScaleFactor: 2, mobile: false })
  await pcall('Emulation.setDefaultBackgroundColorOverride', { color: { r: 28, g: 28, b: 30, a: 1 } })
  await pcall('Page.navigate', { url: `http://127.0.0.1:${server.address().port}/` })
  await until(() => evaluate('window.__ready === true || window.__err'))
  const err = await evaluate('window.__err || null'); assert.equal(err, null, String(err))

  const measure = await evaluate(`(() => {
    const fail = document.querySelector('.extension-failure')
    const head = document.querySelector('.extension-failure-main')
    const list = document.querySelector('.extension-failure-sources')
    const lh = parseFloat(getComputedStyle(head).lineHeight) || 0
    return {
      failH: fail ? fail.getBoundingClientRect().height : 0,
      headH: head ? head.getBoundingClientRect().height : 0,
      lineHeight: lh,
      winH: innerHeight,
      rows: list ? list.querySelectorAll('li').length : 0,
      hasMaxHeight: getComputedStyle(fail).maxHeight,
      mainText: head ? head.textContent : '',
    }
  })()`)
  // ④ 失败块高度 ≤ 40vh（这里 640px 视口 → 256px），主文案 ≤ 2 行，折叠区 3 行。
  assert.ok(measure.failH <= 0.4 * measure.winH + 1, `失败块高 ${measure.failH} 应 ≤ 40vh(${0.4 * measure.winH})`)
  assert.ok(measure.headH <= 2 * measure.lineHeight + 1, `主文案 ${measure.headH}px 应 ≤ 2 行(${2 * measure.lineHeight}px)`)
  assert.equal(measure.rows, 3, '折叠区三行')

  const shoot = async file => {
    const clip = await evaluate(`(() => { const r = document.getElementById('island-settings').getBoundingClientRect(); return { x: Math.max(0, Math.round(r.x)), y: Math.max(0, Math.round(r.y)), width: Math.round(r.width), height: Math.round(r.height), scale: 1 } })()`)
    const img = await pcall('Page.captureScreenshot', { format: 'png', clip, captureBeyondViewport: true })
    fs.writeFileSync(path.join(outDir, file), Buffer.from(img.data, 'base64'))
  }
  await shoot('失败摘要态-dark.png')
  await evaluate("document.querySelector('.extension-failure-more').open = true")
  await pause(120)
  await shoot('展开详情态-dark.png')

  // 压力验证：把折叠区复制成 40 行，失败块必须仍被 40vh 封顶（超出滚动）。
  const stress = await evaluate(`(() => {
    document.querySelector('.extension-failure-more').open = true
    const ul = document.querySelector('.extension-failure-sources'); const li = ul.querySelector('li')
    for (let i = 0; i < 40; i++) ul.appendChild(li.cloneNode(true))
    const fail = document.querySelector('.extension-failure')
    return { failH: fail.getBoundingClientRect().height, winH: innerHeight, cap: 0.4 * innerHeight, overflowY: getComputedStyle(fail).overflowY }
  })()`)
  assert.ok(stress.failH <= stress.cap + 1, `40 行时失败块 ${stress.failH} 仍应 ≤ 40vh(${stress.cap})`)
  assert.equal(stress.overflowY, 'auto', '超出应滚动')

  console.log(JSON.stringify({ ok: true, measure, stress, out: ['build/error-copy/失败摘要态-dark.png', 'build/error-copy/展开详情态-dark.png'] }))
  page.close()
} finally {
  try { await ws?.close() } catch { }
  if (child.exitCode === null) child.kill()
  server.closeAllConnections(); await new Promise(r => server.close(r))
}
