const { test } = require('node:test')
const assert = require('node:assert/strict')
const vm = require('node:vm'), fs = require('node:fs'), path = require('node:path')
const window = {}
vm.runInNewContext(fs.readFileSync(path.join(__dirname,'../src/pet/extension-page.js'),'utf8'), { window, setTimeout, clearTimeout })
const frame = { contentWindow: {} }, ticket = 'a'.repeat(64)
const good = { source: frame.contentWindow, origin: 'null', data: { type:'fish:call', ticket, id:1, method:'status', params:{} } }
test('插件页桥接受当前隔离窗口和当前票据', () => { assert.equal(window.ExtensionPage.accepts(good, frame, ticket),true) })
test('其他窗口、旧票据、非隔离来源、保留方法及过大请求均无效', () => {
 for (const event of [{...good,source:{}},{...good,origin:'http://tauri.localhost'}, {...good,data:{...good.data,ticket:'b'.repeat(64)}}, {...good,data:{...good.data,method:'host.storage.read'}}, {...good,data:{...good.data,id:'bad'}}, {...good,data:{...good.data,params:{value:'x'.repeat(1024*1024)}}}]) assert.equal(window.ExtensionPage.accepts(event,frame,ticket),false)
})

// 插件升级后 digest 变了：旧帧必须退场、旧票据必须关，再进入时拿新票据重建。
// 只退场不重建的话，用户会停在空白页——current 还指着那个 key，enter 直接返回。
function fixture(){
 const calls=[]
 const element=()=>{
  const node={style:{},dataset:{},children:[],removed:false,
    // 重新进入一个已存在的页面时会走 theme()，它要往 iframe 里 postMessage。
    contentWindow:{postMessage(){}},
   classList:{add(){},remove(){},toggle(){},contains(){return false}},
   setAttribute(){},appendChild(child){node.children.push(child);return child},
   append(...items){node.children.push(...items)},
   replaceChildren(...items){node.children=items.slice()},
   replaceWith(){node.removed=true},remove(){node.removed=true},
   querySelectorAll(){return []},
   getBoundingClientRect(){return {height:10,width:10}}}
  return node
 }
 const body=element()
 const document={createElement:element,body,getElementById(){return null},documentElement:element()}
 const hostWindow={addEventListener(){},removeEventListener(){},getComputedStyle(){return {getPropertyValue(){return ''}}}}
 const invoke=async(command)=>{calls.push(command);if(command==='extension_page')return {url:`http://127.0.0.1:1/page?t=${calls.length}`,ticket:'t'.repeat(64-len(String(calls.length)))+String(calls.length)};return null}
 return {document,hostWindow,invoke,calls}
}
function len(text){return text.length}

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
