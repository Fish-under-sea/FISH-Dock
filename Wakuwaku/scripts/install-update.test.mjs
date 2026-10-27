import assert from 'node:assert/strict'
import {test} from 'node:test'
import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import {createHash} from 'node:crypto'
import {createRequire} from 'node:module'
const require=createRequire(import.meta.url)
const load=()=>require('./install-update.cjs')
const sha=b=>createHash('sha256').update(b).digest('hex')
function fixture(){const base=fs.mkdtempSync(path.join(os.tmpdir(),'fish-update-中文 ')),root=path.join(base,'安装 目录'),stage=path.join(base,'stage');fs.mkdirSync(root);fs.mkdirSync(stage);return {base,root,stage}}
function put(root,name,data){fs.mkdirSync(path.dirname(path.join(root,name)),{recursive:true});fs.writeFileSync(path.join(root,name),data)}
function release(stage,files,version='1.2.0'){for(const [p,b]of Object.entries(files))put(stage,p,b);const owned=Object.entries(files).map(([p,b])=>({path:p,bytes:Buffer.byteLength(b),sha256:sha(b)}));put(stage,'install-manifest.json',JSON.stringify({schema:1,product:'FISH Dock',version,files:owned}));}
test('覆盖更新仅清理受管旧文件，保留无归属文件与用户插件资料',()=>{const f=fixture();try{release(f.stage,{'DSH-FISH.exe':'new','resources/node/node.exe':'node24'});put(f.root,'DSH-FISH.exe','old');put(f.root,'resources/obsolete.js','oldjs');put(f.root,'resources/user.txt','mine');put(f.root,'data/plugins/config.json','secret');put(f.root,'install-manifest.json',JSON.stringify({schema:1,product:'FISH Dock',version:'1.1.0',files:[{path:'DSH-FISH.exe',sha256:sha('old'),bytes:3},{path:'resources/obsolete.js',sha256:sha('oldjs'),bytes:5}]}));load().update(f.stage,f.root);assert.equal(fs.readFileSync(path.join(f.root,'DSH-FISH.exe'),'utf8'),'new');assert.equal(fs.existsSync(path.join(f.root,'resources/obsolete.js')),false);assert.equal(fs.readFileSync(path.join(f.root,'resources/user.txt'),'utf8'),'mine');assert.equal(fs.readFileSync(path.join(f.root,'data/plugins/config.json'),'utf8'),'secret');assert.equal(JSON.parse(fs.readFileSync(path.join(f.root,'install-manifest.json'))).version,'1.2.0');load().update(f.stage,f.root)}finally{fs.rmSync(f.base,{recursive:true,force:true})}})
test('摘要错误或越界归属清单在更新前停止，不触碰旧版本',()=>{const f=fixture();try{release(f.stage,{'DSH-FISH.exe':'new'});put(f.root,'DSH-FISH.exe','old');put(f.stage,'DSH-FISH.exe','bad');assert.throws(()=>load().update(f.stage,f.root),/校验/);assert.equal(fs.readFileSync(path.join(f.root,'DSH-FISH.exe'),'utf8'),'old');release(f.stage,{'../escape':'x'});assert.throws(()=>load().update(f.stage,f.root),/路径/)}finally{fs.rmSync(f.base,{recursive:true,force:true})}})
test('更新中途文件替换失败会恢复旧文件和归属清单',()=>{const f=fixture();try{release(f.stage,{'DSH-FISH.exe':'new','resources/new.txt':'new'});put(f.root,'DSH-FISH.exe','old');const io={...fs,renameSync(from,to){if(to===path.join(f.root,'resources/new.txt'))throw Error('模拟文件占用');return fs.renameSync(from,to)}};assert.throws(()=>load().update(f.stage,f.root,io),/模拟文件占用/);assert.equal(fs.readFileSync(path.join(f.root,'DSH-FISH.exe'),'utf8'),'old');assert.equal(fs.existsSync(path.join(f.root,'resources/new.txt')),false);assert.equal(fs.existsSync(path.join(f.root,'install-manifest.json')),false)}finally{fs.rmSync(f.base,{recursive:true,force:true})}})
