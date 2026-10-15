# Compares the startup of two builds with interleaved runs (A, B, A, B...), so background load
# affects both equally. Use it to check a change for startup regressions: build the previous
# commit into another folder (for example with `git worktree`) and compare.
#
# Usage:
#   ./scripts/compare-startup.ps1 -A old\bracket.exe -B target\release\bracket.exe [-Rounds 20]
#
# Prints the median launch->first frame time and the per-phase medians of each build.

param(
    [Parameter(Mandatory = $true)][string]$A,
    [Parameter(Mandatory = $true)][string]$B,
    [int]$Rounds = 20
)

$ErrorActionPreference = 'Stop'
$A = (Resolve-Path -LiteralPath $A).Path
$B = (Resolve-Path -LiteralPath $B).Path
if (-not ('PreciseClock' -as [type])) {
    Add-Type -TypeDefinition @'
using System; using System.Runtime.InteropServices;
public static class PreciseClock {
    [DllImport("kernel32.dll")] static extern void GetSystemTimePreciseAsFileTime(out long ft);
    public static double UnixMicroseconds() { long ft; GetSystemTimePreciseAsFileTime(out ft); return (ft - 116444736000000000L) / 10.0; }
}
'@
}
function Median($xs) { $s = $xs | Sort-Object; $n = $s.Count; if ($n % 2) { $s[[int][math]::Floor($n/2)] } else { ($s[$n/2-1] + $s[$n/2]) / 2 } }
function Run([string]$exe) {
    $out = [IO.Path]::GetTempFileName()
    $psi = New-Object Diagnostics.ProcessStartInfo $exe
    $psi.UseShellExecute = $false
    $psi.EnvironmentVariables['BRACKET_MEASURE_STARTUP'] = $out
    $launch = [PreciseClock]::UnixMicroseconds()
    $p = [Diagnostics.Process]::Start($psi)
    if (-not $p.WaitForExit(30000)) { $p.Kill(); throw 'timeout' }
    $v = @{}
    foreach ($l in Get-Content $out) { if ($l -match '^(\w+)=([0-9.]+)$') { $v[$Matches[1]] = [double]$Matches[2] } }
    Remove-Item $out
    [pscustomobject]@{
        total = ($v.frame_unix_us - $launch) / 1000
        app   = ($v.app_unix_us - $v.main_unix_us) / 1000
        ui    = ($v.first_ui_unix_us - $v.app_unix_us) / 1000
        frame = ($v.frame_unix_us - $v.first_ui_unix_us) / 1000
    }
}
$null = Run $A; $null = Run $B
$ra = @(); $rb = @()
for ($i = 0; $i -lt $Rounds; $i++) { $ra += Run $A; $rb += Run $B }
foreach ($pair in @(@('A', $A, $ra), @('B', $B, $rb))) {
    $r = $pair[2]
    "{0} total {1,6:N1} ms | main->app {2,5:N1} | app->ui {3,4:N1} | ui->frame {4,5:N1}   ({5})" -f $pair[0], (Median $r.total), (Median $r.app), (Median $r.ui), (Median $r.frame), (Split-Path $pair[1] -Leaf)
}
