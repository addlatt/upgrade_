<#
.SYNOPSIS
    Writes golden.json: what New-Job.ps1's pure functions return for every
    call in cases.json. The Rust port (evaluate/job) must return the same
    (RISKS R32; VALIDATION V13).

.DESCRIPTION
    Loads the job writer's own functions from ..\..\windows\New-Job.ps1 (the
    file as it is, cut before its main section). A job's facts are
    base-facts.json with the case's top-level replacements. The clock is
    fixed and the job id is replaced by a constant, so the file is the same
    on every run. Records only.
#>
[CmdletBinding()]
param([string]$Out)
$here = Split-Path -Parent $MyInvocation.MyCommand.Path
if (-not $Out) { $Out = Join-Path $here 'golden.json' }
$src = [IO.File]::ReadAllText((Join-Path $here '..\..\windows\New-Job.ps1'))
$cut = $src.IndexOf('if ($SelfTest) { Invoke-SelfTest; return }')
if ($cut -lt 0) { throw 'golden: the job writer has no main marker to cut at' }
. ([scriptblock]::Create($src.Substring(0, $cut)))
function Get-Date { [DateTime]::SpecifyKind([DateTime]'2026-10-04T12:00:00', 'Utc') }
$fixedId = '00000000-0000-4000-8000-000000000000'

function Fix {
    # JSON -> what the live reads hand over: doubles, not decimals
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
function Norm {
    param($v)
    if ($null -eq $v) { return $null }
    if ($v -is [Collections.Specialized.OrderedDictionary]) { $o = [ordered]@{}; foreach ($k in @($v.PSBase.Keys)) { $o["$k"] = Norm $v[$k] }; return $o }   # PSBase: an entry named 'keys' hides .Keys
    if ($v -is [Collections.IDictionary]) { $o = [ordered]@{}; foreach ($k in @($v.PSBase.Keys | Sort-Object { [string]$_ })) { $o["$k"] = Norm $v[$k] }; return $o }
    if ($v -is [array] -or $v -is [Collections.IList]) { return , @($v | ForEach-Object { Norm $_ }) }
    if ($v.GetType().Name -eq 'PSCustomObject') { $o = [ordered]@{}; foreach ($p in $v.PSObject.Properties) { $o[$p.Name] = Norm $p.Value }; return $o }
    $v
}
$baseText = Get-Content (Join-Path $here 'base-facts.json') -Raw -Encoding UTF8
$cases = Get-Content (Join-Path $here 'cases.json') -Raw -Encoding UTF8 | ConvertFrom-Json
$outLines = @()
foreach ($case in $cases) {
    $returns = @()
    foreach ($call in @($case.calls)) {
        $a = @{}
        foreach ($p in @($call.args.PSObject.Properties)) { $a[$p.Name] = Fix $p.Value }
        try {
            if ($call.fn -eq 'New-JobDocument') {
                $facts = Fix ($baseText | ConvertFrom-Json)
                foreach ($p in @($call.args.F.PSObject.Properties)) { $facts | Add-Member -NotePropertyName $p.Name -NotePropertyValue (Fix $p.Value) -Force }
                $a.F = $facts
                $got = New-JobDocument @a
                if ($got.Job) { $got.Job.job_id = $fixedId }
            } else {
                $got = & $call.fn @a
            }
            $returns += , (Norm $got)
        } catch { $returns += , ([ordered]@{ threw = "$($_.Exception.Message)" }) }
    }
    $outLines += ((ConvertTo-Json $case.name -Compress) + ': ' + (ConvertTo-Json @($returns) -Depth 12 -Compress))
}
$outText = "{`n" + ($outLines -join ",`n") + "`n}`n"
[IO.File]::WriteAllText($Out, $outText, (New-Object Text.UTF8Encoding $false))
Write-Host "  golden: $($outLines.Count) cases written to $Out"
