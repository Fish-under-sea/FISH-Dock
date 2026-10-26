// FISH Dock 插件协议 v1。stdout 专用于协议，业务日志请写 stderr。
const CATEGORIES = new Set(['credentials','roster','keys','config','exports','backups','tmp']);
const DEFAULT_LIMIT = 1024 * 1024;
export function createPlugin({ id, input = process.stdin, output = process.stdout,
  activate = async () => {}, deactivate = async () => {}, onEvent = async () => {}, commands = {},
  onError = () => {}, maxLine = DEFAULT_LIMIT, requestTimeout = 5000 } = {}) {
  if (!/^[a-z0-9]+(?:-[a-z0-9]+)*\.[a-z0-9]+(?:-[a-z0-9]+)*$/.test(id ?? '')) throw new Error('插件 ID 格式无效');
  if (!Number.isSafeInteger(maxLine) || maxLine < 128 || maxLine > DEFAULT_LIMIT) throw new Error('协议帧大小无效');
  if (!Number.isSafeInteger(requestTimeout) || requestTimeout < 1 || requestTimeout > 30000) throw new Error('宿主调用时限无效');
  let buffer = Buffer.alloc(0), closed = false, active = false, hello = null;
  let sequence = 0, queued = 0, chain = Promise.resolve();
  const pending = new Map();
  function close() {
    if (closed) return;
    closed = true; active = false;
    input.off('data', receive); input.off('end', ended); input.off('error', failed);
    output.off('error', failed);
    input.pause();
    for (const { reject, timer } of pending.values()) { clearTimeout(timer); reject(new Error('插件会话已关闭')); }
    pending.clear(); buffer = Buffer.alloc(0);
  }
  function failed() { close(); onError(new Error('插件通信已中断')); }
  function send(value) {
    if (closed) throw new Error('插件会话已关闭');
    const bytes = Buffer.from(JSON.stringify(value) + '\n');
    if (bytes.length > maxLine || output.writableLength > maxLine * 4) { close(); throw new Error('插件通信队列或帧超过限制'); }
    output.write(bytes);
  }
  function host(method, params) {
    if (!hello?.capabilities.includes('storage')) return Promise.reject(new Error('插件未声明存储能力'));
    if (closed || pending.size >= 64) return Promise.reject(new Error('宿主请求队列已满或已关闭'));
    const requestId = `sdk-${++sequence}`;
    return new Promise((resolve, reject) => {
      const timer = setTimeout(() => { pending.delete(requestId); reject(new Error('宿主调用超时')); }, requestTimeout);
      pending.set(requestId, { resolve, reject, timer });
      try { send({ id: requestId, method, params }); }
      catch (error) { clearTimeout(timer); pending.delete(requestId); reject(error); }
    });
  }
  function file(category, name) {
    if (!CATEGORIES.has(category) || typeof name !== 'string' || name.length > 128 || !/^[a-zA-Z0-9_.-]+$/.test(name)
      || name.includes('..') || name.toLowerCase() === 'readme.md' || /^(con|prn|aux|nul|com[1-9]|lpt[1-9])(?:\.|$)/i.test(name)) throw new Error('存储类别或文件名无效');
    return { category, name };
  }
  const context = Object.freeze({
    get dataDirectory() { return hello?.dataDirectory; },
    get storageIdentity() { return hello?.storage; },
    secrets: Object.freeze({
      async read(category, name) {
        const params=file(category,name);if(!['credentials','keys'].includes(category))throw new Error('加密凭据仅允许 credentials 或 keys 类别');
        return (await host('host.storage.readSecret',params)).value;
      },
      async write(category,name,value) {
        const params=file(category,name);if(!['credentials','keys'].includes(category)||typeof value!=='string'||Buffer.byteLength(value)>16384)throw new Error('加密凭据类别或大小无效');
        await host('host.storage.writeSecret',{...params,value});
      },
    }),
    storage: Object.freeze({
      async read(category, name) { const result = await host('host.storage.read', file(category,name)); return result.value; },
      async write(category, name, value) {
        if (typeof value !== 'string') throw new Error('存储写入内容应为文本');
        await host('host.storage.write', { ...file(category,name), value });
      },
    }),
  });
  async function dispatch(row) {
    if (closed) return;
    const requestId = row.id;
    if (!['string','number'].includes(typeof requestId) || typeof row.method !== 'string') throw new Error('插件请求格式无效');
    const reply = result => send({ id: requestId, result: result ?? null });
    const error = (code, message) => send({ id: requestId, error: { code, message } });
    if (row.method === 'host.hello') {
      if (hello || row.params?.protocol !== 1 || row.params?.pluginId !== id || !Array.isArray(row.params.capabilities)) return error('BAD_HANDSHAKE','宿主握手身份或协议无效');
      hello = Object.freeze({ ...row.params, capabilities: Object.freeze([...row.params.capabilities]) });
      return reply({ type:'plugin.hello', protocol:1, pluginId:id });
    }
    if (!hello) return error('HANDSHAKE_REQUIRED','请先完成宿主握手');
    try {
      if (row.method === 'plugin.activate') { if (!active) { await activate(context); active=true; } return reply({active:true}); }
      if (row.method === 'plugin.deactivate') { if (active) { active=false; await deactivate(context); } return reply({active:false}); }
      if (!active) return error('NOT_ACTIVE','插件尚未启用');
      if (row.method === 'plugin.event') {
        if (!hello.capabilities.includes('events')) return error('CAPABILITY_REQUIRED','插件未声明事件能力');
        return reply(await onEvent(row.params ?? {},context));
      }
      if (!Object.hasOwn(commands,row.method) || typeof commands[row.method] !== 'function') return error('UNKNOWN_METHOD','插件方法未注册');
      return reply(await commands[row.method](row.params ?? {},context));
    } catch { return error('COMMAND_FAILED','插件操作失败，请检查配置及运行状态'); }
  }
  function accept(bytes) {
    let row;
    try { row = JSON.parse(new TextDecoder('utf-8',{fatal:true}).decode(bytes)); }
    catch { throw new Error('插件协议包含无效 JSON 或 UTF-8'); }
    if (!row || typeof row !== 'object' || Array.isArray(row)) throw new Error('插件协议应为 JSON 对象');
    if (!Object.hasOwn(row,'method')) {
      const waiter = pending.get(row.id);
      if (!waiter) throw new Error('宿主响应编号不匹配');
      pending.delete(row.id); clearTimeout(waiter.timer);
      if (Object.hasOwn(row,'result') === Object.hasOwn(row,'error')) return waiter.reject(new Error('宿主响应结构无效'));
      if (row.error) waiter.reject(new Error('宿主存储操作失败')); else waiter.resolve(row.result);
      return;
    }
    if (++queued > 64) throw new Error('插件请求队列超过限制');
    chain = chain.then(() => dispatch(row)).catch(() => { close(); onError(new Error('插件协议处理失败')); }).finally(() => queued--);
  }
  function receive(chunk) {
    try {
      const bytes = Buffer.isBuffer(chunk) ? chunk : Buffer.from(chunk);
      let offset = 0;
      while (offset < bytes.length) {
        const newline = bytes.indexOf(10,offset), end = newline < 0 ? bytes.length : newline;
        if (buffer.length + end - offset + 1 > maxLine) throw new Error('插件协议帧超过限制');
        buffer = Buffer.concat([buffer,bytes.subarray(offset,end)]);
        if (newline < 0) break;
        if (!buffer.length) throw new Error('插件协议包含空帧');
        accept(buffer); buffer=Buffer.alloc(0); offset=newline+1;
      }
    } catch (error) { close(); onError(error); }
  }
  function ended() { const partial=buffer.length>0; close(); if (partial) onError(new Error('插件协议尾帧缺少换行')); }
  input.on('data',receive); input.on('end',ended); input.on('error',failed); output.on('error',failed);
  return Object.freeze({ close, get closed() { return closed; } });
}
