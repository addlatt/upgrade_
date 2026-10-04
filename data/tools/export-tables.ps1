<#
.SYNOPSIS
    Writes data\tables.json from data\*.ps1, for the Rust side to read.

.DESCRIPTION
    data\*.ps1 stays the contribution surface (CLAUDE.md): a device is still a
    one-line change to a table there. The Rust scanner (evaluate/scan) cannot
    read PowerShell, so this tool writes the same tables as JSON, and
    port-check.sh fails when tables.json is older than the .ps1 files say.
    Never edit tables.json by hand. Run: ./port-check.sh --export
#>
[CmdletBinding()]
param([string]$Out)
$here = Split-Path -Parent $MyInvocation.MyCommand.Path
$dataDir = Split-Path -Parent $here
if (-not $Out) { $Out = Join-Path $dataDir 'tables.json' }
. (Join-Path $dataDir 'devices.ps1')
. (Join-Path $dataDir 'distros.ps1')
. (Join-Path $dataDir 'releases.ps1')

function Norm {
    param($v)
    if ($null -eq $v) { return $null }
    if ($v -is [Collections.IDictionary]) {
        $o = [ordered]@{}
        foreach ($k in @($v.Keys | Sort-Object { [string]$_ })) { $o["$k"] = Norm $v[$k] }
        return $o
    }
    if ($v -is [array]) { return , @($v | ForEach-Object { Norm $_ }) }
    $v
}
$tables = [ordered]@{
    Wifi                = Norm (Get-UpgWifiDatabase)
    WifiVendorFallback  = Norm (Get-UpgWifiVendorFallback)
    Gpu                 = Norm (Get-UpgGpuDatabase)
    GpuVendorRules      = Norm (Get-UpgGpuVendorRules)
    AudioQuirks         = Norm @(Get-UpgAudioQuirks)
    VmdDeviceIds        = Norm @(Get-UpgVmdDeviceIds)
    VendorQuirks        = Norm @(Get-UpgVendorQuirks)
    AppRisk             = Norm @(Get-UpgAppRiskDatabase)
    DistroTableVerified = $script:UpgDistroTableVerified
    Distros             = Norm @(Get-UpgDistroTable)
    Releases            = Norm @(Get-UpgReleaseTable)
}
$text = (ConvertTo-Json $tables -Depth 12) -replace "`r`n", "`n"
[IO.File]::WriteAllText($Out, $text + "`n", (New-Object Text.UTF8Encoding $false))
Write-Host "  tables: written to $Out"
