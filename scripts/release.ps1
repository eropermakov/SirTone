# Publishes a prototype (FR-UPD-002/003, docs/07 §9.3, D-76): signed build -> GitHub prerelease ->
# the update channel's latest.json -> a copy of the installer on the owner's desktop.
# Installed copies of Сиртон pick the new version up by themselves (60 s after start, then daily).
#
# Usage (clean tree, HEAD pushed, version already bumped in Cargo.toml/tauri.conf.json/package.json):
#   powershell -NoProfile -ExecutionPolicy Bypass -File scripts\release.ps1 -Proto 14 -Notes notes.md
#
# The signing key lives outside the repository: D:\SirVault\SirTone-secrets\sirtone-updater.key
# (minisign, no password). Lose it and installed copies stop accepting updates — keep a copy (OWN-04).
param(
    [Parameter(Mandatory = $true)][int]$Proto,
    [Parameter(Mandatory = $true)][string]$Notes,
    [string]$KeyPath = 'D:\SirVault\SirTone-secrets\sirtone-updater.key',
    [string]$Repo = 'eropermakov/SirTone'
)
$ErrorActionPreference = 'Stop'
$root = Split-Path $PSScriptRoot -Parent
. (Join-Path $PSScriptRoot 'dev-env.ps1')

function Must($what) { if ($LASTEXITCODE -ne 0) { throw "FAILED: $what (exit $LASTEXITCODE)" } }

$conf = Get-Content (Join-Path $root 'apps\desktop\src-tauri\tauri.conf.json') -Raw -Encoding UTF8 | ConvertFrom-Json
$version = $conf.version
$tag = "v$version-proto.$Proto"
Write-Host "release $tag"

if (git -C $root status --porcelain) { throw 'working tree is not clean' }
$head = (git -C $root rev-parse HEAD).Trim()
git -C $root fetch -q origin
$branch = (git -C $root rev-parse --abbrev-ref HEAD).Trim()
if ((git -C $root rev-parse "origin/$branch").Trim() -ne $head) { throw 'HEAD is not pushed' }
if (-not (Test-Path $KeyPath)) { throw "signing key not found: $KeyPath" }

# 1. Signed build: the bundler writes SirTone_<v>_x64-setup.exe and its .sig.
$env:TAURI_SIGNING_PRIVATE_KEY = (Get-Content $KeyPath -Raw).Trim()
$env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD = ''
Push-Location (Join-Path $root 'apps\desktop')
try { pnpm tauri build; Must 'tauri build' } finally { Pop-Location }
Remove-Item Env:TAURI_SIGNING_PRIVATE_KEY, Env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD -ErrorAction SilentlyContinue

$bundle = Join-Path $root 'target\release\bundle\nsis'
$exe = Join-Path $bundle "SirTone_${version}_x64-setup.exe"
$sig = "$exe.sig"
foreach ($f in @($exe, $sig)) { if (-not (Test-Path $f)) { throw "missing artifact: $f" } }
$sha = (Get-FileHash $exe -Algorithm SHA256).Hash

# 2. The prerelease with the installer and its signature.
$notesText = (Get-Content $Notes -Raw -Encoding UTF8).TrimEnd() + "`n`nSHA-256: $sha`n"
$notesFile = Join-Path $env:TEMP "sirtone-notes-$tag.md"
[IO.File]::WriteAllText($notesFile, $notesText, [Text.UTF8Encoding]::new($false))
git -C $root tag $tag $head; Must 'git tag'
git -C $root push -q origin $tag; Must 'git push tag'
gh release create $tag $exe $sig --repo $Repo --prerelease --title "Сиртон $version — прототип $Proto" --notes-file $notesFile --verify-tag
Must 'gh release create'

# 3. The update channel: one fixed release whose latest.json points at this installer.
$url = "https://github.com/$Repo/releases/download/$tag/SirTone_${version}_x64-setup.exe"
$latest = [ordered]@{
    version   = $version
    notes     = (Get-Content $Notes -Raw -Encoding UTF8).Trim()
    pub_date  = (Get-Date).ToUniversalTime().ToString('yyyy-MM-ddTHH:mm:ssZ')
    platforms = [ordered]@{
        'windows-x86_64' = [ordered]@{ signature = (Get-Content $sig -Raw).Trim(); url = $url }
    }
}
$latestFile = Join-Path $env:TEMP 'latest.json'
[IO.File]::WriteAllText($latestFile, ($latest | ConvertTo-Json -Depth 5), [Text.UTF8Encoding]::new($false))
# cmd /c: Windows PowerShell 5.1 turns a native command's stderr into a terminating error.
cmd /c "gh release view update-channel --repo $Repo >nul 2>&1"
if ($LASTEXITCODE -ne 0) {
    gh release create update-channel --repo $Repo --prerelease --title 'Канал обновлений' --notes 'Служебный релиз: latest.json для автообновления установленных копий (D-76). Не удалять.' --target $head
    Must 'gh release create update-channel'
}
gh release upload update-channel $latestFile --repo $Repo --clobber; Must 'upload latest.json'

# 4. A copy on the owner's desktop.
$desktop = [Environment]::GetFolderPath('Desktop')
Copy-Item $exe (Join-Path $desktop "SirTone_$version-proto.${Proto}_x64-setup.exe") -Force

Write-Host "published $tag  sha256 $sha"
Write-Host "update channel -> $url"
