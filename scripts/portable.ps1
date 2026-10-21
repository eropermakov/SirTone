# Portable Сиртон (T6.4, FR-PORT-001/002): a folder (and a zip of it) that runs from a USB stick
# without installation — sirtone-app.exe, the sirtone.portable marker, the notices and a short
# guide (README-RU.txt: an ASCII name — Compress-Archive of PowerShell 5.1 mangles Cyrillic ones); optionally a WebView2 Fixed Runtime for PCs without WebView2.
#
# Usage (after `pnpm tauri build`; nothing is installed or registered on this PC):
#   powershell -NoProfile -ExecutionPolicy Bypass -File scripts\portable.ps1
#   ... -Runtime D:\Downloads\Microsoft.WebView2.FixedVersionRuntime.<ver>.x64
#
# -Runtime: an EXTRACTED Fixed Runtime folder (Microsoft's .cab, «expand <cab> -F:* <dir>»). Its
# msedgewebview2.exe must carry a valid Microsoft Authenticode signature, otherwise the script
# stops. The runtime (~180 MB) is never committed.
param(
    [string]$Runtime = '',
    [string]$Out = ''
)
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
$conf = Get-Content (Join-Path $root 'apps\desktop\src-tauri\tauri.conf.json') -Raw -Encoding UTF8 | ConvertFrom-Json
$version = $conf.version
$exe = Join-Path $root 'target\release\sirtone-app.exe'
if (-not (Test-Path $exe)) { throw "No $exe — run «pnpm tauri build» first." }
if (-not $Out) { $Out = Join-Path $root 'target\portable' }
$name = "SirTone-$version-portable"
$dir = Join-Path $Out $name
if (Test-Path $dir) { Remove-Item -Recurse -Force $dir }   # our own build output only
New-Item -ItemType Directory -Force $dir | Out-Null

Copy-Item $exe (Join-Path $dir 'SirTone.exe')
Set-Content -Path (Join-Path $dir 'sirtone.portable') -Value 'Portable Сиртон: data in .\Data, music in .\Music.' -Encoding UTF8
Copy-Item (Join-Path $root 'THIRD_PARTY_NOTICES.txt') $dir
@"
Сиртон $version — портативная версия

Запуск: SirTone.exe. Ничего не устанавливается и не записывается на компьютер:
настройки и журнал — в папке Data, музыка — в папке Music рядом с программой.
Без интернета всё работает (распознавание по сети и погода — по желанию).

Нужен WebView2 (есть в Windows 11 и в обновлённой Windows 10). Если на компьютере
его нет, положите Fixed Runtime в папку WebView2Runtime рядом с SirTone.exe.
Автозапуска, обновлений и ассоциаций файлов в портативной версии нет.
"@ | Set-Content -Path (Join-Path $dir 'README-RU.txt') -Encoding UTF8

if ($Runtime) {
    $rtExe = Join-Path $Runtime 'msedgewebview2.exe'
    if (-not (Test-Path $rtExe)) { throw "No msedgewebview2.exe in $Runtime." }
    $sig = Get-AuthenticodeSignature $rtExe
    if ($sig.Status -ne 'Valid' -or $sig.SignerCertificate.Subject -notmatch 'O=Microsoft Corporation') {
        throw "msedgewebview2.exe is not signed by Microsoft ($($sig.Status))."
    }
    Copy-Item -Recurse $Runtime (Join-Path $dir 'WebView2Runtime')
    Write-Host "Fixed Runtime included (signed by Microsoft)."
}

$zip = Join-Path $Out "$name.zip"
if (Test-Path $zip) { Remove-Item -Force $zip }
Compress-Archive -Path $dir -DestinationPath $zip
Write-Host "portable -> $dir"
Write-Host "zip      -> $zip"
