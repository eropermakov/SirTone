# The offline installer (T6.3, PLT-003, D-116): the same NSIS installer with the WebView2
# offline installer inside (+~130 MB) — for PCs without internet and without WebView2. A separate
# file next to the usual one; it is not on the update channel (installed copies update with the
# small installer). The bundler downloads the WebView2 offline installer from Microsoft while
# building — run it only when such a file is wanted.
#
# Usage: powershell -NoProfile -ExecutionPolicy Bypass -File scripts\offline-installer.ps1
param([string]$Out = '')
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
. (Join-Path $PSScriptRoot 'dev-env.ps1')
$conf = Get-Content (Join-Path $root 'apps\desktop\src-tauri\tauri.conf.json') -Raw -Encoding UTF8 | ConvertFrom-Json
$version = $conf.version
$override = Join-Path $env:TEMP 'sirtone-offline.json'
'{"bundle":{"createUpdaterArtifacts":false,"windows":{"webviewInstallMode":{"type":"offlineInstaller","silent":true}}}}' |
    Set-Content -Path $override -Encoding ASCII
Push-Location (Join-Path $root 'apps\desktop')
try {
    pnpm tauri build --config $override
    if ($LASTEXITCODE -ne 0) { throw "tauri build failed ($LASTEXITCODE)" }
} finally { Pop-Location }
$built = Join-Path $root "target\release\bundle\nsis\SirTone_${version}_x64-setup.exe"
if (-not $Out) { $Out = Join-Path $root 'target\offline' }
New-Item -ItemType Directory -Force $Out | Out-Null
$dest = Join-Path $Out "SirTone_${version}_x64-offline-setup.exe"
Copy-Item $built $dest -Force
Write-Host "offline installer -> $dest"
