# 安装器在干净 Windows 使用系统 PowerShell 执行；不要求用户预装 pwsh。
param([Parameter(Mandatory=$true)][string]$InstallRoot)
$ErrorActionPreference='Stop'
$root=[IO.Path]::GetFullPath($InstallRoot).TrimEnd('\')
$owned=@(Get-CimInstance Win32_Process | Where-Object {$_.ExecutablePath -and ([IO.Path]::GetDirectoryName($_.ExecutablePath) -eq $root) -and $_.Name -in @('DSH-FISH.exe','wakuwaku.exe')})
$descendants=@();$parents=@($owned | ForEach-Object {[int]$_.ProcessId})
$all=@(Get-CimInstance Win32_Process)
while($parents.Count -gt 0){$children=@($all|Where-Object {$parents -contains [int]$_.ParentProcessId});$descendants+=$children;$parents=@($children|ForEach-Object {[int]$_.ProcessId})}
foreach($app in $owned){
 $ports=@(Get-NetTCPConnection -State Listen -OwningProcess $app.ProcessId -ErrorAction SilentlyContinue|Where-Object {$_.LocalAddress -eq '127.0.0.1'})
 foreach($port in $ports){try{$health=Invoke-RestMethod -Uri ('http://127.0.0.1:{0}/health' -f $port.LocalPort) -TimeoutSec 2;if($health.app -eq 'wakuwaku' -and $health.runtime -eq 'tauri'){Invoke-RestMethod -Uri ('http://127.0.0.1:{0}/quit' -f $port.LocalPort) -Method Post -TimeoutSec 2|Out-Null;break}}catch{}}
 $live=Get-Process -Id $app.ProcessId -ErrorAction SilentlyContinue
 if($live){try{Wait-Process -Id $app.ProcessId -Timeout 12 -ErrorAction Stop}catch{
  $still=Get-CimInstance Win32_Process -Filter ('ProcessId={0}' -f $app.ProcessId)
  if($still -and $still.ExecutablePath -eq $app.ExecutablePath -and $still.CreationDate -eq $app.CreationDate){Stop-Process -Id $app.ProcessId -Force -ErrorAction Stop}
 }}
}
foreach($child in $descendants){$still=Get-CimInstance Win32_Process -Filter ('ProcessId={0}' -f $child.ProcessId);if($still -and $still.CreationDate -eq $child.CreationDate -and $still.ExecutablePath -eq $child.ExecutablePath){Stop-Process -Id $child.ProcessId -Force -ErrorAction Stop}}
if(@(Get-CimInstance Win32_Process|Where-Object {$_.ExecutablePath -and ([IO.Path]::GetDirectoryName($_.ExecutablePath) -eq $root) -and $_.Name -in @('DSH-FISH.exe','wakuwaku.exe')}).Count){throw '安装目录仍有程序运行，请退出该实例后重试'}
