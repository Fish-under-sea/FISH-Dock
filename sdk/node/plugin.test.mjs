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
  return { plugin, input, send, next };
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
test('无换行超大帧终止读取且报告帧错误', async () => {
  let error;const f=fixture({maxLine:128,onError:err=>{error=err;}});
  try {f.input.write('x'.repeat(129));await new Promise(resolve=>setImmediate(resolve));assert.match(error?.message??'',/超过/);assert.equal(f.plugin.closed,true);}
  finally {f.plugin.close();}
});
