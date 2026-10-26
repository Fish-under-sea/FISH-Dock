// 使用已安装 Chromium 验证真实 sandbox、CSP、页面消息和页面切换，不读取用户浏览器配置。
import test from 'node:test'
import assert from 'node:assert/strict'
import { createServer } from 'node:http'
import { readFile, mkdtemp, mkdir, access } from 'node:fs/promises'
import { spawn } from 'node:child_process'
import { fileURLToPath } from 'node:url'
import path from 'node:path'
const root = fileURLToPath(new URL('../..', import.meta.url))
const browser = process.env.FISH_DOCK_TEST_BROWSER || 'C:/Program Files/Google/Chrome/Application/chrome.exe'
const pause = ms => new Promise(resolve => setTimeout(resolve, ms))
async function eventually(job, timeout = 12000) {
  const end = Date.now() + timeout
  let last
  while (Date.now() < end) { try { const value = await job(); if (value) return value } catch (error) { last = error }; await pause(80) }
  throw last || new Error('等待浏览器行为超时')
}
async function connect(url) {
  const socket = new WebSocket(url), pending = new Map(); let sequence = 0
  await new Promise((resolve,reject) => { socket.addEventListener('open',resolve,{once:true});socket.addEventListener('error',reject,{once:true}) })
  socket.addEventListener('message', event => { const row=JSON.parse(event.data), waiter=pending.get(row.id); if (waiter) {pending.delete(row.id);row.error ? waiter.reject(new Error(JSON.stringify(row.error))) : waiter.resolve(row.result)} })
  return { socket, call(method,params={}) { return new Promise((resolve,reject)=> {const id=++sequence;pending.set(id,{resolve,reject});socket.send(JSON.stringify({id,method,params}))}) } }
}
test('真实浏览器中隔离页面可调用自身插件、保持页面并撤销旧版本', { timeout:45000 }, async t => {
  try { await access(browser) } catch { t.skip('设置 FISH_DOCK_TEST_BROWSER 指向已安装 Chromium');return }
  const bridge=await readFile(path.join(root,'sdk/browser/bridge.js'))
  const component=await readFile(path.join(root,'Wakuwaku/src/pet/extension-page.js'))
  const stylesheet=await readFile(path.join(root,'Wakuwaku/src/pet/extensions.css'))
  const ticket='a'.repeat(64), calls=[]
  const requests=[]
  const server=createServer((req,res)=> {
    requests.push(req.url)
    if(req.url==='/') {res.setHeader('Content-Type','text/html');res.end('<!doctype html><link rel="stylesheet" href="/extensions.css"><style>#page{height:600px}</style><body class="tb-light"><main id="page"></main><script src="/component.js"></script><script src="/host.js"></script>');return}
    if(req.url==='/extensions.css'){res.setHeader('Content-Type','text/css');res.end(stylesheet);return}
    if(req.url==='/component.js'){res.setHeader('Content-Type','text/javascript');res.end(component);return}
    if(req.url==='/host.js') {res.setHeader('Content-Type','text/javascript');res.end(`
      window.events=[];window.business=[];window.retiredTickets=[];
      window.addEventListener('message',event=>{if(event.data?.type==='evidence')events.push(event.data)})
      window.page={key:'acme.weather/main',title:'Weather',digest:'digest-1'}
      window.view=ExtensionPage.create({lang:()=> 'zh',invoke:async(command,args)=>{
        if(command==='extension_page')return {ticket:'${ticket}',url:'/${ticket}/ui/index.html'}
        if(command==='extension_page_close'){retiredTickets.push(args.ticket);return}
        if(command==='extension_page_call'){business.push(args);return {temperature:22}}
        throw new Error(command)
      }})
      void view.enter(document.getElementById('page'),page)
    `);return}
    res.setHeader('Content-Security-Policy',"default-src 'none'; script-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' data:; font-src 'self'; connect-src 'none'; frame-src 'none'; form-action 'none'; base-uri 'none'; object-src 'none'")
    res.setHeader('Content-Type',req.url.endsWith('.js')?'text/javascript':'text/html')
    if(req.url===`/${ticket}/_fish_bridge.js`){res.end(bridge);return}
    if(req.url===`/${ticket}/ui/index.html`){res.end(`<!doctype html><script src="/${ticket}/_fish_bridge.js"></script><html><body><p>Weather fixture</p><script src="app.js"></script></body></html>`);return}
    if(req.url===`/${ticket}/ui/app.js`){res.end(`
      ;(async()=>{
        let blocked=false;try{parent.document.body.dataset.stolen='yes'}catch{blocked=true}
        parent.postMessage({type:'fish:call',ticket:'wrong',id:99,method:'steal'},'*')
        const result=await FishDock.call('status',{})
        parent.postMessage({type:'evidence',blocked,result,tauri:typeof window.__TAURI__,theme:document.documentElement.dataset.dockTheme},'*')
      })()
    `);return}
    res.statusCode=404;res.end('missing')
  })
  await new Promise(resolve=>server.listen(0,'127.0.0.1',resolve))
  const base=path.join(root,'.test-results');await mkdir(base,{recursive:true});const profile=await mkdtemp(path.join(base,'extension-browser-'))
  const child=spawn(browser,['--headless=new','--disable-gpu','--no-first-run','--no-default-browser-check','--remote-debugging-port=0',`--user-data-dir=${profile}`,'about:blank'],{stdio:'ignore',windowsHide:true})
  let control, page
  try {
    const port=await eventually(async()=>Number((await readFile(path.join(profile,'DevToolsActivePort'),'utf8')).split('\n')[0]))
    const version=await(await fetch(`http://127.0.0.1:${port}/json/version`)).json();control=await connect(version.webSocketDebuggerUrl)
    const targets=await(await fetch(`http://127.0.0.1:${port}/json/list`)).json();page=await connect(targets.find(r=>r.type==='page').webSocketDebuggerUrl)
    const evaluate=async expression=>{const value=await page.call('Runtime.evaluate',{expression,awaitPromise:true,returnByValue:true});if(value.exceptionDetails)throw new Error(JSON.stringify(value.exceptionDetails));return value.result.value}
    await page.call('Page.navigate',{url:`http://127.0.0.1:${server.address().port}/`})
    const evidence=await eventually(async()=> (await evaluate('window.events'))?.[0])
    assert.equal(evidence.blocked,true);assert.equal(evidence.tauri,'undefined');assert.equal(evidence.result.temperature,22)
     assert.equal(evidence.theme,'light')
     assert.equal(await evaluate('Math.round(document.querySelector("iframe").getBoundingClientRect().height)'),600)
    const business=await evaluate('window.business');assert.equal(business.length,1);assert.equal(business[0].ticket,ticket);assert.equal(business[0].method,'status')
    await evaluate('window.originalFrame=document.querySelector("iframe");view.park();view.enter(document.getElementById("page"),page)')
    assert.equal(await evaluate('window.originalFrame===document.querySelector("iframe")'),true)
    await evaluate('view.sync([{...page,digest:"digest-2"}])')
    assert.equal(await evaluate('document.querySelectorAll("iframe").length'),0)
    assert.deepEqual(await evaluate('window.retiredTickets'),[ticket])
    assert.ok(requests.includes(`/${ticket}/ui/app.js`));assert.ok(requests.includes(`/${ticket}/_fish_bridge.js`))
  } finally {
    page?.socket.close()
    if(control){await control.call('Browser.close').catch(()=>{});control.socket.close()}
    else child.kill()
    server.closeAllConnections();await new Promise(resolve=>server.close(resolve))
  }
})
