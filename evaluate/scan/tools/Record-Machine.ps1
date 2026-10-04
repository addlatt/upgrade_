<#
.SYNOPSIS
    Records what the PowerShell scanner reads on THIS machine and what it
    concludes, so the Rust scanner can be held to it (RISKS R32; V13).

.DESCRIPTION
    Read-only. It runs the scanner's own main section, as written, with its
    own collectors. Each collector's answer is kept on its way through. The
    result is one file:

        upgrade-report-capture-<model>-<stamp>.json

    holding the facts (what was read) and the PowerShell block (the report it
    printed and every check). Then:

        upgrade-scan --replay <that file>

    judges the same facts in Rust and says whether the report is the same,
    line for line.

    The file holds hardware and disk facts and the list of installed
    programs. It is a machine report: never commit it (.gitignore covers the
    name). Run it elevated for the full set of reads.

.PARAMETER OutDir
    Where to write the capture. Default: a captures folder beside this script.
#>
[CmdletBinding()]
param([string]$OutDir)
$ErrorActionPreference = 'Stop'
$here = Split-Path -Parent $MyInvocation.MyCommand.Path
# kept under another name: loading the scanner below brings its own $OutDir
$captureDir = $(if ($OutDir) { $OutDir } else { Join-Path $here 'captures' })
$scannerPath = Join-Path $here '..\..\windows\upgrade-scan.ps1'
$dataDir = Join-Path $here '..\..\..\data'
. (Join-Path $dataDir 'devices.ps1')
. (Join-Path $dataDir 'distros.ps1')
. (Join-Path $dataDir 'releases.ps1')
$full = [IO.File]::ReadAllText($scannerPath)
$cut = $full.IndexOf('if ($SelfTest) { Invoke-UpgSelfTest }')
if ($cut -lt 0) { throw 'record: the scanner has no main marker to cut at' }
. ([scriptblock]::Create(($full.Substring(0, $cut) -replace '(?m)^\. \(Join-Path \$PSScriptRoot .*$', '')))
$mainFrom = $full.IndexOf('$isAdmin = Test-UpgAdmin')
$mainTo = $full.IndexOf('-IsAdmin $isAdmin', $full.IndexOf('= Write-UpgReport -Sys'))
if ($mainFrom -lt 0 -or $mainTo -lt 0) { throw 'record: the scanner main section was not found' }
$mainBlock = [scriptblock]::Create($full.Substring($mainFrom, $mainTo + 17 - $mainFrom))

# one clock for the whole run, so the replay can use the same one
$script:RecNow = Get-Date
function Get-Date { param([string]$Format) if ($Format) { $script:RecNow.ToString($Format) } else { $script:RecNow } }

# every collector, kept on its way through: the original runs, its answer is noted
# (not named $Rec: the scanner's main section has a $rec of its own)
$script:Captured = [ordered]@{}
$orig = @{}
foreach ($n in 'Test-UpgAdmin', 'Get-UpgSystem', 'Get-UpgPnp', 'Get-UpgSecureBootState', 'Get-UpgSbatFacts', 'Get-UpgDbAuthorities', 'Get-UpgResumeFacts', 'Get-UpgDiskFacts',
               'Get-UpgVolumeHealth', 'Get-UpgPhysicalDiskFacts', 'Get-UpgFastStartupState', 'Get-UpgBitLockerState', 'Get-UpgEspFacts', 'Get-UpgInstalledApps') {
    $orig[$n] = (Get-Item "function:$n").ScriptBlock
}
function Test-UpgAdmin { $r = & $orig['Test-UpgAdmin']; $script:Captured.IsAdmin = [bool]$r; $r }
function Get-UpgSystem { $r = & $orig['Get-UpgSystem']; $script:Captured.Sys = $r; $r }
function Get-UpgPnp { $r = @(& $orig['Get-UpgPnp']); $script:Captured.Pnp = $r; $r }
function Get-UpgSecureBootState { $r = & $orig['Get-UpgSecureBootState']; $script:Captured.SecureBoot = $r; $r }
function Get-UpgSbatFacts { param([string]$Root, [bool]$IsAdmin) $r = & $orig['Get-UpgSbatFacts'] -Root $Root -IsAdmin $IsAdmin; $script:Captured.Sbat = $r; $r }
function Get-UpgDbAuthorities { param([byte[]]$Bytes) $r = & $orig['Get-UpgDbAuthorities']; $script:Captured.DbAuthorities = $r; , $r }
function Get-UpgResumeFacts { $r = & $orig['Get-UpgResumeFacts']; $script:Captured.Resume = $r; $r }
function Get-UpgDiskFacts { $r = & $orig['Get-UpgDiskFacts']; $script:Captured.Disk = $r; $r }
function Get-UpgVolumeHealth { param([bool]$IsAdmin, [string]$ShrinkError) $r = & $orig['Get-UpgVolumeHealth'] -IsAdmin $IsAdmin -ShrinkError $ShrinkError; $script:Captured.VolumeHealth = $r; $r }
function Get-UpgPhysicalDiskFacts { param([bool]$IsAdmin) $r = & $orig['Get-UpgPhysicalDiskFacts'] -IsAdmin $IsAdmin; $script:Captured.PhysicalDisk = $r; $r }
function Get-UpgFastStartupState { $r = & $orig['Get-UpgFastStartupState']; $script:Captured.Hiberboot = $r; $r }
function Get-UpgBitLockerState { $r = & $orig['Get-UpgBitLockerState']; $script:Captured.BitLocker = $r; $r }
function Get-UpgEspFacts { $r = & $orig['Get-UpgEspFacts']; $script:Captured.Esp = $r; $r }
function Get-UpgInstalledApps { $r = @(& $orig['Get-UpgInstalledApps']); $script:Captured.Apps = $r; $r }

function Plain {
    # facts -> plain JSON-ready data: dates as 2026-10-04T10:00:00, tables in a fixed order
    param($v)
    if ($null -eq $v) { return $null }
    if ($v -is [DateTime]) { return $v.ToString('yyyy-MM-ddTHH:mm:ss') }
    if ($v -is [string] -or $v -is [bool] -or $v -is [ValueType]) { return $v }
    if ($v -is [Collections.IDictionary]) { $o = [ordered]@{}; foreach ($k in @($v.PSBase.Keys)) { $o["$k"] = Plain $v[$k] }; return $o }
    if ($v -is [array] -or $v -is [Collections.IList]) { return , @($v | ForEach-Object { Plain $_ }) }
    $o = [ordered]@{}
    foreach ($p in $v.PSObject.Properties) { $o[$p.Name] = Plain $p.Value }
    $o
}

Write-Host ''
Write-Host '  recording this machine (read-only)...' -ForegroundColor Cyan
. $mainBlock

$m = [ordered]@{
    Capture        = 'upgrade_ machine capture 1'
    ScannerVersion = $UpgVersion
    Now            = $script:RecNow.ToString('yyyy-MM-ddTHH:mm:ss')
    IsAdmin        = [bool]$script:Captured.IsAdmin
}
foreach ($k in 'Sys', 'Pnp', 'SecureBoot', 'Sbat', 'DbAuthorities', 'Resume', 'Disk', 'VolumeHealth', 'PhysicalDisk', 'Hiberboot', 'BitLocker', 'Esp', 'Apps') {
    $m[$k] = $(if ($script:Captured.Contains($k)) { Plain $script:Captured[$k] } else { $null })
}
$m.PowerShell = [ordered]@{
    Lines          = @($lines)
    Verdict        = "$($verdict.Level)"
    RequiredKernel = $(if ($requiredKernel) { $requiredKernel.ToString() } else { $null })
    Checks         = @($script:Checks | ForEach-Object { [ordered]@{ Section = $_.Section; Title = $_.Title; Status = $_.Status; Detail = $_.Detail; Note = $_.Note; MinKernel = $_.MinKernel; Remedy = $_.Remedy } })
    Releases       = @($script:UpgReleases | Where-Object { $_ } | ForEach-Object { [ordered]@{ Id = $_.Id; Name = $_.Name; Starts = $_.Starts; Why = @($_.Why) } })
    UnmatchedIds   = @($script:Unmatched | Sort-Object -Unique)
}
if (-not (Test-Path $captureDir)) { New-Item -ItemType Directory -Path $captureDir -Force | Out-Null }
$safeModel = ("$($sys.Model)" -replace '[^A-Za-z0-9]+', '-').Trim('-')
$name = 'upgrade-report-capture-{0}-{1}-{2}.json' -f $safeModel, $script:RecNow.ToString('yyyyMMdd-HHmm'), $(if ($script:Captured.IsAdmin) { 'elevated' } else { 'not-elevated' })
$path = Join-Path $captureDir $name
[IO.File]::WriteAllText($path, (($m | ConvertTo-Json -Depth 12) -replace "`r`n", "`n") + "`n", (New-Object Text.UTF8Encoding $false))
Write-Host ''
Write-Host "  verdict: $($verdict.Level)   checks: $($script:Checks.Count)   elevated: $([bool]$script:Captured.IsAdmin)"
Write-Host "  written: $path" -ForegroundColor Cyan
Write-Host '  It holds this machine''s hardware, disk facts and program list. Do not commit it.' -ForegroundColor DarkGray
Write-Host ''
