// 安装包脚本的生成规则：先红后绿。这里不依赖 NSIS 的运行时行为，只验证生成出来的
// 脚本把「装什么、装到哪、怎么卸」说清楚了，并且卸载只动安装器自己放下的文件。
import assert from 'node:assert/strict'
import { test } from 'node:test'
import { mkdtemp, mkdir, writeFile, rm, readFile, stat } from 'node:fs/promises'
import { existsSync, readFileSync } from 'node:fs'
import { spawnSync } from 'node:child_process'
import { createRequire } from 'node:module'
import os from 'node:os'
import path from 'node:path'
import { fileURLToPath } from 'node:url'
import {
  installerScript, resourceDirectories, resourceFiles, scriptSource, writeOwnershipManifest, REQUIRED, findTool, stageMakensis,
  PRODUCT, INSTALL_ID, EXE_NAME, PUBLISHER, RUN_NAME, RUN_KEY, APPROVED_KEY, AT_LOGIN_ARGS, CLEANUP_FLAG,
  LEGACY_RUN_NAME, LEGACY_AT_LOGIN_ARGS, LEGACY_EXE_NAMES, LEGACY_INSTALL_IDS, LEGACY_DISPLAY_NAMES, LEGACY_HOME_DIRECTORY, HOME_DIRECTORY, MIGRATE_HOME_FLAG,
  LEGACY_WORKER_DIRECTORY, LEGACY_WORKER_FILE,
} from './installer.mjs'
// install-update.cjs 是真正动 fs 的那一半（#27-D 的迁移与删旧）；require 它不会
// 触发它的 CLI 主分支（require.main !== module），所以本机上任何目录都不会被碰。
const updater = createRequire(import.meta.url)('./install-update.cjs')

import { REQUIRED as RUNTIME_REQUIRED } from './prepare-installer.mjs'

test('安装器与运行资源必需清单一致且不预装业务插件', () => {
  assert.deepEqual(REQUIRED, RUNTIME_REQUIRED)
  assert.equal(REQUIRED.some(name => name.startsWith('ofm/')), false)
  assert.ok(REQUIRED.includes('windhawk/Compiler/bin/clang++.exe'))
})
const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')
// makensis 这棵工具树在 ASCII 路径下才写得动安装包（见 stageMakensis 的注释）；
// 本仓可以落在中文目录，所以这里统一拿暂存后的那一个。
const makensisTool = () => stageMakensis(process.env.FISH_MAKENSIS || findTool('nsis-3.13/makensis.exe'))

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
  const app = path.join(dir, 'FISH-Dock.exe')
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
    assert.match(text, /!define MUI_ICON ".*FISH-Dock-Windows\.ico"/)
    assert.match(text, /!define MUI_UNICON ".*FISH-Dock-Windows\.ico"/)
    assert.match(text, /!insertmacro MUI_PAGE_DIRECTORY/)
    assert.match(text, /InstallDir "\$LOCALAPPDATA\\Programs\\\$\{PRODUCT\}"/)
    assert.match(text, /InstallDirRegKey HKCU "\$\{UNINSTALL_KEY\}" "InstallLocation"/)
    assert.match(text, /CreateShortCut "\$SMPROGRAMS\\\$\{PRODUCT\}\\\$\{PRODUCT\}\.lnk" "\$INSTDIR\\FISH-Dock\.exe"/)
    assert.match(text, /CreateShortCut "\$DESKTOP\\\$\{PRODUCT\}\.lnk" "\$INSTDIR\\FISH-Dock\.exe"/)
    assert.match(text, /CreateShortCut "\$SMPROGRAMS\\\$\{PRODUCT\}\\卸载 \$\{PRODUCT\}\.lnk" "\$INSTDIR\\uninstall\.exe"/)
    assert.match(text, /WriteUninstaller "\$INSTDIR\\uninstall\.exe"/)
    assert.match(text, /Uninstall\\FISH Dock/)
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
    const owned = new Set(['FISH-Dock.exe', 'DSH-FISH.exe', 'wakuwaku.exe', 'wakuwaku-sensors.exe', 'uninstall.exe', 'install-manifest.json', 'install-state.json', 'install-update.cjs', 'install-stop.ps1', 'install.log', ...files.map(name => `resources/${name}`)])
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
// CLEANUP_FLAG 的字面量由上面「#27 沿用档」那条用例钉住，这里直接用它。

test('卸载前先让本安装的程序清掉属于自己的登录任务，且清理排在删除主程序之前', async () => {
  const env = await fakeResources()
  try {
    const { text } = await script(env)
    const uninstall = text.slice(text.indexOf('Section "Uninstall"'))
    const call = uninstall.indexOf(CLEANUP_FLAG)
    assert.ok(call > 0, `卸载段没有让程序清理登录任务（${CLEANUP_FLAG}）`)
    assert.match(uninstall, /nsExec::ExecToLog[^\n]*\$INSTDIR\\FISH-Dock\.exe"/, '清理要用安装目录里的这个 exe 跑')
    const beforeProgram = uninstall.indexOf('Delete "$INSTDIR\\FISH-Dock.exe"')
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

// 安装日志乱码（#16）：安装段用 nsExec::ExecToLog 把 node.exe 的输出转进
// 「显示详细信息」，而 nsExec 按 ANSI/OEM 代码页解释那些字节 —— node 写的是
// UTF-8，中文进去就是一堆问号方块。安装器本身是 Unicode true，所以问题不在
// NSIS 的界面文字，而在被转发的这一路输出：它必须是纯 ASCII。
// 转出去的每一句 console 输出都是一段字面量或 SAY 里的模板，合起来只许 ASCII。
test('写给安装日志的输出只用 ASCII，中文细节另写一份 install.log', async () => {
  const file = path.join(path.dirname(fileURLToPath(import.meta.url)), 'install-update.cjs')
  const text = await readFile(file, 'utf8')
  const shown = [...text.matchAll(/console\.(?:log|error)\(([^;\n]*)/g)].map(m => m[1])
  assert.deepEqual(shown.length, 4, '安装日志的输出点应当只有这些：停止旧进程、进度与用户数据迁移（同一个 report 出口）、结果 JSON、失败')
  for (const line of shown) assert.doesNotMatch(line, /[^\x00-\x7f]/, `安装日志里出现了非 ASCII：${line}`)
  // 字面量干净还不够：报错会带上安装目录的路径，中文目录是常事，出口得再过
  // 一道 ascii() 才保证那一屏永远是 ASCII。
  assert.match(text, /const ascii = text/, '出口要有一个把非 ASCII 换掉的过滤')
  for (const line of shown) assert.match(line, /^ascii\(/, `这条输出没过 ascii()：${line}`)
  const say = text.slice(text.indexOf('const SAY = {'), text.indexOf('// 细节日志'))
  assert.ok(say.length > 0, '输出的措辞都在 SAY 里')
  for (const str of say.match(/'[^'\n]*'|`[^`\n]*`/g) || []) assert.doesNotMatch(str, /[^\x00-\x7f]/, `SAY 里出现了非 ASCII：${str}`)
  // 进度与结果都经 report 出去：实参只能是 SAY 里的话，或者进度里那句
  // 纯数字的模板（`${label} ${done}/${total} (${percent}%)`）。
  const shape = /^SAY\.\w+\(.*\)$|^SAY\.\w+$/
  const counts = /^`\$\{label\} \$\{done\}\/\$\{total\} \(\$\{percent\}%\)`$/
  for (const call of text.matchAll(/report\(([^;\n]*)\)/g)) assert.ok(shape.test(call[1]) || counts.test(call[1]), `report 只许用 SAY：${call[1]}`)
  // 给用户看的细节：UTF-8 带 BOM 的 install.log，一句一行。
  assert.match(text, /install\.log/)
  assert.match(text, /'\\ufeff'/)
  assert.match(text, /appendFileSync\(file, [^;]*?,\s*'utf8'\)/)
})

// ── #27 旧名清理：2.0.0 的新名字 / 旧名字 / 绝不动的内部身份 ────────────────
// issue #27 的 P1/P2：用户看得见的名字全换成 FISH Dock，旧名只留来识别与清理。
test('#27 新名字档：2.0.0 起卸载键、主程序、Run 值、登录开关都用 FISH Dock', () => {
  assert.equal(PRODUCT, 'FISH Dock')
  assert.equal(PUBLISHER, 'Fish-under-sea')
  assert.equal(INSTALL_ID, 'FISH Dock', '卸载表登记名 → Uninstall\\FISH Dock（P1）')
  assert.equal(EXE_NAME, 'FISH-Dock.exe', '主程序名 → FISH-Dock.exe（P2）')
  assert.equal(RUN_NAME, 'FISH Dock', 'Run 值名 → FISH Dock（P0）')
  assert.equal(AT_LOGIN_ARGS, '--fish-dock-at-login run', '登录开关 → --fish-dock-at-login（P0）')
  assert.equal(CLEANUP_FLAG, '--fish-dock-uninstall-cleanup')
  assert.equal(RUN_KEY, 'Software\\Microsoft\\Windows\\CurrentVersion\\Run')
  assert.equal(APPROVED_KEY, 'Software\\Microsoft\\Windows\\CurrentVersion\\Explorer\\StartupApproved\\Run')
  // 旧名：留着识别与清理，绝不再写出去。
  assert.equal(LEGACY_RUN_NAME, 'Wakuwaku')
  assert.equal(LEGACY_AT_LOGIN_ARGS, '--wakuwaku-at-login run')
  assert.deepEqual(LEGACY_EXE_NAMES, ['DSH-FISH.exe', 'wakuwaku.exe', 'wakuwaku-sensors.exe'], '旧主程序与旧传感器助手都要按旧名删（#t16）')
  assert.ok(LEGACY_INSTALL_IDS.includes('DSH-FISH') && LEGACY_INSTALL_IDS.includes('Wakuwaku'), LEGACY_INSTALL_IDS.join(','))
  assert.equal(LEGACY_WORKER_DIRECTORY, 'FISH')
  assert.equal(LEGACY_WORKER_FILE, 'worker.json')
  const config = JSON.parse(readFileSync(new URL('../src-tauri/tauri.conf.json', import.meta.url), 'utf8'))
  // #t16：应用标识与名字层一起换成 FISH Dock（com.fish.dock）。旧标识
  // com.fish.dshfish 的 WebView2 用户数据目录不再被使用——那只是浏览器缓存/存储，
  // 换成新目录后首次启动自己重建；插件身份与令牌在我们自己的数据根里，不受影响。
  assert.equal(config.identifier, 'com.fish.dock', '应用标识随名字层一起换（旧标识的 WebView2 缓存作废，数据根不受影响）')
  // 图标：新名进安装包与 bundle，旧名保留一版别名（文件还在）。
  assert.ok(config.bundle.icon.includes('icons/FISH-Dock-Windows.ico'), config.bundle.icon.join(','))
  for (const alias of ['src-tauri/icons/DSH-FISH-Windows.ico', 'src-tauri/icons/dsh-fish.png']) {
    assert.ok(existsSync(new URL(`../${alias}`, import.meta.url)), `旧图标别名要留一版：${alias}`)
  }
  const runtime = readFileSync(new URL('../src-tauri/src/runtime.rs', import.meta.url), 'utf8')
  // 盯的是「宿主还读不读这个环境变量」，不是文件里能不能出现这五个字：
  // runtime.rs 里只剩一处注释，写明 2.0.0 起不再认旧名，那是该留的说明。
  assert.doesNotMatch(runtime, /(?:\bvar|\bvar_os)\(\s*"DSH_FISH_RUNTIME_HOME"/, '宿主已不再读 DSH_FISH_RUNTIME_HOME，安装器也不能把它当显式指定')
  assert.match(runtime, /"FISH Dock",\s*"DSH-FISH"/, '资源清单 product：新名与旧包的名字都要认')
})

test('#27 迁移档：用户数据目录的名字在宿主、安装器与更新器三处完全一致', () => {
  // 数据根名字已收进 config 单一入口（config.rs HOME_NAME），runtime.rs 只引用它：
  // 断言跟着入口走，才真的盯得住「三处一致」。
  const runtime = readFileSync(new URL('../src-tauri/src/runtime.rs', import.meta.url), 'utf8')
  assert.match(runtime, /crate::config::HOME_NAME/, '宿主的数据根目录名要走 config 单一入口')
  const config = readFileSync(new URL('../src-tauri/src/config.rs', import.meta.url), 'utf8')
  assert.ok(config.includes(`pub const HOME_NAME: &str = "${HOME_DIRECTORY}";`), `config::HOME_NAME 必须是安装器的目录名：${HOME_DIRECTORY}`)
  assert.equal(updater.HOME_TO, HOME_DIRECTORY, 'install-update.cjs 的迁移目标名与安装器一致')
  assert.equal(updater.HOME_FROM, LEGACY_HOME_DIRECTORY, 'install-update.cjs 的迁移来源名与安装器一致')
  assert.equal(updater.MIGRATE_FLAG, MIGRATE_HOME_FLAG, 'NSIS 传的参数名与更新器收的参数名一致')
  assert.deepEqual(updater.HOME_OVERRIDES, ['FISH_DOCK_APP_HOME'], '只认 2.0.0 的覆盖名；残留的 DSH_FISH_RUNTIME_HOME 不得让 #27-D 静默失效')
  // logon.rs 属迁移档：任务计划里给人看的名字换成 FISH Dock，用来匹配的名字不换。
  const logon = readFileSync(new URL('../src-tauri/src/logon.rs', import.meta.url), 'utf8')
  // 只看生产代码：这个文件自己的 Rust 测试里当然要写旧名字来做反证
  // （`assert!(!xml.contains("Starts Wakuwaku"))` 那种），扫全文会把断言当违规。
  const tested = logon.indexOf('\n#[cfg(test)]')
  const logonProduction = tested === -1 ? logon : logon.slice(0, tested)
  assert.ok(logonProduction.includes('<Author>FISH Dock</Author>'), '任务计划程序里的作者名换成 FISH Dock')
  assert.ok(logonProduction.includes('Starts FISH Dock as you log on'), '任务说明换成 FISH Dock')
  assert.ok(!logonProduction.includes('Starts Wakuwaku') && !logonProduction.includes('<Author>Wakuwaku</Author>'), '旧发行名不得留在给人看的字段里')
  assert.ok(logonProduction.includes('AT_LOGIN_FLAG') || logonProduction.includes('--wakuwaku-at-login'), '沿用档的命令行开关不能动人读的文字时顺手改掉')
})

test('#27 清理档：安装时删掉旧 exe、指向旧 exe 的自启动值与旧卸载登记', async () => {
  const env = await fakeResources()
  try {
    const { text } = await script(env)
    const install = text.slice(text.indexOf('Section "${PRODUCT}" SecInstall'), text.indexOf('Section "Uninstall"'))
    // 删旧建新：旧的 DSH-FISH.exe / wakuwaku.exe 在安装段就删掉，不等卸载。
    for (const name of LEGACY_EXE_NAMES) assert.match(install, new RegExp(`^  Delete "\\$INSTDIR\\\\${name.replace('.', '\\.')}"$`, 'm'), name)
    // Run 值：只匹配本安装目录里的旧主程序；指向当前主程序的那条是用户的选择，保留。
    assert.match(install, /ReadRegStr \$1 HKCU "\$\{RUN_KEY\}" "\$\{RUN_NAME\}"/)
    for (const name of LEGACY_EXE_NAMES) {
      assert.ok(install.includes(`$1 == '"$INSTDIR\\${name}"'`), `裸路径 ${name}`)
      assert.ok(install.includes(`$1 == '"$INSTDIR\\${name}" ${LEGACY_AT_LOGIN_ARGS}'`), `旧开关名 ${name}`)
    }
    // 新主程序自己那条 Run 值（用户的选择）：不在旧 exe 的匹配名单里。
    assert.ok(!install.includes(`$1 == '"$INSTDIR\\FISH-Dock.exe"'`), '指向当前主程序的 Run 值属用户的选择，安装时不得删')
    // StartupApproved\Run 的同名标记与 Run 值一起清，不留指不到程序的记录。
    assert.match(install, /DeleteRegValue HKCU "\$\{RUN_KEY\}" "\$\{RUN_NAME\}"\r?\n\s*DeleteRegValue HKCU "\$\{APPROVED_KEY\}" "\$\{RUN_NAME\}"/)
    // 卸载表旧登记项：一个旧名一条 !define，逐条按归属判定后整键删除。
    assert.deepEqual(LEGACY_INSTALL_IDS, ['DSH-FISH', 'Wakuwaku', 'FISH Dock'])
    for (const [index, id] of LEGACY_INSTALL_IDS.entries()) {
      assert.ok(text.includes(`!define LEGACY_UNINSTALL_${index + 1} "Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\${id}"`), id)
      assert.ok(install.includes(`ReadRegStr $1 HKCU "\${LEGACY_UNINSTALL_${index + 1}}" "InstallLocation"`), id)
      assert.ok(install.includes(`ReadRegStr $2 HKCU "\${LEGACY_UNINSTALL_${index + 1}}" "DisplayName"`), id)
      assert.ok(install.includes(`DeleteRegKey HKCU "\${LEGACY_UNINSTALL_${index + 1}}"`), id)
    }
    // 归属判定只有两种：登记位置就是本目录，或显示名是我们自己用过的旧名。
    assert.match(install, /\$\{If\} \$1 == "\$INSTDIR"/)
    for (const name of LEGACY_DISPLAY_NAMES) assert.ok(install.includes(`\${OrIf} $2 == '${name}'`), name)
    // 卸载表里那条带旧发行名的 ProductName 删掉；自己的登记项照常写回。
    assert.match(install, /DeleteRegValue HKCU "\$\{UNINSTALL_KEY\}" "ProductName"/)
    assert.match(install, /WriteRegStr HKCU "\$\{UNINSTALL_KEY\}" "InstallLocation" "\$INSTDIR"/)
    assert.doesNotMatch(install, /DeleteRegKey HKCU "\$\{UNINSTALL_KEY\}"/, '安装段不得删掉本次安装的卸载登记')
  } finally { await rm(env.dir, { recursive: true, force: true }) }
})

test('#27 清理档：卸载段也兜一遍底，且不越出安装目录去删东西', async () => {
  const env = await fakeResources()
  try {
    const { text } = await script(env)
    const uninstall = text.slice(text.indexOf('Section "Uninstall"'))
    assert.match(uninstall, /\$\{If\} \$0 == '"\$INSTDIR\\FISH-Dock\.exe" --fish-dock-at-login run'/)
    assert.match(uninstall, /\$\{OrIf\} \$0 == '"\$INSTDIR\\DSH-FISH\.exe" --wakuwaku-at-login run'/)
    assert.match(uninstall, /\$\{OrIf\} \$0 == '"\$INSTDIR\\wakuwaku\.exe"'/)
    assert.match(uninstall, /DeleteRegValue HKCU "\$\{APPROVED_KEY\}" "\$\{RUN_NAME\}"/)
    // 旧名 Wakuwaku 的那条值也兜一遍（1.x 写的），连它的 StartupApproved 标记。
    assert.match(uninstall, /ReadRegStr \$2 HKCU "\$\{RUN_KEY\}" "\$\{LEGACY_RUN_NAME\}"/)
    assert.match(uninstall, /DeleteRegValue HKCU "\$\{APPROVED_KEY\}" "\$\{LEGACY_RUN_NAME\}"/)
    for (const name of LEGACY_EXE_NAMES) assert.match(uninstall, new RegExp(`^  Delete "\\$INSTDIR\\\\${name.replace('.', '\\.')}"$`, 'm'), '卸载后安装目录里不该留下任何能启动的主程序')
    assert.equal((uninstall.match(/DeleteRegKey HKCU "\$\{LEGACY_UNINSTALL_\d+\}"/g) || []).length, LEGACY_INSTALL_IDS.length)
    // 注册表清理只许用 DeleteRegValue / DeleteRegKey；不能出现 schtasks，也不碰计划任务。
    assert.doesNotMatch(uninstall, /schtasks/i)
    // 用户数据仍然只是「保留」，删除动作一次都没有。
    assert.match(uninstall, /DetailPrint "已保留用户数据：%LOCALAPPDATA%\\FISH Dock"/)
    assert.doesNotMatch(uninstall, /RMDir \/r/i, '卸载段依旧不得递归删目录')
  } finally { await rm(env.dir, { recursive: true, force: true }) }
})

test('#27-D：只有安装器显式传 --migrate-home 才动用户数据目录，默认什么都不碰', async () => {
  const env = await fakeResources()
  try {
    const { text } = await script(env)
    assert.match(text, /install-update\.cjs" "\$StageDir" "\$INSTDIR" --migrate-home'/, 'NSIS 必须显式带上迁移参数')
    // 迁移的失败只提示、不回滚已经装好的程序；标记文件在暂存目录删掉之前读出来。
    assert.match(text, /IfFileExists "\$StageDir\\home-migration-failed"/)
    assert.ok(text.indexOf('$HomeMigrationFailed 1') < text.indexOf('RMDir /r "$StageDir"'), '标记要在暂存目录删除前读')
    assert.match(text, /MessageBox MB_OK\|MB_ICONEXCLAMATION "用户数据没能迁到新的目录，旧目录已原样保留/)
    // 三闸之一：跳过开关与覆盖变量都由 install-update.cjs 判，安装器只负责传参。
    assert.equal(updater.MIGRATE_SKIP, 'FISH_DOCK_SKIP_HOME_MIGRATION')
    assert.equal(updater.homePaths({ LOCALAPPDATA: 'C:\\Users\\me\\AppData\\Local' }).from, 'C:\\Users\\me\\AppData\\Local\\DSH-FISH')
    assert.equal(updater.homePaths({ LOCALAPPDATA: 'C:\\Users\\me\\AppData\\Local', FISH_DOCK_SKIP_HOME_MIGRATION: '1' }), null)
    assert.equal(updater.homePaths({ LOCALAPPDATA: 'C:\\Users\\me\\AppData\\Local', FISH_DOCK_APP_HOME: 'D:\\home' }), null)
    assert.equal(updater.homePaths({ LOCALAPPDATA: 'C:\\Users\\me\\AppData\\Local', DSH_FISH_RUNTIME_HOME: 'D:\\old-override' }).from, 'C:\\Users\\me\\AppData\\Local\\DSH-FISH', '宿主已不读 DSH_FISH_RUNTIME_HOME：残留它也要照常迁移')
    assert.equal(updater.homePaths({}), null, '读不到 LOCALAPPDATA 就不动任何目录')
    assert.ok(updater.MIGRATE_ATTEMPTS >= 1, 'WebView2 子进程还在写时要重试')
  } finally { await rm(env.dir, { recursive: true, force: true }) }
})

test('生成的脚本能被离线 makensis 真正编译出安装包', async () => {
  const makensis = makensisTool()
  assert.ok(existsSync(makensis), `缺少离线 NSIS：${makensis}`)
  const env = await fakeResources()
  try {
    const outFile = path.join(env.dir, 'FISH-Dock-Setup-0.3.3.exe')
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

// ── #27 P0 收尾：Run 值改名与迁移（①②） ──────────────────────────────────
// 旧版本把「开机自动启动」写在 Run 值 "Wakuwaku" 下；2.0.0 改叫 "FISH Dock"。
// 只搬「指向本安装目录里的旧主程序」的那一项；指向别的程序（真的 Wakuwaku）原样
// 保留并提示。注册表真值不在本机读写（真实安装由 t5 负责），这里验的是生成脚本里
// 的迁移链与它的边界；同一条判断的纯函数版本在 connection.rs 的 legacy_fix 里，
// 由 `legacy_tests` 单独覆盖。
test('#27 P0 ①：旧 Run 项指向本安装目录 → 安装时迁到新名字，旧项与它的 StartupApproved 标记一起清', async () => {
  const env = await fakeResources()
  try {
    const { text } = await script(env)
    const install = text.slice(text.indexOf('Section "${PRODUCT}" SecInstall'), text.indexOf('Section "Uninstall"'))
    // 只在「新名字还没有值」时迁移，绝不覆盖用户后来设的那一条。
    assert.match(install, /ReadRegStr \$1 HKCU "\$\{RUN_KEY\}" "\$\{RUN_NAME\}"\r?\n\s*\$\{If\} \$1 == ""\r?\n\s*ReadRegStr \$1 HKCU "\$\{RUN_KEY\}" "\$\{LEGACY_RUN_NAME\}"/)
    // 指向本安装目录里的旧主程序（裸路径或带旧开关名）才算我们的。
    const own = [`"$INSTDIR\\DSH-FISH.exe"`, `"$INSTDIR\\DSH-FISH.exe" --wakuwaku-at-login run`, `"$INSTDIR\\wakuwaku.exe"`]
    for (const form of own) assert.ok(install.includes(`$1 == '${form}'`), `该认这种写法：${form}`)
    // 迁移动作：写新名字 → 清旧名字的 StartupApproved 标记 → 删旧 Run 值。
    assert.ok(install.includes(`WriteRegStr HKCU "\${RUN_KEY}" "\${RUN_NAME}" '"$INSTDIR\\FISH-Dock.exe" --fish-dock-at-login run'`), '写到新名字、新主程序、新开关')
    assert.match(install, /DeleteRegValue HKCU "\$\{APPROVED_KEY\}" "\$\{LEGACY_RUN_NAME\}"/)
    assert.match(install, /DeleteRegValue HKCU "\$\{RUN_KEY\}" "\$\{LEGACY_RUN_NAME\}"/)
    assert.match(install, /开机自动启动已从旧名字 Wakuwaku 迁到/)
  } finally { await rm(env.dir, { recursive: true, force: true }) }
})

test('#27 P0 ②：旧 Run 项指向别的程序 → 原样保留，只在详情里提示', async () => {
  const env = await fakeResources()
  try {
    const { text } = await script(env)
    const install = text.slice(text.indexOf('Section "${PRODUCT}" SecInstall'), text.indexOf('Section "Uninstall"'))
    // 删旧名 / 删旧标记 / 写新名字三件事都在「匹配到本目录旧主程序」的分支里；
    // 不匹配时走 ElseIf，只打印一句，不动注册表。
    const elsewhere = install.indexOf('旧的 Wakuwaku 自启动项指向别的程序，已原样保留')
    assert.ok(elsewhere > 0, '指向别的程序时要有这句提示')
    // 这句落在 ${ElseIf} 分支里，且该分支从开头到这句之间没有任何写/删注册表的动作。
    const guard = install.lastIndexOf('${ElseIf} $1 != ""', elsewhere)
    assert.ok(guard > 0 && guard < elsewhere, '提示必须是 ElseIf 分支')
    const branch = install.slice(guard, elsewhere)
    assert.ok(!/WriteRegStr|DeleteRegValue/.test(branch), '不匹配的分支里不许有任何写/删注册表的动作')
    // 旧名只在迁移成功后删；脚本里不存在「无条件删 LEGACY_RUN_NAME」的写法。
    const legacyDeletes = [...install.matchAll(/^(\s*)DeleteRegValue HKCU "\\?\$\{RUN_KEY\}" "\\?\$\{LEGACY_RUN_NAME\}"$/gm)]
    assert.ok(legacyDeletes.length >= 1, '迁移成功时要删旧项')
    for (const m of legacyDeletes) assert.ok(m[1].length >= 6, '删除动作必须缩进在条件分支里（不能被无条件执行）')
  } finally { await rm(env.dir, { recursive: true, force: true }) }
})

// ── #27 P1 ③：卸载键搬迁后应用列表只剩一项 ────────────────────────────────
// 旧版本登记在 Uninstall\DSH-FISH，新版本登记在 Uninstall\FISH Dock。升级时：
// 先按新键找安装位置（读不到就用旧键登记的），文件事务之后把旧键整键删掉，
// 再写回新键 —— 应用列表里只会有一条 FISH Dock。本机不写真实注册表。
test('#27 P1 ③：卸载键从 Uninstall\\DSH-FISH 搬到 Uninstall\\FISH Dock，应用列表只剩一项', async () => {
  const env = await fakeResources()
  try {
    const { text } = await script(env)
    const install = text.slice(text.indexOf('Section "${PRODUCT}" SecInstall'), text.indexOf('Section "Uninstall"'))
    // 1) 新键名：Uninstall\FISH Dock；旧键名 DSH-FISH 只作为「旧登记项」被认出来。
    assert.ok(text.includes('!define UNINSTALL_KEY "Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\FISH Dock"'), '新登记键名')
    assert.ok(text.includes('!define LEGACY_UNINSTALL_1 "Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\DSH-FISH"'), '旧键名要认得')
    // 2) 升级沿用旧键登记的安装位置（否则会装出第二份）。
    const init = text.slice(text.indexOf('Function .onInit'), text.indexOf('FunctionEnd', text.indexOf('Function .onInit')))
    assert.match(init, /ReadRegStr \$1 HKCU "\$\{UNINSTALL_KEY\}" "InstallLocation"/)
    assert.match(init, /ReadRegStr \$1 HKCU "\$\{LEGACY_UNINSTALL_1\}" "InstallLocation"/)
    assert.match(init, /StrCpy \$INSTDIR \$1/)
    // 3) 旧键在文件事务之后整键删除（只按归属：InstallLocation 是本目录，或显示名是我们）。
    assert.ok(install.includes('DeleteRegKey HKCU "${LEGACY_UNINSTALL_1}"'), '旧键要删掉')
    // 4) 新键写回，且安装段不删自己这一条 —— 于是列表里只剩 FISH Dock 一项。
    assert.match(install, /WriteRegStr HKCU "\$\{UNINSTALL_KEY\}" "DisplayName" "\$\{PRODUCT\}"/)
    assert.doesNotMatch(install, /DeleteRegKey HKCU "\$\{UNINSTALL_KEY\}"/)
    // 5) 卸载段再兜一次底（旧键 + 新键都清）；旧键的整键删除次数 = 旧登记项个数。
    const uninstall = text.slice(text.indexOf('Section "Uninstall"'))
    assert.equal((uninstall.match(/DeleteRegKey HKCU "\$\{LEGACY_UNINSTALL_\d+\}"/g) || []).length, LEGACY_INSTALL_IDS.length)
  } finally { await rm(env.dir, { recursive: true, force: true }) }
})

// ── #27 P0 后半：%LOCALAPPDATA%\FISH\worker.json 的清理 ────────────────────
// 删除以「新数据根已就绪」为前提：%LOCALAPPDATA%\FISH Dock 在，才认为迁移/落位
// 完成，删这个文件；不在就一个字节都不碰。三闸（显式数据目录 / 跳过开关 /
// 读不到 LOCALAPPDATA）照旧。只在 mkdtemp 临时目录里跑。
test('#27 P0：新数据根就绪后才删 %LOCALAPPDATA%\\FISH\\worker.json，旧根没就绪就不动', async () => {
  const { mkdtemp, mkdir, writeFile, readFile: read } = await import('node:fs/promises')
  const base = await mkdtemp(path.join(os.tmpdir(), 'fish-worker-'))
  try {
    const local = path.join(base, 'Local')
    await mkdir(path.join(local, 'FISH'), { recursive: true })
    await writeFile(path.join(local, 'FISH', 'worker.json'), '{"old":true}')
    // 1) 新数据根不在：不动。
    assert.deepEqual(updater.staleWorker({ LOCALAPPDATA: local }), { removed: false, reason: 'no-new-root' })
    await read(path.join(local, 'FISH', 'worker.json'))
    // 2) 新数据根就绪：删文件，空的 FISH 目录顺手收掉。
    await mkdir(path.join(local, updater.HOME_TO), { recursive: true })
    const done = updater.staleWorker({ LOCALAPPDATA: local })
    assert.equal(done.removed, true)
    assert.equal(existsSync(path.join(local, 'FISH', 'worker.json')), false)
    assert.equal(existsSync(path.join(local, 'FISH')), false, '空了才收掉目录')
    // 3) FISH 目录里还有别人的东西：只删 worker.json，目录留着。
    await mkdir(path.join(local, 'FISH'), { recursive: true })
    await writeFile(path.join(local, 'FISH', 'worker.json'), '{}')
    await writeFile(path.join(local, 'FISH', 'other.txt'), 'keep')
    assert.equal(updater.staleWorker({ LOCALAPPDATA: local }).removed, true)
    assert.equal(existsSync(path.join(local, 'FISH', 'other.txt')), true, '别人的文件一个都不能动')
    assert.equal(existsSync(path.join(local, 'FISH')), true)
    // 4) 三闸：跳过开关 / 显式数据目录 / 读不到 LOCALAPPDATA 都不动。
    await writeFile(path.join(local, 'FISH', 'worker.json'), '{}')
    assert.deepEqual(updater.staleWorker({ LOCALAPPDATA: local, FISH_DOCK_SKIP_HOME_MIGRATION: '1' }), { removed: false, reason: 'skipped' })
    assert.deepEqual(updater.staleWorker({ LOCALAPPDATA: local, FISH_DOCK_APP_HOME: 'D:\\home' }), { removed: false, reason: 'skipped' })
    assert.deepEqual(updater.staleWorker({}), { removed: false, reason: 'skipped' })
    assert.equal(existsSync(path.join(local, 'FISH', 'worker.json')), true, '被拦住时文件必须还在')
  } finally { await rm(base, { recursive: true, force: true }) }
})
