# Runs every local check in order and stops at the first failure.
# The same checks run in continuous integration.

# 'Continue' on purpose: cargo writes progress to stderr, and Windows PowerShell 5.1 turns
# redirected native stderr into errors. Failures are detected through $LASTEXITCODE instead.
$ErrorActionPreference = 'Continue'
Set-Location (Split-Path -Parent $PSScriptRoot)

function Invoke-Step([string]$name, [scriptblock]$command) {
    Write-Host "==> $name" -ForegroundColor Cyan
    & $command
    if ($LASTEXITCODE -ne 0) {
        Write-Host "FAILED: $name" -ForegroundColor Red
        exit $LASTEXITCODE
    }
}

Invoke-Step 'cargo fmt' { cargo fmt --all --check }
Invoke-Step 'cargo clippy' { cargo clippy --workspace --all-targets -- -D warnings }
Invoke-Step 'cargo test' { cargo test --workspace }
Invoke-Step 'documentation pairs' { & "$PSScriptRoot/check-doc-pairs.ps1" }

Write-Host 'All checks passed.' -ForegroundColor Green
