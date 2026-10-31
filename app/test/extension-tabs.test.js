const { test } = require('node:test')
const assert = require('node:assert/strict')
const vm = require('node:vm')
const fs = require('node:fs')
const path = require('node:path')
const window = {}
vm.runInNewContext(fs.readFileSync(path.join(__dirname, '../src/pet/extension-tabs.js'), 'utf8'), { window })
const tabs = window.ExtensionTabs
const plain = x => JSON.parse(JSON.stringify(x))
function strip(){const listeners=new Map(),classes=new Set();const el={clientWidth:300,scrollWidth:900,scrollLeft:0,classList:{add:n=>classes.add(n),remove:n=>classes.delete(n)},addEventListener(name,fn){listeners.set(name,fn)},setPointerCapture(){},releasePointerCapture(){},hasPointerCapture(){return true}};tabs.bindScroll(el);return{el,classes,fire(name,patch={}){let prevented=false,stopped=false;listeners.get(name)?.({pointerId:1,button:0,isPrimary:true,pointerType:'mouse',clientX:180,clientY:10,deltaX:0,deltaY:80,deltaMode:0,detail:1,preventDefault(){prevented=true},stopImmediatePropagation(){stopped=true},...patch});return{prevented,stopped}}}}
test('标签栏支持鼠标拖动，拖动后的单击不会切换标签',()=>{const s=strip();s.fire('pointerdown');assert.equal(s.fire('pointermove',{clientX:70}).prevented,true);assert.equal(s.el.scrollLeft,110);assert.ok(s.classes.has('dragging'));s.fire('pointerup',{clientX:70});assert.ok(!s.classes.has('dragging'));assert.equal(s.fire('click').stopped,true);s.fire('pointerdown');s.fire('pointerup');assert.equal(s.fire('click').stopped,false)})
test('微小位移保留正常点击，触控取消后不残留拖动状态',()=>{const s=strip();s.fire('pointerdown');s.fire('pointermove',{clientX:177});s.fire('pointerup');assert.equal(s.el.scrollLeft,0);assert.equal(s.fire('click').stopped,false);s.fire('pointerdown',{pointerType:'touch'});s.fire('pointermove',{clientX:80,pointerType:'touch'});s.fire('pointercancel');assert.ok(!s.classes.has('dragging'));assert.equal(s.fire('click',{detail:0}).stopped,false)})
test('纵向滚轮横移标签，控制键缩放与原生横向滚动保留',()=>{const s=strip();assert.equal(s.fire('wheel').prevented,true);assert.equal(s.el.scrollLeft,80);assert.equal(s.fire('wheel',{ctrlKey:true}).prevented,false);assert.equal(s.fire('wheel',{deltaX:100}).prevented,false)})
test('默认仅核心页；新插件页追加，旧 models 固定入口不再渲染', () => {
  assert.deepEqual(plain(tabs.order([], ['fish.ofm/console'])), ['now','agents','look','mail','widgets','fish.ofm/console'])
})
test('插件停用隐藏但重启后恢复位置；无效与重复ID不渲染', () => {
  const saved = ['look','fish.ofm/console','now','look','bad/path/escape','mail']
  assert.deepEqual(plain(tabs.order(saved, [])), ['look','now','mail','agents','widgets'])
  assert.deepEqual(plain(tabs.order(saved, ['fish.ofm/console'])), ['look','fish.ofm/console','now','mail','agents','widgets'])
})
test('移动可见标签保留隐藏历史并限制128条；恢复默认只需空顺序', () => {
  const saved = ['look','fish.magpie/console','fish.ofm/console','now']
  const moved = tabs.move(saved, ['fish.ofm/console'], 'fish.ofm/console', -1)
  assert.deepEqual(plain(tabs.order(moved, ['fish.ofm/console'])).slice(0,3), ['fish.ofm/console','look','now'])
  assert.ok(moved.includes('fish.magpie/console'))
  assert.ok(moved.length <= 128)
  assert.deepEqual(plain(tabs.order([], [])), ['now','agents','look','mail','widgets'])
})
test('移除当前页面时选择最近可见项，合法键双向一致', () => {
  assert.equal(tabs.nearest('fish.ofm/console', ['now','fish.ofm/console','look'], ['now','look']), 'look')
  for (const good of ['now','fish.ofm/console','acme.weather/details']) assert.equal(tabs.valid(good),true)
  for (const bad of ['models','../a','fish.ofm/a/b','A.b/page','fish.ofm/']) assert.equal(tabs.valid(bad),false)
})
