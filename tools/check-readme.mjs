import fs from 'node:fs'
import path from 'node:path'
import crypto from 'node:crypto'
import assert from 'node:assert/strict'
import { fileURLToPath } from 'node:url'
const root=path.resolve(path.dirname(fileURLToPath(import.meta.url)),'..')
const read=p=>fs.readFileSync(path.join(root,p),'utf8')
const hash=p=>crypto.createHash('sha256').update(fs.readFileSync(path.join(root,p))).digest('hex')
const rewrite=text=>text.replaceAll('src="src-tauri/','src="Wakuwaku/src-tauri/').replaceAll('href="docs/','href="Wakuwaku/docs/').replaceAll('](docs/','](Wakuwaku/docs/').replaceAll('](scripts/','](Wakuwaku/scripts/').replaceAll('](LICENSE)','](Wakuwaku/LICENSE)').replaceAll('](../dsh-our-free-model/','](dsh-our-free-model/')
for(const file of ['README.md','README.en.md']){
  const text=read(file), baseline=rewrite(read(`Wakuwaku/${file}`))
  assert.equal(text.slice(text.indexOf('\n> **')),baseline.slice(baseline.indexOf('\n> **')),'正文或上游署名发生意外变化')
  const headings=new Set([...text.matchAll(/^#{1,6}\s+(.+)$/gm)].map(m=>m[1].toLowerCase().replace(/[^\p{L}\p{N}\s_-]/gu,'').replace(/\s/g,'-')))
  for(const m of text.matchAll(/(?:\]\(|(?:src|srcset|href)=")([^\s")]+)/g)){
    const target=m[1]
    if(/^(https?:|mailto:|data:)/.test(target))continue
    if(target.startsWith('#')){assert.ok(headings.has(target.slice(1)),`失效锚点 ${file}: ${target}`);continue}
    assert.ok(fs.existsSync(path.join(root,target.split('#')[0])),`缺失引用 ${file}: ${target}`)
  }
}
assert.equal(hash('assets/readme/mascot.png'),'8e2bdd1c5b5ddb851db88b8ed29f9660f0ffe18f73266a89ba8cc03f9360952e')
assert.equal(hash('Wakuwaku/src-tauri/icons/dsh-fish.png'),'5688f69de350b8c75e0832e7b5928c680a1b7faa14ecd0259306afbd5c4178b9')
const metadata=JSON.parse(read('assets/readme/metadata.json'))
assert.equal(metadata.assets.length,40)
for(const row of metadata.assets){assert.equal(hash(row.path),row.contentHash);assert.equal(fs.statSync(path.join(root,row.path)).size,row.size)}
console.log(JSON.stringify({languages:2,bodyPreserved:true,localReferences:true,anchors:true,originalIcon:true,originalMascot:true,verifiedImages:metadata.assets.length}))
