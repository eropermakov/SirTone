# SirTone dev environment bootstrap (Windows PowerShell 5.1+).
# Dot-source it in any shell that was started BEFORE the toolchain was installed (stale PATH):
#     . .\scripts\dev-env.ps1
# New shells / a restarted Claude app pick everything up from the registry on their own.

$DevRoot = 'D:\DevTools'

# Rust toolchain lives on D: (C: is small). Same values are persisted at User level.
$env:CARGO_HOME  = "$DevRoot\cargo"
$env:RUSTUP_HOME = "$DevRoot\rustup"

# Re-read PATH from the registry so freshly installed tools (cargo, cmake, gh, pnpm) are visible.
$env:Path = [Environment]::GetEnvironmentVariable('Path', 'Machine') + ';' + [Environment]::GetEnvironmentVariable('Path', 'User')

Write-Host "SirTone dev env ready: cargo=$env:CARGO_HOME  rustup=$env:RUSTUP_HOME"
