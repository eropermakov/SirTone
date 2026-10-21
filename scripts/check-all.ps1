# Run every quality gate that exists at the current stage of the project.
# Usage (Windows PowerShell 5.1, pwsh is not installed locally; CI uses pwsh):
#     powershell -NoProfile -ExecutionPolicy Bypass -File scripts\check-all.ps1
# -ExecutionPolicy Bypass applies to this one process only; system settings are not changed.
$ErrorActionPreference = 'Stop'
. "$PSScriptRoot\dev-env.ps1"
Set-Location (Split-Path $PSScriptRoot -Parent)

function Step($name, [scriptblock]$body) {
    Write-Host "`n=== $name ===" -ForegroundColor Cyan
    & $body
    if ($LASTEXITCODE -and $LASTEXITCODE -ne 0) { throw "FAILED: $name (exit $LASTEXITCODE)" }
}

if (Test-Path 'Cargo.toml') {
    Step 'cargo fmt'      { cargo fmt --all -- --check }
    Step 'cargo clippy'   { cargo clippy --workspace --all-targets -- -D warnings }
    Step 'cargo deny'     { cargo deny check }
    # T6.6: the third-party notices match the dependencies (regenerate: python tools/notices.py).
    Step 'notices'        { python tools/notices.py; git diff --exit-code -- assets/legal/notices.json THIRD_PARTY_NOTICES.txt }
    # --no-tests=warn: empty crates of Stage 0 have no tests yet; nextest's default (auto) exits with code 4.
    Step 'cargo nextest'  { cargo nextest run --workspace --no-tests=warn }
    # NFR-PERF-004 (T3.24): a wave rebuild on 50 000 synthetic tracks stays under 15 ms p95.
    Step 'wave bench'     { cargo run --release -q -p sirtone-wave --example bench }
} else { Write-Host 'No Cargo workspace yet - Rust gates skipped.' -ForegroundColor Yellow }

if (Test-Path 'package.json') {
    Step 'pnpm install'   { pnpm install --frozen-lockfile }
    Step 'pnpm lint'      { pnpm run --if-present lint }
    Step 'pnpm typecheck' { pnpm run --if-present typecheck }
    Step 'pnpm test'      { pnpm run --if-present test }
    # UI smoke in Edge (same engine as WebView2) against a mocked Tauri bridge (T0.11).
    Step 'pnpm e2e'       { pnpm --filter ./apps/desktop run e2e }
} else { Write-Host 'No package.json yet - JS gates skipped.' -ForegroundColor Yellow }

Write-Host "`nAll available gates passed." -ForegroundColor Green
