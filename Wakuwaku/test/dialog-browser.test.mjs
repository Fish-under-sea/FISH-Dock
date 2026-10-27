import test from 'node:test'
import assert from 'node:assert/strict'
import { createServer } from 'node:http'
import { readFile, writeFile, mkdir, mkdtemp, access } from 'node:fs/promises'
import { spawn } from 'node:child_process'
import path from 'node:path'
import { fileURLToPath } from 'node:url'
const root=fileURLToPath(new URL('../..',import.meta.url))
const browser=process.env.FISH_DOCK_TEST_BROWSER||'C:/Program Files/Google/Chrome/Application/chrome.exe'
async function eventually(job){const end=Date.now()+15000;let error;while(Date.now()<end){try{const value=await job();if(value)return value}catch(e){error=e}await new Promise(r=>setTimeout(r,80))}throw error||Error('浏览器等待超时')}
async function connect(url){const socket=new WebSocket(url),pending=new Map();let seq=0;await new Promise((resolve,reject)=>{socket.addEventListener('open',resolve,{once:true});socket.addEventListener('error',reject,{once:true})});socket.addEventListener('message',e=>{const row=JSON.parse(e.data),p=pending.get(row.id);if(p){pending.delete(row.id);row.error?p.reject(Error(JSON.stringify(row.error))):p.resolve(row.result)}});return{socket,call:(method,params={})=>new Promise((resolve,reject)=>{const id=++seq;pending.set(id,{resolve,reject});socket.send(JSON.stringify({id,method,params}))})}}
test('真实浏览器校验 Dock 弹窗尺寸、透明遮罩、长文本滚动和 Escape 焦点恢复',{timeout:60000},async t=>{
 try{await access(browser)}catch{t.skip('设置 FISH_DOCK_TEST_BROWSER');return}
 const src=await readFile(path.join(root,'Wakuwaku/src/pet/settings.js'),'utf8'),css=await readFile(path.join(root,'Wakuwaku/src/pet/settings.css'),'utf8')
 const markup=src.match(/<dialog class="s-confirm-dialog"[\s\S]*?<\/dialog>/)[0]
 const behavior=src.slice(src.indexOf("  const confirmDialog ="),src.indexOf("  const body = layer.querySelector('.s-body')"))
 const html=`<!doctype html><meta charset="utf-8"><link rel="stylesheet" href="/settings.css"><style>html,body{margin:0;background:transparent}#island{position:relative;width:100vw;height:100vh;background:#202124;border-radius:16px}#island-settings{width:100%;height:100%;opacity:1;pointer-events:auto}.pbtn{font:13px sans-serif;min-height:32px}</style><main id="island" data-view="settings"><div id="island-settings"><button id="origin">操作入口</button>${markup}</div></main><script>
 const layer=document.getElementById('island-settings');let isOpen=true,lang='zh',confirmActive=null;const confirmQueue=[];
 let snap={},openRevision=1;function draw(){};const extensionPanel={auth:async()=>{},load:async()=>{}};
 window.pet={settings:{get:async()=>({})}};window.__TAURI__={core:{invoke:async()=>({codeCopied:true})}};
 ${behavior}
 window.fixture={confirmInDock,showDeviceDialog};window.answer=null;
 </script>`
 const server=createServer((req,res)=>{res.setHeader('Content-Type',req.url==='/settings.css'?'text/css':'text/html');res.end(req.url==='/settings.css'?css:html)});await new Promise(r=>server.listen(0,'127.0.0.1',r))
 const results=path.join(root,'.test-results');await mkdir(results,{recursive:true});const profile=await mkdtemp(path.join(results,'dialog-profile-'))
 const child=spawn(browser,['--headless=new','--disable-gpu','--no-first-run','--no-default-browser-check','--remote-debugging-port=0',`--user-data-dir=${profile}`,'about:blank'],{stdio:'ignore',windowsHide:true})
 let control,page
 try{
  const port=await eventually(async()=>Number((await readFile(path.join(profile,'DevToolsActivePort'),'utf8')).split('\n')[0]))
  control=await connect((await(await fetch(`http://127.0.0.1:${port}/json/version`)).json()).webSocketDebuggerUrl)
  page=await connect((await(await fetch(`http://127.0.0.1:${port}/json/list`)).json()).find(p=>p.type==='page').webSocketDebuggerUrl)
  const evaluate=async expression=>{const got=await page.call('Runtime.evaluate',{expression,awaitPromise:true,returnByValue:true});if(got.exceptionDetails)throw Error(JSON.stringify(got.exceptionDetails));return got.result.value}
  await page.call('Page.navigate',{url:`http://127.0.0.1:${server.address().port}/`});await eventually(()=>evaluate('!!window.fixture'))
  for(const theme of ['dark','light'])for(const [width,height]of [[320,480],[520,600],[1120,760]]){
   await page.call('Emulation.setDeviceMetricsOverride',{width,height,deviceScaleFactor:1,mobile:false})
   await evaluate(`document.body.className=${JSON.stringify(theme==='light'?'tb-light':'')};document.getElementById('origin').focus();fixture.confirmInDock('确认安装插件？\\n'+ '较长说明，用于验证窗口内部滚动。'.repeat(35)).then(value=>window.answer=value); true`)
   const geometry=await evaluate(`(()=>{const d=document.querySelector('dialog'),r=d.getBoundingClientRect(),s=getComputedStyle(d);return{x:r.x,y:r.y,width:r.width,height:r.height,shadow:s.boxShadow,backdrop:getComputedStyle(d,'::backdrop').backgroundColor,scroll:d.scrollHeight>d.clientHeight}})()`)
   assert.ok(geometry.width<=360 && geometry.width<=width-39)
   assert.ok(geometry.height<=height*.7+1)
   assert.ok(geometry.x>=0 && geometry.y>=0)
   assert.equal(geometry.shadow,'none');assert.equal(geometry.backdrop,'rgba(0, 0, 0, 0)');assert.equal(geometry.scroll,true)
   await page.call('Input.dispatchKeyEvent',{type:'keyDown',key:'Escape',code:'Escape',windowsVirtualKeyCode:27});await page.call('Input.dispatchKeyEvent',{type:'keyUp',key:'Escape',code:'Escape',windowsVirtualKeyCode:27})
   await eventually(()=>evaluate('!document.querySelector("dialog").open && document.activeElement.id==="origin"'))
  }
  await page.call('Emulation.setDeviceMetricsOverride',{width:520,height:600,deviceScaleFactor:1,mobile:false})
  await evaluate(`document.body.className='';fixture.showDeviceDialog({userCode:'ABCD-EFGH',copied:true})`)
  const screenshot=await page.call('Page.captureScreenshot',{format:'png'});await writeFile(path.join(results,'dialog-compact-dark.png'),Buffer.from(screenshot.data,'base64'))
  await evaluate(`document.body.className='tb-light'`)
  const light=await page.call('Page.captureScreenshot',{format:'png'});await writeFile(path.join(results,'dialog-compact-light.png'),Buffer.from(light.data,'base64'))
  assert.equal(await evaluate(`document.querySelector('[data-device-code]').textContent`),'ABCD-EFGH')
  await evaluate('fixture.showDeviceDialog(null)');assert.equal(await evaluate('document.querySelector("dialog").open'),false)
 }finally{page?.socket.close();if(control){await control.call('Browser.close').catch(()=>{});control.socket.close()}else child.kill();server.closeAllConnections();await new Promise(r=>server.close(r))}
})
