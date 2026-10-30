const assert = require('node:assert/strict')
const fs = require('node:fs')
const vm = require('node:vm')
const path = require('node:path')
const { test } = require('node:test')
function make({ agree = true, offline = false, installed = [], manage, verified = true, authenticated = true, configured = true, bytes = 4096, auth, open, deviceDialog = () => {}, runtimeInfo = null, syncAuthFocus = async () => {}, initialStar = {}, entries = null, third = null } = {}) {
  const calls = [], window = {}, timers = new Map(); let star = { verified, authenticated, configured, ...initialStar }, sequence = 0
  vm.runInNewContext(fs.readFileSync(path.join(__dirname, '../src/pet/extension-panel.js'), 'utf8'), { window, Date, setTimeout: (fn, ms) => { const id = ++sequence; timers.set(id, { fn, ms }); return id }, clearTimeout: id => timers.delete(id) })
  const panel = window.ExtensionPanel.create({ deviceDialog, syncAuthFocus, lang: () => 'zh', redraw() {}, confirm: () => agree, invoke: async (command, args) => {
    calls.push({ command, args })
    if (command === 'extension_view') return { installed, star, runtime: runtimeInfo, node: null, root: 'X:/合成/.fish-dock' }
    if (command === 'extension_auth') { star = auth ? await auth(args) : { verified, authenticated, configured, pending: { userCode: 'TEST-CODE' } }; return star }
    if (command === 'settings_open_site') return open ? open(args) : undefined
    if (command === 'extension_catalog') {
      if (offline) throw new Error('商店暂时离线')
      return { cached: true, error: '当前离线，使用缓存', catalog: { plugins: entries || [{ id: 'fish.ofm', name: '<OFM>', version: '2.5.3', description: '模型', authors: ['甲作者', '乙作者'], note: '此插件于 10 月 25 日暂停维护', hostApi: 1, platform: 'windows-x86_64', bytes, dependencies: ['Node 24'] }] } }
    }
    if (command === 'extension_manage') return manage ? manage(args) : { installed, node: 'X:/node.exe' }
    if (command === 'extension_thirdparty') return third ? third(args) : { hits: [], hidden: 0 }
  } })
  return { panel, calls, timers }
}
test('已登录但 Star 待复核时保留账号身份并自动检查', async () => {
  const { panel, calls, timers } = make({ authenticated: true, verified: false, initialStar: { login: 'fixture-user', needsStarCheck: true }, auth: async () => ({ authenticated: true, verified: true, login: 'fixture-user', pending: null }) })
  await panel.load(); assert.match(panel.render(), /GitHub：fixture-user/); assert.match(panel.render(), /复核/)
  assert.equal(timers.size, 1); const [id, timer] = [...timers][0]; timers.delete(id); await timer.fn()
  assert.ok(calls.some(c => c.command === 'extension_auth' && c.args.action === 'check'))
  assert.match(panel.render(), /主仓 Star 已验证/)
})
test('已登录的访问令牌到期时自动续期，不重新打开设备登录', async () => {
  const { panel, calls, timers } = make({ initialStar: { login: 'fixture-user', needsRefresh: true }, auth: async () => ({ authenticated: true, verified: true, login: 'fixture-user', pending: null }) })
  await panel.load(); assert.equal(timers.size, 1); const [id, timer] = [...timers][0]; timers.delete(id); await timer.fn()
  assert.ok(calls.some(c => c.command === 'extension_auth' && c.args.action === 'check'))
  assert.equal(calls.some(c => c.command === 'settings_open_site'), false)
})
test('临时网络失败保留已登录界面，自动复核按退避间隔重试', async () => {
  const { panel, timers } = make({ initialStar: { login: 'fixture-user', needsRefresh: true }, auth: async () => { throw Error('合成网络超时') } })
  await panel.load(); assert.equal(timers.size, 1); const [id, timer] = [...timers][0]; timers.delete(id); await timer.fn()
  assert.match(panel.render(), /GitHub：fixture-user/); assert.match(panel.render(), /登录资料已保留/)
  assert.ok([...timers.values()].every(t => t.ms >= 59000))
})
// 2026-10-31 三态统一：开＝红「关闭自启动」、关＝蓝「随 dock 启动」、没有本机服务＝绿
// 「无需自启动」（纯装饰按钮，点了不发 IPC）。三态必须同一个 class、同高同内衬。
test('自启动三态：同一个按钮 class、文案与 aria 语义各归各的', async () => {
  const withService = { ...row, plugin: { ...row.plugin, manifest: { ...row.plugin.manifest, behaviors: { autostart: true } } } }
  const on = { ...withService, plugin: { ...withService.plugin, autostart: true } }
  const off = { ...withService, plugin: { ...withService.plugin, autostart: false } }
  const none = { ...row, plugin: { ...row.plugin, manifest: { ...row.plugin.manifest } } } // 没有 behaviors.autostart
  const { panel } = make({ installed: [on, off, none] })
  await panel.load()
  const html = panel.render()
  const hooks = [...html.matchAll(/<button[^>]*class="pbtn sm hook"[^>]*>[^<]*<\/button>/g)].map(m => m[0])
  assert.equal(hooks.length, 3, `三态各一个按钮（实测 ${hooks.length}）`)
  for (const button of hooks) assert.match(button, /class="pbtn sm hook"/, '同一个 class')
  const byState = state => hooks.find(b => b.includes(`data-hook="${state}"`))
  assert.ok(byState('on') && byState('off') && byState('none'), '三个 data-hook 都在')
  assert.match(byState('on'), />关闭自启动</, '开着的动作是「关闭自启动」')
  assert.match(byState('off'), />随 dock 启动</, '关着的动作是「随 dock 启动」')
  assert.match(byState('none'), />无需自启动</, '没有本机服务＝「无需自启动」')
  assert.match(byState('on'), /data-extension-action="autostart"[^>]*aria-pressed="true"/, '开＝aria-pressed true')
  assert.match(byState('off'), /data-extension-action="autostart"[^>]*aria-pressed="false"/, '关＝aria-pressed false')
  // 没有本机服务那一态：还是 <button>，aria-disabled=true，且**不带** data-extension-action
  // —— mount 只给带这个属性的按钮绑 onclick，所以它点了不会有任何 IPC。
  assert.match(byState('none'), /^<button type="button"/, '是真按钮，不是 span')
  assert.match(byState('none'), /aria-disabled="true"/)
  assert.doesNotMatch(byState('none'), /data-extension-action/, '点了不发 IPC（mount 不绑 action）')
  assert.doesNotMatch(byState('none'), /\sdisabled(?=[\s>])/, '不加 disabled：视觉上要是正常按钮，不能被 .pbtn:disabled 灰掉')
  assert.doesNotMatch(html, /hook-off/, '旧的灰字 span 已经删掉')
})

test('旧版没有续期凭据时准确提示重新登录，关闭面板不退出已保存账号', async () => {
  const { panel, calls, timers } = make({ authenticated: false, verified: false, initialStar: { login: 'fixture-user', requiresLogin: true } })
  await panel.load(); assert.match(panel.render(), /需要重新登录/)
  await panel.closeAuth(); assert.equal(timers.size, 0); assert.equal(calls.some(c => c.command === 'extension_auth' && c.args.action === 'logout'), false)
})
test('旧账号无刷新凭据时不在到期前循环复核，关闭后停止计时但不退出', async () => {
  const { panel, calls, timers } = make({ initialStar: { login: 'fixture-user', expiresAt: Date.now() + 120000, refreshable: false, lastCheckedAt: Date.now() } })
  await panel.load(); assert.equal(timers.size, 1); assert.ok([...timers.values()][0].ms >= 119000)
  await panel.closeAuth(); assert.equal(timers.size, 0); assert.equal(calls.some(c => c.command === 'extension_auth'), false)
  await panel.load(true); assert.equal(timers.size, 1); assert.match(panel.render(), /GitHub：fixture-user/)
})
test('明确未 Star 的自动检查显示已登录，不显示断网或退出提示', async () => {
  const { panel, timers } = make({ verified: false, initialStar: { login: 'fixture-user', needsStarCheck: true }, auth: async () => ({ authenticated: true, verified: false, login: 'fixture-user', needsStarCheck: false, lastCheckedAt: Date.now() }) })
  await panel.load(); const [id, timer] = [...timers][0]; timers.delete(id); await timer.fn()
  assert.match(panel.render(), /GitHub：fixture-user/); assert.match(panel.render(), /已登录，请点亮/); assert.doesNotMatch(panel.render(), /暂时连接失败|需要重新登录/)
})
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
test('配置恢复后程序包缺失时，同版本也提供重新安装入口', async () => {
  const { panel } = make({ installed: [{ ...row, packageMissing: true }] }); await panel.load(); await panel.store()
  assert.match(panel.render(), /重新安装/)
  assert.match(panel.render(), /data-extension-action="install"/)
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
  const buttons = Object.fromEntries(['login','check','logout','release','cancel'].map(action=>[action,{dataset:{extensionAuth:action}}]))
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
test('登录仅发起一次内部验证码弹窗并传递真实复制状态', async () => {
  for (const copied of [true, false]) {
    const requests = []
    const { panel } = make({ authenticated: false, verified: false, open: async () => ({ codeCopied: copied }), deviceDialog: request => requests.push(request) })
    await panel.load(); const buttons = await mountAuth(panel); await buttons.login.onclick()
    assert.equal(requests.length, 1); assert.equal(requests[0].userCode, 'TEST-CODE'); assert.equal(requests[0].copied, copied)
    assert.match(panel.render(), /data-extension-auth="release"/)
    await panel.load(true); assert.equal(requests.length, 1)
  }
})
test('手动解除临时状态保留验证，取消验证清理提示且停止轮询', async () => {
  const dialogs = [], pending = { userCode: 'ABCD-EFGH', interval: 5, expiresAt: Date.now() + 900000 }
  const { panel, calls, timers } = make({ authenticated: false, verified: false, deviceDialog: value => dialogs.push(value), auth: async ({ action }) => ({ authenticated: false, verified: false, pending: action === 'cancel' ? null : pending }) })
  await panel.load(); const buttons = await mountAuth(panel); await buttons.login.onclick()
  await buttons.release.onclick(); assert.equal(timers.size, 1); assert.match(panel.render(), /ABCD-EFGH/)
  await buttons.cancel.onclick(); assert.equal(timers.size, 0); assert.equal(dialogs.at(-1), null)
  assert.ok(calls.some(c => c.command === 'extension_auth' && c.args.action === 'release'))
  assert.doesNotMatch(panel.render(), /ABCD-EFGH/)
})
test('重新打开设备页面使用实际复制状态并再次展示内部提示', async () => {
  const dialogs = [], { panel } = make({ authenticated: false, verified: false, open: async () => ({ codeCopied: true }), deviceDialog: value => dialogs.push(value) })
  await panel.load(); const buttons = await mountAuth(panel); await buttons.login.onclick()
  const site = { dataset: { extensionSite: 'githubDevice' } }
  panel.mount({ querySelector: () => ({ querySelector: () => ({}), querySelectorAll: selector => selector === '[data-extension-site]' ? [site] : [] }) })
  await site.onclick(); assert.equal(dialogs.length, 2); assert.equal(dialogs[1].copied, true)
})
test('依赖来源和版本可见且开发版不声称宿主已提供', async () => {
  const { panel } = make({ runtimeInfo: { installed: false, source: 'path', path: 'D:/中文 Node/node.exe', version: 'v24.13.0', candidates: [], warning: null } })
  await panel.load(); const html = panel.render(); assert.match(html, /系统 PATH/); assert.match(html, /v24\.13\.0/); assert.match(html, /开发构建/)
})
test('缓存说明与作者数组可见，维护者行已去掉', async () => {
  const { panel } = make(); await panel.load(); await panel.store()
  const html = panel.render()
  assert.match(html, /缓存/); assert.match(html, /&lt;OFM&gt;/)
  assert.match(html, /作者/); assert.match(html, /甲作者、乙作者/); assert.match(html, /暂停维护/)
  assert.doesNotMatch(html, /维护者|Maintainer/)
  assert.doesNotMatch(html, /<OFM>/)
})
test('作者单/多元素与 note 有无的四种组合都不渲染 undefined', async () => {
  const base = { id: 'fish.ofm', name: 'OFM', version: '2.5.3', description: '模型', hostApi: 1, platform: 'windows-x86_64', bytes: 1024, dependencies: [] }
  for (const authors of [['FISH'], ['Elysia395', 'FISH']]) {
    for (const note of [undefined, '此插件于 10 月 25 日暂停维护']) {
      const { panel } = make({ entries: [{ ...base, authors, ...(note ? { note } : {}) }] })
      await panel.load(); await panel.store()
      const html = panel.render()
      const where = `authors=${authors.length} note=${Boolean(note)}`
      assert.doesNotMatch(html, /undefined/, where)
      assert.ok(html.includes(`<dd>${authors.join('、')}</dd>`), `作者按「A、B」渲染（${authors.join('、')}）`)
      assert.equal((html.match(/class="extension-note"/g) || []).length, note ? 1 : 0, `note 有无与说明行一一对应（${where}）`)
    }
  }
})
// #f3：同一版号重打的包（都是 2.0.0、摘要不同）也算有更新。以前只比 x.y.z，
// 2.0.0 对 2.0.0 恒为 false——插件卡永远写「已安装」，用户根本点不到「更新」。
test('同版本重打的包按摘要判定为有更新，摘要相同才写已安装', async () => {
  const entry = { id: 'fish.ofm', name: 'OFM', version: '2.0.0', description: '模型', authors: ['FISH'], hostApi: 1, platform: 'windows-x86_64', bytes: 4096, dependencies: [] }
  const installed = digest => ({ plugin: { manifest: { ...row.plugin.manifest, version: '2.0.0' }, digest, enabled: true }, running: false })
  const rebuilt = make({ installed: [installed('a'.repeat(64))], entries: [{ ...entry, sha256: 'b'.repeat(64) }] })
  await rebuilt.panel.load(); await rebuilt.panel.store()
  assert.match(rebuilt.panel.render(), /有更新 v2\.0\.0/, '同版本、不同摘要必须提示有更新')
  assert.match(rebuilt.panel.render(), /data-extension-action="install"/)
  const same = make({ installed: [installed('b'.repeat(64))], entries: [{ ...entry, sha256: 'b'.repeat(64) }] })
  await same.panel.load(); await same.panel.store()
  assert.doesNotMatch(same.panel.render(), /有更新 v2\.0\.0/, '摘要一致就是这一版，不该提示更新')
  assert.match(same.panel.render(), /已安装/)
})
// #f3：安装崩在写注册表之前，包已经在盘上、注册表里却没有这一行。以前这张卡
// 直接消失，用户看到的是「装完了，但更新拿不到」，也没有任何可点的补救。
test('安装未完成的那一版要留在插件页：只给重试安装，不给启用与卸载', async () => {
  const pending = { plugin: { manifest: { ...row.plugin.manifest, version: '2.0.0' }, digest: 'c'.repeat(64), enabled: false, pending: true }, running: false, packageMissing: false, error: '安装未完成：这一版已经落地但没登记，点「重试安装」补上' }
  const { panel, calls } = make({ installed: [pending] })
  await panel.load()
  const html = panel.render()
  assert.match(html, /安装未完成/)
  assert.match(html, /重试安装/)
  assert.match(html, /data-extension-action="install"/)
  assert.doesNotMatch(html, /data-extension-action="uninstall"/, '没有注册行就没有卸载')
  assert.doesNotMatch(html, /data-extension-action="enabled"/, '没有注册行就没有启用')
  await panel.action('install', 'fish.ofm')
  assert.equal(calls.some(c => c.command === 'extension_manage' && c.args.action === 'install'), false, '目录里没有官方目录条目时不该发安装请求')
})
// #f3：宿主回滚一次装不上的安装时会留下 notice（哪一版、为什么、改用哪条路），
// 但前端从来没渲染它——用户因此完全看不到「装失败」。
test('安装被回滚时把哪一版没装成、为什么显示在插件卡上', async () => {
  const failed = { ...row, notice: { version: '2.0.0', reason: '数据 schema 变化需要专用迁移适配器', hint: '改用 npm 安装：npm install --no-save @fish-under-sea/fish-dock-plugin-ofm@latest', at: 1793287000000 } }
  const { panel } = make({ installed: [failed] })
  await panel.load()
  const html = panel.render()
  assert.match(html, /安装 v2\.0\.0 未完成/)
  assert.match(html, /数据 schema 变化需要专用迁移适配器/)
  assert.match(html, /fish-dock-plugin-ofm/)
  assert.match(html, /role="alert"/)
})
// #t8：清理失败是「装上了，旧文件没清干净」，不能写成「安装未完成」——两句意思正好
// 相反，混用会让用户以为插件没装上。
test('清理失败的提示是警告而不是安装未完成', async () => {
  const warned = { ...row, notice: { version: '2.0.0', reason: '过期版本目录清理失败：1.0.0', hint: '', at: 1793287000000, cleanup: true } }
  const { panel } = make({ installed: [warned] })
  await panel.load()
  const html = panel.render()
  assert.match(html, /v2\.0\.0 已安装，但旧版本文件没清干净/)
  assert.match(html, /过期版本目录清理失败/)
  assert.doesNotMatch(html, /未完成/, '清理警告不许写成安装未完成')
  assert.match(html, /role="status"/, '警告不是打断式报错')
})

test('第三方检索：合规包列出可安装，安装按钮同卡片几何', async () => {
  const { panel } = make({ third: () => ({ hits: [
    { name: 'fish-dock-plugin-weather', version: '1.0.0', description: '天气', publisher: 'acme', updated: '2026-10-01' },
    { name: '@scope/fish-dock-plugin-clock', version: '2.0.0', description: '时钟', publisher: 's', updated: '2026-09-09' },
  ], hidden: 0 }) })
  await panel.load()
  await panel.thirdparty('fish-dock-plugin')
  const html = panel.render()
  assert.match(html, /fish-dock-plugin-weather/)
  assert.match(html, /@scope\/fish-dock-plugin-clock/)
  assert.equal((html.match(/data-extension-third="install"/g) || []).length, 2)
  assert.match(html, /class="pbtn sm"/, '安装按钮沿用 pbtn sm 高度')
  assert.doesNotMatch(html, /已隐藏/)
})
test('第三方检索：不合规包被隐藏，界面写出被隐藏的数量而不是静默空列表', async () => {
  const { panel } = make({ third: () => ({ hits: [], hidden: 3 }) })
  await panel.load()
  await panel.thirdparty('lodash')
  const html = panel.render()
  assert.match(html, /已隐藏 3 个不符合 fish-dock-plugin-\* 命名的包/)
  assert.doesNotMatch(html, /data-extension-third="install"/)
})
test('第三方检索：提示行是 note 小字，合规结果与隐藏提示可并存', async () => {
  const { panel } = make({ third: () => ({ hits: [{ name: 'fish-dock-plugin-x', version: '1.0.0', description: '', publisher: '', updated: '' }], hidden: 2 }) })
  await panel.load()
  await panel.thirdparty('fish-dock')
  const html = panel.render()
  assert.match(html, /<p role="status" class="note">已隐藏 2 个不符合 fish-dock-plugin-\* 命名的包<\/p>/)
  assert.match(html, /data-extension-third="install"/)
})

