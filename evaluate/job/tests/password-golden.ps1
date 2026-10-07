<#
.SYNOPSIS
    Writes password-golden.json: what Read-Password.ps1's pure functions
    return for every case in password-cases.json. The Rust port
    (evaluate/job/src/password.rs) must return the same (RISKS R32;
    VALIDATION V13).

.DESCRIPTION
    Loads the hasher's own functions from ..\..\windows\Read-Password.ps1
    (the file as it is, cut before its main section). The two shape cases
    (a fresh salt, a fresh hash) are random by nature: the script's own
    test is replayed and its true/false recorded. Records only.
#>
[CmdletBinding()]
param([string]$Out)
$here = Split-Path -Parent $MyInvocation.MyCommand.Path
if (-not $Out) { $Out = Join-Path $here 'password-golden.json' }
$src = [IO.File]::ReadAllText((Join-Path $here '..\..\windows\Read-Password.ps1'))
$cut = $src.IndexOf('if ($SelfTest) { Invoke-SelfTest; return }')
if ($cut -lt 0) { throw 'golden: the hasher has no main marker to cut at' }
. ([scriptblock]::Create($src.Substring(0, $cut)))

$cases = Get-Content (Join-Path $here 'password-cases.json') -Raw -Encoding UTF8 | ConvertFrom-Json
$outLines = @()
foreach ($case in $cases) {
    $a = @{}
    foreach ($p in @($case.args.PSObject.Properties)) { $a[$p.Name] = $p.Value }
    $got = $null
    try {
        switch ($case.fn) {
            'salt-shape' { $s = New-CryptSalt; $got = (($s.Length -eq 16) -and ($s -cmatch '^[./0-9A-Za-z]{16}$')) }
            'hash-shape' { $got = ((ConvertTo-Sha512Crypt -Password 'pässwörd' -Salt (New-CryptSalt)) -cmatch '^\$6\$[./0-9A-Za-z]{16}\$[./0-9A-Za-z]{86}$') }
            default { $got = & $case.fn @a }
        }
    } catch { $got = [ordered]@{ threw = "$($_.Exception.Message)" } }
    # ConvertTo-Json $null prints nothing at all; a no-refusal is written as null
    $json = if ($null -eq $got) { 'null' } else { ConvertTo-Json $got -Depth 4 -Compress }
    $outLines += ((ConvertTo-Json $case.name -Compress) + ': ' + $json)
}
$outText = "{`n" + ($outLines -join ",`n") + "`n}`n"
[IO.File]::WriteAllText($Out, $outText, (New-Object Text.UTF8Encoding $false))
Write-Host "  golden: $($outLines.Count) cases written to $Out"
