import assert from 'node:assert/strict'
import {test} from 'node:test'
import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import {createHash} from 'node:crypto'
import {createRequire} from 'node:module'
import {spawnSync} from 'node:child_process'
import {fileURLToPath} from 'node:url'
const require=createRequire(import.meta.url)
const load=()=>require('./install-update.cjs')
const sha=b=>createHash('sha256').update(b).digest('hex')
function fixture(){const base=fs.mkdtempSync(path.join(os.tmpdir(),'fish-update-中文 ')),root=path.join(base,'安装 目录'),stage=path.join(base,'stage');fs.mkdirSync(root);fs.mkdirSync(stage);return {base,root,stage}}
function put(root,name,data){fs.mkdirSync(path.dirname(path.join(root,name)),{recursive:true});fs.writeFileSync(path.join(root,name),data)}
function release(stage,files,version='1.2.0'){for(const [p,b]of Object.entries(files))put(stage,p,b);const owned=Object.entries(files).map(([p,b])=>({path:p,bytes:Buffer.byteLength(b),sha256:sha(b)}));put(stage,'install-manifest.json',JSON.stringify({schema:1,product:'FISH Dock',version,files:owned}));}
test('覆盖更新仅清理受管旧文件，保留无归属文件与用户插件资料',()=>{const f=fixture();try{release(f.stage,{'DSH-FISH.exe':'new','resources/node/node.exe':'node24'});put(f.root,'DSH-FISH.exe','old');put(f.root,'resources/obsolete.js','oldjs');put(f.root,'resources/user.txt','mine');put(f.root,'data/plugins/config.json','secret');put(f.root,'install-manifest.json',JSON.stringify({schema:1,product:'FISH Dock',version:'1.1.0',files:[{path:'DSH-FISH.exe',sha256:sha('old'),bytes:3},{path:'resources/obsolete.js',sha256:sha('oldjs'),bytes:5}]}));load().update(f.stage,f.root);assert.equal(fs.readFileSync(path.join(f.root,'DSH-FISH.exe'),'utf8'),'new');assert.equal(fs.existsSync(path.join(f.root,'resources/obsolete.js')),false);assert.equal(fs.readFileSync(path.join(f.root,'resources/user.txt'),'utf8'),'mine');assert.equal(fs.readFileSync(path.join(f.root,'data/plugins/config.json'),'utf8'),'secret');assert.equal(JSON.parse(fs.readFileSync(path.join(f.root,'install-manifest.json'))).version,'1.2.0');load().update(f.stage,f.root)}finally{fs.rmSync(f.base,{recursive:true,force:true})}})
test('摘要错误或越界归属清单在更新前停止，不触碰旧版本',()=>{const f=fixture();try{release(f.stage,{'DSH-FISH.exe':'new'});put(f.root,'DSH-FISH.exe','old');put(f.stage,'DSH-FISH.exe','bad');assert.throws(()=>load().update(f.stage,f.root),/校验/);assert.equal(fs.readFileSync(path.join(f.root,'DSH-FISH.exe'),'utf8'),'old');release(f.stage,{'../escape':'x'});assert.throws(()=>load().update(f.stage,f.root),/路径/)}finally{fs.rmSync(f.base,{recursive:true,force:true})}})
test('更新中途文件替换失败会恢复旧文件和归属清单',()=>{const f=fixture();try{release(f.stage,{'DSH-FISH.exe':'new','resources/new.txt':'new'});put(f.root,'DSH-FISH.exe','old');const io={...fs,renameSync(from,to){if(to===path.join(f.root,'resources/new.txt'))throw Error('模拟文件占用');return fs.renameSync(from,to)}};assert.throws(()=>load().update(f.stage,f.root,io),/模拟文件占用/);assert.equal(fs.readFileSync(path.join(f.root,'DSH-FISH.exe'),'utf8'),'old');assert.equal(fs.existsSync(path.join(f.root,'resources/new.txt')),false);assert.equal(fs.existsSync(path.join(f.root,'install-manifest.json')),false)}finally{fs.rmSync(f.base,{recursive:true,force:true})}})

// 安装日志乱码（#16）：安装器用 nsExec::ExecToLog 转发这个脚本的输出，nsExec 按
// ANSI/OEM 代码页解释字节，Node 写的是 UTF-8 —— 中文就成了乱码。所以走安装器
// 那一屏的输出只用 ASCII；细节另写 install.log（UTF-8 带 BOM）。这里把两者的
// 字节都取出来看：进程的输出重定向到文件（沙箱里管道不可用），像安装器那样。
test('命令行输出走安装器日志时是纯 ASCII，中文细节写进带 BOM 的 install.log', () => {
  const f = fixture()
  const out = path.join(f.base, 'stdio.txt'), err = path.join(f.base, 'stderr.txt')
  try {
    release(f.stage, { 'DSH-FISH.exe': 'new', 'resources/runtime.bin': 'runtime' })
    const script = path.join(path.dirname(fileURLToPath(import.meta.url)), 'install-update.cjs')
    const ofd = fs.openSync(out, 'w'), efd = fs.openSync(err, 'w')
    let status
    try {
      status = spawnSync(process.execPath, [script, f.stage, f.root], { stdio: ['ignore', ofd, efd], windowsHide: true, timeout: 60000 }).status
    } finally { fs.closeSync(ofd); fs.closeSync(efd) }
    assert.equal(status, 0)
    const shown = fs.readFileSync(out)
    const text = shown.toString('utf8')
    assert.ok(text.includes('Installation complete'), `安装器那一屏该说结果：${text}`)
    for (const byte of shown) assert.ok(byte < 0x80, '安装器日志的字节只能是 ASCII（nsExec 按 OEM 代码页解释）')
    assert.equal(fs.readFileSync(err).length, 0, '成功安装不该往 stderr 写东西')
    assert.match(text.trimEnd(), /\{"version":"[^"]+","replaced":\d+.*\}$/, '最后一行仍是给安装器读的结果 JSON')
    const log = fs.readFileSync(path.join(f.root, 'install.log'))
    assert.deepEqual([...log.subarray(0, 3)], [0xef, 0xbb, 0xbf], 'install.log 要带 BOM，记事本才认得出 UTF-8')
    const detail = log.subarray(3).toString('utf8')
    assert.ok(detail.includes('安装完成'), `细节里该有用中文写的这一句：${detail}`)
    assert.match(detail, /\r\n/, '一句一行，记事本能正常换行')
  } finally { fs.rmSync(f.base, { recursive: true, force: true }) }
})

// 装不成的时候最会把中文漏出去：报错里带着安装目录的路径，而中文目录是常事
// （"安装 目录" 就是这个用例的根）。安装器那一屏只看得到问号，真正的原因在
// install.log 里原样留着。
test('装不成的报错也不把中文漏到安装器日志：路径换成问号，细节留在 install.log', () => {
  const f = fixture()
  const out = path.join(f.base, 'stdio.txt'), err = path.join(f.base, 'stderr.txt')
  try {
    // 暂存里没有归属清单：第一句就停。
    put(f.stage, 'DSH-FISH.exe', 'new')
    const script = path.join(path.dirname(fileURLToPath(import.meta.url)), 'install-update.cjs')
    const ofd = fs.openSync(out, 'w'), efd = fs.openSync(err, 'w')
    let status
    try {
      status = spawnSync(process.execPath, [script, f.stage, f.root], { stdio: ['ignore', ofd, efd], windowsHide: true, timeout: 60000 }).status
    } finally { fs.closeSync(ofd); fs.closeSync(efd) }
    assert.equal(status, 1, '装不成要给安装器一个失败码')
    const said = fs.readFileSync(err)
    for (const byte of said) assert.ok(byte < 0x80, 'stderr 也只能是 ASCII')
    assert.match(said.toString('utf8'), /^Installation update failed: /)
    for (const byte of fs.readFileSync(out)) assert.ok(byte < 0x80, 'stdout 也只能是 ASCII')
    const detail = fs.readFileSync(path.join(f.root, 'install.log')).subarray(3).toString('utf8')
    assert.match(detail, /安装更新失败：.*归属清单/, `install.log 里该有中文原因：${detail}`)
  } finally { fs.rmSync(f.base, { recursive: true, force: true }) }
})