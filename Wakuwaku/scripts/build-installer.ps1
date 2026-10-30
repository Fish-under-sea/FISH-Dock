<#
一次构建 DSH-FISH 安装版：准备安装资源 → 编译主程序（feature installed）→ 生成安装包。

安装包在安装时才把主程序、Node、固定版 WebView2 与 Windhawk 展开到用户选的目录；
日常启动直接使用安装目录里的这些文件，不解压、不整树哈希、不递归授权。

用法：
  & FISH本地模型工作台/Wakuwaku/scripts/build-installer.ps1
  & FISH本地模型工作台/Wakuwaku/scripts/build-installer.ps1 -Resources .tools/installer-resources

-Resources 指向已准备好的资源目录（含 manifest.json 与 node/webview2/windhawk/licenses）；
不给时用 -Downloads 里的官方下载件现场准备（需要 node 可执行文件）。
#>
param(
    [string]$Downloads = '',
    [string]$Resources = '',
    [string]$Output = '',
    [string]$Node = '',
    [string]$Cargo = '',
    [string]$Makensis = '',
    [switch]$ResolveToolsOnly
)
$ErrorActionPreference = 'Stop'
$app = Split-Path $PSScriptRoot -Parent
# 工具目录 .tools 与仓库之间可能隔有产品目录：向上找到真正含 .tools 的祖先。
$workspace = Split-Path (Split-Path $app -Parent) -Parent
$probe = Split-Path $app -Parent
while ($probe) {
    if (Test-Path -LiteralPath (Join-Path $probe '.tools') -PathType Container) { $workspace = $probe; break }
    $next = Split-Path $probe -Parent
    if (!$next -or $next -eq $probe) { break }
    $probe = $next
}
# —— 工具解析：可配优先（-参数 > 环境变量 > 已知 Medium IL 位置 > .tools 兜底）——
# 本机工作区根带 Low Mandatory Label，.tools 下的 node/cargo/makensis 会以 Low IL 运行，
# 写 Medium 的 dist/%TEMP% 会被拒（EPERM / os error 5）。所以先找可用的 Medium IL 工具，
# 只有都找不到才回落 .tools；回落到的 .tools 若是 Low，直接报错说清下一步，不跑到 EPERM 才炸。
# 详见 build\ship\构建环境-坑与根因.md。
function Test-LowIl([string]$Path) {
    if (!(Test-Path -LiteralPath $Path -PathType Leaf)) { return $false }
    $acl = (& icacls $Path 2>$null) -join "`n"
    return [bool]($acl -match 'Mandatory Label.*Low' -or $acl -match '强制.*低')
}
function Resolve-Tool([string]$Given, [string[]]$EnvNames, [string[]]$Known, [string]$Fallback, [string]$Label) {
    if ($Given) {
        if (Test-Path -LiteralPath $Given -PathType Leaf) { return (Resolve-Path -LiteralPath $Given).Path }
        throw "指定的 $Label 不存在：$Given"
    }
    foreach ($n in $EnvNames) { $v = [Environment]::GetEnvironmentVariable($n); if ($v -and (Test-Path -LiteralPath $v -PathType Leaf)) { return $v } }
    foreach ($k in $Known) { if ($k -and (Test-Path -LiteralPath $k -PathType Leaf)) { return $k } }
    if ($Fallback -and (Test-Path -LiteralPath $Fallback -PathType Leaf)) { return $Fallback }
    return $null
}
$nodeKnown = @('C:\Program Files\nodejs\node.exe')
$cargoKnown = @('D:\Fish-code\Codex\.toolchains\cargo\bin\cargo.exe')
$makensisKnown = @((Join-Path $env:LOCALAPPDATA 'FishDockShip/nsis-3.13/makensis.exe'))
$Node = Resolve-Tool $Node @('FISH_DOCK_NODE','FISH_NODE') $nodeKnown (Join-Path $workspace '.tools/offline-downloads/node/node-v24.14.0-win-x64/node.exe') 'Node'
$Cargo = Resolve-Tool $Cargo @('FISH_DOCK_CARGO','FISH_CARGO') $cargoKnown (Join-Path $workspace '.tools/cargo/bin/cargo.exe') 'Cargo'
$Makensis = Resolve-Tool $Makensis @('FISH_DOCK_MAKENSIS','FISH_MAKENSIS') $makensisKnown (Join-Path $workspace '.tools/nsis-3.13/makensis.exe') 'Makensis'
foreach ($pair in @(@($Node,'Node'), @($Cargo,'Cargo'), @($Makensis,'Makensis'))) {
    if ($pair[0] -and (Test-LowIl $pair[0])) {
        throw "$($pair[1]) 解析到 Low IL 工具（$($pair[0])）：子进程会以 Low 运行，写 Medium 的 dist/%TEMP% 会被拒（EPERM/os error 5）。请用 Medium IL 工具：设环境变量 FISH_DOCK_$($pair[1].ToUpper())，或显式 -$($pair[1])，或按 build\ship\构建环境-坑与根因.md 修 ACL。"
    }
}
if (!$Node) { throw '找不到可用的 node.exe：设 FISH_DOCK_NODE，或把 Node 装到 C:\Program Files\nodejs（见 build\ship\构建环境-坑与根因.md）' }
if (!$Cargo) { throw '找不到可用的 cargo.exe：设 FISH_DOCK_CARGO，或装 Rust 工具链（见 build\ship\构建环境-坑与根因.md）' }
if (!$Makensis) { throw '找不到可用的 makensis.exe：设 FISH_DOCK_MAKENSIS，或把 NSIS 复制到 %LOCALAPPDATA%\FishDockShip\nsis-3.13（见 build\ship\构建环境-坑与根因.md）' }
if (!$Downloads) { $Downloads = Join-Path $workspace '.tools/offline-downloads' }
if (!$Resources) { $Resources = Join-Path $workspace '.tools/installer-resources' }
if ($ResolveToolsOnly) {
    Write-Host "Workspace=$workspace"
    foreach ($pair in @(@($Node,'Node'), @($Cargo,'Cargo'), @($Makensis,'Makensis'))) {
        $il = if (Test-LowIl $pair[0]) { 'Low（不可用）' } else { 'Medium/默认' }
        Write-Host ("{0}={1}  IL={2}" -f $pair[1], $pair[0], $il)
    }
    exit 0
}

$version = (Get-Content -LiteralPath (Join-Path $app 'src-tauri/tauri.conf.json') -Raw | ConvertFrom-Json).version
if (!$Output) { $Output = Join-Path (Split-Path $app -Parent) "dist/installer/FISH-Dock-Setup-$version.exe" }
$Output = $ExecutionContext.SessionState.Path.GetUnresolvedProviderPathFromPSPath($Output)

# 1. 安装资源：只从锁定的官方下载件解包一次，产出 <Resources>/resources + 清单（schema 2）。
$resDir = Join-Path $Resources 'resources'
if (!(Test-Path -LiteralPath (Join-Path $resDir 'manifest.json') -PathType Leaf)) {
    if (Test-Path -LiteralPath $Resources) { throw "资源目录里没有 resources/manifest.json，请换一个 -Resources 路径或先清理：$Resources" }
    Write-Host "准备安装资源 → $Resources"
    & $Node (Join-Path $PSScriptRoot 'prepare-installer.mjs') --downloads $Downloads --out $Resources --version $version
    if ($LASTEXITCODE -ne 0) { throw "准备安装资源失败：$LASTEXITCODE" }
}

# 2. 主程序：feature installed 下不再内嵌运行时归档。
Write-Host "编译主程序（feature installed）"
# 本机工具链的 LTO 曾异常退出，沿用已验证的替代：关闭 LTO、16 个代码生成单元。
$env:CARGO_PROFILE_RELEASE_LTO = 'false'
$env:CARGO_PROFILE_RELEASE_CODEGEN_UNITS = '16'
# 本机 cargo 的 TLS 栈取不到凭据（SEC_E_NO_CREDENTIALS），crates.io 连不上；
# FISH_CARGO_VENDOR 指向一个本地 vendor 目录时改为纯离线解析，不再碰网络。
$vendorArgs = @()
if ($env:FISH_CARGO_VENDOR) {
    $vendorArgs = @('--config', 'source.crates-io.replace-with="vendored"',
                    '--config', "source.vendored.directory=`"$($env:FISH_CARGO_VENDOR -replace '\\','/')`"")
}
& $Cargo build --locked --offline --release --features installed @vendorArgs --manifest-path (Join-Path $app 'src-tauri/Cargo.toml')
if ($LASTEXITCODE -ne 0) { throw "主程序编译失败：$LASTEXITCODE" }

$stage = Join-Path (Split-Path $Output -Parent) 'stage'
New-Item -ItemType Directory -Force -Path $stage | Out-Null
$target = if ($env:CARGO_TARGET_DIR) { $env:CARGO_TARGET_DIR } else { Join-Path $app 'src-tauri/target' }
$binary = Join-Path $target 'release/FISH-Dock.exe'
# #27 P2 / #t16：Cargo 包名是 fish-dock、二进制名是 FISH-Dock.exe（不是旧的
# wakuwaku.exe）；安装后的主程序名同样是 FISH-Dock.exe（旧名不再产出）。
Copy-Item -LiteralPath $binary -Destination (Join-Path $stage 'FISH-Dock.exe') -Force

# 3. 安装包：NSIS 3，装到用户选的目录，建快捷方式与卸载入口。
Write-Host "生成安装包 → $Output"
& $Node (Join-Path $PSScriptRoot 'installer.mjs') --app (Join-Path $stage 'FISH-Dock.exe') --resources $resDir --version $version --out $Output --makensis $Makensis
if ($LASTEXITCODE -ne 0) { throw "生成安装包失败：$LASTEXITCODE" }

Get-Item -LiteralPath $Output | Select-Object FullName, Length, @{Name = 'ProductVersion'; Expression = { $_.VersionInfo.ProductVersion } }
Get-FileHash -LiteralPath $Output -Algorithm SHA256
