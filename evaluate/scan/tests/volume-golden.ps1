<#
.SYNOPSIS
    Records tests/volume-golden.json: what DIAG-VOLUME.cmd's inline
    PowerShell wrote for the made-up facts in tests/volume-cases.json.

.DESCRIPTION
    The launcher carried its diagnostic as one -Command string. This
    recorder lifts that string out of the .cmd (the quoted segments between
    the powershell line and the WriteAllLines line, un-escaped), points its
    output at a temporary file, and runs it with every cmdlet and tool it
    calls shadowed by a function answering from the case (a function wins
    over a cmdlet or a program when a command is resolved): Get-Date, fsutil,
    chkntfs, Get-ItemProperty, Get-ChildItem, Get-Volume, Repair-Volume,
    Get-WinEvent, Get-PhysicalDisk, Test-Path, Get-CimInstance. Format-List
    and Format-Table run for real on the mocked objects, so the golden
    carries PowerShell's own layout of those two blocks. Nothing on this
    computer is read.

    Ran once, 2026-10-14, from the commit whose DIAG-VOLUME.cmd still held
    the block (the parent of the one that replaced it with upgrade-scan diag
    volume). It stays as the record of how volume-golden.json was made.

    Run from WSL:
      powershell.exe -NoProfile -ExecutionPolicy Bypass -File tests/volume-golden.ps1 `
        -Cases <win path>\tests\volume-cases.json -Launcher <win path>\evaluate\windows\usb-kit\DIAG-VOLUME.cmd -Out <win path>\tests\volume-golden.json
#>
param([string]$Cases, [string]$Launcher, [string]$Out)
$ErrorActionPreference = 'Continue'
$caseList = @((Get-Content -Raw -LiteralPath $Cases | ConvertFrom-Json) | ForEach-Object { $_ })

# ---- the block, lifted from the launcher
$cmdLines = (Get-Content -Raw -LiteralPath $Launcher) -split "`r?`n"
$start = [array]::FindIndex($cmdLines, [Predicate[string]]{ param($l) $l -like 'powershell -NoProfile -ExecutionPolicy Bypass -Command ^*' })
if ($start -lt 0) { throw 'the launcher does not look like DIAG-VOLUME.cmd' }
$segments = @()
for ($i = $start + 1; $i -lt $cmdLines.Count; $i++) {
    $l = $cmdLines[$i].Trim()
    if (-not $l.StartsWith('"')) { break }
    $last = -not $l.EndsWith('^')
    $l = $l.TrimEnd('^').Trim()
    $l = $l.Substring(1, $l.Length - 2).Replace('\"', '"')
    $segments += $l
    if ($last) { break }
}
$script:Block = ($segments -join "`n").Replace("'%OUT%'", '$script:OutFile')
if ($script:Block -notmatch 'WriteAllLines') { throw 'the block has no WriteAllLines' }

function ParseLocal([string]$t) { [datetime]::ParseExact($t, "yyyy-MM-dd'T'HH:mm:ss", [cultureinfo]::InvariantCulture) }
function ParseO([string]$t) { [datetime]::ParseExact($t, 'o', [cultureinfo]::InvariantCulture, [Globalization.DateTimeStyles]::RoundtripKind) }

# ---- the shadows
function Get-Date { (ParseO $script:F.Now).ToLocalTime() }
function fsutil {
    $a = @($args | ForEach-Object { "$_" })
    if ($a[0] -eq 'dirty') { @($script:F.FsutilDirty) | ForEach-Object { $_ } }
    elseif ($a[0] -eq 'fsinfo') { @($script:F.NtfsInfo) | ForEach-Object { $_ } }
}
function chkntfs { @($script:F.Chkntfs) | ForEach-Object { $_ } }
function Get-ItemProperty {
    [CmdletBinding()] param([Parameter(Position = 0)]$Path, $Name)
    if ($Name -eq 'BootExecute') { if ($null -eq $script:F.BootExecute) { return $null }; return [pscustomobject]@{ BootExecute = [string[]]@($script:F.BootExecute) } }
    if ($Name -eq 'HiberbootEnabled') { if ($null -eq $script:F.HiberbootEnabled) { return $null }; return [pscustomobject]@{ HiberbootEnabled = [int]$script:F.HiberbootEnabled } }
    $null
}
function Get-ChildItem {
    [CmdletBinding()] param([Parameter(Position = 0)]$Path, [switch]$Force, [switch]$Directory, [switch]$Recurse, [switch]$File)
    if ($Directory) { return @($script:F.Found) | ForEach-Object { [pscustomobject]@{ Name = (Split-Path $_.FullName -Leaf); FullName = $_.FullName; LastWriteTime = (ParseLocal $_.LastWriteTime) } } }
    $d = @($script:F.Found) | Where-Object { $_.FullName -eq $Path } | Select-Object -First 1
    if ($d -and $d.Files -gt 0) { 1..([int]$d.Files) | ForEach-Object { [pscustomobject]@{ Name = "file$_" } } }
}
function Get-Volume { [CmdletBinding()] param($DriveLetter) $v = $script:F.Volume; if ($null -eq $v) { return $null }
    [pscustomobject]@{ DriveLetter = [char]$v.DriveLetter; FileSystem = $v.FileSystem; HealthStatus = $v.HealthStatus; OperationalStatus = $v.OperationalStatus; Size = [uint64]$v.Size; SizeRemaining = [uint64]$v.SizeRemaining } }
function Repair-Volume { [CmdletBinding()] param($DriveLetter, [switch]$Scan) if ($null -ne $script:F.RepairScan) { "$($script:F.RepairScan)" } }
function Get-WinEvent {
    [CmdletBinding()] param($FilterHashtable)
    $h = $FilterHashtable
    $list = if ($h.LogName -eq 'Application' -and $h.Contains('Id')) { $script:F.Wininit }
            elseif ($h.LogName -eq 'Application') { $script:F.Chkdsk }
            elseif ($h.Contains('ProviderName')) { $script:F.Ntfs }
            else { $script:F.DiskEvents }
    @($list) | ForEach-Object { [pscustomobject]@{ TimeCreated = (ParseLocal $_.TimeLocal); ProviderName = $_.Provider; Id = [int]$_.Id; Message = "$($_.Message)" } }
}
function Get-PhysicalDisk { @($script:F.PhysicalDisks) | ForEach-Object { [pscustomobject]@{ DeviceId = "$($_.DeviceId)"; FriendlyName = $_.FriendlyName; MediaType = $_.MediaType; HealthStatus = $_.HealthStatus; OperationalStatus = $_.OperationalStatus; Size = [uint64]$_.Size } } }
function Test-Path { [CmdletBinding()] param([Parameter(Position = 0)]$Path) [bool]$script:F.Hiberfil }
function Get-CimInstance { [CmdletBinding()] param([Parameter(Position = 0)]$ClassName) [pscustomobject]@{ LastBootUpTime = (ParseLocal $script:F.LastBootUpTime) } }

$tmp = Join-Path $env:TEMP ('upgrade-volume-golden-' + [guid]::NewGuid().ToString('n'))
New-Item -ItemType Directory -Path $tmp -Force | Out-Null
$golden = [ordered]@{}
$i = 0
foreach ($case in $caseList) {
    $i++
    $script:F = $case.facts
    $script:OutFile = Join-Path $tmp "case-$i.txt"
    $old = $env:COMPUTERNAME; $env:COMPUTERNAME = $script:F.Computer
    Invoke-Expression $script:Block
    $env:COMPUTERNAME = $old
    $text = [IO.File]::ReadAllText($script:OutFile, [Text.Encoding]::UTF8)
    $lines = $text -split "`r`n"
    if ($lines.Count -gt 0 -and $lines[-1] -eq '') { $lines = $lines[0..($lines.Count - 2)] }
    $golden[$case.name] = @($lines)
    Microsoft.PowerShell.Utility\Write-Host ("  {0,3} lines  {1}" -f $lines.Count, $case.name)
}
Remove-Item -Recurse -Force $tmp -ErrorAction SilentlyContinue
$json = $golden | ConvertTo-Json -Depth 4
[IO.File]::WriteAllText($Out, $json + "`n", (New-Object Text.UTF8Encoding($false)))
Microsoft.PowerShell.Utility\Write-Host "  written: $Out ($($caseList.Count) cases)"
