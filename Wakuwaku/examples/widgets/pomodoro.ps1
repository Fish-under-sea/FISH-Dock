# A pomodoro in Wakuwaku's island: focus for a while, then a break, round
# after round; the island shows the time left, and opens when it is time to
# stop or start again. Ctrl+C ends it.
#
#   pwsh examples/widgets/pomodoro.ps1
#   pwsh examples/widgets/pomodoro.ps1 -Work 50 -Break 10 -Rounds 3
param(
  [double]$Work = 25,
  [double]$Break = 5,
  [double]$LongBreak = 15,
  # A long break after every so many rounds.
  [int]$Every = 4,
  # Rounds before it stops; 0, until Ctrl+C.
  [int]$Rounds = 0,
  [int]$Port = 47213
)

function Send($widget) {
  try {
    $body = $widget | ConvertTo-Json -Compress -Depth 6
    Invoke-RestMethod -Method Post "http://127.0.0.1:$Port/widget" -ContentType 'application/json; charset=utf-8' -Body ([Text.Encoding]::UTF8.GetBytes($body)) | Out-Null
  } catch {
    Write-Warning $_
  }
}

# A stretch of time, shown counting down; the island opens at its end. Its
# larger sizes: how much of it is left (a ring: the seconds left of all of
# it), which round of the set it is, and what comes after.
function Phase($label, $color, $minutes, $words, $after, $round) {
  $end = (Get-Date).AddMinutes($minutes)
  $all = [math]::Max(1, [int]($minutes * 60))
  while ($true) {
    $left = $end - (Get-Date)
    if ($left.TotalSeconds -le 0) { break }
    $detail = @{ sizes = @('s', 'm'); extra = $after; series = @{ points = @([int]$left.TotalSeconds); hi = $all; lo = 0; unit = 's' }; rows = @(@{ kind = 'round'; value = "$round"; unit = "$Every" }) }
    Send @{ id = 'pomodoro'; icon = 'timer'; color = $color; label = $label; value = '{0}:{1:ss}' -f [int][math]::Floor($left.TotalMinutes), $left; detail = $detail; ttl = 60 }
    Start-Sleep -Seconds ([math]::Min(10, [math]::Max(1, [math]::Ceiling($left.TotalSeconds))))
  }
  Send @{ id = 'pomodoro'; icon = 'timer'; color = $color; label = $label; value = '0:00'; ttl = 60; nudge = $words }
}

try {
  for ($round = 1; $Rounds -eq 0 -or $round -le $Rounds; $round++) {
    $isLong = $Every -gt 0 -and $round % $Every -eq 0
    $rest = if ($isLong) { $LongBreak } else { $Break }
    $inSet = if ($Every -gt 0) { ($round - 1) % $Every + 1 } else { $round }
    Phase "专注 · 第 $round 轮" '#ff5c6c' $Work "第 $round 轮专注结束，休息 $rest 分钟" "之后休息 $rest 分钟" $inSet
    $isLast = $Rounds -gt 0 -and $round -eq $Rounds
    Phase $(if ($isLong) { '长休息' } else { '休息' }) '#34d27b' $rest $(if ($isLast) { '番茄钟结束了' } else { "休息结束，开始第 $($round + 1) 轮" }) $(if ($isLast) { '之后结束' } else { "之后专注 $Work 分钟" }) $inSet
  }
} finally {
  Send @{ id = 'pomodoro'; remove = $true }
}
