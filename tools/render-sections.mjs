// Render README section banners from assets/readme/section.html.
// The static section-*.png banners predate any checked-in tool; this renders the
// ones we own in the same style/size (1432x83) for both themes.
//
// The output file name is derived from the tab word:
//   section-<tab-lowercased>-<theme>.png
//
// Usage:
//   node tools/render-sections.mjs                 # the default set below
//   node tools/render-sections.mjs "WORKSPACE=工作区" "DATA=数据边界"
//     (each arg: TAB=TITLE; rendered for both light and dark)
import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import { spawn } from 'node:child_process'
import { fileURLToPath, pathToFileURL } from 'node:url'
import assert from 'node:assert/strict'

const DEFAULT_SECTIONS = [
  { tab: 'ACKNOWLEDGEMENTS', title: '致谢' },
  { tab: 'WORKSPACE', title: '工作区' },
  { tab: 'DATA', title: '数据边界' },
]
const sections = process.argv.slice(2).length
  ? process.argv.slice(2).map(arg => { const i = arg.indexOf('='); return { tab: arg.slice(0, i), title: arg.slice(i + 1) } })
  : DEFAULT_SECTIONS

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')
const assets = path.join(root, 'assets/readme')
// 资源后缀用于破 GitHub 图床缓存：README_ASSET_SUFFIX=-v2 时输出 xxx-v2.png
const suffix = process.env.README_ASSET_SUFFIX || ''
const edge = process.env.EDGE_BINARY || 'C:/Program Files (x86)/Microsoft/Edge/Application/msedge.exe'
const profile = fs.mkdtempSync(path.join(os.tmpdir(), 'fish-section-'))
const browser = spawn(edge, ['--headless=new','--disable-gpu','--disable-gpu-sandbox','--disable-features=CalculateNativeWinOcclusion','--no-first-run','--no-default-browser-check','--remote-debugging-port=0',`--user-data-dir=${profile}`,'about:blank'], { stdio:'ignore' })
const pause = () => new Promise(resolve => setTimeout(resolve, 80))
async function until(check) { const end = Date.now() + 25000; while (Date.now() < end) { const v = await check(); if (v) return v; if (browser.exitCode !== null) throw Error(`Edge exited ${browser.exitCode}`); await pause() } throw Error('浏览器就绪超时') }
let ws, serial = 0
const pending = new Map(), errors = [], results = []
try {
  const port = await until(() => { try { return Number(fs.readFileSync(path.join(profile, 'DevToolsActivePort'), 'utf8').split('\n')[0]) } catch { return null } })
  const targets = await (await fetch(`http://127.0.0.1:${port}/json/list`)).json()
  ws = new WebSocket(targets.find(t => t.type === 'page').webSocketDebuggerUrl)
  await new Promise((resolve, reject) => { ws.addEventListener('open', resolve); ws.addEventListener('error', reject) })
  const call = (method, params = {}) => new Promise((resolve, reject) => { const id = ++serial; pending.set(id, { resolve, reject }); ws.send(JSON.stringify({ id, method, params })) })
  ws.addEventListener('message', event => { const data = JSON.parse(event.data); if (data.id && pending.has(data.id)) { const p = pending.get(data.id); pending.delete(data.id); data.error ? p.reject(Error(JSON.stringify(data.error))) : p.resolve(data.result) } if (data.method === 'Runtime.exceptionThrown') errors.push(data.params.exceptionDetails.text) })
  const evaluate = async expression => { const r = await call('Runtime.evaluate', { expression, returnByValue: true, awaitPromise: true }); if (r.exceptionDetails) throw Error(JSON.stringify(r.exceptionDetails)); return r.result.value }
  await call('Page.enable'); await call('Runtime.enable')
  await call('Emulation.setDeviceMetricsOverride', { width: 1500, height: 200, deviceScaleFactor: 1, mobile: false })
  await call('Emulation.setDefaultBackgroundColorOverride', { color: { r: 0, g: 0, b: 0, a: 0 } })
  for (const { tab, title } of sections) {
    for (const theme of ['light', 'dark']) {
      const url = pathToFileURL(path.join(assets, 'section.html')).href + `?theme=${theme}&tab=${encodeURIComponent(tab)}&title=${encodeURIComponent(title)}`
      await call('Page.navigate', { url })
      await until(() => evaluate(`document.readyState==='complete' && document.documentElement.dataset.theme==='${theme}'`))
      await evaluate('document.fonts.ready.then(()=>true)')
      const clip = await evaluate(`(()=>{const r=document.getElementById('sec').getBoundingClientRect();return {x:Math.round(r.x),y:Math.round(r.y),width:Math.round(r.width),height:Math.round(r.height),scale:1}})()`)
      const image = await call('Page.captureScreenshot', { format: 'png', clip, captureBeyondViewport: true })
      const file = `section-${tab.toLowerCase()}-${theme}${suffix}.png`
      const bytes = Buffer.from(image.data, 'base64')
      fs.writeFileSync(path.join(assets, file), bytes)
      results.push({ path: `assets/readme/${file}`, tab, title, theme, size: bytes.length })
    }
  }
  assert.deepEqual(errors, [])
  console.log(JSON.stringify({ images: results.length, consoleErrors: 0, results }))
  await call('Browser.close')
} finally {
  ws?.close()
  if (browser.exitCode === null) browser.kill()
}
