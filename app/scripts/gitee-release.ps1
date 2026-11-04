#!/usr/bin/env pwsh
# Copies a FISH Dock GitHub release to Gitee (default: gitee.com/Fish-under-sea/FISH-Dock),
# for those in China: the same tag, title and notes; the same exe, taken from
# GitHub's release (or given) and checked by GitHub's SHA-256; and
# <exe>.sha256 beside it, put up after the exe: the updater takes a release
# on Gitee as there once its .sha256 is, and checks by it when GitHub gives
# no SHA-256 (src-tauri/src/updates.rs). Then the exe downloaded back from
# Gitee, as the updater does it (no account, no API), and checked again.
#
#   $env:GITEE_TOKEN = '<a Gitee private token, scope projects>'
#   pwsh scripts/gitee-release.ps1 -Version 2.0.4 [-Asset FISH-Dock-Setup-2.0.4.exe] [-DryRun]
#
# 2.0.4 起这是 FISH Dock 自己的发布入口：默认仓库是 Fish-under-sea/FISH-Dock，
# 指向原项目（zhoupengjie/Wakuwaku）必须显式 -AllowUpstream，避免把新版本发到上游。
# The tag must be on Gitee first (git push <gitee remote> v2.0.4). Run again,
# it adds only what is missing. Without GITEE_TOKEN it writes nothing and
# only checks what is there. -DryRun says what it would write, and checks
# nothing after.
param(
    [Parameter(Mandatory)][string]$Version,
    [string]$Asset,
    [string]$Exe,
    [string]$Owner = 'Fish-under-sea',
    [string]$Repo = 'FISH-Dock',
    [switch]$AllowUpstream,
    [switch]$DryRun
)
$ErrorActionPreference = 'Stop'
$Version = $Version.TrimStart('v', 'V')
$tag = "v$Version"
if (-not $Asset) { $Asset = "FISH-Dock-Setup-$Version.exe" }
$name = $Asset
# 上游仓库只读归属，不作为默认发布目标；要发必须显式声明。
$upstream = ($Owner -eq 'zhoupengjie' -and $Repo -eq 'Wakuwaku')
if ($upstream -and -not $AllowUpstream) {
    throw "拒绝向上游仓库发布：$Owner/$Repo 不是 FISH Dock 的发布目标（确需如此请显式加 -AllowUpstream）"
}
$api = "https://gitee.com/api/v5/repos/$Owner/$Repo"
$headers = @{ 'User-Agent' = 'fish-dock-release' }
$token = $env:GITEE_TOKEN
# The token in a header, as Gitee takes it everywhere (an access_token field
# in a multipart upload was refused: 40001, after the whole exe had gone).
$auth = $headers + @{ Authorization = "token $token" }

function Sha256([string]$path) { (Get-FileHash -Algorithm SHA256 -LiteralPath $path).Hash.ToLowerInvariant() }

# The token tried first, so a bad one fails in a second.
if ($token -and -not $DryRun) {
    $me = Invoke-RestMethod -Headers $auth 'https://gitee.com/api/v5/user'
    "Gitee: signed in as $($me.login)"
}

# GitHub's release: its title, its notes, its exe's SHA-256.
$gh = Invoke-RestMethod -Headers ($headers + @{ Accept = 'application/vnd.github+json' }) "https://api.github.com/repos/$Owner/$Repo/releases/tags/$tag"
# 不能叫 $asset：它与参数 $Asset 是同一个变量（PowerShell 变量不区分大小写），
# 而 [string]$Asset 声明了类型，赋值对象会被强制转成字符串 → $asset.digest 恒为空。
# 2.1.0 首发踩到：GitHub 明明返回 sha256:…，脚本却报 "GitHub gives no SHA-256"。
$ghAsset = $gh.assets | Where-Object name -EQ $name
if (-not $ghAsset) { throw "GitHub's $tag has no $name" }
$digest = "$($ghAsset.digest)" -replace '^sha256:', ''
if ($digest -notmatch '^[0-9a-fA-F]{64}$') { throw "GitHub gives no SHA-256 for $name" }
$digest = $digest.ToLowerInvariant()

# The exe, as GitHub has it, and its .sha256 as sha256sum writes it.
$work = Join-Path ([IO.Path]::GetTempPath()) "fish-dock-gitee-$Version"
New-Item -ItemType Directory -Force $work | Out-Null
$file = Join-Path $work $name
if ($Exe) { Copy-Item -LiteralPath $Exe $file -Force }
else { Invoke-WebRequest -Headers $headers $ghAsset.browser_download_url -OutFile $file }
if ((Sha256 $file) -ne $digest) { throw "$name is not the one GitHub gives (SHA-256 $digest)" }
$sums = "$file.sha256"
[IO.File]::WriteAllText($sums, "$digest  $name`n", [Text.UTF8Encoding]::new($false))
"GitHub's $name`: SHA-256 $digest"

# The tag on Gitee, and the commit it is on.
$refs = git ls-remote --tags "https://gitee.com/$Owner/$Repo.git"
$line = $refs | Where-Object { $_ -match "\trefs/tags/$([regex]::Escape($tag))\^\{\}$" } | Select-Object -First 1
if (-not $line) { $line = $refs | Where-Object { $_ -match "\trefs/tags/$([regex]::Escape($tag))$" } | Select-Object -First 1 }
if (-not $line) { throw "$tag is not on Gitee yet: push it first (git push <gitee remote> $tag)" }
$commit = ($line -split "`t")[0]
"Gitee's $tag`: $commit"

# Gitee's API only with the token (the token's allowance): without an
# account it allows an address some sixty calls, then refuses it for over an
# hour. With no token, nothing is written and only what is there is checked.
if ($token) {
    # The release there, made if missing (GitHub's title and notes).
    $release = Invoke-RestMethod -Headers $auth "$api/releases/tags/$tag"
    if (-not $release.id) {
        if ($DryRun) { "would make the release $tag on Gitee: $($gh.name)"; return }
        $form = @{ tag_name = $tag; name = $gh.name; body = "$($gh.body)"; prerelease = 'false'; target_commitish = $commit }
        $release = Invoke-RestMethod -Method Post -Headers $auth -ContentType 'application/x-www-form-urlencoded; charset=utf-8' -Body $form "$api/releases"
        "made the release $tag on Gitee (id $($release.id))"
    }
    # The exe, then its .sha256, those not there yet: the updater takes the
    # release as whole once the .sha256 is there.
    $have = @((Invoke-RestMethod -Headers $auth "$api/releases/$($release.id)/attach_files").name)
    foreach ($f in @($file, $sums)) {
        $leaf = Split-Path $f -Leaf
        if ($have -contains $leaf) { "on Gitee already: $leaf"; continue }
        if ($DryRun) { "would upload: $leaf"; continue }
        # No progress is shown; from outside China it goes at some 25 KB a second.
        $size = (Get-Item -LiteralPath $f).Length
        if ($size -gt 1MB) { "uploading $leaf ({0:N1} MB; minutes from outside China)…" -f ($size / 1MB) } else { "uploading $leaf…" }
        Invoke-RestMethod -Method Post -Headers $auth -Form @{ file = Get-Item -LiteralPath $f } "$api/releases/$($release.id)/attach_files" | Out-Null
        "uploaded: $leaf"
    }
} elseif ($DryRun) {
    'no GITEE_TOKEN: what is missing on Gitee is not looked at'
} else {
    'no GITEE_TOKEN: nothing written, only what is there checked'
}
if ($DryRun) { return }

# Downloaded back as the updater does: no account, no API, from where Gitee
# keeps a release's files (a moment's wait, should one just put up not be
# there yet).
$url = "https://gitee.com/$Owner/$Repo/releases/download/$tag/$name"
$back = Join-Path $work "back-$name"
"downloading $name back from Gitee to check it…"
for ($i = 0; ; $i++) {
    try { Invoke-WebRequest -Headers $headers $url -OutFile $back; break }
    catch {
        if ($i -ge 5 -or $_.Exception.Response.StatusCode -ne 404) { throw "Gitee has no $name for $tag ($($_.Exception.Message))" }
        Start-Sleep -Seconds 10
    }
}
if ((Sha256 $back) -ne $digest) { throw "Gitee's $name is not GitHub's: delete it there and run this again" }
try { Invoke-WebRequest -Headers $headers "$url.sha256" -OutFile "$back.sha256" }
catch { throw "Gitee has no $name.sha256 for $tag ($($_.Exception.Message)): the updater does not take this release from Gitee without it" }
$said = ((Get-Content -Raw -LiteralPath "$back.sha256") -split '\s+' | Where-Object { $_ })[0].ToLowerInvariant()
if ($said -ne $digest) { throw "Gitee's $name.sha256 says $said, not $digest" }
"checked: Gitee gives the same exe, and its .sha256 agrees"
"https://gitee.com/$Owner/$Repo/releases/tag/$tag"