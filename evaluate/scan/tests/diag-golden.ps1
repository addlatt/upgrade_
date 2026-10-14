<#
.SYNOPSIS
    Records tests/diag-golden.json: what Diag-SecureBoot.ps1 and
    Diag-Smart.ps1 answer for the made-up facts in tests/diag-cases.json.

.DESCRIPTION
    The scripts have no seams (they read the machine as they run), so the
    seams are put in from outside: every cmdlet they call is shadowed by a
    function of the same name that answers from the case's facts (a function
    wins over a cmdlet when a command is resolved), and the P/Invoke helper
    the Secure Boot script compiles (UpgFw) is compiled here first with the
    same name and canned answers, so the script's own Add-Type fails with
    "already exists" (a non-terminating error) and the script carries on
    with this one. The script is copied into a temporary folder that holds
    the case's stick files, runs with -OutFile, and the file it writes is
    read back as the golden lines. Nothing on this computer is read.

    Ran once, 2026-10-14, from the commit that still carried the scripts
    (the parent of the one that deleted them). It cannot run again without
    them; it stays as the record of how diag-golden.json was made.

    Run from WSL:
      powershell.exe -NoProfile -ExecutionPolicy Bypass -File tests/diag-golden.ps1 `
        -Cases <win path>\tests\diag-cases.json -Scripts <win path>\evaluate\windows\usb-kit -Out <win path>\tests\diag-golden.json
#>
param([string]$Cases, [string]$Scripts, [string]$Out)
$ErrorActionPreference = 'Continue'
# PS 5.1 hands a JSON array over as one object; the pipe unrolls it
$caseList = @((Get-Content -Raw -LiteralPath $Cases | ConvertFrom-Json) | ForEach-Object { $_ })

Add-Type -TypeDefinition @'
using System;
using System.Collections.Generic;
public static class UpgFw {
    public static string Privilege = "ok";
    public static Dictionary<string, byte[]> Vars = new Dictionary<string, byte[]>();
    public static Dictionary<string, string> Status = new Dictionary<string, string>();
    public static string Enable() { return Privilege; }
    public static byte[] Read(string name, string guid, out string err) {
        err = Status.ContainsKey(name) ? Status[name] : "mock: no such variable";
        return Vars.ContainsKey(name) ? Vars[name] : null;
    }
}
'@

function Unhex([string]$h) {
    if (-not $h) { return $null }
    $b = New-Object byte[] ($h.Length / 2)
    for ($i = 0; $i -lt $b.Length; $i++) { $b[$i] = [Convert]::ToByte($h.Substring(2 * $i, 2), 16) }
    return ,$b
}
function ParseO([string]$t) { [datetime]::ParseExact($t, 'o', [cultureinfo]::InvariantCulture, [Globalization.DateTimeStyles]::RoundtripKind) }
function ParseLocal([string]$t) { [datetime]::ParseExact($t, "yyyy-MM-dd'T'HH:mm:ss.fffffff", [cultureinfo]::InvariantCulture) }

function Invoke-DiagCase($case, $script, $outFile) {
    $f = $case.facts
    $oldName = $env:COMPUTERNAME
    $env:COMPUTERNAME = $f.Computer
    function Get-Date { ParseO $f.Now }
    if ($case.diag -eq 'secureboot') {
        [UpgFw]::Privilege = $f.Privilege
        [UpgFw]::Vars.Clear(); [UpgFw]::Status.Clear()
        foreach ($v in $f.Variables) { [UpgFw]::Status[$v.Name] = $v.Status; if ($v.Bytes) { [UpgFw]::Vars[$v.Name] = Unhex $v.Bytes } }
        function Get-SecureBootUEFI { [CmdletBinding()] param($Name)
            $d = $f.Databases | Where-Object { $_.Name -eq $Name }
            if ($d.Bytes) { [pscustomobject]@{ Name = $Name; Bytes = (Unhex $d.Bytes) } } else { throw $d.Error } }
        function Confirm-SecureBootUEFI { [CmdletBinding()] param()
            if ($f.ConfirmSecureBoot -eq 'True') { $true } elseif ($f.ConfirmSecureBoot -eq 'False') { $false } else { throw $f.ConfirmSecureBoot } }
        function New-RegKey($entry) {
            $k = [pscustomobject]@{ Name = ($entry.Key -replace '^HKLM', 'HKEY_LOCAL_MACHINE'); Vals = @($entry.Values) }
            $k | Add-Member -MemberType ScriptMethod -Name GetValueNames -Value { @($this.Vals | ForEach-Object { $_.Name }) }
            $k | Add-Member -MemberType ScriptMethod -Name GetValue -Value { param($n) ($this.Vals | Where-Object { $_.Name -ceq $n } | Select-Object -First 1).Data }
            $k }
        function Get-Item { [CmdletBinding()] param($Path) if (@($f.Registry).Count -gt 0) { New-RegKey $f.Registry[0] } }
        function Get-ChildItem { [CmdletBinding()] param($Path, [switch]$Recurse) @($f.Registry) | Select-Object -Skip 1 | ForEach-Object { New-RegKey $_ } }
        function Get-WinEvent { [CmdletBinding()] param($FilterHashtable)
            @($f.Events) | Where-Object { $_.Provider -eq $FilterHashtable.ProviderName } | ForEach-Object {
                [pscustomobject]@{ TimeCreated = (ParseO $_.TimeUtc); ProviderName = $_.Provider; Id = $_.Id; Message = $_.Message } } }
        foreach ($file in $f.Files) {
            if ($file.Bytes) {
                $p = Join-Path (Split-Path $script -Parent) $file.Name
                New-Item -ItemType Directory -Path (Split-Path $p -Parent) -Force | Out-Null
                [IO.File]::WriteAllBytes($p, (Unhex $file.Bytes))
            }
        }
    } else {
        function Get-PhysicalDisk { [CmdletBinding()] param()
            @($f.PhysicalDisks) | ForEach-Object { [pscustomobject]@{ DeviceId = $_.DeviceId; FriendlyName = $_.FriendlyName; BusType = $_.BusType; MediaType = $_.MediaType; HealthStatus = $_.HealthStatus; OperationalStatus = [string[]]$_.OperationalStatus; Size = $_.Size; FirmwareVersion = $_.FirmwareVersion; SerialNumber = $_.SerialNumber; Reliability = $_.Reliability; ReliabilityError = $_.ReliabilityError } } }
        function Get-StorageReliabilityCounter { [CmdletBinding()] param([Parameter(ValueFromPipeline = $true)]$InputObject)
            process { if ($InputObject.ReliabilityError) { throw $InputObject.ReliabilityError }; if ($null -ne $InputObject.Reliability) { $InputObject.Reliability } } }
        function Get-CimInstance { [CmdletBinding()] param([Parameter(Position = 0)]$ClassName, $Namespace)
            switch ($ClassName) {
                'Win32_DiskDrive' { @($f.DiskDrives) | ForEach-Object { [pscustomobject]@{ Index = $_.Index; Model = $_.Model; InterfaceType = $_.InterfaceType; Status = $_.Status; PNPDeviceID = $_.PNPDeviceID } } }
                'MSStorageDriver_FailurePredictStatus' { $r = $f.FailurePredictStatus; if ($r.Error) { throw $r.Error }; @($r.Items) | ForEach-Object { [pscustomobject]@{ InstanceName = $_.InstanceName; PredictFailure = $_.PredictFailure; Reason = $_.Reason } } }
                'MSStorageDriver_FailurePredictData' { $r = $f.FailurePredictData; if ($r.Error) { throw $r.Error }; @($r.Items) | ForEach-Object { [pscustomobject]@{ InstanceName = $_.InstanceName; VendorSpecific = (Unhex $_.VendorSpecific) } } }
                'MSStorageDriver_FailurePredictThresholds' { $r = $f.FailurePredictThresholds; if ($r.Error) { throw $r.Error }; @($r.Items) | ForEach-Object { [pscustomobject]@{ InstanceName = $_.InstanceName; VendorSpecific = (Unhex $_.VendorSpecific) } } }
                default { throw "mock: no class $ClassName" }
            } }
        function Get-WinEvent { [CmdletBinding()] param($FilterHashtable)
            $list = if ($FilterHashtable.ProviderName -eq 'disk') { $f.DiskEvents } else { $f.StorageEvents }
            @($list) | ForEach-Object { [pscustomobject]@{ TimeCreated = (ParseLocal $_.TimeLocal); ProviderName = $_.Provider; Id = $_.Id; Message = $_.Message } } }
        function Get-Volume { [CmdletBinding()] param()
            @($f.Volumes) | ForEach-Object { [pscustomobject]@{ DriveLetter = [char]$_.DriveLetter; FileSystemLabel = $_.FileSystemLabel; FileSystem = $_.FileSystem; HealthStatus = $_.HealthStatus; OperationalStatus = $_.OperationalStatus; Size = $_.Size; SizeRemaining = $_.SizeRemaining } } }
    }
    $errors = & $script -OutFile $outFile 6>$null 2>&1 | Where-Object { $_ -is [Management.Automation.ErrorRecord] -and $_.FullyQualifiedErrorId -notlike 'TYPE_ALREADY_EXISTS*' }
    foreach ($e in $errors) { Write-Warning ("[" + $case.name + "] " + $e) }
    $env:COMPUTERNAME = $oldName
    $text = [IO.File]::ReadAllText($outFile, [Text.Encoding]::UTF8)
    $text = $text.TrimStart([char]0xFEFF)
    $lines = $text -split "`r`n"
    if ($lines.Count -gt 0 -and $lines[-1] -eq '') { $lines = $lines[0..($lines.Count - 2)] }
    return ,@($lines)
}

$tmp = Join-Path $env:TEMP ('upgrade-diag-golden-' + [guid]::NewGuid().ToString('n'))
$golden = [ordered]@{}
$i = 0
foreach ($case in $caseList) {
    $i++
    $dir = Join-Path $tmp ("case-$i")
    New-Item -ItemType Directory -Path $dir -Force | Out-Null
    $name = if ($case.diag -eq 'secureboot') { 'Diag-SecureBoot.ps1' } else { 'Diag-Smart.ps1' }
    $script = Join-Path $dir $name
    Copy-Item (Join-Path $Scripts $name) $script
    $lines = Invoke-DiagCase $case $script (Join-Path $dir 'out.txt')
    $golden[$case.name] = $lines
    Write-Host ("  {0,3} lines  {1}" -f $lines.Count, $case.name)
}
Remove-Item -Recurse -Force $tmp -ErrorAction SilentlyContinue
$json = $golden | ConvertTo-Json -Depth 4
[IO.File]::WriteAllText($Out, $json + "`n", (New-Object Text.UTF8Encoding($false)))
Write-Host "  written: $Out ($($caseList.Count) cases)"
