<#
.SYNOPSIS
    FISH Dock 内存采样器（单进程树）：按进程逐条记录 RSS/私有工作集/句柄/线程。

.DESCRIPTION
    红线《禁止屎山十条》内存预算一节要求「必须实测，不许估」，并且要逐进程
    列表而不是一个总数。本脚本是采样核心（MemSample.psm1）的单根命令行入口：
    把一个根进程（DSH-FISH.exe / wakuwaku.exe）连同它的整棵子树（WebView2 的
    browser / renderer / gpu-process / utility / crashpad-handler，以及插件派生
    的 node 后端与插件自带 runtime）按固定间隔落一份 CSV。

    改前/改后的同条件对照用 mem-ab.ps1（同一个核心，两棵树一起采）。
    CSV 的解读与三时段对照表用 mem-report.mjs。

.PARAMETER RootPid
    根进程 id（宿主 exe）。

.PARAMETER OutCsv
    输出 CSV。不存在则写表头，存在则追加（便于分段续采）。

.PARAMETER IntervalSec
    采样间隔秒数，默认 20s：足够密以看清 5/30/60 分钟三个点，又不会让 WMI
    查询本身变成负载。

.PARAMETER DurationSec
    总时长秒数。到点退出；根进程提前消失也退出并写明原因。

.PARAMETER Label
    写进每行的标签，用于区分「改前/改后」「有插件/无插件」等不同条件。

.PARAMETER Scope
    host = 只统计宿主进程自己；tree = 整棵子树（默认，含 WebView2 与插件进程）。

.EXAMPLE
    pwsh -File scripts/mem-sample.ps1 -RootPid 31388 -OutCsv build/mem/installed.csv -Label installed-plugins-on -DurationSec 4200
#>
[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)][int]$RootPid,
    [Parameter(Mandatory = $true)][string]$OutCsv,
    [int]$IntervalSec = 20,
    [int]$DurationSec = 4200,
    [string]$Label = 'run',
    [ValidateSet('host', 'tree')][string]$Scope = 'tree'
)

$ErrorActionPreference = 'Stop'
Import-Module (Join-Path $PSScriptRoot 'MemSample.psm1') -Force

if ($IntervalSec -lt 1) { throw "IntervalSec 必须 >= 1（收到 $IntervalSec）" }
if ($DurationSec -lt $IntervalSec) { throw "DurationSec($DurationSec) 不能小于 IntervalSec($IntervalSec)" }

# 进程年龄原点：用根进程真实启动时间，采样器晚启动也能对齐 5/30/60 分钟点。
$rootInfo = Get-CimInstance Win32_Process -Filter "ProcessId=$RootPid" | Select-Object -First 1
if (-not $rootInfo) { throw "根进程 $RootPid 不存在，无法采样" }
if (-not $rootInfo.CreationDate) { throw "根进程 $RootPid 读不到 CreationDate" }

Initialize-MemCsv -Path $OutCsv
$target = @{ Label = $Label; RootPid = $RootPid; OutCsv = $OutCsv; Origin = $rootInfo.CreationDate; Scope = $Scope }

$deadline = (Get-Date).AddSeconds($DurationSec)
$tick = 0

while ($true) {
    $tickStart = Get-Date
    if ($tickStart -ge $deadline) { break }
    if ((Write-MemSampleTick -Targets @($target) -Now $tickStart) -eq 0) {
        Write-Warning "根进程 $RootPid 已消失，采样在第 $tick 个 tick 结束"
        break
    }
    $tick++
    # 睡眠把剩余时长一起算进去：既不睡过 deadline，也不会在最后几秒空转刷样本。
    $wait = [math]::Min($IntervalSec - ((Get-Date) - $tickStart).TotalSeconds, ($deadline - (Get-Date)).TotalSeconds)
    if ($wait -le 0) { break }
    Start-Sleep -Seconds $wait
}

Write-Host ("采样结束：label={0} root={1} ticks={2} scope={3} 起点={4:o} 输出={5}" -f $Label, $RootPid, $tick, $Scope, $target.Origin, $OutCsv)
