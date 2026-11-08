// 构建期准备安装资源：从已锁定的官方下载件解出 Node（#73 起不再准备 Windhawk，2.1.9 起
// 不再准备固定版 WebView2），目录结构与安装后一致，并写一份清单（schema 2）供程序首次
// 启动全量校验、日常启动只查必需文件。安装包里只装这些文件；安装器不做压缩解包的二次搬运。
import fs from 'node:fs/promises'
import { createReadStream } from 'node:fs'
import { createHash } from 'node:crypto'
import { spawnSync } from 'node:child_process'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

export const MANIFEST_SCHEMA = 2
// 2.0.0 起资源清单里的 product 与安装归属清单统一成 "FISH Dock"（#27 P1）。
// 旧包写的是 "DSH-FISH"：宿主 runtime.rs 与安装器都还认它（升级链不断）。
export const PRODUCT = 'FISH Dock'
export const ARCHITECTURE = 'x86_64'
/// 程序启动必需的文件；与 src-tauri/src/runtime.rs 的 REQUIRED 保持一致。
export const REQUIRED = [
  'node/node.exe',
  'node/LICENSE',
  'licenses/Wakuwaku.txt',
]
// #73：不再携带 `windhawk/`（引擎 + LLVM-MinGW 编译器，原始 557.8 MB / 8413 个文件）。
// 外观页改为连接用户自己安装的 Windhawk；具体见 runtime.rs 的 REQUIRED 注释与 #73。
// 2.1.9（#78）：`webview2/` 与固定版许可也不再随包——改用系统 Evergreen 运行时，
// 缺失由 Setup 的前置检测挡下（见 installer.mjs 的 WEBVIEW2_UPDATE_KEY）。

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
/// 2.1.9 起不再写 `webview2_version`：固定版运行时已不随包，宿主 runtime.rs 的
/// `Manifest` 把它当可选字段，旧清单（写着它）仍能解析。
export async function writeManifest(directory, { version, nodeVersion, architecture = ARCHITECTURE, product = PRODUCT }) {
  if (!version) throw new Error('manifest needs the product version')
  const files = (await inventory(directory)).filter(file => file.path !== 'manifest.json')
  const manifest = { schema: MANIFEST_SCHEMA, product, version, architecture, node_version: nodeVersion, files }
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

/// 安装包里随附的第三方许可说明。2.1.9 起不再随包分发固定版 WebView2 与它的许可原文，
/// 这段文字里也就不能再出现固定版与它的条款文件（Setup 只负责检测系统 Evergreen 运行时）。
export const NOTICE_TEXT = [
  'FISH Dock runtime resources',
  'Node.js and its third-party terms: ../node/LICENSE',
  'Wakuwaku terms: Wakuwaku.txt',
  '',
  'Microsoft Edge WebView2 Runtime is provided by the system (Evergreen) and is not bundled with this installer.',
  '',
].join('\n')

/// 从官方下载件准备 `output/resources`：解包 Node（#73 起 Windhawk 不再随包，2.1.9 起
/// 固定版 WebView2 也不再随包），放许可原文，最后写清单。每一步都先核对摘要，
/// 不信任之前的解压工作目录。
export async function prepareResources(downloads, output, options = {}) {
  if (process.platform !== 'win32') throw new Error('This resource builder requires Windows x64 tools')
  const lock = options.lock ?? JSON.parse(await fs.readFile(path.join(root, 'scripts/runtime-sources.json'), 'utf8'))
  const file = path.join(downloads, lock.node.filename)
  if (await hashFile(file) !== lock.node.sha256) throw new Error(`Official source SHA-256 mismatch: ${lock.node.filename}`)
  const version = options.version ?? await productVersion()
  await fs.mkdir(path.dirname(output), { recursive: true })
  await fs.mkdir(output)
  const resources = path.join(output, 'resources')
  await fs.mkdir(resources)
  const unpackNode = path.join(output, 'node-unpack')
  await fs.mkdir(unpackNode)
  run('tar.exe', ['-xf', file, '-C', unpackNode], true)
  await fs.rename(path.join(unpackNode, lock.node.directory), path.join(resources, 'node'))
  await fs.mkdir(path.join(resources, 'licenses'))
  await fs.copyFile(path.join(root, 'LICENSE'), path.join(resources, 'licenses/Wakuwaku.txt'))
  await fs.writeFile(path.join(resources, 'licenses/runtime-sources.json'), `${JSON.stringify(lock, null, 2)}\n`)
  await fs.writeFile(path.join(resources, 'licenses/NOTICE.txt'), NOTICE_TEXT)
  // 解包目录已用完：安装资源就是 resources/，安装器不再压缩打包。
  await fs.rm(unpackNode, { recursive: true, force: true })
  // #73：不再解 Windhawk 引擎与编译器——随包分发已移除，外观页改用用户自己装的那份。
  // 2.1.9：不再解固定版 WebView2——Setup 只检测系统 Evergreen 运行时。
  console.log('Hashing the install resources...')
  const manifest = await writeManifest(resources, { version, nodeVersion: lock.node.version })
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
