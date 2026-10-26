const assert = require('node:assert/strict')
const fs = require('node:fs')
const vm = require('node:vm')
const path = require('node:path')
const { test } = require('node:test')
function make({ agree = true, offline = false, installed = [], manage, verified = true, authenticated = true, configured = true, bytes = 4096, auth, open, syncAuthFocus = async () => {} } = {}) {
  const calls = [], window = {}, timers = new Map(); let star = { verified, authenticated, configured }, sequence = 0
  vm.runInNewContext(fs.readFileSync(path.join(__dirname, '../src/pet/extension-panel.js'), 'utf8'), { window, Date, setTimeout: (fn, ms) => { const id = ++sequence; timers.set(id, { fn, ms }); return id }, clearTimeout: id => timers.delete(id) })
  const panel = window.ExtensionPanel.create({ syncAuthFocus, lang: () => 'zh', redraw() {}, confirm: () => agree, invoke: async (command, args) => {
    calls.push({ command, args })
    if (command === 'extension_view') return { installed, star, node: null, root: 'X:/合成/.fish-dock' }
    if (command === 'extension_auth') { star = auth ? await auth(args) : { verified, authenticated, configured, pending: { userCode: 'TEST-CODE' } }; return star }
    if (command === 'settings_open_site') return open ? open(args) : undefined
    if (command === 'extension_catalog') {
      if (offline) throw new Error('商店暂时离线')
      return { cached: true, error: '当前离线，使用缓存', catalog: { plugins: [{ id: 'fish.ofm', name: '<OFM>', version: '2.5.3', description: '模型', author: '作者', maintainer: '维护者', hostApi: 1, platform: 'windows-x86_64', bytes, dependencies: ['Node 24'] }] } }
    }
    if (command === 'extension_manage') return manage ? manage(args) : { installed, node: 'X:/node.exe' }
  } })
  return { panel, calls, timers }
}
test('设备码复制提示以宿主的实际结果为准', async () => {
  for (const copied of [true, false]) {
    const { panel } = make({ authenticated: false, verified: false, open: async () => ({ codeCopied: copied }) })
    await panel.load(); const buttons = await mountAuth(panel); await buttons.login.onclick()
    if (copied) assert.match(panel.render(), /验证码已自动复制/)
    else assert.doesNotMatch(panel.render(), /验证码已自动复制/)
  }
})
test('打开验证浏览器前先读取宿主临时焦点状态', async () => {
  const order = []
  const { panel } = make({ authenticated: false, verified: false, syncAuthFocus: async () => { order.push('focus') }, open: async () => { order.push('browser') } })
  await panel.load(); const buttons = await mountAuth(panel); await buttons.login.onclick()
  assert.deepEqual(order, ['focus', 'browser'])
})
test('设备验证按服务端间隔自动检查，完成或取消后停止', async () => {
  const { panel, calls, timers } = make({ authenticated: false, verified: false, auth: async ({ action }) => action === 'login' ? { pending: { userCode: 'ABCD-EFGH', interval: 5, expiresAt: Date.now() + 900000 } } : { authenticated: true, verified: true, pending: null } })
  await panel.load(); const buttons = await mountAuth(panel); await buttons.login.onclick()
  assert.equal(timers.size, 1)
  const [id, timer] = [...timers][0]; assert.ok(timer.ms >= 5000); timers.delete(id); await timer.fn()
  assert.ok(calls.some(c => c.command === 'extension_auth' && c.args.action === 'check'))
  assert.match(panel.render(), /主仓 Star 已验证/)
  assert.equal(timers.size, 0)
  await buttons.login.onclick(); assert.equal(timers.size, 1)
  await buttons.logout.onclick(); assert.equal(timers.size, 0)
})
const row = { plugin: { manifest: { id: 'fish.ofm', name: 'OFM', version: '2.5.3', capabilities: ['network'], pages: [], storage: { directory: '.fishplugin-ourfreemodel' } }, enabled: true }, running: false, error: '进程退出 <trace>' }
test('首次只读取已安装列表，不发起商店网络或安装操作', async () => {
  const { panel, calls } = make()
  await panel.load()
  assert.deepEqual(calls.map(c => c.command), ['extension_view'])
  assert.match(panel.render(), /尚未安装/)
  assert.match(panel.render(), /Node/)
})
test('商店离线不遮住已安装和失败恢复入口，外部文本经过转义', async () => {
  const { panel } = make({ offline: true, installed: [row] })
  await panel.load(); await panel.store()
  const html = panel.render()
  assert.match(html, /商店暂时离线/)
  assert.match(html, /OFM/)
  assert.match(html, /进程退出 &lt;trace&gt;/)
  assert.match(html, /重试启用/)
  assert.doesNotMatch(html, /<trace>/)
})
test('首次安装默认启用，并把确认结果作为异步操作等待', async () => {
  const { panel, calls } = make({ installed: [] })
  await panel.load(); await panel.store()
  await panel.action('install', 'fish.ofm')
  const manage = calls.find(c => c.command === 'extension_manage')
  assert.equal(manage.args.action, 'install')
  assert.equal(manage.args.enabled, true)
})

test('安装和卸载需实际确认，任意ID不提交给宿主', async () => {
  const { panel, calls } = make({ agree: false, installed: [row] })
  await panel.load(); await panel.store()
  await panel.action('install', 'fish.ofm')
  await panel.action('uninstall', 'fish.ofm')
  await panel.action('enabled', 'unknown.plugin')
  assert.equal(calls.filter(c => c.command === 'extension_manage').length, 0)
})
test('操作失败保留原状态并可重试，不展示凭空成功', async () => {
  let failures = 1
  const { panel, calls } = make({ installed: [row], manage: async () => { if (failures--) throw new Error('配置无效'); return { installed: [{ ...row, running: true, error: null }] } } })
  await panel.load(); await panel.action('enabled', 'fish.ofm')
  assert.match(panel.render(), /配置无效/)
  await panel.action('enabled', 'fish.ofm')
  assert.match(panel.render(), /运行中/)
  assert.equal(calls.filter(c => c.command === 'extension_manage').length, 2)
  assert.equal(calls.at(-1).args.confirm, true)
})
test('检查更新入口可见且商店不重复展示已安装卡片', async () => {
  const { panel } = make({ installed: [row] }); await panel.load(); await panel.store()
  const html = panel.render()
  assert.match(html, /检查更新/)
  assert.equal((html.match(/class="extension-card"/g) || []).length, 1)
  assert.doesNotMatch(html, /<h4>已安装扩展/)
})
test('本地版本高于目录时保持已安装而不展示降级更新', async () => {
  const newer = { ...row, plugin: { ...row.plugin, manifest: { ...row.plugin.manifest, version: '2.10.0' } } }
  const { panel } = make({ installed: [newer] }); await panel.load(); await panel.store()
  assert.doesNotMatch(panel.render(), /data-extension-action="install"/)
  assert.match(panel.render(), /已安装/)
})
test('未验证 Star 时阻断官方插件安装但保留商店浏览', async () => {
  const { panel, calls } = make({ verified: false }); await panel.load(); await panel.store()
  await panel.action('install', 'fish.ofm')
  assert.equal(calls.filter(c => c.command === 'extension_manage').length, 0)
  assert.match(panel.render(), /⭐ 去 Star/)
})
test('提供登录及复核入口，安装结果缺少许可字段时仍保留宿主已有许可', async () => {
  const locked = make({ verified: false }); await locked.panel.load()
  assert.match(locked.panel.render(), /data-extension-auth="login"/)
  assert.match(locked.panel.render(), /data-extension-auth="check"/)
  const { panel, calls } = make(); await panel.load(); await panel.store()
  await panel.action('install', 'fish.ofm'); await panel.action('install', 'fish.ofm')
  assert.equal(calls.filter(c => c.command === 'extension_manage').length, 2)
})
test('已安装页检查后显示新版本操作，无更新给出明确结果', async () => {
  const older = { ...row, plugin: { ...row.plugin, manifest: { ...row.plugin.manifest, version: '1.0.0' } } }
  const check = async panel => {
    const buttons = { '[data-extension-refresh]': {}, '[data-extension-runtime]': {} }
    panel.mount({querySelector:()=>({querySelector:key=>buttons[key],querySelectorAll:()=>[]})})
    await buttons['[data-extension-refresh]'].onclick()
  }
  const { panel } = make({ installed: [older] }); await panel.load(); await check(panel)
  assert.match(panel.render(), /有更新 v2\.5\.3/)
  assert.match(panel.render(), /data-extension-action="install"/)
  const current = make({ installed: [row] }); await current.panel.load(); await check(current.panel)
  assert.match(current.panel.render(), /已是最新版本/)
})
test('未验证 Star 时隐藏官方功能标签但保留卸载；第三方功能不受影响', async () => {
  const official = { ...row, running: true, plugin: { ...row.plugin, manifest: { ...row.plugin.manifest, pages: [{id:'main',title:'OFM'}] } } }
  const third = { ...official, plugin: { ...official.plugin, manifest: { ...official.plugin.manifest, id: 'acme.weather' } } }
  const { panel } = make({ verified:false, installed:[official,third] }); await panel.load()
  assert.deepEqual(Array.from(panel.pages(), p=>p.key), ['acme.weather/main'])
  assert.match(panel.render(), /data-extension-action="uninstall"/)
  assert.match(panel.render(), /Star/)
})

test('登录未开通时禁用不可用入口，并用一条状态提示而非构建术语', async () => {
  const { panel } = make({ verified: false, authenticated: false, configured: false }); await panel.load()
  const html = panel.render()
  assert.match(html, /data-extension-auth="login" disabled/)
  assert.match(html, /data-extension-auth="check" disabled/)
  assert.match(html, /GitHub 登录暂未开放/)
  assert.doesNotMatch(html, /此构建|联系维护者/)
})
test('商店体积自动换算为 KB 或 MB，最多保留一位小数', async () => {
  for (const [bytes, expected] of [[0,'0 B'],[4096,'4.1 KB'],[999000,'999 KB'],[1000000,'1 MB'],[11911*1024,'12.2 MB'],[2000000000,'2 GB']]) {
    const { panel } = make({ bytes }); await panel.load(); await panel.store()
    assert.ok(panel.render().includes('>'+expected+'</dd>'), expected)
    assert.doesNotMatch(panel.render(), /KiB/)
  }
})
test('官方功能和安装入口同时要求登录和 Star，缺一即隐藏', async () => {
  const page = { ...row, running: true, plugin: { ...row.plugin, manifest: { ...row.plugin.manifest, pages: [{id:'main',title:'OFM'}] } } }
  for (const authenticated of [false,true]) for (const verified of [false,true]) {
    const { panel } = make({ authenticated, verified, installed: [page] }); await panel.load(); await panel.store()
    assert.equal(panel.pages().length, authenticated && verified ? 1 : 0)
    assert.equal(/data-extension-page=/.test(panel.render()), false) // 商店不重复已安装功能。
    if (!authenticated || !verified) assert.doesNotMatch(panel.render(), /data-extension-action="install"/)
  }
})
async function mountAuth(panel) {
  const buttons = Object.fromEntries(['login','check','logout'].map(action=>[action,{dataset:{extensionAuth:action}}]))
  panel.mount({ querySelector:()=>({querySelector:()=>({}),querySelectorAll:selector=>selector==='[data-extension-auth]'?Object.values(buttons):[]}) })
  return buttons
}
test('设备登录成功后自动打开官方登录页，只打开一次', async () => {
  const { panel, calls } = make({ authenticated:false, verified:false }); await panel.load()
  const buttons = await mountAuth(panel); await buttons.login.onclick()
  assert.equal(calls.filter(c=>c.command==='settings_open_site').length,1)
  assert.equal(calls.find(c=>c.command==='settings_open_site').args.place,'githubDevice')
})
test('浏览器打开失败保留设备码并在 Dock 内报告，允许重试', async () => {
  const { panel } = make({ authenticated:false, verified:false, open:()=>{throw Error('默认浏览器打开失败')} });await panel.load()
  const buttons=await mountAuth(panel);await buttons.login.onclick()
  assert.match(panel.render(),/默认浏览器打开失败/)
})
test('缓存说明与作者维护者可见，商店名称不会变成标记', async () => {
  const { panel } = make(); await panel.load(); await panel.store()
  const html = panel.render()
  assert.match(html, /缓存/); assert.match(html, /&lt;OFM&gt;/)
  assert.match(html, /作者/); assert.match(html, /维护者/)
  assert.doesNotMatch(html, /<OFM>/)
})
