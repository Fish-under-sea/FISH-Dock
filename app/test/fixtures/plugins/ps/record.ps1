# How the files beside this one were made: what each example script
# (examples/widgets/*.ps1) posts, run with the web and the clock played by
# this script. Invoke-RestMethod answers from the recorded responses in
# test/fixtures/plugins (and takes the POSTs to /widget), Invoke-WebRequest
# says which dev servers answer each turn, Get-Date is a fake clock that
# Start-Sleep moves on. Each scenario writes { posts: [{ t, body }],
# warnings } to -Out; plugins/*.rs compares its own widgets with them.
#
#   pwsh test/fixtures/plugins/ps/record.ps1 -Scenario pomodoro -Out test/fixtures/plugins/ps/pomodoro.json
#
# Scenarios: weather-zh weather-en weather-none stock-red stock-green ci
# ci-none devserver pomodoro countdown countdown-days countdown-bad stretch
param(
  [Parameter(Mandatory)][string]$Scenario,
  # The repository (this file is four folders down in it).
  [string]$Repo = (Resolve-Path (Join-Path $PSScriptRoot '../../../..')).Path,
  [Parameter(Mandatory)][string]$Out
)
$ErrorActionPreference = 'Stop'
[Console]::OutputEncoding = [Text.Encoding]::UTF8
$fx = Join-Path $Repo 'test/fixtures/plugins'
$ex = Join-Path $Repo 'examples/widgets'
$global:posts = [System.Collections.Generic.List[object]]::new()
$global:t0 = [datetime]'2026-10-18 09:00:00'
$global:now = $global:t0
$global:sleeps = 0
$global:maxSleeps = 0
$global:routes = @{}
$global:turn = 0

function global:Fixture($name) { Get-Content -Raw -Encoding utf8 (Join-Path $fx $name) }

function global:Get-Date { $global:now }
function global:Start-Sleep { param([double]$Seconds)
  $global:sleeps++
  if ($global:sleeps -gt $global:maxSleeps) { throw 'enough' }
  $global:now = $global:now.AddSeconds($Seconds)
  $global:turn++
}

function global:Invoke-RestMethod {
  param([Parameter(Position = 0)][string]$Uri, [string]$Method = 'Get', [string]$ContentType, $Body, $Headers, $TimeoutSec)
  if ($Method -eq 'Post') {
    $text = [Text.Encoding]::UTF8.GetString($Body)
    $global:posts.Add([ordered]@{ t = ($global:now - $global:t0).TotalSeconds; body = ($text | ConvertFrom-Json -AsHashtable) })
    return [pscustomobject]@{ ok = $true }
  }
  foreach ($k in $global:routes.Keys) {
    if ($Uri -like $k) {
      $r = $global:routes[$k]
      if ($r -is [scriptblock]) { $r = & $r $Uri }
      if ($r.status -and $r.status -ge 400) {
        $er = [System.Management.Automation.ErrorRecord]::new([Exception]::new("Response status code does not indicate success: $($r.status)."), 'WebCmdletWebResponseException', 'InvalidOperation', $null)
        $er.ErrorDetails = [System.Management.Automation.ErrorDetails]::new($r.body)
        throw $er
      }
      return ($r.body | ConvertFrom-Json)
    }
  }
  throw "no route for $Uri"
}

function global:Invoke-WebRequest {
  param([Parameter(Position = 0)][string]$Uri, $TimeoutSec, [switch]$UseBasicParsing, $MaximumRedirection)
  $answer = $global:web[$global:turn][$Uri]
  if ($answer -eq 'up') { return [pscustomobject]@{ StatusCode = 200 } }
  $e = [Exception]::new('no')
  if ($answer -eq 'error') { Add-Member -InputObject $e -NotePropertyName Response -NotePropertyValue 'r' }
  throw $e
}

$global:warnings = @()
function Run($path, [scriptblock]$call) { & $call $path 3>&1 | ForEach-Object { if ($_ -is [System.Management.Automation.WarningRecord]) { $global:warnings += $_.Message } } }
try {
  switch ($Scenario) {
    'weather-zh' {
      $global:routes['https://geocoding-api.open-meteo.com/*'] = @{ body = Fixture 'geocode-shanghai-zh.json' }
      $global:routes['https://api.open-meteo.com/*'] = @{ body = Fixture 'forecast-shanghai.json' }
      Run (Join-Path $ex 'weather.ps1') { & $args[0] -City '上海' -Once -Lang zh }
    }
    'weather-en' {
      $global:routes['https://geocoding-api.open-meteo.com/*'] = @{ body = Fixture 'geocode-shanghai-zh.json' }
      $global:routes['https://api.open-meteo.com/*'] = @{ body = Fixture 'forecast-shanghai.json' }
      Run (Join-Path $ex 'weather.ps1') { & $args[0] -Once -Lang en }
    }
    'weather-none' {
      $global:routes['https://geocoding-api.open-meteo.com/*'] = @{ body = Fixture 'geocode-none.json' }
      Run (Join-Path $ex 'weather.ps1') { & $args[0] -City 'Nowhereville' -Once -Lang en }
      Run (Join-Path $ex 'weather.ps1') { & $args[0] -City '无此城' -Once -Lang zh }
    }
    { $_ -like 'stock-*' } {
      $global:routes['*/chart/600519.SS?*'] = @{ body = Fixture 'chart-600519ss.json' }
      $global:routes['*/chart/AAPL?*'] = @{ body = Fixture 'chart-aapl.json' }
      $global:routes['*/chart/BTC-USD?*'] = @{ body = Fixture 'chart-btc.json' }
      $global:routes['*/chart/USDCNY%3DX?*'] = @{ body = Fixture 'chart-usdcny.json' }
      $global:routes['*/chart/NOSUCHSYMBOLX?*'] = @{ status = 404; body = Fixture 'chart-none.json' }
      $sym = '600519.SS,AAPL,NOSUCHSYMBOLX,BTC-USD,USDCNY=X'
      if ($Scenario -eq 'stock-red') { Run (Join-Path $ex 'stock.ps1') { & $args[0] -Symbol $sym -Name '茅台,,坏的,比特币' -RedUp -Once } }
      else { Run (Join-Path $ex 'stock.ps1') { & $args[0] -Symbol $sym -GreenUp -Once } }
    }
    'ci' {
      $global:maxSleeps = 2
      $global:routes['*/actions/runs?*'] = {
        param($u)
        $r = Fixture 'gh-runs.json' | ConvertFrom-Json
        if ($global:turn -ge 1) { $r.workflow_runs[0].status = 'completed'; $r.workflow_runs[0].conclusion = $(if ($global:turn -ge 2) { 'success' } else { 'failure' }) }
        @{ body = ($r | ConvertTo-Json -Depth 5) }
      }
      $global:routes['*/pulls?*'] = @{ body = Fixture 'gh-pulls.json' }
      foreach ($n in 29450, 29448, 29447) {
        $sha = ((Fixture 'gh-pulls.json' | ConvertFrom-Json) | Where-Object number -EQ $n).head.sha
        $global:routes["*/commits/$sha/check-runs?*"] = @{ body = Fixture "gh-checks-$n.json" }
      }
      Run (Join-Path $ex 'ci.ps1') { & $args[0] -Repo 'astral-sh/ruff' -Prs -Token 'fake' }
    }
    'ci-none' {
      $global:routes['*/actions/runs?*'] = @{ body = Fixture 'gh-runs-none.json' }
      $global:routes['*/pulls?*'] = @{ body = '[]' }
      Run (Join-Path $ex 'ci.ps1') { & $args[0] -Repo 'Octo.Cat/Hello_World' -Prs -Token 'fake' -Once }
    }
    'devserver' {
      $global:maxSleeps = 3
      $a = 'http://localhost:3000'; $b = 'http://localhost:5173/api/health'; $c = 'https://example.com'
      $global:web = @(
        @{ $a = 'up'; $b = 'error'; $c = 'down' },
        @{ $a = 'down'; $b = 'up'; $c = 'down' },
        @{ $a = 'down'; $b = 'down'; $c = 'up' },
        @{ $a = 'up'; $b = 'up'; $c = 'up' }
      )
      Run (Join-Path $ex 'devserver.ps1') { & $args[0] -Url "$a,$b,$c" }
    }
    'pomodoro' {
      $global:maxSleeps = 140
      Run (Join-Path $ex 'pomodoro.ps1') { & $args[0] -Work 0.5 -Break 0.25 }
    }
    'countdown' {
      $global:maxSleeps = 110
      $global:now = $global:t0 = [datetime]'2026-10-20 16:58:30'
      Run (Join-Path $ex 'countdown.ps1') { & $args[0] -At '2026-10-20 18:00' -Title '交周报' }
    }
    'countdown-days' {
      $global:maxSleeps = 2
      $global:now = $global:t0 = [datetime]'2026-10-18 09:00:00'
      Run (Join-Path $ex 'countdown.ps1') { & $args[0] -At '2026-12-31' }
    }
    'countdown-bad' {
      $global:maxSleeps = 0
      Run (Join-Path $ex 'countdown.ps1') { & $args[0] -At 'xyz' -Title 'a' }
    }
    'stretch' {
      $global:maxSleeps = 8
      Run (Join-Path $ex 'stretch.ps1') { & $args[0] -Minutes 3 }
    }
  }
} catch {
  if ("$_" -ne 'enough') { $global:warnings += "THROWN: $_" }
}
$result = [ordered]@{ posts = $global:posts; warnings = @($global:warnings | ForEach-Object { "$_" }) }
[IO.File]::WriteAllText($ExecutionContext.SessionState.Path.GetUnresolvedProviderPathFromPSPath($Out), ($result | ConvertTo-Json -Depth 12 -Compress), [Text.UTF8Encoding]::new($false))
