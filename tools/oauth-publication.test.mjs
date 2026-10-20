import fs from 'node:fs'
import path from 'node:path'
import vm from 'node:vm'
import assert from 'node:assert/strict'
import { test } from 'node:test'
import { fileURLToPath } from 'node:url'
const root=path.resolve(path.dirname(fileURLToPath(import.meta.url)),'..')
const files=['dsh-our-free-model/packages/standalone/channels/business.mjs','dsh-our-free-model/vendor/channel-pack/pack.js','dsh-our-free-model/vendor/channel-pack/src/gemini-oauth.ts']
for(const relative of files)test(`公开源码不带默认 OAuth 凭据，并保留环境变量覆盖：${relative}`,()=>{
  const text=fs.readFileSync(path.join(root,relative),'utf8')
  const definitions=[...text.matchAll(/\b(?:const|var) (GEMINI_DEFAULT_CLIENT_(?:ID|SECRET))\s*=\s*("(?:\\.|[^"\\])*"|'(?:\\.|[^'\\])*')/g)]
  assert.equal(definitions.length,2)
  assert.ok(definitions.every(m=>m[2]==="''"||m[2]==='""'),'公开源码的 OAuth 默认值必须为空，禁止硬编码客户端凭据')
  const functions=['geminiClientId','geminiClientSecret'].map(name=>{
    const match=text.match(new RegExp(`function ${name}\\(\\)(?:: string)? \\{[\\s\\S]*?\\n\\}`))
    assert.ok(match,`缺少原有配置读取函数 ${name}`)
    return match[0].replace(/\(\): string/g,'()')
  }).join('\n')
  const run=env=>vm.runInNewContext(`const GEMINI_DEFAULT_CLIENT_ID='';const GEMINI_DEFAULT_CLIENT_SECRET='';${functions};({id:geminiClientId(),secret:geminiClientSecret()})`,{process:{env}})
  const empty=run({})
  assert.equal(empty.id,'');assert.equal(empty.secret,'')
  const configured=run({CMDC_PAK_GOOGLE_CLIENT_ID:' TEST_ONLY_CLIENT_ID ',CMDC_PAK_GOOGLE_CLIENT_SECRET:' TEST_ONLY_CLIENT_SECRET '})
  assert.equal(configured.id,'TEST_ONLY_CLIENT_ID');assert.equal(configured.secret,'TEST_ONLY_CLIENT_SECRET')
})
