<#
.SYNOPSIS
    Writes rollback-golden.json: what Invoke-Rollback.ps1's judging functions
    return for every call in rollback-cases.json. The Rust port
    (upgrade_/prologue/src/rollback.rs) must return the same (RISKS R32;
    VALIDATION V13).

.DESCRIPTION
    Loads the script's functions from ..\..\windows\Invoke-Rollback.ps1, from
    its first global ($ErrorActionPreference) to its self-test, so its
    parameter block is left out. The self-test's fixtures (the two-line
    snapshot manifest, the outcome, the job) are rebuilt here word for word;
    "fixture" in a case's arguments means that one. A plan is recorded as
    its refusals, its Restore and its WantSha. Records only.
#>
[CmdletBinding()]
param([string]$Out)
$here = Split-Path -Parent $MyInvocation.MyCommand.Path
if (-not $Out) { $Out = Join-Path $here 'rollback-golden.json' }
$src = [IO.File]::ReadAllText((Join-Path $here '..\..\windows\Invoke-Rollback.ps1'))
$from = $src.IndexOf("`$ErrorActionPreference = 'Stop'")
$cut = $src.IndexOf('function Invoke-SelfTest')
if ($from -lt 0 -or $cut -lt 0) { throw 'rollback-golden: the script has no markers to cut at' }
. ([scriptblock]::Create($src.Substring($from, $cut - $from)))

$fixSums = ConvertFrom-RollbackSums @(
    'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa  ./EFI/Boot/bootx64.efi',
    'bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb  ./EFI/Microsoft/Boot/bootmgfw.efi')
function New-FixOut { [pscustomobject]@{ schema = 'outcome/1'; path_taken = 'keep-windows'; windows = [pscustomobject]@{ kept = $true }; cutover = [pscustomobject]@{ esp_snapshot = [pscustomobject]@{ path = 'upgrade_/esp-snapshot' } } } }
$fixJob = [pscustomobject]@{ identity = [pscustomobject]@{ system_disk = [pscustomobject]@{ unique_id = 'eui.1'; size_bytes = 100 } } }
function SumsOf($a) { if ("$a" -eq 'fixture') { return $fixSums }; ConvertFrom-RollbackSums @($a | ForEach-Object { [string]$_ }) }
function OutOf($a) {
    if ($null -eq $a) { return $null }
    if ("$a" -eq 'fixture') { return New-FixOut }
    $o = New-FixOut
    foreach ($p in $a.PSObject.Properties) {
        $parts = $p.Name -split '\.'
        $cur = $o
        for ($i = 0; $i -lt $parts.Count - 1; $i++) { $cur = $cur.($parts[$i]) }
        $cur.($parts[-1]) = $p.Value
    }
    $o
}
function SortedSums($h) { $o = [ordered]@{}; foreach ($k in @($h.Keys | Sort-Object)) { $o[$k] = $h[$k] }; $o }

$cases = (Get-Content (Join-Path $here 'rollback-cases.json') -Raw | ConvertFrom-Json)
$calls = @()
foreach ($c in $cases) {
    $a = @($c.args)
    $r = switch ($c.fn) {
        'sums'          { SortedSums (SumsOf $a[0]) }
        'plan'          { $p = Get-RollbackPlan -Outcome (OutOf $a[0]) -Sums (SumsOf $a[1]) -CurrentSha ([string]$a[2]) -IdentityMismatches @($a[3] | ForEach-Object { [string]$_ })
                          [ordered]@{ Refusals = @($p.Refusals | ForEach-Object { [string]$_ }); Restore = [bool]$p.Restore; WantSha = $(if ($null -eq $p.WantSha) { $null } else { [string]$p.WantSha }) } }
        'identity'      { , @($a[0] | ForEach-Object { , @(Compare-RollbackIdentity -Job $fixJob -Disk @{ UniqueId = $_.UniqueId; Size = $_.Size } | ForEach-Object { [string]$_ }) }) }
        'displayorder'  { , @(Get-DisplayOrderTokens ([string]$a[0]) | ForEach-Object { [string]$_ }) }
        'windows_first' { , @($a[0] | ForEach-Object { [bool](Test-WindowsFirst @($_ | ForEach-Object { [string]$_ })) }) }
        'drive'         { , @($a[0] | ForEach-Object { try { Get-DriveRoot ([string]$_) } catch { 'refused' } }) }
        default         { throw "rollback-golden: no recorder for $($c.fn)" }
    }
    $calls += , [ordered]@{ name = $c.name; fn = $c.fn; result = $r }
}
$doc = [ordered]@{ recorder = "rollback-golden.ps1 (Invoke-Rollback.ps1 $RollbackVersion)"; rollback_version = $RollbackVersion; calls = $calls }
[IO.File]::WriteAllText($Out, (($doc | ConvertTo-Json -Depth 8) -replace "`r`n", "`n") + "`n", (New-Object Text.UTF8Encoding($false)))
Write-Host "  rollback-golden: $($calls.Count) calls recorded from Invoke-Rollback.ps1 $RollbackVersion -> $Out"
