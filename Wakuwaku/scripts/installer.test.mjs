// 安装包脚本的生成规则：先红后绿。这里不依赖 NSIS 的运行时行为，只验证生成出来的
// 脚本把「装什么、装到哪、怎么卸」说清楚了，并且卸载只动安装器自己放下的文件。
import assert from 'node:assert/strict'
import { test } from 'node:test'
import { mkdtemp, mkdir, writeFile, rm, readFile, stat } from 'node:fs/promises'
import { existsSync } from 'node:fs'
import { spawnSync } from 'node:child_process'
import os from 'node:os'
import path from 'node:path'
import { fileURLToPath } from 'node:url'
import { installerScript, resourceDirectories, resourceFiles, scriptSource, writeOwnershipManifest, REQUIRED, findTool } from './installer.mjs'

import { REQUIRED as RUNTIME_REQUIRED } from './prepare-installer.mjs'

test('安装器与运行资源必需清单一致且不预装业务插件', () => {
  assert.deepEqual(REQUIRED, RUNTIME_REQUIRED)
  assert.equal(REQUIRED.some(name => name.startsWith('ofm/')), false)
  assert.ok(REQUIRED.includes('windhawk/Compiler/bin/clang++.exe'))
})
const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')
const makensis = process.env.FISH_MAKENSIS || findTool('nsis-3.13/makensis.exe')

const LISTED = [...REQUIRED, 'licenses/WebView2-Fixed-Version.html', 'licenses/NOTICE.txt', 'webview2/locales/zh-CN.pak', 'webview2/EBWebView/x.txt']

// 一份最小的安装资源：真实目录结构，内容随便，够生成脚本即可。
// node/node.exe 用不可压缩的字节，好让「安装包里确实带上了资源」能按体积判断。
// 另外故意留一个空目录：真实 Node 发行包里就有这种只有目录、没有文件的层级
// （node/node_modules/npm/tap-snapshots/...），它们不在文件清单里，卸载时同样要清掉。
const EMPTY = 'node/node_modules/npm/tap-snapshots/workspaces/arborist'

function noise(bytes) {
  const buffer = Buffer.alloc(bytes)
  let state = 0x9e3779b9
  for (let i = 0; i < bytes; i += 1) {
    state = (state * 1664525 + 1013904223) >>> 0
    buffer[i] = state >>> 24
  }
  return buffer
}

async function fakeResources() {
  const dir = await mkdtemp(path.join(os.tmpdir(), 'fish-nsi-'))
  const resources = path.join(dir, 'resources')
  for (const name of LISTED) {
    await mkdir(path.dirname(path.join(resources, name)), { recursive: true })
    await writeFile(path.join(resources, name), name === 'node/node.exe' ? noise(400_000) : `${name}\n`)
  }
  await mkdir(path.join(resources, EMPTY), { recursive: true })
  const files = LISTED.map(name => ({ path: name }))
  await writeFile(path.join(resources, 'manifest.json'), JSON.stringify({ schema: 2, files }, null, 2))
  const app = path.join(dir, 'DSH-FISH.exe')
  await writeFile(app, 'MZ fake program')
  return { dir, resources, app, listed: [...LISTED, 'manifest.json'].sort() }
}

async function script(env, extra = {}) {
  const files = await resourceFiles(env.resources)
  const directories = await resourceDirectories(env.resources)
  await writeOwnershipManifest(env.app, env.resources, files, path.join(env.dir, 'install-manifest.json'), '0.3.3')
  return { files, directories, text: installerScript({ version: '0.3.3', app: env.app, resources: env.resources, outFile: path.join(env.dir, 'Setup.exe'), files, directories, ...extra }) }
}

test('资源清单里的每个文件都进安装包，安装到 $INSTDIR\\resources', async () => {
  const env = await fakeResources()
  try {
    const { files, text } = await script(env)
    assert.match(text, /^Unicode true$/m)
    assert.match(text, /SetOutPath "\$StageDir\\resources"/)
    assert.match(text, /install-update\.cjs/)
    assert.match(text, /File \/r "\$\{RESOURCES\}\\\*\.\*"/)
    assert.equal(files.length, env.listed.length)
    for (const name of env.listed) assert.ok(files.includes(name), name)
    assert.match(text, /File "\$\{APPFILE\}"/)
  } finally { await rm(env.dir, { recursive: true, force: true }) }
})

test('允许选安装位置，建立开始菜单与桌面快捷方式，并注册卸载入口', async () => {
  const env = await fakeResources()
  try {
    const { text } = await script(env)
    assert.match(text, /!define PRODUCT "FISH Dock"/)
    assert.match(text, /!define MUI_ICON ".*DSH-FISH-Windows\.ico"/)
    assert.match(text, /!define MUI_UNICON ".*DSH-FISH-Windows\.ico"/)
    assert.match(text, /!insertmacro MUI_PAGE_DIRECTORY/)
    assert.match(text, /InstallDir "\$LOCALAPPDATA\\Programs\\\$\{PRODUCT\}"/)
    assert.match(text, /InstallDirRegKey HKCU "\$\{UNINSTALL_KEY\}" "InstallLocation"/)
    assert.match(text, /CreateShortCut "\$SMPROGRAMS\\\$\{PRODUCT\}\\\$\{PRODUCT\}\.lnk" "\$INSTDIR\\DSH-FISH\.exe"/)
    assert.match(text, /CreateShortCut "\$DESKTOP\\\$\{PRODUCT\}\.lnk" "\$INSTDIR\\DSH-FISH\.exe"/)
    assert.match(text, /CreateShortCut "\$SMPROGRAMS\\\$\{PRODUCT\}\\卸载 \$\{PRODUCT\}\.lnk" "\$INSTDIR\\uninstall\.exe"/)
    assert.match(text, /WriteUninstaller "\$INSTDIR\\uninstall\.exe"/)
    assert.match(text, /Uninstall\\DSH-FISH/)
    assert.match(text, /WriteRegStr HKCU "\$\{UNINSTALL_KEY\}" "UninstallString" '"\$INSTDIR\\uninstall\.exe"'/)
    assert.match(text, /WriteRegStr HKCU "\$\{UNINSTALL_KEY\}" "DisplayVersion" "\$\{VERSION\}"/)
    assert.match(text, /WriteRegStr HKCU "\$\{UNINSTALL_KEY\}" "InstallLocation" "\$INSTDIR"/)
  } finally { await rm(env.dir, { recursive: true, force: true }) }
})

test('卸载只删安装器放下的文件，绝不递归删除目录、绝不碰用户数据', async () => {
  const env = await fakeResources()
  try {
    const { files, text } = await script(env)
    assert.doesNotMatch(text.slice(text.indexOf('Section "Uninstall"')), /RMDir \/r/i, '卸载不得递归删目录：用户可能把程序装进别的目录')
    assert.doesNotMatch(text.slice(text.indexOf('Section "Uninstall"')), /DSH-FISH\\data|DSH-FISH\\webview|\.dsh-fish|\.fish-dock|mail-secrets|channel-credentials|channel-pack/, '账号、密钥、用量、设置与 WebView 数据不在安装目录里，卸载不得提到删除它们')
    const uninstall = text.slice(text.indexOf('Section "Uninstall"'))
    const inner = [...uninstall.matchAll(/^\s*Delete "(\$INSTDIR\\[^"]+)"/gm)].map(m => m[1].replace(/^\$INSTDIR\\/, '').replace(/\\/g, '/'))
    const owned = new Set(['DSH-FISH.exe', 'uninstall.exe', 'install-manifest.json', 'install-state.json', 'install-update.cjs', 'install-stop.ps1', ...files.map(name => `resources/${name}`)])
    for (const name of inner) assert.ok(owned.has(name), `卸载删除了安装器没放下的文件：${name}`)
    for (const name of files) assert.ok(inner.includes(`resources/${name}`), `卸载漏掉了：${name}`)
    // 安装目录之外只允许删自己建的两个快捷方式和开始菜单文件夹。
    const outside = [...uninstall.matchAll(/^\s*(?:Delete|RMDir) "([^"]+)"/gm)].map(m => m[1]).filter(name => name !== '$INSTDIR' && !name.startsWith('$INSTDIR\\'))
    for (const name of outside) assert.match(name, /^\$(SMPROGRAMS|DESKTOP)\\\$\{PRODUCT\}(\.lnk|\\\$\{PRODUCT\}\.lnk|\\卸载 \$\{PRODUCT\}\.lnk)?$/, name)
    // 目录只用非递归 RMDir，且只删清单里出现过的子目录。
    const removed = [...uninstall.matchAll(/^\s*RMDir "(.+)"$/gm)].map(m => m[1]).filter(name => name === '$INSTDIR' || name.startsWith('$INSTDIR\\'))
    for (const name of removed) assert.match(name, /^\$INSTDIR(\\resources(\\.+)?)?$/, name)
    assert.ok(removed.includes('$INSTDIR\\resources\\webview2\\EBWebView'), '深层目录也要清掉')
    assert.ok(!removed.includes('$INSTDIR\\resources\\node\\include'), '没出现过的目录不必列举')
  } finally { await rm(env.dir, { recursive: true, force: true }) }
})

// 卸载前让程序自己把「开机自动启动」的那个计划任务收掉：NSIS 侧拿不到用户 SID，
// 拼不出任务名；程序侧才知道任务名，也能先比对任务指向的正是本安装目录里的
// 这个 exe，才动手删除（别的安装、别的程序的任务不能碰）。
// 顺序也得对：先清理任务、再删主程序——主程序删了就没法再问了。
const CLEANUP_FLAG = '--wakuwaku-uninstall-cleanup'

test('卸载前先让本安装的程序清掉属于自己的登录任务，且清理排在删除主程序之前', async () => {
  const env = await fakeResources()
  try {
    const { text } = await script(env)
    const uninstall = text.slice(text.indexOf('Section "Uninstall"'))
    const call = uninstall.indexOf(CLEANUP_FLAG)
    assert.ok(call > 0, `卸载段没有让程序清理登录任务（${CLEANUP_FLAG}）`)
    assert.match(uninstall, /nsExec::ExecToLog[^\n]*\$INSTDIR\\DSH-FISH\.exe"/, '清理要用安装目录里的这个 exe 跑')
    const beforeProgram = uninstall.indexOf('Delete "$INSTDIR\\DSH-FISH.exe"')
    assert.ok(beforeProgram > 0, '卸载段应当删除主程序')
    assert.ok(call < beforeProgram, '清理必须排在删除主程序之前')
    // 清理失败不能挡住卸载：那条计划任务由程序在下次启动时按现状自愈。
    assert.match(uninstall, /ExecToLog[^\n]*\n\s*Pop \$0/, '清理的结果要取回来，不能让它中断卸载')
    // 归属判断只在程序侧做一次（比对任务指向的正是本安装目录里的这个 exe）：
    // 卸载脚本拿不到用户 SID，自己枚举或删除计划任务就有误删别人任务的风险。
    assert.doesNotMatch(uninstall, /schtasks/i, '卸载脚本不得自己枚举计划任务，归属判断交给程序')
    assert.doesNotMatch(text, /\/Delete\b/i, '卸载脚本不得直接删除任何计划任务')
  } finally { await rm(env.dir, { recursive: true, force: true }) }
})

test('资源里的空目录也要清掉，否则卸载后留下一串空壳文件夹', async () => {
  const env = await fakeResources()
  try {
    const { directories, text } = await script(env)
    const expected = EMPTY.replace(/\//g, '\\')
    assert.ok(directories.includes(EMPTY), `目录清单里应有空目录：${EMPTY}`)
    assert.match(text, new RegExp(`^  RMDir "\\$INSTDIR\\\\resources\\\\${expected.replace(/\\/g, '\\\\')}"$`, 'm'))
    // 空目录的每一层祖先都要删，否则最深那层删完还剩父级空壳。
    const uninstall = text.slice(text.indexOf('Section "Uninstall"'))
    const removed = [...uninstall.matchAll(/^\s*RMDir "(.+)"$/gm)].map(m => m[1])
    const parts = expected.split('\\')
    for (let i = 1; i <= parts.length; i += 1) {
      assert.ok(removed.includes(`$INSTDIR\\resources\\${parts.slice(0, i).join('\\')}`), `缺少目录：${parts.slice(0, i).join('\\')}`)
    }
  } finally { await rm(env.dir, { recursive: true, force: true }) }
})

test('安装时一次性给 WebView2 沙箱只读授权，日常启动不再需要递归授权', async () => {
  const env = await fakeResources()
  try {
    const { text } = await script(env)
    assert.match(text, /icacls "\$INSTDIR\\resources\\webview2"/)
    assert.match(text, /\*S-1-15-2-1:\(OI\)\(CI\)\(RX\) \*S-1-15-2-2:\(OI\)\(CI\)\(RX\) \/T/)
    assert.match(text, /nsExec::ExecToLog/)
  } finally { await rm(env.dir, { recursive: true, force: true }) }
})

test('资源目录缺少清单或混入清单外文件时拒绝生成安装包', async () => {
  const env = await fakeResources()
  try {
    const manifest = await readFile(path.join(env.resources, 'manifest.json'), 'utf8')
    await rm(path.join(env.resources, 'manifest.json'))
    await assert.rejects(resourceFiles(env.resources), /manifest\.json/)
    await writeFile(path.join(env.resources, 'manifest.json'), manifest)
    await writeFile(path.join(env.resources, 'stranger.txt'), 'not in the manifest\n')
    await assert.rejects(resourceFiles(env.resources), /清单之外/)
    await rm(path.join(env.resources, 'stranger.txt'))
    await rm(path.join(env.resources, 'node/node.exe'))
    await assert.rejects(resourceFiles(env.resources), /node\/node\.exe/)
  } finally { await rm(env.dir, { recursive: true, force: true }) }
})

test('生成的脚本能被离线 makensis 真正编译出安装包', async () => {
  assert.ok(existsSync(makensis), `缺少离线 NSIS：${makensis}`)
  const env = await fakeResources()
  try {
    const outFile = path.join(env.dir, 'DSH-FISH-Setup-0.3.3.exe')
    const { text } = await script(env, { outFile })
    const scriptPath = path.join(env.dir, 'installer.nsi')
    await writeFile(scriptPath, scriptSource(text), 'utf8')
    // 沙箱里带管道的 spawnSync 会被拒，用 inherit 让 makensis 直接写终端。
    const result = spawnSync(makensis, ['/V2', scriptPath], { stdio: 'inherit' })
    assert.equal(result.error, undefined, String(result.error))
    assert.equal(result.status, 0)
    assert.ok((await stat(outFile)).size > 100_000, '安装包应当包含全部安装资源')
  } finally { await rm(env.dir, { recursive: true, force: true }) }
})
