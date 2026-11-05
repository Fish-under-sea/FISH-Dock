const assert = require('node:assert/strict')
const fs = require('node:fs')
const vm = require('node:vm')
const path = require('node:path')
const { test } = require('node:test')
function make({ agree = true, offline = false, installed = [], manage, verified = true, authenticated = true, configured = true, bytes = 4096, auth, open, deviceDialog = () => {}, runtimeInfo = null, syncAuthFocus = async () => {}, initialStar = {}, entries = null, third = null, progressState = null, fetch: fetchRecord = null } = {}) {
  const calls = [], window = {}, timers = new Map(); let star = { verified, authenticated, configured, ...initialStar }, sequence = 0
  vm.runInNewContext(fs.readFileSync(path.join(__dirname, '../src/pet/extension-panel.js'), 'utf8'), { window, Date, setTimeout: (fn, ms) => { const id = ++sequence; timers.set(id, { fn, ms }); return id }, clearTimeout: id => timers.delete(id) })
  const panel = window.ExtensionPanel.create({ deviceDialog, syncAuthFocus, lang: () => 'zh', redraw() {}, confirm: () => agree, invoke: async (command, args) => {
    calls.push({ command, args })
    if (command === 'extension_view') return { installed, star, runtime: runtimeInfo, node: null, root: 'X:/合成/.fish-dock', fetch: fetchRecord }
    if (command === 'extension_auth') { star = auth ? await auth(args) : { verified, authenticated, configured, pending: { userCode: 'TEST-CODE' } }; return star }
    if (command === 'settings_open_site') return open ? open(args) : undefined
    if (command === 'extension_catalog') {
      if (offline) throw new Error('商店暂时离线')
      return { cached: true, error: '当前离线，使用缓存', catalog: { plugins: entries || [{ id: 'fish.magpie', name: '<OFM>', version: '2.5.3', description: '模型', authors: ['甲作者', '乙作者'], note: '此插件于 10 月 25 日暂停维护', hostApi: 1, platform: 'windows-x86_64', bytes, dependencies: ['Node 24'] }] } }
    }
    if (command === 'extension_manage') return manage ? manage(args) : { installed, node: 'X:/node.exe' }
    if (command === 'extension_progress') return typeof progressState === 'function' ? progressState() : progressState
    if (command === 'extension_cancel') return { id: args?.id, active: false }
    if (command === 'extension_thirdparty') return third ? third(args) : { hits: [], hidden: 0 }
    if (command === 'taskbar_app') return true
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
// 2026-10-31 三态统一：开＝红「关闭自启动」、关＝蓝「随 Dock 启动」、没有本机服务＝绿
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
  assert.match(byState('off'), />随 Dock 启动</, '关着的动作是「随 Dock 启动」')
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
const row = { plugin: { manifest: { id: 'fish.magpie', name: 'OFM', version: '2.5.3', capabilities: ['network'], pages: [], storage: { directory: '.fishplugin-magpie' } }, enabled: true }, running: false, error: '进程退出 <trace>' }
// T01：探针这一轮没应答只报「未响应」，不能说成「启动失败」——它进程还在，
// 下一轮重绘自己会好；说成失败会让人去查一个不存在的崩溃。
test('探针没应答报「未响应，正在重试」，不冒充启动失败', async () => {
  const busy = { plugin: { manifest: { id: 'fish.magpie', name: 'OFM', version: '2.5.3', capabilities: ['network'], pages: [], storage: { directory: '.fishplugin-magpie' } }, enabled: true }, running: true, unresponsive: true }
  const { panel } = make({ offline: true, installed: [busy] })
  await panel.load(); await panel.store()
  const html = panel.render()
  assert.match(html, /未响应，正在重试/)
  assert.doesNotMatch(html, /启动失败/)
  assert.doesNotMatch(html, /运行中</, '未响应时不该同时说运行中')
})
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
  await panel.action('install', 'fish.magpie')
  const manage = calls.find(c => c.command === 'extension_manage')
  assert.equal(manage.args.action, 'install')
  assert.equal(manage.args.enabled, true)
})

test('卸载需实际确认，安装不再确认（2026-11-01 用户要求）', async () => {
  const denied = make({ agree: false, installed: [row] })
  await denied.panel.load(); await denied.panel.store()
  await denied.panel.action('uninstall', 'fish.magpie')
  assert.equal(denied.calls.filter(c => c.command === 'extension_manage').length, 0, '卸载拒绝确认后不得下发')
  const allowed = make({ agree: true, installed: [row] })
  await allowed.panel.load(); await allowed.panel.store()
  await allowed.panel.action('install', 'fish.magpie')
  assert.equal(allowed.calls.filter(c => c.command === 'extension_manage').length, 1, '安装直接下发、不弹确认')
})
test('操作失败保留原状态并可重试，不展示凭空成功', async () => {
  let failures = 1
  const { panel, calls } = make({ installed: [row], manage: async () => { if (failures--) throw new Error('配置无效'); return { installed: [{ ...row, running: true, error: null }] } } })
  await panel.load(); await panel.action('enabled', 'fish.magpie')
  assert.match(panel.render(), /配置无效/)
  await panel.action('enabled', 'fish.magpie')
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
  await panel.action('install', 'fish.magpie')
  assert.equal(calls.filter(c => c.command === 'extension_manage').length, 0)
  assert.match(panel.render(), /⭐ 去 Star/)
})
test('提供登录及复核入口，安装结果缺少许可字段时仍保留宿主已有许可', async () => {
  const locked = make({ verified: false }); await locked.panel.load()
  assert.match(locked.panel.render(), /data-extension-auth="login"/)
  assert.match(locked.panel.render(), /data-extension-auth="check"/)
  const { panel, calls } = make(); await panel.load(); await panel.store()
  await panel.action('install', 'fish.magpie'); await panel.action('install', 'fish.magpie')
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
  // T11：版本与「有更新」合成一条文案（V1.0.0 → v2.5.3），不再另起一个「有更新」标签。
  assert.match(panel.render(), /v1\.0\.0 → v2\.5\.3/, '有更新写成版本迁移，一条文案')
  assert.doesNotMatch(panel.render(), /有更新 v/, '旧的「有更新 v…」标签已删')
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

// #14-7 / T05：标签页与运行态解耦。以前 pages() 只认 r.running，插件进程一退出
// 标签页就消失（Dock 重开设置面板时忽隐忽现）。现在只看「已安装 + 已启用 + 已解锁」，
// 没运行也留着标签页，由页面显示错误态；插件卡另给「打开插件日志」一步开目录。
test('插件未运行时标签页仍在，功能页按钮不再禁用，插件卡可一步打开插件日志', async () => {
  const page = { id: 'main', title: 'Clash' }
  const stopped = { plugin: { manifest: { ...row.plugin.manifest, enabled: true, pages: [page] }, enabled: true }, running: false, error: '插件进程已退出（退出码：1） · 日志：X:/合成/.fish-dock/host/plugin-logs/fish.magpie.log', logPath: 'X:/合成/.fish-dock/host/plugin-logs/fish.magpie.log' }
  const disabled = { ...stopped, plugin: { ...stopped.plugin, enabled: false } }
  const pending = { ...stopped, plugin: { ...stopped.plugin, pending: true } }
  const { panel, calls } = make({ installed: [stopped, disabled, pending] })
  await panel.load()
  const pages = panel.pages()
  assert.deepEqual(Array.from(pages, p => p.key), ['fish.magpie/main'], '没运行也要有标签页；停用与安装未完成的不算')
  assert.equal(pages[0].running, false, '页面要知道插件没在跑')
  assert.match(pages[0].error, /退出码/, '错误态文案沿用宿主的 error（带退出码与日志路径）')
  const html = panel.render()
  assert.doesNotMatch(html, /data-extension-page="fish\.magpie\/main"[^>]*\sdisabled(?=[\s>])/, '未运行不再把功能页按钮禁用')
  assert.match(html, /data-extension-page="fish\.magpie\/main"[^>]*插件未运行：打开后会显示错误态与「启动」按钮/)
  // 插件卡上的「打开插件日志」：一步打开 plugin-logs/（宿主 extension_manage action=logs）
  assert.match(html, /data-extension-action="logs" data-extension-id="fish\.magpie"/, '插件卡上有日志入口')
  await panel.action('logs', 'fish.magpie')
  const manage = calls.filter(c => c.command === 'extension_manage').at(-1)
  assert.equal(manage.args.action, 'logs')
  assert.equal(manage.args.id, 'fish.magpie')
  assert.equal(manage.args.confirm, true)
})

// 宿主还没接线 action=logs 时不许留一颗死按钮：退回同一个 open-directory 能力
// （taskbar_app 的「launch」，ShellExecute 打开目录），路径取宿主给的 logPath。
test('打开插件日志：宿主还没有 logs 动作时退回同一条 open-directory 能力', async () => {
  const page = { id: 'main', title: 'Clash' }
  const stopped = { plugin: { manifest: { ...row.plugin.manifest, enabled: true, pages: [page] }, enabled: true }, running: false, logPath: 'X:/合成/.fish-dock/host/plugin-logs/fish.magpie.log' }
  const { panel, calls } = make({ installed: [stopped], manage: async () => { throw new Error('插件管理操作无效') } })
  await panel.load()
  await panel.action('logs', 'fish.magpie')
  const fallback = calls.filter(c => c.command === 'taskbar_app').at(-1)
  assert.equal(fallback.args.what, 'launch')
  assert.equal(fallback.args.path, 'X:/合成/.fish-dock/host/plugin-logs', '目录而非文件')
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
  const base = { id: 'fish.magpie', name: 'OFM', version: '2.5.3', description: '模型', hostApi: 1, platform: 'windows-x86_64', bytes: 1024, dependencies: [] }
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
  const entry = { id: 'fish.magpie', name: 'OFM', version: '2.0.0', description: '模型', authors: ['FISH'], hostApi: 1, platform: 'windows-x86_64', bytes: 4096, dependencies: [] }
  const installed = digest => ({ plugin: { manifest: { ...row.plugin.manifest, version: '2.0.0' }, digest, enabled: true }, running: false })
  const rebuilt = make({ installed: [installed('a'.repeat(64))], entries: [{ ...entry, sha256: 'b'.repeat(64) }] })
  await rebuilt.panel.load(); await rebuilt.panel.store()
  assert.match(rebuilt.panel.render(), /class="extension-version" data-update="1">v2\.0\.0</, '同版本、不同摘要必须在这一版上标出可更新')
  assert.match(rebuilt.panel.render(), /data-extension-action="install"/)
  const same = make({ installed: [installed('b'.repeat(64))], entries: [{ ...entry, sha256: 'b'.repeat(64) }] })
  await same.panel.load(); await same.panel.store()
  assert.doesNotMatch(same.panel.render(), /data-update="1"/, '摘要一致就是这一版，不该提示更新')
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
  await panel.action('install', 'fish.magpie')
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
  assert.match(html, /本页已隐藏 3 个不符合 fish-dock-plugin-\* 命名的包/)
  assert.doesNotMatch(html, /data-extension-third="install"/)
})
test('第三方检索：提示行是 note 小字，合规结果与隐藏提示可并存', async () => {
  const { panel } = make({ third: () => ({ hits: [{ name: 'fish-dock-plugin-x', version: '1.0.0', description: '', publisher: '', updated: '' }], hidden: 2 }) })
  await panel.load()
  await panel.thirdparty('fish-dock')
  const html = panel.render()
  assert.match(html, /<p role="status" class="note">本页已隐藏 2 个不符合 fish-dock-plugin-\* 命名的包<\/p>/)
  assert.match(html, /data-extension-third="install"/)
})
// t29：安装/更新期间显示进度弹窗（插件名 + 阶段 + 百分比 + 已下载/总量 + 取消），
// 官方与第三方共用同一套渲染；取消按钮发 extension_cancel（真取消由宿主下载循环处理）。
test('下载进度弹窗：阶段/百分比/取消，官方与第三方共用', async () => {
  const entry = { id: 'fish.magpie', name: 'Magpie', version: '2.1.0', description: '', authors: [], hostApi: 1, platform: 'windows-x86_64', bytes: 4096, dependencies: [] }
  let release
  const gate = new Promise(r => { release = r })
  const { panel, timers, calls } = make({ entries: [entry], manage: () => gate, progressState: () => ({ items: [{ id: 'fish.magpie', phase: '下载', received: 50, total: 100, active: true }] }) })
  await panel.load()
  await panel.store()
  const pending = panel.action('install', 'fish.magpie')
  // 第一次轮询（50ms 后）拿回进度快照并重画。
  const first = [...timers.entries()].find(([, t]) => t.ms === 50)
  assert.ok(first, 'run 会排一次进度轮询')
  timers.delete(first[0]); await first[1].fn()
  const html = panel.render()
  assert.match(html, /ext-progress-backdrop/)
  assert.match(html, /Magpie/)
  assert.match(html, /下载中/)
  assert.match(html, /50%/)
  assert.match(html, /50 B \/ 100 B/)
  assert.match(html, /data-extension-cancel="fish\.magpie"/)
  // 取消按钮：发出 extension_cancel，带当前插件 id。
  const cancelBtn = { dataset: { extensionCancel: 'fish.magpie' } }
  panel.mount({ querySelector: () => ({ querySelectorAll: sel => sel === '[data-extension-cancel]' ? [cancelBtn] : [], querySelector: sel => sel === '#extension-third-query' ? null : ({}) }) })
  await cancelBtn.onclick()
  assert.ok(calls.some(c => c.command === 'extension_cancel' && c.args.id === 'fish.magpie'), '取消要通知宿主')
  release({ installed: [] })
  await pending
  // 不在下载时（无 active 进度）没有弹窗。
  assert.doesNotMatch(panel.render(), /ext-progress-backdrop/)
})

// ── t44：显示层改造（阶段文案 / 字节进位 / 进度不回弹 / 粒子层 DOM）──────────
function loadPanelModule() {
  const window = {}
  vm.runInNewContext(fs.readFileSync(path.join(__dirname, '../src/pet/extension-panel.js'), 'utf8'), { window })
  return window.ExtensionPanel
}
const nextTimer = async (timers, ms) => {
  const hit = [...timers.entries()].find(([, t]) => t.ms === ms)
  assert.ok(hit, `应有 ${ms}ms 的定时器`)
  timers.delete(hit[0])
  await hit[1].fn()
}

test('t44 formatBytes：1024 进制自动进位，B 整数，KB/MB 两位小数', () => {
  const { formatBytes } = loadPanelModule()
  assert.equal(formatBytes(0), '0 B')
  assert.equal(formatBytes(1023), '1023 B')
  assert.equal(formatBytes(1024), '1.00 KB')
  assert.equal(formatBytes(1048576), '1.00 MB')
  assert.equal(formatBytes(1073741824), '1.00 GB')
  // 宿主 Content-Length 的真实量级（自己核算：30859277/1024/1024=29.43，30957961=29.52）。
  assert.equal(formatBytes(30859277), '29.43 MB')
  assert.equal(formatBytes(30957961), '29.52 MB')
})

test('t44 阶段文案是「动词+中」，不是裸名词', async () => {
  const { PHASE_WORDS } = loadPanelModule()
  // vm 里造出来的数组是另一个 realm 的原型，用字符串比较避免跨 realm 差异。
  assert.equal(PHASE_WORDS['下载'].join('|'), '下载中|Downloading')
  assert.equal(PHASE_WORDS['校验'].join('|'), '校验中|Verifying')
  assert.equal(PHASE_WORDS['解包'].join('|'), '解包中|Unpacking')
  assert.equal(PHASE_WORDS['提交'].join('|'), '提交中|Committing')
  assert.equal(PHASE_WORDS['卸载'].join('|'), '卸载中|Uninstalling')
  assert.equal(PHASE_WORDS['切换来源'].join('|'), '切换来源|Switching source')
})

test('t44 mergeProgress：换源时百分比单调不减，并标出切换来源', () => {
  const { mergeProgress } = loadPanelModule()
  // 源 1 到 60%
  const first = mergeProgress(undefined, { phase: '下载', received: 600, total: 1000 })
  assert.equal(first.peak, 60)
  assert.equal(first.switching, false)
  // 源 2 从头开始：原始 0%，但显示仍是 60%，阶段切换来源
  const switched = mergeProgress(first, { phase: '下载', received: 0, total: 800 })
  assert.equal(switched.peak, 60, '不得回退到 0')
  assert.equal(switched.raw, 0)
  assert.equal(switched.switching, true)
  // 源 2 涨到 50%：仍低于峰值，继续显示 60%
  const caughtUp = mergeProgress(switched, { phase: '下载', received: 400, total: 800 })
  assert.equal(caughtUp.peak, 60)
  assert.equal(caughtUp.switching, true)
  // 源 2 涨到 75%：超过旧峰值，继续往上走
  const beyond = mergeProgress(caughtUp, { phase: '下载', received: 600, total: 800 })
  assert.equal(beyond.peak, 75)
  assert.equal(beyond.switching, false, '追上以后回到下载中')
})

test('t44 进度不回弹：轮询换源后 DOM 的百分比与阶段不倒退', async () => {
  const entry = { id: 'fish.magpie', name: 'Magpie', version: '2.1.0', description: '', authors: [], hostApi: 1, platform: 'windows-x86_64', bytes: 4096, dependencies: [] }
  let snap = { items: [{ id: 'fish.magpie', phase: '下载', received: 600, total: 1000, active: true }] }
  let release; const gate = new Promise(r => { release = r })
  const { panel, timers } = make({ entries: [entry], manage: () => gate, progressState: () => snap })
  await panel.load(); await panel.store()
  const pending = panel.action('install', 'fish.magpie')
  await nextTimer(timers, 50)
  let html = panel.render()
  assert.match(html, /aria-valuenow="60"/)
  assert.match(html, /60% · 600 B \/ 1000 B/)
  assert.match(html, /下载中/)
  // 换源：新源从 0 开始、总量也变了
  snap = { items: [{ id: 'fish.magpie', phase: '下载', received: 0, total: 800, active: true }] }
  await nextTimer(timers, 150)
  html = panel.render()
  assert.match(html, /aria-valuenow="60"/, '换源不得把进度打回 0')
  assert.match(html, /60%/)
  assert.match(html, /切换来源/)
  // 新源涨过旧峰值
  snap = { items: [{ id: 'fish.magpie', phase: '下载', received: 720, total: 800, active: true }] }
  await nextTimer(timers, 150)
  html = panel.render()
  assert.match(html, /aria-valuenow="90"/)
  assert.match(html, /90% · 720 B \/ 800 B/)
  assert.doesNotMatch(html, /切换来源/)
  release({ installed: [] })
  await pending
})

test('进度卡只有基本条：没有 canvas 粒子层，条与文字仍在（2026-11-01 用户要求）', async () => {
  const entry = { id: 'fish.magpie', name: 'Magpie', version: '2.1.0', description: '', authors: [], hostApi: 1, platform: 'windows-x86_64', dependencies: [], url: 'https://example/x', sha256: 'a'.repeat(64), bytes: 1 }
  let release; const gate = new Promise(r => { release = r })
  const { panel, timers } = make({ entries: [entry], manage: () => gate, progressState: () => ({ items: [{ id: 'fish.magpie', phase: '下载', received: 300, total: 1000, active: true }] }) })
  await panel.load(); await panel.store()
  const pending = panel.action('install', 'fish.magpie')
  await nextTimer(timers, 50)
  const html = panel.render()
  assert.doesNotMatch(html, /ext-bar-fx|canvas/, '粒子层已删除')
  assert.match(html, /<div class="meter"><div class="bar"/, '基本进度条还在')
  assert.match(html, /30% · 300 B \/ 1000 B/, '百分比与字节仍在')
  release({ installed: [] })
  await pending
})

test('t44 卸载：没有宿主进度时显示「卸载中」并给出本地合成的消散进度', async () => {
  const row = { plugin: { manifest: { id: 'fish.magpie', name: 'Magpie', version: '2.1.0', pages: [], storage: { directory: '.plugin.fish.magpie' } }, digest: 'd'.repeat(64), enabled: true, autostart: false, running: true, pending: false, official: true }, running: true }
  let release; const gate = new Promise(r => { release = r })
  const { panel, timers } = make({ installed: [row], manage: () => gate, progressState: () => ({ items: [] }) })
  await panel.load()
  const pending = panel.action('uninstall', 'fish.magpie')
  await nextTimer(timers, 50)
  const html = panel.render()
  assert.match(html, /卸载中/)
  // 2026-11-01：粒子层已删除，不再有 data-mode 的 canvas
  assert.doesNotMatch(html, /data-mode="out"/)
  release({ installed: [] })
  await pending
})



// t45：下载失败改成「一句人话 + 可展开详情」，不再把三源诊断拼成一堵墙。
test('下载失败：一句人话 + 三个来源逐行，原始诊断留在折叠区', async () => {
  const raw = {
    fishFailure: true, stage: 'download',
    message: '三个下载来源都没成功，插件没能更新。', hint: '检查网络或代理后重试。',
    sources: [
      { origin: 'npm 官方源', short: '版本与目录不一致', error: 'npm 包里的插件归档大小与官方目录不一致（30859277 ≠ 30957961）' },
      { origin: 'npm 镜像', short: '跳转被拒绝', error: '插件下载跳转不属于官方发行资源服务' },
      { origin: 'GitHub 发行包', short: '连接超时', error: '插件下载中断或超过大小限制' },
    ],
  }
  const { panel } = make({ manage: async () => { throw JSON.stringify(raw) } })
  await panel.load(); await panel.store()
  await panel.action('install', 'fish.magpie')
  const html = panel.render()
  assert.match(html, /class="extension-failure"/)
  assert.match(html, /<strong>下载失败<\/strong>/)
  assert.match(html, /检查网络或代理后重试/)
  const main = html.match(/<p class="extension-failure-main"[^>]*>([\s\S]*?)<\/p>/)[1]
  assert.ok(!main.includes('；'), '主文案不许拼三源')
  assert.ok(!main.includes('npm 官方源'), '主文案不逐源')
  assert.ok(main.replace(/<[^>]+>/g, '').length <= 40, '主文案一句人话')
  const list = html.match(/<ul class="extension-failure-sources">([\s\S]*?)<\/ul>/)[1]
  assert.equal([...list.matchAll(/<li>/g)].length, 3, '折叠区三行')
  assert.match(list, /npm 官方源<\/span> · <span class="extension-failure-short">版本与目录不一致/)
  assert.match(list, /npm 镜像<\/span> · <span class="extension-failure-short">跳转被拒绝/)
  assert.match(list, /GitHub 发行包<\/span> · <span class="extension-failure-short">连接超时/)
  assert.match(list, /title="npm 包里的插件归档大小与官方目录不一致（30859277 ≠ 30957961）"/, '原始串在 title')
  assert.match(html, /三个来源都没成功/)
})
test('校验失败/卸载失败按阶段给各自的人话，长技术串进折叠区', async () => {
  for (const [stage, title] of [['verify', '校验失败'], ['uninstall', '卸载失败']]) {
    const raw = { fishFailure: true, stage, kind: 'verify', message: '包可能损坏，请重试。', detail: '插件资源校验失败：backend.mjs 与清单摘要不符（很长很长的原始诊断）', sources: [] }
    const { panel } = make({ installed: [row], manage: async () => { throw JSON.stringify(raw) } })
    await panel.load(); await panel.store()
    await panel.action(stage === 'uninstall' ? 'uninstall' : 'install', 'fish.magpie')
    const html = panel.render()
    assert.match(html, new RegExp(`<strong>${title}</strong>`), `${stage} 用「${title}」`)
    assert.match(html, /技术细节/, '无逐源时折叠区给「技术细节」')
    assert.match(html, /插件资源校验失败：backend\.mjs/, '原始串在折叠区')
  }
})

// t45 验收 ④（静态兜底）：失败块样式必须最高 40vh、超出滚动、含内衬用 border-box。
test('失败块样式：最高 40vh、超出滚动（t45 验收 ④）', () => {
  const css = fs.readFileSync(path.join(__dirname, '../src/pet/extensions.css'), 'utf8')
  const block = css.match(/#island-settings \.extension-failure \{[^}]*\}/)
  assert.ok(block, '有 .extension-failure 样式块')
  assert.match(block[0], /max-height:\s*40vh/)
  assert.match(block[0], /overflow:\s*auto/)
  assert.match(block[0], /box-sizing:\s*border-box/)
})

// ── t47：粒子只在“前沿”汇聚，并入即消失 ────────────────────────────────────
test('t47 particleFlight：阈值铺满 0→100%，前沿约 12 个在飞，并入即不可见', () => {
  const { particleFlight, FLIGHT_WINDOW } = loadPanelModule()
  const N = 48
  for (const pct of [0.3, 0.5, 0.7]) {
    const flying = []
    for (let i = 0; i < N; i++) { const f = particleFlight(pct, i, N, 'in'); if (f) flying.push({ i, ...f }) }
    assert.ok(flying.length >= 10 && flying.length <= 15, `pct=${pct} 前沿应有 10~15 个粒子，实际 ${flying.length}`)
    for (const f of flying) assert.ok(f.slot > pct + 1e-9 && f.slot <= pct + FLIGHT_WINDOW + 1e-9, `pct=${pct} 粒子 ${f.i} 的列 ${f.slot.toFixed(3)} 不在前沿附近`)
  }
  // 70%：所有列 <= 70% 的粒子必须已并入（返回 null → 不绘制，已填区域干净）
  for (let i = 0; i < N; i++) { const f = particleFlight(0.7, i, N, 'in'); if (f) assert.ok(f.slot > 0.7) }
  // 41% 不再“全部并入”：仍有前沿粒子，且最后一个（slot≈0.99）还没出发
  const at41 = Array.from({ length: N }, (_, i) => particleFlight(0.41, i, N, 'in')).filter(Boolean).length
  assert.ok(at41 >= 10 && at41 <= 15, `41% 应有前沿粒子，实际 ${at41}`)
  assert.equal(particleFlight(0.41, 47, N, 'in'), null, '41% 时最后一个粒子还没出发')
  // 90%：只剩尾部少数还在飞，绝不再是整条散点
  const at90 = Array.from({ length: N }, (_, i) => particleFlight(0.9, i, N, 'in')).filter(Boolean).length
  assert.ok(at90 >= 1 && at90 <= 15, `90% 只剩尾部粒子，实际 ${at90}`)
  // 卸载镜像：50% / 20% 时同样约 12 个从前沿剥离，不是整条散点
  const outFlying = Array.from({ length: N }, (_, i) => particleFlight(0.5, i, N, 'out')).filter(Boolean).length
  assert.ok(outFlying >= 10 && outFlying <= 15, `卸载 50% 前沿应有 10~15 个，实际 ${outFlying}`)
  const outEarly = Array.from({ length: N }, (_, i) => particleFlight(0.2, i, N, 'out')).filter(Boolean).length
  assert.ok(outEarly >= 10 && outEarly <= 15)
})

// ── T11：插件页卡面重做 ───────────────────────────────────────────────────────
// 设计约束写在 app/src/pet/extensions.css 的文件头：一张卡＝一行在跑的服务，
// 左身份（名字）→ 中信号（LED + 运行态 + 版本迁移）→ 右操作（一颗实心）。
const carded = (extra = {}) => {
  const page = { id: 'main', title: 'Clash 代理' }
  const enabled = { plugin: { manifest: { ...row.plugin.manifest, version: '1.0.0', enabled: true, pages: [page] }, enabled: true, digest: 'a'.repeat(64) }, running: true, logPath: 'X:/合成/.fish-dock/host/plugin-logs/fish.magpie.log' }
  return make({
    installed: [enabled],
    entries: [{ id: 'fish.magpie', name: 'OFM', version: '2.5.3', description: '模型', authors: ['FISH'], hostApi: 1, platform: 'windows-x86_64', bytes: 4096, dependencies: [], sha256: 'b'.repeat(64) }],
    fetch: { 'fish.magpie': { origin: 'npmRegistry', elapsedMs: 1412 } },
    ...extra,
  })
}
// 点「检查更新」= 读目录，但停在「已安装」视图：T11 要看的是插件卡本身，不是商店列表。
const refresh = async panel => {
  const buttons = { '[data-extension-refresh]': {}, '[data-extension-runtime]': {} }
  panel.mount({ querySelector: () => ({ querySelector: key => buttons[key], querySelectorAll: () => [] }) })
  await buttons['[data-extension-refresh]'].onclick()
}
test('T11 每张卡最多一颗实心主按钮：只有「更新」是 .al，卸载与日志收进「更多」', async () => {
  const { panel } = carded(); await panel.load(); await refresh(panel)
  const html = panel.render()
  const solid = [...html.matchAll(/<button[^>]*class="pbtn sm al"[^>]*>/g)].map(m => m[0])
  assert.equal(solid.length, 1, `一张卡只许一颗实心（实测 ${solid.length}）`)
  assert.match(solid[0], /data-extension-action="install" data-extension-id="fish\.magpie"/, '实心只给「更新」')
  assert.match(html, /class="pbtn sm al"[^>]*>更新</, '实心那颗写的是「更新」')
  // 启停/自启动是次级：既不是 .al，也不是旧的三色实心 hook。
  assert.match(html, /class="pbtn sm hook"/, '自启动仍是同一个按钮 class（三态统一不改）')
  assert.doesNotMatch(html, /class="pbtn sm al" data-extension-action="autostart"/, '自启动不得是实心')
  assert.doesNotMatch(html, /class="pbtn sm al" data-extension-action="enabled"/, '停用不得是实心')
  // 卸载与打开日志在「更多」里，不在主操作行。
  const more = html.split('<details class="extension-more">')[1]?.split('</details>')[0] || ''
  assert.match(more, /data-extension-action="uninstall"/, '卸载进「更多」')
  assert.match(more, /data-extension-action="logs"/, '打开插件日志进「更多」')
  // T-C / #63：行式动作列表——两行都整行可点（.extension-more-row），日志在上、
  // 卸载在下；卸载常态就是危险色（class 决定颜色，不必悬停）。
  const rows = [...more.matchAll(/<button[^>]*class="([^"]*extension-more-row[^"]*)"[^>]*data-extension-action="(\w+)"[^>]*>([^<]*)</g)].map(m => ({ classes: m[1], action: m[2], label: m[3] }))
  assert.deepEqual(rows.map(r => r.action), ['logs', 'uninstall'], '日志在上、卸载在下')
  assert.ok(rows.every(r => r.classes.includes('pbtn') && r.classes.includes('sm')), '两行沿用 pbtn sm 的尺寸语言')
  assert.match(rows[1].classes, /extension-danger/, '卸载带危险 class（常态危险色，由 CSS 决定）')
  assert.doesNotMatch(rows[0].classes, /extension-danger/, '日志不是危险动作')
  const actionRow = html.split('<div class="extension-card-actions">')[1]?.split('<details')[0] || ''
  assert.doesNotMatch(actionRow, /data-extension-action="uninstall"/, '主操作行里没有卸载')
  assert.doesNotMatch(actionRow, /data-extension-action="logs"/, '主操作行里没有日志入口')
})
test('T11 状态区只剩一条：LED + 运行态 + 版本迁移，旧的三个并列标签不再出现', async () => {
  const { panel } = carded(); await panel.load(); await refresh(panel)
  const html = panel.render()
  assert.equal((html.match(/class="extension-signal"/g) || []).length, 1, '一张卡一条状态信号')
  assert.equal((html.match(/class="extension-state"/g) || []).length, 1)
  assert.equal((html.match(/class="extension-version"/g) || []).length, 1)
  assert.match(html, /data-signal="running"[^>]*><span class="extension-led" aria-hidden="true"><\/span><span class="extension-state">运行中</, '运行态是这条信号的文字')
  assert.match(html, /<span class="extension-version" data-update="1">v1\.0\.0 → v2\.5\.3<\/span>/, '版本与有更新合成一条迁移文案')
  assert.doesNotMatch(html, /class="extension-update"/, '不再有独立的「有更新」标签')
  assert.doesNotMatch(html, />1\.0\.0</, '版本不再单独裸排')
})
test('T11 元数据下沉为一行次级信息：路径与安装来源同一个 .extension-card-meta', async () => {
  const { panel } = carded(); await panel.load(); await refresh(panel)
  const html = panel.render()
  assert.match(html, /<p class="extension-card-meta"><span class="extension-location mono">\.fishplugin-magpie<\/span><span class="extension-source" role="status">安装来源：npm 官方源 1\.4s<\/span><\/p>/, '路径与来源同一行')
  assert.doesNotMatch(html, /<p class="extension-location/, '路径不再独占正文级一行')
  assert.doesNotMatch(html, /安装来源：[^<]*·/, '来源行不再用中点拼串')
  // 小一档 + 次级色由 extensions.css 的 .extension-card-meta（11.5px / --g4）保证。
})
test('T11 视图是真 tablist：role/aria-selected/roving tabindex，键盘左右与 Home/End 换视图', async () => {
  const { panel, calls } = make({ installed: [row] }); await panel.load()
  let html = panel.render()
  assert.match(html, /<div class="extension-views" role="tablist" aria-label="扩展视图">/, '容器是 tablist 且有可访问名')
  const tabs = [...html.matchAll(/<button[^>]*role="tab"[^>]*>[^<]*<\/button>/g)].map(m => m[0])
  assert.equal(tabs.length, 3, '三个视图各一颗 tab')
  assert.match(tabs[0], /aria-selected="true" tabindex="0"[^>]*data-extension-view="installed"/, '当前视图 aria-selected=true 且 tabindex=0')
  for (const other of tabs.slice(1)) assert.match(other, /aria-selected="false" tabindex="-1"/, '其余 tab tabindex=-1（roving）')
  assert.equal((html.match(/aria-selected="true"/g) || []).length, 1, '同一时刻只有一个 aria-selected=true')
  assert.match(html, /<div class="extension-list" id="extension-view-panel" role="tabpanel" aria-labelledby="extension-tab-installed">/, '内容区是 tabpanel 且指向当前 tab')
  // 键盘：ArrowRight 从「已安装」切到「官方插件」，走的是与点击同一条路（真去读目录）。
  const installedTab = { dataset: { extensionView: 'installed' } }
  const storeTab = { dataset: { extensionView: 'store' } }
  const thirdTab = { dataset: { extensionView: 'thirdparty' } }
  const landing = { focused: false, focus() { this.focused = true } }
  const stub = () => ({ querySelector: key => key === '[data-extension-view="store"]' ? landing : {}, querySelectorAll: selector => selector === '[data-extension-view]' ? [installedTab, storeTab, thirdTab] : [] })
  panel.mount({ querySelector: stub })
  let prevented = false
  installedTab.onkeydown({ key: 'ArrowRight', preventDefault() { prevented = true } })
  assert.equal(prevented, true, '方向键要阻止页面滚动')
  await new Promise(resolve => setImmediate(resolve))
  assert.ok(calls.some(c => c.command === 'extension_catalog'), '换视图真的去读目录，不是只改样式')
  html = panel.render()
  assert.match(html, /aria-selected="true" tabindex="0" data-extension-view="store"/, '切过去以后 roving tabindex 跟着走')
  // redraw() 会换掉整棵 DOM：焦点由下一次 mount 按新节点还回去。
  panel.mount({ querySelector: stub })
  assert.equal(landing.focused, true, '换视图后焦点落到新的 tab 上')
  // Home/End：从头到尾、从尾到头。
  thirdTab.onkeydown({ key: 'Home', preventDefault() {} })
  await new Promise(resolve => setImmediate(resolve))
  assert.match(panel.render(), /aria-selected="true" tabindex="0" data-extension-view="installed"/)
})
test('T11 功能页入口是按钮不是标签：自有 class、带 ›、不占主操作行', async () => {
  const { panel } = carded(); await panel.load(); await refresh(panel)
  const html = panel.render()
  assert.match(html, /<button type="button" class="extension-page-entry" data-extension-page="fish\.magpie\/main"[^>]*><span class="extension-page-title">Clash 代理<\/span><span class="extension-page-go" aria-hidden="true">›<\/span><\/button>/, '入口自带 › 且是独立 class')
  assert.doesNotMatch(html, /class="pbtn sm" data-extension-page/, '旧的胶囊样式不再用于功能页入口')
  assert.match(html, /<div class="extension-pages" role="group" aria-label="功能页">/, '入口成组且有可访问名')
})
test('T11 检索是一个字段：输入框有可访问名，按钮缩进字段里', async () => {
  const { panel } = make({ third: () => ({ hits: [], hidden: 0 }) })
  await panel.load(); await panel.thirdparty('fish-dock-plugin')
  const html = panel.render()
  assert.match(html, /<div class="extension-search" role="search">/, '输入框与按钮在同一个字段容器里')
  assert.match(html, /<input id="extension-third-query" class="extension-search-input" type="text" aria-label="npm 包名或关键字"/, '输入框有 aria-label（placeholder 不能当名字）')
  assert.match(html, /class="pbtn sm extension-search-go"/, '「检索」是字段内的次级按钮，不再与同排按钮同权重')
  assert.doesNotMatch(html, /extension-third-search/, '旧的平铺搜索行已删')
})

// T-C / #63：placeholder 只留一句短话；整句命名规则移到下方 note——
// 窄窗原来把规则截成半句，用户看到的是一段被吃掉的提示。
test('检索占位符是短句，命名规则落在下方 note 行', async () => {
  const { panel } = make({ third: () => ({ hits: [], hidden: 0 }) })
  await panel.load(); await panel.thirdparty('fish-dock-plugin')
  const html = panel.render()
  assert.match(html, /placeholder="搜索 npm 包，如 fish-dock-plugin-weather"/, 'placeholder 是一句短话')
  assert.doesNotMatch(html, /placeholder="包名需为/, '整句规则不再放在 placeholder 里')
  assert.match(html, /<p class="note">包名需为 fish-dock-plugin-&lt;名字&gt;（可带 @scope）/, '规则改到下方 note 行')
})

// ── T-C / A10：npm 发现式检索（分页、去重、迟到隔离、错误重试）──────────
const thirdCalls = calls => calls.filter(c => c.command === 'extension_thirdparty').map(c => c.args)

test('首次进入第三方页做默认发现：空词 + discovery，不要求用户先认识包名', async () => {
  const { panel, calls } = make({ third: () => ({ hits: [{ name: 'fish-dock-plugin-weather', version: '1.0.0', description: '', publisher: 'acme', updated: '' }], hidden: 0, total: 1, filtered: 1, hasMore: false }) })
  await panel.load(); await panel.thirdparty('', { discovery: true })
  const [asked] = thirdCalls(calls)
  assert.equal(asked.query, '', '空词交给后端做默认发现')
  assert.equal(asked.discovery, true, '带 discovery 标志')
  assert.equal(asked.from, 0); assert.equal(asked.size, 20, '分页大小 20')
  assert.match(panel.render(), /fish-dock-plugin-weather/, '发现结果直接可安装')
})

test('精确包名与普通关键字都原样交给后端，前端不自己拼搜索语义', async () => {
  const { panel, calls } = make({ third: () => ({ hits: [], hidden: 0 }) })
  await panel.load(); await panel.thirdparty('@acme/fish-dock-plugin-weather')
  await panel.thirdparty('weather widget')
  const asked = thirdCalls(calls)
  assert.equal(asked[0].query, '@acme/fish-dock-plugin-weather'); assert.equal(asked[0].discovery, false, '点名要包就不是发现式')
  assert.equal(asked[1].query, 'weather widget'); assert.equal(asked[1].discovery, false)
})

test('加载更多带 from 累加，并按包名跨页去重', async () => {
  const pages = [
    { hits: [{ name: 'fish-dock-plugin-a', version: '1.0.0' }, { name: 'fish-dock-plugin-b', version: '1.0.0' }], hidden: 0, total: 4, filtered: 2, hasMore: true, nextFrom: 2 },
    { hits: [{ name: 'fish-dock-plugin-b', version: '1.0.0' }, { name: 'fish-dock-plugin-c', version: '1.0.0' }], hidden: 0, total: 4, filtered: 2, hasMore: false, nextFrom: 4 }
  ]
  let next = 0
  const { panel, calls } = make({ third: () => pages[next++] })
  await panel.load(); await panel.thirdparty('fish-dock-plugin', { discovery: true })
  assert.match(panel.render(), /data-extension-third="more"/, '还有下一页时给「加载更多」')
  await panel.thirdparty('fish-dock-plugin', { append: true })
  const asked = thirdCalls(calls)
  assert.equal(asked[1].from, 2, '第二页用后端给的 nextFrom 接着要')
  const html = panel.render()
  assert.equal((html.match(/data-extension-third="install"/g) || []).length, 3, 'b 重复出现只保留一条')
  assert.doesNotMatch(html, /data-extension-third="more"/, '到底了就不再给「加载更多」')
})

// t19 F1：默认发现是「空词 + discovery」，翻页时不能退化成 discovery=false
// （后端对「空词且非 discovery」直接报错，等于默认发现永远只能看第一页）。
test('默认发现的「加载更多」仍然带 discovery=true', async () => {
  const { panel, calls } = make({ third: args => ({ hits: [], hidden: 0, total: 40, hasMore: true, nextFrom: args.from + 20 }) })
  await panel.load(); await panel.thirdparty('', { discovery: true })
  await panel.thirdparty('', { append: true })
  const asked = thirdCalls(calls)
  assert.equal(asked.length, 2)
  assert.equal(asked[0].discovery, true, '第一页是发现式')
  assert.equal(asked[1].discovery, true, '第二页仍然是发现式（F1）')
  assert.equal(asked[1].query, '', '词仍然是空的，后端用默认前缀')
  assert.equal(asked[1].from, 20, '偏移前进')
})

// t19 F2：偏移按 registry 页长（nextFrom）走，不是按可见条数；
// 整页被过滤时也必须前进，否则「加载更多」永远返回同一页。
test('整页被过滤时加载更多仍前进（不会原地死循环）', async () => {
  const asked = []
  const { panel } = make({ third: args => {
    asked.push(args)
    // 第一页：20 个对象全被命名规范过滤，registry 说还有更多。
    if (asked.length === 1) return { hits: [], hidden: 20, total: 40, filtered: 0, hasMore: true, nextFrom: 20 }
    return { hits: [{ name: 'fish-dock-plugin-ok', version: '1.0.0' }], hidden: 0, total: 40, filtered: 1, hasMore: false, nextFrom: 40 }
  } })
  await panel.load(); await panel.thirdparty('fish-dock-plugin', { discovery: true })
  await panel.thirdparty('fish-dock-plugin', { append: true })
  assert.deepEqual(asked.map(call => call.from), [0, 20], '第二次必须从 20 开始，不是 0')
  assert.match(panel.render(), /fish-dock-plugin-ok/, '第二页结果进来了')
})

// t19 F2 的退化保护：后端没给 nextFrom 且页面没有前进时，不许再给「加载更多」。
test('偏移没有前进时不再显示「加载更多」', async () => {
  const { panel } = make({ third: () => ({ hits: [], hidden: 0, total: 0, filtered: 0, hasMore: true, nextFrom: 0 }) })
  await panel.load(); await panel.thirdparty('fish-dock-plugin', { discovery: true })
  assert.doesNotMatch(panel.render(), /data-extension-third="more"/, '没有前进就当到底，避免死循环')
})

// t42 F-A/F-B 端到端：喂给面板的是**命令返回体的真实字段形状**
// （app/test/fixtures/thirdparty-search.json，Rust 契约测试断言 `search_json` 的字段集合
// 与它逐一相同），而不是在桩里手写 nextFrom。
// 场景就按 t27 复现的那条：首页 20 对象、次页只有 5 对象、total 41、hasMore=true
// → 第三次请求必须 from=25（不是 40，否则跳过 25..39）。
test('真实命令返回体驱动分页：短页按 nextFrom=25 继续，不跳到 40', async () => {
  const fixture = JSON.parse(fs.readFileSync(path.join(__dirname, 'fixtures/thirdparty-search.json'), 'utf8'))
  const pages = [fixture.firstPage, fixture.secondPage, fixture.lastPage]
  const asked = []
  const { panel } = make({ third: args => { asked.push(args); return pages[asked.length - 1] } })
  await panel.load()
  await panel.thirdparty('', { discovery: true })
  await panel.thirdparty('', { append: true })
  assert.match(panel.render(), /data-extension-third="more"/, '短页但 hasMore=true 时仍给「加载更多」')
  await panel.thirdparty('', { append: true })
  assert.deepEqual(asked.map(call => call.from), [0, 20, 25], '短页从 25 继续（不是 40），中间对象不被跳过')
  assert.equal(asked.every(call => call.discovery === true), true, '默认发现翻页始终 discovery=true')
  const html = panel.render()
  assert.match(html, /fish-dock-plugin-p1\b/, '第一页第一条在')
  assert.match(html, /fish-dock-plugin-p21/, '第二页（21..25）确实进来了')
  assert.match(html, /fish-dock-plugin-p25/)
  assert.doesNotMatch(html, /data-extension-third="more"/, 'hasMore=false 之后不再给「加载更多」')
})

// t19 F5：被隐藏的数量是「本页」的，文案要说清楚，不能像全局总数。
test('被隐藏数量写成「本页」，并显示 registry 总数与本页可用数', async () => {
  const { panel } = make({ third: () => ({ hits: [{ name: 'fish-dock-plugin-a', version: '1.0.0' }], hidden: 2, total: 41, filtered: 18, hasMore: false, nextFrom: 20 }) })
  await panel.load(); await panel.thirdparty('fish-dock-plugin')
  const html = panel.render()
  assert.match(html, /本页已隐藏 2 个不符合 fish-dock-plugin-\* 命名的包/, 'hidden 文案说明是本页')
  assert.match(html, /共 41 个结果，本页可用 18 个/, '总数与本页可用数分开写')
})

// t19 F6：增量（加载更多）失败后点「重试」，要重发原来那一页，
// 不能把已加载的列表清空、从第 0 页重来。
test('加载更多失败后的重试回到原来那一页，并保留已加载条目', async () => {
  const asked = []
  let failAppend = true
  const { panel } = make({ third: args => {
    asked.push(args)
    if (args.from === 0) return { hits: [{ name: 'fish-dock-plugin-a', version: '1.0.0' }], hidden: 0, total: 40, filtered: 1, hasMore: true, nextFrom: 20 }
    if (failAppend) { failAppend = false; throw new Error('registry 暂时不可达') }
    return { hits: [{ name: 'fish-dock-plugin-b', version: '1.0.0' }], hidden: 0, total: 40, filtered: 1, hasMore: false, nextFrom: 40 }
  } })
  await panel.load(); await panel.thirdparty('fish-dock-plugin', { discovery: true })
  await panel.thirdparty('fish-dock-plugin', { append: true })
  assert.match(panel.render(), /registry 暂时不可达/, '增量失败要有错误')
  assert.match(panel.render(), /fish-dock-plugin-a/, '已加载的条目还在')
  await panel.retryThird()
  assert.deepEqual(asked.map(call => call.from), [0, 20, 20], '重试发的是原来那一页（20），不是 0')
  const html = panel.render()
  assert.match(html, /fish-dock-plugin-a/); assert.match(html, /fish-dock-plugin-b/, '重试成功后两页都在')
  assert.doesNotMatch(html, /registry 暂时不可达/, '成功后悔错误收起')
})

test('迟到结果不覆盖新词：先发的后到也只能被丢弃', async () => {
  const pending = new Map()
  const third = args => new Promise(resolve => pending.set(args.query, resolve))
  const { panel } = make({ third })
  await panel.load()
  const first = panel.thirdparty('first')
  const second = panel.thirdparty('second')
  pending.get('second')({ hits: [{ name: 'fish-dock-plugin-second', version: '1.0.0' }], hidden: 0 })
  await second
  pending.get('first')({ hits: [{ name: 'fish-dock-plugin-first', version: '0.0.1' }], hidden: 0 })
  await first
  const html = panel.render()
  assert.match(html, /fish-dock-plugin-second/, '新词的结果在')
  assert.doesNotMatch(html, /fish-dock-plugin-first/, '旧词的迟到结果没盖上来')
})

test('检索失败显示原因与「重试」，重试沿用上一次的词', async () => {
  let attempt = 0
  const { panel, calls } = make({ third: () => {
    attempt += 1
    if (attempt === 1) throw new Error('registry 连接被拒绝')
    return { hits: [{ name: 'fish-dock-plugin-ok', version: '1.0.0' }], hidden: 0 }
  } })
  await panel.load(); await panel.thirdparty('fish-dock-plugin')
  let html = panel.render()
  assert.match(html, /registry 连接被拒绝/, '失败原因照实写')
  assert.match(html, /data-extension-third="retry"/, '给重试入口')
  assert.doesNotMatch(html, /没有找到可安装的包/, '失败不冒充空结果')
  await panel.retryThird()
  html = panel.render()
  assert.match(html, /fish-dock-plugin-ok/, '重试成功后结果出现')
  assert.doesNotMatch(html, /registry 连接被拒绝/, '错误提示收起')
  assert.equal(thirdCalls(calls)[1].query, 'fish-dock-plugin', '重试用的是同一个词')
})

test('registry 总数、本页可用数与空结果三态各写各的', async () => {
  const { panel } = make({ third: () => ({ hits: [{ name: 'fish-dock-plugin-a', version: '1.0.0' }], hidden: 2, total: 41, filtered: 18, hasMore: true }) })
  await panel.load(); await panel.thirdparty('fish-dock-plugin')
  const html = panel.render()
  assert.match(html, /共 41 个结果，本页可用 18 个/, 'registry 总数与本页可用数分开写')
  assert.match(html, /本页已隐藏 2 个不符合 fish-dock-plugin-\* 命名的包/, '被过滤的数量照实说')
  const empty = make({ third: () => ({ hits: [], hidden: 0, total: 0, filtered: 0, hasMore: false }) })
  await empty.panel.load(); await empty.panel.thirdparty('fish-dock-plugin')
  assert.match(empty.panel.render(), /没有找到可安装的包。可换一个包名再试。/, '空结果单独一句')
})

// #72：官方三个插件在 npm 上的包被后端排除，前端要把「被排除了几个、去哪儿装」说出来，
// 不能让人以为「官方插件不见了 / 搜索坏了」；两种计数同时出现时两句都要在。
test('被排除的官方插件数量单独提示，并指回官方插件页', async () => {
  const { panel } = make({ third: () => ({ hits: [{ name: 'fish-dock-plugin-a', version: '1.0.0' }], hidden: 0, official: 3, total: 4, filtered: 1, hasMore: false, nextFrom: 4 }) })
  await panel.load(); await panel.thirdparty('fish-dock-plugin')
  const html = panel.render()
  assert.match(html, /本页另有 3 个官方插件的 npm 包/, 'official 数量要单独说')
  assert.match(html, /请到「官方插件」页安装或更新/, '要指回去哪儿装')
  assert.doesNotMatch(html, /不符合 fish-dock-plugin-\* 命名的包/, '官方包不是「命名不合规」，不许混进 hidden 文案')
  assert.match(html, /官方插件不在本页/, '规则行也要说明官方插件不在这页')
})

test('官方插件与命名不合规同时出现时，两句提示都在', async () => {
  const { panel } = make({ third: () => ({ hits: [], hidden: 2, official: 1, total: 3, filtered: 0, hasMore: false, nextFrom: 3 }) })
  await panel.load(); await panel.thirdparty('fish-dock-plugin')
  const html = panel.render()
  assert.match(html, /本页已隐藏 2 个不符合/)
  assert.match(html, /本页另有 1 个官方插件的 npm 包/)
})
