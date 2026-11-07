# 精简更新包失败后的兜底：把完整安装包（Setup）下载下来并运行它。
#
# 为什么是它：install-update.cjs 的事务失败时已经把旧版本原样恢复，但那台机器上
# 已经没有可用的更新界面了（安装器是静默 /S，页面不显示）。这里用一个**不依赖
# WebView2** 的 WinForms 窗口把进度说清楚：下载 -> 校验 -> 启动 Setup 覆盖安装。
#
# 由 installer.mjs 生成的 NSIS 在「事务失败」分支里调用：
#   powershell.exe -NoProfile -ExecutionPolicy Bypass -WindowStyle Hidden \
#     -File "%TEMP%\fish-dock-update-fallback.ps1" -Url <Setup 地址> -Sha256 <摘要> \
#     -Version <版本> -Bytes <字节数> -Page <发行页>
#
# 退出码：0 = 已启动 Setup；1 = 失败（窗口里已说明原因）；2 = 用户取消。
# 日志：%TEMP%\fish-dock-update-fallback.log（只追加，下一次运行前不清空）。
param(
  [Parameter(Mandatory = $true)][string]$Url,
  [Parameter(Mandatory = $true)][string]$Sha256,
  [Parameter(Mandatory = $true)][string]$Version,
  [long]$Bytes = 0,
  [string]$Page = '',
  # 窗口图标：传主程序（或 .ico）的路径，用她自己的那个，而不是 powershell.exe 的。
  [string]$Icon = ''
)

$ErrorActionPreference = 'Stop'
# 先装程序集再碰它们：Windows PowerShell 5.1 默认不加载 WinForms，顺序反了第一行
# 就抛「找不到类型」，连日志都写不出来。
Add-Type -AssemblyName System.Windows.Forms, System.Drawing
# HttpClient 在 5.1 里也要显式装：它不在默认加载的那几个程序集里。
try { Add-Type -AssemblyName System.Net.Http } catch {}
[System.Windows.Forms.Application]::EnableVisualStyles()

$log = Join-Path $env:TEMP 'fish-dock-update-fallback.log'
function Say([string]$line) {
  try { Add-Content -LiteralPath $log -Value ('{0} {1}' -f (Get-Date).ToString('yyyy-MM-dd HH:mm:ss'), $line) -Encoding UTF8 } catch {}
}
Say "start: version=$Version bytes=$Bytes url=$Url"

# 同一个用户只跑一个：两个窗口同时往同一个 .part 里写只会互相破坏。
$mutex = New-Object System.Threading.Mutex($false, ('Local\FISH-Dock-update-fallback-{0}' -f $env:USERNAME))
$owned = $false
try { $owned = $mutex.WaitOne(0) } catch [System.Threading.AbandonedMutexException] { $owned = $true }
if (-not $owned) { Say 'another fallback is already running'; exit 1 }

$setup = Join-Path $env:TEMP ('FISH-Dock-Setup-{0}.exe' -f $Version)
$part = "$setup.part"

# ── 窗口 ────────────────────────────────────────────────────────────────
$ink = [System.Drawing.Color]::FromArgb(24, 26, 32)
$paper = [System.Drawing.Color]::FromArgb(247, 248, 250)
$accent = [System.Drawing.Color]::FromArgb(56, 132, 255)
$muted = [System.Drawing.Color]::FromArgb(110, 116, 128)

$form = New-Object System.Windows.Forms.Form
$form.Text = 'FISH Dock 更新修复'
$form.ClientSize = New-Object System.Drawing.Size(500, 288)
$form.FormBorderStyle = 'FixedSingle'
$form.MaximizeBox = $false
$form.MinimizeBox = $false
$form.StartPosition = 'CenterScreen'
$form.TopMost = $true
$form.BackColor = $paper
$form.Font = New-Object System.Drawing.Font('Microsoft YaHei UI', 9)

# 窗口图标用她自己的：从安装目录里的主程序取，比自己嵌一份 ico 更不会走样。
# 取不到就留默认——图标是锦上添花，不该让兜底因为图标失败。
if ($Icon -and (Test-Path -LiteralPath $Icon)) {
  try {
    if ($Icon -match '\.ico$') { $form.Icon = New-Object System.Drawing.Icon($Icon) }
    else { $form.Icon = [System.Drawing.Icon]::ExtractAssociatedIcon($Icon) }
  } catch { Say "icon: $($_.Exception.Message)" }
}

$head = New-Object System.Windows.Forms.Panel
$head.Dock = 'Top'
$head.Height = 64
$head.BackColor = $ink
$form.Controls.Add($head)

$mark = New-Object System.Windows.Forms.PictureBox
$mark.Location = New-Object System.Drawing.Point(16, 14)
$mark.Size = New-Object System.Drawing.Size(36, 36)
$mark.SizeMode = 'Zoom'
$mark.BackColor = [System.Drawing.Color]::Transparent
if ($form.Icon) {
  try { $mark.Image = $form.Icon.ToBitmap() } catch {}
}
$head.Controls.Add($mark)

$title = New-Object System.Windows.Forms.Label
$title.Text = '正在获取完整安装包'
$title.ForeColor = [System.Drawing.Color]::White
$title.Font = New-Object System.Drawing.Font('Microsoft YaHei UI', 12, [System.Drawing.FontStyle]::Bold)
$title.AutoSize = $true
$title.Location = New-Object System.Drawing.Point(64, 12)
$head.Controls.Add($title)

$subtitle = New-Object System.Windows.Forms.Label
$subtitle.Text = ('增量更新没有完成，已自动改用完整安装包  ·  v{0}' -f $Version)
$subtitle.ForeColor = [System.Drawing.Color]::FromArgb(170, 178, 192)
$subtitle.AutoSize = $true
$subtitle.Location = New-Object System.Drawing.Point(66, 38)
$head.Controls.Add($subtitle)

$bar = New-Object System.Windows.Forms.ProgressBar
$bar.Location = New-Object System.Drawing.Point(20, 84)
$bar.Size = New-Object System.Drawing.Size(460, 14)
$bar.Style = 'Continuous'
$bar.Minimum = 0
$bar.Maximum = 1000
$form.Controls.Add($bar)

# 文字区一行一行排开、各有各的高度，谁也不压谁：之前失败信息挤进 22 像素高的
# 那一行，被裁成两截（用户截图里的「字被折叠」就是这么来的）。
$detail = New-Object System.Windows.Forms.Label
$detail.Text = '正在连接…'
$detail.ForeColor = $muted
$detail.AutoSize = $false
$detail.Location = New-Object System.Drawing.Point(20, 106)
$detail.Size = New-Object System.Drawing.Size(460, 24)
$form.Controls.Add($detail)

$note = New-Object System.Windows.Forms.Label
$note.Text = '下载完成后会校验摘要并自动启动安装程序；取消不会影响现在能用的版本。'
$note.ForeColor = $muted
$note.AutoSize = $false
$note.Location = New-Object System.Drawing.Point(20, 132)
$note.Size = New-Object System.Drawing.Size(460, 40)
$form.Controls.Add($note)

$reason = New-Object System.Windows.Forms.Label
$reason.Text = ''
$reason.ForeColor = [System.Drawing.Color]::FromArgb(184, 58, 58)
$reason.AutoSize = $false
$reason.Visible = $false
$reason.Location = New-Object System.Drawing.Point(20, 176)
$reason.Size = New-Object System.Drawing.Size(460, 56)
$form.Controls.Add($reason)

$cancel = New-Object System.Windows.Forms.Button
$cancel.Text = '取消'
$cancel.Size = New-Object System.Drawing.Size(96, 32)
$cancel.Location = New-Object System.Drawing.Point(384, 240)
$cancel.FlatStyle = 'System'
$form.Controls.Add($cancel)

$retry = New-Object System.Windows.Forms.Button
$retry.Text = '重试'
$retry.Size = New-Object System.Drawing.Size(96, 32)
$retry.Location = New-Object System.Drawing.Point(280, 240)
$retry.Visible = $false
$form.Controls.Add($retry)

$open = New-Object System.Windows.Forms.Button
$open.Text = '手动下载'
$open.Size = New-Object System.Drawing.Size(96, 32)
$open.Location = New-Object System.Drawing.Point(176, 240)
$open.Visible = $false
$form.Controls.Add($open)

$state = @{ cancelled = $false; retry = $false; exit = 1 }
$cancel.Add_Click({ $state.cancelled = $true; $cancel.Enabled = $false; $detail.Text = '正在取消…' })
$retry.Add_Click({ $state.retry = $true })
$open.Add_Click({
  if ($Page) { try { Start-Process $Page } catch {} }
})

function Pump { [System.Windows.Forms.Application]::DoEvents() }

# 一个文件的小写十六进制 SHA-256。不依赖任何 PowerShell 模块。
function Get-Sha256([string]$path) {
  $stream = [System.IO.File]::OpenRead($path)
  try {
    $sha = [System.Security.Cryptography.SHA256]::Create()
    try { $bytes = $sha.ComputeHash($stream) } finally { $sha.Dispose() }
  } finally { $stream.Dispose() }
  return ([System.BitConverter]::ToString($bytes) -replace '-', '').ToLowerInvariant()
}

function Human([double]$value) {
  if ($value -ge 1GB) { return ('{0:N2} GB' -f ($value / 1GB)) }
  if ($value -ge 1MB) { return ('{0:N1} MB' -f ($value / 1MB)) }
  if ($value -ge 1KB) { return ('{0:N0} KB' -f ($value / 1KB)) }
  return ('{0:N0} B' -f $value)
}

function Span([double]$seconds) {
  if ($seconds -lt 0 -or [double]::IsInfinity($seconds) -or [double]::IsNaN($seconds)) { return '' }
  if ($seconds -lt 60) { return ('{0:N0} 秒' -f [math]::Ceiling($seconds)) }
  if ($seconds -lt 3600) { return ('{0:N0} 分 {1:N0} 秒' -f [math]::Floor($seconds / 60), ($seconds % 60)) }
  return ('{0:N0} 小时 {1:N0} 分' -f [math]::Floor($seconds / 3600), [math]::Floor(($seconds % 3600) / 60))
}

# 一次下载：先直连，失败再试系统代理（代理坏了很常见，见 2.1.8 的断网修复）。
# 支持续传：.part 已有一截就用 Range 接着要，服务器不认 206 就从头来。
function Fetch([bool]$useProxy) {
  $handler = New-Object System.Net.Http.HttpClientHandler
  $handler.UseProxy = $useProxy
  $handler.AllowAutoRedirect = $true
  $client = New-Object System.Net.Http.HttpClient($handler)
  $client.Timeout = [TimeSpan]::FromMinutes(30)
  try {
    $offset = 0L
    if (Test-Path -LiteralPath $part) { $offset = (Get-Item -LiteralPath $part).Length }
    $request = New-Object System.Net.Http.HttpRequestMessage('GET', $Url)
    if ($offset -gt 0) { $request.Headers.Range = New-Object System.Net.Http.Headers.RangeHeaderValue($offset, $null) }
    $request.Headers.UserAgent.ParseAdd('FISH-Dock-Fallback/1')
    $response = $client.SendAsync($request, [System.Net.Http.HttpCompletionOption]::ResponseHeadersRead).GetAwaiter().GetResult()
    if (-not $response.IsSuccessStatusCode) {
      throw ('服务器返回 {0} {1}' -f [int]$response.StatusCode, $response.ReasonPhrase)
    }
    $total = 0L
    if ($response.Content.Headers.ContentLength) { $total = $offset + [long]$response.Content.Headers.ContentLength }
    if ($total -le 0 -and $Bytes -gt 0) { $total = $Bytes }
    $mode = if ($offset -gt 0 -and $response.StatusCode -eq [System.Net.HttpStatusCode]::PartialContent) { 'Append' } else { 'Create' }
    if ($mode -eq 'Create') { $offset = 0L }
    $stream = $response.Content.ReadAsStreamAsync().GetAwaiter().GetResult()
    $file = [System.IO.File]::Open($part, [System.IO.FileMode]$mode, [System.IO.FileAccess]::Write, [System.IO.FileShare]::None)
    try {
      $buffer = New-Object byte[] 65536
      $done = $offset
      $mark = Get-Date
      $markBytes = $done
      $speed = 0.0
      $last = Get-Date
      while ($true) {
        if ($state.cancelled) { throw [System.OperationCanceledException]::new() }
        $read = $stream.Read($buffer, 0, $buffer.Length)
        if ($read -le 0) { break }
        $file.Write($buffer, 0, $read)
        $done += $read
        $now = Get-Date
        if (($now - $last).TotalMilliseconds -ge 120) {
          $last = $now
          $elapsed = ($now - $mark).TotalSeconds
          if ($elapsed -ge 0.6) { $speed = ($done - $markBytes) / $elapsed; $mark = $now; $markBytes = $done }
          if ($total -gt 0) {
            $percent = [math]::Min(1000, [math]::Floor($done * 1000 / $total))
            $bar.Value = [int]$percent
            $eta = if ($speed -gt 0) { ($total - $done) / $speed } else { -1 }
            $detail.Text = ('已下载 {0:N0}%  ·  {1} / {2}  ·  {3}/s  ·  剩余 {4}' -f ($done * 100 / $total), (Human $done), (Human $total), (Human $speed), (Span $eta))
          } else {
            $bar.Style = 'Marquee'
            $detail.Text = ('已下载 {0}  ·  {1}/s' -f (Human $done), (Human $speed))
          }
          Pump
        }
      }
      $file.Flush()
    } finally { $file.Dispose() }
    if ($state.cancelled) { throw [System.OperationCanceledException]::new() }
    if ($total -gt 0 -and (Get-Item -LiteralPath $part).Length -ne $total) {
      throw ('下载不完整：{0} / {1}' -f (Get-Item -LiteralPath $part).Length, $total)
    }
    $bar.Value = 1000
    $detail.Text = '校验安装包…'
    Pump
    # 摘要用 .NET 直接算，不走 Get-FileHash：那个 cmdlet 要加载
    # Microsoft.PowerShell.Utility 模块，而兜底跑在某些环境里 PSModulePath 是被
    # 别的东西带进来的（实测过），取不到模块就会把「校验」错报成「没这个命令」。
    $hash = (Get-Sha256 $part)
    if ($hash -ne $Sha256.ToLowerInvariant()) {
      Remove-Item -LiteralPath $part -Force -ErrorAction SilentlyContinue
      throw ('摘要不符（得到 {0}，应为 {1}）' -f $hash, $Sha256)
    }
    Move-Item -LiteralPath $part -Destination $setup -Force
    return $true
  } finally {
    $client.Dispose()
  }
}

function Fail([string]$message) {
  Say "failed: $message"
  $bar.Style = 'Continuous'
  $bar.Value = 0
  $title.Text = '没能自动完成'
  # 失败原因单独一块（红），不再挤进上面那一行：长句要能折行、不能被裁掉。
  $detail.Text = '完整安装包没有下载成功。'
  $detail.ForeColor = $muted
  $reason.Text = $message
  $reason.Visible = $true
  $note.Text = ('可以点「重试」再来一次，或「手动下载」到发行页自己取完整安装包。日志：{0}' -f $log)
  $note.Visible = $true
  $retry.Visible = $true
  $open.Visible = ($Page -ne '')
  $cancel.Text = '关闭'
  Pump
}

$form.Add_Shown({
  $state.retry = $false
  $attempt = 0
  while ($true) {
    $attempt++
    $ok = $false
    $failure = ''
    foreach ($proxy in @($false, $true)) {
      try {
        $bar.Style = 'Continuous'
        $detail.ForeColor = $muted
        $detail.Text = if ($proxy) { '直连失败，改用系统代理重试…' } else { '正在连接…' }
        Pump
        if (Fetch $proxy) { $ok = $true; break }
      } catch [System.OperationCanceledException] {
        $state.exit = 2
        Say 'cancelled'
        $form.Close()
        return
      } catch {
        $failure = $_.Exception.Message
        Say ('attempt {0} proxy={1}: {2}' -f $attempt, $proxy, $failure)
        Start-Sleep -Milliseconds 600
      }
    }
    if ($ok) { break }
    Fail $failure
    $retry.Visible = $true
    while (-not $state.retry -and -not $state.cancelled) { Pump; Start-Sleep -Milliseconds 80 }
    if ($state.cancelled) { $state.exit = 2; Say 'cancelled'; $form.Close(); return }
    $state.retry = $false
    $retry.Visible = $false
    $reason.Visible = $false
    $note.Visible = $true
    $title.Text = '正在获取完整安装包'
    $detail.Text = '正在连接…'
    $detail.ForeColor = $muted
  }
  Say "verified: $setup"
  $title.Text = '正在启动安装程序'
  $detail.Text = '完整安装包已就绪，安装程序马上接管。'
  $bar.Value = 1000
  Pump
  try {
    Start-Process -FilePath $setup | Out-Null
    $state.exit = 0
    Say 'setup started'
  } catch {
    Fail ('安装程序启动失败：{0}' -f $_.Exception.Message)
    $state.exit = 1
    return
  }
  Start-Sleep -Milliseconds 400
  $form.Close()
})

[void]$form.ShowDialog()
$form.Dispose()
try { if ($owned) { $mutex.ReleaseMutex() } } catch {}
$mutex.Dispose()
Say ('exit {0}' -f $state.exit)
exit $state.exit