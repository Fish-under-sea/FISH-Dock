import test from 'node:test';
import assert from 'node:assert/strict';
import { PassThrough } from 'node:stream';
import { createPlugin } from './plugin.mjs';
function fixture(options = {}) {
  const input = new PassThrough(), output = new PassThrough();
  const rows = [], waiters = [];
  let buffer = '';
  output.on('data', chunk => {
    buffer += chunk;
    for (;;) {
      const index = buffer.indexOf('\n'); if (index < 0) break;
      const row = JSON.parse(buffer.slice(0,index)); buffer = buffer.slice(index+1);
      const take = waiters.shift(); if (take) take(row); else rows.push(row);
    }
  });
  const plugin = createPlugin({ id: 'acme.weather', input, output, ...options });
  const send = row => input.write(JSON.stringify(row)+'\n');
  const next = () => rows.length ? Promise.resolve(rows.shift()) : new Promise((resolve,reject) => {
    const timer = setTimeout(() => reject(new Error('协议测试响应超时')), 1500);
    waiters.push(row => { clearTimeout(timer); resolve(row); });
  });
  // rows 也交出去：反过来的断言（「没拒绝」）要能直接看有没有新的拒绝行落地。
  return { plugin, input, send, next, rows };
}
async function activate(f) {
  f.send({id:1,method:'host.hello',params:{protocol:1,pluginId:'acme.weather',capabilities:['storage'],dataDirectory:'fixture'}});
  assert.equal((await f.next()).result.type,'plugin.hello');
  f.send({id:2,method:'plugin.activate',params:{}}); assert.deepEqual((await f.next()).result,{active:true});
}
test('校验握手身份、执行生命周期、停用后撤销命令', async () => {
  let activations=0,deactivations=0;
  const f=fixture({activate:()=>{activations++},deactivate:()=>{deactivations++},commands:{echo:value=>value}});
  try {
    f.send({id:0,method:'host.hello',params:{protocol:1,pluginId:'fish.ofm'}});
    assert.equal((await f.next()).error.code,'BAD_HANDSHAKE');
    await activate(f);assert.equal(activations,1);
    f.send({id:3,method:'echo',params:{value:42}});assert.deepEqual((await f.next()).result,{value:42});
    f.send({id:4,method:'plugin.deactivate'});assert.equal((await f.next()).result.active,false);assert.equal(deactivations,1);
    f.send({id:5,method:'echo',params:{}});assert.equal((await f.next()).error.code,'NOT_ACTIVE');
  } finally {f.plugin.close();}
});
test('存储经宿主能力与分类接口回调，不把磁盘路径当公开接口', async () => {
  const f=fixture({commands:{save:async (_,context)=>{await context.storage.write('keys','api.json','synthetic');return context.storage.read('keys','api.json');}}});
  try {
    await activate(f);f.send({id:3,method:'save',params:{}});
    const write=await f.next();assert.equal(write.method,'host.storage.write');assert.deepEqual(write.params,{category:'keys',name:'api.json',value:'synthetic'});
    f.send({id:write.id,result:{written:true}});
    const read=await f.next();assert.equal(read.method,'host.storage.read');f.send({id:read.id,result:{value:'synthetic'}});
    assert.equal((await f.next()).result,'synthetic');
  } finally {f.plugin.close();}
});
test('命令异常不泄漏错误详情；存储文件越界提前失败', async () => {
  const f=fixture({commands:{broken:()=>{throw new Error('synthetic-secret')},escape:(_,ctx)=>ctx.storage.read('keys','../secret')}});
  try {
    await activate(f);for (const method of ['broken','escape']) {f.send({id:3,method});const row=await f.next();assert.equal(row.error.code,'COMMAND_FAILED');assert.ok(!JSON.stringify(row).includes('synthetic-secret'));}
  } finally {f.plugin.close();}
});
test('加密存储采用独立宿主方法，事件能力控制分发且停用后停止',async()=>{
 let events=0;
 const f=fixture({onEvent:async row=>{events++;return row.type},commands:{saveSecret:async(_,ctx)=>{await ctx.secrets.write('credentials','robot.json','synthetic');return ctx.secrets.read('credentials','robot.json')}}});
 try{
  f.send({id:1,method:'host.hello',params:{protocol:1,pluginId:'acme.weather',capabilities:['storage','events']}});await f.next();f.send({id:2,method:'plugin.activate'});await f.next();
  f.send({id:3,method:'saveSecret'});const write=await f.next();assert.equal(write.method,'host.storage.writeSecret');assert.equal(write.params.value,'synthetic');f.send({id:write.id,result:{written:true}});
  const read=await f.next();assert.equal(read.method,'host.storage.readSecret');f.send({id:read.id,result:{value:'synthetic'}});assert.equal((await f.next()).result,'synthetic');
  f.send({id:4,method:'plugin.event',params:{type:'agent.alert'}});assert.equal((await f.next()).result,'agent.alert');assert.equal(events,1);
  f.send({id:5,method:'plugin.deactivate'});await f.next();f.send({id:6,method:'plugin.event',params:{type:'agent.alert'}});assert.equal((await f.next()).error.code,'NOT_ACTIVE');assert.equal(events,1);
 }finally{f.plugin.close()}
 const noEvents=fixture({onEvent:()=>{events++}});try{await activate(noEvents);noEvents.send({id:3,method:'plugin.event',params:{type:'agent.alert'}});assert.equal((await noEvents.next()).error.code,'CAPABILITY_REQUIRED');assert.equal(events,1)}finally{noEvents.plugin.close()}
});
// A2 下游（2.1.6）：宿主晚到的响应（编号已经超时作废）不许把会话打死。
// 旧实现 pending 里找不到就 throw，receive 的 catch 直接 close()——一次超时之后插件
// 再也不应答，只能重启进程，宿主侧看到的是「插件响应超时」。
test('超时作废的迟到响应被忽略，会话继续可用', async () => {
  const f=fixture({commands:{probe:async(_,ctx)=>{try{await ctx.storage.read('keys','x.json');return 'answered'}catch{throw Object.assign(new Error('TIMEOUT'),{code:'HOST_TIMEOUT'})}},echo:value=>value}});
  try {
    f.send({id:1,method:'host.hello',params:{protocol:1,pluginId:'acme.weather',capabilities:['storage'],hostCallTimeoutMs:5}});
    await f.next();
    f.send({id:2,method:'plugin.activate',params:{}});await f.next();
    f.send({id:3,method:'probe',params:{}});
    const write=await f.next();assert.equal(write.method,'host.storage.read'); // 宿主故意不答
    const timedOut=await f.next();assert.equal(timedOut.error.code,'HOST_TIMEOUT');
    // 宿主此刻才把那条已经作废的编号回写回来。
    f.send({id:write.id,result:{value:'late'}});
    f.send({id:4,method:'echo',params:{value:'alive'}});
    const echo=await f.next();
    assert.deepEqual(echo.result,{value:'alive'},'迟到响应不得关闭会话');
    assert.equal(f.plugin.closed,false,'会话必须仍然可用');
  } finally {f.plugin.close();}
});
test('无换行超大帧终止读取且报告帧错误', async () => {
  let error;const f=fixture({maxLine:128,onError:err=>{error=err;}});
  try {f.input.write('x'.repeat(129));await new Promise(resolve=>setImmediate(resolve));assert.match(error?.message??'',/超过/);assert.equal(f.plugin.closed,true);}
  finally {f.plugin.close();}
});
// T04：宿主在 host.hello 下发 hostCallTimeoutMs，SDK 的宿主调用时限按它走；
// 旧宿主不带字段回落默认 5000；非法值按没下发处理（握手不因此失败）。
test('host.hello 的 hostCallTimeoutMs 覆盖宿主调用时限', async () => {
  const waits=[];
  // 用一条 storage 回调的时序验证时限值：宿主故意不答，看拒绝快慢。
  // 这里直接读内部行为太绕，改为验证「下发值生效」的最小闭环——
  // 用 1 ms 时限：宿主不答时 reject 必须几乎立刻发生。
  const f=fixture({commands:{probe:async(_,ctx)=>{try{await ctx.storage.read('keys','x.json');return 'answered'}catch{throw Object.assign(new Error('TIMEOUT'),{code:'HOST_TIMEOUT'})}}}});
  try {
    f.send({id:1,method:'host.hello',params:{protocol:1,pluginId:'acme.weather',capabilities:['storage'],hostCallTimeoutMs:1}});
    await f.next();
    f.send({id:2,method:'plugin.activate',params:{}});await f.next();
    const started=Date.now();
    f.send({id:3,method:'probe',params:{}});
    const write=await f.next();assert.equal(write.method,'host.storage.read'); // 宿主调用发出，不答
    const row=await f.next();assert.equal(row.error.code,'HOST_TIMEOUT');
    waits.push(Date.now()-started);
    assert.ok(waits[0]<1000,'1 ms 时限下宿主调用必须立刻超时拒绝');
  } finally {f.plugin.close();}
});
test('host.hello 不带 hostCallTimeoutMs 时回落默认，非法值按未下发', async () => {
  // 旧宿主：字段缺席（默认 5000 生效，真正的反证见下一条用例），
  // 这里只确认握手照常成功；再用越界值确认「非法值按未下发」也不让握手失败。
  const f=fixture({});
  try {
    f.send({id:1,method:'host.hello',params:{protocol:1,pluginId:'acme.weather',capabilities:['storage']}});
    const ok=await f.next();assert.equal(ok.result.type,'plugin.hello');
  } finally {f.plugin.close();}
  const bad=fixture({});
  try {
    bad.send({id:1,method:'host.hello',params:{protocol:1,pluginId:'acme.weather',capabilities:['storage'],hostCallTimeoutMs:999999}});
    const still=await bad.next();assert.equal(still.result.type,'plugin.hello','非法值按未下发，握手照常成功');
  } finally {bad.plugin.close();}
});
// T04 / F2 的反向断言：字段缺席时必须走默认 5000 一档——宿主不答时，50 ms 内**不许**
// 出现拒绝。0/1 ms 的时限会在这里当场拒绝，所以这条能真正区分「默认 5000」与「0/1 ms」，
// 而不是只断言握手成功那种摆设。不真等 5 s：等 50 ms 就够反证「远远大于 50 ms」。
test('host.hello 不下发 hostCallTimeoutMs 时，宿主调用 50 ms 内不得被拒绝', async () => {
  const f=fixture({commands:{probe:async(_,ctx)=>{try{await ctx.storage.read('keys','x.json');return 'answered'}catch{throw Object.assign(new Error('CLOSED'),{code:'HOST_TIMEOUT'})}}}});
  try {
    f.send({id:1,method:'host.hello',params:{protocol:1,pluginId:'acme.weather',capabilities:['storage']}});
    assert.equal((await f.next()).result.type,'plugin.hello','字段缺席时握手照常成功');
    f.send({id:2,method:'plugin.activate',params:{}});await f.next();
    f.send({id:3,method:'probe',params:{}});
    const write=await f.next();assert.equal(write.method,'host.storage.read'); // 宿主故意不答
    await new Promise(resolve=>setTimeout(resolve,50));
    assert.equal(f.rows.length,0,'50 ms 内不许有拒绝行：默认 5000（0/1 ms 的时限会在这里就超时）');
    assert.equal(f.plugin.closed,false,'反证期间会话仍在，说明是「等」而不是「已断」');
  } finally {f.plugin.close();}
});

// 2.2.0：外部原生窗口与定向拖入。SDK 只做三件事——能力不满足就本地拒绝、
// 把同一套协议方法发给宿主、把宿主的单向窗口事件分给监听器；身份与句柄由宿主校验。
async function handshake(f,capabilities){
  f.send({id:1,method:'host.hello',params:{protocol:1,pluginId:'acme.weather',capabilities}});
  assert.equal((await f.next()).result.type,'plugin.hello');
  f.send({id:2,method:'plugin.activate',params:{}});assert.deepEqual((await f.next()).result,{active:true});
}
test('窗口：能力、句柄与代次都由宿主校验，SDK 只转发协议', async () => {
  const seen=[];
  const f=fixture({commands:{
    panel:async(_,ctx)=>({handle:await ctx.windows.open({key:'panel',page:'main',width:420})}),
    resize:async(_,ctx)=>ctx.windows.update('h-1',{height:260}),
    shut:async(_,ctx)=>ctx.windows.close('h-1'),
    listing:async(_,ctx)=>ctx.windows.list(),
  }});
  try{
    await handshake(f,['storage']);
    f.send({id:3,method:'panel'});
    assert.equal((await f.next()).error.code,'COMMAND_FAILED','没声明 windows 能力时本地就拒绝');
    assert.equal(f.rows.length,0,'本地拒绝不该向宿主要求开窗');

    f.send({id:4,method:'host.hello',params:{protocol:1,pluginId:'acme.weather',capabilities:['storage','windows','file-drop']}});
    // 重复握手在 SDK 里是 BAD_HANDSHAKE：新能力要重新拉起会话才生效（测试里另起一个）。
  } finally {f.plugin.close();}

  const w=fixture({commands:{
    panel:async(_,ctx)=>({handle:await ctx.windows.open({key:'panel',page:'main',width:420})}),
    resize:async(_,ctx)=>ctx.windows.update('h-1',{height:260}),
    shut:async(_,ctx)=>ctx.windows.close('h-1'),
    listing:async(_,ctx)=>ctx.windows.list(),
  }});
  try{
    await handshake(w,['storage','windows','file-drop']);
    w.send({id:3,method:'panel'});
    const open=await w.next();
    assert.equal(open.method,'host.windows.open');
    assert.deepEqual(open.params,{key:'panel',page:'main',width:420});
    w.send({id:open.id,result:{handle:'h-1',key:'panel',ready:true}});
    assert.equal((await w.next()).result.handle,'h-1','open 把宿主的 opaque handle 交回');

    w.send({id:4,method:'resize'});
    const update=await w.next();
    assert.equal(update.method,'host.windows.update');
    assert.deepEqual(update.params,{height:260,handle:'h-1'},'update 必须带上自己的句柄');
    w.send({id:update.id,result:{handle:'h-1'}});
    assert.equal((await w.next()).result.handle,'h-1');

    w.send({id:5,method:'shut'});
    const close=await w.next();
    assert.equal(close.method,'host.windows.close');
    assert.deepEqual(close.params,{handle:'h-1'});
    w.send({id:close.id,result:{closed:true}});
    assert.equal((await w.next()).result,true);

    w.send({id:6,method:'listing'});
    const list=await w.next();
    assert.equal(list.method,'host.windows.list');
    w.send({id:list.id,result:{windows:[{handle:'h-1',key:'panel'}]}});
    assert.deepEqual((await w.next()).result,[{handle:'h-1',key:'panel'}]);
    void seen;
  } finally {w.plugin.close();}
});
test('窗口事件：宿主单向通知按能力分发给 windows / fileDrop 监听器', async () => {
  const windowsSeen=[],dropsSeen=[],off=[];
  const f=fixture({commands:{wire:async(_,ctx)=>{off.push(ctx.windows.onEvent(row=>windowsSeen.push(row)));off.push(ctx.fileDrop.onEvent(row=>dropsSeen.push(row)));return 'wired'}}});
  try{
    await handshake(f,['storage','windows','file-drop']);
    f.send({id:3,method:'wire'});assert.equal((await f.next()).result,'wired');
    f.send({id:9,method:'plugin.windowEvent',params:{event:'ready',handle:'h-1'}});
    assert.deepEqual((await f.next()).result,{handled:true});
    assert.equal(windowsSeen.length,1);assert.equal(windowsSeen[0].event,'ready');
    f.send({id:10,method:'plugin.windowEvent',params:{event:'fileDrop',phase:'drop',scope:'window',handle:'h-1',paths:['synthetic-one.txt','synthetic-two.png']}});
    assert.deepEqual((await f.next()).result,{handled:true});
    assert.equal(windowsSeen.length,2,'窗口监听器也收到拖入事件');
    assert.equal(dropsSeen.length,1);assert.deepEqual(dropsSeen[0].paths,['synthetic-one.txt','synthetic-two.png']);
    // 监听器抛错不断会话：后面还能正常应答。
    const off2=off.slice();off2.forEach(fn=>fn());
    f.send({id:11,method:'plugin.windowEvent',params:{event:'closed',handle:'h-1',reason:'user'}});
    assert.deepEqual((await f.next()).result,{handled:true});
    assert.equal(windowsSeen.length,2,'退订之后不再分发');
  } finally {f.plugin.close();}
});
test('窗口事件：能力不足时宿主通知被拒绝且监听器不被调用', async () => {
  let seen=0;
  const f=fixture({commands:{wire:async(_,ctx)=>{ctx.windows.onEvent(()=>{seen++});return 'wired'}}});
  try{
    await handshake(f,['storage']);
    f.send({id:3,method:'wire'});assert.equal((await f.next()).result,'wired');
    f.send({id:9,method:'plugin.windowEvent',params:{event:'ready',handle:'h-1'}});
    assert.equal((await f.next()).error.code,'CAPABILITY_REQUIRED');
    assert.equal(seen,0,'没有 windows 能力时监听器不该被调用');
    // fileDrop 订阅同样本地拒绝。
    const g=fixture({commands:{wire:async(_,ctx)=>ctx.fileDrop.subscribe({scope:'dock'})}});
    try{await handshake(g,['storage','windows']);g.send({id:3,method:'wire'});assert.equal((await g.next()).error.code,'COMMAND_FAILED')}
    finally{g.plugin.close()}
  } finally {f.plugin.close();}
});
