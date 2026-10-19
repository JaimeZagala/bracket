# Measures executable size, startup time and idle memory of a Bracket build (or any executable
# that honours BRACKET_MEASURE_STARTUP).
#
# Startup: the executable is launched with BRACKET_MEASURE_STARTUP=<file>. When its first frame is
# on screen it writes three values to that file and exits:
#   main_unix_us   wall-clock time when main() started
#   frame_unix_us  wall-clock time when the first frame had been presented
#   main_to_frame_ms
# The script takes a precise wall-clock timestamp right before launching the process, so it can
# report launch-to-main and launch-to-first-frame without counting process shutdown.
#
# The first run after a reboot approximates a cold start; later runs are warm (files cached).
#
# Every run uses a copy of the executable in a temporary folder with portable.toml next to it, so
# the user's configuration, session and window position are neither read nor changed, and an
# open Bracket (single instance) does not take the files instead.
#
# Usage:
#   ./scripts/measure.ps1 -Exe target/release/bracket.exe [-Runs 10] [-Arguments 'file.txt']
#       [-Maximized] [-Scroll [-ScrollRuns 5]] [-MaxStartupMs 150]
#       [-Config config.toml]
#
# -Config copies that file next to the executable as its config.toml (for example, to measure
# with [experimental] project_navigation = true).
# -Maximized starts the window maximized (BRACKET_MEASURE_MAXIMIZED=1).
# -MaxStartupMs N fails (exit code 1) when the warm median launch->first frame exceeds N ms.
# -Scroll (needs a file in -Arguments) also runs BRACKET_MEASURE_SCROLL=<file>: after the first
# content frame the app scrolls one page per frame for 300 frames and reports their wall time.
# CPU per frame = (process CPU of a scroll run - median process CPU of a startup run) / frames.

param(
    [Parameter(Mandatory = $true)][string]$Exe,
    [int]$Runs = 10,
    [string]$Arguments = '',
    [int]$IdleSeconds = 3,
    [switch]$Maximized,
    [switch]$Scroll,
    [int]$ScrollRuns = 5,
    [double]$MaxStartupMs = 0,
    [string]$Config = ''
)

$ErrorActionPreference = 'Stop'
$exePath = (Resolve-Path -LiteralPath $Exe).Path
$work = Join-Path ([System.IO.Path]::GetTempPath()) ('bracket-measure-' + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $work | Out-Null
$runExe = Join-Path $work (Split-Path -Leaf $exePath)
Copy-Item -LiteralPath $exePath -Destination $runExe
New-Item -ItemType File -Path (Join-Path $work 'portable.toml') | Out-Null
if ($Config) {
    Copy-Item -LiteralPath (Resolve-Path -LiteralPath $Config).Path -Destination (Join-Path $work 'config.toml')
}

function Remove-Work {
    Remove-Item -LiteralPath $work -Recurse -Force -ErrorAction SilentlyContinue
}

# GetSystemTimePreciseAsFileTime gives sub-microsecond wall-clock time, the same clock that
# Rust's SystemTime uses on Windows.
if (-not ('PreciseClock' -as [type])) {
    Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public static class PreciseClock {
    [DllImport("kernel32.dll")]
    static extern void GetSystemTimePreciseAsFileTime(out long fileTime);
    public static double UnixMicroseconds() {
        long ft;
        GetSystemTimePreciseAsFileTime(out ft);
        return (ft - 116444736000000000L) / 10.0;
    }
}
'@
}

function Get-Median([double[]]$values) {
    $sorted = $values | Sort-Object
    $n = $sorted.Count
    if ($n -eq 0) { return [double]::NaN }
    if ($n % 2 -eq 1) { return $sorted[[int][math]::Floor($n / 2)] }
    return ($sorted[$n / 2 - 1] + $sorted[$n / 2]) / 2
}

function Start-Target([hashtable]$extraEnv) {
    $psi = New-Object System.Diagnostics.ProcessStartInfo
    $psi.FileName = $runExe
    $psi.Arguments = $Arguments
    $psi.UseShellExecute = $false
    foreach ($k in $extraEnv.Keys) { $psi.EnvironmentVariables[$k] = $extraEnv[$k] }
    if ($Maximized) { $psi.EnvironmentVariables['BRACKET_MEASURE_MAXIMIZED'] = '1' }
    return [System.Diagnostics.Process]::Start($psi)
}

# Test machine (no host name or other identifying data).
$cpu = (Get-CimInstance Win32_Processor | Select-Object -First 1).Name.Trim()
$ramGb = [math]::Round((Get-CimInstance Win32_ComputerSystem).TotalPhysicalMemory / 1GB, 1)
$os = Get-CimInstance Win32_OperatingSystem
$gpu = (Get-CimInstance Win32_VideoController | Select-Object -ExpandProperty Name) -join ', '
Write-Host "Machine: $cpu | $ramGb GB RAM | $($os.Caption) build $($os.BuildNumber) | GPU: $gpu"

# Size.
$bytes = (Get-Item -LiteralPath $exePath).Length
Write-Host ("Size: {0:N0} bytes ({1:N2} MB)" -f $bytes, ($bytes / 1MB))

# Startup.
$toMain = @()
$toFrame = @()
$startupCpu = @()
$phaseApp = @()
$phaseUi = @()
$phaseFrame = @()
for ($i = 1; $i -le $Runs; $i++) {
    $out = [System.IO.Path]::GetTempFileName()
    $launch = [PreciseClock]::UnixMicroseconds()
    $p = Start-Target @{ BRACKET_MEASURE_STARTUP = $out }
    if (-not $p.WaitForExit(30000)) { $p.Kill(); throw "Run $i did not exit within 30 s" }
    $startupCpu += $p.TotalProcessorTime.TotalMilliseconds
    $values = @{}
    foreach ($line in Get-Content -LiteralPath $out) {
        if ($line -match '^(\w+)=([0-9.]+)$') { $values[$Matches[1]] = [double]$Matches[2] }
    }
    Remove-Item -LiteralPath $out
    if (-not $values.ContainsKey('frame_unix_us')) { throw "Run $i wrote no measurement" }
    $m = ($values['main_unix_us'] - $launch) / 1000
    $f = ($values['frame_unix_us'] - $launch) / 1000
    $toMain += $m
    $toFrame += $f
    if ($values.ContainsKey('app_unix_us') -and $values.ContainsKey('first_ui_unix_us')) {
        $phaseApp += ($values['app_unix_us'] - $values['main_unix_us']) / 1000
        $phaseUi += ($values['first_ui_unix_us'] - $values['app_unix_us']) / 1000
        $phaseFrame += ($values['frame_unix_us'] - $values['first_ui_unix_us']) / 1000
    }
    Write-Host ("  run {0,2}: launch->main {1,6:N1} ms | launch->first frame {2,6:N1} ms" -f $i, $m, $f)
}
# The first run is reported separately: after a fresh build or reboot it includes antivirus
# scanning and cold file caches. The median uses the remaining (warm) runs.
$warmFrame = if ($Runs -gt 1) { $toFrame[1..($Runs - 1)] } else { $toFrame }
$warmMain = if ($Runs -gt 1) { $toMain[1..($Runs - 1)] } else { $toMain }
Write-Host ("Startup launch->first frame: first run {0:N1} ms | warm median {1:N1} ms, min {2:N1} ms" -f $toFrame[0], (Get-Median $warmFrame), ($warmFrame | Measure-Object -Minimum).Minimum)
Write-Host ("Startup launch->main: warm median {0:N1} ms" -f (Get-Median $warmMain))
if ($phaseApp.Count -gt 1) {
    $w = 1..($phaseApp.Count - 1)
    Write-Host ("Startup phases (warm medians): main->app created {0:N1} ms | app->first ui {1:N1} ms | first ui->frame presented {2:N1} ms" -f (Get-Median $phaseApp[$w]), (Get-Median $phaseUi[$w]), (Get-Median $phaseFrame[$w]))
}

# Idle memory.
$p = Start-Target @{}
Start-Sleep -Seconds $IdleSeconds
$p.Refresh()
$ws = $p.WorkingSet64
$priv = $p.PrivateMemorySize64
# Private working set is the "Memory" column of Task Manager. WMI class names are not localized,
# unlike performance counter paths.
$perf = Get-CimInstance Win32_PerfFormattedData_PerfProc_Process -Filter "IDProcess = $($p.Id)"
$privWs = if ($perf) { [double]$perf.WorkingSetPrivate } else { [double]::NaN }
$p.Kill()
$p.WaitForExit()
Write-Host ("Idle memory after {0} s: private working set {1:N1} MB (Task Manager), working set {2:N1} MB, private bytes {3:N1} MB" -f $IdleSeconds, ($privWs / 1MB), ($ws / 1MB), ($priv / 1MB))

# Scrolling cost.
if ($Scroll) {
    if (-not $Arguments) { throw '-Scroll needs a file in -Arguments' }
    $baseCpu = Get-Median $startupCpu
    $wallPerFrame = @()
    $cpuPerFrame = @()
    for ($i = 1; $i -le $ScrollRuns; $i++) {
        $out = [System.IO.Path]::GetTempFileName()
        $p = Start-Target @{ BRACKET_MEASURE_SCROLL = $out }
        if (-not $p.WaitForExit(60000)) { $p.Kill(); throw "Scroll run $i did not exit within 60 s" }
        $cpuMs = $p.TotalProcessorTime.TotalMilliseconds
        $values = @{}
        foreach ($line in Get-Content -LiteralPath $out) {
            if ($line -match '^(\w+)=([0-9.]+)$') { $values[$Matches[1]] = [double]$Matches[2] }
        }
        Remove-Item -LiteralPath $out
        if (-not $values.ContainsKey('scroll_ms')) { throw "Scroll run $i wrote no measurement" }
        $frames = $values['frames']
        $wall = $values['scroll_ms'] / $frames
        $cpuFrame = ($cpuMs - $baseCpu) / $frames
        $wallPerFrame += $wall
        $cpuPerFrame += $cpuFrame
        Write-Host ("  scroll run {0}: {1:N2} ms/frame ({2:N1} fps), CPU {3:N2} ms/frame" -f $i, $wall, (1000 / $wall), $cpuFrame)
    }
    $medianWall = Get-Median $wallPerFrame
    Write-Host ("Scroll (one page per frame): median {0:N2} ms/frame ({1:N1} fps), CPU {2:N2} ms/frame" -f $medianWall, (1000 / $medianWall), (Get-Median $cpuPerFrame))
}

# Startup budget check.
if ($MaxStartupMs -gt 0) {
    $median = Get-Median $warmFrame
    if ($median -gt $MaxStartupMs) {
        Write-Host ("Startup budget exceeded: warm median {0:N1} ms > {1:N0} ms" -f $median, $MaxStartupMs) -ForegroundColor Red
        Remove-Work
        exit 1
    }
    Write-Host ("Startup budget OK: warm median {0:N1} ms <= {1:N0} ms" -f $median, $MaxStartupMs) -ForegroundColor Green
}

Remove-Work
