#!/usr/bin/env pwsh
# Draws compare.js's pairs: for each shot and mode, one PNG with the shot
# before (改前) on the left and after (改后) on the right, each under its
# label, the shot's title and verdict on top, and what changed boxed on both.
#
#   pwsh tools/preview/pairs.ps1 -List <pairs.json>     (compare.js writes it)
#
# pairs.json: { before, after, status: { same: '相同', ... }, items: [{ id, mode,
# title, before, after (PNG paths or null), out, k (each side drawn at this
# much of its pixels), density (its pixels to a screen px: the words drawn as
# large), status, changed, box: [x, y, w, h] or null }] }
param([Parameter(Mandatory)][string]$List)
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Drawing
$plan = Get-Content -LiteralPath $List -Raw -Encoding UTF8 | ConvertFrom-Json

$font = 'Microsoft YaHei UI'
$hit = [System.Drawing.Color]::FromArgb(255, 45, 149)

foreach ($it in $plan.items) {
    $d = [Math]::Max(1, [double]$it.density)
    $fTitle = [System.Drawing.Font]::new($font, 18 * $d, [System.Drawing.FontStyle]::Bold, [System.Drawing.GraphicsUnit]::Pixel)
    $fLabel = [System.Drawing.Font]::new($font, 14 * $d, [System.Drawing.FontStyle]::Regular, [System.Drawing.GraphicsUnit]::Pixel)
    $pad = [int](16 * $d); $gap = [int](16 * $d); $titleH = [int](30 * $d); $labelH = [int](24 * $d)
    $dark = $it.mode -eq 'dark'
    $bg = if ($dark) { [System.Drawing.Color]::FromArgb(28, 28, 31) } else { [System.Drawing.Color]::FromArgb(243, 243, 243) }
    $ink = if ($dark) { [System.Drawing.Color]::FromArgb(235, 235, 240) } else { [System.Drawing.Color]::FromArgb(28, 28, 32) }
    $soft = if ($dark) { [System.Drawing.Color]::FromArgb(150, 150, 158) } else { [System.Drawing.Color]::FromArgb(100, 100, 108) }
    $line = if ($dark) { [System.Drawing.Color]::FromArgb(60, 60, 66) } else { [System.Drawing.Color]::FromArgb(210, 210, 214) }

    $imgs = [System.Collections.Generic.List[object]]::new()
    foreach ($file in @($it.before, $it.after)) { $imgs.Add($(if ($file -and (Test-Path -LiteralPath $file)) { [System.Drawing.Image]::FromFile($file) } else { $null })) }
    try {
        $some = $imgs | Where-Object { $_ } | Select-Object -First 1
        if (-not $some) { continue }
        $k = [double]$it.k
        $w = [int][Math]::Round($some.Width * $k); $h = [int][Math]::Round($some.Height * $k)
        $width = 2 * $pad + 2 * $w + $gap
        $height = $pad + $titleH + $labelH + $h + $pad
        $sheet = [System.Drawing.Bitmap]::new($width, $height)
        $g = [System.Drawing.Graphics]::FromImage($sheet)
        $g.InterpolationMode = 'HighQualityBicubic'; $g.PixelOffsetMode = 'HighQuality'; $g.TextRenderingHint = 'AntiAliasGridFit'
        $g.Clear($bg)
        $inkB = [System.Drawing.SolidBrush]::new($ink); $softB = [System.Drawing.SolidBrush]::new($soft); $hitB = [System.Drawing.SolidBrush]::new($hit)
        $pen = [System.Drawing.Pen]::new($line); $hitPen = [System.Drawing.Pen]::new($hit, [float](2 * $d))

        $verdict = $plan.status.($it.status)
        if ($it.status -eq 'changed') { $verdict = "$verdict · $($it.changed) 像素" }
        $g.DrawString("$($it.title)", $fTitle, $inkB, $pad - 2, $pad - 4)
        $tw = $g.MeasureString("$($it.title)", $fTitle).Width
        $g.DrawString("$($it.id) · $($it.mode) · $verdict", $fLabel, $(if ($it.status -eq 'same') { $softB } else { $hitB }), $pad + $tw + 8 * $d, $pad + 3 * $d)

        $labels = @($plan.before, $plan.after)
        for ($i = 0; $i -lt 2; $i++) {
            $x = $pad + $i * ($w + $gap); $y = $pad + $titleH
            $g.DrawString($labels[$i], $fLabel, $softB, $x - 1, $y)
            $y += $labelH
            $img = $imgs[$i]
            if ($img) {
                $g.DrawImage($img, [System.Drawing.Rectangle]::new($x, $y, $w, $h))
            } else {
                $g.DrawString('（这一边没有这张）', $fLabel, $softB, $x + 12, $y + 12)
            }
            $g.DrawRectangle($pen, $x - 1, $y - 1, $w + 1, $h + 1)
            if ($it.box -and $img) {
                $b = $it.box
                $g.DrawRectangle($hitPen, [float]($x + $b[0] * $k - 2 * $d), [float]($y + $b[1] * $k - 2 * $d), [float]($b[2] * $k + 4 * $d), [float]($b[3] * $k + 4 * $d))
            }
        }
        $sheet.Save($it.out, [System.Drawing.Imaging.ImageFormat]::Png)
        $g.Dispose(); $sheet.Dispose(); $inkB.Dispose(); $softB.Dispose(); $hitB.Dispose(); $pen.Dispose(); $hitPen.Dispose(); $fTitle.Dispose(); $fLabel.Dispose()
    } finally {
        foreach ($img in $imgs) { if ($img) { $img.Dispose() } }
    }
}
"$(@($plan.items).Count) pairs drawn"
