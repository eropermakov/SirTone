# Publishes the iPhone app for the hub's install (D-154, FR-HUB-006): builds it in WSL, signs the
# unsigned .ipa with the updater key and puts it in a GitHub prerelease; `ios-latest.json` in the
# update channel tells every PC-Сиртон which file to take and how to check it. It carries nobody's
# Apple signature: the hub signs it with the Apple ID of the phone's owner.
#
# Usage (clean tree, HEAD pushed; the installed iPhones are not touched):
#   powershell -NoProfile -ExecutionPolicy Bypass -File scripts\release-ios.ps1
param(
    [string]$KeyPath = 'D:\SirVault\SirTone-secrets\sirtone-updater.key',
    [string]$Repo = 'eropermakov/SirTone',
    # The phone app's own number when only it is republished (e.g. 0.2.1 beside PC 0.2.0);
    # empty - the PC's version from tauri.conf.json.
    [string]$Version = ''
)
$ErrorActionPreference = 'Stop'
$root = Split-Path $PSScriptRoot -Parent
. (Join-Path $PSScriptRoot 'dev-env.ps1')
function Must($what) { if ($LASTEXITCODE -ne 0) { throw "FAILED: $what (exit $LASTEXITCODE)" } }

$version = (Get-Content (Join-Path $root 'apps\desktop\src-tauri\tauri.conf.json') -Raw -Encoding UTF8 | ConvertFrom-Json).version
if ($Version) { $version = $Version }
$tag = "mobile-v$version"
$name = "sirtone-ios-$version-unsigned.ipa"
if (git -C $root status --porcelain) { throw 'working tree is not clean' }
$head = (git -C $root rev-parse HEAD).Trim()
git -C $root fetch -q origin
$branch = (git -C $root rev-parse --abbrev-ref HEAD).Trim()
if ((git -C $root rev-parse "origin/$branch").Trim() -ne $head) { throw 'HEAD is not pushed' }
if (-not (Test-Path $KeyPath)) { throw "signing key not found: $KeyPath" }

# 1. The app: the visualiser page on Windows, the core and the SwiftUI app in WSL.
pnpm --dir (Join-Path $root 'apps\desktop') build:mobile-viz; Must 'build:mobile-viz'
$wslRepo = '/mnt/' + $root.Substring(0, 1).ToLower() + ($root.Substring(2) -replace '\\', '/')
$outDir = Join-Path $root 'target\ios'
New-Item -ItemType Directory -Force $outDir | Out-Null
$ipa = Join-Path $outDir $name
$wslIpa = "$wslRepo/target/ios/$name"
foreach ($step in 'core', 'app') {
    wsl -d Ubuntu-24.04 -- bash -lc "bash $wslRepo/scripts/wsl-ios.sh $step"; Must "wsl-ios $step"
}
wsl -d Ubuntu-24.04 -- bash -lc "SIRTONE_IOS_IPA=$wslIpa bash $wslRepo/scripts/wsl-ios.sh ipa"; Must 'wsl-ios ipa'
if (-not (Test-Path $ipa)) { throw "missing artifact: $ipa" }

# 2. The channel's signature (the key of the PC updates).
$env:TAURI_SIGNING_PRIVATE_KEY = (Get-Content $KeyPath -Raw).Trim()
$env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD = ''
Push-Location (Join-Path $root 'apps\desktop')
try { pnpm tauri signer sign $ipa; Must 'tauri signer sign' } finally { Pop-Location }
Remove-Item Env:TAURI_SIGNING_PRIVATE_KEY, Env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD -ErrorAction SilentlyContinue
$sig = "$ipa.sig"
$sha = (Get-FileHash $ipa -Algorithm SHA256).Hash.ToLower()

# 3. The prerelease, then the channel's pointer to it.
git -C $root tag $tag $head; Must 'git tag'
git -C $root push -q origin $tag; Must 'git push tag'
gh release create $tag $ipa $sig --repo $Repo --prerelease --title "Сиртон для iPhone $version" --notes "Неподписанное приложение для установки из ПК-Сиртона (раздел «Телефоны»). SHA-256: $sha" --verify-tag
Must 'gh release create'
$latest = [ordered]@{ version = $version; file = "$tag/$name"; sha256 = $sha; signature = (Get-Content $sig -Raw).Trim() }
$latestFile = Join-Path $env:TEMP 'ios-latest.json'
[IO.File]::WriteAllText($latestFile, ($latest | ConvertTo-Json), [Text.UTF8Encoding]::new($false))
gh release upload update-channel $latestFile --repo $Repo --clobber; Must 'upload ios-latest.json'
Write-Host "published $tag  sha256 $sha"
