<#
.SYNOPSIS
    Records what Invoke-Prologue.ps1's own Get-PrologueFacts reads on this
    machine, as JSON, for the side-by-side comparison with
    `upgrade-prologue facts` (RISKS R32, VALIDATION V13). Read-only; the
    file holds disk identities and is never committed (its name matches
    .gitignore's upgrade-report-* rule).

.DESCRIPTION
    Loads the prologue's functions from ..\..\windows\Invoke-Prologue.ps1,
    from its first global to its main marker (past the parameter block,
    which has mandatory parameters), and runs Get-PrologueFacts for the
    stick given.
#>
[CmdletBinding()]
param([string]$StickDrive, [string]$Out)
$ErrorActionPreference = 'Stop'
$recStick = $StickDrive; $recOut = $Out
$here = Split-Path -Parent $MyInvocation.MyCommand.Path
$src = [IO.File]::ReadAllText((Join-Path $here '..\..\windows\Invoke-Prologue.ps1'))
$from = $src.IndexOf("`$ErrorActionPreference = 'Stop'")
$cut = $src.IndexOf('if ($SelfTest) { Invoke-SelfTest; return }')
if ($from -lt 0 -or $cut -lt 0) { throw 'the prologue has no markers to cut at' }
. ([scriptblock]::Create($src.Substring($from, $cut - $from)))
if (-not $recOut) { $recOut = Join-Path $env:TEMP ('upgrade-report-prologuefacts-powershell-' + (Get-Date -Format 'yyyyMMdd-HHmm') + '.json') }
$f = Get-PrologueFacts -Root (Get-DriveRoot $recStick)
$doc = [ordered]@{ Recorder = "Record-PrologueFacts.ps1 (Invoke-Prologue.ps1 $PrologueVersion)"; Now = (Get-Date).ToUniversalTime().ToString('o'); Facts = $f }
[IO.File]::WriteAllText($recOut, (($doc | ConvertTo-Json -Depth 10) -replace "`r`n", "`n"), (New-Object Text.UTF8Encoding($false)))
Write-Host "  prologue facts written: $recOut"
