import assert from 'node:assert/strict'
import { test } from 'node:test'
import { installerScript, scriptSource, writeOwnershipManifest, findTool, stageMakensis, MIGRATE_HOME_FLAG } from './installer.mjs'
import fs from 'node:fs'
import os from 'node:os'
import { spawnSync } from 'node:child_process'
import { createHash } from 'node:crypto'
import { createRequire } from 'node:module'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')
// require 不会触发 install-update.cjs 的 CLI 主分支（require.main !== module），
// 所以本机的 %LOCALAPPDATA% 在任何用例里都不会被读到、更不会被删。
const updater = createRequire(import.meta.url)('./install-update.cjs')

// ── #27-D 用户数据目录迁移的临时夹具 ─────────────────────────────────────
// 目录名必须是 DSH-FISH 与 FISH Dock：migrateHome 只认这一对名字，并要求两者在
// 同一个父目录下。父目录是 mkdtemp 出来的临时目录，与本机真实的 %LOCALAPPDATA%
// 没有任何关系 —— 所有删除动作都只发生在这些临时目录里。
function homeFixture() {
  const base = fs.mkdtempSync(path.join(os.tmpdir(), 'fish-home-'))
  return { base, from: path.join(base, updater.HOME_FROM), to: path.join(base, updater.HOME_TO) }
}
function seed(directory, rows) {
  for (const [name, text] of Object.entries(rows)) {
    const file = path.join(directory, ...name.split('/'))
    fs.mkdirSync(path.dirname(file), { recursive: true })
    fs.writeFileSync(file, text)
  }
}
/// 一棵树的「相对路径 → 内容」快照，用来证明旧目录一个字节都没动。
function snapshot(directory) {
  const rows = {}
  const walk = dir => {
    for (const entry of fs.readdirSync(dir, { withFileTypes: true })) {
      const full = path.join(dir, entry.name), name = path.relative(directory, full).split(path.sep).join('/')
      if (entry.isSymbolicLink()) { rows[name] = '<link>'; continue }
      if (entry.isDirectory()) walk(full)
      else rows[name] = fs.readFileSync(full, 'utf8')
    }
  }
  walk(directory)
  return rows
}
/// 下面那个用例会真的编译并静默运行安装包，跑的是生产那一份 install-update.cjs。
/// 生产脚本带着 #27-D 的 --migrate-home，而本机 %LOCALAPPDATA%\DSH-FISH 是用户的
/// 活数据、%LOCALAPPDATA%\FISH Dock 又不存在 —— 正好是「会迁移并删掉旧目录」的
/// 组合。所以这里剥掉参数，让安装包只验文件事务；迁移行为由本文件后面的用例在
/// 临时目录里覆盖，不在真实目录上跑。剥完还要断言确实剥掉了，免得哪天参数改了名、
/// 这道闸悄悄失效。
function withoutHomeMigration(source) {
  assert.ok(source.includes(MIGRATE_HOME_FLAG), '生产脚本必须带 #27-D 的迁移参数，否则这里要跟着改')
  const stripped = source.replaceAll(` ${MIGRATE_HOME_FLAG}`, '')
  assert.ok(!stripped.includes(MIGRATE_HOME_FLAG), '迁移参数必须剥干净')
  return stripped
}

/// #27 P1：生产的 .onInit 会在升级时补读旧卸载表登记的安装位置（Uninstall\DSH-FISH）。
/// 本机真的装过 FISH Dock，那个键就在 —— 留着它，安装包会把 $INSTDIR 改到用户真实的
/// 安装目录里去。所以跑真实安装包之前，把这段 .onInit 整块剥掉，只验文件事务。
function withoutLegacyInstallDir(source) {
  const start = source.indexOf('Function .onInit')
  assert.ok(start > 0, '生产脚本必须有 .onInit 的升级回填')
  const end = source.indexOf('FunctionEnd', start)
  assert.ok(end > start, '.onInit 要有 FunctionEnd')
  const stripped = source.slice(0, start) + source.slice(end + 'FunctionEnd'.length)
  assert.ok(!stripped.includes('Function .onInit'), '.onInit 必须剥干净（否则会读本机真实注册表改 $INSTDIR）')
  return stripped
}

test('安装前置 PowerShell 脚本带 UTF-8 BOM，兼容 Windows PowerShell 5.1', () => {
  const file = path.join(root, 'scripts/install-stop.ps1')
  const bytes = fs.readFileSync(file)
  assert.deepEqual([...bytes.subarray(0, 3)], [0xef, 0xbb, 0xbf])
  if (process.platform === 'win32') {
    const powershell = path.join(process.env.SystemRoot || 'C:\\Windows', 'System32/WindowsPowerShell/v1.0/powershell.exe')
    const result = spawnSync(powershell, ['-NoProfile', '-NonInteractive', '-Command', `[scriptblock]::Create([IO.File]::ReadAllText('${file.replace(/'/g,"''")}')) | Out-Null`], { stdio: 'inherit', windowsHide: true })
    assert.equal(result.status, 0, 'Windows PowerShell 5.1 必须能解析安装脚本')
  }
})

test('覆盖安装先暂存完整发行资源，再事务更新受管文件并记录归属', () => {
  const source = installerScript({ version: '1.1.0', app: 'C:/build/FISH-Dock.exe', resources: 'C:/build/resources', outFile: 'C:/build/setup.exe', files: ['manifest.json', 'node/node.exe'] })
  assert.match(source, /InitPluginsDir/, '覆盖当前安装前先解压到独立暂存目录')
  assert.ok(source.includes(String.raw`$PLUGINSDIR\bootstrap\node.exe`), 'Windows 路径不应被 JS 转义成换行')
  assert.ok(source.includes(String.raw`$StageDir\install-update.cjs`))
  assert.match(source, /install-update\.cjs/, '使用事务更新器校验新文件、备份旧文件、更新归属清单')
  assert.ok(source.indexOf('install-update.cjs') < source.indexOf('CreateShortCut'), '文件事务成功后才更新快捷方式')
  assert.doesNotMatch(source, /SetOutPath "\$INSTDIR\\resources"\s+File \/r/, '禁止直接将资源覆盖到运行中的安装树')
})

test('资源直接解压到目标卷隔离暂存，Node 引导程序单独运行并在完成后清理', () => {
  const source = installerScript({ version: '1.1.0', app: 'C:/build/FISH-Dock.exe', resources: 'C:/build/resources', outFile: 'C:/build/setup.exe', files: ['manifest.json', 'node/node.exe'] })
  assert.match(source, /GetTempFileName \$StageSeed "\$INSTDIR"/)
  assert.match(source, /SetOutPath "\$StageDir\\resources"/)
  assert.match(source, /\$PLUGINSDIR\\bootstrap\\node\.exe/)
  assert.match(source, /install-update\.cjs" "\$StageDir" "\$INSTDIR"/)
  assert.match(source, /RMDir \/r "\$StageDir"/)
})

test('NSIS 通过真实 Node 和 PowerShell 5.1 完成中文路径安装与覆盖', { skip: process.platform !== 'win32', timeout: 120000 }, () => {
  const results = path.resolve(root, '../.test-results')
  fs.mkdirSync(results, { recursive: true })
  const fixture = fs.mkdtempSync(path.join(results, 'installer-runtime-'))
  const resources = path.join(fixture, 'resources'), target = path.join(fixture, '中文 安装目录')
  fs.mkdirSync(path.join(resources, 'node'), { recursive: true })
  fs.copyFileSync(process.execPath, path.join(resources, 'node/node.exe'))
  fs.writeFileSync(path.join(resources, 'manifest.json'), JSON.stringify({ schema: 2, product: 'DSH-FISH', files: [] }))
  const app = path.join(fixture, 'FISH-Dock.exe'), output = path.join(fixture, 'setup.exe')
  fs.writeFileSync(app, 'MZ synthetic application; never executed')
  const ownership = path.join(fixture, 'install-manifest.json')
  // 复用生产安装段，成功后截断系统注册与快捷方式步骤，仅作用于隔离文件树。
  let source = withoutLegacyInstallDir(withoutHomeMigration(installerScript({ version: '1.1.0', app, resources, outFile: output, files: ['node/node.exe', 'manifest.json'], installManifest: ownership })))
  const stop = source.indexOf('  SetOutPath "$INSTDIR"\n')
  assert.ok(stop > 0)
  source = source.slice(0, stop) + '  SetOutPath "$INSTDIR"\n  RMDir /r "$StageDir"\nSectionEnd\n'
  source = source.replace('SetCompressor /SOLID lzma', 'SetCompressor zlib').replace('SetCompressorDictSize 32', '')
  source = source.replace(/^InstallDir .*$/m, `InstallDir "${target.replaceAll('/', '\\')}"`).replace(/^InstallDirRegKey .*\r?\n/m, '')
  assert.ok(source.includes(`InstallDir "${target}"`))
  assert.doesNotMatch(source, /^InstallDirRegKey/m)
  const script = path.join(fixture, 'test.nsi')
  fs.writeFileSync(script, scriptSource(source))
  // makensis 这棵工具树只有放在 ASCII 路径下才写得动安装包；本仓可以落在中文目录。
  const makensis = stageMakensis(process.env.FISH_MAKENSIS || findTool('nsis-3.13/makensis.exe'))
  // 第二道闸：就算脚本里的参数哪天漏剥了，子进程带着跳过开关也不会迁移。
  const noMigrate = { ...process.env, [updater.MIGRATE_SKIP]: '1' }
  // 这道断言盯的就是上面那个风险本身：跑完这个用例，本机真实的
  // %LOCALAPPDATA%\FISH Dock 不该被凭空创建出来 —— 被创建了就说明迁移真的在
  // 用户目录上跑了一次，旧的 %LOCALAPPDATA%\DSH-FISH 可能已经被删掉。
  const realHome = process.env.LOCALAPPDATA ? path.join(process.env.LOCALAPPDATA, updater.HOME_TO) : null
  const realHomeBefore = realHome ? fs.existsSync(realHome) : true
  return writeOwnershipManifest(app, resources, ['node/node.exe', 'manifest.json'], ownership, '1.1.0').then(async () => {
    const built = spawnSync(makensis, ['/V2', script], { stdio: 'inherit', windowsHide: true, timeout: 60000 })
    assert.equal(built.status, 0)
    for (let attempt = 0; attempt < 2; attempt++) {
      const run = spawnSync(output, ['/S'], { stdio: 'inherit', windowsHide: true, timeout: 60000, env: noMigrate })
      assert.equal(run.status, 0, '真实安装调用链应成功退出')
      assert.equal(fs.readFileSync(path.join(target, 'FISH-Dock.exe'), 'utf8'), 'MZ synthetic application; never executed')
      assert.equal(JSON.parse(fs.readFileSync(path.join(target, 'install-manifest.json'), 'utf8')).version, '1.1.0')
      assert.deepEqual(fs.readFileSync(path.join(target, 'install-stop.ps1')), fs.readFileSync(path.join(root, 'scripts/install-stop.ps1')))
      if (!attempt) fs.writeFileSync(path.join(target, 'user-note.txt'), 'keep user data')
    }
    const runtimeBefore = fs.readFileSync(path.join(target, 'resources/node/node.exe'))
    fs.writeFileSync(app, 'MZ updated application; never executed')
    const delta = await writeOwnershipManifest(app, resources, ['node/node.exe', 'manifest.json'], ownership, '1.1.0')
    delta.payload = ['FISH-Dock.exe', 'install-update.cjs', 'install-stop.ps1']
    fs.writeFileSync(ownership, JSON.stringify(delta))
    let deltaSource = withoutLegacyInstallDir(withoutHomeMigration(installerScript({ version: '1.1.0', app, resources, outFile: path.join(fixture, 'delta.exe'), files: ['node/node.exe', 'manifest.json'], installManifest: ownership, payloadFiles: [] })))
    deltaSource = deltaSource.slice(0, deltaSource.indexOf('  SetOutPath "$INSTDIR"\n')) + '  SetOutPath "$INSTDIR"\n  RMDir /r "$StageDir"\nSectionEnd\n'
    deltaSource = deltaSource.replace('SetCompressor /SOLID lzma', 'SetCompressor zlib').replace('SetCompressorDictSize 32', '').replace(/^InstallDir .*$/m, `InstallDir "${target}"`).replace(/^InstallDirRegKey .*\r?\n/m, '')
    assert.doesNotMatch(deltaSource, /File \/r/)
    fs.writeFileSync(script, scriptSource(deltaSource))
    assert.equal(spawnSync(makensis, ['/V2', script], { stdio: 'inherit', windowsHide: true, timeout: 60000 }).status, 0)
    assert.equal(spawnSync(path.join(fixture, 'delta.exe'), ['/S'], { stdio: 'inherit', windowsHide: true, timeout: 60000, env: noMigrate }).status, 0)
    assert.equal(fs.readFileSync(path.join(target, 'FISH-Dock.exe'), 'utf8'), 'MZ updated application; never executed')
    assert.deepEqual(fs.readFileSync(path.join(target, 'resources/node/node.exe')), runtimeBefore)
    assert.equal(fs.readFileSync(path.join(target, 'user-note.txt'), 'utf8'), 'keep user data')
    if (realHome) assert.equal(fs.existsSync(realHome), realHomeBefore, '本用例不得在真实的 %LOCALAPPDATA% 下迁移用户数据')
  }).finally(() => fs.rmSync(fixture, { recursive: true, force: true, maxRetries: 10, retryDelay: 100 }))
})

// ── #27-D：迁移完成后直接删掉 %LOCALAPPDATA%\DSH-FISH，不留回滚副本 ────────
// 顺序是「整树盘点 → 逐个复制 → 逐个核对摘要 → 两份清单对齐 → 才删旧目录」。
// 全部用例都在 mkdtemp 出来的临时目录里跑，绝不触碰本机真实的 %LOCALAPPDATA%。
test('#27-D 迁完删除：逐个核对摘要之后直接删掉旧目录，不留回滚副本', () => {
  const { base, from, to } = homeFixture()
  try {
    const rows = {
      'data/config.json': '{"dock":"settings"}',
      'state/verified': 'resource-digest',
      'webview/Default/Cookies': 'sqlite-cookies',
      'webview/EBWebView/Local State': '{"profile":true}',
    }
    seed(from, rows)
    const said = []
    const result = updater.migrateHome(from, to, fs, { report: text => said.push(text), log: () => {}, attempts: 1 })
    assert.equal(result.migrated, true)
    assert.equal(result.removed, true, '迁完就直接删，不保留回滚副本（任务清单：遗留目录迁完直接删）')
    assert.equal(result.reason, 'removed')
    assert.equal(result.files, Object.keys(rows).length)
    assert.equal(fs.existsSync(from), false, '旧目录必须不存在')
    // 内容逐个对上，深层目录结构原样带过来。
    assert.deepEqual(snapshot(to), rows)
    // 迁移是「复制 + 核对 + 删旧」，不留副本、临时目录或半成品。
    assert.deepEqual(fs.readdirSync(base).sort(), [updater.HOME_TO])
    // 走安装器那一屏的话只能是 ASCII（nsExec 按 OEM 代码页转出去）。
    assert.ok(said.length >= 2, said.join(' | '))
    for (const line of said) assert.match(line, /^[\x20-\x7e]*$/, line)
    assert.ok(said.some(line => line.includes(`${Object.keys(rows).length} files verified`)), said.join(' | '))
  } finally { fs.rmSync(base, { recursive: true, force: true }) }
})

test('#27-D 未迁完不删：新目录已存在、复制中断、摘要不符、旧目录不在，四种情况旧目录都原样保留', () => {
  // 1) 新目录已存在：整体跳过、不合并、不删除（与 README「已有新目录始终优先，
  //    不自动合并」一致）。旧目录留着，新目录里的东西也不被覆盖。
  {
    const { base, from, to } = homeFixture()
    try {
      seed(from, { 'data/config.json': 'old-settings' })
      seed(to, { 'data/config.json': 'new-settings' })
      const said = []
      const result = updater.migrateHome(from, to, fs, { report: text => said.push(text), attempts: 1 })
      assert.deepEqual(result, { migrated: false, removed: false, reason: 'target-exists', files: 0 })
      assert.equal(fs.readFileSync(path.join(from, 'data/config.json'), 'utf8'), 'old-settings', '旧目录原样保留')
      assert.equal(fs.readFileSync(path.join(to, 'data/config.json'), 'utf8'), 'new-settings', '已有新目录不被覆盖')
      assert.ok(said.some(line => /left where it is/.test(line)), said.join(' | '))
    } finally { fs.rmSync(base, { recursive: true, force: true }) }
  }
  // 2) 复制到一半中断：抛错、半成品新目录清干净、旧目录一个字节都不少。
  //    半成品必须删掉 —— 留着它，下一次就会被「新目录已存在」永久挡住，再也迁不完。
  {
    const { base, from, to } = homeFixture()
    try {
      const rows = { 'data/a.json': 'a', 'data/b.json': 'b', 'webview/c.bin': 'cccc' }
      seed(from, rows)
      const before = snapshot(from)
      let copies = 0
      const io = { ...fs, copyFileSync: (source, dest) => { if (++copies === 2) throw Object.assign(Error('模拟磁盘写满'), { code: 'ENOSPC' }); fs.copyFileSync(source, dest) } }
      assert.throws(() => updater.migrateHome(from, to, io, { attempts: 1 }), /模拟磁盘写满/)
      assert.equal(fs.existsSync(to), false, '半成品新目录要清掉')
      assert.deepEqual(snapshot(from), before, '旧目录原样保留')
      assert.deepEqual(fs.readdirSync(base).sort(), [updater.HOME_FROM])
    } finally { fs.rmSync(base, { recursive: true, force: true }) }
  }
  // 3) 摘要对不上（复制途中被改写、磁盘出错）：同样抛错、清半成品、留旧目录。
  {
    const { base, from, to } = homeFixture()
    try {
      const rows = { 'data/a.json': 'a', 'webview/c.bin': 'cccc' }
      seed(from, rows)
      const before = snapshot(from)
      const io = { ...fs, copyFileSync: (source, dest) => fs.writeFileSync(dest, 'torn write on purpose') }
      assert.throws(() => updater.migrateHome(from, to, io, { attempts: 1 }), /摘要不一致|清单与旧目录不一致/)
      assert.equal(fs.existsSync(to), false)
      assert.deepEqual(snapshot(from), before, '旧目录原样保留')
    } finally { fs.rmSync(base, { recursive: true, force: true }) }
  }
  // 4) 旧目录不存在：什么都不做，也不该顺手建一个新目录。
  //    另外目录名与父目录都被校验过，喂别的路径进来直接拒绝。
  {
    const { base, from, to } = homeFixture()
    try {
      assert.deepEqual(updater.migrateHome(from, to, fs, { attempts: 1 }), { migrated: false, removed: false, reason: 'absent', files: 0 })
      assert.equal(fs.existsSync(to), false, '旧目录不在时不该建出新目录')
      assert.throws(() => updater.migrateHome(path.join(base, 'something-else'), to, fs, { attempts: 1 }), /目录名不对/)
      assert.throws(() => updater.migrateHome(from, path.join(base, 'elsewhere', updater.HOME_TO), fs, { attempts: 1 }), /同一个父目录/)
      assert.deepEqual(fs.readdirSync(base), [])
    } finally { fs.rmSync(base, { recursive: true, force: true }) }
  }
})

test('#27-D 旧目录还在被写时重试：第一次核对不过，第二次迁完并删旧', () => {
  const { base, from, to } = homeFixture()
  try {
    const rows = { 'webview/Default/Cookies': 'sqlite-cookies', 'data/config.json': '{"dock":true}' }
    seed(from, rows)
    // WebView2 的子进程可能还在往旧目录里写：第一次复制被撕开一道口子，
    // 核对不过就整份回退重来，而不是把半份数据当迁移成功。
    let copies = 0
    const io = {
      ...fs,
      copyFileSync: (source, dest) => {
        fs.copyFileSync(source, dest)
        if (++copies === 1) fs.appendFileSync(dest, 'torn write')
      },
    }
    const logged = []
    const result = updater.migrateHome(from, to, io, { attempts: 3, log: line => logged.push(line) })
    assert.equal(result.migrated, true)
    assert.equal(result.attempts, 2, '第一次失败要重试，而不是一次就放弃')
    assert.equal(result.removed, true)
    assert.equal(fs.existsSync(from), false, '迁完就删旧目录')
    assert.deepEqual(snapshot(to), rows, '重试后拿到的是完整数据，不含被撕开的那一份')
    assert.ok(logged.some(line => /第 1\/3 次未成功/.test(line)), logged.join(' | '))
  } finally { fs.rmSync(base, { recursive: true, force: true }) }
})

test('#27-D 删旧目录本身失败不算迁移失败：数据已核对进新目录，旧目录留下并记一行', () => {
  const { base, from, to } = homeFixture()
  try {
    const rows = { 'data/config.json': '{"dock":true}', 'webview/Default/Cookies': 'cookies' }
    seed(from, rows)
    const logged = []
    // 旧目录被占用（WebView2 还开着）时 rmSync 会失败：数据已经在新目录里核对过，
    // 不能因为「删不掉旧的」就说迁移没做成，更不能回头去动新目录。
    const io = { ...fs, rmSync: (target, options) => { if (path.resolve(target) === path.resolve(from)) throw Object.assign(Error('模拟文件被占用'), { code: 'EBUSY' }); fs.rmSync(target, options) } }
    const result = updater.migrateHome(from, to, io, { attempts: 1, log: line => logged.push(line) })
    assert.equal(result.migrated, true)
    assert.equal(result.removed, false)
    assert.equal(result.reason, 'delete-failed')
    assert.deepEqual(snapshot(to), rows, '新目录里的数据完整')
    assert.equal(fs.existsSync(from), true, '删不掉就留着，交给用户手动清')
    assert.ok(logged.some(line => /删除失败/.test(line)), logged.join(' | '))
  } finally { fs.rmSync(base, { recursive: true, force: true }) }
})

test('#27-D 目录树里有链接或非普通文件时拒绝迁移，不跟着链接把树外的东西算进来', () => {
  const { base, from, to } = homeFixture()
  try {
    seed(from, { 'data/config.json': '{"dock":true}' })
    const outside = path.join(base, 'outside')
    fs.mkdirSync(outside, { recursive: true })
    fs.writeFileSync(path.join(outside, 'secret.txt'), 'not part of the home')
    let linked = false
    try { fs.symlinkSync(outside, path.join(from, 'data', 'link'), 'junction'); linked = true } catch { /* 建不出链接就跳过这一条 */ }
    if (linked) {
      const before = snapshot(from)
      assert.throws(() => updater.migrateHome(from, to, fs, { attempts: 1 }), /链接|非普通文件/)
      assert.equal(fs.existsSync(to), false)
      assert.equal(fs.existsSync(path.join(outside, 'secret.txt')), true, '树外的文件一个都不能动')
      assert.deepEqual(snapshot(from), before)
    }
  } finally { fs.rmSync(base, { recursive: true, force: true }) }
})

// ── #27 P2 ④：升级后安装目录里只有一个 exe（FISH-Dock.exe） ────────────────
// 用户已确认「取消回滚点，旧 exe 不留」。这里真编译并静默跑一遍安装包：目标目录先
// 摆成旧版本的样子（DSH-FISH.exe + wakuwaku.exe + 旧归属清单），装完只该剩
// FISH-Dock.exe。为了绝不碰本机真实注册表与用户数据，脚本截断在 SetOutPath 之后
// （不写卸载键、不建快捷方式），并剥掉 .onInit 与 --migrate-home。
test('#27 P2 ④：覆盖升级后安装目录只有 FISH-Dock.exe，旧 exe 一个不留', { skip: process.platform !== 'win32', timeout: 120000 }, async () => {
  const results = path.resolve(root, '../.test-results')
  fs.mkdirSync(results, { recursive: true })
  const fixture = fs.mkdtempSync(path.join(results, 'installer-exe-'))
  const resources = path.join(fixture, 'resources'), target = path.join(fixture, '升级 目录')
  try {
    fs.mkdirSync(path.join(resources, 'node'), { recursive: true })
    fs.copyFileSync(process.execPath, path.join(resources, 'node/node.exe'))
    // 旧包写的是 product "DSH-FISH"：升级链必须还读得动它。
    fs.writeFileSync(path.join(resources, 'manifest.json'), JSON.stringify({ schema: 2, product: 'DSH-FISH', files: [] }))
    const app = path.join(fixture, 'FISH-Dock.exe'), output = path.join(fixture, 'setup.exe'), ownership = path.join(fixture, 'install-manifest.json')
    fs.writeFileSync(app, 'MZ synthetic application; never executed')
    // 旧安装：两个旧 exe + 旧归属清单（升级前的现场）。
    const oldRows = { 'DSH-FISH.exe': 'MZ old program', 'wakuwaku.exe': 'MZ older program', 'resources/node/node.exe': fs.readFileSync(process.execPath) }
    fs.mkdirSync(path.join(target, 'resources', 'node'), { recursive: true })
    for (const [name, bytes] of Object.entries(oldRows)) fs.writeFileSync(path.join(target, ...name.split('/')), bytes)
    fs.writeFileSync(path.join(target, 'install-manifest.json'), JSON.stringify({
      schema: 1, product: 'FISH Dock', version: '1.2.3',
      files: Object.entries(oldRows).map(([name, bytes]) => ({ path: name, bytes: bytes.length, sha256: createHash('sha256').update(bytes).digest('hex') })),
    }))
    await writeOwnershipManifest(app, resources, ['node/node.exe', 'manifest.json'], ownership, '2.0.0')
    let source = withoutLegacyInstallDir(withoutHomeMigration(installerScript({ version: '2.0.0', app, resources, outFile: output, files: ['node/node.exe', 'manifest.json'], installManifest: ownership })))
    const stop = source.indexOf('  SetOutPath "$INSTDIR"\n')
    assert.ok(stop > 0)
    source = source.slice(0, stop) + '  SetOutPath "$INSTDIR"\n  RMDir /r "$StageDir"\nSectionEnd\n'
    source = source.replace('SetCompressor /SOLID lzma', 'SetCompressor zlib').replace('SetCompressorDictSize 32', '').replace(/^InstallDir .*$/m, `InstallDir "${target.replaceAll('/', '\\')}"`).replace(/^InstallDirRegKey .*\r?\n/m, '')
    const scriptPath = path.join(fixture, 'test.nsi')
    fs.writeFileSync(scriptPath, scriptSource(source))
    const makensis = stageMakensis(process.env.FISH_MAKENSIS || findTool('nsis-3.13/makensis.exe'))
    assert.equal(spawnSync(makensis, ['/V2', scriptPath], { stdio: 'inherit', windowsHide: true, timeout: 60000 }).status, 0)
    const run = spawnSync(output, ['/S'], { stdio: 'inherit', windowsHide: true, timeout: 60000, env: { ...process.env, [updater.MIGRATE_SKIP]: '1' } })
    assert.equal(run.status, 0)
    const exes = fs.readdirSync(target).filter(name => name.toLowerCase().endsWith('.exe')).sort()
    assert.deepEqual(exes, ['FISH-Dock.exe'], `安装目录里只该有一个主程序：${exes.join(', ')}`)
    assert.equal(fs.existsSync(path.join(target, 'DSH-FISH.exe')), false, '旧 exe 必须删掉')
    assert.equal(fs.existsSync(path.join(target, 'wakuwaku.exe')), false, '0.x 的旧 exe 必须删掉')
    assert.equal(fs.readFileSync(path.join(target, 'FISH-Dock.exe'), 'utf8'), 'MZ synthetic application; never executed')
    assert.equal(JSON.parse(fs.readFileSync(path.join(target, 'install-manifest.json'), 'utf8')).version, '2.0.0')
  } finally { fs.rmSync(fixture, { recursive: true, force: true, maxRetries: 10, retryDelay: 100 }) }
})
