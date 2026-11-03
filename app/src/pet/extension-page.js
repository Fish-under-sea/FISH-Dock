// 隔离页面由宿主票据绑定插件，子页面只发送业务方法与参数。
;(function () {
  // 被 park 的插件页面（设置面板关了、切到别的标签页）多久之后整页退场。
  //
  // 为什么必须有这个上限：插件页里的「管理界面」是 gateway.open 起的**跨源**
  // iframe（插件自己的 127.0.0.1 服务），跨源 iframe 在 WebView2 里是独立的
  // renderer 进程，park 之后它一直挂在隐藏的 parked 容器里不释放。本机实测
  // （scripts/mem-sample.ps1，2026-10-30，现装 2.0.0，进程年龄 1 小时 50 分）：
  // 设置面板在 16:07:21 打开、16:07:28 关闭，那个 renderer 16:07:25 建起来后
  // 常驻到 17:56 仍在 —— 私有工作集 141.3 MB、RSS 73.2 MB、42 线程、518 句柄，
  // 比 pet 与岛两个页面共用的那个 renderer（83.3 MB 私有）还重。
  // 红线《禁止屎山十条》内存预算：「不可见的 webview 不得常驻」。
  //
  // 退场只动这一页的 iframe 与票据：插件自己的服务是插件后端起的独立进程，
  // 不受影响；再进来时 enter 重新取票据、重新装载，等同于 dock 重启后第一次
  // 打开该页，功能一件不少（不是用砍功能换内存）。
  const PARK_DISCARD_MS = 5 * 60 * 1000

  // 服务工具栏（gateway.open 起的本机服务页）那一条的高度范围：它只是工具栏，
  // 不是「整个插件页」的高度（A15：52 曾被当成普通页初值）。
  const TOOLBAR_MIN = 44, TOOLBAR_MAX = 320, TOOLBAR_FALLBACK = 52
  // 普通插件页在首个有效 layout 之前的稳定占位高度：宁可先给一块固定高度，
  // 也不要让面板从 0 长出来（N-1「先半屏」）。最终值待真机测量后再定。
  const PAGE_PLACEHOLDER_H = 240
  // 首个有效 layout 的独立 deadline：和 ready 握手分开计时（A14）。
  const FIRST_LAYOUT_MS = 8000
  // 没有 per-key 记忆时，第一次上报最多等多久（首个 layout / gateway.open 通常远早于此）。
  // t19 F4：先报普通页占位再缩成工具栏高度，就是换了个位置的「先大后小」。
  const FIRST_REPORT_MS = 400

  function identity(event, frame, ticket) { return event.source === frame.contentWindow && event.origin === 'null' && event.data?.ticket === ticket }
  function accepts(event, frame, ticket) {
    const row = event.data
    if (!identity(event,frame,ticket) || row.type !== 'fish:call' || !Number.isSafeInteger(row.id) || row.id < 1 || typeof row.method !== 'string' || !/^[a-zA-Z0-9_.-]{1,100}$/.test(row.method) || /^(host|plugin)\./.test(row.method)) return false
    try { return JSON.stringify(row).length <= 512 * 1024 } catch { return false }
  }
  function create({ invoke, lang, onLayout = () => {}, hostWindow = window, document = window.document, parkDiscardMs = PARK_DISCARD_MS, firstLayoutMs = FIRST_LAYOUT_MS, firstReportMs = FIRST_REPORT_MS, placeholderHeight = PAGE_PLACEHOLDER_H, remembered = () => null }) {
    const frames = new Map(), parked = document.createElement('div')
    parked.hidden = true; document.body.appendChild(parked)
    let current = '', revision = 0
    // 被 park、正等着到点退场的那一页，与它的计时器（PARK_DISCARD_MS）。
    let parkedKey = '', discarding = null
    const text = (zh,en) => lang() === 'en' ? en : zh
    function theme(record) {
      const light = document.body.classList.contains('tb-light')
      const target = document.getElementById('island-settings') || document.body
      const style = hostWindow.getComputedStyle?.(target)
      const colors = { '--dock-bg': light ? '#f3f3f3' : '#000' }
      for (const [name, source] of Object.entries({ card:'--g1',control:'--g2',line:'--g3',muted:'--g5',text:'--fg' })) {
        const value = style?.getPropertyValue(source).trim()
        if (value) colors[`--dock-${name}`] = value
      }
      colors['--dock-accent'] = light ? '#005fb8' : '#5e9bff'
      record.frame.contentWindow.postMessage({ type:'fish:theme', ticket:record.ticket, title:record.title, mode:light?'light':'dark', colors }, '*')
    }
    // 首个有效 layout 之前用宿主侧 per-key 记忆（settings.js 的 extensionLayouts），
    // 不要求沙盒 iframe 访问 localStorage；没有记忆才退到固定占位高度。
    function rememberedHeight(key) {
      try {
        const last = remembered(key)
        return last && Number.isFinite(last.height) && last.height > 0 ? last.height : 0
      } catch { return 0 }
    }
    // 普通页整窗高度、服务工具栏高度、业务 iframe 三种尺寸分开（A15）：
    // 服务页上报的是工具栏那一条（44–320），不是整个窗口。
    function reportedHeight(record) {
      if (record.isService) return Math.max(TOOLBAR_MIN, Math.min(TOOLBAR_MAX, record.toolbarHeight || TOOLBAR_FALLBACK))
      return record.pageHeight || rememberedHeight(record.key) || placeholderHeight
    }
    function report(record) {
      if (frames.get(current) !== record) return
      // 第一次真正上报之后，兜底计时就没有意义了（t19 F4）。
      record.reported = true
      if (record.initialTimer) { clearTimeout(record.initialTimer); record.initialTimer = null }
      onLayout(current, { height: reportedHeight(record), layout: record.layout || 'single', service: !!record.isService })
    }
    // 工具栏 iframe 自身的高度；普通页不碰（它的高度由 CSS 100% 撑满）。
    function applyServiceFrame(record) {
      if (!record.isService) return
      record.frame.style.height = `${Math.max(TOOLBAR_MIN, Math.min(TOOLBAR_MAX, record.toolbarHeight || TOOLBAR_FALLBACK))}px`
    }
    // 第一个有效 layout 到了：撤掉 deadline，清掉「没报尺寸」的提示。
    function markLayout(record) {
      if (!record.layoutSeen) { record.layoutSeen = true; clearTimeout(record.layoutTimer); record.layoutTimer = null }
      if (record.notice) { try { record.notice.remove?.() } catch {} ; record.notice = null }
    }
    // 首个 layout 超时：不藏页面、也不假装它好了——在帧前面给一条可重试的状态。
    function showLayoutTimeout(record) {
      if (record.layoutSeen || record.notice || frames.get(current) !== record) return
      try {
        const box = document.createElement('div')
        box.className = 'extension-page-notice'
        const line = document.createElement('p')
        line.setAttribute('role', 'status')
        line.textContent = text('插件页面没有报告尺寸，可能显示不完整。', 'The extension page did not report its size; it may be clipped.')
        const again = document.createElement('button')
        again.type = 'button'; again.className = 'pbtn'
        again.textContent = text('重试', 'Retry')
        again.onclick = () => {
          // t19 F3：retire() 会把 current 清空，所以「是不是当前页」必须在它之前记住，
          // 否则条件永远为假——用户点重试只会看到空白，且不会再取票。
          const key = record.key, page = record.page, root = record.root
          const wasCurrent = frames.get(current) === record
          retire(key)
          if (wasCurrent) void enter(root, page)
        }
        box.appendChild(line); box.appendChild(again)
        record.notice = box
        const shell = record.outer || record.frame
        if (shell && typeof shell.parentNode?.insertBefore === 'function') shell.parentNode.insertBefore(box, shell)
        else if (record.root && typeof record.root.replaceChildren === 'function') record.root.replaceChildren(box, shell)
      } catch { /* 夹具没有真实 DOM 时只保留状态 */ }
    }
    function retire(key) {
      const record = frames.get(key); if (!record) return
      if (parkedKey === key) clearDiscard()
      clearTimeout(record.timer); clearTimeout(record.layoutTimer); clearTimeout(record.initialTimer); (record.outer||record.frame).remove(); frames.delete(key)
      if (record.ticket) void invoke('extension_page_close',{ ticket:record.ticket }).catch(() => {})
      // 正在看的那一页被 retire（升级后 digest 变了）：清掉 current，否则下一次
      // enter 看到 key 没变就直接返回，留下一块空白，用户永远看不到新界面。
      if (current === key) { current = ''; revision++ }
    }
    function clearDiscard() {
      if (discarding) { clearTimeout(discarding); discarding = null }
      parkedKey = ''
    }
    // park 之后开始计时：到点还没人回来看这一页，就把它整页退场，它那个
    // 跨源 renderer 进程随之退出（理由与实测数字见 PARK_DISCARD_MS）。
    function armDiscard(key) {
      clearDiscard()
      parkedKey = key
      discarding = setTimeout(() => { discarding = null; retire(key) }, parkDiscardMs)
    }
    function sync(pages) {
      for (const [key,record] of frames) if (!pages.some(p => p.key === key && p.digest === record.digest)) retire(key)
    }
    function park() {
      const key = current
      if (key && frames.has(key)) parked.appendChild(frames.get(key).outer||frames.get(key).frame)
      current = ''; revision++
      // 这一页从此刻起「看不见」：给它上退场计时（PARK_DISCARD_MS）。
      if (key) armDiscard(key)
    }
    function notice(root,message,retry) {
      root.replaceChildren()
      const p = document.createElement('p'); p.setAttribute('role','status'); p.textContent = message; root.appendChild(p)
      if (retry) { const button = document.createElement('button'); button.type='button'; button.className='pbtn'; button.textContent=text('重试','Retry'); button.onclick=retry; root.appendChild(button) }
    }
    async function enter(root,page) {
      if (current === page.key) return
      park(); current=page.key; const asked=revision
      const existing=frames.get(page.key)
      // 回来看的正是等着退场的那一页：把计时撤掉，页面原样接着用。
      if (existing) { if (parkedKey === page.key) clearDiscard(); root.replaceChildren(existing.outer||existing.frame); theme(existing); report(existing); return }
      notice(root,text('正在打开插件页面…','Opening extension…'))
      let granted
      try {
        granted=await invoke('extension_page',{key:page.key})
        if (asked !== revision || current !== page.key) { void invoke('extension_page_close',{ticket:granted.ticket}).catch(()=>{}); return }
        const frame=document.createElement('iframe')
        frame.title=page.title; frame.className='extension-frame'; frame.setAttribute('sandbox','allow-scripts'); frame.setAttribute('referrerpolicy','no-referrer')
        const outer=document.createElement('div');outer.className='extension-service-shell';outer.appendChild(frame)
        const record={frame,outer,key:page.key,page,root,title:page.title,ticket:granted.ticket,digest:page.digest,pending:new Set(),ready:false,timer:null,layoutTimer:null,layoutSeen:false,pageHeight:0,toolbarHeight:0,isService:false,layout:'single',notice:null,reported:false,initialTimer:null}
        frames.set(page.key,record)
        record.timer=setTimeout(()=>{
          if (record.ready) return
          retire(page.key)
          if (current===page.key) notice(root,text('页面加载失败，请重试或检查插件状态。','Page failed to load. Retry or check the extension.'),()=>{current='';void enter(root,page)})
        },8000)
        // 首个有效 layout 的独立 deadline：ready 只说明脚本跑起来了，不说明尺寸到了。
        record.layoutTimer=setTimeout(()=>showLayoutTimeout(record),firstLayoutMs)
        root.replaceChildren(outer); frame.src=granted.url
        // t19 F4：服务页的工具栏高度要等 gateway.open 才知道。先按普通页占位报 240、
        // 再缩成 52 就是换了个位置的「先大后小」。所以：
        //   · 有记忆：立刻按记忆报（记忆带 service 就用工具栏高度）；
        //   · 无记忆：推迟到「首个 layout / gateway.open / firstReportMs 兜底」里最先到的那个。
        const memory = remembered(record.key)
        if (memory && Number.isFinite(memory.height) && memory.height > 0) {
          if (memory.service === true) { record.isService = true; record.toolbarHeight = memory.height; applyServiceFrame(record) }
          else record.pageHeight = memory.height
          report(record)
        } else {
          record.initialTimer = setTimeout(() => { record.initialTimer = null; report(record) }, firstReportMs)
        }
      } catch (error) {
        if (asked === revision && current === page.key) { current=''; notice(root,String(error?.message||error),()=>void enter(root,page)) }
      }
    }
    async function receive(event) {
      const record=[...frames.values()].find(record=>identity(event,record.frame,record.ticket))
      if (!record) return
      if (event.data?.type === 'fish:ready') { record.ready=true; clearTimeout(record.timer); theme(record); return }
      if (event.data?.type === 'fish:layout') {
        const height = Number(event.data.height)
        if (['single','double'].includes(event.data.layout)) record.layout = event.data.layout
        if (Number.isFinite(height) && height > 0 && height <= 10000) {
          markLayout(record)
          if (record.isService) { record.toolbarHeight = height; applyServiceFrame(record); report(record) }
          else { record.pageHeight = height; report(record) }
        }
        return
      }
      if (!accepts(event,record.frame,record.ticket) || record.pending.size>=16 || record.pending.has(event.data.id)) return
      const row=event.data;record.pending.add(row.id)
      let response
      try {
        const result=await invoke('extension_page_call',{ticket:record.ticket,method:row.method,params:row.params??{}})
        if(row.method==='gateway.open'){
          const url=new URL(result.url);if(url.protocol!=='http:'||url.hostname!=='127.0.0.1'||url.username||url.password)throw Error('服务地址无效')
          if(!record.outer){record.outer=document.createElement('div');record.outer.className='extension-service-shell';record.frame.replaceWith(record.outer);record.outer.appendChild(record.frame);record.frame.classList.add('extension-service-toolbar')}
          // 服务页：这里上报的是工具栏那一条（44–320），业务 iframe 自己铺剩余高度。
          record.isService=true;record.frame.classList.add('extension-service-toolbar');applyServiceFrame(record);report(record)
          if (!record.service || record.service.src !== url.href) {
            record.service?.remove();const service=document.createElement('iframe');service.title=`${record.title}管理界面`;service.className='extension-service-content';service.setAttribute('sandbox','allow-scripts allow-same-origin allow-forms allow-popups allow-downloads');service.setAttribute('referrerpolicy','no-referrer');service.src=url.href;record.outer.appendChild(service);record.service=service;applyServiceFrame(record);report(record)
          }
        }
        if(row.method==='gateway.stop'){record.service?.remove();record.service=null;record.frame.classList.remove('extension-service-toolbar');record.frame.style.height='100%';record.isService=false;report(record)}
        response={result}
      }
      catch (error) { response={error:String(error?.message||error)} }
      finally { record.pending.delete(row.id) }
      if ([...frames.values()].includes(record)) record.frame.contentWindow.postMessage({type:'fish:result',ticket:record.ticket,id:row.id,...response},'*')
    }
    const themeObserver = hostWindow.MutationObserver ? new hostWindow.MutationObserver(() => { for (const record of frames.values()) if (record.ready) theme(record) }) : null
    themeObserver?.observe(document.body, { attributes:true, attributeFilter:['class'] })
    hostWindow.addEventListener('message',receive)
    return { enter,park,sync,dispose(){themeObserver?.disconnect();hostWindow.removeEventListener('message',receive);clearDiscard();for(const key of frames.keys())retire(key);parked.remove()} }
  }
  window.ExtensionPage = { accepts, create }
})()
