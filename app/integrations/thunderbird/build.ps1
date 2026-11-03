# Packs the Thunderbird extension: integrations/thunderbird/fish-dock-mail.xpi
# (an .xpi is a zip with the manifest at its top).
$here = $PSScriptRoot
$zip = Join-Path $here 'fish-dock-mail.zip'
$xpi = Join-Path $here 'fish-dock-mail.xpi'
Remove-Item $zip, $xpi -ErrorAction SilentlyContinue
Compress-Archive -Path (Join-Path $here 'fish-dock-mail\*') -DestinationPath $zip
Move-Item $zip $xpi
Write-Host "Packed: $xpi"
