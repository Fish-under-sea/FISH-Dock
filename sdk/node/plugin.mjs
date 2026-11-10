// FISH Dock 插件协议 v1。stdout 专用于协议，业务日志请写 stderr。
const CATEGORIES = new Set(['credentials','roster','keys','config','exports','backups','tmp']);
const DEFAULT_LIMIT = 1024 * 1024;
export function createPlugin({ id, input = process.stdin, output = process.stdout,
  activate = async () => {}, deactivate = async () => {}, onEvent = async () => {}, commands = {},
  onError = () => {}, maxLine = DEFAULT_LIMIT, requestTimeout: initialTimeout = 5000 } = {}) {
  if (!/^[a-z0-9]+(?:-[a-z0-9]+)*\.[a-z0-9]+(?:-[a-z0-9]+)*$/.test(id ?? '')) throw new Error('插件 ID 格式无效');
  if (!Number.isSafeInteger(maxLine) || maxLine < 128 || maxLine > DEFAULT_LIMIT) throw new Error('协议帧大小无效');
  if (!Number.isSafeInteger(initialTimeout) || initialTimeout < 1 || initialTimeout > 30000) throw new Error('宿主调用时限无效');
  // 宿主调用时限：构造参数先定初值（默认 5000，与宿主热态下发值同一个数），握手时
  // 若宿主下发 hostCallTimeoutMs（T04 冷/热分档）则按它覆盖——宿主热态给 5000、
  // 冷态给 20000；旧宿主不带这个字段时就一直是构造时的初值。
  let requestTimeout = initialTimeout;
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
  function hostFor(capability, method, params) {
    if (!hello?.capabilities.includes(capability)) return Promise.reject(new Error(capability === 'storage' ? '插件未声明存储能力' : `插件未声明 ${capability} 能力`));
    if (closed || pending.size >= 64) return Promise.reject(new Error('宿主请求队列已满或已关闭'));
    const requestId = `sdk-${++sequence}`;
    return new Promise((resolve, reject) => {
      const timer = setTimeout(() => { pending.delete(requestId); reject(new Error('宿主调用超时')); }, requestTimeout);
      pending.set(requestId, { resolve, reject, timer });
      try { send({ id: requestId, method, params }); }
      catch (error) { clearTimeout(timer); pending.delete(requestId); reject(error); }
    });
  }
  function host(method, params) { return hostFor('storage', method, params); }
  function file(category, name) {
    if (!CATEGORIES.has(category) || typeof name !== 'string' || name.length > 128 || !/^[a-zA-Z0-9_.-]+$/.test(name)
      || name.includes('..') || name.toLowerCase() === 'readme.md' || /^(con|prn|aux|nul|com[1-9]|lpt[1-9])(?:\.|$)/i.test(name)) throw new Error('存储类别或文件名无效');
    return { category, name };
  }
  // 外部原生窗口与定向拖入（2.2.0）：与浏览器 SDK 同一套协议方法，宿主侧同一套
  // 权限、代次与句柄检查。句柄是宿主给的字符串，插件不能自造，也不从中读身份。
  const listeners = { windows: new Set(), fileDrop: new Set(), dock: new Set() };
  function listen(kind, listener) {
    if (typeof listener !== 'function') throw new TypeError('事件监听器必须是函数');
    listeners[kind].add(listener);
    return () => listeners[kind].delete(listener);
  }
  function handleOf(value, what) {
    if (typeof value !== 'string' || !value) throw new Error(`${what}句柄无效`);
    return value;
  }
  const windows = Object.freeze({
    async open(options = {}) {
      if (!options || typeof options !== 'object' || Array.isArray(options)) throw new Error('窗口参数应为对象');
      const result = await hostFor('windows', 'host.windows.open', options);
      return result?.handle;
    },
    async update(handle, options = {}) {
      return hostFor('windows', 'host.windows.update', { ...options, handle: handleOf(handle, '窗口') });
    },
    async close(handle) {
      const result = await hostFor('windows', 'host.windows.close', { handle: handleOf(handle, '窗口') });
      return !!result?.closed;
    },
    async list() {
      const result = await hostFor('windows', 'host.windows.list', {});
      return result?.windows ?? [];
    },
    // 2.2.3（B3）：把本窗口里的文件拖回资源管理器（系统 OLE 拖放）。
    // 立即返回；会话在宿主线程里跑到用户松手。
    async beginDragOut(handle, paths) {
      if (!Array.isArray(paths) || paths.length === 0) throw new Error('beginDragOut 需要文件路径数组');
      const result = await hostFor('windows', 'host.windows.beginDragOut', { handle: handleOf(handle, '窗口'), paths: paths.map(String) });
      return !!result?.started;
    },
    onEvent: listener => listen('windows', listener),
  });
  const fileDrop = Object.freeze({
    // scope: 'dock'（主 Dock，独占）或 'window' + handle（本插件自己的窗口）。
    async subscribe(options = {}) {
      const result = await hostFor('file-drop', 'host.fileDrop.subscribe', options);
      return result?.handle;
    },
    async unsubscribe(handle) {
      const result = await hostFor('file-drop', 'host.fileDrop.unsubscribe', { handle: handleOf(handle, '拖入订阅') });
      return !!result?.unsubscribed;
    },
    onEvent: listener => listen('fileDrop', listener),
  });
  // 2.2.2：主岛几何与状态（只读 + 事件订阅）。rect() 一次读实时快照；
  // onEvent 订阅后宿主才开始按需上报（没有任何订阅者时页面不上报，零开销）。
  const dock = Object.freeze({
    // 返回 { window, capsule, state:{view,stowed,hidden,fullscreen}, scale }；
    // capsule 在页面还没报过胶囊尺寸时为 null。
    async rect() {
      return hostFor('dock', 'host.dock.rect', {});
    },
    async onEvent(listener) {
      if (typeof listener !== 'function') throw new TypeError('事件监听器必须是函数');
      const off = listen('dock', listener);
      await hostFor('dock', 'host.dock.subscribe', {});
      return async () => {
        off();
        try { await hostFor('dock', 'host.dock.unsubscribe', {}); } catch { /* 会话已关：宿主那边已经清干净 */ }
      };
    },
    // 2.2.3（D2）：在 Dock 设置面板里打开本插件的内部页。
    // 这是「设置里的那一页」，不是再开一个外部窗口；pageId 必须是本插件
    // pages[] 里登记过的 id。
    async openPage(pageId) {
      if (typeof pageId !== 'string' || !pageId) throw new Error('openPage 需要一个页面 ID');
      return hostFor('dock', 'host.dock.openPage', { page: pageId });
    },
  });
  const context = Object.freeze({
    get dataDirectory() { return hello?.dataDirectory; },
    get storageIdentity() { return hello?.storage; },
    // 2.2.0：外部原生窗口与定向拖入（与浏览器 SDK 同名同义）。
    windows,
    fileDrop,
    // 2.2.2：主岛几何与状态。
    dock,
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
      // 宿主下发的宿主调用时限（T04）：登录后 5 分钟内宿主会按冷态档给更宽的值；
      // 旧宿主不带这个字段时回落构造时的 requestTimeout（默认 5000）。非法值
      // （非整数/越界）按没下发处理，不让握手因此失败。
      const offered = row.params.hostCallTimeoutMs;
      const parsed = Number.isSafeInteger(offered) && offered > 0 && offered <= 30000 ? offered : 0;
      if (parsed) requestTimeout = parsed;
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
      // 2.2.0：宿主单向通知本插件的窗口事件（ready/closed/anchorChanged/fileDrop）。
      // 与 plugin.event 分开：窗口事件属于 windows / file-drop 能力，不要求 events。
      if (row.method === 'plugin.windowEvent') {
        const payload = row.params ?? {};
        const need = payload.event === 'fileDrop' ? 'file-drop' : 'windows';
        if (!hello.capabilities.includes(need)) return error('CAPABILITY_REQUIRED',`插件未声明 ${need} 能力`);
        for (const listener of [...listeners.windows]) {
          try { await listener(payload); } catch { /* 监听器自己的错不该断会话 */ }
        }
        if (payload.event === 'fileDrop') {
          for (const listener of [...listeners.fileDrop]) {
            try { await listener(payload); } catch { /* 同上 */ }
          }
        }
        return reply({ handled:true });
      }
      if (!Object.hasOwn(commands,row.method) || typeof commands[row.method] !== 'function') return error('UNKNOWN_METHOD','插件方法未注册');
      return reply(await commands[row.method](row.params ?? {},context));
    } catch (thrown) {
      // 业务错误带码上行：插件自己抛出的 {code,message}（如 serviceError('PORT_IN_USE')）
      // 原样交给宿主，页面才能按码查表显示专属文案；认不出来的异常仍折叠成 COMMAND_FAILED，
      // 免得把可能夹带地址或凭据的原文透给宿主。
      const code = typeof thrown?.code === 'string' ? thrown.code : '';
      const message = typeof thrown?.message === 'string' ? thrown.message.trim() : '';
      if (/^[A-Z][A-Z0-9_]{0,39}$/.test(code) && message && message.length <= 200 && !/[\u0000-\u001f\u007f]/.test(message))
        return error(code, message);
      return error('COMMAND_FAILED','插件操作失败，请检查配置及运行状态');
    }
  }
  function accept(bytes) {
    let row;
    try { row = JSON.parse(new TextDecoder('utf-8',{fatal:true}).decode(bytes)); }
    catch { throw new Error('插件协议包含无效 JSON 或 UTF-8'); }
    if (!row || typeof row !== 'object' || Array.isArray(row)) throw new Error('插件协议应为 JSON 对象');
    if (!Object.hasOwn(row,'method')) {
      const waiter = pending.get(row.id);
      // 编号已经作废（宿主晚付的回执）就丢掉：这里以前直接 throw，receive 的 catch 会
      // close() 整个会话——一次超时之后插件再也不应答，宿主只能重启进程。
      if (!waiter) return;
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
