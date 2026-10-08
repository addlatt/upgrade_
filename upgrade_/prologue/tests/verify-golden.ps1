<#
.SYNOPSIS
    Writes verify-golden.json: what Test-Handoff.ps1's judging functions
    return for every call in verify-cases.json. The Rust port
    (upgrade_/prologue/src/verify.rs, with judge.rs) must return the same
    (RISKS R32; VALIDATION V13).

.DESCRIPTION
    Loads the harness's functions from ..\..\windows\Test-Handoff.ps1, from
    its first global ($ErrorActionPreference) to the main marker, so its
    parameter block (parameter sets with mandatory switches) is left out.
    A few names stand for a self-test case that is not one call: the clean
    grubenv block's shape, a grubenv built from text, the CSV header written
    to a file. Records only; nothing on this machine is read.
#>
[CmdletBinding()]
param([string]$Out)
$here = Split-Path -Parent $MyInvocation.MyCommand.Path
if (-not $Out) { $Out = Join-Path $here 'verify-golden.json' }
$src = [IO.File]::ReadAllText((Join-Path $here '..\..\windows\Test-Handoff.ps1'))
$from = $src.IndexOf("`$ErrorActionPreference = 'Stop'")
$cut = $src.IndexOf('if ($SelfTest) { Invoke-SelfTest; return }')
if ($from -lt 0 -or $cut -lt 0) { throw 'verify-golden: the harness has no markers to cut at' }
. ([scriptblock]::Create($src.Substring($from, $cut - $from)))

$cases = (Get-Content (Join-Path $here 'verify-cases.json') -Raw | ConvertFrom-Json)
$calls = @()
foreach ($c in $cases) {
    $a = @($c.args)
    $r = switch ($c.fn) {
        'handoff_result'     { Get-HandoffResult -Fired ([bool]$a[0]) -SequenceCleared ([bool]$a[1]) -OrderUnchanged ([bool]$a[2]) -FailMode ([string]$a[3]) }
        'manage_bde'         { ConvertFrom-ManageBdeStatus -Lines @($a[0] | ForEach-Object { [string]$_ }) }
        'grubenv_clean'      { $b = New-GrubEnvBlock; if ($b.Length -eq 1024 -and [Text.Encoding]::ASCII.GetString($b, 0, 25) -eq "# GRUB Environment Block`n" -and $b[1023] -eq 0x23) { 'ok' } else { "bad: len=$($b.Length)" } }
        'grubenv_fired_clean' { Test-GrubEnvFired -Bytes (New-GrubEnvBlock) }
        'grubenv_fired_text' { $t = [string]$a[0] + ('#' * [int]$a[1]); Test-GrubEnvFired -Bytes ([Text.Encoding]::ASCII.GetBytes($t)) }
        'payload_path'       { try { Get-PayloadPath -PayloadName ([string]$a[0]) -FailMode ([string]$a[1]) } catch { 'refused' } }
        'find_stick_root'    { $x = Find-StickRoot -UniqueId ([string]$a[1]) -Volumes @($a[0] | ForEach-Object { [pscustomobject]@{ DriveLetter = $_.DriveLetter; UniqueId = $_.UniqueId } }); if ($null -eq $x) { 'null' } else { $x } }
        'csv_header_bom'     { $f = [IO.Path]::GetTempFileName(); Remove-Item $f -Force; [IO.File]::WriteAllText($f, $CsvHeader + "`r`n", (New-Object Text.UTF8Encoding($false))); $b = [IO.File]::ReadAllBytes($f); Remove-Item $f -Force -ErrorAction SilentlyContinue; if ($b[0] -eq 0xEF -and $b[1] -eq 0xBB -and $b[2] -eq 0xBF) { 'bom' } else { 'no-bom' } }
        'csv_header_text'    { $f = [IO.Path]::GetTempFileName(); Remove-Item $f -Force; [IO.File]::WriteAllText($f, $CsvHeader + "`r`n", (New-Object Text.UTF8Encoding($false))); $h = (Get-Content $f -TotalCount 1); Remove-Item $f -Force -ErrorAction SilentlyContinue; if ($h -eq $CsvHeader) { 'ok' } else { "differs: $h" } }
        'drive_root'         { try { Get-DriveRoot ([string]$a[0]) } catch { 'refused' } }
        default              { throw "verify-golden: no recorder for $($c.fn)" }
    }
    $calls += , [ordered]@{ name = $c.name; fn = $c.fn; result = $r }
}
$doc = [ordered]@{ recorder = "verify-golden.ps1 (Test-Handoff.ps1 $HarnessVersion)"; harness_version = $HarnessVersion; calls = $calls
                   csv_header = $CsvHeader; payload_paths = [ordered]@{ shim = $PayloadPaths['shim']; shell = $PayloadPaths['shell'] }
                   return_task_name = $ReturnTaskName; fired_marker = $FiredMarker }
[IO.File]::WriteAllText($Out, (($doc | ConvertTo-Json -Depth 6) -replace "`r`n", "`n") + "`n", (New-Object Text.UTF8Encoding($false)))
Write-Host "  verify-golden: $($calls.Count) calls recorded from Test-Handoff.ps1 $HarnessVersion -> $Out"
