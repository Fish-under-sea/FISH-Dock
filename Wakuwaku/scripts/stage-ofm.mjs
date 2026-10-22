// 把 dsh-our-free-model 的独立服务运行文件拷进 DSH-FISH 的资源目录。
// 清单与上游 `dsh-our-free-model/scripts/prepare-desktop.mjs` 的 stageDesktopService 保持一致，
// 区别只在于：这里只整理 OFM，不带 node.exe；完整的安装资源由 prepare-installer.mjs 组合。
//
// 用法：
//   node scripts/stage-ofm.mjs                      # 拷到 .runtime/ofm（开发期）
//   node scripts/stage-ofm.mjs --out src-tauri/resources/ofm   # 打包前拷到 bundle 资源
//   node scripts/stage-ofm.mjs --source <path>     # 指定 OFM 仓库位置（默认 ../dsh-our-free-model）
import fs from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const scriptDirectory = path.dirname(fileURLToPath(import.meta.url))
const appRoot = path.resolve(scriptDirectory, '..')
const productRoot = path.resolve(appRoot, '..')

const args = process.argv.slice(2)
const option = name => {
  const index = args.indexOf(`--${name}`)
  return index === -1 ? undefined : args[index + 1]
}

const sourceRoot = path.resolve(option('source') ?? path.join(productRoot, 'dsh-our-free-model'))
const destination = path.resolve(option('out') ?? path.join(appRoot, '.runtime', 'ofm'))

if (!fs.existsSync(path.join(sourceRoot, 'packages/standalone/service.mjs'))) {
  console.error(`没有找到 OFM 仓库：${sourceRoot}\n用 --source 指定 dsh-our-free-model 的路径。`)
  process.exit(1)
}

let files = 0
let bytes = 0
fs.rmSync(destination, { recursive: true, force: true })
fs.mkdirSync(destination, { recursive: true })

const copy = (relative, target = relative) => {
  const source = path.join(sourceRoot, relative)
  if (!fs.lstatSync(source).isFile()) throw new Error(`不是普通资源文件：${relative}`)
  const output = path.join(destination, target)
  fs.mkdirSync(path.dirname(output), { recursive: true })
  fs.copyFileSync(source, output)
  files += 1
  bytes += fs.statSync(output).size
}

const tree = (relative, allowed) => {
  for (const entry of fs.readdirSync(path.join(sourceRoot, relative), { withFileTypes: true })) {
    const child = `${relative}/${entry.name}`
    if (entry.isDirectory()) tree(child, allowed)
    else if (entry.isFile() && allowed(child)) copy(child)
  }
}

tree('src', file => /\.(js|json)$/.test(file))
for (const name of ['cli', 'service', 'management', 'login-terminal', 'open-external', 'eac', 'backup']) {
  copy(`packages/standalone/${name}.mjs`)
}
copy('packages/standalone/backup-readme.txt')
tree('packages/standalone/vendor', file => file.endsWith('.mjs') || path.basename(file) === 'LICENSE')
tree('packages/standalone/channels', file => file.endsWith('.mjs'))
tree('packages/standalone/web', file => /\.(html|js|css|json)$/.test(file))
copy('packages/standalone/package.json')
copy('packages/desktop/service-host.mjs')
for (const name of ['qoder-auth-wasm.wasm', 'LICENSE', 'NOTICE.md']) {
  copy(`vendor/channel-pack/${name}`)
}
copy('LICENSE')
copy('scripts/standalone-third-party.txt', 'THIRD_PARTY_NOTICES.txt')
// 上游最后会用私有包的 package.json 覆盖：独立服务只依赖 Node 内置模块。
fs.writeFileSync(path.join(destination, 'package.json'), `${JSON.stringify({ private: true, type: 'module' })}\n`)

console.log(`staged ${files} files (${(bytes / 1024 / 1024).toFixed(1)} MB)`)
console.log(`  from ${sourceRoot}`)
console.log(`  to   ${destination}`)
