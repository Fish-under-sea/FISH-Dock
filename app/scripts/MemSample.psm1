<#
.SYNOPSIS
    FISH Dock 内存采样的共用核心：进程树收集、角色定性、CSV 行拼装。

.DESCRIPTION
    为什么要抽成模块：单根采样（mem-sample.ps1）与改前/改后同条件对照
    （mem-ab.ps1）用的是同一套逻辑。红线《禁止屎山十条》第 3 条——同一逻辑
    出现两次就要抽函数，改一处必须只改一处。

    三个实测校准点（本机 Windows + PowerShell 7.6.6，2026-10-30）：
    1. 一个 tick 只做一次 Win32_Process 查询，内存/句柄/线程全部取自同一快照，
       避免 Get-Process 与 CIM 两次查询之间的时间差。
    2. Win32_Process.PrivatePageCount 名字里带 Page，**单位其实是字节**：
       实测 PID 31388 的 PrivatePageCount 与 Get-Process 的 PrivateMemorySize64
       完全相等（同为 20.1MB）。按字节直接用，不要再乘页大小。
    3. ParentProcessId 会被 PID 复用骗到（父进程退出后新进程顶了同一个 id），
       因此收集子树时要求「子的 CreationDate 不早于父的 CreationDate」。
    4. PowerShell 7 上 @() 包住「装着 Hashtable 的 List[object]」会抛
       ArgumentException（Argument types do not match），因此队列元素用
       PSCustomObject，出参一律 ToArray()。
#>

$script:CsvHeader = 'sample_utc,elapsed_sec,label,depth,pid,ppid,name,role,ws_bytes,private_bytes,handles,threads,pagefile_bytes,detail'

function Get-MemCsvHeader { return $script:CsvHeader }

# CSV 字段转义：换行压成空格，含引号/分号/逗号时整体加引号。
function ConvertTo-MemCsvText {
    param([string]$Value)

    if ([string]::IsNullOrEmpty($Value)) { return '' }
    $flat = ($Value -replace '[\r\n]+', ' ')
    if ($flat -match '[";,]') { return '"' + ($flat -replace '"', '""') + '"' }
    return $flat
}

# role 从命令行推导：WebView2 用 --type= 区分进程种类，utility 再带子类型；
# 没有 --type= 的那个是每个 WebView2 环境一个的 browser 进程。
# 插件后端（node.exe 跑 backend.mjs）标成 plugin-backend，插件自己再派生的
# runtime 标成 plugin-runtime，这样报表能把「dock 自身」与「插件」分开算。
function Get-MemProcessRole {
    param([string]$Name, [string]$CommandLine, [int]$Depth)

    # 深度 0 就是被测的宿主 exe，它的命令行里没有可识别特征，按位置定性。
    if ($Depth -eq 0) { return 'host' }
    if ([string]::IsNullOrWhiteSpace($CommandLine)) { return 'child' }
    if ($CommandLine -match '--type=([\w\.-]+)') {
        $type = $Matches[1]
        if ($type -eq 'utility' -and $CommandLine -match '--utility-sub-type=([\w\.-]+)') {
            return "webview:$($Matches[1])"
        }
        return "webview:$type"
    }
    if ($CommandLine -match '--embedded-browser-webview=1') { return 'webview:browser' }
    if ($CommandLine -match 'backend\.mjs') { return 'plugin-backend' }
    if ($CommandLine -match '\.fishplugin-') { return 'plugin-runtime' }
    if ($Name -eq 'conhost.exe') { return 'console' }
    return 'child'
}

# detail 只留身份标识（插件 id@版本 / 宿主 exe 名 / 插件监听地址），不落整条
# 命令行：命令行里有本机绝对路径，进 CSV 既臃肿又会把用户目录带进报表。
function Get-MemProcessDetail {
    param([string]$Role, [string]$CommandLine)

    if ([string]::IsNullOrWhiteSpace($CommandLine)) { return '' }
    if ($Role -eq 'plugin-backend' -and $CommandLine -match 'packages[\\/]([\w\.-]+)[\\/]([\d\.]+)') {
        return "$($Matches[1])@$($Matches[2])"
    }
    if ($Role -eq 'plugin-runtime' -and $CommandLine -match '\.fishplugin-([\w\.-]+)') { return $Matches[1] }
    if ($CommandLine -match '--webview-exe-name=([\w\.-]+)') { return "host-exe=$($Matches[1])" }
    if ($CommandLine -match '--addr\s+([\d\.]+:\d+)') { return "addr=$($Matches[1])" }
    return ''
}

# 广度优先收集整棵树；PID 复用防护见文件头校准点 3。
function Get-MemProcessTree {
    param($All, [int]$Root, [string]$Scope = 'tree')

    $byId = @{}
    foreach ($proc in $All) { $byId[[uint32]$proc.ProcessId] = $proc }
    if (-not $byId.ContainsKey([uint32]$Root)) { return @() }

    $childrenOf = @{}
    foreach ($proc in $All) {
        $parentId = [uint32]$proc.ParentProcessId
        if (-not $byId.ContainsKey($parentId)) { continue }
        $parent = $byId[$parentId]
        if ($proc.CreationDate -and $parent.CreationDate -and $proc.CreationDate -lt $parent.CreationDate) { continue }
        if (-not $childrenOf.ContainsKey($parentId)) { $childrenOf[$parentId] = New-Object System.Collections.Generic.List[object] }
        $childrenOf[$parentId].Add($proc)
    }

    $found = New-Object System.Collections.Generic.List[object]
    $queue = New-Object System.Collections.Generic.Queue[object]
    $queue.Enqueue([pscustomobject]@{ Proc = $byId[[uint32]$Root]; Depth = 0 })
    $seen = New-Object System.Collections.Generic.HashSet[uint32]
    [void]$seen.Add([uint32]$Root)

    while ($queue.Count -gt 0) {
        $node = $queue.Dequeue()
        $found.Add($node)
        if ($Scope -eq 'host') { continue }
        $currentId = [uint32]$node.Proc.ProcessId
        if (-not $childrenOf.ContainsKey($currentId)) { continue }
        foreach ($child in $childrenOf[$currentId]) {
            $childId = [uint32]$child.ProcessId
            if ($seen.Add($childId)) {
                $queue.Enqueue([pscustomobject]@{ Proc = $child; Depth = $node.Depth + 1 })
            }
        }
    }
    return $found.ToArray()
}

# 一行样本。PrivatePageCount 按字节用（校准点 2）。
function New-MemSampleRow {
    param($Proc, [datetime]$Now, [int]$ElapsedSec, [int]$Depth, [string]$Label)

    $role = Get-MemProcessRole -Name $Proc.Name -CommandLine $Proc.CommandLine -Depth $Depth
    $privateBytes = if ($Proc.PrivatePageCount) { [uint64]$Proc.PrivatePageCount } else { [uint64]0 }
    $pagefileBytes = if ($Proc.PageFileUsage) { [uint64]$Proc.PageFileUsage * 1024 } else { [uint64]0 }
    $detail = Get-MemProcessDetail -Role $role -CommandLine $Proc.CommandLine

    return @(
        $Now.ToString('o'), $ElapsedSec, (ConvertTo-MemCsvText $Label), $Depth,
        $Proc.ProcessId, $Proc.ParentProcessId, (ConvertTo-MemCsvText $Proc.Name), $role,
        [uint64]$Proc.WorkingSetSize, $privateBytes, $Proc.HandleCount, $Proc.ThreadCount,
        $pagefileBytes, (ConvertTo-MemCsvText $detail)
    ) -join ','
}

# 一个 tick：一次 Win32_Process 查询，喂给若干棵树，各自追加到自己的 CSV。
# Targets 是 @{ Label; RootPid; OutCsv; Origin; Scope } 的数组。
function Write-MemSampleTick {
    param($Targets, [datetime]$Now)

    $all = @(Get-CimInstance Win32_Process)
    $alive = 0
    foreach ($target in $Targets) {
        $tree = @(Get-MemProcessTree -All $all -Root $target.RootPid -Scope $target.Scope)
        if ($tree.Count -eq 0) { continue }
        $alive++
        $elapsed = [int][math]::Max(0, ($Now - $target.Origin).TotalSeconds)
        $lines = foreach ($node in $tree) {
            New-MemSampleRow -Proc $node.Proc -Now $Now -ElapsedSec $elapsed -Depth $node.Depth -Label $target.Label
        }
        Add-Content -LiteralPath $target.OutCsv -Value $lines -Encoding utf8NoBOM
    }
    return $alive
}

# CSV 不存在则写表头（存在则追加，便于分段续采）。
function Initialize-MemCsv {
    param([string]$Path)

    $dir = Split-Path -Parent $Path
    if ($dir -and -not (Test-Path -LiteralPath $dir)) { New-Item -ItemType Directory -Path $dir -Force | Out-Null }
    if (-not (Test-Path -LiteralPath $Path)) { Set-Content -LiteralPath $Path -Value (Get-MemCsvHeader) -Encoding utf8NoBOM }
}

Export-ModuleMember -Function Get-MemCsvHeader, ConvertTo-MemCsvText, Get-MemProcessRole, Get-MemProcessDetail, Get-MemProcessTree, New-MemSampleRow, Write-MemSampleTick, Initialize-MemCsv
