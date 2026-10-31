// 从锁定摘要的官方离线发行包准备内置引擎；不运行安装器。
import fs from 'node:fs/promises'
import path from 'node:path'
import {spawnSync} from 'node:child_process'
import {fileURLToPath} from 'node:url'
import {createReadStream} from 'node:fs'
import {createHash} from 'node:crypto'
async function hashFile(file){const hash=createHash('sha256');for await(const chunk of createReadStream(file))hash.update(chunk);return hash.digest('hex')}
export const WINDHAWK_REQUIRED=['windhawk.exe','windhawk-x64-helper.exe','Engine/32/windhawk.dll','Engine/32/windhawk.lib','Engine/64/windhawk.dll','Engine/64/windhawk.lib','Compiler/bin/clang++.exe','Compiler/include/windhawk_api.h','LICENSE-Windhawk.txt','Compiler/LICENSE.TXT']
export async function stageWindhawk(archive,output,{source,sevenZip='7z.exe'}={}){
 const lock=source??JSON.parse(await fs.readFile(new URL('./runtime-sources.json',import.meta.url),'utf8')).windhawk
 if(!lock||await hashFile(archive)!==lock.sha256)throw Error('Windhawk 官方离线发行包 SHA-256 不符')
 output=path.resolve(output);await fs.mkdir(path.dirname(output),{recursive:true});await fs.mkdir(output)
 const unpack=await fs.mkdtemp(path.join(path.dirname(output),'.windhawk-unpack-'))
 const result=spawnSync(sevenZip,['x',path.resolve(archive),`-o${unpack}`,'-y','-bso0','-bsp0','Compiler/*','Engine/*','windhawk.exe','windhawk-x64-helper.exe','UI/resources/app/extensions/windhawk/LICENSE.txt'],{stdio:'inherit',windowsHide:true})
 if(result.error)throw result.error
 if(result.status!==0)throw Error('官方 Windhawk 发行包解压失败')
 async function move(relative,target){const from=path.resolve(unpack,relative),to=path.resolve(output,target);if(!from.startsWith(unpack+path.sep)||!to.startsWith(output+path.sep))throw Error('解包路径越界');await fs.rename(from,to)}
 await move('Compiler','Compiler')
 const engines=await fs.readdir(path.join(unpack,'Engine'),{withFileTypes:true})
 if(engines.length!==1||!engines[0].isDirectory())throw Error('官方引擎版本布局与锁定版本不符')
 await move(path.join('Engine',engines[0].name),'Engine')
 for(const name of ['windhawk.exe','windhawk-x64-helper.exe'])await move(name,name)
 await move('UI/resources/app/extensions/windhawk/LICENSE.txt','LICENSE-Windhawk.txt')
 await fs.writeFile(path.join(output,'SOURCE.json'),JSON.stringify({version:lock.version,url:lock.url,sha256:lock.sha256,source:lock.source,notes:'Official unmodified engine and LLVM-MinGW compiler. UI is provided by FISH Dock. User data is stored separately.'},null,2)+'\n')
 for(const name of WINDHAWK_REQUIRED)if(!(await fs.stat(path.join(output,name))).isFile())throw Error(`内置引擎缺少文件：${name}`)
 return {version:lock.version,output}
}
if(process.argv[1]&&path.resolve(process.argv[1])===fileURLToPath(import.meta.url)){
 const args=process.argv.slice(2),get=name=>args[args.indexOf(name)+1]
 if(!args.includes('--archive')||!args.includes('--out'))throw Error('Usage: node scripts/stage-windhawk.mjs --archive <official-offline.exe> --out <new-resource-directory> [--sevenzip <7z.exe>]')
 console.log(JSON.stringify(await stageWindhawk(get('--archive'),get('--out'),{sevenZip:args.includes('--sevenzip')?get('--sevenzip'):undefined})))
}
