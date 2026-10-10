<#
.SYNOPSIS
    Writes golden.json: what the PowerShell harvester's pure functions return
    for every call in cases.json. The Rust port (evaluate/harvest) must
    return the same (RISKS R32; VALIDATION V13).

.DESCRIPTION
    Loads the harvester's own functions from ..\..\windows\Harvest-UpgradeState.ps1
    (the file as it is, cut before its main section). Records only.
#>
[CmdletBinding()]
param([string]$Out)
$here = Split-Path -Parent $MyInvocation.MyCommand.Path
if (-not $Out) { $Out = Join-Path $here 'golden.json' }
$src = [IO.File]::ReadAllText((Join-Path $here '..\..\windows\Harvest-UpgradeState.ps1'))
$cut = $src.IndexOf("#  main")
if ($cut -lt 0) { throw 'golden: the harvester has no main marker to cut at' }
. ([scriptblock]::Create($src.Substring(0, $src.LastIndexOf('# ====', $cut))))

function Norm {
    param($v)
    if ($null -eq $v) { return $null }
    if ($v -is [array]) { return , @($v | ForEach-Object { Norm $_ }) }
    if ($v -is [pscustomobject]) {
        $o = [ordered]@{}
        foreach ($p in $v.PSObject.Properties) { $o[$p.Name] = Norm $p.Value }
        return $o
    }
    $v
}
$tmp = Join-Path $env:TEMP ('upgrade-harvest-golden-' + [guid]::NewGuid().ToString('N') + '.xml')
$cases = Get-Content (Join-Path $here 'cases.json') -Raw -Encoding UTF8 | ConvertFrom-Json
$outLines = @()
try {
    foreach ($case in $cases) {
        $returns = @()
        foreach ($call in @($case.calls)) {
            $a = @{}
            foreach ($p in @($call.args.PSObject.Properties)) { $a[$p.Name] = $p.Value }
            try {
            if ($call.fn -eq 'ConvertFrom-HarvestWlanProfileXml') {
                # as netsh writes it: a file, UTF-8 with a byte order mark
                [IO.File]::WriteAllText($tmp, $a.Xml, (New-Object Text.UTF8Encoding $true))
                $got = ConvertFrom-HarvestWlanProfileXml -XmlPath $tmp -IncludeSecrets $a.IncludeSecrets
            } else {
                foreach ($k in 'Folders', 'UserFolders', 'Browsers', 'ExternalDrives') { if ($a.ContainsKey($k)) { $a[$k] = @($a[$k]) } }
                $got = & $call.fn @a
            }
            } catch { $got = [pscustomobject]@{ threw = $true } }   # the words are PowerShell's own; only the fact is recorded
            $returns += , (Norm $got)
        }
        $outLines += ((ConvertTo-Json $case.name -Compress) + ': ' + (ConvertTo-Json @($returns) -Depth 8 -Compress))
    }
} finally { Remove-Item $tmp -Force -ErrorAction SilentlyContinue }
$outText = "{`n" + ($outLines -join ",`n") + "`n}`n"
[IO.File]::WriteAllText($Out, $outText, (New-Object Text.UTF8Encoding $false))
Write-Host "  golden: $($outLines.Count) cases written to $Out"
