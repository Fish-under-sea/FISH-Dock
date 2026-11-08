// 合成示例的可运行闭环：在**进程内**用真实 SDK 跑一遍完整协议，不需要 Dock，
// 也不需要子进程（沙箱里 spawn 管道 stdio 会被拒）。
//
//   node sdk/examples/window-file-drop/harness.mjs
//
// 它扮演宿主：握手 → 激活 → 投一条主 Dock 拖入事件 → 回答 windows.open →
// 再投一条关闭事件，然后核对插件自己的业务状态。
import { PassThrough } from 'node:stream';
import assert from 'node:assert/strict';
import { plugin } from './backend.mjs';

const input = new PassThrough();
const output = new PassThrough();
const pending = [];
const rows = [];
let unsubscribed = 0;
let buffer = '';
output.on('data', chunk => {
  buffer += chunk;
  for (;;) {
    const index = buffer.indexOf('\n');
    if (index < 0) break;
    const row = JSON.parse(buffer.slice(0, index));
    buffer = buffer.slice(index + 1);
    const take = pending.shift();
    if (take) take(row); else rows.push(row);
  }
});
const send = row => input.write(JSON.stringify(row) + '\n');
const next = () => rows.length ? Promise.resolve(rows.shift()) : new Promise((resolve, reject) => {
  const timer = setTimeout(() => reject(new Error('示例协议响应超时')), 4000);
  pending.push(row => { clearTimeout(timer); resolve(row); });
});

// 插件 → 宿主的回调：示例里只有这几种，按协议回答。
async function answerHost(row) {
  if (row.method === 'host.fileDrop.subscribe') return send({ id: row.id, result: { handle: 'drop-1', scope: 'dock' } });
  if (row.method === 'host.fileDrop.unsubscribe') {
    assert.equal(row.params.handle, 'drop-1');
    unsubscribed++;
    return send({ id: row.id, result: { unsubscribed: true } });
  }
  if (row.method === 'host.windows.open') {
    assert.equal(row.params.page, 'panel', '窗口只能引用插件已登记的页面');
    return send({ id: row.id, result: { handle: 'h-1', key: row.params.key, ready: true } });
  }
  if (row.method === 'host.windows.update') return send({ id: row.id, result: { handle: row.params.handle } });
  if (row.method === 'host.windows.close') return send({ id: row.id, result: { closed: true } });
  if (row.method === 'host.windows.list') return send({ id: row.id, result: { windows: [] } });
  throw new Error(`示例没有预料到的宿主回调：${row.method}`);
}

// 读一行；带 method 的是插件回调，先替他回答，再继续等真正的回执。
async function reply() {
  for (;;) {
    const row = await next();
    if (row.method) { await answerHost(row); continue; }
    return row;
  }
}

const session = plugin({ input, output });
try {
  send({ id: 1, method: 'host.hello', params: { protocol: 1, pluginId: 'acme.notes', capabilities: ['storage', 'windows', 'file-drop'] } });
  assert.equal((await reply()).result.type, 'plugin.hello');

  // 激活期间插件自己订阅主 Dock 拖入（宿主回调 host.fileDrop.subscribe）。
  send({ id: 2, method: 'plugin.activate', params: {} });
  assert.deepEqual((await reply()).result, { active: true });

  send({ id: 3, method: 'status', params: {} });
  assert.deepEqual((await reply()).result, { dropped: 0, panel: null }, '还没拖东西时没有窗口');

  // 主 Dock 的一次真实拖入：宿主把路径交给这个插件，插件据此开自己的窗口。
  send({ id: 4, method: 'plugin.windowEvent', params: { event: 'fileDrop', phase: 'drop', scope: 'dock', handle: 'drop-1', paths: ['sample/report.txt', 'sample/shot.png'] } });
  assert.deepEqual((await reply()).result, { handled: true });

  send({ id: 5, method: 'status', params: {} });
  assert.deepEqual((await reply()).result, { dropped: 2, panel: 'h-1' }, '收到两份路径并开了自己的窗口');

  // 用户关掉窗口：插件收到 closed 事件，不再持有句柄。
  send({ id: 6, method: 'plugin.windowEvent', params: { event: 'closed', handle: 'h-1', reason: 'user' } });
  assert.deepEqual((await reply()).result, { handled: true });

  send({ id: 7, method: 'status', params: {} });
  assert.deepEqual((await reply()).result, { dropped: 2, panel: null });

  send({ id: 8, method: 'plugin.deactivate', params: {} });
  assert.deepEqual((await reply()).result, { active: false });
  assert.equal(unsubscribed, 0, '宿主在停用前已撤销订阅，deactivate 不得再调用失效的会话');
  console.log('示例通过：握手 → 激活 → 订阅 → 拖入开窗 → 关闭 → 停用本地清理');
} finally {
  session?.close?.();
}