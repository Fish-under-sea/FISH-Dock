#!/usr/bin/env pwsh
# 编译发布主程序并把本机路径洗掉（2.0.4 T-L）。
#
# 为什么要有这一步：cargo 默认把源码/工具链的绝对路径写进 exe 的调试信息与
# `std::panic::Location` 一类的字符串，用户机器上看到 `D:\Fish-code\...` 既泄
# 露开发机目录，也让崩溃报告多出无关路径。这里用 `--remap-path-prefix` 把它们
# 映射成稳定前缀（fish-dock / rust/cargo / rust/rustup / user），构建后再扫
# exe 的 UTF-8 与 UTF-16LE 字节，残留即失败。
#
# 用法（build-installer.ps1 已接入，正常不用手敲）：
#   pwsh app/scripts/build-release.ps1 -Cargo <cargo.exe> -Manifest app/src-tauri/Cargo.toml
#
# -Features 默认 installed（安装版）；-Profile 默认 release。
param(
    [Parameter(Mandatory)][string]$Cargo,
    [Parameter(Mandatory)][string]$Manifest,
    [string]$Features = 'installed',
    [string]$Profile = 'release',
    [string]$VendorDir = $env:FISH_CARGO_VENDOR,
    [string[]]$Scan = @(),
    [switch]$SkipBuild
)
$ErrorActionPreference = 'Stop'
$manifestPath = (Resolve-Path -LiteralPath $Manifest).Path
$srcTauri = Split-Path $manifestPath -Parent
$app = Split-Path $srcTauri -Parent
$repo = Split-Path $app -Parent

$target = if ($env:CARGO_TARGET_DIR) { $env:CARGO_TARGET_DIR } else { Join-Path $srcTauri 'target' }
$binary = Join-Path $target "$Profile/FISH-Dock.exe"

# 1. 路径映射：开发机目录 → 稳定前缀。带空格的路径在这里是单条参数，由
#    CARGO_ENCODED_RUSTFLAGS 的 0x1f 分隔，不经过 shell 拆分。
$pairs = [System.Collections.Generic.List[string]]::new()
$map = @(
    @($repo, 'fish-dock'),
    @($srcTauri, 'fish-dock'),
    @($env:CARGO_HOME, 'rust/cargo'),
    @($env:RUSTUP_HOME, 'rust/rustup'),
    @($env:USERPROFILE, 'user')
)
foreach ($pair in $map) {
    $from = $pair[0]
    if (!$from) { continue }
    $pairs.Add("--remap-path-prefix=$from=$($pair[1])")
}
if ($pairs.Count -eq 0) { throw '没有可映射的本机路径：请先设置 CARGO_HOME、RUSTUP_HOME 或 USERPROFILE' }

# 2. 保住调用方已有的 rustflags（原样拼接，不改它们的语义）。
$encoded = [System.Collections.Generic.List[string]]::new()
foreach ($source in @($env:CARGO_ENCODED_RUSTFLAGS, $env:RUSTFLAGS)) {
    if (!$source) { continue }
    foreach ($flag in ($source -split [char]0x1f)) {
        foreach ($part in ($flag -split ' ')) {
            if ($part) { $encoded.Add($part) }
        }
    }
}
$encoded.AddRange($pairs)
$env:CARGO_ENCODED_RUSTFLAGS = ($encoded -join [char]0x1f)
# RUSTFLAGS 与 CARGO_ENCODED_RUSTFLAGS 同时存在时后者优先，这里清掉前者避免歧义。
Remove-Item Env:\RUSTFLAGS -ErrorAction SilentlyContinue

Write-Host "编译主程序（features=$Features, profile=$Profile）"
if (!$SkipBuild) {
    $cargoArgs = @('build', '--locked', '--offline', '--manifest-path', $manifestPath)
    if ($Profile -eq 'release') { $cargoArgs += '--release' } else { $cargoArgs += @('--profile', $Profile) }
    if ($Features) { $cargoArgs += @('--features', $Features) }
    if ($VendorDir) {
        $cargoArgs += @('--config', 'source.crates-io.replace-with="vendored"',
                        '--config', "source.vendored.directory=`"$($VendorDir -replace '\\','/')`"")
    }
    & $Cargo @cargoArgs
    if ($LASTEXITCODE -ne 0) { throw "主程序编译失败：$LASTEXITCODE" }
}
if (!(Test-Path -LiteralPath $binary -PathType Leaf)) { throw "找不到编译产物：$binary" }

# 3. 扫残留：UTF-8 与 UTF-16LE 两种写法都查。命中即失败，并说出是哪条路径。
#
# haystack 是 `Latin1.GetString(bytes)`：一个字节对应一个字符（UTF-16LE 的 0x00 变成 NUL
# 字符）。所以 pattern 也必须按**同样的方式**从字节转成字符——`Latin1.GetString(编码后的字节)`。
# 不能写成 `$encoding.GetString($encoding.GetBytes($needle))`：那对任何编码都还原成原串，
# 只有 ASCII 那份能命中，纯 UTF-16LE 的路径会永远漏检（t20 审查 F1）。
function Find-InBytes([byte[]]$bytes, [string]$needle) {
    $hay = [System.Text.Encoding]::Latin1.GetString($bytes)
    foreach ($encoding in @([System.Text.Encoding]::UTF8, [System.Text.Encoding]::Unicode)) {
        $pattern = [System.Text.Encoding]::Latin1.GetString($encoding.GetBytes($needle))
        $at = $hay.IndexOf($pattern, [System.StringComparison]::Ordinal)
        if ($at -ge 0) { return $at }
    }
    return -1
}

$leaks = @($repo, $srcTauri, $env:CARGO_HOME, $env:RUSTUP_HOME, $env:USERPROFILE) | Where-Object { $_ } | Select-Object -Unique
$targets = @($binary) + @($Scan | Where-Object { $_ -and (Test-Path -LiteralPath $_ -PathType Leaf) })
$failed = 0
foreach ($file in $targets) {
    $bytes = [System.IO.File]::ReadAllBytes($file)
    foreach ($leak in $leaks) {
        $at = Find-InBytes $bytes $leak
        if ($at -ge 0) {
            Write-Host "路径残留：$([IO.Path]::GetFileName($file)) 偏移 $at 处出现 $leak" -ForegroundColor Red
            $failed++
        }
    }
    $size = [Math]::Round((Get-Item -LiteralPath $file).Length / 1MB, 2)
    Write-Host ("{0}：{1} MB" -f [IO.Path]::GetFileName($file), $size)
}
if ($failed -gt 0) { throw "发布构建里仍有本机绝对路径（$failed 处），已失败" }
Write-Host "路径扫描通过：exe（以及给出的最终包）里没有仓库、cargo、rustup 或用户目录的绝对路径"