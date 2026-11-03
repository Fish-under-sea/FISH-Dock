const {test}=require('node:test')
const assert=require('node:assert/strict')
const fs=require('node:fs'),path=require('node:path'),vm=require('node:vm')
function make({view={installed:false,mods:[]},outcomes=[],fail=false}={}){const window={},calls=[];vm.runInNewContext(fs.readFileSync(path.join(__dirname,'../src/pet/windhawk-panel.js'),'utf8'),{window});const panel=window.WindhawkPanel.create({lang:()=> 'zh',redraw(){},invoke:async(command,args)=>{calls.push({command,args});if(command==='windhawk_state')return view;if(command==='windhawk_preview')return {metadata:{id:'sample',name:'示例',version:'2',author:'Fish'},existing:view.mods?.[0]||null,fingerprint:'synthetic-fingerprint'};if(command==='windhawk_fetch_source')return {source:'downloaded source',preview:{metadata:{id:'remote',name:'远程',version:'1'},fingerprint:'remote-fingerprint'}};if(command==='windhawk_collection_preview')return {id:'desktop-set',name:'桌面集合',mods:[{member:{id:'sample',version:'1.2',source:'collection source',sha256:'fixture',settings:{count:7}},preview:{metadata:{id:'sample',name:'示例',version:'1.2'},fingerprint:'collection-fingerprint'}}]};if(command==='windhawk_collection_import')return {results:[{id:'sample',ok:false,error:'编译失败，原配置已保留'}]};if(command==='windhawk_import'){if(fail)throw Error('编译失败，旧配置已保留');return{token:'wh-fixture',disabled:true}};if(command==='windhawk_rollback')return {restored:true};if(command==='windhawk_apply'){if(fail)throw Error('系统提权已取消');return outcomes}}});return{panel,calls}}
test('远程下载只预览，集合确认后报告逐项结果与预设',async()=>{const{panel,calls}=make({view:{installed:true,layout:{},mods:[]}});await panel.load();await panel.fetchSource('https://example.com/sample.wh.cpp');assert.equal(calls.at(-1).command,'windhawk_fetch_source');assert.equal(calls.filter(c=>c.command==='windhawk_import').length,0);await panel.inspectCollection({size:20,text:async()=> '{"schema":1}'},[]);assert.match(panel.render(),/count/);assert.match(panel.render(),/data-windhawk-collection-import/);assert.equal(calls.filter(c=>c.command==='windhawk_collection_import').length,0);await panel.installCollection();assert.equal(calls.at(-1).command,'windhawk_state');assert.match(panel.render(),/编译失败，原配置已保留/);assert.doesNotMatch(panel.render(),/全部导入成功/)})
test('预览不直接安装，确认编译后重新读实际状态并可恢复',async()=>{const{panel,calls}=make({view:{installed:true,layout:{},mods:[]}});await panel.load();await panel.inspect({name:'sample.wh.cpp',size:100,text:async()=> 'synthetic source'});assert.equal(calls.at(-1).command,'windhawk_preview');assert.match(panel.render(),/data-windhawk-import/);await panel.install();assert.deepEqual(JSON.parse(JSON.stringify(calls.find(c=>c.command==='windhawk_import').args)),{source:'synthetic source',fingerprint:'synthetic-fingerprint'});assert.equal(calls.at(-1).command,'windhawk_state');assert.match(panel.render(),/data-windhawk-rollback/);await panel.rollback();assert.equal(calls.at(-2).command,'windhawk_rollback');assert.equal(calls.at(-1).command,'windhawk_state')})
test('编译失败显示错误且不显示安装成功，未确认前不调用编译器',async()=>{const{panel,calls}=make({fail:true,view:{installed:true,layout:{},mods:[]}});await panel.load();await panel.inspect({name:'sample.wh.cpp',size:10,text:async()=> 'synthetic'});assert.equal(calls.filter(c=>c.command==='windhawk_import').length,0);await panel.install();assert.match(panel.render(),/编译失败/);assert.doesNotMatch(panel.render(),/已编译并导入/);assert.equal(calls.filter(c=>c.command==='windhawk_state').length,1)})
test('Windhawk 缺失显示选择入口，不影响其他外观设置',async()=>{const{panel,calls}=make();await panel.load();assert.match(panel.render(),/未检测到 Windhawk/);assert.match(panel.render(),/data-windhawk-select/);assert.deepEqual(calls.map(c=>c.command),['windhawk_state'])})
test('内置引擎直接提供模组导入，不要求外部安装；路径入口收在高级选项',async()=>{const{panel}=make({view:{installed:true,bundled:true,layout:{portable:true,install:'X:/Dock/data/windhawk'},mods:[]}});await panel.load();const html=panel.render();assert.match(html,/内置引擎已就绪/);assert.doesNotMatch(html,/安装 Windhawk/);assert.match(html,/<input[^>]*class="in"[^>]*id="windhawk-url"/);assert.match(html,/<details[^>]*data-windhawk-advanced/);assert.doesNotMatch(html,/href="https:\/\/windhawk.net\//)})
test('开关只显示重新读取的实际状态，批量部分失败逐条可见',async()=>{const{panel,calls}=make({view:{installed:true,layout:{portable:true,install:'X:/Windhawk'},mods:[{id:'test-mod',disabled:true,libraryFile:'test.dll',version:'1',metadata:{name:'<测试>',author:'Fish'}}]},outcomes:[{id:'test-mod',ok:false,error:'写入失败'}]});await panel.load();await panel.apply([{id:'test-mod',disabled:false}]);assert.match(panel.render(),/写入失败/);assert.match(panel.render(),/已停用/);assert.match(panel.render(),/&lt;测试&gt;/);assert.equal(calls.at(-1).command,'windhawk_state')})
test('取消提权保留状态并给出错误，未编译 mod 无启用入口',async()=>{const{panel}=make({fail:true,view:{installed:true,layout:{},mods:[{id:'draft',disabled:true,libraryFile:'',version:'1'}]}});await panel.load();await panel.apply([{id:'draft',disabled:false}],true);assert.match(panel.render(),/系统提权已取消/);assert.match(panel.render(),/未编译/);assert.match(panel.render(),/data-windhawk-toggle="draft"[^>]*disabled/)})

// —— 2.0.4 T-D（审计 A11）新增 ——
// 1) busy 文案来自实际阶段 + 配置预算（view.budget 由 windhawk_state 下发），
// 不再写死「20 秒 / 120 秒」。
test('busy 文案取实际阶段与配置预算，不再是写死的秒数',async()=>{
  const fs0=fs.readFileSync(path.join(__dirname,'../src/pet/windhawk-panel.js'),'utf8')
  let release, gate=new Promise(r=>{release=r})
  const window={},calls=[]
  vm.runInNewContext(fs0,{window})
  const panel=window.WindhawkPanel.create({lang:()=> 'zh',redraw(){},invoke:async(command)=>{
    calls.push(command)
    if(command==='windhawk_state')return {installed:true,mods:[],budget:{downloadSeconds:7,compileSeconds:9}}
    if(command==='windhawk_fetch_source'){await gate;return {source:'s',preview:{metadata:{id:'x',name:'x',version:'1'},fingerprint:'f'}}}
    if(command==='windhawk_preview'){await gate;return {metadata:{id:'x',name:'x',version:'1'},existing:null,fingerprint:'f'}}
    return {}
  }})
  await panel.load()
  const fetching=panel.fetchSource('https://example.com/a.wh.cpp')
  await new Promise(r=>setTimeout(r,0))
  assert.match(panel.render(),/最多 7 秒/,'下载阶段文案用下载预算')
  release();await fetching
  // 编译阶段：把 gate 换成一个新的挂起。
  let release2,gate2=new Promise(r=>{release2=r})
  const panel2ctx=window.WindhawkPanel.create({lang:()=> 'zh',redraw(){},invoke:async(command)=>{
    if(command==='windhawk_state')return {installed:true,mods:[],budget:{downloadSeconds:7,compileSeconds:9}}
    if(command==='windhawk_preview'){await gate2;return {metadata:{id:'x',name:'x',version:'1'},existing:null,fingerprint:'f'}}
    return {}
  }})
  await panel2ctx.load()
  const compiling=panel2ctx.inspect({name:'a.wh.cpp',size:10,text:async()=> 'src'})
  await new Promise(r=>setTimeout(r,0))
  assert.match(panel2ctx.render(),/最多 9 秒/,'编译阶段文案用编译预算')
  release2();await compiling
})

// 2) 失败时不收起源码区（<details open>），错误渲染在触发按钮下一行并带「重试」。
test('下载失败不收起导入区，错误可见且带重试',async()=>{
  const window={},calls=[]
  vm.runInNewContext(fs.readFileSync(path.join(__dirname,'../src/pet/windhawk-panel.js'),'utf8'),{window})
  let attempts=0
  const panel=window.WindhawkPanel.create({lang:()=> 'zh',redraw(){},invoke:async(command)=>{
    calls.push(command)
    if(command==='windhawk_state')return {installed:true,mods:[],budget:{compileSeconds:120,downloadSeconds:30}}
    if(command==='windhawk_fetch_source'){attempts++;throw Error('连接被重置')}
    return {}
  }})
  await panel.load()
  await panel.fetchSource('https://example.com/a.wh.cpp')
  const html=panel.render()
  assert.match(html,/<details open/,'失败后导入区仍展开')
  assert.match(html,/连接被重置/,'错误要显示')
  assert.match(html,/data-windhawk-retry/,'错误旁要有重试')
  assert.equal(attempts,1)
  // 再点一次下载（模拟重试入口的动作）仍然会真的再试，不被静默吞掉。
  await panel.fetchSource('https://example.com/a.wh.cpp')
  assert.equal(attempts,2)
})

// 3) 集合预览失败：集合区保持展开、错误可见、带重试；未确认前不会调用导入。
test('集合预览失败保留展开与重试，未确认不导入',async()=>{
  const window={},calls=[]
  vm.runInNewContext(fs.readFileSync(path.join(__dirname,'../src/pet/windhawk-panel.js'),'utf8'),{window})
  const panel=window.WindhawkPanel.create({lang:()=> 'zh',redraw(){},invoke:async(command)=>{
    calls.push(command)
    if(command==='windhawk_state')return {installed:true,mods:[],budget:{compileSeconds:120,downloadSeconds:30}}
    if(command==='windhawk_collection_preview')throw Error('集合清单格式无效')
    throw Error('unexpected '+command)
  }})
  await panel.load()
  await panel.inspectCollection({size:20,text:async()=> '{}'},[])
  const html=panel.render()
  assert.match(html,/<details open/,'集合区保持展开')
  assert.match(html,/集合清单格式无效/)
  assert.match(html,/data-windhawk-retry/)
  assert.equal(calls.filter(c=>c==='windhawk_collection_import').length,0,'未确认不导入')
})
