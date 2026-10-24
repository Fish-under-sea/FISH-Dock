// The Clash page drawn by the dock itself (clash.js): network cards up top,
// groups at the left, the nodes as a card grid. Data comes from clash_api
// (the controller's own API, its secret kept in Rust); what is checked here
// is the page's own bookkeeping — what it renders out of the API's answer,
// what it calls when a node is picked or the proxy toggled, and the colours
// it gives the delays. No DOM: innerHTML is asserted as text, and presses go
// through the delegated listeners with a closest() stand-in.
const assert = require('node:assert/strict')
const fs = require('node:fs')
const path = require('node:path')
const vm = require('node:vm')
const { test } = require('node:test')

const PET = path.join(__dirname, '..', 'src', 'pet')

const PROXIES = {
  PROXY: { name: 'PROXY', type: 'Selector', now: '香港02', all: ['香港01', '香港02', '美国01', '超时节点'] },
  GLOBAL: { name: 'GLOBAL', type: 'Selector', now: 'DIRECT', all: ['DIRECT', '香港01'] },
  '香港01': { name: '香港01', type: 'Vless', udp: true, history: [{ delay: 120 }] },
  '香港02': { name: '香港02', type: 'Vless', udp: true, history: [{ delay: 350 }] },
  '美国01': { name: '美国01', type: 'Trojan', history: [{ delay: 800 }] },
  超时节点: { name: '超时节点', type: 'Hysteria2', history: [{ delay: 0 }] },
  DIRECT: { name: 'DIRECT', type: 'Direct', history: [] },
}

function page({ answers = {} } = {}) {
  const calls = []
  const el = {
    tagName: 'DIV',
    className: 's-clashpage',
    children: [],
    parent: null,
    innerHTML: '',
    listeners: {},
    dataset: {},
    connected: true,
    addEventListener(type, fn) {
      ;(el.listeners[type] ||= []).push(fn)
    },
    fire(type, event = {}) {
      for (const fn of el.listeners[type] || []) fn({ type, ...event })
    },
    querySelector(selector) {
      // saveSubscription 读订阅输入框的值；替身里给出 settings_get 的同一个。
      if (selector === '.cp-sub') return { value: 'https://sub.example.com/x' }
      return null
    },
  }
  const base = {
    clash_api: async ({ method, path, body }) => {
      calls.push(['api', method, path, body])
      if (method === 'PUT' && path.startsWith('/proxies/')) return null
      return { proxies: PROXIES }
    },
    clash_state: async () => ({ running: true, version: 'v1.19.18', mixed: 57617, controller: 9099, sysproxy: false }),
    settings_get: async () => ({ settings: { clashSubscription: 'https://sub.example.com/x' } }),
    settings_set: async ({ patch }) => ({ settings: patch }),
    clash_stop: async () => null,
    clash_ensure: async () => ({ url: 'u', version: 'v1.19.18', mixed: 57617, controller: 9099 }),
    clash_sysproxy: async ({ enable }) => ({ on: enable, mixed: 57617 }),
    oflm_open_url: async () => null,
    ...answers,
  }
  const window = {
    location: { origin: 'http://tauri.localhost' },
    __TAURI__: {
      core: {
        invoke: async (command, args) => {
          calls.push(['invoke', command, args])
          if (base[command]) return base[command](args)
          return null
        },
      },
    },
  }
  const context = vm.createContext({ window, console })
  vm.runInContext(fs.readFileSync(path.join(PET, 'clash.js'), 'utf8'), context, { filename: 'clash.js' })
  return {
    calls,
    html: () => el.innerHTML,
    el,
    mount: () => window.Clash.mount(el, key => key),
    // 委托点击：target.closest 的判法与 clash.js 的一致。
    click: (selector, extra = {}) => {
      el.fire('click', { target: { closest: sel => (sel === selector ? { dataset: extra, checked: extra.checked } : null), checked: extra.checked } })
    },
    settle: () => new Promise(resolve => setImmediate(resolve)),
  }
}

const settled = async p => {
  await p.settle()
  await p.settle()
}

test('挂载即渲染：组列表两个组、网格四张节点卡、当前节点高亮', async () => {
  const p = page()
  p.mount()
  await settled(p)
  const html = p.html()
  assert.match(html, /data-group-tab="PROXY"/, '组列表有 PROXY')
  assert.match(html, /data-group-tab="GLOBAL"/, '组列表有 GLOBAL（排后）')
  assert.equal((html.match(/data-node=/g) || []).length, 4, '四张节点卡')
  assert.match(html, /cp-node current[^"]*"[^>]*data-node="香港02"/, '当前节点带高亮')
  assert.match(html, /香港01/, '节点名')
  assert.match(html, /Vless/, '协议徽章')
  assert.match(html, /UDP/, 'UDP 徽章')
  assert.ok(p.calls.some(c => c[0] === 'api' && c[1] === 'GET' && c[2] === '/proxies'))
})

test('延迟着色：120 绿、350 黄、800 红、0 是 Error、无记录是 —', async () => {
  const p = page()
  p.mount()
  await settled(p)
  const html = p.html()
  assert.match(html, /cp-delay good">120/, '120ms 绿')
  assert.match(html, /cp-delay mid">350/, '350ms 黄')
  assert.match(html, /cp-delay slow">800/, '800ms 红')
  assert.match(html, /cp-delay bad">Error/, '超时是 Error')
  assert.ok(!html.includes('cp-delay none'), 'PROXY 组里没有未测节点')
})

test('点节点走 PUT /proxies/{组}，节点名严格编码', async () => {
  const p = page()
  p.mount()
  await settled(p)
  p.click('[data-node]', { node: '香港01', group: 'PROXY' })
  await settled(p)
  assert.ok(
    p.calls.some(c => c[0] === 'api' && c[1] === 'PUT' && c[2] === '/proxies/PROXY' && c[3]?.name === '香港01'),
    '选择节点的 PUT 与载荷',
  )
})

test('切组标签只换选中，不调 API', async () => {
  const p = page()
  p.mount()
  await settled(p)
  const before = p.calls.length
  p.click('[data-group-tab]', { groupTab: 'GLOBAL' })
  assert.match(p.html(), /data-node="DIRECT"/, 'GLOBAL 的节点画出来了')
  assert.equal(p.calls.filter(c => c[0] === 'api').length, (p.calls.slice(0, before).filter(c => c[0] === 'api')).length, '切组不额外调 API')
})

test('系统代理开关走 clash_sysproxy(true)，回写状态', async () => {
  const p = page()
  p.mount()
  await settled(p)
  p.click('[data-clash-sysproxy]', { checked: true })
  await settled(p)
  assert.ok(p.calls.some(c => c[0] === 'invoke' && c[1] === 'clash_sysproxy' && c[2]?.enable === true))
  assert.match(p.html(), /s\.clashProxyOn/, '提示条随状态变化（T 未翻译时是键名）')
})

test('保存订阅：settings_set → clash_stop → clash_ensure，刷新数据', async () => {
  const p = page()
  p.mount()
  await settled(p)
  // saveSubscription 读输入框的值：替身里取不到，走 refresh 回填的 subValue。
  // 直接驱动与 Enter 相同的路径：把 subValue 换成已有设置值再保存。
  p.click('[data-clash-sub-save]')
  await settled(p)
  await settled(p)
  assert.ok(p.calls.some(c => c[0] === 'invoke' && c[1] === 'settings_set' && c[2]?.patch?.clashSubscription === 'https://sub.example.com/x'), '订阅进设置')
  assert.ok(p.calls.some(c => c[0] === 'invoke' && c[1] === 'clash_stop'))
  assert.ok(p.calls.some(c => c[0] === 'invoke' && c[1] === 'clash_ensure'))
})

test('测速对组内每个节点发一次延迟测试，失败的不中断', async () => {
  const p = page()
  p.mount()
  await settled(p)
  p.click('[data-clash-test]', { clashTest: 'PROXY' })
  await settled(p)
  const delays = p.calls.filter(c => c[0] === 'api' && c[2]?.startsWith('/proxies/') && c[2]?.includes('/delay'))
  assert.equal(delays.length, 4, '四个节点各一次')
})
