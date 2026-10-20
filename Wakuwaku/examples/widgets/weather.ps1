# A weather widget for Wakuwaku's island: the sky and the temperature in a
# city, and today's lowest and highest (shown when the island is open), from
# Open-Meteo (no key needed), every 10 minutes. The city can be named in
# English or Chinese. Its larger sizes (the open island's card): the next
# day's temperatures hour by hour as a curve, every second hour with its sky,
# tomorrow and the day after; all from the same one request.
#
#   pwsh examples/widgets/weather.ps1 -City Shanghai
#   pwsh examples/widgets/weather.ps1 -City 杭州 -Once
#   pwsh examples/widgets/weather.ps1 -City Berlin -Lang en
param(
  [string]$City = 'Shanghai',
  [int]$Port = 47213,
  [int]$Minutes = 10,
  [switch]$Once,
  # zh or en: Chinese if Windows' language or its formats are.
  [string]$Lang = $(if ((Get-UICulture).Name -like 'zh*' -or (Get-Culture).Name -like 'zh*') { 'zh' } else { 'en' })
)

# The sky by its WMO code, in Chinese and in English.
$sky = @{
  0 = '晴', 'Clear'; 1 = '晴', 'Mostly clear'; 2 = '多云', 'Partly cloudy'; 3 = '阴', 'Overcast'
  45 = '雾', 'Fog'; 48 = '雾', 'Fog'
  51 = '毛毛雨', 'Drizzle'; 53 = '毛毛雨', 'Drizzle'; 55 = '毛毛雨', 'Drizzle'; 56 = '冻雨', 'Freezing drizzle'; 57 = '冻雨', 'Freezing drizzle'
  61 = '小雨', 'Light rain'; 63 = '中雨', 'Rain'; 65 = '大雨', 'Heavy rain'; 66 = '冻雨', 'Freezing rain'; 67 = '冻雨', 'Freezing rain'
  71 = '小雪', 'Light snow'; 73 = '中雪', 'Snow'; 75 = '大雪', 'Heavy snow'; 77 = '雪粒', 'Snow grains'
  80 = '阵雨', 'Showers'; 81 = '阵雨', 'Showers'; 82 = '强阵雨', 'Heavy showers'; 85 = '阵雪', 'Snow showers'; 86 = '阵雪', 'Snow showers'
  95 = '雷阵雨', 'Thunderstorm'; 96 = '雷阵雨伴冰雹', 'Thunderstorm, hail'; 99 = '雷阵雨伴冰雹', 'Thunderstorm, hail'
}

# The sky by its WMO code as one of the island's weather icons (wcards.js:
# Windows' own glyphs), by day or by night.
function SkyIcon($code, $isDay) {
  $c = [int]$code
  if ($c -le 1) { if ($isDay -eq 0) { 'moon' } else { 'sun' } }
  elseif ($c -eq 2) { if ($isDay -eq 0) { 'moon' } else { 'partly' } }
  elseif ($c -eq 3) { 'cloud' }
  elseif ($c -in 45, 48) { 'fog' }
  elseif ($c -in 71, 73, 75, 77, 85, 86) { 'snow' }
  elseif ($c -ge 95) { 'storm' }
  else { 'rain' }
}
function SkyWords($code) {
  $w = $sky[[int]$code]
  if (-not $w) { '' } elseif ($Lang -eq 'zh') { $w[0] } else { $w[1] }
}
function Deg($t) { "$([Math]::Round([double]$t))°" }
$dash = if ($Lang -eq 'zh') { '~' } else { '–' }

# Where the city is: found once, again next time if the network was down.
$place = $null
while ($true) {
  try {
    if (-not $place) {
      $found = Invoke-RestMethod "https://geocoding-api.open-meteo.com/v1/search?name=$([uri]::EscapeDataString($City))&count=1&language=$Lang" -TimeoutSec 20
      $place = $found.results | Select-Object -First 1
      if (-not $place) { throw $(if ($Lang -eq 'zh') { "找不到这个城市：$City" } else { "no city called $City" }) }
    }
    # Now, today's lowest and highest there (its own time zone), and for the
    # card the hours ahead and the next two days: one request.
    $got = Invoke-RestMethod "https://api.open-meteo.com/v1/forecast?latitude=$($place.latitude)&longitude=$($place.longitude)&current=temperature_2m,weather_code,is_day&hourly=temperature_2m,weather_code,is_day&daily=temperature_2m_min,temperature_2m_max,weather_code&timezone=auto&forecast_days=3" -TimeoutSec 20
    $now = $got.current
    $low, $high = @($got.daily.temperature_2m_min)[0], @($got.daily.temperature_2m_max)[0]
    # The range, shown after the temperature in the open island only.
    $range = if ($null -ne $low -and $null -ne $high) { "$(Deg $low)$dash$(Deg $high)" } else { '' }
    $words = SkyWords $now.weather_code
    # The hours from this one on: the next 24 as a curve, every second one
    # (seven) with its sky; then tomorrow and the day after.
    $times = @($got.hourly.time)
    $first = [Array]::IndexOf($times, "$("$($now.time)".Substring(0, 13)):00")
    $rows = @()
    $series = @()
    if ($first -ge 0) {
      $temps = @($got.hourly.temperature_2m)
      $series = @($temps[$first..([Math]::Min($times.Count, $first + 24) - 1)] | Where-Object { $null -ne $_ } | ForEach-Object { [Math]::Round([double]$_, 1) })
      for ($i = $first; $i -le $first + 12 -and $i -lt $times.Count; $i += 2) {
        $hour = [int]"$($times[$i])".Substring(11, 2)
        $rows += @{ kind = 'hour'; when = $(if ($Lang -eq 'zh') { "${hour}时" } else { '{0:00}:00' -f $hour }); icon = SkyIcon (@($got.hourly.weather_code)[$i]) (@($got.hourly.is_day)[$i]); value = Deg $temps[$i] }
      }
    }
    $days = @($got.daily.time)
    for ($d = 1; $d -le 2 -and $d -lt $days.Count; $d++) {
      $when = if ($Lang -eq 'zh') { @('', '明天', '后天')[$d] } else { if ($d -eq 1) { 'Tomorrow' } else { ([datetime]$days[$d]).ToString('dddd', [Globalization.CultureInfo]::InvariantCulture) } }
      $code = @($got.daily.weather_code)[$d]
      $range2 = "$(Deg (@($got.daily.temperature_2m_min)[$d]))$dash$(Deg (@($got.daily.temperature_2m_max)[$d]))"
      $rows += @{ kind = 'day'; when = $when; icon = SkyIcon $code 1; value = $range2; name = SkyWords $code }
    }
    $detail = @{ sizes = @('s', 'm', 'l'); extra = $range; rows = $rows }
    if ($series.Count) { $detail.series = @{ points = $series; unit = '°' } }
    $widget = @{
      id    = 'weather'
      icon  = 'weather'
      color = '#ffb340'
      label = "$City · $words".TrimEnd(' ', '·')
      value = Deg $now.temperature_2m
      detail = $detail
      # A little longer than the wait, so it stays between two updates.
      ttl   = $Minutes * 60 + 120
    } | ConvertTo-Json -Compress -Depth 6
    # As UTF-8 bytes: Windows PowerShell 5 would send a string in another encoding.
    Invoke-RestMethod -Method Post "http://127.0.0.1:$Port/widget" -ContentType 'application/json; charset=utf-8' -Body ([Text.Encoding]::UTF8.GetBytes($widget)) | Out-Null
  } catch {
    Write-Warning $_
  }
  if ($Once) { break }
  Start-Sleep -Seconds ($Minutes * 60)
}
