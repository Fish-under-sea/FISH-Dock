// 弹窗归位（t2）实测：真实 Chromium 里把岛的骨架 + settings.css 的确认弹窗
// 放进岛窗口（铺满整个工作区宽度、高度按开设置后会长高），触发一次确认弹窗，
// 量出弹窗与岛的矩形。
//
// 复现（改前，dialog-anchor.js 不存在 → 只有 showModal 的 UA 定位）：
//   dialog 落在视口正中（岛窗口铺满整个工作区宽度，所以视口正中就是屏幕中央），
//   与岛在哪一侧无关；岛在左/右时弹窗都不在岛内。断言因此红。
// 修好（dialog-anchor.js 按岛的矩形定位）：左/中/右三种锚点、窄窗、DIP 1/1.5
// 全部落在岛内，长文本在弹窗内滚动。
import test from 'node:test'
import assert from 'node:assert/strict'
import { createServer } from 'node:http'
import { readFile, mkdir, mkdtemp, access, writeFile } from 'node:fs/promises'
import { spawn } from 'node:child_process'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const root = fileURLToPath(new URL('../..', import.meta.url))
const pet = path.join(root, 'Wakuwaku/src/pet')
const browser = process.env.FISH_DOCK_TEST_BROWSER || 'C:/Program Files/Google/Chrome/Application/chrome.exe'
const pause = ms => new Promise(resolve => setTimeout(resolve, ms))

async function eventually(job, timeout = 15000) {
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

// 岛的骨架与 index.html 一致；确认弹窗的标记从 settings.js 里取真的那一份。
const page = markup => `<!doctype html><meta charset="utf-8">
<link rel="stylesheet" href="/style.css"><link rel="stylesheet" href="/settings.css"><link rel="stylesheet" href="/extensions.css"><link rel="stylesheet" href="/picker.css">
<style>html,body{margin:0;width:100%;height:100%;background:#202124}</style>
<body class="island home-island">
  <div id="stage"><div id="panel" hidden></div><div id="bubble" class="hidden"></div><div id="pet"><div id="pet-film"></div></div></div>
  <div id="island-bar"><div id="island-row"><div id="island" data-view="settings"><div id="island-compact"></div><div id="island-expanded"></div><div id="island-her"><div class="sheet"></div></div><div id="island-settings" tabindex="-1">${markup}</div></div></div></div>
  <script src="/dialog-anchor.js"></script>
  <script>
    // settings.js 的 pumpConfirm 走的就是这两步：showModal，然后按岛的实际矩形定位。
    window.openConfirm = (message, anchor) => {
      document.body.classList.toggle('isl-at-left', anchor === 'left')
      document.body.classList.toggle('isl-at-right', anchor === 'right')
      const dialog = document.querySelector('.s-confirm-dialog')
      dialog.querySelector('#s-confirm-message').textContent = message
      dialog.showModal()
      // settings.js 的 pumpConfirm 走的就是这两步：showModal，然后 seatDialog + watch。
      const island = document.getElementById('island')
      const viewportNow = () => ({ width: innerWidth, height: innerHeight })
      const anchorNow = () => window.DialogAnchor?.anchorOf(document.body.classList) || 'center'
      const placed = window.DialogAnchor?.apply(dialog, island.getBoundingClientRect(), viewportNow(), anchorNow()) || null
      window.DialogAnchor?.watch(dialog, island, viewportNow, anchorNow)
      return placed
    }
    // settings.js 的 reseatConfirm：显示事件 / 设置重画时把开着的弹窗再摆一次。
    window.seatOpen = () => {
      const d = document.querySelector('.s-confirm-dialog')
      if (!d.open) return
      const island = document.getElementById('island')
      window.DialogAnchor?.apply(d, island.getBoundingClientRect(), { width: innerWidth, height: innerHeight }, window.DialogAnchor.anchorOf(document.body.classList))
    }
    window.measure = () => {
      const island = document.getElementById('island').getBoundingClientRect()
      const dialog = document.querySelector('.s-confirm-dialog')
      const box = dialog.getBoundingClientRect()
      const style = getComputedStyle(dialog)
      return {
        island: { left: island.left, top: island.top, right: island.right, bottom: island.bottom, width: island.width, height: island.height },
        dialog: { left: box.left, top: box.top, right: box.right, bottom: box.bottom, width: box.width, height: box.height },
        position: style.position,
        margin: style.margin,
        maxHeight: style.maxHeight,
        backdrop: getComputedStyle(dialog, '::backdrop').backgroundColor,
        scrolls: dialog.scrollHeight > dialog.clientHeight,
        scrollHeight: dialog.scrollHeight,
        clientHeight: dialog.clientHeight,
        vw: innerWidth,
        vh: innerHeight,
        dsf: devicePixelRatio,
        anchor: document.body.className,
      }
    }
  </script>
</body>`

test('确认弹窗落在岛内、跟随岛的位置，不再钉在屏幕中央', { timeout: 120000 }, async t => {
  try {
    await access(browser)
  } catch {
    t.skip('设置 FISH_DOCK_TEST_BROWSER 指向已安装 Chromium')
    return
  }
  const source = await readFile(path.join(pet, 'settings.js'), 'utf8')
  const markup = source.match(/<dialog class="s-confirm-dialog"[\s\S]*?<\/dialog>/)[0]
  const files = {}
  for (const css of ['style.css', 'settings.css', 'extensions.css', 'picker.css']) files[css] = await readFile(path.join(pet, css), 'utf8')
  try {
    files['dialog-anchor.js'] = await readFile(path.join(pet, 'dialog-anchor.js'), 'utf8')
  } catch {
    files['dialog-anchor.js'] = null // 改前：模块还不存在，页面只剩 showModal 的 UA 居中。
  }
  const html = page(markup)

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
  const profile = await mkdtemp(path.join(results, 'dialog-island-'))
  const child = spawn(browser, ['--headless=new', '--disable-gpu', '--no-first-run', '--no-default-browser-check', '--remote-debugging-port=0', `--user-data-dir=${profile}`, 'about:blank'], { stdio: 'ignore', windowsHide: true })
  let control
  let pageSocket
  const measured = []
  try {
    const port = await eventually(async () => Number((await readFile(path.join(profile, 'DevToolsActivePort'), 'utf8')).split('\n')[0]))
    control = await connect((await (await fetch(`http://127.0.0.1:${port}/json/version`)).json()).webSocketDebuggerUrl)
    pageSocket = await connect((await (await fetch(`http://127.0.0.1:${port}/json/list`)).json()).find(p => p.type === 'page').webSocketDebuggerUrl)
    const evaluate = async expression => {
      const got = await pageSocket.call('Runtime.evaluate', { expression, awaitPromise: true, returnByValue: true })
      if (got.exceptionDetails) throw new Error(JSON.stringify(got.exceptionDetails))
      return got.result.value
    }
    await pageSocket.call('Page.navigate', { url: `http://127.0.0.1:${server.address().port}/` })
    await eventually(() => evaluate('!!window.openConfirm'))

    // 文本要长到在任何一档宽度下都超出岛给的 max-height（360px 宽时 30 遍
    // 只有 513px，刚好装下；60 遍才真的溢出，验证「多了就滚动」）。
    const long = '确认安装插件？\n' + '较长说明，用于验证弹窗内部滚动。'.repeat(60)
    // 岛窗口：铺满整个工作区宽度（island.rs size_for），高度是开设置后长起来的那一截。
    const cases = []
    for (const dsf of [1, 1.5]) for (const [vw, vh] of [[1920, 740], [1366, 600], [1120, 760]]) for (const anchor of ['left', 'center', 'right']) cases.push({ vw, vh, dsf, anchor })

    for (const c of cases) {
      await pageSocket.call('Emulation.setDeviceMetricsOverride', { width: c.vw, height: c.vh, deviceScaleFactor: c.dsf, mobile: false })
      await evaluate(`document.querySelector('.s-confirm-dialog').open && document.querySelector('.s-confirm-dialog').close(); true`)
      // 岛的实际尺寸按 anchor 由 CSS 摆出来（isl-at-left / isl-at-right 的内衬）。
      await evaluate(`document.getElementById('island').style.width='520px'; document.getElementById('island').style.height='700px'; true`)
      // 岛的宽高带 0.56s 弹簧（style.css #island 的 transition）：等它停下来再量，
      // 否则量到的是过渡中途的宽度（诊断时实测到 246px，岛还在长）。
      await eventually(async () => {
        const a = await evaluate(`Math.round(document.getElementById('island').getBoundingClientRect().width)`)
        await pause(120)
        return a === await evaluate(`Math.round(document.getElementById('island').getBoundingClientRect().width)`) ? a : 0
      }, 6000)
      const placed = await evaluate(`window.openConfirm(${JSON.stringify(long)}, ${JSON.stringify(c.anchor)})`)
      await pause(80)
      const g = await evaluate('window.measure()')
      measured.push({ ...c, placed, ...g })

      const where = `[${c.anchor} ${c.vw}x${c.vh} dsf${c.dsf}]`
      // 1) 弹窗整个落在岛内（±1 px 取整）。
      assert.ok(g.dialog.left >= g.island.left - 1 && g.dialog.right <= g.island.right + 1, `${where} 弹窗横向越出岛：弹窗 ${g.dialog.left.toFixed(1)}..${g.dialog.right.toFixed(1)}，岛 ${g.island.left.toFixed(1)}..${g.island.right.toFixed(1)}`)
      assert.ok(g.dialog.top >= g.island.top - 1 && g.dialog.bottom <= g.island.bottom + 1, `${where} 弹窗纵向越出岛：弹窗 ${g.dialog.top.toFixed(1)}..${g.dialog.bottom.toFixed(1)}，岛 ${g.island.top.toFixed(1)}..${g.island.bottom.toFixed(1)}`)
      // 2) 不再是 UA 的居中：位置/外边距由我们定。
      assert.equal(g.position, 'fixed', `${where} 弹窗应是 fixed（top layer 里按算出来的 left/top 摆）`)
      assert.equal(g.margin, '0px', `${where} 弹窗不能留 UA 的 margin:auto（那是居中的来源）`)
      // 3) 岛不在中间时，弹窗也不能落在视口正中（屏幕中央锁定）。
      const dialogMid = (g.dialog.left + g.dialog.right) / 2
      if (c.anchor !== 'center') assert.ok(Math.abs(dialogMid - g.vw / 2) > 40, `${where} 弹窗被钉在屏幕正中：弹窗中心 ${dialogMid.toFixed(1)}，视口中心 ${(g.vw / 2).toFixed(1)}`)
      // 4) 高度受限、长文本在弹窗内滚动。
      assert.ok(Number.parseFloat(g.maxHeight) <= g.island.height - 15, `${where} 弹窗 max-height ${g.maxHeight} 应受限在岛内（岛高 ${g.island.height}）`)
      // 内容确实比弹窗高：验证「多了就滚动」而不是「刚好装下」。
      assert.ok(g.scrollHeight > g.clientHeight, `${where} 长文本应超出弹窗并滚动（内容 ${g.scrollHeight}，可见 ${g.clientHeight}）`)
      // 5) 遮罩透明：不把整个屏幕压暗。
      assert.equal(g.backdrop, 'rgba(0, 0, 0, 0)', `${where} 弹窗遮罩应透明`)
    }

    await writeFile(path.join(results, 'dialog-in-island.json'), JSON.stringify(measured, null, 2))
    console.log('dialog-in-island 实测（' + measured.length + ' 组）：' + JSON.stringify(measured.slice(0, 3).map(m => ({ anchor: m.anchor.split(' ').find(x => x.startsWith('isl-at')) || 'center', vw: m.vw, dialog: [Math.round(m.dialog.left), Math.round(m.dialog.top), Math.round(m.dialog.width), Math.round(m.dialog.height)], island: [Math.round(m.island.left), Math.round(m.island.top), Math.round(m.island.width), Math.round(m.island.height)] }))))

    // 岛开着弹窗时自己长高（设置页展开更多内容）：弹窗跟着重摆，不能留在旧位置。
    await pageSocket.call('Emulation.setDeviceMetricsOverride', { width: 1920, height: 900, deviceScaleFactor: 1, mobile: false })
    await evaluate(`document.getElementById('island').style.width='520px'; document.getElementById('island').style.height='300px'; document.body.className='island home-island isl-at-right'; true`)
    await pause(700)
    await evaluate(`window.openConfirm('确认操作？', 'right')`)
    await pause(120)
    const before = await evaluate('window.measure()')
    await evaluate(`document.getElementById('island').style.height='760px'; true`)
    await pause(900)
    const after = await evaluate('window.measure()')
    assert.ok(after.dialog.bottom <= after.island.bottom + 1, `岛长高后弹窗应仍在岛内：弹窗底 ${after.dialog.bottom.toFixed(1)}，岛底 ${after.island.bottom.toFixed(1)}`)
    assert.ok(after.dialog.bottom > before.dialog.bottom + 40, `岛长高后弹窗应跟着往下摆：改前底 ${before.dialog.bottom.toFixed(1)}，改后 ${after.dialog.bottom.toFixed(1)}`)
    console.log('岛长高跟随：岛高 ' + Math.round(before.island.height) + '→' + Math.round(after.island.height) + '，弹窗底 ' + Math.round(before.dialog.bottom) + '→' + Math.round(after.dialog.bottom))

    // ---- t22 FAIL-4：重摆时机（只挂 ResizeObserver 会攒丢尺寸不变的变化）----
    // A) 尺寸不变、只换锚点类：RO 看尺寸，不报；靠 body class 的 MutationObserver 补。
    await pageSocket.call('Emulation.setDeviceMetricsOverride', { width: 1920, height: 900, deviceScaleFactor: 1, mobile: false })
    await evaluate(`document.querySelector('.s-confirm-dialog').open && document.querySelector('.s-confirm-dialog').close(); document.body.className='island home-island'; document.getElementById('island').style.transform=''; document.getElementById('island').style.width='520px'; document.getElementById('island').style.height='700px'; true`)
    await pause(500) // 岛的尺寸/位置稳定（锚点居中）
    await evaluate(`window.openConfirm('确认操作？', 'center')`)
    await pause(120)
    const centred = await evaluate('window.measure()')
    const centredMid = (centred.dialog.left + centred.dialog.right) / 2
    // 只加锚点类：岛整体平移到左边，宽高不变。
    await evaluate(`document.body.classList.add('isl-at-left'); true`)
    const moved = await eventually(async () => {
      const m = await evaluate('window.measure()')
      return Math.abs((m.dialog.left + m.dialog.right) / 2 - centredMid) > 40 ? m : null
    }, 3000)
    assert.ok(Math.abs(moved.island.width - centred.island.width) < 1 && Math.abs(moved.island.height - centred.island.height) < 1, `这一路不该改岛尺寸：改前 ${centred.island.width}x${centred.island.height}，改后 ${moved.island.width}x${moved.island.height}`)
    assert.ok(moved.dialog.left >= moved.island.left - 1 && moved.dialog.right <= moved.island.right + 1, `只换锚点后弹窗应落到左锚点的岛内：弹窗 ${moved.dialog.left.toFixed(1)}..${moved.dialog.right.toFixed(1)}，岛 ${moved.island.left.toFixed(1)}..${moved.island.right.toFixed(1)}`)
    console.log('只换锚点（尺寸不变）：弹窗中心 ' + Math.round(centredMid) + '→' + Math.round((moved.dialog.left + moved.dialog.right) / 2))

    // B) 隐藏期 RO 停摆、显示后补一次：只平移（改 rect 不改尺寸、不改 class）→ RO 不报，
    //    模拟 settings.js 在显示事件里的 reseatConfirm（window.seatOpen）补摆一次。
    await evaluate(`document.querySelector('.s-confirm-dialog').close(); document.body.className='island home-island'; document.getElementById('island').style.width='520px'; document.getElementById('island').style.height='700px'; true`)
    await pause(400)
    await evaluate(`window.openConfirm('确认操作？', 'center')`)
    await pause(120)
    const beforeShift = await evaluate('window.measure()')
    // 只平移岛（transform 改 rect，不改尺寸；RO 只看尺寸 → 不触发）
    await evaluate(`document.getElementById('island').style.transform='translateX(300px)'; true`)
    await pause(300)
    const stale = await evaluate('window.measure()')
    assert.ok(Math.abs(stale.dialog.left - beforeShift.dialog.left) < 1, `只平移（RO 不报）时弹窗应还在旧位置（证明只靠 RO 会攒丢）：${beforeShift.dialog.left.toFixed(1)} vs ${stale.dialog.left.toFixed(1)}`)
    await evaluate('window.seatOpen()')
    await pause(150)
    const afterShow = await evaluate('window.measure()')
    assert.ok(afterShow.dialog.left - stale.dialog.left > 100, `显示事件补摆后弹窗应到新位置：旧左 ${stale.dialog.left.toFixed(1)}，新左 ${afterShow.dialog.left.toFixed(1)}`)
    console.log('显示后补摆：弹窗左 ' + Math.round(stale.dialog.left) + '→' + Math.round(afterShow.dialog.left))

    const shot = await pageSocket.call('Page.captureScreenshot', { format: 'png' })
    await writeFile(path.join(results, 'dialog-in-island.png'), Buffer.from(shot.data, 'base64'))
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
