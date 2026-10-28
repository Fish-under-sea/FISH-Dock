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
