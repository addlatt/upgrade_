<#
.SYNOPSIS
    Writes golden.json: what the PowerShell scanner says for every case in
    cases.json, word for word. The Rust port (evaluate/scan) must say the same
    (RISKS R32; VALIDATION V13, method 2, "differential").

.DESCRIPTION
    Loads the scanner's own functions from ..\..\windows\upgrade-scan.ps1 (the
    file as it is, cut before its main section), feeds each case's arguments
    to the function the case names, and records every check it emits and
    every value it returns. Nothing here judges anything: it only records.

    Run from WSL:  ./port-check.sh   (or this file with powershell.exe -File)
#>
[CmdletBinding()]
param([string]$Out)

$here = Split-Path -Parent $MyInvocation.MyCommand.Path
$scanner = Join-Path $here '..\..\windows\upgrade-scan.ps1'
$data = Join-Path $here '..\..\..\data'
if (-not $Out) { $Out = Join-Path $here 'golden.json' }

. (Join-Path $data 'devices.ps1')
. (Join-Path $data 'distros.ps1')
. (Join-Path $data 'releases.ps1')
$text = [IO.File]::ReadAllText($scanner)
$cut = $text.IndexOf('if ($SelfTest) { Invoke-UpgSelfTest }')
if ($cut -lt 0) { throw 'golden: the scanner has no main marker to cut at' }
$text = $text.Substring(0, $cut) -replace '(?m)^\. \(Join-Path \$PSScriptRoot .*$', ''
$full = [IO.File]::ReadAllText($scanner)
. ([scriptblock]::Create($text))

# --- a whole scan, replayed (the 'scan' cases) --------------------------------
# The scanner's own main section runs as written, from its first read to the
# report. Only the reads are replaced: each collector hands back the case's
# facts instead of asking the machine, and the clock is the case's clock.
$mainFrom = $full.IndexOf('$isAdmin = Test-UpgAdmin')
$mainTo = $full.IndexOf('-IsAdmin $isAdmin', $full.IndexOf('$lines = Write-UpgReport'))
if ($mainFrom -lt 0 -or $mainTo -lt 0) { throw 'golden: the scanner main section was not found' }
$mainBlock = [scriptblock]::Create($full.Substring($mainFrom, $mainTo + 17 - $mainFrom))
$script:Now = [DateTime]'2026-10-04T10:00:00'
function Get-Date { param([string]$Format) if ($Format) { $script:Now.ToString($Format) } else { $script:Now } }
function Test-UpgAdmin { [bool]$script:M.IsAdmin }
function Get-UpgSystem { $script:M.Sys }
function Get-UpgPnp { @($script:M.Pnp) }
function Get-UpgSecureBootState { $script:M.SecureBoot }
function Get-UpgSbatFacts { param($Root, $IsAdmin) @{ Levels = @($script:M.Sbat.Levels | Where-Object { $_ }); Files = @($script:M.Sbat.Files | Where-Object { $_ }) } }
function Get-UpgDbAuthorities { param($Bytes) if ($null -eq $script:M.DbAuthorities) { return $null }; , @($script:M.DbAuthorities) }
function Get-UpgResumeFacts { $script:M.Resume }
function Get-UpgDiskFacts { $script:M.Disk }
function Get-UpgVolumeHealth { param($IsAdmin, $ShrinkError) $script:M.VolumeHealth }
function Get-UpgPhysicalDiskFacts { param($IsAdmin) $script:M.PhysicalDisk }
function Get-UpgFastStartupState { $script:M.Hiberboot }
function Get-UpgBitLockerState { $script:M.BitLocker }
function Get-UpgEspFacts { $script:M.Esp }
function Get-UpgInstalledApps { @($script:M.Apps) }

$dateKeys = @('NtfsFullChkdsk', 'LastCheck', 'First', 'Last', 'TimeCreated', 'When', 'Now')
function Fix {
    # JSON -> what the live collectors hand the judging functions: doubles
    # (not decimals) and real DateTime values.
    param($v, [string]$Key)
    if ($null -eq $v) { return $null }
    if ($v -is [decimal]) { return [double]$v }
    if ($v -is [string]) {
        if ($dateKeys -contains $Key) { return [DateTime]::ParseExact($v, 'yyyy-MM-ddTHH:mm:ss', [Globalization.CultureInfo]::InvariantCulture) }
        return $v
    }
    if ($v -is [array]) { return , @($v | ForEach-Object { Fix $_ $Key }) }
    if ($v -is [pscustomobject]) {
        foreach ($p in @($v.PSObject.Properties)) { $p.Value = Fix $p.Value $p.Name }
        return $v
    }
    $v
}
function Norm {
    # a returned value -> plain JSON-ready data
    param($v)
    if ($null -eq $v) { return $null }
    if ($v -is [DateTime]) { return $v.ToString('yyyy-MM-ddTHH:mm:ss') }
    if ($v -is [version]) { return $v.ToString() }
    if ($v -is [Collections.IDictionary]) {
        $o = [ordered]@{}
        foreach ($k in @($v.Keys | Sort-Object { [string]$_ })) { $o["$k"] = Norm $v[$k] }
        return $o
    }
    if ($v -is [array]) { return , @($v | ForEach-Object { Norm $_ }) }
    if ($v -is [pscustomobject]) {
        $o = [ordered]@{}
        foreach ($p in $v.PSObject.Properties) { $o[$p.Name] = Norm $p.Value }
        return $o
    }
    $v
}

$cases = Get-Content (Join-Path $here 'cases.json') -Raw -Encoding UTF8 | ConvertFrom-Json
$outLines = @()
foreach ($case in $cases) {
    $script:Checks = @(); $script:Unmatched = @(); $script:UpgReleases = $null
    $r = [ordered]@{}
    $returns = @()
    foreach ($call in @($case.calls)) {
        $a = @{}
        foreach ($p in @($call.args.PSObject.Properties)) { $a[$p.Name] = Fix $p.Value $p.Name }
        switch ($call.fn) {
            'verdict' {
                foreach ($c in @($a.Checks)) { New-UpgCheck -Section $c.Section -Title $c.Title -Status $c.Status -Detail $c.Detail -MinKernel $c.MinKernel }
                $k = Get-UpgRequiredKernel
                $v = Get-UpgVerdict
                $rec = Get-UpgRecommendation -RequiredKernel $k
                $r.verdict = [ordered]@{ Level = $v.Level; Summary = $v.Summary; Groups = @($v.Groups | ForEach-Object { [ordered]@{ Priority = $_.Priority; Label = $_.Label; Items = @($_.Items) } }) }
                $r.kernel = $(if ($k) { $k.ToString() } else { $null })
                $r.recommendation = [ordered]@{ Distros = @($rec.Distros | ForEach-Object { $_.Name }); HasNvidia = $rec.HasNvidia; LowRam = $rec.LowRam; Excluded = @($rec.Excluded | ForEach-Object { $_.Name }) }
                $returns += , $null
            }
            'corpus' {
                $cap = Get-Content (Join-Path $here "..\..\windows\corpus\$($a.File)") -Raw | ConvertFrom-Json
                $pnp = @($cap.Pnp)
                Test-UpgArchitecture -Sys $cap.Sys
                Test-UpgMemory       -Sys $cap.Sys
                Test-UpgStorageMode  -Pnp $pnp
                Test-UpgWifi         -Pnp $pnp
                Test-UpgGpu          -Pnp $pnp
                Test-UpgAudio        -Pnp $pnp
                Test-UpgVendor       -Sys $cap.Sys
                $returns += , $null
            }
            'scan' {
                $script:Now = $a.Now
                $m = $a.Machine
                if ($m.Corpus) {
                    $cap = Get-Content (Join-Path $here "..\..\windows\corpus\$($m.Corpus)") -Raw | ConvertFrom-Json
                    foreach ($p in @($m.Sys.PSObject.Properties)) { $cap.Sys | Add-Member -NotePropertyName $p.Name -NotePropertyValue $p.Value -Force }
                    $m | Add-Member -NotePropertyName Sys -NotePropertyValue $cap.Sys -Force
                    $m | Add-Member -NotePropertyName Pnp -NotePropertyValue @($cap.Pnp) -Force
                }
                $script:M = $m
                . $mainBlock 6>$null
                $r.lines = @($lines)
                $r.verdict = [ordered]@{ Level = $verdict.Level; Summary = $verdict.Summary; Groups = @($verdict.Groups | ForEach-Object { [ordered]@{ Priority = $_.Priority; Label = $_.Label; Items = @($_.Items) } }) }
                $r.kernel = $(if ($requiredKernel) { $requiredKernel.ToString() } else { $null })
                $r.recommendation = [ordered]@{ Distros = @($rec.Distros | ForEach-Object { $_.Name }); HasNvidia = $rec.HasNvidia; LowRam = $rec.LowRam; Excluded = @($rec.Excluded | ForEach-Object { $_.Name }) }
                $r.unmatchedIds = @($script:Unmatched | Sort-Object -Unique)
                $script:Now = [DateTime]'2026-10-04T10:00:00'
                $returns += , $null
            }
            'distro-table' {
                $returns += , @(Get-UpgDistroTable | ForEach-Object { $v = ConvertTo-UpgVersion $_.Kernel; [ordered]@{ Name = $_.Name; Kernel = $_.Kernel; Parsed = $(if ($v) { $v.ToString() } else { $null }) } })
            }
            'Test-UpgReleases' {
                $level = ConvertFrom-UpgSbatText $a.LevelText
                $table = if ($a.Table -is [string] -and $a.Table -eq 'data') { Get-UpgReleaseTable } else { @($a.Table) }
                Test-UpgReleases -SecureBoot $a.SecureBoot -Level $level -DbAuthorities $a.DbAuthorities -Table $table
                $returns += , $null
            }
            default {
                $got = & $call.fn @a
                $returns += , (Norm $got)
            }
        }
    }
    $r.checks = @($script:Checks | ForEach-Object { [ordered]@{ Section = $_.Section; Title = $_.Title; Status = $_.Status; Detail = $_.Detail; Note = $_.Note; MinKernel = $_.MinKernel; Remedy = $_.Remedy } })
    $r.unmatched = @($script:Unmatched)
    $r.releases = @($script:UpgReleases | Where-Object { $_ } | ForEach-Object { [ordered]@{ Id = $_.Id; Name = $_.Name; Starts = $_.Starts; Why = @($_.Why) } })
    $r.returns = $returns
    $outLines += ((ConvertTo-Json $case.name -Compress) + ': ' + (ConvertTo-Json $r -Depth 12 -Compress))
}
$outText = "{`n" + ($outLines -join ",`n") + "`n}`n"
[IO.File]::WriteAllText($Out, $outText, (New-Object Text.UTF8Encoding $false))
Write-Host "  golden: $($outLines.Count) cases written to $Out"
