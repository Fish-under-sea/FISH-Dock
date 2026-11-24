// Issue #99 回归测试：真实无头 Chrome（CDP）验证液态玻璃 / 深色 / 浅色模式下
// 宿主根文档与插件 <iframe> 的 color-scheme 匹配，防止 Chromium 因 color-scheme 失配
// 强行给 iframe 合成层填上不透明 #121212（rgb(18,18,18)）或 #ffffff 背景。
// 同时验证：
//   1. OS prefers-color-scheme 为 light 与 dark 两种系统偏好下均能真正透明穿透；
//   2. 顶层透明窗口（WebView2 透明背景）在声明 color-scheme 后仍保持完全透明（alpha=0）；
//   3. 插件页（.models .s-body 含 iframe）不被 clip-path 裁切边角，普通设置页仍保留圆角裁切。
import test from 'node:test'
import assert from 'node:assert/strict'
import { createServer } from 'node:http'
import { readFile, mkdtemp, mkdir, access, rm } from 'node:fs/promises'
import { inflateSync } from 'node:zlib'
import { spawn } from 'node:child_process'
import { fileURLToPath } from 'node:url'
import path from 'node:path'

const root = fileURLToPath(new URL('../..', import.meta.url))
const browser =
  process.env.FISH_DOCK_TEST_BROWSER ||
  process.env.EDGE_BINARY ||
  'C:/Program Files/Google/Chrome/Application/chrome.exe'

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
  let sequence = 0
  await new Promise((resolve, reject) => {
    socket.addEventListener('open', resolve, { once: true })
    socket.addEventListener('error', reject, { once: true })
  })
  socket.addEventListener('message', event => {
    const row = JSON.parse(event.data)
    const waiter = pending.get(row.id)
    if (!waiter) return
    pending.delete(row.id)
    row.error ? waiter.reject(new Error(JSON.stringify(row.error))) : waiter.resolve(row.result)
  })
  return {
    socket,
    call(method, params = {}) {
      return new Promise((resolve, reject) => {
        const id = ++sequence
        pending.set(id, { resolve, reject })
        socket.send(JSON.stringify({ id, method, params }))
      })
    },
  }
}

// 轻量纯 Node PNG 解码器（支持标准 8-bit RGB/RGBA，用于断言真实合成像素）。
function decodePng(buffer) {
  const sig = Buffer.from([137, 80, 78, 71, 13, 10, 26, 10])
  assert.ok(buffer.subarray(0, 8).equals(sig), 'PNG 魔数无效')
  let width = 0, height = 0, bitDepth = 0, colorType = 0
  const idats = []
  let offset = 8
  while (offset < buffer.length) {
    const length = buffer.readUInt32BE(offset)
    const type = buffer.toString('ascii', offset + 4, offset + 8)
    const data = buffer.subarray(offset + 8, offset + 8 + length)
    if (type === 'IHDR') {
      width = data.readUInt32BE(0)
      height = data.readUInt32BE(4)
      bitDepth = data[8]
      colorType = data[9]
    } else if (type === 'IDAT') {
      idats.push(data)
    } else if (type === 'IEND') {
      break
    }
    offset += 12 + length
  }
  assert.equal(bitDepth, 8, `仅支持 8-bit PNG（实际 ${bitDepth}）`)
  const bpp = colorType === 6 ? 4 : colorType === 2 ? 3 : 0
  assert.ok(bpp > 0, `不支持的 PNG colorType=${colorType}`)
  const raw = inflateSync(Buffer.concat(idats))
  const stride = width * bpp
  const pixels = Buffer.alloc(height * stride)
  const paeth = (a, b, c) => {
    const p = a + b - c
    const pa = Math.abs(p - a), pb = Math.abs(p - b), pc = Math.abs(p - c)
    return pa <= pb && pa <= pc ? a : pb <= pc ? b : c
  }
  for (let y = 0; y < height; y++) {
    const filter = raw[y * (stride + 1)]
    const srcStart = y * (stride + 1) + 1
    const dstStart = y * stride
    for (let x = 0; x < stride; x++) {
      const byte = raw[srcStart + x]
      const a = x >= bpp ? pixels[dstStart + x - bpp] : 0
      const b = y > 0 ? pixels[dstStart - stride + x] : 0
      const c = x >= bpp && y > 0 ? pixels[dstStart - stride + x - bpp] : 0
      let val = byte
      if (filter === 1) val = (byte + a) & 255
      else if (filter === 2) val = (byte + b) & 255
      else if (filter === 3) val = (byte + ((a + b) >> 1)) & 255
      else if (filter === 4) val = (byte + paeth(a, b, c)) & 255
      pixels[dstStart + x] = val
    }
  }
  return {
    width,
    height,
    pixel(x, y) {
      const ix = Math.max(0, Math.min(width - 1, Math.round(x)))
      const iy = Math.max(0, Math.min(height - 1, Math.round(y)))
      const i = (iy * width + ix) * bpp
      return { r: pixels[i], g: pixels[i + 1], b: pixels[i + 2], a: bpp === 4 ? pixels[i + 3] : 255 }
    },
  }
}

function rgbDiff(p1, p2) {
  return Math.max(Math.abs(p1.r - p2.r), Math.abs(p1.g - p2.g), Math.abs(p1.b - p2.b))
}

function fmtRgb(p) {
  return `rgba(${p.r},${p.g},${p.b},${p.a})`
}

test('Issue #99：液态玻璃与深/浅模式下插件 iframe 跨 OS 主题透明穿透（无 #121212 黑底）且不裁切边角', { timeout: 60000 }, async t => {
  try { await access(browser) } catch { t.skip('未检测到可用 Chromium（请设置 FISH_DOCK_TEST_BROWSER 或 EDGE_BINARY）'); return }

  const styleCss = await readFile(path.join(root, 'app/src/pet/style.css'))
  const settingsCss = await readFile(path.join(root, 'app/src/pet/settings.css'))
  const extensionsCss = await readFile(path.join(root, 'app/src/pet/extensions.css'))
  const extensionPageJs = await readFile(path.join(root, 'app/src/pet/extension-page.js'))
  const bridgeJs = await readFile(path.join(root, 'sdk/browser/bridge.js'))
  const indexHtml = await readFile(path.join(root, 'app/src/pet/index.html'), 'utf8')
  const metaMatch = indexHtml.match(/<meta\s+name=["']color-scheme["'][^>]*>/i)
  const metaTag = metaMatch ? metaMatch[0] : ''

  const ticket = 'b'.repeat(64)

  const hostHtml = `<!doctype html>
<html lang="zh-CN">
<head>
  <meta charset="utf-8" />
  ${metaTag}
  <link rel="stylesheet" href="/style.css" />
  <link rel="stylesheet" href="/settings.css" />
  <link rel="stylesheet" href="/extensions.css" />
  <style>
    /* 测试定位与清晰背景板：#backdrop 模拟桌面彩色壁纸，位于 #island 背后，不改产品样式规则。 */
    #backdrop {
      position: fixed;
      inset: 0;
      z-index: 0;
      background: rgb(30, 118, 224);
    }
    #backdrop.hidden-bg {
      display: none;
    }
    #island {
      position: fixed;
      left: 40px;
      top: 32px;
      z-index: 1;
      width: 620px;
      height: 360px;
      transition: none;
    }
    #island-settings {
      width: 620px;
      height: 360px;
      transition: none;
    }
    #island-settings .s-body {
      height: 220px;
    }
    #island-settings .s-body > * {
      animation: none;
    }
    #island-settings .s-head {
      padding-left: 8px;
      margin-bottom: 10px;
    }
  </style>
</head>
<body class="dock-glass" data-dock-mode="glass" data-dock-glass="1">
  <div id="backdrop"></div>
  <div id="island" data-view="settings">
    <div id="island-settings" class="models">
      <div class="s-head"><div class="grow"><span class="t">插件设置</span></div></div>
      <div class="s-tabs"><button type="button" class="on"><span>Magpie</span></button></div>
      <div class="s-body"></div>
    </div>
  </div>
  <script src="/extension-page.js"></script>
  <script>
    window.__appearance = {
      mode: 'glass',
      baseMode: 'dark',
      accent: '#5e9bff',
      glassTinted: true,
      glass: { enabled: true, preset: 'balanced', surfaceOp: 38, surfaceBlur: 18, cardOp: 22, cardBlur: 12, overlayOp: 45, overlayBlur: 16, controlOp: 28, controlBlur: 8 },
    }
    window.__childReports = []
    window.addEventListener('message', event => {
      if (event.data?.type === 'test:child-state') window.__childReports.push(event.data)
    })
    window.__page = { key: 'fish.magpie/main', title: '模型服务', digest: 'digest-99' }
    window.__ext = window.ExtensionPage.create({
      lang: () => 'zh',
      getAppearance: () => window.__appearance,
      invoke: async command => {
        if (command === 'extension_page') return { ticket: '${ticket}', url: '/${ticket}/ui/index.html', generation: 1 }
        if (command === 'extension_page_close') return {}
        return {}
      },
    })
    window.__setHostMode = mode => {
      const light = mode === 'light'
      const glassOn = mode === 'glass'
      window.__appearance = {
        ...window.__appearance,
        mode,
        baseMode: light ? 'light' : 'dark',
        glass: { ...window.__appearance.glass, enabled: glassOn },
      }
      document.documentElement.dataset.dockMode = mode
      document.documentElement.dataset.dockGlass = glassOn ? '1' : '0'
      document.body.classList.toggle('dock-light', light)
      document.body.classList.toggle('dock-glass', glassOn)
      document.body.classList.toggle('tb-light', light)
      document.body.dataset.dockMode = mode
      document.body.dataset.dockGlass = glassOn ? '1' : '0'
      window.__ext.refreshTheme()
    }
    window.__mountPlugin = () => window.__ext.enter(document.querySelector('#island-settings .s-body'), window.__page)
    void window.__mountPlugin()
  </script>
</body>
</html>`

  // 严格复刻 FISH-DOCK-Plugins/shared/dock.css + shared/gateway-page.css 的 color-scheme 与透明声明
  const childHtml = `<!doctype html>
<html lang="zh-CN">
<head>
  <meta charset="utf-8" />
  <script src="/${ticket}/_fish_bridge.js"></script>
  <style>
    :root {
      color-scheme: dark;
      --dock-bg: #000;
      --dock-text: #f5f5f7;
    }
    :root[data-dock-theme=light] {
      color-scheme: light;
    }
    /* 测试透明穿透：所有模式均让子文档根/body 透明，从而直接暴露 Chromium iframe 合成层底色。 */
    html, body {
      margin: 0;
      width: 100%;
      height: 100%;
      background: transparent !important;
      color: var(--dock-text);
      font: 13px/1.45 "Segoe UI", "Microsoft YaHei", sans-serif;
    }
    /* 只在底部放一条文字，上方大片空白区域直接用于像素采样，验证是否被填 #121212 或 #ffffff */
    .label {
      position: absolute;
      left: 12px;
      bottom: 10px;
      margin: 0;
      font-weight: 600;
    }
  </style>
</head>
<body>
  <p class="label">Plugin Content</p>
  <script>
    function report() {
      parent.postMessage({
        type: 'test:child-state',
        dockTheme: document.documentElement.dataset.dockTheme || '',
        dockMode: document.documentElement.dataset.dockMode || '',
        dockGlass: document.documentElement.dataset.dockGlass || '',
        colorScheme: getComputedStyle(document.documentElement).colorScheme,
      }, '*')
    }
    if (window.FishDock?.theme?.onChange) {
      window.FishDock.theme.onChange(() => requestAnimationFrame(report))
    }
    requestAnimationFrame(report)
  </script>
</body>
</html>`

  const server = createServer((req, res) => {
    if (req.url === '/') { res.setHeader('Content-Type', 'text/html; charset=utf-8'); res.end(hostHtml); return }
    if (req.url === '/style.css') { res.setHeader('Content-Type', 'text/css; charset=utf-8'); res.end(styleCss); return }
    if (req.url === '/settings.css') { res.setHeader('Content-Type', 'text/css; charset=utf-8'); res.end(settingsCss); return }
    if (req.url === '/extensions.css') { res.setHeader('Content-Type', 'text/css; charset=utf-8'); res.end(extensionsCss); return }
    if (req.url === '/extension-page.js') { res.setHeader('Content-Type', 'text/javascript; charset=utf-8'); res.end(extensionPageJs); return }
    res.setHeader('Content-Security-Policy', "default-src 'none'; script-src 'self' 'unsafe-inline'; style-src 'self' 'unsafe-inline'; img-src 'self' data:; font-src 'self'; connect-src 'none'; frame-src 'none'; form-action 'none'; base-uri 'none'; object-src 'none'")
    if (req.url === `/${ticket}/_fish_bridge.js`) { res.setHeader('Content-Type', 'text/javascript; charset=utf-8'); res.end(bridgeJs); return }
    if (req.url === `/${ticket}/ui/index.html`) { res.setHeader('Content-Type', 'text/html; charset=utf-8'); res.end(childHtml); return }
    res.statusCode = 404
    res.end('missing')
  })
  await new Promise(resolve => server.listen(0, '127.0.0.1', resolve))

  const base = path.join(root, '.test-results')
  await mkdir(base, { recursive: true })
  const profile = await mkdtemp(path.join(base, 'color-scheme-'))
  const child = spawn(
    browser,
    ['--headless=new', '--disable-gpu', '--no-first-run', '--no-default-browser-check', '--remote-debugging-port=0', `--user-data-dir=${profile}`, 'about:blank'],
    { stdio: 'ignore', windowsHide: true },
  )
  let control, page
  try {
    const port = await eventually(async () => Number((await readFile(path.join(profile, 'DevToolsActivePort'), 'utf8')).split('\n')[0]))
    const version = await (await fetch(`http://127.0.0.1:${port}/json/version`)).json()
    control = await connect(version.webSocketDebuggerUrl)
    const targets = await (await fetch(`http://127.0.0.1:${port}/json/list`)).json()
    page = await connect(targets.find(r => r.type === 'page').webSocketDebuggerUrl)
    const evaluate = async expression => {
      const value = await page.call('Runtime.evaluate', { expression, awaitPromise: true, returnByValue: true })
      if (value.exceptionDetails) throw new Error(JSON.stringify(value.exceptionDetails))
      return value.result.value
    }

    await page.call('Emulation.setDeviceMetricsOverride', { width: 720, height: 460, deviceScaleFactor: 1, mobile: false })
    await page.call('Page.navigate', { url: `http://127.0.0.1:${server.address().port}/` })
    await eventually(async () => (await evaluate('window.__childReports.length')) > 0)

    const waitTwoFrames = () => evaluate('new Promise(r => requestAnimationFrame(() => requestAnimationFrame(() => r(true))))')

    // 采样几何坐标：ref 取 #island 内部位于 .s-body 左侧留白（纯 #island 背景，不含 iframe/卡片），
    // frame 取 iframe 上半部透明空白区中心。
    const points = await evaluate(`(() => {
      const island = document.getElementById('island').getBoundingClientRect()
      const frame = document.querySelector('#island-settings iframe.extension-frame').getBoundingClientRect()
      return {
        refX: Math.round(island.left + 6),
        refY: Math.round(frame.top + 40),
        frameX: Math.round(frame.left + frame.width * 0.5),
        frameY: Math.round(frame.top + 40),
      }
    })()`)

    for (const osScheme of ['light', 'dark']) {
      await page.call('Emulation.setEmulatedMedia', {
        features: [{ name: 'prefers-color-scheme', value: osScheme }],
      })

      for (const mode of ['glass', 'dark', 'light']) {
        const expectedScheme = mode === 'light' ? 'light' : 'dark'
        const expectedDockTheme = mode === 'light' ? 'light' : 'dark'
        const beforeCount = await evaluate('window.__childReports.length')
        await evaluate(`window.__setHostMode(${JSON.stringify(mode)}); true`)
        const report = await eventually(async () => {
          const rows = await evaluate(`window.__childReports.slice(${beforeCount})`)
          return rows.find(r => r.dockMode === mode && r.dockTheme === expectedDockTheme) || null
        })
        assert.equal(report.colorScheme, expectedScheme, `[OS=${osScheme}, mode=${mode}] 子文档 color-scheme 应为 ${expectedScheme}`)

        await waitTwoFrames()

        const shot = await page.call('Page.captureScreenshot', { format: 'png' })
        const img = decodePng(Buffer.from(shot.data, 'base64'))
        const ref = img.pixel(points.refX, points.refY)
        const inside = img.pixel(points.frameX, points.frameY)

        if (mode === 'glass') {
          // 液态玻璃叠在 rgb(30,118,224) 蓝色背板上，参考像素明显偏蓝而非 #121212
          assert.ok(ref.b > ref.r + 30, `[OS=${osScheme}, glass] 参考玻璃背板应呈蓝色半透（实测 ${fmtRgb(ref)}）`)
        }
        // 核心像素断言：iframe 内部透明区必须与外部 #island 表面像素一致，禁止出现 #121212(18,18,18) 或 #ffffff(255,255,255) 不透明底
        assert.ok(
          !(inside.r === 18 && inside.g === 18 && inside.b === 18),
          `[OS=${osScheme}, mode=${mode}] 命中 #121212 黑底 bug！iframe=${fmtRgb(inside)}, island=${fmtRgb(ref)}`,
        )
        assert.ok(
          rgbDiff(inside, ref) <= 4,
          `[OS=${osScheme}, mode=${mode}] iframe 未透明穿透：iframe=${fmtRgb(inside)} vs island=${fmtRgb(ref)}`,
        )

        const hostSchemes = await evaluate(`(() => {
          const frame = document.querySelector('#island-settings iframe.extension-frame')
          return {
            root: getComputedStyle(document.documentElement).colorScheme,
            settings: getComputedStyle(document.getElementById('island-settings')).colorScheme,
            frame: getComputedStyle(frame).colorScheme,
          }
        })()`)
        assert.equal(hostSchemes.root, expectedScheme, `[OS=${osScheme}, mode=${mode}] 宿主 :root color-scheme 失配（实测 ${hostSchemes.root}）`)
        assert.equal(hostSchemes.settings, expectedScheme, `[OS=${osScheme}, mode=${mode}] #island-settings color-scheme 失配（实测 ${hostSchemes.settings}）`)
        assert.equal(hostSchemes.frame, expectedScheme, `[OS=${osScheme}, mode=${mode}] iframe color-scheme 失配（实测 ${hostSchemes.frame}）`)
      }
    }

    // 边界检查：仅通过 body.className（例如 OS 浅色时 body 同时带 dock-glass 与 tb-light、未写 html dataset）
    // 宿主与 iframe 仍须稳定保持 color-scheme: dark 并透明穿透
    await page.call('Emulation.setEmulatedMedia', { features: [{ name: 'prefers-color-scheme', value: 'light' }] })
    await evaluate(`(() => {
      delete document.documentElement.dataset.dockMode
      delete document.documentElement.dataset.dockGlass
      delete document.body.dataset.dockMode
      delete document.body.dataset.dockGlass
      window.__appearance = { ...window.__appearance, mode: 'glass', baseMode: 'dark', glass: { ...window.__appearance.glass, enabled: true } }
      document.body.className = 'dock-glass tb-light'
      window.__ext.refreshTheme()
      return true
    })()`)
    await waitTwoFrames()
    const mixedShot = await page.call('Page.captureScreenshot', { format: 'png' })
    const mixedImg = decodePng(Buffer.from(mixedShot.data, 'base64'))
    const mixedRef = mixedImg.pixel(points.refX, points.refY)
    const mixedInside = mixedImg.pixel(points.frameX, points.frameY)
    assert.ok(
      !(mixedInside.r === 18 && mixedInside.g === 18 && mixedInside.b === 18) && rgbDiff(mixedInside, mixedRef) <= 4,
      `[OS=light, body.dock-glass.tb-light] 应保持透明穿透（iframe=${fmtRgb(mixedInside)} vs island=${fmtRgb(mixedRef)}）`,
    )
    const mixedRoot = await evaluate('getComputedStyle(document.documentElement).colorScheme')
    assert.equal(mixedRoot, 'dark', 'dock-glass 与 tb-light 共存时 :root 仍须保持 dark')

    // 检查 .models .s-body 嵌入 iframe 时不被 clip-path 裁切边角，而普通设置页仍保留 inset(0px round 14px)
    const clipCheck = await evaluate(`(() => {
      const layer = document.getElementById('island-settings')
      const body = layer.querySelector('.s-body')
      const withPlugin = getComputedStyle(body).clipPath
      layer.classList.remove('models')
      const ordinary = getComputedStyle(body).clipPath
      layer.classList.add('models')
      return { withPlugin, ordinary }
    })()`)
    assert.equal(clipCheck.withPlugin, 'none', `嵌入插件 iframe 时 .models .s-body 不应继续 clip-path 裁切边角（实测 ${clipCheck.withPlugin}）`)
    assert.match(clipCheck.ordinary, /inset\(0px round 14px\)/, `普通设置页 .s-body 必须保留圆角裁切（实测 ${clipCheck.ordinary}）`)

    // 检查顶层 WebView 透明背景：隐藏测试彩色背板 + 设置透明画布底色后，#island 外部像素必须保持 alpha=0
    await evaluate(`window.__setHostMode('glass'); document.getElementById('backdrop').classList.add('hidden-bg'); true`)
    await page.call('Emulation.setDefaultBackgroundColorOverride', { color: { r: 0, g: 0, b: 0, a: 0 } })
    await waitTwoFrames()
    const transparentShot = await page.call('Page.captureScreenshot', { format: 'png' })
    const transparentImg = decodePng(Buffer.from(transparentShot.data, 'base64'))
    const outside = transparentImg.pixel(5, 5)
    assert.equal(outside.a, 0, `声明 color-scheme 后宿主顶层透明窗口外区仍须保持完全透明（实测 ${fmtRgb(outside)}）`)
  } finally {
    page?.socket.close()
    if (control) {
      await control.call('Browser.close').catch(() => {})
      control.socket.close()
    } else {
      child.kill()
    }
    server.closeAllConnections()
    await new Promise(resolve => server.close(resolve))
    await rm(profile, { recursive: true, force: true }).catch(() => {})
  }
})
