# Checks that every documentation file has its counterpart in the other language
# and that its first line links to that counterpart.
#
# Rules:
#   - Root:  X.md (Spanish) <-> X.en.md (English). CLAUDE.md is Spanish-only.
#   - docs/: docs/es/<path>.md <-> docs/en/<path>.md, same relative path.
#
# Exit code 0 when everything is paired, 1 otherwise. Works on Windows PowerShell 5.1 and PowerShell 7.

$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
$singleLanguage = @('CLAUDE.md')
$errors = New-Object System.Collections.Generic.List[string]

function Get-FirstLine([string]$path) {
    $line = Get-Content -LiteralPath $path -TotalCount 1 -Encoding UTF8
    if ($null -eq $line) { return '' }
    return [string]$line
}

function Test-Pair([string]$file, [string]$counterpart, [string]$expectedLink, [string]$label) {
    if (-not (Test-Path -LiteralPath $counterpart -PathType Leaf)) {
        $errors.Add("$label has no counterpart: expected $counterpart")
        return
    }
    $first = Get-FirstLine $file
    if (-not $first.Contains("($expectedLink)")) {
        $errors.Add("$label first line must link to '$expectedLink'")
    }
}

# Tracked and new (not ignored) Markdown files, as repository-relative paths with forward slashes.
[Console]::OutputEncoding = [System.Text.Encoding]::UTF8
$files = git -C $root -c core.quotepath=false ls-files --cached --others --exclude-standard -- '*.md'
if ($LASTEXITCODE -ne 0) { throw 'git ls-files failed' }
$files = $files | Where-Object { Test-Path -LiteralPath (Join-Path $root $_) -PathType Leaf } | Sort-Object -Unique

foreach ($rel in $files) {
    $full = Join-Path $root $rel
    $parts = $rel.Split('/')

    if ($parts.Length -eq 1) {
        # Root file.
        if ($singleLanguage -contains $rel) { continue }
        if ($rel.EndsWith('.en.md')) {
            $other = $rel.Substring(0, $rel.Length - '.en.md'.Length) + '.md'
        } else {
            $other = $rel.Substring(0, $rel.Length - '.md'.Length) + '.en.md'
        }
        Test-Pair $full (Join-Path $root $other) $other $rel
    } elseif ($parts[0] -eq 'docs') {
        $lang = $parts[1]
        if ($parts.Length -lt 3 -or ($lang -ne 'es' -and $lang -ne 'en')) {
            $errors.Add("$rel is outside docs/es and docs/en")
            continue
        }
        $otherLang = if ($lang -eq 'es') { 'en' } else { 'es' }
        $inner = ($parts[2..($parts.Length - 1)]) -join '/'
        # Relative link from docs/<lang>/<inner> to docs/<otherLang>/<inner>.
        $up = '../' * ($parts.Length - 2)
        $link = "$up$otherLang/$inner"
        Test-Pair $full (Join-Path $root "docs/$otherLang/$inner") $link $rel
    }
}

if ($errors.Count -gt 0) {
    Write-Host "Documentation language pairs: $($errors.Count) problem(s)" -ForegroundColor Red
    foreach ($e in $errors) { Write-Host "  - $e" }
    exit 1
}
Write-Host 'Documentation language pairs: OK' -ForegroundColor Green
exit 0
