# Stocks, exchange rates and coins in Wakuwaku's island: each symbol's price
# and the day's change, from Yahoo Finance's chart data (unofficial, no key
# needed), every 5 minutes. One widget a symbol.
#   Shanghai and Shenzhen shares end in .SS and .SZ: 600519.SS, 000001.SZ
#   exchange rates: USDCNY=X, EURCNY=X; coins: BTC-USD, ETH-USD
#
#   pwsh examples/widgets/stock.ps1 -Symbol 600519.SS -Name 茅台
#   pwsh examples/widgets/stock.ps1 -Symbol USDCNY=X,BTC-USD -Name 美元,比特币
#   pwsh examples/widgets/stock.ps1 -Symbol AAPL -Once
#
# Up is red and down green, as Chinese markets show them; -GreenUp turns it
# round (-RedUp says the first). Run by her, it is given one of the two, the
# Plugins page's colours (as her language has it until chosen). The colour
# it sends is the one place that says so: every place the widget is drawn
# follows it (wcards.js trendOf).
[CmdletBinding(PositionalBinding = $false)]
param(
  [string[]]$Symbol = @('AAPL'),
  [string[]]$Name = @(),
  [double]$Minutes = 5,
  [switch]$RedUp,
  [switch]$GreenUp,
  [int]$Port = 47213,
  [switch]$Once
)

# From cmd or bash, "-Symbol a,b" comes in as one word: the parts.
$Symbol = @($Symbol | ForEach-Object { $_ -split ',' } | ForEach-Object { $_.Trim() } | Where-Object { $_ })
$Name = @($Name | ForEach-Object { $_ -split ',' } | ForEach-Object { $_.Trim() })

function Send($widget) {
  $body = $widget | ConvertTo-Json -Compress -Depth 6
  Invoke-RestMethod -Method Post "http://127.0.0.1:$Port/widget" -ContentType 'application/json; charset=utf-8' -Body ([Text.Encoding]::UTF8.GetBytes($body)) | Out-Null
}

# Few digits for big prices, more for small ones (a rate like 7.1234).
function Price([double]$p) {
  if ($p -ge 1000) { '{0:N0}' -f $p } elseif ($p -ge 10) { '{0:N2}' -f $p } else { '{0:N4}' -f $p }
}

# A count in a few characters: 812, 48.1K, 48.1M, 1.2B.
function Short([double]$n) {
  foreach ($u in @(@(1e9, 'B'), @(1e6, 'M'), @(1e3, 'K'))) { if ($n -ge $u[0]) { return '{0:0.#}{1}' -f ($n / $u[0]), $u[1] } }
  '{0:0}' -f $n
}

# Green up only when asked for, and not red up as well.
$green = $GreenUp -and -not $RedUp

while ($true) {
  for ($i = 0; $i -lt $Symbol.Count; $i++) {
    $s = $Symbol[$i]
    try {
      # The day in five-minute steps: its price now and the curve of the
      # day for the larger sizes (the open island's card), one request.
      $chart = (Invoke-RestMethod "https://query1.finance.yahoo.com/v8/finance/chart/$([uri]::EscapeDataString($s))?range=1d&interval=5m" -Headers @{ 'User-Agent' = 'Mozilla/5.0' } -TimeoutSec 20).chart.result[0]
      $meta = $chart.meta
      $price = [double]$meta.regularMarketPrice
      $before = [double]$meta.chartPreviousClose
      $change = if ($before) { ($price - $before) / $before * 100 } else { 0 }
      $isUp = $change -ge 0
      $icon = if ($s -match '-USD$|-CNY$') { 'coin' } elseif ($s -match '=X$') { 'coin' } else { 'stock' }
      $label = $(if ($i -lt $Name.Count -and $Name[$i]) { $Name[$i] } else { $s })
      # Its prices that day, 120 at most (a coin's day has 288: every
      # third), the last always.
      $closes = @(@($chart.indicators.quote)[0].close | Where-Object { $null -ne $_ } | ForEach-Object { [double]$_ })
      $step = [Math]::Max(1, [Math]::Ceiling($closes.Count / 120))
      $points = @(for ($k = ($closes.Count - 1) % $step; $k -lt $closes.Count; $k += $step) { [Math]::Round($closes[$k], 4) })
      $row = @{ code = $s; name = $label; price = Price $price; change = '{0}{1:N2}%' -f $(if ($isUp) { '+' } else { '−' }), [math]::Abs($change) }
      if ($meta.regularMarketVolume) { $row.value = Short ([double]$meta.regularMarketVolume) }
      $detail = @{ sizes = @('s', 'm', 'l'); rows = @($row) }
      if ($points.Count -gt 1) {
        $detail.series = @{ points = $points }
        if ($meta.regularMarketDayHigh) { $detail.series.hi = [double]$meta.regularMarketDayHigh }
        if ($meta.regularMarketDayLow) { $detail.series.lo = [double]$meta.regularMarketDayLow }
      }
      Send @{
        id    = "stock-$($s.ToLower() -replace '[^a-z0-9_-]', '-')"
        icon  = $icon
        color = $(if ($isUp -xor $green) { '#ff5c6c' } else { '#34d27b' })
        label = $label
        value = '{0} {1}{2:N2}%' -f (Price $price), $(if ($isUp) { '▲' } else { '▼' }), [math]::Abs($change)
        detail = $detail
        ttl   = [int]($Minutes * 60) + 120
      }
    } catch {
      Write-Warning "${s}: $_"
    }
  }
  if ($Once) { break }
  Start-Sleep -Seconds ([int]($Minutes * 60))
}
