import fs from 'node:fs'
import path from 'node:path'
import crypto from 'node:crypto'
import assert from 'node:assert/strict'
import { fileURLToPath } from 'node:url'
const root=path.resolve(path.dirname(fileURLToPath(import.meta.url)),'..')
const hash=file=>crypto.createHash('sha256').update(fs.readFileSync(path.join(root,file))).digest('hex')
const files=['README.md','README.en.md','Wakuwaku/README.md','Wakuwaku/README.en.md']
for(const file of files){
  const text=fs.readFileSync(path.join(root,file),'utf8')
  assert.match(text,/1\.1\.0/)
  assert.ok(!text.includes('\u001b'),'文档不含终端控制符')
  for(const match of text.matchAll(/(?:\]\(|(?:src|srcset|href)=")([^\s")]+)/g)){
    const link=match[1]
    if(/^(?:https?:|mailto:|data:|#)/.test(link))continue
    const [relative,fragment]=link.split('#')
    const dest=path.resolve(root,path.dirname(file),relative)
    assert.ok(dest.startsWith(root+path.sep),`引用越界 ${file}`)
    assert.ok(fs.existsSync(dest),`缺失引用 ${file}: ${link}`)
    if(fragment&&dest.endsWith('.md')){
      const content=fs.readFileSync(dest,'utf8')
      const anchors=[...content.matchAll(/^#{1,6}\s+(.+)$/gm)].map(m=>m[1].toLowerCase().replace(/[^\p{L}\p{N}\s_-]/gu,'').replace(/\s/g,'-'))
      assert.ok(anchors.includes(fragment),`失效锚点 ${file}: ${link}`)
    }
  }
}
assert.equal(hash('Wakuwaku/src-tauri/icons/DSH-FISH-Windows.ico'),'5130fe9d4e381de4692446aa08729ad2de59cca443f7d40367fbc dff41fbfed0'.replace(' ',''))
assert.equal(hash('Wakuwaku/src-tauri/icons/dsh-fish.png'),'5688f69de350b8c75e0832e7b5928c680a1b7faa14ecd0259306afbd5c4178b9')
for(const file of ['sdk/node/plugin.mjs','sdk/browser/bridge.js','licenses/Our-Free-Model.txt','Wakuwaku/src-tauri/src/plugin_install.rs','Wakuwaku/src-tauri/src/model_focus.rs'])assert.ok(fs.existsSync(path.join(root,file)),`缺少宿主源码 ${file}`)
assert.ok(!fs.existsSync(path.join(root,'dsh-our-free-model')),'主仓不再包含废弃 OFM 服务树')
console.log(JSON.stringify({documents:files.length,version:'1.1.0',independentPlugins:true,localReferences:true,anchors:true,originalIcon:true,fullSource:true}))
