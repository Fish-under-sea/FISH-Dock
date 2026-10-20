<#
一次构建 DSH-FISH 安装版：准备安装资源 → 编译主程序（feature installed）→ 生成安装包。

安装包在安装时才把主程序、Node、固定版 WebView2 与本地模型服务展开到用户选的目录；
日常启动直接使用安装目录里的这些文件，不解压、不整树哈希、不递归授权。

用法：
  & FISH本地模型工作台/Wakuwaku/scripts/build-installer.ps1
  & FISH本地模型工作台/Wakuwaku/scripts/build-installer.ps1 -Resources .tools/installer-resources

-Resources 指向已准备好的资源目录（含 manifest.json 与 node/webview2/ofm/licenses）；
不给时用 -Downloads 里的官方下载件现场准备（需要 node 可执行文件）。
#>
param(
    [string]$Downloads = '',
    [string]$Resources = '',
    [string]$Output = '',
    [string]$Node = '',
    [string]$Cargo = '',
    [string]$Makensis = ''
)
$ErrorActionPreference = 'Stop'
$app = Split-Path $PSScriptRoot -Parent
$workspace = Split-Path (Split-Path $app -Parent) -Parent
if (!$Node) { $Node = Join-Path $workspace '.tools/offline-downloads/node/node-v24.14.0-win-x64/node.exe' }
if (!(Test-Path -LiteralPath $Node -PathType Leaf)) { $Node = (Get-Command node -ErrorAction Stop).Source }
if (!$Cargo) { $Cargo = Join-Path $workspace '.tools/cargo/bin/cargo.exe' }
if (!(Test-Path -LiteralPath $Cargo -PathType Leaf)) { $Cargo = (Get-Command cargo -ErrorAction Stop).Source }
if (!$Makensis) { $Makensis = Join-Path $workspace '.tools/nsis-3.13/makensis.exe' }
if (!(Test-Path -LiteralPath $Makensis -PathType Leaf)) { throw "缺少离线 NSIS（makensis）：$Makensis" }
if (!$Downloads) { $Downloads = Join-Path $workspace '.tools/offline-downloads' }
if (!$Resources) { $Resources = Join-Path $workspace '.tools/installer-resources' }

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
& $Cargo build --locked --offline --release --features installed --manifest-path (Join-Path $app 'src-tauri/Cargo.toml')
if ($LASTEXITCODE -ne 0) { throw "主程序编译失败：$LASTEXITCODE" }

$stage = Join-Path (Split-Path $Output -Parent) 'stage'
New-Item -ItemType Directory -Force -Path $stage | Out-Null
$binary = Join-Path $app 'src-tauri/target/release/wakuwaku.exe'
Copy-Item -LiteralPath $binary -Destination (Join-Path $stage 'DSH-FISH.exe') -Force

# 3. 安装包：NSIS 3，装到用户选的目录，建快捷方式与卸载入口。
Write-Host "生成安装包 → $Output"
& $Node (Join-Path $PSScriptRoot 'installer.mjs') --app (Join-Path $stage 'DSH-FISH.exe') --resources $resDir --version $version --out $Output --makensis $Makensis
if ($LASTEXITCODE -ne 0) { throw "生成安装包失败：$LASTEXITCODE" }

Get-Item -LiteralPath $Output | Select-Object FullName, Length, @{Name = 'ProductVersion'; Expression = { $_.VersionInfo.ProductVersion } }
Get-FileHash -LiteralPath $Output -Algorithm SHA256
