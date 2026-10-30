// 生成 FISH Dock 的 Windows 安装包脚本（NSIS 3）：安装时把主程序、Node、固定版
// WebView2 与本地模型服务一次展开到安装目录，日常启动直接使用这些文件。
// 卸载只删除安装器自己放下的文件与快捷方式，绝不递归删目录、绝不碰用户数据
// （%LOCALAPPDATA%\FISH Dock 里的账号、设置、用量与 WebView 配置）。
import fs from 'node:fs/promises'
import { copyFileSync, existsSync, mkdirSync, readdirSync } from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import { spawnSync } from 'node:child_process'
import { createHash } from 'node:crypto'
import { fileURLToPath } from 'node:url'

export const PRODUCT = 'FISH Dock'
export const INSTALL_ID = 'FISH Dock'
export const EXE_NAME = 'FISH-Dock.exe'
export const PUBLISHER = 'Fish-under-sea'
/// 程序启动必需的文件；与 src-tauri/src/runtime.rs 的 REQUIRED 保持一致。
export { REQUIRED } from './prepare-installer.mjs'
import { REQUIRED } from './prepare-installer.mjs'

// ── #27 旧名清理：2.0.0 起用户看得见的名字全换成 FISH Dock ────────────────
// issue #27 的 P1/P2。改名不能只管新装：机器上还装着旧版本，升级时得把旧名
// 留下的登记、文件、目录按「属于我们自己」逐条迁过来或清掉，否则应用列表里会
// 出现两个、自启动会看起来失效、安装目录里会留两个 exe。
//
// 【2.0.0 的名字】
//   INSTALL_ID = 'FISH Dock' → 卸载表登记名 Uninstall\FISH Dock；
//   EXE_NAME = 'FISH-Dock.exe' → 主程序名（P2）；
//   RUN_NAME = 'FISH Dock'、AT_LOGIN_ARGS = '--fish-dock-at-login run'
//     → 开机自动启动的 Run 值名与参数（P0，connection.rs 写、安装器迁移）；
//   CLEANUP_FLAG = '--fish-dock-uninstall-cleanup'（main.rs 同时认旧值）。
//
// 【仍然沿用的内部身份】应用标识 com.fish.dshfish（tauri.conf.json）、
//   Cargo 包名 wakuwaku、licenses/Wakuwaku.txt（上游许可原文的文件名）。
//
// 【迁移 / 清理】
//   LEGACY_INSTALL_IDS —— 旧卸载表登记项（DSH-FISH / Wakuwaku）：升级时把
//     InstallLocation 搬过来当安装目录，再把旧键删掉，应用列表里只剩一项；
//   LEGACY_EXE_NAMES —— 旧主程序（DSH-FISH.exe / 0.x 的 wakuwaku.exe）：
//     删旧建新，升级后安装目录里只有一个 exe（用户已确认「旧 exe 不留」）；
//   LEGACY_RUN_NAME —— 旧 Run 值名 Wakuwaku：只把「指向本安装目录」的那一项
//     迁到新名下（指向别的程序的原样保留并提示）；
//   LEGACY_HOME_DIRECTORY → HOME_DIRECTORY（#27-D 用户数据，见下）。
export const RUN_NAME = 'FISH Dock'
export const RUN_KEY = 'Software\\Microsoft\\Windows\\CurrentVersion\\Run'
export const APPROVED_KEY = 'Software\\Microsoft\\Windows\\CurrentVersion\\Explorer\\StartupApproved\\Run'
export const AT_LOGIN_ARGS = '--fish-dock-at-login run'
/// 1.x 写进 Run 值的旧参数（#27 P0）：认出来才能迁走或清掉。
export const LEGACY_AT_LOGIN_ARGS = '--wakuwaku-at-login run'
export const LEGACY_RUN_NAME = 'Wakuwaku'
export const LEGACY_EXE_NAMES = ['DSH-FISH.exe', 'wakuwaku.exe', 'wakuwaku-sensors.exe']
export const LEGACY_INSTALL_IDS = ['DSH-FISH', 'Wakuwaku', 'FISH Dock']
/// 旧卸载表登记项里出现过的 DisplayName；只有它对上，或 InstallLocation 就是本
/// 安装目录，才认定那条登记属于本产品，别的程序的同名登记一律不碰。
export const LEGACY_DISPLAY_NAMES = ['Wakuwaku', 'FISH Dock', 'DSH-FISH']
/// #27-D：%LOCALAPPDATA% 下的用户数据目录，旧名与新名。
/// HOME_DIRECTORY 的字面量必须与 src-tauri/src/runtime.rs 的 app_home()、
/// scripts/install-update.cjs 的 HOME_TO 完全一致（installer.test.mjs 有断言盯着）。
/// 迁移动作在 install-update.cjs 里做（那边才有真实 fs），安装器只负责传 --migrate-home。
export const LEGACY_HOME_DIRECTORY = 'DSH-FISH'
export const HOME_DIRECTORY = 'FISH Dock'
/// 更早的遗留：%LOCALAPPDATA%\FISH\worker.json（issue #27 第 3 节）。
/// 新数据根就绪之后由 install-update.cjs 删掉，安装器只传开关。
export const LEGACY_WORKER_DIRECTORY = 'FISH'
export const LEGACY_WORKER_FILE = 'worker.json'
/// 交给 install-update.cjs 的迁移开关：不带它，谁都不会动用户数据目录。
export const MIGRATE_HOME_FLAG = '--migrate-home'

/// 卸载时让程序自己收掉「开机自动启动」的计划任务。
/// 任务名带用户 SID，卸载器拼不出来；程序侧才知道名字，也能先确认任务指向的
/// 正是本安装目录里的这个 exe，才动手删（别的安装、别的程序的任务不碰）。
/// 与 src-tauri/src/main.rs 的 `--fish-dock-uninstall-cleanup` 分支一致（旧值也认）。
export const CLEANUP_FLAG = '--fish-dock-uninstall-cleanup'

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
export function installerScript({ version, app, resources, outFile, files, directories, product = PRODUCT, publisher = PUBLISHER, icon = path.join(root, 'src-tauri/icons/FISH-Dock-Windows.ico'), estimatedBytes = 0, payloadFiles = null, installManifest = path.join(path.dirname(resources), 'install-manifest.json') }) {
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
  // ── #27 清理档要生成的 NSIS 行 ──────────────────────────────────────────
  // 旧卸载表登记项：一个旧名一条 !define，清理时逐条按归属判定。
  const legacyKeyDefines = LEGACY_INSTALL_IDS
    .map((id, index) => `!define LEGACY_UNINSTALL_${index + 1} "Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\${id}"`)
    .join('\n')
  // 旧主程序对应的 Run 值文本：只认「本安装目录 + 我们自己的旧文件名」这几种写法，
  // 精确匹配，绝不做子串搜索 —— 免得删掉别的程序写的同名值。旧版本写的是旧开关名，
  // 也兼容旧 exe 配新开关名这种手工改过的写法。
  const legacyRunForms = LEGACY_EXE_NAMES.flatMap(name => [
    `"$INSTDIR\\${name}"`,
    `"$INSTDIR\\${name}" ${LEGACY_AT_LOGIN_ARGS}`,
    `"$INSTDIR\\${name}" ${AT_LOGIN_ARGS}`,
  ])
  const legacyRunLines = legacyRunForms.map((form, index) => `  ${index ? '${OrIf}' : '${If}'} $1 == '${form}'`).join('\n')
  const legacyRunLines2 = legacyRunForms.map((form, index) => `  ${index ? '${OrIf}' : '${If}'} $2 == '${form}'`).join('\n')
  const legacyKeyLines = LEGACY_INSTALL_IDS.map((id, index) => [
    `  ; 旧卸载表登记项「${id}」：只有它登记的安装位置就是本目录，或显示名是我们自己`,
    '  ; 用过的旧名，才认定属于本产品；别的程序、别的目录的同名登记一概不动。',
    `  ReadRegStr $1 HKCU "\${LEGACY_UNINSTALL_${index + 1}}" "InstallLocation"`,
    `  ReadRegStr $2 HKCU "\${LEGACY_UNINSTALL_${index + 1}}" "DisplayName"`,
    '  ${If} $1 == "$INSTDIR"',
    ...LEGACY_DISPLAY_NAMES.map(name => `  \${OrIf} $2 == '${name}'`),
    `    DeleteRegKey HKCU "\${LEGACY_UNINSTALL_${index + 1}}"`,
    '  ${EndIf}',
  ].join('\n')).join('\n')
  // 卸载段：Run 值指向本安装目录里的哪一个 exe 都算我们的（当前名 + 旧名）。
  const ownedRunForms = [`"$INSTDIR\\${fileName}" ${AT_LOGIN_ARGS}`, ...legacyRunForms]
  const ownedRunLines = ownedRunForms.map((form, index) => `  ${index ? '${OrIf}' : '${If}'} $0 == '${form}'`).join('\n')
  const legacyExeDeleteLines = LEGACY_EXE_NAMES.map(name => `  Delete "$INSTDIR\\${name}"`).join('\n')
  // #t16：旧名字的快捷方式。1.x 的「Wakuwaku」「DSH-FISH」在开始菜单与桌面上各留过
  // 一份，改名后它们仍指向刚被删掉的旧 exe（点开什么也不发生）。按旧显示名精确删，
  // 删完顺手收掉空文件夹；不是我们名字的快捷方式一概不碰。
  const legacyShortcutLines = LEGACY_DISPLAY_NAMES.map(name => [
    `  Delete "$SMPROGRAMS\\${name}\\${name}.lnk"`,
    `  Delete "$SMPROGRAMS\\${name}\\卸载 ${name}.lnk"`,
    `  Delete "$SMPROGRAMS\\${name}.lnk"`,
    `  RMDir "$SMPROGRAMS\\${name}"`,
    `  Delete "$DESKTOP\\${name}.lnk"`,
  ].join('\n')).join('\n')
  // 升级时沿用旧卸载表登记的安装位置：新登记名（FISH Dock）在第一次用新版装过之前
  // 并不存在，InstallDirRegKey 读不到，会装出第二份。这里补读旧键：依次取第一个
  // 非空的 InstallLocation。
  const legacyInstallDirLines = [
    `  ReadRegStr $1 HKCU "\${LEGACY_UNINSTALL_1}" "InstallLocation"`,
    ...LEGACY_INSTALL_IDS.slice(1).map((id, index) => `  \${If} $1 == ""\n    ReadRegStr $1 HKCU "\${LEGACY_UNINSTALL_${index + 2}}" "InstallLocation"\n  \${EndIf}`),
  ].join('\n')
  const runMigrationLines = legacyRunForms.map((form, index) => `      ${index ? '${OrIf}' : '${If}'} $1 == '${form}'`).join('\n')
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
!define RUN_KEY "${RUN_KEY}"
!define APPROVED_KEY "${APPROVED_KEY}"
!define RUN_NAME "${RUN_NAME}"
!define LEGACY_RUN_NAME "${LEGACY_RUN_NAME}"
${legacyKeyDefines}

Name "\${PRODUCT}"
OutFile ${quote(windowsPath(outFile))}
InstallDir "$LOCALAPPDATA\\Programs\\\${PRODUCT}"
InstallDirRegKey HKCU "\${UNINSTALL_KEY}" "InstallLocation"
RequestExecutionLevel user
Var StageSeed
Var StageName
Var StageDir
Var NeedsWebviewAcl
Var HomeMigrationFailed
SetCompressor /SOLID lzma
SetCompressorDictSize 32
ShowInstDetails show
ShowUninstDetails show
BrandingText "\${PRODUCT} \${VERSION}"

; #27 P1：升级时沿用旧卸载表登记里的安装位置。旧的登记名（Uninstall\DSH-FISH /
; Uninstall\Wakuwaku）才记得用户上次装在哪；新名在第一次用新版安装过之前并不存在，
; InstallDirRegKey 读不到，会装出第二份。
Function .onInit
  ReadRegStr $1 HKCU "\${UNINSTALL_KEY}" "InstallLocation"
  \${If} $1 == ""
${legacyInstallDirLines}
    \${If} $1 != ""
    \${AndIf} \${FileExists} "$1\\*.*"
      StrCpy $INSTDIR $1
    \${EndIf}
  \${EndIf}
FunctionEnd

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
  ; #27-D：带上 ${MIGRATE_HOME_FLAG}，install-update.cjs 才会在文件事务提交之后
  ; 迁移用户数据目录（%LOCALAPPDATA%\\${LEGACY_HOME_DIRECTORY} → %LOCALAPPDATA%\\${HOME_DIRECTORY}）：
  ; 复制 → 逐个核对摘要 → 两份清单对齐 → 才删旧目录。不带这个参数它一个目录都不碰。
  nsExec::ExecToLog '"$PLUGINSDIR\\bootstrap\\node.exe" "$StageDir\\install-update.cjs" "$StageDir" "$INSTDIR" ${MIGRATE_HOME_FLAG}'
  Pop $0
  ; 两个标记都要在暂存目录被删掉之前读出来：更新失败那条分支同样会把暂存目录整份删掉，
  ; 读晚了（或者等失败分支走完再读）就什么也读不到。
  StrCpy $NeedsWebviewAcl 0
  IfFileExists "$StageDir\\webview-acl-needed" 0 +2
    StrCpy $NeedsWebviewAcl 1
  StrCpy $HomeMigrationFailed 0
  IfFileExists "$StageDir\\home-migration-failed" 0 +2
    StrCpy $HomeMigrationFailed 1
  \${If} $0 != 0
    RMDir /r "$StageDir"
    MessageBox MB_OK|MB_ICONSTOP "更新未完成，旧版本已保留或恢复。请查看安装详情后重试。" /SD IDOK
    SetErrorLevel 1
    Abort
  \${EndIf}
  SetOutPath "$INSTDIR"
  RMDir /r "$StageDir"

  ; 用户数据迁移没做成，不回滚已经装好的程序：旧目录一个字节都没动，账号与设置都还在。
  ; 只把话说清楚 —— 完全退出程序后重跑一次安装程序就能补上。
  \${If} $HomeMigrationFailed == 1
    DetailPrint "用户数据迁移未完成：旧目录已原样保留，细节见安装目录里的 install.log"
    MessageBox MB_OK|MB_ICONEXCLAMATION "用户数据没能迁到新的目录，旧目录已原样保留（账号、设置与 WebView 数据都还在）。请从托盘完全退出 \${PRODUCT}，再重新运行一次安装程序。" /SD IDOK
  \${EndIf}

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

  ; ── #27 清理档：旧发行名留下的东西 ─────────────────────────────────────
  ; 新版本已经事务提交，旧 exe 一律不留（删旧建新）。
  DetailPrint "清理旧版本留下的文件与登记..."
${legacyExeDeleteLines}
${legacyShortcutLines}

  ; 指向那个旧 exe 的「开机自动启动」值已经启动不了任何东西：连它在任务管理器
  ; 「启动应用」里的同名标记一起清掉。只按精确值匹配本安装目录里的旧主程序，
  ; 别的安装、别的程序写的值一概不动；指向当前主程序的那条值是用户的选择，保留。
  ReadRegStr $1 HKCU "\${RUN_KEY}" "\${RUN_NAME}"
${legacyRunLines}
    DeleteRegValue HKCU "\${RUN_KEY}" "\${RUN_NAME}"
    DeleteRegValue HKCU "\${APPROVED_KEY}" "\${RUN_NAME}"
  \${EndIf}

  ; #27 P0：旧发行名 Wakuwaku 的「开机自动启动」项。只有它指向本安装目录里的旧
  ; 主程序时才搬到新名下（连 StartupApproved 的旧标记一起清，否则新名会被它
  ; “看起来关着”）；指向别的程序（真的 Wakuwaku）原样保留，只在这里说一句。
  ReadRegStr $1 HKCU "\${RUN_KEY}" "\${RUN_NAME}"
  \${If} $1 == ""
    ReadRegStr $1 HKCU "\${RUN_KEY}" "\${LEGACY_RUN_NAME}"
${runMigrationLines}
      WriteRegStr HKCU "\${RUN_KEY}" "\${RUN_NAME}" '"$INSTDIR\\${fileName}" ${AT_LOGIN_ARGS}'
      DeleteRegValue HKCU "\${APPROVED_KEY}" "\${LEGACY_RUN_NAME}"
      DeleteRegValue HKCU "\${RUN_KEY}" "\${LEGACY_RUN_NAME}"
      DetailPrint "开机自动启动已从旧名字 Wakuwaku 迁到 \${RUN_NAME}"
    \${ElseIf} $1 != ""
      DetailPrint "旧的 Wakuwaku 自启动项指向别的程序，已原样保留"
    \${EndIf}
  \${EndIf}

${legacyKeyLines}

  ; 卸载表里那条带旧发行名的 ProductName（0.x 写过）：2.0.0 起只用 DisplayName +
  ; DisplayVersion，旧值留着会在「设置 → 应用」里显示成两个名字。
  DeleteRegValue HKCU "\${UNINSTALL_KEY}" "ProductName"

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
  ; #27 P0：值名是新的 ${RUN_NAME}（connection.rs 写的），同时兜旧名
  ; ${LEGACY_RUN_NAME}（1.x 写的），匹配的写法覆盖当前主程序名与旧主程序名；
  ; 连任务管理器「启动应用」里的同名标记一起删，不留一条指不到程序的记录。
  ; 别的安装、别的程序写的同名值一概不碰（精确匹配整个值文本）。
  ReadRegStr $0 HKCU "\${RUN_KEY}" "\${RUN_NAME}"
${ownedRunLines}
    DeleteRegValue HKCU "\${RUN_KEY}" "\${RUN_NAME}"
    DeleteRegValue HKCU "\${APPROVED_KEY}" "\${RUN_NAME}"
  \${EndIf}

  ReadRegStr $2 HKCU "\${RUN_KEY}" "\${LEGACY_RUN_NAME}"
${legacyRunLines2}
    DeleteRegValue HKCU "\${RUN_KEY}" "\${LEGACY_RUN_NAME}"
    DeleteRegValue HKCU "\${APPROVED_KEY}" "\${LEGACY_RUN_NAME}"
  \${EndIf}

  ; 计划任务的名字里带着用户 SID，卸载器拼不出来：让本安装的程序自己去比对
  ; 「FISH Dock at logon-<SID>」与 1.x 的「Wakuwaku at logon-<SID>」指向的是不是
  ; 这个安装目录里的 exe，是才删。别的安装、别的程序的任务一律不碰。
  ; 必须排在删除主程序之前——删了就问不了了。
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
  ; #27 清理档：0.x 的旧主程序。正常安装时已经删过一次，这里是兜底 —— 卸载后
  ; 安装目录里不该留下任何一个能启动的主程序（不留旧 exe）。
${legacyExeDeleteLines}
  Delete "$INSTDIR\\uninstall.exe"
  Delete "$INSTDIR\\install-manifest.json"
  Delete "$INSTDIR\\install-state.json"
  Delete "$INSTDIR\\install-update.cjs"
  Delete "$INSTDIR\\install-stop.ps1"
  ; 更新器写在安装目录里的详情日志（install-update.cjs，UTF-8）：不留下一份。
  Delete "$INSTDIR\\install.log"
  RMDir "$INSTDIR"
  DeleteRegKey HKCU "\${UNINSTALL_KEY}"
  ; 卸载表里的旧登记项：安装时按归属清过一遍，卸载再兜一次底。
${legacyKeyLines}

  ; 账号、密钥、用量、设置与 WebView 用户数据在 %LOCALAPPDATA%\\${HOME_DIRECTORY}，不在安装目录里：
  ; 默认卸载不删除它们，重新安装后仍沿用原来的位置。
  ; #27-D 之后旧目录 %LOCALAPPDATA%\\${LEGACY_HOME_DIRECTORY} 在安装时已迁移并删除，这里不再提它。
  DetailPrint "已保留用户数据：%LOCALAPPDATA%\\${HOME_DIRECTORY}"
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
  makensis = stageMakensis(makensis)
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

/// 找离线工具（如 NSIS）。**可配优先**：FISH_DOCK_TOOL_DIR（工具根）> 已知 Medium IL 位置
/// > 逐级向上的 .tools 兜底。本机 .tools 带 Low Mandatory Label，其可执行体会以 Low IL 运行、
/// 写 Medium 的 dist 被拒（EPERM）；所以已知可用位置优先，避免静默选中 .tools 跑到 EPERM 才炸。
/// 详见 build\ship\构建环境-坑与根因.md。
export function findTool(relative) {
  const envDir = process.env.FISH_DOCK_TOOL_DIR
  if (envDir) {
    const candidate = path.join(envDir, relative)
    if (existsSync(candidate)) return candidate
  }
  if (/makensis/i.test(relative)) {
    const known = path.join(process.env.LOCALAPPDATA || '', 'FishDockShip', 'nsis-3.13', 'makensis.exe')
    if (process.env.LOCALAPPDATA && existsSync(known)) return known
  }
  let directory = root
  for (;;) {
    const candidate = path.join(directory, '.tools', relative)
    if (existsSync(candidate)) return candidate
    const parent = path.dirname(directory)
    if (parent === directory) return path.join(root, '.tools', relative)
    directory = parent
  }
}

/// 只要 makensis 自己躺在含非 ASCII 字符的路径里，它最后一步写出安装包时必然失败
/// （`Can't open output file`，退出码 1）—— 实测：同一份 NSIS 3.13 放在
/// `D:\tools\nsis-3.13` 能编出包，放到含中文的目录里一个字节都写不出来；把脚本与
/// 输出都挪到纯 ASCII 目录也不管用，唯一变量是 makensis 自身所在路径。
/// 本仓可以落在中文目录（`.tools` 就在仓上层），所以这里把整棵 NSIS 工具树
/// （约 7 MB：Stubs / Include / Plugins / nsisconf.nsh）暂存到纯 ASCII 的临时目录再用。
/// 工具本来就在 ASCII 路径上时原样返回，不做任何复制；生成脚本与断言都不受影响。
const ASCII_PATH = /^[\x20-\x7e]+$/

export function stageMakensis(tool, temporary = os.tmpdir()) {
  if (!tool) throw new Error('需要 makensis（NSIS 3）的路径')
  if (ASCII_PATH.test(tool) && ASCII_PATH.test(temporary)) return tool
  const parent = path.dirname(tool)
  // 工具树的根（Stubs / Include / Plugins / nsisconf.nsh 所在）：给的是 Bin\makensis.exe
  // 就上两级，给的是根目录下的启动器就本目录。暂存必须整棵搬，只搬 exe 会因为找不到
  // stub 文件而编不出安装包。
  const source = path.basename(parent).toLowerCase() === 'bin' ? path.dirname(parent) : parent
  const staged = path.join(temporary, `fish-dock-nsis-${createHash('sha1').update(source).digest('hex').slice(0, 12)}`, path.basename(source))
  const stagedTool = path.join(staged, path.relative(source, tool))
  if (!ASCII_PATH.test(stagedTool)) return tool
  if (!existsSync(stagedTool)) copyTree(source, staged)
  return stagedTool
}

/// 逐层复制一棵目录树。不用 fs.cpSync：源路径带中文时它会直接把进程撞崩
/// （Windows 上实测 STATUS_STACK_BUFFER_OVERRUN），逐层 readdir + copyFile 稳。
function copyTree(from, to) {
  mkdirSync(to, { recursive: true })
  for (const entry of readdirSync(from, { withFileTypes: true })) {
    const source = path.join(from, entry.name), target = path.join(to, entry.name)
    if (entry.isDirectory()) copyTree(source, target)
    else if (entry.isFile()) copyFileSync(source, target)
  }
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const args = process.argv.slice(2)
  const option = name => args[args.indexOf(`--${name}`) + 1]
  const needed = ['app', 'resources', 'version', 'out'].filter(name => !args.includes(`--${name}`))
  if (needed.length) throw new Error(`缺少参数：${needed.map(n => `--${n}`).join(', ')}；用法：node scripts/installer.mjs --app <FISH-Dock.exe> --resources <resources> --version <version> --out <setup.exe> [--makensis <makensis.exe>]`)
  const makensisTool = option('makensis') ?? process.env.FISH_MAKENSIS ?? findTool('nsis-3.13/makensis.exe')
  if (!existsSync(makensisTool)) throw new Error(`找不到 makensis：${makensisTool}；设 FISH_MAKENSIS 或把 NSIS 复制到 %LOCALAPPDATA%\\FishDockShip\\nsis-3.13（不要用 .tools 的 Low IL 副本，见 build\\ship\\构建环境-坑与根因.md）`)
  const makensis = stageMakensis(makensisTool)
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
