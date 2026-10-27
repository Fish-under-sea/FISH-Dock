// 生成 DSH-FISH 的 Windows 安装包脚本（NSIS 3）：安装时把主程序、Node、固定版
// WebView2 与本地模型服务一次展开到安装目录，日常启动直接使用这些文件。
// 卸载只删除安装器自己放下的文件与快捷方式，绝不递归删目录、绝不碰用户数据
// （%LOCALAPPDATA%\DSH-FISH 里的账号、设置、用量与 WebView 配置）。
import fs from 'node:fs/promises'
import { existsSync } from 'node:fs'
import path from 'node:path'
import { spawnSync } from 'node:child_process'
import { createHash } from 'node:crypto'
import { fileURLToPath } from 'node:url'

export const PRODUCT = 'FISH Dock'
export const INSTALL_ID = 'DSH-FISH'
export const EXE_NAME = 'DSH-FISH.exe'
export const PUBLISHER = 'Fish-under-sea'
/// 程序启动必需的文件；与 src-tauri/src/runtime.rs 的 REQUIRED 保持一致。
export { REQUIRED } from './prepare-installer.mjs'
import { REQUIRED } from './prepare-installer.mjs'

/// 卸载时让程序自己收掉「开机自动启动」的计划任务。
/// 任务名带用户 SID，卸载器拼不出来；程序侧才知道名字，也能先确认任务指向的
/// 正是本安装目录里的这个 exe，才动手删（别的安装、别的程序的任务不碰）。
/// 与 src-tauri/src/main.rs 的 `--wakuwaku-uninstall-cleanup` 分支一致。
export const CLEANUP_FLAG = '--wakuwaku-uninstall-cleanup'

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')
const SAFE = /^[^/\\:*?"<>|]+(\/[^/\\:*?"<>|]+)*$/

function windowsPath(target) {
  return path.resolve(target).replace(/\//g, '\\')
}

/// 安装资源目录里的全部文件（含清单自身）：清单给出要校验的文件表，
/// 清单自己也要装、也要卸。清单之外的文件一律拒绝，安装目录与清单一一对应。
export async function resourceFiles(resources) {
  const manifestPath = path.join(resources, 'manifest.json')
  let manifest
  try {
    manifest = JSON.parse(await fs.readFile(manifestPath, 'utf8'))
  } catch (error) {
    throw new Error(`安装资源缺少可用的 manifest.json：${manifestPath}（${error.code ?? error.message}）`)
  }
  if (manifest.schema !== 2) throw new Error(`安装资源清单 schema 不是 2：${manifest.schema}`)
  const listed = (manifest.files ?? []).map(file => file.path)
  const missing = REQUIRED.filter(name => !listed.includes(name))
  if (missing.length) throw new Error(`安装资源清单缺少必需文件：${missing.join(', ')}`)
  const files = [...listed, 'manifest.json'].sort()
  for (const name of files) {
    if (!SAFE.test(name) || name.split('/').includes('..')) throw new Error(`安装资源里有不安全的相对路径：${name}`)
    const stat = await fs.stat(path.join(resources, name)).catch(() => undefined)
    if (!stat?.isFile()) throw new Error(`安装资源缺少文件：${name}`)
  }
  const onDisk = []
  async function walk(relative) {
    for (const entry of await fs.readdir(path.join(resources, relative), { withFileTypes: true })) {
      const name = relative ? `${relative}/${entry.name}` : entry.name
      if (entry.isDirectory()) await walk(name)
      else if (entry.isFile()) onDisk.push(name)
      else throw new Error(`安装资源里有非普通文件：${name}`)
    }
  }
  await walk('')
  const unlisted = onDisk.filter(name => !files.includes(name))
  if (unlisted.length) throw new Error(`安装资源里有清单之外的文件：${unlisted.slice(0, 5).join(', ')}`)
  return files
}

/// 资源目录里的全部子目录（相对路径，最深的在前）。
/// 不能只看清单里的文件路径：真实的 Node 发行包里有只有目录、没有文件的层级
/// （node/node_modules/npm/tap-snapshots/…），漏掉它们，卸载后会在安装目录里
/// 留下一串空壳文件夹。这里走真实目录树，跟安装时 `File /r` 放下的东西一一对应。
export async function resourceDirectories(resources) {
  const dirs = []
  async function walk(relative) {
    for (const entry of await fs.readdir(path.join(resources, relative), { withFileTypes: true })) {
      if (!entry.isDirectory()) continue
      const name = relative ? `${relative}/${entry.name}` : entry.name
      if (!SAFE.test(name) || name.split('/').includes('..')) throw new Error(`安装资源里有不安全的相对路径：${name}`)
      dirs.push(name)
      await walk(name)
    }
  }
  await walk('')
  return dirs.sort((a, b) => b.split('/').length - a.split('/').length || (a < b ? -1 : 1))
}

/// 清单里出现过的目录，最深的在前（卸载时先清空子目录，再清父目录）。
function directoriesOf(files) {
  const dirs = new Set()
  for (const file of files) {
    const parts = file.split('/')
    for (let i = 1; i < parts.length; i += 1) dirs.add(parts.slice(0, i).join('/'))
  }
  return [...dirs].sort((a, b) => b.split('/').length - a.split('/').length || (a < b ? -1 : 1))
}

/// 卸载要删的目录：真实目录树 ∪ 清单推导出的目录（后者兜底，防止目录清单缺失时漏删）。
function unmountDirectories(files, directories) {
  const set = new Set(directories ?? [])
  for (const name of directoriesOf(files)) set.add(name)
  return [...set].sort((a, b) => b.split('/').length - a.split('/').length || (a < b ? -1 : 1))
}

function quote(text) {
  return `"${text}"`
}

/// makensis 默认按系统代码页读脚本；脚本里有中文提示，所以落盘时带 UTF-8 BOM，
/// 让它按 UTF-8 解析（NSIS 3 见到 BOM 就切到 UTF-8）。
export function scriptSource(text) {
  return `\ufeff${text}`
}

/// 生成 NSIS 脚本。`outFile` 是安装包，`app` 是编译好的主程序，`resources` 是解好的资源目录。
export function installerScript({ version, app, resources, outFile, files, directories, product = PRODUCT, publisher = PUBLISHER, icon = path.join(root, 'src-tauri/icons/DSH-FISH-Windows.ico'), estimatedBytes = 0, payloadFiles = null, installManifest = path.join(path.dirname(resources), 'install-manifest.json') }) {
  if (!version) throw new Error('安装包需要产品版本')
  if (!files?.length) throw new Error('安装包需要资源文件清单（resourceFiles）')
  const fileName = path.basename(app)
  const uninstall = files.map(name => name.replace(/\//g, '\\'))
  const deleteLines = uninstall.map(name => `  Delete "$INSTDIR\\resources\\${name}"`).join('\n')
  const directoryLines = unmountDirectories(files, directories)
    .map(name => `  RMDir "$INSTDIR\\resources\\${name.replace(/\//g, '\\')}"`)
    .join('\n')
  const payloadLines = payloadFiles === null ? '  File /r "\${RESOURCES}\\*.*"' : payloadFiles.map(name => {
    if (!SAFE.test(name) || name.split('/').includes('..')) throw Error('精简载荷路径无效')
    const folder = path.posix.dirname(name)
    return '  SetOutPath "$StageDir\\resources' + (folder === '.' ? '' : '\\' + folder.replaceAll('/', '\\')) + '"\n  File "\${RESOURCES}\\' + name.replaceAll('/', '\\') + '"'
  }).join('\n')
  const kilobytes = Math.max(1, Math.round(estimatedBytes / 1024))
  return `; 由 scripts/installer.mjs 生成，请勿手改。
Unicode true
!include "MUI2.nsh"
!include "LogicLib.nsh"
!include "FileFunc.nsh"

!define PRODUCT ${quote(product)}
!define VERSION ${quote(version)}
!define PUBLISHER ${quote(publisher)}
!define APPFILE ${quote(windowsPath(app))}
!define RESOURCES ${quote(windowsPath(resources))}
!define OWNERSHIP ${quote(windowsPath(installManifest))}
!define UPDATER ${quote(windowsPath(path.join(root, 'scripts/install-update.cjs')))}
!define STOPPER ${quote(windowsPath(path.join(root, 'scripts/install-stop.ps1')))}
!define UNINSTALL_KEY "Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\${INSTALL_ID}"
!define RUN_KEY "Software\\Microsoft\\Windows\\CurrentVersion\\Run"

Name "\${PRODUCT}"
OutFile ${quote(windowsPath(outFile))}
InstallDir "$LOCALAPPDATA\\Programs\\\${PRODUCT}"
InstallDirRegKey HKCU "\${UNINSTALL_KEY}" "InstallLocation"
RequestExecutionLevel user
Var StageSeed
Var StageName
Var StageDir
Var NeedsWebviewAcl
SetCompressor /SOLID lzma
SetCompressorDictSize 32
ShowInstDetails show
ShowUninstDetails show
BrandingText "\${PRODUCT} \${VERSION}"

VIProductVersion "\${VERSION}.0"
VIAddVersionKey "ProductName" "\${PRODUCT}"
VIAddVersionKey "ProductVersion" "\${VERSION}"
; 少写 FileVersion 时 makensis 会对每个语言报 9100 警告，读文件属性的工具也拿不到版本号。
VIAddVersionKey "FileVersion" "\${VERSION}"
VIAddVersionKey "CompanyName" "\${PUBLISHER}"
VIAddVersionKey "FileDescription" "\${PRODUCT} 安装程序"
VIAddVersionKey "LegalCopyright" "\${PUBLISHER}"

!define MUI_ICON ${quote(windowsPath(icon))}
!define MUI_UNICON ${quote(windowsPath(icon))}
!define MUI_ABORTWARNING
!define MUI_FINISHPAGE_RUN "$INSTDIR\\${fileName}"
!define MUI_FINISHPAGE_RUN_TEXT "启动 \${PRODUCT}"
!define MUI_FINISHPAGE_RUN_NOTCHECKED
!insertmacro MUI_PAGE_WELCOME
!insertmacro MUI_PAGE_DIRECTORY
!insertmacro MUI_PAGE_INSTFILES
!insertmacro MUI_PAGE_FINISH
!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES
!insertmacro MUI_LANGUAGE "SimpChinese"
!insertmacro MUI_LANGUAGE "English"

Section "\${PRODUCT}" SecInstall
  SectionIn RO
  InitPluginsDir
  SetOutPath "$PLUGINSDIR\\bootstrap"
  File /oname=node.exe "\${RESOURCES}\\node\\node.exe"
  CreateDirectory "$INSTDIR"
  GetTempFileName $StageSeed "$INSTDIR"
  Delete "$StageSeed"
  \${GetFileName} "$StageSeed" $StageName
  StrCpy $StageDir "$INSTDIR\\.fish-dock-stage-$StageName"
  CreateDirectory "$StageDir"
  SetOutPath "$StageDir"
  File "\${APPFILE}"
  File /oname=install-manifest.json "\${OWNERSHIP}"
  File /oname=install-update.cjs "\${UPDATER}"
  File /oname=install-stop.ps1 "\${STOPPER}"
  SetOutPath "$StageDir\\resources"
${payloadLines}
  DetailPrint "校验发行文件并事务更新本安装..."
  nsExec::ExecToLog '"$PLUGINSDIR\\bootstrap\\node.exe" "$StageDir\\install-update.cjs" "$StageDir" "$INSTDIR"'
  Pop $0
  \${If} $0 != 0
    RMDir /r "$StageDir"
    MessageBox MB_OK|MB_ICONSTOP "更新未完成，旧版本已保留或恢复。请查看安装详情后重试。" /SD IDOK
    SetErrorLevel 1
    Abort
  \${EndIf}
  StrCpy $NeedsWebviewAcl 0
  IfFileExists "$StageDir\\webview-acl-needed" 0 +2
    StrCpy $NeedsWebviewAcl 1
  SetOutPath "$INSTDIR"
  RMDir /r "$StageDir"

  ; 固定版 WebView2 的沙箱主体只需读取运行时：安装时授权一次（仅这两个 SID、只读、仅本目录），
  ; 日常启动不再递归设置权限。这里失败不阻止安装，但首次启动会再试一次并把失败说清楚：
  ; 没有这份授权，WebView2 建不出界面。
  \${If} $NeedsWebviewAcl == 1
  DetailPrint "设置内置浏览器运行时权限..."
  nsExec::ExecToLog 'icacls "$INSTDIR\\resources\\webview2" /grant *S-1-15-2-1:(OI)(CI)(RX) *S-1-15-2-2:(OI)(CI)(RX) /T'
  Pop $0
  \${If} $0 != 0
    DetailPrint "内置浏览器运行时授权未成功（$0）：首次启动会再试一次并提示修复方式"
  \${EndIf}
  \${EndIf}

  CreateDirectory "$SMPROGRAMS\\\${PRODUCT}"
  CreateShortCut "$SMPROGRAMS\\\${PRODUCT}\\\${PRODUCT}.lnk" "$INSTDIR\\${fileName}" "" "$INSTDIR\\${fileName}" 0
  CreateShortCut "$DESKTOP\\\${PRODUCT}.lnk" "$INSTDIR\\${fileName}" "" "$INSTDIR\\${fileName}" 0
  CreateShortCut "$SMPROGRAMS\\\${PRODUCT}\\卸载 \${PRODUCT}.lnk" "$INSTDIR\\uninstall.exe"

  WriteUninstaller "$INSTDIR\\uninstall.exe"

  ; 卸载入口：Windows「设置 → 应用」里能看到，安装位置也记在这里。
  WriteRegStr HKCU "\${UNINSTALL_KEY}" "DisplayName" "\${PRODUCT}"
  WriteRegStr HKCU "\${UNINSTALL_KEY}" "DisplayVersion" "\${VERSION}"
  WriteRegStr HKCU "\${UNINSTALL_KEY}" "Publisher" "\${PUBLISHER}"
  WriteRegStr HKCU "\${UNINSTALL_KEY}" "DisplayIcon" "$INSTDIR\\${fileName}"
  WriteRegStr HKCU "\${UNINSTALL_KEY}" "InstallLocation" "$INSTDIR"
  WriteRegStr HKCU "\${UNINSTALL_KEY}" "UninstallString" '"$INSTDIR\\uninstall.exe"'
  WriteRegStr HKCU "\${UNINSTALL_KEY}" "QuietUninstallString" '"$INSTDIR\\uninstall.exe" /S'
  WriteRegDWORD HKCU "\${UNINSTALL_KEY}" "NoModify" 1
  WriteRegDWORD HKCU "\${UNINSTALL_KEY}" "NoRepair" 1
  WriteRegDWORD HKCU "\${UNINSTALL_KEY}" "EstimatedSize" ${kilobytes}
SectionEnd

Section "Uninstall"
  ; 只删安装器放下的东西：快捷方式、资源文件、主程序、卸载器。
  Delete "$SMPROGRAMS\\\${PRODUCT}\\\${PRODUCT}.lnk"
  Delete "$SMPROGRAMS\\\${PRODUCT}\\卸载 \${PRODUCT}.lnk"
  RMDir "$SMPROGRAMS\\\${PRODUCT}"
  Delete "$DESKTOP\\\${PRODUCT}.lnk"

  ; 指向本安装的「开机自动启动」值一并清掉，免得登录时去启动一个已删除的程序。
  ReadRegStr $0 HKCU "\${RUN_KEY}" "Wakuwaku"
  \${If} $0 == '"$INSTDIR\\${fileName}" --wakuwaku-at-login run'
    DeleteRegValue HKCU "\${RUN_KEY}" "Wakuwaku"
  \${EndIf}

  ; 计划任务的名字里带着用户 SID，卸载器拼不出来：让本安装的程序自己去比对
  ; 「Wakuwaku at logon-<SID>」指向的是不是这个安装目录里的 exe，是才删。
  ; 别的安装、别的程序的任务一律不碰。必须排在删除主程序之前——删了就问不了了。
  ; 清理失败也不中断卸载：那条任务由程序下次启动时按现状自愈。
  DetailPrint "清理本安装的「开机自动启动」任务..."
  nsExec::ExecToLog '"$INSTDIR\\${fileName}" ${CLEANUP_FLAG}'
  Pop $0
  \${If} $0 != 0
    DetailPrint "登录任务清理未成功（$0）：安装目录删除后它不会再启动任何程序"
  \${EndIf}

${deleteLines}
${directoryLines}
  RMDir "$INSTDIR\\resources"
  Delete "$INSTDIR\\${fileName}"
  Delete "$INSTDIR\\uninstall.exe"
  Delete "$INSTDIR\\install-manifest.json"
  Delete "$INSTDIR\\install-state.json"
  Delete "$INSTDIR\\install-update.cjs"
  Delete "$INSTDIR\\install-stop.ps1"
  RMDir "$INSTDIR"
  DeleteRegKey HKCU "\${UNINSTALL_KEY}"

  ; 账号、密钥、用量、设置与 WebView 用户数据在 %LOCALAPPDATA%\\DSH-FISH，不在安装目录里：
  ; 默认卸载不删除它们，重新安装后仍沿用原来的位置。
  DetailPrint "已保留用户数据：%LOCALAPPDATA%\\DSH-FISH"
SectionEnd
`
}

export async function writeOwnershipManifest(app, resources, files, output, version) {
  const inputs = [[path.basename(app), app], ...files.map(name => ['resources/' + name, path.join(resources, name)]),
    ['install-update.cjs', path.join(root, 'scripts/install-update.cjs')], ['install-stop.ps1', path.join(root, 'scripts/install-stop.ps1')]]
  const records = []
  for (const [name, source] of inputs) { const bytes = await fs.readFile(source); records.push({path:name, bytes:bytes.length, sha256:createHash('sha256').update(bytes).digest('hex')}) }
  const manifest = {schema:1, product:'FISH Dock', version, files:records}
  await fs.writeFile(output, JSON.stringify(manifest, null, 2))
  return manifest
}

/// 一次构建：读资源清单、生成 .nsi、调用 makensis。
export async function buildInstaller({ version, app, resources, outFile, makensis, product, publisher, baselineManifest }) {
  if (!makensis) throw new Error('需要 makensis（NSIS 3）的路径')
  const files = await resourceFiles(resources)
  const directories = await resourceDirectories(resources)
  const total = (await Promise.all(files.map(name => fs.stat(path.join(resources, name))))).reduce((n, s) => n + s.size, 0)
  const appStat = await fs.stat(app)
  const target = path.resolve(outFile)
  await fs.mkdir(path.dirname(target), { recursive: true })
  const installManifest = path.join(path.dirname(target), `install-manifest-${version}.json`)
  const ownership = await writeOwnershipManifest(app, resources, files, installManifest, version)
  let payloadFiles = null
  if (baselineManifest) {
    const baseline = JSON.parse(await fs.readFile(baselineManifest, 'utf8'))
    if (baseline.schema !== 1 || baseline.product !== 'FISH Dock' || !Array.isArray(baseline.files)) throw Error('精简更新的基础清单无效')
    const previous = new Map(baseline.files.map(f => [f.path, f]))
    ownership.payload = ownership.files.filter(f => { const old = previous.get(f.path); return !old || old.bytes !== f.bytes || old.sha256 !== f.sha256 }).map(f => f.path)
    payloadFiles = ownership.payload.filter(p => p.startsWith('resources/')).map(p => p.slice(10))
    await fs.writeFile(installManifest, JSON.stringify(ownership, null, 2))
  }
  const script = installerScript({
    version, app: path.resolve(app), resources: path.resolve(resources), outFile: target,
    files, directories, product, publisher, payloadFiles, estimatedBytes: total + appStat.size, installManifest,
  })
  await fs.mkdir(path.dirname(target), { recursive: true })
  const scriptPath = path.join(path.dirname(target), `${path.basename(target, '.exe')}.nsi`)
  await fs.writeFile(scriptPath, scriptSource(script))
  const result = spawnSync(makensis, ['/V2', scriptPath], { stdio: 'inherit' })
  if (result.error) throw result.error
  if (result.status !== 0) throw new Error(`makensis 失败（${result.status}）：脚本 ${scriptPath}`)
  const built = await fs.stat(target)
  return { outFile: target, scriptPath, bytes: built.size, files: files.length }
}

/// 离线工具（如 NSIS）放在仓库上层的 .tools：逐级向上找，兼容多一层产品目录的检出。
export function findTool(relative) {
  let directory = root
  for (;;) {
    const candidate = path.join(directory, '.tools', relative)
    if (existsSync(candidate)) return candidate
    const parent = path.dirname(directory)
    if (parent === directory) return path.join(root, '.tools', relative)
    directory = parent
  }
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const args = process.argv.slice(2)
  const option = name => args[args.indexOf(`--${name}`) + 1]
  const needed = ['app', 'resources', 'version', 'out'].filter(name => !args.includes(`--${name}`))
  if (needed.length) throw new Error(`缺少参数：${needed.map(n => `--${n}`).join(', ')}；用法：node scripts/installer.mjs --app <DSH-FISH.exe> --resources <resources> --version <version> --out <setup.exe> [--makensis <makensis.exe>]`)
  const makensis = option('makensis') ?? process.env.FISH_MAKENSIS ?? findTool('nsis-3.13/makensis.exe')
  const result = await buildInstaller({
    version: option('version'),
    app: path.resolve(option('app')),
    resources: path.resolve(option('resources')),
    outFile: path.resolve(option('out')),
    makensis,
    baselineManifest: args.includes('--baseline') ? path.resolve(option('baseline')) : undefined,
  })
  console.log(JSON.stringify(result, null, 2))
}
