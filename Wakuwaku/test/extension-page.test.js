const { test } = require('node:test')
const assert = require('node:assert/strict')
const vm = require('node:vm'), fs = require('node:fs'), path = require('node:path')
const window = {}
vm.runInNewContext(fs.readFileSync(path.join(__dirname,'../src/pet/extension-page.js'),'utf8'), { window })
const frame = { contentWindow: {} }, ticket = 'a'.repeat(64)
const good = { source: frame.contentWindow, origin: 'null', data: { type:'fish:call', ticket, id:1, method:'status', params:{} } }
test('插件页桥接受当前隔离窗口和当前票据', () => { assert.equal(window.ExtensionPage.accepts(good, frame, ticket),true) })
test('其他窗口、旧票据、非隔离来源、保留方法及过大请求均无效', () => {
 for (const event of [{...good,source:{}},{...good,origin:'http://tauri.localhost'}, {...good,data:{...good.data,ticket:'b'.repeat(64)}}, {...good,data:{...good.data,method:'host.storage.read'}}, {...good,data:{...good.data,id:'bad'}}, {...good,data:{...good.data,params:{value:'x'.repeat(1024*1024)}}}]) assert.equal(window.ExtensionPage.accepts(event,frame,ticket),false)
})
