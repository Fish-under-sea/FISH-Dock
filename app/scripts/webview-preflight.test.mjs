// 执行正式生成器的预检；机器登记映射到唯一 HKCU 测试键，不修改真实 WebView2 登记。
import { test } from 'node:test'
import assert from 'node:assert/strict'
import { mkdtemp, mkdir, writeFile, readFile, rm } from 'node:fs/promises'
import { spawnSync } from 'node:child_process'
import os from 'node:os'
import path from 'node:path'
import { installerScript, scriptSource, stageMakensis, findTool, WEBVIEW2_RUNTIME_GUID } from './installer.mjs'

test('WebView2 预检：用户/机器登记、注册表视图、自定义路径与失效运行时', { skip: process.platform !== 'win32' || process.env.FISH_DOCK_ISOLATED_INSTALL_TESTS !== '1' ? 'NSIS 原生探针仅在专用隔离机器显式运行' : false }, async () => {
  const dir = await mkdtemp(path.join(os.tmpdir(), 'fish-webview-'))
  const key = `Software\\FishDockPreflightTest\\${path.basename(dir)}`
  try {
    const runtime = path.join(dir, 'custom-runtime', '157.0.4341.42')
    await mkdir(path.join(runtime, 'EBWebView', 'x64'), { recursive: true })
    // 仅验证发现规则；合成 exe/DLL 不加载、不用于证明二进制有效性。
    await writeFile(path.join(runtime, 'msedgewebview2.exe'), 'synthetic browser')
    await writeFile(path.join(runtime, 'EBWebView', 'x64', 'EmbeddedBrowserWebView.dll'), 'synthetic client')
    const generated = installerScript({ version: '2.2.0', app: path.join(dir, 'fake.exe'), resources: dir, outFile: path.join(dir, 'unused.exe'), files: ['node/node.exe'] })
    const start = generated.indexOf('  StrCpy $2 0')
    const end = generated.indexOf('  ${If} $2 == 0\n    ; 2.2.0', start)
    assert.ok(start > 0 && end > start)
    const check = generated.slice(start, end).replace(
      /ReadRegStr (\$\d) (HKCU|HKLM) "Software\\Microsoft\\EdgeUpdate\\(Clients|ClientState)\\[^"\r\n]+"/g,
      (_, register, hive, section) => `ReadRegStr ${register} HKCU "${key}\\${hive}\\${section}"`,
    )
    assert.ok(!check.includes(WEBVIEW2_RUNTIME_GUID), '所有生产登记读取必须重定向到夹具')
    const clear = `SetRegView 32\nDeleteRegKey HKCU "${key}"\nSetRegView 64\nDeleteRegKey HKCU "${key}"\nSetRegView 32\n`
    const state = (hive, location) => `WriteRegStr HKCU "${key}\\${hive}\\ClientState" "EBWebView" "${location}"\n`
    const clients = (hive, view, version = '157.0.4341.42') => `SetRegView ${view}\nWriteRegStr HKCU "${key}\\${hive}\\Clients" "pv" "${version}"\nWriteRegStr HKCU "${key}\\${hive}\\Clients" "location" "${path.dirname(runtime)}"\nSetRegView 32\n`
    const cases = [
      ['user-state', state('HKCU', runtime), 1],
      ['machine-state', state('HKLM', runtime), 1],
      ['user-clients', clients('HKCU', 32), 1],
      ['machine-clients64', clients('HKLM', 64), 1],
      ['machine-clients32', clients('HKLM', 32), 1],
      ['uninstalled', clients('HKLM', 32, '0.0.0.0'), 0],
      ['missing-path', state('HKCU', runtime + '\\missing'), 0],
      ['empty-state', state('HKCU', ''), 0],
      ['absent', '', 0],
      ['missing-client-dll', `Delete "${runtime}\\EBWebView\\x64\\EmbeddedBrowserWebView.dll"\n` + state('HKLM', runtime), 0],
      ['missing-browser', `Delete "${runtime}\\msedgewebview2.exe"\n` + state('HKLM', runtime), 0],
    ]
    const probes = cases.map(([name, setup]) => `${clear}${setup}${check}\nFileOpen $0 "$EXEDIR\\${name}.txt" w\nFileWrite $0 "$2"\nFileClose $0\n`).join('')
    const exe = path.join(dir, 'probe.exe')
    const nsi = path.join(dir, 'probe.nsi')
    await writeFile(nsi, scriptSource(`Unicode true\n!include "LogicLib.nsh"\nName "WebView2 fixture"\nOutFile "${exe}"\nRequestExecutionLevel user\nSilentInstall silent\nSection\n${probes}${clear}SectionEnd\n`))
    const tool = stageMakensis(process.env.FISH_MAKENSIS || findTool('nsis-3.13/makensis.exe'))
    assert.equal(spawnSync(tool, ['/V2', nsi], { stdio: 'inherit', timeout: 20000 }).status, 0)
    assert.equal(spawnSync(exe, ['/S'], { stdio: 'inherit', timeout: 10000 }).status, 0)
    for (const [name, , expected] of cases) {
      assert.equal((await readFile(path.join(dir, `${name}.txt`), 'utf8')).trim(), String(expected), name)
    }
  } finally {
    assert.ok(path.resolve(dir).startsWith(path.resolve(os.tmpdir()) + path.sep) && path.basename(dir).startsWith('fish-webview-'))
    await rm(dir, { recursive: true, force: true })
  }
})
