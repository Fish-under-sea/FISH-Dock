<#
FISH Dock 2.0.1 预览版启动脚本（脚手架，2.0.1 合并后由 t15 产物就位即可跑）。

这是**预览版**，和已装版本完全隔离：
  · 数据根：build\review-home（新目录，不与旧批的 build\portable-home 混用）
  · 端口：从 48101 起找第一个空闲端口（避开 47213 已装实例、47413 旧预览、47901）
  · 不碰 %USERPROFILE%\.fish-dock、%LOCALAPPDATA%\FISH Dock，也不写注册表/任务计划

为什么必须先播种配置：全新数据根的默认 display 是 taskbar，实例会去接管 Windows 的
任务栏与托盘（2026-10-30 已因此出过一次事故）。所以这里先写 display=island，读回来
硬校验一次，不是 island 就拒绝启动。

用法：
  pwsh -NoProfile -File preview-start.ps1            # 启动并等到 /health 就绪
  pwsh -NoProfile -File preview-start.ps1 -WhatIf     # 只打印将要做的事，不启动
  pwsh -NoProfile -File preview-start.ps1 -Stop       # 只结束本脚本启动的那个 PID
#>
param(
    [switch]$WhatIf,
    [switch]$Stop
)

$ErrorActionPreference = 'Stop'

$here = Split-Path -Parent $MyInvocation.MyCommand.Path
$exe = Join-Path $here 'FISH-Dock.exe'

# 数据根、端口起点、保留端口只在这一次定义（脚本、文档、报告同一口径）。
$productRoot = [System.IO.Path]::GetFullPath((Join-Path $here '..\..'))
$root = [System.IO.Path]::GetFullPath((Join-Path $productRoot 'build\review-home'))
$dataDir = Join-Path $root 'data'
$homeDir = Join-Path $root 'home'
$hostRoot = Join-Path $dataDir 'host'
$configPath = Join-Path $hostRoot 'config\config.json'
$pidFile = Join-Path $root 'preview.pid'
$logPath = Join-Path $hostRoot 'recent.log'
$portStart = 48101
$reserved = @(47213, 47413, 47901)   # 已装实例 / 旧预览 / 上一批预留

function Get-FreePort {
    param([int]$From, [int[]]$Skip)
    for ($p = $From; $p -lt ($From + 200); $p++) {
        if ($Skip -contains $p) { continue }
        $listener = $null
        try {
            $listener = [System.Net.Sockets.TcpListener]::new([System.Net.IPAddress]::Loopback, $p)
            $listener.Start()
            return $p
        }
        catch { continue }
        finally { if ($listener) { try { $listener.Stop() } catch { } } }
    }
    throw "从 $From 起找不到空闲端口（试了 200 个）"
}

# ── -Stop：只结束本脚本启动的那个 PID ────────────────────────────────────────
# 不许按进程名杀：用户已装的 FISH-Dock.exe 同名，按名字杀会误杀正式实例。
if ($Stop) {
    if (-not (Test-Path -LiteralPath $pidFile)) {
        Write-Host "没有 $pidFile：本脚本没有记录过启动的实例，什么也不做。"
        exit 0
    }
    $recorded = (Get-Content -LiteralPath $pidFile -Raw).Trim()
    if (-not ($recorded -match '^\d+$')) {
        Write-Host "pid 文件内容不是数字（$recorded）：不动作，请人工确认。"
        exit 2
    }
    $targetId = [int]$recorded
    $proc = Get-Process -Id $targetId -ErrorAction SilentlyContinue
    if (-not $proc) {
        Write-Host "pid=$targetId 已不在运行：清掉 pid 文件即可。"
        Remove-Item -LiteralPath $pidFile -Force
        exit 0
    }
    # PID 可能被复用：必须确认这个进程就是我们便携目录里的那个 exe。
    $procPath = $null
    try { $procPath = $proc.Path } catch { $procPath = $null }
    if (-not $procPath -or -not (Test-Path -LiteralPath $procPath)) {
        Write-Host "pid=$targetId 的路径读不到（可能已退出或权限不足）：不动作。"
        exit 2
    }
    if ([System.IO.Path]::GetFullPath($procPath) -ne [System.IO.Path]::GetFullPath($exe)) {
        Write-Host "pid=$targetId 跑的是 $procPath，不是本预览目录的 $exe：拒绝结束（避免误杀正式实例）。"
        exit 2
    }
    Stop-Process -Id $targetId -Force
    Write-Host "已结束预览实例 pid=$targetId（数据根保留在 $root）。"
    Remove-Item -LiteralPath $pidFile -Force
    exit 0
}

# ── 版本：从构建配置读，读不到退回 exe 版本资源（不写死）────────────────────
$version = $null
foreach ($conf in @(
        (Join-Path $productRoot 'dock\app\src-tauri\tauri.conf.json'),
        (Join-Path $productRoot 'app\src-tauri\tauri.conf.json')
    )) {
    if (Test-Path -LiteralPath $conf) { $version = (Get-Content -LiteralPath $conf -Raw | ConvertFrom-Json).version; break }
}
if (-not $version -and (Test-Path -LiteralPath $exe)) { $version = (Get-Item -LiteralPath $exe).VersionInfo.ProductVersion }
if (-not $version) { $version = '2.0.1' }

# 播种的键：只补缺的，不覆盖已有值（重复运行不该把用户改过的设置刷回去）。
$seed = [ordered]@{
    display         = 'island'   # 关键：taskbar 会接管用户的任务栏与托盘
    islandHideUnder = $false     # 预览要求：不躲在别的窗口下面
    autoUpdateCheck = $false     # 预览期间不查更新
    onboarded       = $true      # 不弹首次引导
    islandPos       = 'right'
    walk            = $false
    lang            = 'zh'
}

if ($WhatIf) {
    $port = Get-FreePort -From $portStart -Skip $reserved
    Write-Host "【-WhatIf】只打印，不启动、不写任何文件。"
    Write-Host ("程序      : {0}（存在={1}）" -f $exe, (Test-Path -LiteralPath $exe))
    Write-Host ("版本      : {0}" -f $version)
    Write-Host ("数据根    : {0}" -f $root)
    Write-Host ("  配置    : {0}（已存在={1}）" -f $configPath, (Test-Path -LiteralPath $configPath))
    Write-Host ("  日志    : {0}" -f $logPath)
    Write-Host ("  端口记录: {0}" -f $pidFile)
    Write-Host ("端口      : {0}（从 {1} 起第一个空闲，跳过 {2}）" -f $port, $portStart, ($reserved -join '/'))
    Write-Host ("将播种    : {0}" -f (($seed.GetEnumerator() | ForEach-Object { "$($_.Key)=$($_.Value)" }) -join ', '))
    Write-Host "将校验    : 读回配置，display 必须是 island，否则拒绝启动"
    Write-Host "将轮询    : http://127.0.0.1:$port/health 最多 60 秒，输出 ready.island / ready.pet"
    Write-Host "停止方式  : pwsh -NoProfile -File preview-start.ps1 -Stop（只结束本脚本记录的 PID）"
    exit 0
}

# ── 按程序自己的规矩建数据根 ────────────────────────────────────────────────
New-Item -ItemType Directory -Path $homeDir, $dataDir -Force | Out-Null
foreach ($dir in @('backups', 'config', 'crashes', 'credentials', 'exports', 'keys', 'pets', 'plugins', 'roster', 'tmp')) {
    New-Item -ItemType Directory -Path (Join-Path $hostRoot $dir) -Force | Out-Null
}

$ownership = [ordered]@{ pluginId = 'host'; directory = 'host'; pluginVersion = $version; dataSchema = 1 }
$ownership | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $hostRoot 'storage.json') -Encoding utf8NoBOM

# 已有配置就以它为准、只补齐缺的键；islandHideUnder 是预览硬要求，若被设成 true 会显式改回并打印。
$config = if (Test-Path -LiteralPath $configPath) { Get-Content -LiteralPath $configPath -Raw | ConvertFrom-Json -AsHashtable } else { @{} }
$forced = @()
foreach ($key in $seed.Keys) {
    if (-not $config.Contains($key)) { $config[$key] = $seed[$key] }
}
if ($config.Contains('islandHideUnder') -and $config['islandHideUnder'] -ne $false) {
    $forced += "islandHideUnder 原为 $($config['islandHideUnder'])"
    $config['islandHideUnder'] = $false
}
$config | ConvertTo-Json -Depth 6 | Set-Content -LiteralPath $configPath -Encoding utf8NoBOM

# 启动前硬校验：display 必须是 island。这道校验就是那次任务栏事故留下的防线。
$seeded = Get-Content -LiteralPath $configPath -Raw | ConvertFrom-Json
if ($seeded.display -ne 'island') {
    throw "配置 display=$($seeded.display)，不是 island：拒绝启动（否则实例会接管用户的任务栏与托盘）"
}
if ($seeded.islandHideUnder -ne $false) {
    throw "配置 islandHideUnder=$($seeded.islandHideUnder)，预览要求 false：拒绝启动"
}
foreach ($note in $forced) { Write-Host "注意：$note → 已改为 false（预览要求）" }

# 产物必须在：缺 exe 时到此为止（配置校验已经先跑完，问题一眼看得出）。
if (-not (Test-Path -LiteralPath $exe)) {
    throw "找不到 $exe：便携目录不完整（2.0.1 产物还没放进来？）"
}

# 迁移标记：storage_bootstrap::prepare() 见「有 config.json 却没有 migration-v1.json」会报红字。
# 形状与 storage_bootstrap.rs 一致：紧凑 JSON、无空格、无结尾换行。
$markerPath = Join-Path $hostRoot 'config\migration-v1.json'
if (-not (Test-Path -LiteralPath $markerPath)) {
    [System.IO.File]::WriteAllText($markerPath, '{"version":1,"sourceRetained":true}')
}

$port = Get-FreePort -From $portStart -Skip $reserved

# ── 环境 ────────────────────────────────────────────────────────────────────
$env:FISH_DOCK_HOME = $dataDir
$env:FISH_DOCK_APP_HOME = $homeDir
$env:FISH_DOCK_PORT = [string]$port
$env:FISH_DOCK_SKIP_HOME_MIGRATION = '1'
try {
    # --fish-dock-out-of-job：父进程在 job 里时程序会把自己重新拉起再让父进程退出；
    # 带这个开关就原地起，拿到的 PID 就是真实例。
    $started = Start-Process -FilePath $exe -ArgumentList '--fish-dock-out-of-job' -WorkingDirectory $root -PassThru -WindowStyle Hidden
}
finally {
    Remove-Item Env:FISH_DOCK_HOME, Env:FISH_DOCK_APP_HOME, Env:FISH_DOCK_PORT, Env:FISH_DOCK_SKIP_HOME_MIGRATION -ErrorAction SilentlyContinue
}

Set-Content -LiteralPath $pidFile -Value $started.Id -Encoding utf8NoBOM

Write-Host ("FISH Dock {0} 预览版已启动：pid={1} port={2}" -f $version, $started.Id, $port)
Write-Host ("数据根（关掉也不会删）：{0}" -f $root)
Write-Host ("日志：{0}" -f $logPath)

# ── 等 /health 就绪（最多 60 秒）────────────────────────────────────────────
$deadline = (Get-Date).AddSeconds(60)
$health = $null
while ((Get-Date) -lt $deadline) {
    if ($started.HasExited) { break }
    try {
        $health = Invoke-RestMethod -Uri ("http://127.0.0.1:{0}/health" -f $port) -TimeoutSec 2 -ErrorAction Stop
        if ($health.ready.island -and $health.ready.pet) { break }
    }
    catch { $health = $null }
    Start-Sleep -Milliseconds 500
}

if ($health -and $health.ready.island -and $health.ready.pet) {
    Write-Host ("就绪：ready.island={0} ready.pet={1}" -f $health.ready.island, $health.ready.pet)
    exit 0
}

Write-Host "60 秒内没有就绪（ready.island / ready.pet 未同时为 true）。最后 20 行日志："
if (Test-Path -LiteralPath $logPath) {
    Get-Content -LiteralPath $logPath -Tail 20 | ForEach-Object { Write-Host $_ }
}
else {
    Write-Host "（还没有日志：$logPath 不存在，实例可能根本没起来）"
}
exit 1
