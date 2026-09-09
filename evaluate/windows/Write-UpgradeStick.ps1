<#
.SYNOPSIS
    upgrade_ - the stick writer. The first component that writes to a
    device, and the one RISKS R16 is about.

.DESCRIPTION
    Writes the upgrade_ kit onto ONE USB stick: a FAT32 boot partition
    carrying the kit layout (what make-kit.sh assembles), and an exFAT
    staging partition in the rest of the space. Raw writes to a disk are the
    one place this project could destroy data BEFORE the commit line, so the
    device selection refuses by default and the write path re-resolves the
    target by its unique id immediately before the first destructive call.

    A disk is written only if ALL of these hold - each is reported, and any
    one refuses:
      - it is on the USB bus, and its media type is not HDD or SSD (a USB
        hard drive or SSD enclosure is somebody's backup, not a stick);
      - it is not the system or boot disk, and holds no volume with a
        drive letter that Windows is running from;
      - it is exactly the device pointed at: -Target is the disk's
        UniqueId, and exactly one attached disk carries it (two sticks
        with the same id - cloned serials exist - is ambiguity, refused);
      - its size matches what the person was shown (-ExpectedSizeBytes,
        or -ExpectedSizeGB within 10 %, decimal gigabytes as printed on
        the packaging);
      - it is online and writable;
      - the person confirms it by typing its current volume label (or
        its model name when it has no label) - the one device prompt in
        the whole conversion's budget. -ConfirmedLabel passes that answer
        in for a caller that already asked.

    -Plan is read-only: it enumerates every attached disk, prints the
    decision each one gets, writes nothing, and appends an evidence row.
    Every write also appends a row (docs/validation-results/
    r16-stick-writer.csv): the matrix R16 closes on is "several sticks and
    a USB hard drive attached at once, the right one written, the rest
    refused" - and those rows are written by this script, never by hand.

.PARAMETER List
    Print every attached disk with the identity the other parameters need.

.PARAMETER Plan
    Decide, print, record - write nothing.

.PARAMETER Target
    The UniqueId of the disk to write (from -List). Never a disk number:
    numbers change between boots and plug events.

.PARAMETER ExpectedSizeBytes / ExpectedSizeGB
    The size the person was shown. One of the two is required.

.PARAMETER Source
    Directory whose contents go onto the boot partition (dist/kit/stick
    from make-kit.sh). Must contain SHA256SUMS; every file is re-verified
    on the stick after the copy.

.PARAMETER Label
    Volume label for the boot partition (default UPGRADE; exFAT staging
    partition gets UPGDATA).

.PARAMETER BootSizeGB
    Size of the FAT32 boot partition (default 4; the live image will need
    it). The staging partition takes the rest.

.PARAMETER ConfirmedLabel
    The confirmation answer, for non-interactive callers.

.PARAMETER SelfTest
    Logic-only tests of the selection rules against fabricated devices -
    the matrix no single desk has plugged in.
#>
[CmdletBinding(DefaultParameterSetName = 'List')]
param(
    [Parameter(ParameterSetName = 'List')][switch]$List,
    [Parameter(ParameterSetName = 'Plan', Mandatory = $true)][switch]$Plan,
    [Parameter(ParameterSetName = 'Write', Mandatory = $true)][switch]$Write,
    [Parameter(ParameterSetName = 'Plan', Mandatory = $true)]
    [Parameter(ParameterSetName = 'Write', Mandatory = $true)]
    [string]$Target,
    [Parameter(ParameterSetName = 'Plan')][Parameter(ParameterSetName = 'Write')][long]$ExpectedSizeBytes,
    [Parameter(ParameterSetName = 'Plan')][Parameter(ParameterSetName = 'Write')][double]$ExpectedSizeGB,
    [Parameter(ParameterSetName = 'Write', Mandatory = $true)][string]$Source,
    [string]$Label = 'UPGRADE',
    [int]$BootSizeGB = 4,
    [string]$ConfirmedLabel,
    [string]$ResultsCsv,
    [Parameter(ParameterSetName = 'SelfTest', Mandatory = $true)][switch]$SelfTest
)

$ErrorActionPreference = 'Stop'
$WriterVersion = '0.1.0'
$CsvHeader = 'timestamp,writer,machine,os_build,mode,disks_attached,usb_disks,target,expected_bytes,decision,refusals,written,verified,notes'

function New-Line { param([string]$s = '', [string]$c = 'Gray') Write-Host $s -ForegroundColor $c }

function Test-Elevated {
    $id = [Security.Principal.WindowsIdentity]::GetCurrent()
    (New-Object Security.Principal.WindowsPrincipal($id)).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
}

# =============================================================================
#  collection - every attached disk, with what the rules need
# =============================================================================

function Get-UpgAttachedDisks {
    $physical = @{}
    try { foreach ($p in Get-PhysicalDisk -ErrorAction Stop) { $physical["$($p.DeviceId)"] = $p } } catch { }
    $sysLetters = @('C')
    try { $sysLetters += @((Get-CimInstance Win32_PageFileUsage -ErrorAction Stop).Name | ForEach-Object { $_.Substring(0, 1) }) } catch { }
    try { $sysLetters += $env:SystemDrive.Substring(0, 1) } catch { }
    $out = @()
    foreach ($d in Get-Disk) {
        $vols = @()
        try {
            foreach ($p in (Get-Partition -DiskNumber $d.Number -ErrorAction SilentlyContinue)) {
                $v = $null
                if ($p.DriveLetter) { $v = Get-Volume -DriveLetter $p.DriveLetter -ErrorAction SilentlyContinue }
                $vols += [pscustomobject]@{
                    DriveLetter = if ($p.DriveLetter) { "$($p.DriveLetter)" } else { $null }
                    Label       = if ($v) { $v.FileSystemLabel } else { $null }
                    FileSystem  = if ($v) { $v.FileSystemType } else { $null }
                    SizeBytes   = $p.Size
                }
            }
        } catch { }
        $pd = $physical["$($d.Number)"]
        $out += [pscustomobject]@{
            Number         = $d.Number
            FriendlyName   = $d.FriendlyName
            UniqueId       = $d.UniqueId
            SerialNumber   = ("$($d.SerialNumber)" -replace '\s', '')
            BusType        = "$($d.BusType)"
            MediaType      = if ($pd) { "$($pd.MediaType)" } else { 'unknown' }
            SizeBytes      = [long]$d.Size
            PartitionStyle = "$($d.PartitionStyle)"
            IsSystem       = [bool]$d.IsSystem
            IsBoot         = [bool]$d.IsBoot
            IsOffline      = [bool]$d.IsOffline
            IsReadOnly     = [bool]$d.IsReadOnly
            Volumes        = $vols
            HoldsSystemLetter = [bool](@($vols | Where-Object { $_.DriveLetter -and ($sysLetters -contains $_.DriveLetter.ToUpper()) }).Count -gt 0)
        }
    }
    $out
}

# =============================================================================
#  judgment - pure, self-tested
# =============================================================================

function Test-UpgStickSize {
    # Pure: does a disk's size match what the person was shown? Exact bytes
    # when the caller has them (job.json does); +/-10 % of decimal GB when a
    # human typed the number on the packaging.
    param([long]$SizeBytes, [long]$ExpectedBytes, [double]$ExpectedGB)
    if ($ExpectedBytes -gt 0) { return ($SizeBytes -eq $ExpectedBytes) }
    if ($ExpectedGB -gt 0) {
        $exp = $ExpectedGB * 1e9
        return ([math]::Abs($SizeBytes - $exp) -le ($exp * 0.10))
    }
    $false
}

function Get-UpgDiskRefusals {
    # Pure: every reason ONE disk must not be written, given the device the
    # caller points at. Empty list = writable candidate. The identity test
    # is here too, so a disk that is not the target is refused for that
    # reason (and any others it has) rather than silently skipped.
    param($Disk, [string]$Target, [long]$ExpectedBytes, [double]$ExpectedGB)
    $r = @()
    if ($Disk.UniqueId -ne $Target)                        { $r += 'not the device pointed at' }
    if ($Disk.BusType -ne 'USB')                            { $r += "bus is $($Disk.BusType), not USB" }
    if ($Disk.MediaType -in @('HDD', 'SSD'))                { $r += "media type $($Disk.MediaType) - a drive, not a stick" }
    if ($Disk.IsSystem)                                     { $r += 'is the system disk' }
    if ($Disk.IsBoot)                                       { $r += 'is the boot disk' }
    if ($Disk.HoldsSystemLetter)                            { $r += 'holds a volume Windows is running from' }
    if ($Disk.IsOffline)                                    { $r += 'is offline' }
    if ($Disk.IsReadOnly)                                   { $r += 'is read-only' }
    if (-not (Test-UpgStickSize -SizeBytes $Disk.SizeBytes -ExpectedBytes $ExpectedBytes -ExpectedGB $ExpectedGB)) {
        $shown = if ($ExpectedBytes -gt 0) { "$ExpectedBytes bytes" } elseif ($ExpectedGB -gt 0) { "$ExpectedGB GB" } else { 'no expected size given' }
        $r += "size $($Disk.SizeBytes) bytes does not match $shown"
    }
    , $r
}

function Resolve-UpgStickTarget {
    # Pure: the whole decision for a set of attached disks. Returns the
    # decision, the chosen disk (or null), and the per-disk reasons.
    param($Disks, [string]$Target, [long]$ExpectedBytes, [double]$ExpectedGB)
    $rows = @()
    foreach ($d in @($Disks)) {
        $why = Get-UpgDiskRefusals -Disk $d -Target $Target -ExpectedBytes $ExpectedBytes -ExpectedGB $ExpectedGB
        $rows += [pscustomobject]@{ Disk = $d; Refusals = @($why) }
    }
    $matches = @($rows | Where-Object { $_.Disk.UniqueId -eq $Target })
    $candidates = @($rows | Where-Object { $_.Refusals.Count -eq 0 })
    $decision = 'refused'; $reason = ''; $chosen = $null
    if ([string]::IsNullOrWhiteSpace($Target)) {
        $reason = 'no target given'
    } elseif ($matches.Count -eq 0) {
        $reason = 'the device pointed at is not attached'
    } elseif ($matches.Count -gt 1) {
        $reason = "ambiguous: $($matches.Count) attached disks carry the id $Target"
    } elseif ($candidates.Count -eq 1 -and $candidates[0].Disk.UniqueId -eq $Target) {
        $decision = 'selected'; $chosen = $candidates[0].Disk
    } else {
        $reason = 'the device pointed at is refused: ' + ($matches[0].Refusals -join '; ')
    }
    [pscustomobject]@{ Decision = $decision; Reason = $reason; Chosen = $chosen; Rows = $rows }
}

function Get-UpgConfirmWord {
    # Pure: what the person must type to confirm a device - its current
    # label if it has one, else its model name. Never a bare "yes".
    param($Disk)
    $labels = @($Disk.Volumes | Where-Object { $_.Label } | ForEach-Object { $_.Label })
    if ($labels.Count -gt 0) { return "$($labels[0])" }
    "$($Disk.FriendlyName)".Trim()
}

function ConvertFrom-UpgSha256Sums {
    # Pure: sha256sum -c format ("<hex>  ./path") -> @{ path = hex }
    param([string[]]$Lines)
    $m = @{}
    foreach ($l in @($Lines)) {
        if ($l -match '^([0-9a-fA-F]{64})\s[\s*](.+)$') {
            $p = $matches[2].Trim() -replace '^\./', '' -replace '/', '\'
            $m[$p] = $matches[1].ToLower()
        }
    }
    $m
}

# =============================================================================
#  evidence
# =============================================================================

function Resolve-ResultsCsv {
    if ($ResultsCsv) { return $ResultsCsv }
    $repo = Join-Path $PSScriptRoot '..\..\docs\validation-results\r16-stick-writer.csv'
    try { $repo = [IO.Path]::GetFullPath($repo) } catch { }
    if (Test-Path (Split-Path $repo -Parent)) { return $repo }
    Join-Path $env:ProgramData 'upgrade_\r16\r16-stick-writer.csv'
}

function Write-EvidenceRow {
    param([hashtable]$Row)
    $csv = Resolve-ResultsCsv
    $dir = Split-Path $csv -Parent
    if (-not (Test-Path $dir)) { New-Item -ItemType Directory -Path $dir -Force | Out-Null }
    if (-not (Test-Path $csv)) { [IO.File]::WriteAllText($csv, $CsvHeader + "`r`n", (New-Object Text.UTF8Encoding($false))) }
    function Esc { param($v) '"' + (($v -as [string]) -replace '"', '""') + '"' }
    $line = @(
        Esc((Get-Date).ToUniversalTime().ToString('o')); Esc($WriterVersion)
        Esc($Row.Machine); Esc($Row.OsBuild); Esc($Row.Mode)
        Esc($Row.DisksAttached); Esc($Row.UsbDisks); Esc($Row.Target); Esc($Row.ExpectedBytes)
        Esc($Row.Decision); Esc($Row.Refusals); Esc($Row.Written); Esc($Row.Verified); Esc($Row.Notes)
    ) -join ','
    Add-Content -Path $csv -Value $line -Encoding UTF8
    $csv
}

function Format-DiskLine {
    param($Disk)
    $vols = @($Disk.Volumes | ForEach-Object { "$(if ($_.DriveLetter) { $_.DriveLetter + ':' } else { '-' })$(if ($_.Label) { ' ' + $_.Label } )$(if ($_.FileSystem) { ' ' + $_.FileSystem })" }) -join ', '
    ('  disk {0}  {1,-28} {2,7:N1} GB  {3,-5} {4,-11} {5}{6}' -f $Disk.Number, $Disk.FriendlyName, ($Disk.SizeBytes / 1e9), $Disk.BusType, $Disk.MediaType,
        $(if ($Disk.IsSystem -or $Disk.IsBoot) { '[SYSTEM] ' } else { '' }), $vols)
}

# =============================================================================
#  the write
# =============================================================================

function Invoke-UpgStickWrite {
    param($Disk, [string]$SourceDir, [string]$BootLabel, [int]$BootGB)
    $result = [pscustomobject]@{ Written = $false; Verified = $false; Files = 0; Failed = @(); BootLetter = $null; DataLetter = $null; Notes = @(); Stick = $null }

    # Re-resolve by unique id NOW, from a fresh enumeration, and hand the
    # object - never a number - to the destructive cmdlets. A plug event
    # between the plan and this line must not move the write.
    $fresh = @(Get-Disk | Where-Object { $_.UniqueId -eq $Disk.UniqueId })
    if ($fresh.Count -ne 1) { throw "re-resolve: $($fresh.Count) disks carry the id $($Disk.UniqueId) now; refusing" }
    $live = $fresh[0]
    if ("$($live.BusType)" -ne 'USB' -or $live.IsSystem -or $live.IsBoot) { throw 're-resolve: the disk no longer satisfies the rules; refusing' }
    if ([long]$live.Size -ne $Disk.SizeBytes) { throw 're-resolve: the disk size changed; refusing' }

    New-Line "  clearing disk $($live.Number) ($($live.FriendlyName))..." 'DarkGray'
    Clear-Disk -InputObject $live -RemoveData -RemoveOEM -Confirm:$false -ErrorAction Stop
    $live = Get-Disk -UniqueId $Disk.UniqueId -ErrorAction Stop
    Initialize-Disk -InputObject $live -PartitionStyle MBR -ErrorAction Stop
    $live = Get-Disk -UniqueId $Disk.UniqueId -ErrorAction Stop
    $bootBytes = [long]$BootGB * 1GB
    if ($bootBytes -ge [long]$live.Size - 64MB) { $bootBytes = [long]$live.Size - 64MB; $result.Notes += 'boot partition capped to the disk' }
    New-Line "  boot partition: FAT32 $BootLabel, $([math]::Round($bootBytes / 1GB, 1)) GiB..." 'DarkGray'
    $boot = New-Partition -InputObject $live -Size $bootBytes -IsActive -AssignDriveLetter -ErrorAction Stop
    $bootVol = Format-Volume -Partition $boot -FileSystem FAT32 -NewFileSystemLabel $BootLabel -Confirm:$false -ErrorAction Stop
    $result.BootLetter = "$($boot.DriveLetter)"
    if (-not $result.BootLetter -or $result.BootLetter -eq ' ') {
        $boot = Get-Partition -DiskNumber $live.Number -PartitionNumber $boot.PartitionNumber
        $result.BootLetter = "$($boot.DriveLetter)"
    }
    $live = Get-Disk -UniqueId $Disk.UniqueId -ErrorAction Stop
    $remaining = [long]$live.LargestFreeExtent
    if ($remaining -gt 64MB) {
        New-Line "  staging partition: exFAT UPGDATA, $([math]::Round($remaining / 1GB, 1)) GiB..." 'DarkGray'
        $data = New-Partition -InputObject $live -UseMaximumSize -AssignDriveLetter -ErrorAction Stop
        $null = Format-Volume -Partition $data -FileSystem exFAT -NewFileSystemLabel 'UPGDATA' -Confirm:$false -ErrorAction Stop
        $data = Get-Partition -DiskNumber $live.Number -PartitionNumber $data.PartitionNumber
        $result.DataLetter = "$($data.DriveLetter)"
    } else { $result.Notes += 'no room for a staging partition' }
    $result.Written = $true

    # copy, then verify by reading every file back against SHA256SUMS
    $root = "$($result.BootLetter):\"
    New-Line "  copying $SourceDir -> $root ..." 'DarkGray'
    Copy-Item -Path (Join-Path $SourceDir '*') -Destination $root -Recurse -Force -ErrorAction Stop
    $sums = ConvertFrom-UpgSha256Sums -Lines (Get-Content (Join-Path $SourceDir 'SHA256SUMS'))
    $sha = [Security.Cryptography.SHA256]::Create()
    try {
        foreach ($rel in $sums.Keys) {
            $p = Join-Path $root $rel
            $ok = $false
            if (Test-Path -LiteralPath $p) {
                $fs = [IO.File]::Open($p, 'Open', 'Read', 'Read')
                try { $ok = (([BitConverter]::ToString($sha.ComputeHash($fs)) -replace '-', '').ToLower() -eq $sums[$rel]) } finally { $fs.Dispose() }
            }
            if ($ok) { $result.Files++ } else { $result.Failed += $rel }
        }
    } finally { $sha.Dispose() }
    $result.Verified = ($result.Failed.Count -eq 0 -and $result.Files -eq $sums.Count -and $sums.Count -gt 0)

    $live = Get-Disk -UniqueId $Disk.UniqueId -ErrorAction Stop
    $result.Stick = [pscustomobject]@{
        unique_id = $live.UniqueId; serial_number = ("$($live.SerialNumber)" -replace '\s', ''); size_bytes = [long]$live.Size
        friendly_name = $live.FriendlyName; label = $BootLabel; manifest = 'SHA256SUMS'
    }
    $result
}

# =============================================================================
#  self-test - the matrix, fabricated
# =============================================================================

function Invoke-SelfTest {
    function D { param([hashtable]$h)
        $o = [pscustomobject]@{ Number = 0; FriendlyName = 'X'; UniqueId = 'ID'; SerialNumber = ''; BusType = 'USB'; MediaType = 'Unspecified'
            SizeBytes = 8053063680; PartitionStyle = 'MBR'; IsSystem = $false; IsBoot = $false; IsOffline = $false; IsReadOnly = $false
            Volumes = @(); HoldsSystemLetter = $false }
        foreach ($k in $h.Keys) { $o.$k = $h[$k] }
        $o
    }
    $sys   = D @{ Number = 0; FriendlyName = 'Samsung NVMe'; UniqueId = 'eui.SYS'; BusType = 'NVMe'; MediaType = 'SSD'; SizeBytes = 1024209543168; IsSystem = $true; IsBoot = $true; HoldsSystemLetter = $true; Volumes = @([pscustomobject]@{ DriveLetter = 'C'; Label = 'OS'; FileSystem = 'NTFS'; SizeBytes = 1 }) }
    $stick = D @{ Number = 1; FriendlyName = 'General UDisk'; UniqueId = 'USB-STICK-A'; Volumes = @([pscustomobject]@{ DriveLetter = 'D'; Label = 'UPGV0'; FileSystem = 'FAT32'; SizeBytes = 8036253696 }) }
    $stick2 = D @{ Number = 2; FriendlyName = 'SanDisk Ultra'; UniqueId = 'USB-STICK-B'; SizeBytes = 32010000000 }
    $clone  = D @{ Number = 3; FriendlyName = 'General UDisk'; UniqueId = 'USB-STICK-A'; Volumes = @() }
    $usbHdd = D @{ Number = 4; FriendlyName = 'WD Elements 2TB'; UniqueId = 'USB-HDD'; MediaType = 'HDD'; SizeBytes = 2000398934016 }
    $usbSsd = D @{ Number = 5; FriendlyName = 'Samsung T7'; UniqueId = 'USB-SSD'; MediaType = 'SSD'; SizeBytes = 1000204886016 }
    $sd     = D @{ Number = 6; FriendlyName = 'SD Card'; UniqueId = 'SD-1'; BusType = 'SD' }
    $sas    = D @{ Number = 7; FriendlyName = 'Msft Virtual Disk'; UniqueId = 'VHDX-1'; BusType = 'SAS'; SizeBytes = 8053063680 }
    $off    = D @{ Number = 8; FriendlyName = 'Kingston'; UniqueId = 'USB-OFF'; IsOffline = $true }
    $pf     = D @{ Number = 9; FriendlyName = 'Kingston'; UniqueId = 'USB-PF'; HoldsSystemLetter = $true; Volumes = @([pscustomobject]@{ DriveLetter = 'C'; Label = ''; FileSystem = 'NTFS'; SizeBytes = 1 }) }

    $cases = @(
        @{ Name = 'one stick beside the system disk, pointed at, right size: selected'
           Run = { (Resolve-UpgStickTarget -Disks @($sys, $stick) -Target 'USB-STICK-A' -ExpectedGB 8).Decision }; Expect = 'selected' }
        @{ Name = 'the system disk is never a candidate even when pointed at'
           Run = { $r = Resolve-UpgStickTarget -Disks @($sys, $stick) -Target 'eui.SYS' -ExpectedGB 1024; "$($r.Decision):$($r.Reason)" }; Expect = 'refused:the device pointed at is refused: bus is NVMe, not USB; media type SSD - a drive, not a stick; is the system disk; is the boot disk; holds a volume Windows is running from' }
        @{ Name = 'two sticks + a USB hard drive attached: the pointed-at stick is selected'
           Run = { $r = Resolve-UpgStickTarget -Disks @($sys, $stick, $stick2, $usbHdd) -Target 'USB-STICK-B' -ExpectedGB 32; "$($r.Decision):$($r.Chosen.Number)" }; Expect = 'selected:2' }
        @{ Name = 'the USB hard drive pointed at is refused for its media type'
           Run = { $r = Resolve-UpgStickTarget -Disks @($sys, $stick, $usbHdd) -Target 'USB-HDD' -ExpectedGB 2000; $r.Rows[2].Refusals -join ';' }; Expect = 'media type HDD - a drive, not a stick' }
        @{ Name = 'a USB SSD enclosure is refused the same way'
           Run = { (Resolve-UpgStickTarget -Disks @($sys, $usbSsd) -Target 'USB-SSD' -ExpectedGB 1000).Decision }; Expect = 'refused' }
        @{ Name = 'right stick, wrong expected size: refused'
           Run = { $r = Resolve-UpgStickTarget -Disks @($sys, $stick) -Target 'USB-STICK-A' -ExpectedGB 32; $r.Reason }; Expect = 'the device pointed at is refused: size 8053063680 bytes does not match 32 GB' }
        @{ Name = 'exact bytes from a job match exactly'
           Run = { (Resolve-UpgStickTarget -Disks @($stick) -Target 'USB-STICK-A' -ExpectedBytes 8053063680).Decision }; Expect = 'selected' }
        @{ Name = 'exact bytes off by one byte: refused'
           Run = { (Resolve-UpgStickTarget -Disks @($stick) -Target 'USB-STICK-A' -ExpectedBytes 8053063681).Decision }; Expect = 'refused' }
        @{ Name = 'no expected size at all: refused'
           Run = { (Resolve-UpgStickTarget -Disks @($stick) -Target 'USB-STICK-A').Decision }; Expect = 'refused' }
        @{ Name = 'two sticks with the same unique id (cloned serials): ambiguous, refused'
           Run = { $r = Resolve-UpgStickTarget -Disks @($sys, $stick, $clone) -Target 'USB-STICK-A' -ExpectedGB 8; $r.Reason }; Expect = 'ambiguous: 2 attached disks carry the id USB-STICK-A' }
        @{ Name = 'the target is not attached: refused, says so'
           Run = { (Resolve-UpgStickTarget -Disks @($sys, $stick) -Target 'USB-STICK-B' -ExpectedGB 32).Reason }; Expect = 'the device pointed at is not attached' }
        @{ Name = 'an SD card is not USB: refused'
           Run = { (Resolve-UpgStickTarget -Disks @($sd) -Target 'SD-1' -ExpectedGB 8).Decision }; Expect = 'refused' }
        @{ Name = 'a VHDX on the SAS bus (the rig stand-in) is refused for its bus'
           Run = { (Resolve-UpgStickTarget -Disks @($sas) -Target 'VHDX-1' -ExpectedGB 8).Reason }; Expect = 'the device pointed at is refused: bus is SAS, not USB' }
        @{ Name = 'an offline stick is refused'
           Run = { (Resolve-UpgStickTarget -Disks @($off) -Target 'USB-OFF' -ExpectedGB 8).Decision }; Expect = 'refused' }
        @{ Name = 'a USB disk holding the letter Windows runs from is refused (Windows To Go)'
           Run = { (Resolve-UpgStickTarget -Disks @($pf) -Target 'USB-PF' -ExpectedGB 8).Reason }; Expect = 'the device pointed at is refused: holds a volume Windows is running from' }
        @{ Name = 'empty target: refused'
           Run = { (Resolve-UpgStickTarget -Disks @($stick) -Target '' -ExpectedGB 8).Reason }; Expect = 'no target given' }
        @{ Name = 'every non-target disk gets "not the device pointed at" among its reasons'
           Run = { $r = Resolve-UpgStickTarget -Disks @($sys, $stick, $stick2) -Target 'USB-STICK-B' -ExpectedGB 32; ($r.Rows[1].Refusals -contains 'not the device pointed at') }; Expect = $true }
        @{ Name = 'size: 8 GB packaging matches a 8053063680-byte stick'
           Run = { Test-UpgStickSize -SizeBytes 8053063680 -ExpectedGB 8 }; Expect = $true }
        @{ Name = 'size: 8 GB packaging does not match a 16 GB stick'
           Run = { Test-UpgStickSize -SizeBytes 15931539456 -ExpectedGB 8 }; Expect = $false }
        @{ Name = 'size: 64 GB does not match a 2 TB drive'
           Run = { Test-UpgStickSize -SizeBytes 2000398934016 -ExpectedGB 64 }; Expect = $false }
        @{ Name = 'confirm word: the current label when there is one'
           Run = { Get-UpgConfirmWord -Disk $stick }; Expect = 'UPGV0' }
        @{ Name = 'confirm word: the model name when the stick is blank'
           Run = { Get-UpgConfirmWord -Disk $stick2 }; Expect = 'SanDisk Ultra' }
        @{ Name = 'sha256sums: parses sha256sum -c lines into path -> hex'
           Run = { $m = ConvertFrom-UpgSha256Sums -Lines @('ABCDEF0123456789abcdef0123456789abcdef0123456789abcdef0123456789  ./EFI/BOOT/BOOTX64.EFI', 'not a sum line'); "$($m.Count):$($m['EFI\BOOT\BOOTX64.EFI'])" }; Expect = '1:abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789' }
    )
    $failed = 0
    New-Line ''; New-Line "  upgrade_  stick writer $WriterVersion  -  SELF-TEST" 'Cyan'; New-Line ''
    foreach ($c in $cases) {
        $got = & $c.Run
        if ("$got" -eq "$($c.Expect)") { New-Line "    PASS  $($c.Name)" 'Green' }
        else { New-Line "    FAIL  $($c.Name)" 'Red'; New-Line "          expected '$($c.Expect)'" 'Red'; New-Line "          got      '$got'" 'Red'; $failed++ }
    }
    New-Line ''
    if ($failed -gt 0) { New-Line "  $failed check(s) failed" 'Red'; exit 1 }
    New-Line '  all checks passed' 'Green'; New-Line ''
}

# =============================================================================
#  main
# =============================================================================

if ($SelfTest) { Invoke-SelfTest; return }

$disks = @(Get-UpgAttachedDisks)
$cs = Get-CimInstance Win32_ComputerSystem
$os = Get-CimInstance Win32_OperatingSystem
$machine = "$($cs.Manufacturer) $($cs.Model)"

if ($PSCmdlet.ParameterSetName -eq 'List') {
    New-Line ''; New-Line "  upgrade_  stick writer $WriterVersion  -  attached disks" 'Cyan'; New-Line ''
    foreach ($d in $disks) { New-Line (Format-DiskLine $d); New-Line "          id: $($d.UniqueId)   serial: $($d.SerialNumber)" 'DarkGray' }
    New-Line ''
    New-Line '  Point at a disk with -Target <id> and -ExpectedSizeGB <as printed on it>; -Plan first.' 'DarkGray'
    New-Line ''
    return
}

$r = Resolve-UpgStickTarget -Disks $disks -Target $Target -ExpectedBytes $ExpectedSizeBytes -ExpectedGB $ExpectedSizeGB
$usb = @($disks | Where-Object { $_.BusType -eq 'USB' })
$expected = if ($ExpectedSizeBytes -gt 0) { $ExpectedSizeBytes } elseif ($ExpectedSizeGB -gt 0) { [long]($ExpectedSizeGB * 1e9) } else { 0 }
$mode = if ($Write) { 'write' } else { 'plan' }

New-Line ''; New-Line "  upgrade_  stick writer $WriterVersion  -  $($mode.ToUpper())" 'Cyan'
New-Line "  $machine   $($os.Caption) $($os.BuildNumber)   $($disks.Count) disks attached, $($usb.Count) on USB" 'DarkGray'
New-Line ''
foreach ($row in $r.Rows) {
    $tag = if ($row.Refusals.Count -eq 0) { 'WRITABLE' } else { 'refused ' }
    New-Line ("  {0} {1}" -f $tag, (Format-DiskLine $row.Disk).TrimStart()) $(if ($row.Refusals.Count -eq 0) { 'Green' } else { 'DarkGray' })
    if ($row.Refusals.Count -gt 0) { New-Line ("             " + ($row.Refusals -join '; ')) 'DarkGray' }
}
New-Line ''
New-Line "  DECISION: $($r.Decision)$(if ($r.Reason) { ' - ' + $r.Reason })" $(if ($r.Decision -eq 'selected') { 'Green' } else { 'Yellow' })

$perDisk = @($r.Rows | ForEach-Object { "disk$($_.Disk.Number)[$($_.Disk.BusType)/$($_.Disk.MediaType)/$($_.Disk.SizeBytes)]=$(if ($_.Refusals.Count -eq 0) { 'writable' } else { $_.Refusals -join ';' })" }) -join ' | '
$ev = @{ Machine = $machine; OsBuild = $os.BuildNumber; Mode = $mode; DisksAttached = $disks.Count; UsbDisks = $usb.Count
         Target = $Target; ExpectedBytes = $expected; Decision = $r.Decision; Refusals = $r.Reason; Written = 'n'; Verified = 'n'
         Notes = "[writer: elevated=$(Test-Elevated)] $perDisk" }

if ($Plan -or $r.Decision -ne 'selected') {
    $csv = Write-EvidenceRow -Row $ev
    New-Line "  nothing written. logged to $csv" 'Cyan'; New-Line ''
    if ($Write) { exit 2 }
    return
}

# --- write -------------------------------------------------------------------
if (-not (Test-Elevated)) { throw 'Writing a stick needs an elevated PowerShell (Administrator).' }
if (-not (Test-Path (Join-Path $Source 'SHA256SUMS'))) { throw "-Source must contain SHA256SUMS (make-kit.sh writes it): $Source" }
$chosen = $r.Chosen
$word = Get-UpgConfirmWord -Disk $chosen
New-Line ''
New-Line "  About to ERASE disk $($chosen.Number): $($chosen.FriendlyName), $([math]::Round($chosen.SizeBytes / 1e9, 1)) GB, $(if ($word -ne $chosen.FriendlyName) { "label '$word'" } else { 'no label' })." 'Yellow'
New-Line '  Everything on it will be gone. This is the only device this run will touch.' 'Yellow'
$answer = if ($ConfirmedLabel) { $ConfirmedLabel } else { Read-Host "  Type the $(if ($word -ne $chosen.FriendlyName) { 'label' } else { 'model name' }) shown above to confirm" }
if ($answer -cne $word) {
    $ev.Decision = 'refused'; $ev.Refusals = "confirmation did not match ('$answer' vs '$word')"
    $csv = Write-EvidenceRow -Row $ev
    New-Line "  confirmation did not match. nothing written. logged to $csv" 'Red'; New-Line ''
    exit 2
}
$w = $null
try {
    $w = Invoke-UpgStickWrite -Disk $chosen -SourceDir $Source -BootLabel $Label -BootGB $BootSizeGB
    $ev.Written = if ($w.Written) { 'y' } else { 'n' }
    $ev.Verified = if ($w.Verified) { 'y' } else { 'n' }
    $ev.Notes += " | wrote boot=$($w.BootLetter): data=$($w.DataLetter): files_verified=$($w.Files) failed=$($w.Failed -join ',')$(if ($w.Notes) { ' ' + ($w.Notes -join '; ') })"
} catch {
    $ev.Notes += " | write failed: $($_.Exception.Message)"
    New-Line "  ! $($_.Exception.Message)" 'Red'
}
$csv = Write-EvidenceRow -Row $ev
New-Line ''
if ($w -and $w.Verified) {
    New-Line "  written and verified: $($w.Files) files on $($w.BootLetter):  staging partition $($w.DataLetter):" 'Green'
    $stickJson = Join-Path "$($w.BootLetter):\" 'stick.json'
    $w.Stick | ConvertTo-Json | Out-File -FilePath $stickJson -Encoding ASCII
    New-Line "  identity for job.json written to $stickJson" 'DarkGray'
} else {
    New-Line '  NOT verified - do not use this stick.' 'Red'
}
New-Line "  logged to $csv" 'Cyan'; New-Line ''
if (-not ($w -and $w.Verified)) { exit 1 }
