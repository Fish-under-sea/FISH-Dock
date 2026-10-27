import assert from 'node:assert/strict'
import { test } from 'node:test'
import { installerScript, scriptSource, writeOwnershipManifest, findTool } from './installer.mjs'
import fs from 'node:fs'
import { spawnSync } from 'node:child_process'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')

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
  const source = installerScript({ version: '1.1.0', app: 'C:/build/DSH-FISH.exe', resources: 'C:/build/resources', outFile: 'C:/build/setup.exe', files: ['manifest.json', 'node/node.exe'] })
  assert.match(source, /InitPluginsDir/, '覆盖当前安装前先解压到独立暂存目录')
  assert.ok(source.includes(String.raw`$PLUGINSDIR\bootstrap\node.exe`), 'Windows 路径不应被 JS 转义成换行')
  assert.ok(source.includes(String.raw`$StageDir\install-update.cjs`))
  assert.match(source, /install-update\.cjs/, '使用事务更新器校验新文件、备份旧文件、更新归属清单')
  assert.ok(source.indexOf('install-update.cjs') < source.indexOf('CreateShortCut'), '文件事务成功后才更新快捷方式')
  assert.doesNotMatch(source, /SetOutPath "\$INSTDIR\\resources"\s+File \/r/, '禁止直接将资源覆盖到运行中的安装树')
})

test('资源直接解压到目标卷隔离暂存，Node 引导程序单独运行并在完成后清理', () => {
  const source = installerScript({ version: '1.1.0', app: 'C:/build/DSH-FISH.exe', resources: 'C:/build/resources', outFile: 'C:/build/setup.exe', files: ['manifest.json', 'node/node.exe'] })
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
  const app = path.join(fixture, 'DSH-FISH.exe'), output = path.join(fixture, 'setup.exe')
  fs.writeFileSync(app, 'MZ synthetic application; never executed')
  const ownership = path.join(fixture, 'install-manifest.json')
  // 复用生产安装段，成功后截断系统注册与快捷方式步骤，仅作用于隔离文件树。
  let source = installerScript({ version: '1.1.0', app, resources, outFile: output, files: ['node/node.exe', 'manifest.json'], installManifest: ownership })
  const stop = source.indexOf('  SetOutPath "$INSTDIR"\n')
  assert.ok(stop > 0)
  source = source.slice(0, stop) + '  SetOutPath "$INSTDIR"\n  RMDir /r "$StageDir"\nSectionEnd\n'
  source = source.replace('SetCompressor /SOLID lzma', 'SetCompressor zlib').replace('SetCompressorDictSize 32', '')
  source = source.replace(/^InstallDir .*$/m, `InstallDir "${target.replaceAll('/', '\\')}"`).replace(/^InstallDirRegKey .*\r?\n/m, '')
  assert.ok(source.includes(`InstallDir "${target}"`))
  assert.doesNotMatch(source, /^InstallDirRegKey/m)
  const script = path.join(fixture, 'test.nsi')
  fs.writeFileSync(script, scriptSource(source))
  const makensis = process.env.FISH_MAKENSIS || findTool('nsis-3.13/makensis.exe')
  return writeOwnershipManifest(app, resources, ['node/node.exe', 'manifest.json'], ownership, '1.1.0').then(async () => {
    const built = spawnSync(makensis, ['/V2', script], { stdio: 'inherit', windowsHide: true, timeout: 60000 })
    assert.equal(built.status, 0)
    for (let attempt = 0; attempt < 2; attempt++) {
      const run = spawnSync(output, ['/S'], { stdio: 'inherit', windowsHide: true, timeout: 60000 })
      assert.equal(run.status, 0, '真实安装调用链应成功退出')
      assert.equal(fs.readFileSync(path.join(target, 'DSH-FISH.exe'), 'utf8'), 'MZ synthetic application; never executed')
      assert.equal(JSON.parse(fs.readFileSync(path.join(target, 'install-manifest.json'), 'utf8')).version, '1.1.0')
      assert.deepEqual(fs.readFileSync(path.join(target, 'install-stop.ps1')), fs.readFileSync(path.join(root, 'scripts/install-stop.ps1')))
      if (!attempt) fs.writeFileSync(path.join(target, 'user-note.txt'), 'keep user data')
    }
    const runtimeBefore = fs.readFileSync(path.join(target, 'resources/node/node.exe'))
    fs.writeFileSync(app, 'MZ updated application; never executed')
    const delta = await writeOwnershipManifest(app, resources, ['node/node.exe', 'manifest.json'], ownership, '1.1.0')
    delta.payload = ['DSH-FISH.exe', 'install-update.cjs', 'install-stop.ps1']
    fs.writeFileSync(ownership, JSON.stringify(delta))
    let deltaSource = installerScript({ version: '1.1.0', app, resources, outFile: path.join(fixture, 'delta.exe'), files: ['node/node.exe', 'manifest.json'], installManifest: ownership, payloadFiles: [] })
    deltaSource = deltaSource.slice(0, deltaSource.indexOf('  SetOutPath "$INSTDIR"\n')) + '  SetOutPath "$INSTDIR"\n  RMDir /r "$StageDir"\nSectionEnd\n'
    deltaSource = deltaSource.replace('SetCompressor /SOLID lzma', 'SetCompressor zlib').replace('SetCompressorDictSize 32', '').replace(/^InstallDir .*$/m, `InstallDir "${target}"`).replace(/^InstallDirRegKey .*\r?\n/m, '')
    assert.doesNotMatch(deltaSource, /File \/r/)
    fs.writeFileSync(script, scriptSource(deltaSource))
    assert.equal(spawnSync(makensis, ['/V2', script], { stdio: 'inherit', windowsHide: true, timeout: 60000 }).status, 0)
    assert.equal(spawnSync(path.join(fixture, 'delta.exe'), ['/S'], { stdio: 'inherit', windowsHide: true, timeout: 60000 }).status, 0)
    assert.equal(fs.readFileSync(path.join(target, 'DSH-FISH.exe'), 'utf8'), 'MZ updated application; never executed')
    assert.deepEqual(fs.readFileSync(path.join(target, 'resources/node/node.exe')), runtimeBefore)
    assert.equal(fs.readFileSync(path.join(target, 'user-note.txt'), 'utf8'), 'keep user data')
  }).finally(() => fs.rmSync(fixture, { recursive: true, force: true, maxRetries: 10, retryDelay: 100 }))
})
