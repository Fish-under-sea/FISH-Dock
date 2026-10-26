// 构建期准备安装资源：从已锁定的官方下载件解出 Node、固定版 WebView2 与 Windhawk，
// 目录结构与安装后一致，并写一份清单（schema 2）供程序首次启动全量校验、日常启动只查必需文件。
// 安装包里只装这些文件；安装器不做压缩解包的二次搬运。
import fs from 'node:fs/promises'
import { createReadStream } from 'node:fs'
import { createHash } from 'node:crypto'
import { spawnSync } from 'node:child_process'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

export const MANIFEST_SCHEMA = 2
export const PRODUCT = 'DSH-FISH'
export const ARCHITECTURE = 'x86_64'
/// 程序启动必需的文件；与 src-tauri/src/runtime.rs 的 REQUIRED 保持一致。
export const REQUIRED = [
  'node/node.exe',
  'node/LICENSE',
  'webview2/msedgewebview2.exe',
  'licenses/Wakuwaku.txt',
  'windhawk/windhawk.exe',
  'windhawk/windhawk-x64-helper.exe',
  'windhawk/Engine/32/windhawk.dll',
  'windhawk/Engine/32/windhawk.lib',
  'windhawk/Engine/64/windhawk.dll',
  'windhawk/Engine/64/windhawk.lib',
  'windhawk/Compiler/bin/clang++.exe',
  'windhawk/Compiler/include/windhawk_api.h',
  'windhawk/LICENSE-Windhawk.txt',
  'windhawk/Compiler/LICENSE.TXT',
]

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')

export async function hashFile(file) {
  const hash = createHash('sha256')
  for await (const chunk of createReadStream(file)) hash.update(chunk)
  return hash.digest('hex')
}

export async function inventory(directory) {
  const files = []
  async function walk(relative) {
    for (const entry of await fs.readdir(path.join(directory, relative), { withFileTypes: true })) {
      const name = relative ? `${relative}/${entry.name}` : entry.name
      if (entry.isDirectory()) await walk(name)
      else if (entry.isFile()) {
        const file = path.join(directory, name)
        files.push({ path: name, bytes: (await fs.stat(file)).size, sha256: await hashFile(file) })
      } else throw new Error(`Resource is not a regular file: ${name}`)
    }
  }
  await walk('')
  return files.sort((a, b) => a.path < b.path ? -1 : a.path > b.path ? 1 : 0)
}

export async function productVersion(project = root) {
  const config = JSON.parse(await fs.readFile(path.join(project, 'src-tauri/tauri.conf.json'), 'utf8'))
  if (!config.version) throw new Error('src-tauri/tauri.conf.json has no version')
  return config.version
}

/// 清单写进资源目录（安装器一次放好），但清单本身不属于被校验的文件：
/// 它的 SHA-256 就是“这套资源已验证过”的钥匙。
export async function writeManifest(directory, { version, nodeVersion, webview2Version, architecture = ARCHITECTURE, product = PRODUCT }) {
  if (!version) throw new Error('manifest needs the product version')
  const files = (await inventory(directory)).filter(file => file.path !== 'manifest.json')
  const manifest = { schema: MANIFEST_SCHEMA, product, version, architecture, node_version: nodeVersion, webview2_version: webview2Version, files }
  const missing = REQUIRED.filter(name => !files.some(file => file.path === name))
  if (missing.length) throw new Error(`resources are missing required files: ${missing.join(', ')}`)
  await fs.writeFile(path.join(directory, 'manifest.json'), `${JSON.stringify(manifest, null, 2)}\n`, { flag: 'wx' })
  return manifest
}

function run(command, args, quiet = false) {
  const result = spawnSync(command, args, { stdio: quiet ? 'ignore' : 'inherit', windowsHide: true })
  if (result.error) throw result.error
  if (result.status !== 0) throw new Error(`${command} failed with ${result.status}`)
}

/// 从官方下载件准备 `output/resources`：解包 Node 与固定版 WebView2，整理 Windhawk 工具链，
/// 放许可原文，最后写清单。每一步都先核对摘要，不信任之前的解压工作目录。
export async function prepareResources(downloads, output, options = {}) {
  if (process.platform !== 'win32') throw new Error('This resource builder requires Windows x64 tools')
  const lock = options.lock ?? JSON.parse(await fs.readFile(path.join(root, 'scripts/runtime-sources.json'), 'utf8'))
  for (const item of [lock.node, lock.webview2, lock.webview2_license]) {
    const file = path.join(downloads, item.filename)
    if (await hashFile(file) !== item.sha256) throw new Error(`Official source SHA-256 mismatch: ${item.filename}`)
  }
  const version = options.version ?? await productVersion()
  await fs.mkdir(path.dirname(output), { recursive: true })
  await fs.mkdir(output)
  const resources = path.join(output, 'resources')
  await fs.mkdir(resources)
  const unpackNode = path.join(output, 'node-unpack')
  const unpackWebview = path.join(output, 'webview-unpack')
  await fs.mkdir(unpackNode); await fs.mkdir(unpackWebview)
  run('tar.exe', ['-xf', path.join(downloads, lock.node.filename), '-C', unpackNode], true)
  run('expand.exe', ['-F:*', path.join(downloads, lock.webview2.filename), unpackWebview], true)
  await fs.rename(path.join(unpackNode, lock.node.directory), path.join(resources, 'node'))
  await fs.rename(path.join(unpackWebview, lock.webview2.directory), path.join(resources, 'webview2'))
  await fs.mkdir(path.join(resources, 'licenses'))
  await fs.copyFile(path.join(root, 'LICENSE'), path.join(resources, 'licenses/Wakuwaku.txt'))
  await fs.copyFile(path.join(downloads, lock.webview2_license.filename), path.join(resources, 'licenses/WebView2-Fixed-Version.html'))
  await fs.writeFile(path.join(resources, 'licenses/runtime-sources.json'), `${JSON.stringify(lock, null, 2)}\n`)
  await fs.writeFile(path.join(resources, 'licenses/NOTICE.txt'), 'FISH Dock includes Microsoft Edge WebView2 Runtime (Fixed Version).\nMicrosoft Defender SmartScreen is enabled by default and may collect and send information to Microsoft.\nPrivacy: https://aka.ms/privacy\nSmartScreen: https://learn.microsoft.com/en-us/microsoft-edge/privacy-whitepaper#smartscreen\nTerms: WebView2-Fixed-Version.html\nNode.js and its third-party terms: ../node/LICENSE\nWakuwaku terms: Wakuwaku.txt\n')
  // 解包目录已用完：安装资源就是 resources/，安装器不再压缩打包。
  await fs.rm(unpackNode, { recursive: true, force: true })
  await fs.rm(unpackWebview, { recursive: true, force: true })
  const {stageWindhawk}=await import('./stage-windhawk.mjs')
  await stageWindhawk(path.join(downloads,lock.windhawk.filename),path.join(resources,'windhawk'),{source:lock.windhawk,sevenZip:options.sevenZip??'C:/Program Files/7-Zip/7z.exe'})
  console.log('Hashing the install resources...')
  const manifest = await writeManifest(resources, { version, nodeVersion: lock.node.version, webview2Version: lock.webview2.version })
  console.log(JSON.stringify({ output, resources, version, files: manifest.files.length, expanded_bytes: manifest.files.reduce((n, f) => n + f.bytes, 0) }, null, 2))
  return manifest
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const args = process.argv.slice(2)
  const option = name => args[args.indexOf(`--${name}`) + 1]
  if (!args.includes('--downloads') || !args.includes('--out')) throw new Error('Usage: node scripts/prepare-installer.mjs --downloads <official-downloads> --out <new-output-directory> [--version <version>]')
  await prepareResources(path.resolve(option('downloads')), path.resolve(option('out')), {
    version: args.includes('--version') ? option('version') : undefined,
  })
}
