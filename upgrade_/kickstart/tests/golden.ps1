<#
.SYNOPSIS
    Writes golden.json: what New-Kickstart.ps1 produces, or the words it
    refuses with, for every input in cases.json. The Rust generator
    (upgrade_/kickstart) must produce the same (RISKS R32; VALIDATION V13).

.DESCRIPTION
    Loads the generator's own functions from ..\..\windows\New-Kickstart.ps1
    (the file as it is, cut before it runs). Each input is an example job
    from schemas\examples with the case's edits applied. Records only.
#>
[CmdletBinding()]
param([string]$Out)
$here = Split-Path -Parent $MyInvocation.MyCommand.Path
if (-not $Out) { $Out = Join-Path $here 'golden.json' }
$src = [IO.File]::ReadAllText((Join-Path $here '..\..\windows\New-Kickstart.ps1'))
$cut = $src.IndexOf('if ($SelfTest) { Invoke-SelfTest; return }')
if ($cut -lt 0) { throw 'golden: the generator has no main marker to cut at' }
. ([scriptblock]::Create($src.Substring(0, $cut)))
$examples = Join-Path $here '..\..\..\schemas\examples'

function Edit-Doc {
    # one edit: (path, value) sets, (path) deletes
    param($Doc, $Op)
    $path = @($Op[0]); $cur = $Doc
    for ($n = 0; $n -lt $path.Count - 1; $n++) { $cur = $cur.($path[$n]) }
    $last = $path[$path.Count - 1]
    if (@($Op).Count -gt 1) { $cur | Add-Member -NotePropertyName $last -NotePropertyValue $Op[1] -Force }
    else { $cur.PSObject.Properties.Remove($last) }
}

$cases = Get-Content (Join-Path $here 'cases.json') -Raw -Encoding UTF8 | ConvertFrom-Json
$outLines = @()
foreach ($case in $cases) {
    $results = @()
    foreach ($part in @($case.parts)) {
        $doc = Get-Content (Join-Path $examples $part.base) -Raw -Encoding UTF8 | ConvertFrom-Json
        foreach ($op in @($part.ops)) { if ($null -ne $op) { Edit-Doc $doc $op } }
        $r = [ordered]@{}
        try {
            if ($null -ne $part.manifest) { $r.text = New-Kickstart -Job $doc -Label $part.label -ManifestLines @($part.manifest) }
            else { $r.text = New-Kickstart -Job $doc -Label $part.label }
        } catch { $r.refused = $_.Exception.Message }
        $results += , $r
    }
    $outLines += ((ConvertTo-Json $case.name -Compress) + ': ' + (ConvertTo-Json @($results) -Depth 6 -Compress))
}
$outText = "{`n" + ($outLines -join ",`n") + "`n}`n"
[IO.File]::WriteAllText($Out, $outText, (New-Object Text.UTF8Encoding $false))
Write-Host "  golden: $($outLines.Count) cases written to $Out"
