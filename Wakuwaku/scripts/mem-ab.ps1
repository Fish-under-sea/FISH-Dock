<#
.SYNOPSIS
    FISH Dock 内存「改前 / 改后」同条件对照：同一套采样核心，两个二进制。

.DESCRIPTION
    纪律（captain 2026-10-30 定的四条，加上被用户手动退掉一个实例之后补的两条）：
    - 同机、同时段、同采样间隔、同插件加载集：两个变体都用全新的空数据根，
      因此都是「无插件加载」；现装那一份的「有插件」数字用 mem-sample.ps1 单独采。
    - 只指向 build/mem/ab-<tag>/ 下的独立数据根，绝不碰 %USERPROFILE%\.fish-dock
      与 %LOCALAPPDATA%\FISH Dock 里的用户数据（只从后者**读**两个运行时标记
      文件复制进临时根，见 -SeedHome）。
    - 端口只在这两个实例的环境变量里给（47901/47902），不改代码里的默认端口。
    - 结束时 taskkill /T /F 收干净，PID 与起止时间落进 pids.txt（被强杀时照着它收）。
    - **不许接管用户的任务栏与托盘**：全新数据根的默认配置是 display=taskbar，
      实例会把 Windows 的任务栏和托盘接过来（实测 NIM_ADD ×28、handoff 全套），
      用户当场把它退了。因此启动前先把 config.json 播下去：display=island。
    - **不许查更新**：autoUpdateCheck=false，免得对照期间去下 GitHub 的东西。

    为什么必须带 --wakuwaku-out-of-job：main.rs 的 detach 逻辑发现自己在别人的
    job 里（这里是 pwsh 工具的 job），会把自己重新拉出 job 再退出，于是
    Start-Process 拿到的 PID 立刻就没了（实测 exit=0，什么都没写）。

    为什么必须给 DSH_FISH_RESOURCES 并预置两个标记文件：这两个二进制都是
    --features installed 构建的，启动时 runtime::prepare() 会校验安装资源。
    资源目录用现装那一份（只读）；`state/verified`（清单摘要 + 资源路径）与
    `accepted-runtime-terms`（WebView2 许可摘要）从现装 app home 复制过来，
    prepare() 就认为「上次已全量校验过」，只查必需文件在不在——不再哈希
    776 MiB，也不再对现装目录跑 icacls 授权（那会改到用户安装的 ACL）。

.PARAMETER Minutes
    采样时长（分钟）。要拿到 5/30/60 三个点，至少 61。

.PARAMETER Parallel
    两个变体同时在屏幕上跑一轮（默认，62 分钟）；不加则先 before 再 after
    各跑一轮（124 分钟）。默认同时跑的理由：桌面上多出来的实例是打扰，
    同时跑把打扰时间砍掉一半，而「同机同时段同环境」这一条同时跑满足得更彻底
    （两次分开跑之间，机器上的别的进程、内存压力、温度降频都会漂）。

.PARAMETER DockScreen
    两个实例的岛放到哪块屏（副屏，别压在用户自己那只岛上）。默认是本机副屏
    的矩形，换机器时用 screen::read 报出来的矩形改它。

.EXAMPLE
    pwsh -File scripts/mem-ab.ps1 -Minutes 1 -Which before   # 先验证起得来
    pwsh -File scripts/mem-ab.ps1 -Minutes 62                # 正式对照（两个同时）
#>
[CmdletBinding()]
param(
    [int]$Minutes = 62,
    [int]$IntervalSec = 20,
    [string]$OutDir = '',
    [string]$RunTag = '',
    [ValidateSet('both', 'before', 'after')][string]$Which = 'both',
    [switch]$Sequential,
    [string]$Resources = 'C:\Users\Fish\AppData\Local\Programs\FISH Dock\resources',
    [string]$SeedHome = 'C:\Users\Fish\AppData\Local\FISH Dock',
    [int[]]$DockScreen = @(3840, 575, 2560, 1600),
    [ValidateSet('left', 'center', 'right')][string]$IslandPos = 'right',
    [switch]$LeaveRunning
)

$ErrorActionPreference = 'Stop'
Import-Module (Join-Path $PSScriptRoot 'MemSample.psm1') -Force

$app = Split-Path $PSScriptRoot -Parent                       # …\dock\Wakuwaku
$repo = Split-Path (Split-Path $app -Parent) -Parent           # …\FISH-Dock-1029
if (-not $OutDir) { $OutDir = Join-Path $repo 'build\mem' }
if (-not $RunTag) { $RunTag = (Get-Date).ToString('MMdd-HHmm') }

# 改前那一份是出厂 exe 的字节副本（SHA-256 与现装 DSH-FISH.exe 相同）；
# 改后那一份是本地重新构建的产物。两者同一套 flag：release + installed。
$variants = @(
    [pscustomobject]@{ Name = 'before'; Exe = Join-Path $OutDir 'before\wakuwaku.exe'; Port = 47901 },
    [pscustomobject]@{ Name = 'after'; Exe = Join-Path $OutDir 'after\wakuwaku.exe'; Port = 47902 }
) | Where-Object { $Which -eq 'both' -or $Which -eq $_.Name }

foreach ($variant in $variants) {
    if (-not (Test-Path -LiteralPath $variant.Exe)) { throw "找不到待测二进制：$($variant.Exe)" }
}
if (-not (Test-Path -LiteralPath (Join-Path $Resources 'manifest.json'))) { throw "安装资源里没有 manifest.json：$Resources" }

$runDir = Join-Path $OutDir "ab-$RunTag"
New-Item -ItemType Directory -Path $runDir -Force | Out-Null
$pidFile = Join-Path $runDir 'pids.txt'

# 任务栏被接管的痕迹（宿主日志里的原话）。采样循环每个 tick 查一次，查到就中止：
# 用户已经因为一次接管手动退过程序，不能再来第二次。
$script:TakeoverPattern = 'handoff: in|NIM_ADD|taskbar: strip|handoff: go \(taskbar\)'
if (-not (Test-Path -LiteralPath $pidFile)) {
    Set-Content -LiteralPath $pidFile -Value "# 被强杀时照这个文件收残留实例（taskkill /PID <pid> /T /F）" -Encoding utf8NoBOM
}

# 数据根由脚本按程序自己的规矩建：归属记录 storage.json（程序认这个才肯接管
# 一个非空目录）+ 只写「不打扰用户」那几个键的 config.json，其余键让程序自己
# 按默认补。两条实测出来的坑，别再踩：
# 1. 只播 config.json、没有 storage.json → 程序直接退出，stderr 是「目标插件
#    目录非空且缺少归属记录，请先导入或恢复」（storage_context.rs）。
# 2. 不播配置、用全新数据根的默认值 → display=taskbar，实例会接管 Windows 的
#    任务栏与托盘（实测 handoff: in (pictured) / taskbar: strip / NIM_ADD ×28），
#    用户当场把它退了；硬杀这种实例还有把任务栏留在被接管状态的风险。
#    播了 display=island 之后实测：handoff: go (island)，任务栏接管痕迹 0 条。
function Initialize-DataRoot {
    param([string]$Root, [string]$HomeDir)

    $hostRoot = Join-Path $Root 'data\host'
    New-Item -ItemType Directory -Path (Join-Path $HomeDir 'state'), (Join-Path $Root 'data') -Force | Out-Null
    foreach ($dir in @('backups', 'config', 'crashes', 'credentials', 'exports', 'keys', 'pets', 'plugins', 'roster', 'tmp')) {
        New-Item -ItemType Directory -Path (Join-Path $hostRoot $dir) -Force | Out-Null
    }

    # 只读复制现装 app home 的两个运行时标记（不写回原处）：有它们，
    # runtime::prepare() 就认为上次已全量校验过，不再哈希 776 MiB，也不再对
    # 现装目录跑 icacls（那会改到用户安装的 ACL）。
    foreach ($marker in @('state\verified', 'accepted-runtime-terms')) {
        $from = Join-Path $SeedHome $marker
        if (Test-Path -LiteralPath $from) { Copy-Item -LiteralPath $from -Destination (Join-Path $HomeDir $marker) -Force }
    }

    # 归属记录里的版本跟着 tauri.conf.json 走，不写死（红线第 5 条）。
    $version = (Get-Content -LiteralPath (Join-Path $app 'src-tauri\tauri.conf.json') -Raw | ConvertFrom-Json).version
    $ownership = [ordered]@{ pluginId = 'host'; directory = 'host'; pluginVersion = $version; dataSchema = 1 }
    Set-Content -LiteralPath (Join-Path $hostRoot 'storage.json') -Value ($ownership | ConvertTo-Json) -Encoding utf8NoBOM

    $seed = [ordered]@{
        display         = 'island'       # taskbar 会把用户的任务栏和托盘接过来
        autoUpdateCheck = $false         # 对照期间不去查更新
        onboarded       = $true          # 不弹首次引导
        dockScreen      = @($DockScreen) # 岛放副屏，别压在用户自己那只岛上
        islandPos       = $IslandPos
        walk            = $false         # 宠物不在用户屏幕上乱走
    }
    Set-Content -LiteralPath (Join-Path $hostRoot 'config\config.json') -Value ($seed | ConvertTo-Json -Depth 4) -Encoding utf8NoBOM
}

function Stop-Tree {
    param([int]$ProcessId)

    # /T 连子进程一起：WebView2 的 browser/renderer/gpu 与插件后端都是它的孩子。
    & taskkill.exe /PID $ProcessId /T /F 2>&1 | Out-Null
}

# 这一份 CSV 里该变体已经跑过几次：第一次是 before，之后 before#2、before#3…
# 用途见 Start-Variant 里的断点续跑说明。
function Get-NextAttemptLabel {
    param([string]$Csv, [string]$Name)

    if (-not (Test-Path -LiteralPath $Csv)) { return $Name }
    $labels = @(Import-Csv -LiteralPath $Csv | Select-Object -ExpandProperty label -Unique)
    $mine = @($labels | Where-Object { $_ -eq $Name -or $_ -like "$Name#*" })
    if ($mine.Count -eq 0) { return $Name }
    return "$Name#$($mine.Count + 1)"
}

# 起一个变体，等它的 WebView2 上来，返回采样目标（含 PID，收尾要用）。
function Start-Variant {
    param($Variant)

    $root = Join-Path $runDir $Variant.Name
    # 每次跑都用干净的数据根：残留的 webview 目录会让两次的起点不一样。
    if (Test-Path -LiteralPath $root) { Remove-Item -LiteralPath $root -Recurse -Force }
    $homeDir = Join-Path $root 'home'
    Initialize-DataRoot -Root $root -HomeDir $homeDir

    # 启动前硬校验：display 不是 island 就拒绝启动。第一次跑对照时全新数据根的
    # 默认值是 taskbar，实例真把用户的 Windows 任务栏和托盘接管了（handoff: in
    # (pictured) / taskbar: strip / NIM_ADD ×28），用户当场手动 Quit。这道校验
    # 就是那次事故留下的防线，不许为了少一行代码拿掉。
    $seeded = Get-Content -LiteralPath (Join-Path $root 'data\host\config\config.json') -Raw | ConvertFrom-Json
    if ($seeded.display -ne 'island') {
        throw "播下去的配置 display=$($seeded.display)，不是 island：拒绝启动（否则实例会接管用户的任务栏与托盘）"
    }

    # 子进程在创建那一刻快照父进程的环境，因此逐个设、逐个起、起完就撤。
    $env:FISH_DOCK_HOME = Join-Path $root 'data'
    $env:FISH_DOCK_APP_HOME = $homeDir
    $env:WAKUWAKU_PORT = [string]$Variant.Port
    $env:DSH_FISH_RESOURCES = $Resources
    $env:FISH_DOCK_SKIP_HOME_MIGRATION = '1'
    try {
        $started = Start-Process -FilePath $Variant.Exe -ArgumentList '--wakuwaku-out-of-job' -WorkingDirectory $root -PassThru -WindowStyle Hidden
    }
    finally {
        Remove-Item Env:FISH_DOCK_HOME, Env:FISH_DOCK_APP_HOME, Env:WAKUWAKU_PORT, Env:DSH_FISH_RESOURCES, Env:FISH_DOCK_SKIP_HOME_MIGRATION -ErrorAction SilentlyContinue
    }
    Add-Content -LiteralPath $pidFile -Value ("{0} pid={1} exe={2} port={3} root={4} started={5:o}" -f $Variant.Name, $started.Id, $Variant.Exe, $Variant.Port, $root, (Get-Date)) -Encoding utf8NoBOM
    Write-Host ("起了 {0}：pid={1} port={2} 数据根={3}" -f $Variant.Name, $started.Id, $Variant.Port, $root)

    # 等 WebView2 的 browser 进程起来（起不来就说明环境不对，别把「没起来」
    # 当成「内存很小」记进对照表）。
    foreach ($attempt in 1..60) {
        Start-Sleep -Seconds 2
        $info = Get-CimInstance Win32_Process -Filter "ProcessId=$($started.Id)" | Select-Object -First 1
        if (-not $info) { throw "$($Variant.Name) 启动后退出（pid $($started.Id) 已不在）；数据根 $root，日志 $root\data\host\recent.log" }
        $tree = @(Get-MemProcessTree -All @(Get-CimInstance Win32_Process) -Root $started.Id)
        $hasBrowser = $tree | Where-Object { (Get-MemProcessRole -Name $_.Proc.Name -CommandLine $_.Proc.CommandLine -Depth $_.Depth) -eq 'webview:browser' }
        if ($hasBrowser) {
            Write-Host ("  {0}：WebView2 已起（{1}s，树里 {2} 个进程）" -f $Variant.Name, ($attempt * 2), $tree.Count)
            $csv = Join-Path $runDir "$($Variant.Name).csv"
            Initialize-MemCsv -Path $csv
            # 断点续跑：这个 run 目录里已经有该变体的样本就接着追加，不重头来。
            # 但本次启动另给一个标签（before#2、before#3…）：elapsed_sec 是「进程
            # 年龄」，新起的进程从 0 开始，两次混在同一个标签里会让取点取错行。
            $label = Get-NextAttemptLabel -Csv $csv -Name $Variant.Name
            if ($label -ne $Variant.Name) { Write-Host ("  {0}：续跑，本次样本标签 {1}（旧样本保留在同一个 csv 里）" -f $Variant.Name, $label) }
            return [pscustomobject]@{
                Name    = $Variant.Name
                Pid     = $started.Id
                EndedAt = $null
                Target  = @{ Label = $label; RootPid = $started.Id; OutCsv = $csv; Origin = $info.CreationDate; Scope = 'tree'; LogPath = (Join-Path $root 'data\host\recent.log') }
            }
        }
    }
    Stop-Tree -ProcessId $started.Id
    throw "$($Variant.Name) 120s 内没有拉起 WebView2，对照无意义，已中止"
}

# 采样循环：一个 tick 一次 Win32_Process 查询，喂给所有目标树（同时跑时两棵树
# 取自同一份快照，环境噪声是同一份）。
function Invoke-Sampling {
    param($Targets, [int]$ForMinutes)

    $deadline = (Get-Date).AddMinutes($ForMinutes)
    $tick = 0
    Write-Host ("开始采样 {0} 分钟，每 {1}s 一个 tick，{2} 棵树" -f $ForMinutes, $IntervalSec, $Targets.Count)
    while ($true) {
        $tickStart = Get-Date
        if ($tickStart -ge $deadline) { break }
        $alive = Write-MemSampleTick -Targets $Targets -Now $tickStart
        if ($alive -eq 0) { Write-Warning '所有实例都不在了，采样提前结束'; break }
        if ($alive -lt $Targets.Count) { Write-Warning ("只剩 {0}/{1} 棵树还在" -f $alive, $Targets.Count) }
        $tick++
        # 任务栏接管零容忍（captain 2026-10-30 定的）：日志里一出现接管的痕迹
        # 就立刻停，throw 会走外层 finally 把两棵树 taskkill /T /F 掉。
        # 正常的岛模式也会有 handoff: go (island)，所以匹配的是接管特有的那几条。
        foreach ($target in $Targets) {
            if (-not $target.LogPath -or -not (Test-Path -LiteralPath $target.LogPath)) { continue }
            $hit = Select-String -LiteralPath $target.LogPath -Pattern $script:TakeoverPattern -ErrorAction SilentlyContinue | Select-Object -First 1
            if ($hit) { throw "$($target.Label) 的日志出现任务栏接管痕迹，已中止对照：$($hit.Line)" }
        }
        if ($tick % 15 -eq 0) {
            $ages = $Targets | ForEach-Object { "{0}={1}min" -f $_.Label, [math]::Round(($tickStart - $_.Origin).TotalMinutes, 1) }
            Write-Host ("  tick {0}（{1}）" -f $tick, ($ages -join ' '))
        }
        $wait = [math]::Min($IntervalSec - ((Get-Date) - $tickStart).TotalSeconds, ($deadline - (Get-Date)).TotalSeconds)
        if ($wait -le 0) { break }
        Start-Sleep -Seconds $wait
    }
    Write-Host ("采样结束：ticks={0}" -f $tick)
    return $tick
}

$running = @()
$launched = @()
try {
    if ($Sequential) {
        foreach ($variant in $variants) {
            $one = Start-Variant -Variant $variant
            $running = @($one)
            $launched += $one
            try { Invoke-Sampling -Targets @($one.Target) -ForMinutes $Minutes | Out-Null }
            finally {
                if (-not $LeaveRunning) { Stop-Tree -ProcessId $one.Pid }
                $one.EndedAt = Get-Date
                $running = @()
            }
        }
    }
    else {
        foreach ($variant in $variants) { $one = Start-Variant -Variant $variant; $running += $one; $launched += $one }
        try { Invoke-Sampling -Targets ($running | ForEach-Object { $_.Target }) -ForMinutes $Minutes | Out-Null }
        finally { foreach ($item in $running) { $item.EndedAt = Get-Date } }
    }
}
finally {
    foreach ($item in $running) { if (-not $LeaveRunning) { Stop-Tree -ProcessId $item.Pid } }
    Start-Sleep -Seconds 3
    # 收尾要可核：不说「已清理」这种空话，把实测残留数与每个实例的 PID、起止
    # 时间都打出来（captain 2026-10-30 的要求）。DSH-FISH 那两个是用户现装的，
    # 不该被这次对照动到，所以也要报出来核一遍。
    $leftWaku = @(Get-Process -Name 'wakuwaku' -ErrorAction SilentlyContinue)
    $leftFish = @(Get-Process -Name 'DSH-FISH' -ErrorAction SilentlyContinue)
    foreach ($item in $launched) {
        if (-not $item.EndedAt) { $item.EndedAt = Get-Date }
        Add-Content -LiteralPath $pidFile -Value ("{0} pid={1} ended={2:o} killed={3}" -f $item.Name, $item.Pid, $item.EndedAt, (-not $LeaveRunning)) -Encoding utf8NoBOM
    }
    Add-Content -LiteralPath $pidFile -Value ("finished={0:o} leftover_wakuwaku={1} leftover_dsh_fish={2}" -f (Get-Date), $leftWaku.Count, $leftFish.Count) -Encoding utf8NoBOM
    Write-Host ("收尾核对：Get-Process wakuwaku 残留 {0} 个（必须是 0）；Get-Process DSH-FISH {1} 个（应当只剩用户自己现装的，本次对照不动它）" -f $leftWaku.Count, $leftFish.Count)
    foreach ($item in $leftWaku) { Write-Host ("  残留 wakuwaku pid={0}" -f $item.Id) }
    foreach ($item in $leftFish) { Write-Host ("  现装 DSH-FISH pid={0} 起于 {1:o}" -f $item.Id, $item.StartTime) }
    foreach ($item in $launched) { Write-Host ("  本次实例 {0} pid={1} label={2}" -f $item.Name, $item.Pid, $item.Target.Label) }
    Write-Host ("  PID 与起止时间全在 {0}" -f $pidFile)
}

Write-Host ("对照数据目录：{0}" -f $runDir)
Write-Host "解读：node scripts/mem-report.mjs --dir `"$runDir`" --at 1,3,5,10,15,20"
