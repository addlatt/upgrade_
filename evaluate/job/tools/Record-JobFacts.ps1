<#
.SYNOPSIS
    Records what New-Job.ps1's own Get-JobFacts reads on this machine, as
    JSON, for the side-by-side comparison with `upgrade-job facts` (RISKS
    R32, VALIDATION V13). Read-only. The Wi-Fi block is recorded without its
    passwords (ConvertTo-JobWifi's rows carry none).

.DESCRIPTION
    Loads the job writer's functions from ..\..\windows\New-Job.ps1 (the file
    as it is, cut before its main section) and runs Get-JobFacts with the
    same parameters the job writer uses. The result holds the machine's
    program list and folder paths: do not commit it (its name matches
    .gitignore's upgrade-report-* rule).
#>
[CmdletBinding()]
param([string]$ScanDir, [string]$StickDrive, [string]$Out)
$ErrorActionPreference = 'Stop'
# dot-sourcing the job writer's text runs its param block in this scope and
# blanks $ScanDir and $StickDrive, so they are kept under other names first
$recScanDir = $ScanDir; $recStickDrive = $StickDrive; $recOut = $Out
$here = Split-Path -Parent $MyInvocation.MyCommand.Path
$windows = Join-Path $here '..\..\windows'
$src = [IO.File]::ReadAllText((Join-Path $windows 'New-Job.ps1'))
$cut = $src.IndexOf('if ($SelfTest) { Invoke-SelfTest; return }')
if ($cut -lt 0) { throw 'the job writer has no main marker to cut at' }
# The functions are loaded from a file beside New-Job.ps1, not from a text
# block, so that $PSScriptRoot inside them is the folder that holds the
# harvester (Get-JobFolderMap runs Harvest-UpgradeState.ps1 from there).
$tmp = Join-Path $windows 'upgrade-report-recorder-functions.ps1'
[IO.File]::WriteAllText($tmp, $src.Substring(0, $cut), (New-Object Text.UTF8Encoding($false)))
try { . $tmp } finally { Remove-Item -LiteralPath $tmp -Force -ErrorAction SilentlyContinue }
$Out = $recOut
if (-not $Out) { $Out = Join-Path $env:TEMP ('upgrade-report-jobfacts-powershell-' + (Get-Date -Format 'yyyyMMdd-HHmm') + '.json') }
$f = Get-JobFacts -ScanDir $recScanDir -StickDrive $recStickDrive -Materialize $false
$w = ConvertTo-JobWifi -Api (Get-JobWlanProfiles) -StoredCount (Get-JobWlanStoredCount)
$doc = [ordered]@{ Recorder = "Record-JobFacts.ps1 (New-Job.ps1 $JobWriterVersion)"; Now = (Get-Date).ToUniversalTime().ToString('o'); Facts = $f
                   Wifi = $(if ($w.Refusal) { @{ Refusal = $w.Refusal } } else { @{ Refusal = $null; Wifi = $w.Wifi; FileCount = @($w.Files).Count } }) }
[IO.File]::WriteAllText($Out, (($doc | ConvertTo-Json -Depth 12) -replace "`r`n", "`n"), (New-Object Text.UTF8Encoding($false)))
Write-Host "  job facts written: $Out"
Write-Host '  It holds this machine''s program list and folder paths. Do not commit it.'
