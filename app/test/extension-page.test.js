const { test } = require('node:test')
const assert = require('node:assert/strict')
const vm = require('node:vm'), fs = require('node:fs'), path = require('node:path')
const window = {}
// URL 也传进去：网关地址校验用 `new URL(...)`，vm 的默认全局没有这个 Web API。
vm.runInNewContext(fs.readFileSync(path.join(__dirname,'../src/pet/extension-page.js'),'utf8'), { window, setTimeout, clearTimeout, URL })
const frame = { contentWindow: {} }, ticket = 'a'.repeat(64)
const good = { source: frame.contentWindow, origin: 'null', data: { type:'fish:call', ticket, id:1, method:'status', params:{} } }
test('插件页桥接受当前隔离窗口和当前票据', () => { assert.equal(window.ExtensionPage.accepts(good, frame, ticket),true) })
test('其他窗口、旧票据、非隔离来源、保留方法及过大请求均无效', () => {
 for (const event of [{...good,source:{}},{...good,origin:'http://tauri.localhost'}, {...good,data:{...good.data,ticket:'b'.repeat(64)}}, {...good,data:{...good.data,method:'host.storage.read'}}, {...good,data:{...good.data,id:'bad'}}, {...good,data:{...good.data,params:{value:'x'.repeat(1024*1024)}}}]) assert.equal(window.ExtensionPage.accepts(event,frame,ticket),false)
})

// 插件升级后 digest 变了：旧帧必须退场、旧票据必须关，再进入时拿新票据重建。
// 只退场不重建的话，用户会停在空白页——current 还指着那个 key，enter 直接返回。
function fixture(){
 const calls=[], listeners={}, tickets=[]
 const element=()=>{
  const node={style:{},dataset:{},children:[],removed:false,
    // 重新进入一个已存在的页面时会走 theme()，它要往 iframe 里 postMessage。
    contentWindow:{postMessage(){}},
   classList:{add(){},remove(){},toggle(){},contains(){return false}},
   setAttribute(){},appendChild(child){node.children.push(child);return child},
   append(...items){node.children.push(...items)},
   replaceChildren(...items){node.children=items.slice()},
   insertBefore(child){node.children.unshift(child);return child},
   replaceWith(){node.removed=true},remove(){node.removed=true},
   querySelectorAll(){return []},
   getBoundingClientRect(){return {height:10,width:10}}}
  return node
 }
 const body=element()
 const document={createElement:element,body,getElementById(){return null},documentElement:element()}
 const hostWindow={
  addEventListener(type,fn){listeners[type]=fn},
  removeEventListener(type,fn){if(listeners[type]===fn)delete listeners[type]},
  getComputedStyle(){return {getPropertyValue(){return ''}}},
 }
 const invoke=async(command)=>{
  calls.push(command)
  if(command==='extension_page'){const ticket='a'.repeat(63)+String(calls.length);tickets.push(ticket);return {url:`http://127.0.0.1:1/page?t=${calls.length}`,ticket}}
  if(command==='extension_page_call')return {url:'http://127.0.0.1:3425/ui/index.html'}
  return null
 }
 return {document,hostWindow,invoke,calls,tickets,listeners}
}
function len(text){return text.length}
// 模拟插件页发来的一条消息（来源窗口 + origin:'null' + 票据，与真实沙盒一致）。
const send=(f,frame,data)=>f.listeners.message?.({source:frame.contentWindow,origin:'null',data})

test('插件升级后旧页面被销毁，再进入时用新票据重建', async () => {
 const f=fixture()
 const page=window.ExtensionPage.create({invoke:f.invoke,lang:()=>'zh',hostWindow:f.hostWindow,document:f.document})
 const root=f.document.createElement('div')
 await page.enter(root,{key:'fish.clash',title:'Clash',digest:'old'})
 assert.equal(f.calls.filter(c=>c==='extension_page').length,1)
 page.sync([{key:'fish.clash',digest:'new'}])
 assert.equal(f.calls.filter(c=>c==='extension_page_close').length,1,'旧票据要关')
 await page.enter(root,{key:'fish.clash',title:'Clash',digest:'new'})
 assert.equal(f.calls.filter(c=>c==='extension_page').length,2,'要重新取票据')
 page.dispose()
})

test('digest 没变时重复 sync 不会拆掉正在看的页面', async () => {
 const f=fixture()
 const page=window.ExtensionPage.create({invoke:f.invoke,lang:()=>'zh',hostWindow:f.hostWindow,document:f.document})
 const root=f.document.createElement('div')
 await page.enter(root,{key:'fish.magpie',title:'Magpie',digest:'same'})
 page.sync([{key:'fish.magpie',digest:'same'}])
 assert.equal(f.calls.filter(c=>c==='extension_page_close').length,0)
 page.dispose()
})

// 内存预算（红线《禁止屎山十条》：「不可见的 webview 不得常驻」）：
// 插件页被 park 之后，它的 iframe（含 gateway.open 起的跨源管理界面）在
// WebView2 里是一个独立 renderer 进程，本机实测常驻 141.3 MB 私有工作集、
// 面板关掉 1 小时 50 分后仍在。到点必须整页退场，进程才会退出。
const wait=ms=>new Promise(resolve=>setTimeout(resolve,ms))

test('park 到点后整页退场：iframe 离开 DOM、票据关掉、再进入重新取票', async () => {
 const f=fixture()
 const page=window.ExtensionPage.create({invoke:f.invoke,lang:()=>'zh',hostWindow:f.hostWindow,document:f.document,parkDiscardMs:20})
 const root=f.document.createElement('div')
 await page.enter(root,{key:'fish.magpie',title:'Magpie',digest:'d1'})
 assert.equal(f.calls.filter(c=>c==='extension_page').length,1)
 page.park()
 const parked=f.document.body.children[0]
 assert.equal(parked.children.length,1,'park 先把这一页挪进隐藏容器')
 await wait(60)
 assert.equal(parked.children[0].removed,true,'到点后 iframe 必须离开 DOM，否则它的 renderer 不退')
 assert.equal(f.calls.filter(c=>c==='extension_page_close').length,1,'到点后票据要关')
 await page.enter(root,{key:'fish.magpie',title:'Magpie',digest:'d1'})
 assert.equal(f.calls.filter(c=>c==='extension_page').length,2,'再进来要重新取票据、重新装载')
 page.dispose()
})

test('park 之后没到点就回来：撤掉退场计时，页面原样接着用', async () => {
 const f=fixture()
 const page=window.ExtensionPage.create({invoke:f.invoke,lang:()=>'zh',hostWindow:f.hostWindow,document:f.document,parkDiscardMs:30})
 const root=f.document.createElement('div')
 await page.enter(root,{key:'fish.clash',title:'Clash',digest:'d1'})
 page.park()
 await wait(10)
 await page.enter(root,{key:'fish.clash',title:'Clash',digest:'d1'})
 assert.equal(f.calls.filter(c=>c==='extension_page').length,1,'回来看的还是那一页，不重建')
 await wait(60)
 assert.equal(f.calls.filter(c=>c==='extension_page_close').length,0,'回来之后旧计时不许再把它退场')
 page.dispose()
})

test('dispose 之后不留退场计时器', async () => {
 const f=fixture()
 const page=window.ExtensionPage.create({invoke:f.invoke,lang:()=>'zh',hostWindow:f.hostWindow,document:f.document,parkDiscardMs:20})
 const root=f.document.createElement('div')
 await page.enter(root,{key:'fish.magpie',title:'Magpie',digest:'d1'})
 page.park()
 page.dispose()
 await wait(60)
 assert.equal(f.calls.filter(c=>c==='extension_page_close').length,1,'dispose 自己退场一次，计时器不许再退第二次')
})

// ── T-N / A14–A16：ready 与 layout 分开计时、首帧独立 deadline、
//    普通页/服务工具栏/业务 iframe 三种尺寸分开、迟到票据隔离 ──────────
const listen = seen => (key, value) => seen.push({ key, ...value })

test('无记忆时不抢报占位：等兜底计时才给固定高度（不把 52 当整窗初值）', async () => {
  const f = fixture(), seen = []
  const page = window.ExtensionPage.create({ invoke:f.invoke, lang:()=>'zh', hostWindow:f.hostWindow, document:f.document, firstLayoutMs:5000, firstReportMs:10, placeholderHeight:240, onLayout:listen(seen) })
  const root = f.document.createElement('div')
  await page.enter(root,{key:'fish.clash',title:'Clash',digest:'d1'})
  assert.equal(seen.length, 0, '进来先不报：等首个 layout / gateway.open / 兜底')
  await wait(30)
  assert.deepEqual(seen, [{ key:'fish.clash', height:240, layout:'single', service:false }], '兜底到点才报稳定占位')
  page.dispose()
})

// t19 F4：服务页首开不再出现 240 → 52 的「先大后小」。
test('服务页首个 layout 先到时只报一次真实高度，没有 240 的中间态', async () => {
  const f = fixture(), seen = []
  const page = window.ExtensionPage.create({ invoke:f.invoke, lang:()=>'zh', hostWindow:f.hostWindow, document:f.document, firstLayoutMs:5000, firstReportMs:400, placeholderHeight:240, onLayout:listen(seen) })
  const root = f.document.createElement('div')
  await page.enter(root,{key:'fish.clash',title:'Clash',digest:'d1'})
  const frame = root.children[0].children[0]
  send(f, frame, { type:'fish:ready', ticket:f.tickets[0] })
  send(f, frame, { type:'fish:layout', ticket:f.tickets[0], height:600, layout:'single' })
  assert.deepEqual(seen.map(row => row.height), [600], '只报真实高度，没有 240')
  await wait(30)
  assert.deepEqual(seen.map(row => row.height), [600], '兜底计时已被撤掉')
  page.dispose()
})

// t19 F4：记忆里这一页上次是服务页时，首次上报就该是工具栏高度（不是普通页 240）。
test('记忆带 service 时首报就是工具栏高度，不出现 240→52 跳变', async () => {
  const f = fixture(), seen = []
  const page = window.ExtensionPage.create({ invoke:f.invoke, lang:()=>'zh', hostWindow:f.hostWindow, document:f.document, firstLayoutMs:5000, remembered:key => key==='fish.clash' ? { height:52, layout:'single', service:true } : null, onLayout:listen(seen) })
  const root = f.document.createElement('div')
  await page.enter(root,{key:'fish.clash',title:'Clash',digest:'d1'})
  assert.deepEqual(seen.map(row => [row.height, row.service]), [[52, true]], '首报即工具栏')
  const frame = root.children[0].children[0]
  send(f, frame, { type:'fish:call', ticket:f.tickets[0], id:1, method:'gateway.open', params:{} })
  await wait(10)
  assert.ok(seen.length >= 2, 'gateway.open 之后又报了一次')
  assert.equal(seen.every(row => row.height === 52 && row.service === true), true, '每次都是工具栏 52')
  assert.equal(seen.some(row => row.height === 240), false, '全程没有 240')
  page.dispose()
})

test('有 per-key 记忆时首个 layout 前用记忆高度，没有记忆才用占位', async () => {
  const f = fixture(), seen = []
  const page = window.ExtensionPage.create({ invoke:f.invoke, lang:()=>'zh', hostWindow:f.hostWindow, document:f.document, firstLayoutMs:5000, placeholderHeight:240, remembered:key => key==='fish.clash' ? { height:600, layout:'single', service:false } : null, onLayout:listen(seen) })
  const root = f.document.createElement('div')
  await page.enter(root,{key:'fish.clash',title:'Clash',digest:'d1'})
  assert.equal(seen.at(-1).height, 600, '记忆高度先顶上，面板不跳半屏')
  page.dispose()
})

test('ready 不等于布局到位：首个 layout 超时给可重试状态，页面不被藏起来', async () => {
  const f = fixture(), seen = []
  const page = window.ExtensionPage.create({ invoke:f.invoke, lang:()=>'zh', hostWindow:f.hostWindow, document:f.document, firstLayoutMs:20, placeholderHeight:240, onLayout:listen(seen) })
  const root = f.document.createElement('div')
  await page.enter(root,{key:'fish.clash',title:'Clash',digest:'d1'})
  const frame = root.children[0].children[0]
  send(f, frame, { type:'fish:ready', ticket:f.tickets[0] })
  await wait(45)
  const notice = root.children[0]
  assert.equal(notice.className, 'extension-page-notice', '超时后是一条状态，不是永久空白')
  assert.match(notice.children[0].textContent, /没有报告尺寸/)
  assert.equal(notice.children[1].textContent, '重试')
  page.dispose()
})

// t19 F3：点「重试」必须真的重新取票、重建页面。
// 原来的写法里 retire() 先把 current 清空，`if (current === key)` 永远为假 ——
// 用户点重试只看到空白，且不会再取票（extension_page 调用次数 1 → 1）。
test('首个 layout 超时后的「重试」重新取票并重建页面', async () => {
  const f = fixture(), seen = []
  const page = window.ExtensionPage.create({ invoke:f.invoke, lang:()=>'zh', hostWindow:f.hostWindow, document:f.document, firstLayoutMs:20, firstReportMs:5000, placeholderHeight:240, onLayout:listen(seen) })
  const root = f.document.createElement('div')
  await page.enter(root,{key:'fish.clash',title:'Clash',digest:'d1'})
  const oldFrame = root.children[0].children[0]
  send(f, oldFrame, { type:'fish:ready', ticket:f.tickets[0] })
  await wait(45)
  const notice = root.children[0]
  assert.equal(notice.className, 'extension-page-notice', '超时状态在')
  assert.equal(f.calls.filter(c => c === 'extension_page').length, 1, '此时只取过一次票')
  notice.children[1].onclick()                       // 用户真的点了「重试」
  await wait(15)
  assert.equal(f.calls.filter(c => c === 'extension_page').length, 2, '重试要重新取票（F3）')
  assert.equal(f.calls.filter(c => c === 'extension_page_close').length, 1, '旧票据要关')
  const fresh = root.children[0].children[0]
  assert.notEqual(fresh, oldFrame, '换成了新帧')
  assert.equal(typeof fresh.src, 'string', '新帧真的装载了地址')
  page.dispose()
})

test('首个有效 layout 到达后撤掉 deadline，并上报真实高度', async () => {
  const f = fixture(), seen = []
  const page = window.ExtensionPage.create({ invoke:f.invoke, lang:()=>'zh', hostWindow:f.hostWindow, document:f.document, firstLayoutMs:20, placeholderHeight:240, onLayout:listen(seen) })
  const root = f.document.createElement('div')
  await page.enter(root,{key:'fish.clash',title:'Clash',digest:'d1'})
  const frame = root.children[0].children[0]
  send(f, frame, { type:'fish:ready', ticket:f.tickets[0] })
  send(f, frame, { type:'fish:layout', ticket:f.tickets[0], height:600, layout:'single' })
  assert.equal(seen.at(-1).height, 600, '真实高度覆盖占位')
  await wait(45)
  assert.equal(root.children.length, 1, 'deadline 已撤：不再插超时状态')
  page.dispose()
})

test('服务工具栏与普通页高度分开：工具栏夹在 44–320，普通页照实报', async () => {
  const f = fixture(), seen = []
  const page = window.ExtensionPage.create({ invoke:f.invoke, lang:()=>'zh', hostWindow:f.hostWindow, document:f.document, firstLayoutMs:5000, onLayout:listen(seen) })
  const root = f.document.createElement('div')
  await page.enter(root,{key:'fish.clash',title:'Clash',digest:'d1'})
  const frame = root.children[0].children[0]
  send(f, frame, { type:'fish:ready', ticket:f.tickets[0] })
  send(f, frame, { type:'fish:layout', ticket:f.tickets[0], height:600, layout:'single' })
  assert.deepEqual([seen.at(-1).height, seen.at(-1).service], [600, false], '普通页：整窗高度')
  // gateway.open → 业务服务页：上报的是工具栏那一条，不是整窗 600。
  send(f, frame, { type:'fish:call', ticket:f.tickets[0], id:1, method:'gateway.open', params:{} })
  await wait(10)
  assert.equal(seen.at(-1).service, true, '服务页标记带上')
  assert.equal(seen.at(-1).height, 52, '先报工具栏 fallback，不是 600')
  send(f, frame, { type:'fish:layout', ticket:f.tickets[0], height:900, layout:'double' })
  assert.deepEqual([seen.at(-1).height, seen.at(-1).layout], [320, 'double'], '工具栏高度上限 320')
  send(f, frame, { type:'fish:layout', ticket:f.tickets[0], height:20, layout:'single' })
  assert.equal(seen.at(-1).height, 44, '工具栏高度下限 44')
  page.dispose()
})

test('旧票据的迟到 layout 不改变新页面的尺寸（按票据隔离）', async () => {
  const f = fixture(), seen = []
  const page = window.ExtensionPage.create({ invoke:f.invoke, lang:()=>'zh', hostWindow:f.hostWindow, document:f.document, firstLayoutMs:5000, firstReportMs:10, placeholderHeight:240, onLayout:listen(seen) })
  const root = f.document.createElement('div')
  await page.enter(root,{key:'fish.clash',title:'Clash',digest:'d1'})
  await wait(20)                                     // 让兜底上报发生，才有「当前高度」可比
  const stale = root.children[0].children[0]
  page.sync([{ key:'fish.clash', digest:'d2' }])
  await page.enter(root,{key:'fish.clash',title:'Clash',digest:'d2'})
  await wait(20)
  const fresh = root.children[0].children[0]
  const before = seen.at(-1).height
  send(f, stale, { type:'fish:layout', ticket:f.tickets[0], height:999, layout:'double' })
  assert.equal(seen.at(-1).height, before, '旧票据的消息被丢弃')
  send(f, fresh, { type:'fish:layout', ticket:f.tickets[1], height:600, layout:'double' })
  assert.equal(seen.at(-1).height, 600, '新票据照常生效')
  page.dispose()
})
