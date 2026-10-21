// WorkBuddy 离线保护：只调用内存 fetch 替身，不读取真实凭据或连接网络。
import assert from 'node:assert/strict'
import fs from 'node:fs'
import {registerHooks,createRequire} from 'node:module'
import {fileURLToPath,pathToFileURL} from 'node:url'
const require=createRequire(import.meta.url),ts=(await import(pathToFileURL(require.resolve('typescript')).href)).default
const root=new URL('../vendor/channel-pack/src/',import.meta.url).href,kernel=new URL('./lib/channel-pack-kernel.mjs',import.meta.url).href
const llm='data:text/javascript,'+encodeURIComponent(`export * from ${JSON.stringify(kernel)};export class LlmError extends Error{constructor(message,code,options){super(message,options);this.code=code}}`)
const hooks=registerHooks({resolve(s,c,n){if(['undici','jose'].includes(s))return{url:pathToFileURL(require.resolve(s)).href,shortCircuit:true};if(s==='@deepseek-ai/dsh-llm')return{url:llm,shortCircuit:true};if(s.startsWith('@deepseek-ai/'))return{url:kernel,shortCircuit:true};if(c.parentURL?.startsWith(root)&&s.endsWith('.js')){const u=new URL(s.replace(/\.js$/,'.ts'),c.parentURL).href;if(fs.existsSync(fileURLToPath(u)))return{url:u,shortCircuit:true}}return n(s,c)},load(u,c,n){if(u.startsWith(root)&&u.endsWith('.ts'))return{format:'module',shortCircuit:true,source:ts.transpileModule(fs.readFileSync(fileURLToPath(u),'utf8'),{compilerOptions:{module:ts.ModuleKind.ESNext,target:ts.ScriptTarget.ES2022},fileName:fileURLToPath(u)}).outputText};return n(u,c)}})
const {BuddyAdapter}=await import('../vendor/channel-pack/src/buddy-adapter.ts')
const {WORKBUDDY,CODEBUDDY}=await import('../vendor/channel-pack/src/product.ts')
const options={model:'deepseek-v4.1-flash',system:'User instructions. Powered by DeepSeek Harness\nworking in DEEPSEEK HARNESS: keep this user supplied line.',messages:[{role:'user',content:[{type:'text',text:'PRIVATE_FIXTURE_PROMPT'}]}]}
const credential={access_token:'FIXTURE_ACCESS',refresh_token:'FIXTURE_REFRESH',expires_at:Date.now()+3600000}
const collect=async adapter=>{for await(const _ of adapter.stream(options))void _}
try{
 for(const [status,body]of [[403,{code:4003,message:'PRIVATE_FIXTURE_PROMPT'}],[400,{code:11140,message:'PRIVATE_FIXTURE_PROMPT'}],[429,{code:'rate_limit',message:'PRIVATE_FIXTURE_PROMPT'}]]){
  let calls=0,refreshes=0;const seen=[]
  const adapter=new BuddyAdapter({product:WORKBUDDY,credentialRef:'fixture',resolveCredential:async()=>credential,refresh:async()=>{refreshes++},fetchImpl:async(_url,init)=>{calls++;seen.push(JSON.parse(init.body));return new Response(JSON.stringify(body),{status,headers:{'Content-Type':'application/json'}})}})
  await assert.rejects(collect(adapter),error=>!String(error.message).includes('PRIVATE_FIXTURE_PROMPT'))
  assert.equal(calls,1,'拒绝或风控只允许一次请求');assert.equal(refreshes,0,'拒绝后不得续期并再次发送')
  assert.equal(seen[0].messages[0].content,options.system,'不得删改来源不明的用户 system，也不得冒充官方身份')
 }
 let refreshes=0,calls=0
 const ordinary=new BuddyAdapter({product:CODEBUDDY,credentialRef:'fixture',resolveCredential:async()=>credential,refresh:async()=>{refreshes++},fetchImpl:async()=>{calls++;return new Response('{}',{status:403})}})
 await assert.rejects(collect(ordinary));assert.equal(refreshes,1);assert.equal(calls,2,'非 WorkBuddy 的既有认证流程不变')
 console.log('WorkBuddy offline policy: 4 checks passed; one attempt, no prompt rewrite, no prompt in errors, CodeBuddy unchanged')
}finally{hooks.deregister()}
