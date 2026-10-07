<#
.SYNOPSIS
    Writes golden.json: what Invoke-Prologue.ps1's judging functions return
    for every call in cases.json. The Rust port (upgrade_/prologue) must
    return the same (RISKS R32; VALIDATION V13).

.DESCRIPTION
    Loads the prologue's functions from ..\..\windows\Invoke-Prologue.ps1:
    the file as it is, from its first global ($ErrorActionPreference) to the
    main marker, so its parameter block (which has mandatory parameters) is
    left out. The clock is fixed. A few calls stand for a self-test case
    that works on a state: 'block' makes a fresh state, applies the case's
    edits and asks New-PrologueBlock; 'stopped' does the same for
    New-PrologueStoppedOutcome; 'state' records what a fresh state carries.
    Records only.
#>
[CmdletBinding()]
param([string]$Out)
$here = Split-Path -Parent $MyInvocation.MyCommand.Path
if (-not $Out) { $Out = Join-Path $here 'golden.json' }
$src = [IO.File]::ReadAllText((Join-Path $here '..\..\windows\Invoke-Prologue.ps1'))
$from = $src.IndexOf("`$ErrorActionPreference = 'Stop'")
$cut = $src.IndexOf('if ($SelfTest) { Invoke-SelfTest; return }')
if ($from -lt 0 -or $cut -lt 0) { throw 'golden: the prologue has no markers to cut at' }
. ([scriptblock]::Create($src.Substring($from, $cut - $from)))
function Get-Date { param([string]$Format) $d = [DateTime]::SpecifyKind([DateTime]'2026-10-07T12:00:00', 'Utc'); if ($Format) { $d.ToString($Format) } else { $d } }

function Fix {
    # JSON -> what the live halves hand over: doubles, DateTimes, ordered hashtables for the facts
    param($v)
    if ($null -eq $v) { return $null }
    if ($v -is [decimal]) { return [double]$v }
    if ($v -is [array]) { return , @($v | ForEach-Object { Fix $_ }) }
    if ($v.GetType().Name -eq 'PSCustomObject') {
        if (@($v.PSObject.Properties).Count -eq 1 -and $v.PSObject.Properties['date']) { return [DateTime]::ParseExact($v.date, 'yyyy-MM-ddTHH:mm:ss', [Globalization.CultureInfo]::InvariantCulture) }
        foreach ($p in @($v.PSObject.Properties)) { $p.Value = Fix $p.Value }
    }
    $v
}
function ToHash {
    # the state is worked on as ordered hashtables, as the script keeps it
    param($v)
    if ($null -eq $v) { return $null }
    if ($v -is [array]) { return , @($v | ForEach-Object { ToHash $_ }) }
    if ($v.GetType().Name -eq 'PSCustomObject') { $h = [ordered]@{}; foreach ($p in $v.PSObject.Properties) { $h[$p.Name] = ToHash $p.Value }; return $h }
    $v
}
function Norm {
    param($v)
    if ($null -eq $v) { return $null }
    if ($v -is [DateTime]) { return $v.ToString('yyyy-MM-ddTHH:mm:ss') }
    if ($v -is [byte[]]) { return [Convert]::ToBase64String($v) }
    if ($v -is [Collections.Specialized.OrderedDictionary]) { $o = [ordered]@{}; foreach ($k in @($v.PSBase.Keys)) { $o["$k"] = Norm $v[$k] }; return $o }
    if ($v -is [Collections.IDictionary]) { $o = [ordered]@{}; foreach ($k in @($v.PSBase.Keys | Sort-Object { [string]$_ })) { $o["$k"] = Norm $v[$k] }; return $o }
    if ($v -is [array] -or $v -is [Collections.IList]) { return , @($v | ForEach-Object { Norm $_ }) }
    if ($v.GetType().Name -eq 'PSCustomObject') { $o = [ordered]@{}; foreach ($p in $v.PSObject.Properties) { $o[$p.Name] = Norm $p.Value }; return $o }
    $v
}
function Edit-State {
    # the case's edits laid over a fresh state, nested keys merged
    param($S, $Edits)
    foreach ($p in @($Edits.PSObject.Properties)) {
        $v = ToHash (Fix $p.Value)
        if ($v -is [Collections.IDictionary] -and $S[$p.Name] -is [Collections.IDictionary]) { foreach ($k in @($v.PSBase.Keys)) { $S[$p.Name][$k] = $v[$k] } }
        else { $S[$p.Name] = $v }
    }
    $S
}
function RoundTrip { param($o) ConvertTo-PrologueHashtable ((ConvertTo-PrologueJson $o) | ConvertFrom-Json) }

$cases = Get-Content (Join-Path $here 'cases.json') -Raw -Encoding UTF8 | ConvertFrom-Json
$outLines = @()
foreach ($case in $cases) {
    $returns = @()
    foreach ($call in @($case.calls)) {
        $a = @{}
        foreach ($p in @($call.args.PSObject.Properties)) { $a[$p.Name] = Fix $p.Value }
        try {
            $got = switch ($call.fn) {
                'block' { $s = Edit-State (New-PrologueState -JobId 'j' -StickId 's' -Root 'E:\') $call.args.State; if ($a.RoundTrip) { $s = RoundTrip $s }; New-PrologueBlock $s }
                'stopped' { $s = Edit-State (New-PrologueState -JobId 'j' -StickId 's' -Root 'E:\') $call.args.State; New-PrologueStoppedOutcome -Job $a.Job -S $s -StoppedAt $a.StoppedAt -Reason $a.Reason -WindowsPartition (ToHash $a.WindowsPartition) }
                'state' { $s = New-PrologueState -JobId 'j' -StickId 's' -Root 'E:\'; $old = RoundTrip $s; $old.Remove('Resumes'); [ordered]@{ Resumes = @($s.Resumes).Count; OldContains = $old.Contains('Resumes'); Json = (ConvertTo-PrologueJson $s) } }
                'grubenv-clean' { $b = New-GrubEnvBlock; [ordered]@{ Length = $b.Length; Fired = (Test-GrubEnvFired $b) } }
                'Test-GrubEnvFired' { Test-GrubEnvFired ([Text.Encoding]::ASCII.GetBytes("$($a.Text)")) }
                'trigger-of-queued' { $t = Test-PrologueRepairQueued -VolumeStatus $a.VolumeStatus -NtfsFullChkdsk $a.NtfsFullChkdsk -LastCheck $a.LastCheck; Get-PrologueVolumeTrigger -Dirty $a.Dirty -RepairQueued ([bool]$t.Queued) }
                'Get-PrologueRestorePlan' { $sh = ToHash $a.Shrink; if ($a.RoundTrip) { $sh = RoundTrip $sh }; if ($a.KeepHibernationOff) { , @(Get-PrologueRestorePlan -Shrink $sh -KeepHibernationOff) } else { , @(Get-PrologueRestorePlan -Shrink $sh) } }
                'Compare-PrologueJob' { , @(Compare-PrologueJob -Job $a.Job -F (ToHash $a.F)) }
                'Compare-PrologueEraseDisks' { , @(Compare-PrologueEraseDisks -Job $a.Job -F (ToHash $a.F)) }
                'ConvertFrom-PrologueDiskEvents' { ConvertFrom-PrologueDiskEvents -Events $a.Events -DiskNumber $a.DiskNumber }
                'Get-DriveRoot' { try { Get-DriveRoot $a.Letter } catch { [ordered]@{ threw = "$($_.Exception.Message)" } } }
                'filesystem-only' { 'filesystem-only' }
                default { & $call.fn @a }
            }
            $returns += , (Norm $got)
        } catch { $returns += , ([ordered]@{ threw = "$($_.Exception.Message)" }) }
    }
    $json = ConvertTo-Json @($returns) -Depth 14 -Compress
    $outLines += ((ConvertTo-Json $case.name -Compress) + ': ' + $json)
}
$outText = "{`n" + ($outLines -join ",`n") + "`n}`n"
[IO.File]::WriteAllText($Out, $outText, (New-Object Text.UTF8Encoding $false))
Write-Host "  golden: $($outLines.Count) cases written to $Out"
