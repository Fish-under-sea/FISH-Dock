// 隔离页面由宿主票据绑定插件，子页面只发送业务方法与参数。
;(function () {
  function identity(event, frame, ticket) { return event.source === frame.contentWindow && event.origin === 'null' && event.data?.ticket === ticket }
  function accepts(event, frame, ticket) {
    const row = event.data
    if (!identity(event,frame,ticket) || row.type !== 'fish:call' || !Number.isSafeInteger(row.id) || row.id < 1 || typeof row.method !== 'string' || !/^[a-zA-Z0-9_.-]{1,100}$/.test(row.method) || /^(host|plugin)\./.test(row.method)) return false
    try { return JSON.stringify(row).length <= 512 * 1024 } catch { return false }
  }
  function create({ invoke, lang, onLayout = () => {}, hostWindow = window, document = window.document }) {
    const frames = new Map(), parked = document.createElement('div')
    parked.hidden = true; document.body.appendChild(parked)
    let current = '', revision = 0
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
    function report(record) {
      if (frames.get(current) === record) onLayout(current, { height:record.height, layout:record.layout || 'single', service:!!record.service })
    }
    function toolbarHeight(record) {
      if (record.service) record.frame.style.height = `${Math.max(44, Math.min(320, record.height || 52))}px`
      report(record)
    }
    function retire(key) {
      const record = frames.get(key); if (!record) return
      clearTimeout(record.timer); (record.outer||record.frame).remove(); frames.delete(key)
      if (record.ticket) void invoke('extension_page_close',{ ticket:record.ticket }).catch(() => {})
    }
    function sync(pages) {
      for (const [key,record] of frames) if (!pages.some(p => p.key === key && p.digest === record.digest)) retire(key)
    }
    function park() {
      if (current && frames.has(current)) parked.appendChild(frames.get(current).outer||frames.get(current).frame)
      current = ''; revision++
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
      if (existing) { root.replaceChildren(existing.outer||existing.frame); theme(existing); report(existing); return }
      notice(root,text('正在打开插件页面…','Opening extension…'))
      let granted
      try {
        granted=await invoke('extension_page',{key:page.key})
        if (asked !== revision || current !== page.key) { void invoke('extension_page_close',{ticket:granted.ticket}).catch(()=>{}); return }
        const frame=document.createElement('iframe')
        frame.title=page.title; frame.className='extension-frame'; frame.setAttribute('sandbox','allow-scripts'); frame.setAttribute('referrerpolicy','no-referrer')
        const outer=document.createElement('div');outer.className='extension-service-shell';outer.appendChild(frame)
        const record={frame,outer,title:page.title,ticket:granted.ticket,digest:page.digest,pending:new Set(),ready:false,timer:null,height:52}
        frames.set(page.key,record)
        record.timer=setTimeout(()=>{
          if (record.ready) return
          retire(page.key)
          if (current===page.key) notice(root,text('页面加载失败，请重试或检查插件状态。','Page failed to load. Retry or check the extension.'),()=>{current='';void enter(root,page)})
        },8000)
        root.replaceChildren(outer); frame.src=granted.url
      } catch (error) {
        if (asked === revision && current === page.key) { current=''; notice(root,String(error?.message||error),()=>void enter(root,page)) }
      }
    }
    async function receive(event) {
      const record=[...frames.values()].find(record=>identity(event,record.frame,record.ticket))
      if (!record) return
      if (event.data?.type === 'fish:ready') { record.ready=true; clearTimeout(record.timer); theme(record); return }
      if (event.data?.type === 'fish:layout') {
        if (['single','double'].includes(event.data.layout)) record.layout=event.data.layout
        if (Number.isFinite(event.data.height) && event.data.height > 0 && event.data.height <= 10000) { record.height=event.data.height; toolbarHeight(record) }
        return
      }
      if (!accepts(event,record.frame,record.ticket) || record.pending.size>=16 || record.pending.has(event.data.id)) return
      const row=event.data;record.pending.add(row.id)
      let response
      try {
        const result=await invoke('extension_page_call',{ticket:record.ticket,method:row.method,params:row.params??{}})
        if(row.method==='gateway.open'){
          const url=new URL(result.url);if(url.protocol!=='http:'||url.hostname!=='127.0.0.1'||url.username||url.password)throw Error('服务地址无效')
          if(!record.outer){record.outer=document.createElement('div');record.outer.className='extension-service-shell';record.frame.replaceWith(record.outer);record.outer.appendChild(record.frame);record.frame.classList.add('extension-service-toolbar');toolbarHeight(record)}
          record.frame.classList.add('extension-service-toolbar'); toolbarHeight(record)
          if (!record.service || record.service.src !== url.href) {
            record.service?.remove();const service=document.createElement('iframe');service.title=`${record.title}管理界面`;service.className='extension-service-content';service.setAttribute('sandbox','allow-scripts allow-same-origin allow-forms allow-popups allow-downloads');service.setAttribute('referrerpolicy','no-referrer');service.src=url.href;record.outer.appendChild(service);record.service=service;toolbarHeight(record)
          }
        }
        if(row.method==='gateway.stop'){record.service?.remove();record.service=null;record.frame.classList.remove('extension-service-toolbar');record.frame.style.height='100%'; report(record)}
        response={result}
      }
      catch (error) { response={error:String(error?.message||error)} }
      finally { record.pending.delete(row.id) }
      if ([...frames.values()].includes(record)) record.frame.contentWindow.postMessage({type:'fish:result',ticket:record.ticket,id:row.id,...response},'*')
    }
    const themeObserver = hostWindow.MutationObserver ? new hostWindow.MutationObserver(() => { for (const record of frames.values()) if (record.ready) theme(record) }) : null
    themeObserver?.observe(document.body, { attributes:true, attributeFilter:['class'] })
    hostWindow.addEventListener('message',receive)
    return { enter,park,sync,dispose(){themeObserver?.disconnect();hostWindow.removeEventListener('message',receive);for(const key of frames.keys())retire(key);parked.remove()} }
  }
  window.ExtensionPage = { accepts, create }
})()
