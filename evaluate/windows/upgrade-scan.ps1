<#
.SYNOPSIS
    upgrade_ - preflight scanner. Answers whether this specific computer can run
    Linux, what will break, and what to do about it.

.DESCRIPTION
    Read-only. Makes no changes to this machine, touches no partitions, and
    sends nothing anywhere. Everything it reports comes from WMI, the registry,
    and the disk layout - all queried, none modified.

.PARAMETER OutDir
    Where to write the report. Defaults to your Desktop.

.PARAMETER NoFile
    Print to the console only; do not write a report file.

.PARAMETER Json
    Also write machine-readable JSON alongside the text report.

.PARAMETER DumpMachine
    Write a hardware-only machine capture (JSON) instead of a report: the PCI /
    ACPI / HDAUDIO device enumeration and basic system facts the checks read.
    No account names, no network names, no serial-bearing device paths. A
    curated capture becomes a permanent regression test in corpus/ - every
    machine the scanner meets once can be re-tested forever.

.EXAMPLE
    .\upgrade-scan.ps1

.EXAMPLE
    .\upgrade-scan.ps1 -Json -OutDir C:\Temp

.EXAMPLE
    .\upgrade-scan.ps1 -DumpMachine machine-capture.json
#>
[CmdletBinding()]
param(
    [string]$OutDir,
    [switch]$NoFile,
    [switch]$Json,
    [switch]$SelfTest,
    [string]$DumpMachine
)

$ErrorActionPreference = 'Stop'
$UpgVersion = '0.1.0'

# --- data ---------------------------------------------------------------
# The build script replaces this block with the file contents inline, so the
# released single file has no dependencies. Running from source uses the files.
#!INLINE-DATA-BEGIN!#
. (Join-Path $PSScriptRoot '..\..\data\devices.ps1')
. (Join-Path $PSScriptRoot '..\..\data\distros.ps1')
#!INLINE-DATA-END!#

# =============================================================================
#  infrastructure
# =============================================================================

$script:Checks = @()
$script:Unmatched = @()

function New-UpgCheck {
    param(
        [string]$Section,
        [string]$Title,
        [ValidateSet('ok','warn','fail','unknown','info')][string]$Status,
        [string]$Detail,
        [string]$Note,
        [string]$MinKernel,
        [string]$Remedy
    )
    $script:Checks += [pscustomobject]@{
        Section   = $Section
        Title     = $Title
        Status    = $Status
        Detail    = $Detail
        Note      = $Note
        MinKernel = $MinKernel
        Remedy    = $Remedy
    }
}

function Test-UpgAdmin {
    $id = [Security.Principal.WindowsIdentity]::GetCurrent()
    (New-Object Security.Principal.WindowsPrincipal($id)).IsInRole(
        [Security.Principal.WindowsBuiltInRole]::Administrator)
}

function Get-UpgPciId {
    param([string]$DeviceId)
    if ($DeviceId -match 'PCI\\VEN_([0-9A-Fa-f]{4})&DEV_([0-9A-Fa-f]{4})') {
        return ('{0}:{1}' -f $matches[1].ToLower(), $matches[2].ToLower())
    }
    return $null
}

function ConvertTo-UpgVersion {
    param([string]$Text)
    if ([string]::IsNullOrWhiteSpace($Text)) { return $null }
    if ($Text -eq 'rolling') { return [version]'99.0' }
    try { return [version]$Text } catch { return $null }
}

# =============================================================================
#  collection
# =============================================================================

function Get-UpgSystem {
    $cs  = Get-CimInstance Win32_ComputerSystem
    $os  = Get-CimInstance Win32_OperatingSystem
    $cpu = @(Get-CimInstance Win32_Processor)[0]
    $bios= Get-CimInstance Win32_BIOS
    $bat = @(Get-CimInstance Win32_Battery -ErrorAction SilentlyContinue)

    [pscustomobject]@{
        Vendor      = $cs.Manufacturer
        Model       = $cs.Model
        RamGB       = [math]::Round($cs.TotalPhysicalMemory / 1GB, 1)
        OsCaption   = $os.Caption
        OsBuild     = [int]$os.BuildNumber
        CpuName     = ($cpu.Name -replace '\s+', ' ').Trim()
        CpuArch     = $cpu.Architecture
        CpuCores    = $cpu.NumberOfCores
        BiosVersion = $bios.SMBIOSBIOSVersion
        BiosDate    = $bios.ReleaseDate
        IsLaptop    = ($bat.Count -gt 0)
        Firmware    = $env:firmware_type
    }
}

function Get-UpgPnp {
    # One expensive enumeration, reused by every hardware check.
    # CompatibleID carries the PCI class code (e.g. PCI\CC_0104 = RAID-mode
    # controller) - see "Identifiers for PCI devices" on learn.microsoft.com.
    Get-CimInstance Win32_PnPEntity | Select-Object Name, DeviceID, PNPClass, Service, CompatibleID
}

function Export-UpgMachineCapture {
    # Hardware-only snapshot of what the pure detection checks read, for the
    # corpus replay in -SelfTest. Deliberately narrow: only PCI / ACPI /
    # HDAUDIO / INTELAUDIO device paths (USB and ROOT instance paths can embed
    # serial numbers), and only the Sys fields the checks consume. A capture
    # holds hardware facts, never account, network or file information -
    # that's what makes it safe to commit where a machine report never is.
    param([string]$Path, $Sys, $Pnp)
    $capture = [ordered]@{
        SchemaVersion = 1
        Captured      = (Get-Date -Format 'yyyy-MM-dd')
        Label         = ("$($Sys.Vendor) $($Sys.Model)").Trim()
        Sys           = [ordered]@{
            Vendor   = $Sys.Vendor
            Model    = $Sys.Model
            RamGB    = $Sys.RamGB
            CpuName  = $Sys.CpuName
            CpuArch  = $Sys.CpuArch
            CpuCores = $Sys.CpuCores
            IsLaptop = $Sys.IsLaptop
            Firmware = $Sys.Firmware
        }
        Pnp           = @(
            $Pnp | Where-Object {
                $_.DeviceID -match '^(PCI|ACPI|HDAUDIO|INTELAUDIO)\\'
            } | ForEach-Object {
                [ordered]@{
                    Name         = $_.Name
                    DeviceID     = $_.DeviceID
                    PNPClass     = $_.PNPClass
                    Service      = $_.Service
                    CompatibleID = @($_.CompatibleID)
                }
            }
        )
        # Curator fills before the capture lands in corpus/: check Title ->
        # expected Status (a single status, or an array when a Title repeats,
        # e.g. two 'Graphics' checks on a hybrid laptop).
        Expected      = [ordered]@{}
    }
    $json = $capture | ConvertTo-Json -Depth 6
    [IO.File]::WriteAllText($Path, $json, (New-Object Text.UTF8Encoding $false))
    Write-Host "  machine capture written to $Path" -ForegroundColor Cyan
    Write-Host '  review it, fill Expected, and add it to evaluate/windows/corpus/ to make it a permanent test.' -ForegroundColor DarkGray
}

# =============================================================================
#  checks
# =============================================================================

function Test-UpgArchitecture {
    param($Sys)
    # Architecture 9 = x64, 12 = ARM64, 0 = x86
    if ($Sys.CpuArch -eq 12) {
        New-UpgCheck -Section 'Fundamentals' -Title 'CPU architecture' -Status 'fail' `
            -Detail "$($Sys.CpuName) (ARM64)" `
            -Note 'This is an ARM-based Windows machine. Mainstream Linux distributions do not support these laptops - firmware, GPU and power management support is incomplete to nonexistent. This is the one case where the answer is simply no.' `
            -Remedy 'Do not attempt a conversion on this machine.'
        return
    }
    if ($Sys.CpuArch -eq 0) {
        New-UpgCheck -Section 'Fundamentals' -Title 'CPU architecture' -Status 'warn' `
            -Detail "$($Sys.CpuName) (32-bit)" `
            -Note '32-bit only CPU. Most distributions dropped 32-bit support years ago. Debian and a few lightweight distributions still work.' `
            -Remedy 'Use Debian 32-bit, antiX or Q4OS.'
        return
    }
    New-UpgCheck -Section 'Fundamentals' -Title 'CPU architecture' -Status 'ok' `
        -Detail "$($Sys.CpuName) (64-bit, $($Sys.CpuCores) cores)"
}

function Test-UpgMemory {
    param($Sys)
    if ($Sys.RamGB -lt 3) {
        New-UpgCheck -Section 'Fundamentals' -Title 'Memory' -Status 'warn' `
            -Detail "$($Sys.RamGB) GB" `
            -Note 'Under 4 GB. A mainstream desktop will feel slow. This machine will run Linux noticeably better than it runs Windows, but pick a lightweight desktop.' `
            -Remedy 'Choose Xubuntu, Linux Mint Xfce or Lubuntu rather than a GNOME/KDE default.'
        return
    }
    New-UpgCheck -Section 'Fundamentals' -Title 'Memory' -Status 'ok' -Detail "$($Sys.RamGB) GB"
}

function Get-UpgSecureBootState {
    # 1 = enabled, 0 = disabled, $null = could not determine. Kept separate
    # from the judgment so the judgment is testable without this registry.
    try {
        (Get-ItemProperty -Path 'HKLM:\SYSTEM\CurrentControlSet\Control\SecureBoot\State' `
         -Name UEFISecureBootEnabled -ErrorAction Stop).UEFISecureBootEnabled
    } catch { $null }
}

function Test-UpgFirmware {
    param($Sys, $SecureBoot)
    $mode = if ($Sys.Firmware) { $Sys.Firmware } else { 'unknown' }
    New-UpgCheck -Section 'Fundamentals' -Title 'Firmware mode' -Status 'ok' -Detail $mode

    $sb = $SecureBoot

    if ($sb -eq 1) {
        New-UpgCheck -Section 'Fundamentals' -Title 'Secure Boot' -Status 'ok' `
            -Detail 'enabled' `
            -Note 'Ubuntu, Fedora, Linux Mint, Debian and openSUSE all boot with Secure Boot on. Smaller distributions may not, and the proprietary NVIDIA driver needs an extra enrolment step.' `
            -Remedy 'Leave it on unless your chosen distribution refuses to boot.'
    } elseif ($sb -eq 0) {
        New-UpgCheck -Section 'Fundamentals' -Title 'Secure Boot' -Status 'ok' -Detail 'disabled'
    } else {
        New-UpgCheck -Section 'Fundamentals' -Title 'Secure Boot' -Status 'info' -Detail 'could not determine'
    }
}

function Test-UpgStorageMode {
    param($Pnp)
    # Three signals, strongest first. Sources reconciled 2026-08-22 (R1/V5):
    #
    #  1. A VMD PCI device ID from the kernel's vmd.c table (data/devices.ps1).
    #  2. The RST VMD driver service bound to any device. The VMD-generation
    #     driver is iaStorVD (iaStorVD.inf/.sys - Intel article 000057787 and
    #     every OEM RST 18+ package), so this catches VMD devices whose ID we
    #     don't know yet.
    #  3. An Intel PCI storage controller reporting RAID class code 0104 in its
    #     compatible IDs (PCI\CC_0104 - "Identifiers for PCI devices",
    #     learn.microsoft.com). This is the controller itself declaring RAID /
    #     remap mode, and covers the pre-VMD RST generations (Skylake-Comet
    #     Lake "RST Premium" NVMe remapping) whatever driver is bound.
    #
    # The old regex here was '^iaStorV', which matched only the Vista-era
    # inbox driver (iaStorV) and the VMD driver (iaStorVD). It missed the
    # whole pre-VMD RST family - iaStorA, iaStorAC, iaStorAVC (Intel article
    # 000059291; Microsoft's Win10 1903 RST compatibility-hold notice) - which
    # is exactly the 2015-2020 hardware this tool targets. An iaStor* service
    # on a controller that does NOT report RAID class is RST software managing
    # an AHCI-mode controller: the disk should be visible to Linux, so that
    # case warns and asks the user to verify, rather than failing.
    $vmdIds = Get-UpgVmdDeviceIds
    $hit = $null; $raidHit = $null; $rstSvcHit = $null
    foreach ($d in $Pnp) {
        if (-not $d.DeviceID) { continue }
        $id = Get-UpgPciId $d.DeviceID
        if ($id -and ($vmdIds -contains $id)) { $hit = $d; break }
        if ($d.Service -and ($d.Service -match '^iaStorVD')) { $hit = $d; break }
        if ($id -and $id -like '8086:*' -and $d.CompatibleID -and
            ($d.CompatibleID -match 'CC_0104')) {
            if (-not $raidHit) { $raidHit = $d }
        }
        if ($d.Service -and ($d.Service -match '^iaStor')) {
            if (-not $rstSvcHit) { $rstSvcHit = $d }
        }
    }
    if (-not $hit -and $raidHit) { $hit = $raidHit }

    if ($hit) {
        New-UpgCheck -Section 'Storage' -Title 'Storage controller mode' -Status 'fail' `
            -Detail "Intel RST / VMD active ($($hit.Name))" `
            -Note 'Your SSD is behind Intel RST/VMD. Linux installers will show no disks at all - the drive is simply invisible. This is the single most common reason a Linux install appears to fail on modern laptops, and it looks like a broken installer rather than a setting.' `
            -Remedy @'
Switch the BIOS from RST/VMD to AHCI. Windows will not boot afterwards unless
you prepare it first, so do this only when you are committed:

  1. Open an Administrator Command Prompt in Windows and run:  bcdedit /set safeboot minimal
  2. Reboot into BIOS. Find SATA/NVMe/VMD mode and set it to AHCI.
  3. Let Windows boot into Safe Mode once - it installs the AHCI driver.
  4. Administrator Command Prompt again:  bcdedit /deletevalue safeboot
  5. Reboot. Windows now runs in AHCI mode and Linux installers can see the disk.

If you are wiping Windows entirely you can skip steps 1-4 and simply switch to
AHCI - but then Windows will not boot, so there is no going back.
'@
        return
    }

    if ($rstSvcHit) {
        New-UpgCheck -Section 'Storage' -Title 'Storage controller mode' -Status 'warn' `
            -Detail "Intel RST driver present, controller not in RAID mode ($($rstSvcHit.Service))" `
            -Note 'Windows is using an Intel RST storage driver, but the disk controller reports plain AHCI/NVMe, so Linux installers should see the disk normally. This combination has not been confirmed on real hardware yet - please report what you find.' `
            -Remedy 'Before wiping anything, boot the Linux installer USB and confirm it lists your internal disk. If it shows no disks, look in the BIOS for a setting named VMD, RST, Optane or "RAID mode" and set it to AHCI.'
        return
    }

    New-UpgCheck -Section 'Storage' -Title 'Storage controller mode' -Status 'ok' `
        -Detail 'standard AHCI / NVMe - visible to Linux installers'
}

function Get-UpgDiskFacts {
    # Collection half of the disk check: everything read from the live OS,
    # gathered into one object so the judgment half is testable without it.
    $disks = @(Get-Disk | Where-Object { $_.BusType -ne 'File Backed Virtual' } |
               ForEach-Object {
                   [pscustomobject]@{
                       Number         = $_.Number
                       FriendlyName   = $_.FriendlyName
                       Size           = $_.Size
                       PartitionStyle = $_.PartitionStyle
                       BusType        = $_.BusType
                   }
               })

    $vol = Get-Volume -DriveLetter C -ErrorAction SilentlyContinue
    $sysVolume = if ($vol) {
        [pscustomobject]@{ Size = $vol.Size; SizeRemaining = $vol.SizeRemaining }
    } else { $null }

    # Shrinkable space is the gate for keeping Windows as a fallback, and the
    # same query Disk Management uses. Measuring it on every scan also closes
    # the scanner half of RISKS R18 and feeds the V4 validation gate.
    $shrinkGB = $null
    try {
        $part = Get-Partition -DriveLetter C -ErrorAction Stop
        $supported = Get-PartitionSupportedSize -DriveLetter C -ErrorAction Stop
        $shrinkGB = [math]::Round(($part.Size - $supported.SizeMin) / 1GB, 1)
    } catch { }

    [pscustomobject]@{
        Disks          = $disks
        SysVolume      = $sysVolume
        ShrinkGB       = $shrinkGB
        Disk0PartCount = @(Get-Partition -DiskNumber 0 -ErrorAction SilentlyContinue).Count
    }
}

function Test-UpgDisk {
    param($Facts, [bool]$IsAdmin)
    $disks = @($Facts.Disks)
    foreach ($d in $disks) {
        New-UpgCheck -Section 'Storage' -Title "Disk $($d.Number)" -Status 'info' `
            -Detail ('{0} - {1} GB, {2}, {3}' -f $d.FriendlyName, [math]::Round($d.Size/1GB,1), $d.PartitionStyle, $d.BusType)
    }

    $sys = $Facts.SysVolume
    if (-not $sys) {
        New-UpgCheck -Section 'Storage' -Title 'Disk space' -Status 'unknown' -Detail 'could not read C:'
        return
    }

    $freeGB = [math]::Round($sys.SizeRemaining / 1GB, 1)
    $usedGB = [math]::Round(($sys.Size - $sys.SizeRemaining) / 1GB, 1)

    # No external drive anywhere in this design. Your files either ride a USB
    # stick (clean slate) or stay put while Windows is shrunk aside (safety
    # copy). So the scanner reports what is used, and how far the disk can
    # shrink - never "buy a backup drive".
    New-UpgCheck -Section 'Storage' -Title 'Disk in use' -Status 'info' `
        -Detail "$usedGB GB used, $freeGB GB free" `
        -Note 'Your personal files are part of that used space; the rest is Windows and installed programs. The converter puts your files on a USB stick, or leaves them in place while it fits Linux alongside. It never needs an external hard drive.'

    $shrinkGB = $Facts.ShrinkGB
    if ($null -ne $shrinkGB) {
        # ~20 GB for Fedora itself, plus room for your files to stay in place.
        if ($shrinkGB -lt 25) {
            New-UpgCheck -Section 'Storage' -Title 'Room to keep Windows' -Status 'warn' `
                -Detail "$shrinkGB GB can be freed by shrinking" `
                -Note 'Too little room to install Linux while keeping Windows as a fallback. This machine can still convert - your files travel on the USB stick (the clean-slate path) - but there is no space to keep a safety copy of Windows on the internal disk.' `
                -Remedy 'Emptying the Recycle Bin, clearing Downloads, and removing large unused programs raises this number. No external drive is needed either way.'
        } else {
            New-UpgCheck -Section 'Storage' -Title 'Room to keep Windows' -Status 'ok' `
                -Detail "$shrinkGB GB can be freed by shrinking" `
                -Note 'Enough room to install Linux while keeping Windows shrunk aside as a fallback, until you confirm everything works and reclaim the space.'
        }
    } elseif (-not $IsAdmin) {
        New-UpgCheck -Section 'Storage' -Title 'Room to keep Windows' -Status 'info' `
            -Detail 'requires Administrator to measure' `
            -Note 'Measuring how far the disk can shrink needs Administrator rights. Without it, we cannot yet tell you whether Windows can be kept as a fallback - the clean-slate path (files on the USB stick) still works regardless.' `
            -Remedy 'Re-run this scanner as Administrator to get this number.'
    } else {
        New-UpgCheck -Section 'Storage' -Title 'Room to keep Windows' -Status 'info' `
            -Detail 'could not measure shrinkable space' `
            -Note 'Windows did not report how far its partition can shrink; Fast Startup or a dirty volume can cause this even with Administrator rights. The converter re-checks before doing anything.'
    }

    $partCount = $Facts.Disk0PartCount
    if ($disks.Count -gt 0 -and $disks[0].PartitionStyle -eq 'MBR' -and $partCount -ge 4) {
        New-UpgCheck -Section 'Storage' -Title 'Partition table' -Status 'warn' `
            -Detail "MBR with $partCount primary partitions" `
            -Note 'MBR discs allow only four primary partitions and you are at the limit. The installer will not be able to create a new one.' `
            -Remedy 'Delete or convert a partition, or erase the disk and let the installer create a fresh GPT layout.'
    }
}

function Get-UpgFastStartupState {
    # 1 = enabled, 0 = disabled, $null = key unreadable.
    try {
        (Get-ItemProperty -Path 'HKLM:\SYSTEM\CurrentControlSet\Control\Session Manager\Power' `
         -Name HiberbootEnabled -ErrorAction Stop).HiberbootEnabled
    } catch { $null }
}

function Test-UpgFastStartup {
    # Fast Startup leaves NTFS in a dirty hibernated state. Linux then refuses
    # to mount it read-write, and partition resize is unsafe. Cheap to check,
    # and it silently ruins installs.
    param($HiberbootEnabled)
    $val = $HiberbootEnabled

    if ($val -eq 1) {
        New-UpgCheck -Section 'Storage' -Title 'Fast Startup' -Status 'warn' `
            -Detail 'enabled' `
            -Note 'Windows Fast Startup does not fully shut down - it hibernates. That leaves the Windows partition in a state Linux will not write to, and makes resizing it unsafe. Shutting down does not clear it; only a restart or disabling the feature does.' `
            -Remedy 'Control Panel > Power Options > Choose what the power buttons do > Change settings that are currently unavailable > untick "Turn on fast startup". Then shut down normally.'
    } elseif ($val -eq 0) {
        New-UpgCheck -Section 'Storage' -Title 'Fast Startup' -Status 'ok' -Detail 'disabled'
    }
}

function Get-UpgBitLockerState {
    # Succeeded=$false means the query itself failed (treat the disk as
    # possibly encrypted); EncryptedMounts lists volumes with protection On.
    try {
        $vols = @(Get-BitLockerVolume -ErrorAction Stop | Where-Object { $_.ProtectionStatus -eq 'On' })
        [pscustomobject]@{
            Succeeded       = $true
            EncryptedMounts = @($vols | ForEach-Object { $_.MountPoint })
        }
    } catch {
        [pscustomobject]@{ Succeeded = $false; EncryptedMounts = @() }
    }
}

function Test-UpgBitLocker {
    param([bool]$IsAdmin, $State)
    if (-not $IsAdmin) {
        New-UpgCheck -Section 'Storage' -Title 'BitLocker' -Status 'unknown' `
            -Detail 'requires Administrator to check' `
            -Note 'Could not determine whether this disk is encrypted. This matters more than any other unknown here: if BitLocker is on and you resize or reinstall without the recovery key, the data is gone permanently and no recovery is possible.' `
            -Remedy 'Re-run this scanner as Administrator, or check manually: Settings > Privacy & security > Device encryption.'
        return
    }

    if ($State -and $State.Succeeded) {
        $mounts = @($State.EncryptedMounts)
        if ($mounts.Count -gt 0) {
            New-UpgCheck -Section 'Storage' -Title 'BitLocker' -Status 'warn' `
                -Detail ("enabled on " + ($mounts -join ', ')) `
                -Note 'This disk is encrypted. Any partition change without the recovery key destroys the data irrecoverably.' `
                -Remedy 'Save your recovery key before doing anything: run "manage-bde -protectors -get C:" as Administrator, or retrieve it from account.microsoft.com/devicerecoverykey. Save it somewhere that is not this computer. Then either suspend or fully decrypt BitLocker before touching partitions.'
        } else {
            New-UpgCheck -Section 'Storage' -Title 'BitLocker' -Status 'ok' -Detail 'not enabled'
        }
    } else {
        New-UpgCheck -Section 'Storage' -Title 'BitLocker' -Status 'unknown' `
            -Detail 'query failed' `
            -Note 'Could not read encryption status. Treat the disk as possibly encrypted until you have confirmed otherwise.' `
            -Remedy 'Check Settings > Privacy & security > Device encryption before proceeding.'
    }
}

function Get-UpgNtDeviceName {
    # QueryDosDevice('Z:') -> '\Device\HarddiskVolumeN'. Lets the mounted ESP
    # be compared with bcdedit's device line when bcdedit prints the NT path.
    param([string]$Letter)
    try {
        Add-Type -Namespace Upg -Name Native -MemberDefinition @'
[DllImport("kernel32.dll", SetLastError=true, CharSet=CharSet.Unicode)]
public static extern uint QueryDosDeviceW(string name, System.Text.StringBuilder target, uint max);
'@ -ErrorAction Stop
    } catch { }  # type already loaded on a second call
    $sb = New-Object Text.StringBuilder 1024
    $n = [Upg.Native]::QueryDosDeviceW($Letter.TrimEnd('\'), $sb, 1024)
    if ($n -gt 0) { $sb.ToString() } else { $null }
}

function Get-UpgEspFacts {
    # Collection half of the ESP check - elevated only (mountvol /S needs
    # Administrator; same cap as the BitLocker and shrink queries). Mounts
    # the EFI system partition, measures it, and records which volume the
    # firmware's Windows Boot Manager entry points at. Read-only: the mount
    # is only ever read from, and is unmounted in finally.
    try {
        $letter = $null
        foreach ($c in [char[]]'ZYXWVUT') {
            if (-not (Test-Path ("${c}:" + '\'))) { $letter = "${c}:"; break }
        }
        if (-not $letter) { throw 'no free drive letter' }
        cmd /c "mountvol $letter /S" 2>&1 | Out-Null
        if (-not (Test-Path ($letter + '\'))) { throw 'mountvol /S failed' }
        try {
            $di = New-Object IO.DriveInfo($letter)
            $free  = $di.AvailableFreeSpace
            $total = $di.TotalSize
            $hasBootFiles = Test-Path ($letter + '\EFI\Microsoft\Boot\bootmgfw.efi')
            # Which volume does the firmware's Windows Boot Manager entry load
            # bootmgfw.efi from? bcdedit prints our letter when it is the same
            # volume; otherwise \Device\HarddiskVolumeN, which QueryDosDevice
            # lets us compare against the mount.
            $bcd = (bcdedit /enum '{bootmgr}' 2>&1 | Out-String)
            $bcdDevice = $null
            if ($bcd -match '(?m)^device\s+partition=(\S+)') { $bcdDevice = $matches[1] }
            $points = $null
            if ($bcdDevice) {
                if ($bcdDevice -ieq $letter) { $points = $true }
                elseif ($bcdDevice -match '^\\Device\\') {
                    $nt = Get-UpgNtDeviceName $letter
                    if ($nt) { $points = ($nt -ieq $bcdDevice) }
                } else { $points = $false }
            }
            [pscustomobject]@{
                Succeeded           = $true
                FreeBytes           = $free
                TotalBytes          = $total
                HasWindowsBootFiles = $hasBootFiles
                BootmgrPointsAtEsp  = $points
                BcdDevice           = $bcdDevice
            }
        } finally {
            cmd /c "mountvol $letter /D" 2>&1 | Out-Null
        }
    } catch {
        [pscustomobject]@{ Succeeded = $false; FreeBytes = $null; TotalBytes = $null
                           HasWindowsBootFiles = $null; BootmgrPointsAtEsp = $null; BcdDevice = $null }
    }
}

function Test-UpgEsp {
    # Judgment half. R21's decision (2026-08-30): the keep-Windows default
    # needs >= 32 MiB free on the ESP (5x the ~6.2 MB a Fedora alongside
    # install was measured to add) AND the firmware's Windows Boot Manager
    # entry must point at that same ESP - otherwise the alongside install
    # would add Linux's loader to a partition the machine does not boot from.
    # Failing either steers to clean slate; it never blocks conversion.
    param([bool]$IsAdmin, $Facts)
    if (-not $IsAdmin) {
        New-UpgCheck -Section 'Storage' -Title 'Boot partition (ESP)' -Status 'info' `
            -Detail 'requires Administrator to check' `
            -Note 'Whether the small boot partition has room to add Linux alongside Windows needs Administrator rights to measure. The clean-slate path (files on the USB stick) works regardless.' `
            -Remedy 'Re-run this scanner as Administrator to get this number.'
        return
    }
    if (-not $Facts -or -not $Facts.Succeeded) {
        New-UpgCheck -Section 'Storage' -Title 'Boot partition (ESP)' -Status 'unknown' `
            -Detail 'could not read the EFI system partition' `
            -Note 'The boot partition could not be mounted or measured, so there is no basis to promise the keep-Windows path. The converter re-checks before doing anything.'
        return
    }
    $freeMiB = [math]::Round($Facts.FreeBytes / 1MB, 1)
    if (-not $Facts.HasWindowsBootFiles -or $Facts.BootmgrPointsAtEsp -eq $false) {
        New-UpgCheck -Section 'Storage' -Title 'Boot partition (ESP)' -Status 'warn' `
            -Detail 'Windows does not boot from the expected partition' `
            -Note 'The partition this machine actually boots Windows from is not the one its boot files were found on (or those files are missing). Installing Linux alongside would touch a partition the firmware does not boot from, so the keep-Windows fallback cannot be promised here. The clean-slate path still converts this machine.'
        return
    }
    if ($null -eq $Facts.BootmgrPointsAtEsp) {
        New-UpgCheck -Section 'Storage' -Title 'Boot partition (ESP)' -Status 'unknown' `
            -Detail "$freeMiB MiB free, but the Windows boot entry could not be resolved" `
            -Note 'Could not confirm that the firmware boots Windows from this partition. Treat the keep-Windows path as unconfirmed; the converter re-checks before doing anything.'
        return
    }
    if ($Facts.FreeBytes -lt 32MB) {
        New-UpgCheck -Section 'Storage' -Title 'Boot partition (ESP)' -Status 'warn' `
            -Detail "only $freeMiB MiB free on the boot partition" `
            -Note 'Adding Linux alongside Windows puts its boot files on this partition (about 7 MB measured, and future kernel updates need headroom). Below 32 MiB free the keep-Windows path is not offered; the clean-slate path still works.' `
            -Remedy 'Nothing to do by hand - do not resize or delete files on the boot partition yourself. The converter will steer this machine to the clean-slate path.'
        return
    }
    New-UpgCheck -Section 'Storage' -Title 'Boot partition (ESP)' -Status 'ok' `
        -Detail "$freeMiB MiB free; Windows boots from it" `
        -Note 'The boot partition has room to add Linux''s boot files while keeping Windows bootable alongside.'
}

function Test-UpgWifi {
    param($Pnp)
    $db = Get-UpgWifiDatabase
    $fb = Get-UpgWifiVendorFallback

    $cards = @($Pnp | Where-Object {
        $_.PNPClass -eq 'Net' -and $_.DeviceID -like 'PCI\*' -and
        ($_.Name -match 'Wi-?Fi|Wireless|WLAN|802\.11')
    })

    if ($cards.Count -eq 0) {
        New-UpgCheck -Section 'Hardware' -Title 'Wi-Fi' -Status 'info' `
            -Detail 'no wireless card detected' `
            -Note 'No PCI wireless adapter found. If this is a desktop on Ethernet, that is expected.'
        return
    }

    foreach ($c in $cards) {
        $id = Get-UpgPciId $c.DeviceID
        if (-not $id) { continue }
        $vendor = $id.Split(':')[0]

        if ($db.ContainsKey($id)) {
            $e = $db[$id]
            New-UpgCheck -Section 'Hardware' -Title 'Wi-Fi' -Status $e.Status `
                -Detail "$($e.Name) [$id] - driver $($e.Driver)" `
                -Note $e.Note -MinKernel $e.MinKernel `
                -Remedy $(if ($e.Status -eq 'fail') { 'Before you start, get a USB Ethernet adapter or confirm your phone can do USB tethering. Without one you may finish the install with no way to get online.' })
        } elseif ($fb.ContainsKey($vendor)) {
            $e = $fb[$vendor]
            $script:Unmatched += "wifi $id ($($c.Name))"
            New-UpgCheck -Section 'Hardware' -Title 'Wi-Fi' -Status $e.Status `
                -Detail "$($c.Name) [$id] - unrecognised $($e.Vendor) part" `
                -Note $e.Note
        } else {
            $script:Unmatched += "wifi $id ($($c.Name))"
            New-UpgCheck -Section 'Hardware' -Title 'Wi-Fi' -Status 'unknown' `
                -Detail "$($c.Name) [$id]" `
                -Note 'Unrecognised wireless vendor. Search "linux <this device id>" before committing, and have a wired fallback ready.'
        }
    }
}

function Test-UpgGpu {
    param($Pnp)
    $db    = Get-UpgGpuDatabase
    $rules = Get-UpgGpuVendorRules

    $gpus = @($Pnp | Where-Object { $_.PNPClass -eq 'Display' -and $_.DeviceID -like 'PCI\*' })
    if ($gpus.Count -eq 0) {
        New-UpgCheck -Section 'Hardware' -Title 'Graphics' -Status 'unknown' -Detail 'none detected'
        return
    }

    foreach ($g in $gpus) {
        $id = Get-UpgPciId $g.DeviceID
        if (-not $id) { continue }
        $vendor = $id.Split(':')[0]

        if ($db.ContainsKey($id)) {
            $e = $db[$id]
            New-UpgCheck -Section 'Hardware' -Title 'Graphics' -Status $e.Status `
                -Detail "$($e.Name) [$id] - driver $($e.Driver)" `
                -Note $e.Note -MinKernel $e.MinKernel
        } elseif ($rules.ContainsKey($vendor)) {
            $e = $rules[$vendor]
            $script:Unmatched += "gpu $id ($($g.Name))"
            New-UpgCheck -Section 'Hardware' -Title 'Graphics' -Status $e.Status `
                -Detail "$($g.Name) [$id] - driver $($e.Driver)" `
                -Note $e.Note
        } else {
            $script:Unmatched += "gpu $id ($($g.Name))"
            New-UpgCheck -Section 'Hardware' -Title 'Graphics' -Status 'unknown' `
                -Detail "$($g.Name) [$id]" -Note 'Unrecognised graphics vendor.'
        }
    }

    if ($gpus.Count -gt 1) {
        New-UpgCheck -Section 'Hardware' -Title 'Hybrid graphics' -Status 'warn' `
            -Detail "$($gpus.Count) GPUs present" `
            -Note 'This laptop switches between an integrated and a discrete GPU. On Linux that switching works but is manual and less transparent than under Windows - expect to choose which GPU an application uses, and expect worse battery life than Windows if the discrete GPU stays awake.' `
            -Remedy 'Pick a distribution with graphics switching built in - Pop!_OS and Fedora handle this best.'
    }
}

function Test-UpgAudio {
    param($Pnp)
    $quirks = Get-UpgAudioQuirks
    $found = $false

    foreach ($q in $quirks) {
        $hit = $Pnp | Where-Object { $_.DeviceID -and $_.DeviceID -match $q.Match } | Select-Object -First 1
        if ($hit) {
            $found = $true
            New-UpgCheck -Section 'Hardware' -Title 'Audio' -Status $q.Status `
                -Detail "$($q.Name) detected" -Note $q.Note -MinKernel $q.MinKernel `
                -Remedy 'Test audio in the live USB session before installing. If headphones work but the internal speakers do not, that is this exact issue.'
        }
    }

    if (-not $found) {
        New-UpgCheck -Section 'Hardware' -Title 'Audio' -Status 'ok' `
            -Detail 'standard HD Audio - no known smart-amp quirk'
    }
}

function Test-UpgVendor {
    param($Sys)
    $subject = "$($Sys.Vendor) $($Sys.Model)"
    foreach ($q in Get-UpgVendorQuirks) {
        if ($subject -match $q.Match) {
            New-UpgCheck -Section 'Hardware' -Title 'Vendor-specific' -Status $q.Status `
                -Detail $subject -Note $q.Note
            return
        }
    }
}

function Test-UpgWin11Context {
    param($Sys, [bool]$IsAdmin)
    if ($Sys.OsBuild -ge 22000) {
        New-UpgCheck -Section 'Context' -Title 'Current OS' -Status 'info' `
            -Detail "$($Sys.OsCaption) (build $($Sys.OsBuild))"
        return
    }
    New-UpgCheck -Section 'Context' -Title 'Current OS' -Status 'info' `
        -Detail "$($Sys.OsCaption) (build $($Sys.OsBuild))" `
        -Note 'Windows 10 stopped receiving security updates in October 2025. This machine is not getting patched any more, which is the reason most people are reading this report.'
}

function Get-UpgInstalledApps {
    $paths = @(
        'HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\*',
        'HKLM:\SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall\*',
        'HKCU:\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\*'
    )
    $names = @()
    foreach ($p in $paths) {
        try {
            $names += Get-ItemProperty $p -ErrorAction SilentlyContinue |
                      Where-Object { $_.DisplayName } |
                      ForEach-Object { $_.DisplayName }
        } catch { }
    }
    $names | Sort-Object -Unique
}

function Test-UpgApps {
    # $Apps injectable for the self-test; omitted = enumerate the live registry.
    param($Apps)
    $apps = if ($null -ne $Apps) { @($Apps) } else { Get-UpgInstalledApps }
    if (-not $apps -or $apps.Count -eq 0) {
        New-UpgCheck -Section 'Software' -Title 'Installed software' -Status 'unknown' `
            -Detail 'could not enumerate'
        return
    }

    $reported = @{}
    foreach ($rule in Get-UpgAppRiskDatabase) {
        $matched = @($apps | Where-Object { $_ -match $rule.Match })
        if ($matched.Count -eq 0) { continue }
        $key = $rule.Match
        if ($reported.ContainsKey($key)) { continue }
        $reported[$key] = $true

        $status = switch ($rule.Severity) {
            'blocker'  { 'fail' }
            'friction' { 'warn' }
            default    { 'info' }
        }
        $shown = ($matched | Select-Object -First 3) -join ', '
        if ($matched.Count -gt 3) { $shown += " (+$($matched.Count - 3) more)" }

        New-UpgCheck -Section 'Software' -Title $(
            switch ($rule.Severity) {
                'blocker'  { 'No Linux equivalent' }
                'friction' { 'Works differently' }
                default    { 'Worth knowing' }
            }) -Status $status -Detail $shown -Note $rule.Note
    }

    New-UpgCheck -Section 'Software' -Title 'Programs installed' -Status 'info' `
        -Detail "$($apps.Count) total"
}

# =============================================================================
#  verdict
# =============================================================================

function Get-UpgRequiredKernel {
    $best = $null
    foreach ($c in $script:Checks) {
        $v = ConvertTo-UpgVersion $c.MinKernel
        if ($v -and (-not $best -or $v -gt $best)) { $best = $v }
    }
    $best
}

function Get-UpgCheckPriority {
    # A flat list of problems is useless - "Fast Startup is on" and "this disk
    # may be encrypted and you may lose everything" are not the same kind of
    # thing. Everything the user sees gets ranked into one of these buckets.
    param($Check)
    if ($Check.Section -eq 'Software') { return 4 }
    if ($Check.Status -eq 'fail')      { return 1 }
    if ($Check.Status -eq 'unknown')   { return 1 }  # unknown risk is still risk
    if ($Check.Section -eq 'Storage')  { return 2 }
    return 3
}

function Get-UpgPriorityLabel {
    param([int]$Priority)
    switch ($Priority) {
        1 { 'MUST RESOLVE - these can cost you data or stop the install' }
        2 { 'DO BEFORE YOU START' }
        3 { 'EXPECT TO DEAL WITH' }
        4 { 'SOFTWARE YOU WILL LOSE OR HAVE TO REPLACE' }
    }
}

function Get-UpgVerdict {
    # Software blockers are a different kind of "no" from hardware blockers:
    # the machine still runs Linux fine, the person just loses tools they need.
    # Conflating them would tell someone their laptop is unsuitable when the
    # real answer is "your laptop is fine, but Photoshop is not coming with you".
    $hwFail = @($script:Checks | Where-Object { $_.Status -eq 'fail' -and $_.Section -ne 'Software' })
    $swFail = @($script:Checks | Where-Object { $_.Status -eq 'fail' -and $_.Section -eq 'Software' })
    $issues = @($script:Checks | Where-Object { $_.Status -eq 'fail' -or $_.Status -eq 'warn' -or $_.Status -eq 'unknown' })

    $grouped = @()
    foreach ($p in 1..4) {
        $inBucket = @($issues | Where-Object { (Get-UpgCheckPriority $_) -eq $p })
        if ($inBucket.Count -eq 0) { continue }
        $grouped += [pscustomobject]@{
            Priority = $p
            Label    = Get-UpgPriorityLabel $p
            Items    = @($inBucket | ForEach-Object { "$($_.Title): $($_.Detail)" })
        }
    }

    if ($hwFail.Count -gt 0) {
        return [pscustomobject]@{
            Level = 'RED'
            Summary = 'Do not convert this machine as it stands. Something here blocks the install outright - resolve it, or use a different machine.'
            Groups = $grouped
        }
    }
    if ($swFail.Count -gt 0) {
        return [pscustomobject]@{
            Level = 'YELLOW'
            Summary = 'The hardware is fine. The real question is whether you can work without the software listed below - that is a decision only you can make.'
            Groups = $grouped
        }
    }
    if ($grouped.Count -gt 0) {
        return [pscustomobject]@{
            Level = 'YELLOW'
            Summary = 'Convertible, with specific steps to take first.'
            Groups = $grouped
        }
    }
    [pscustomobject]@{
        Level = 'GREEN'
        Summary = 'No obstacles found. This machine should convert cleanly.'
        Groups = @()
    }
}

function Get-UpgRecommendation {
    param($RequiredKernel)
    $hasNvidia = @($script:Checks | Where-Object { $_.Detail -match 'NVIDIA' }).Count -gt 0
    $lowRam    = @($script:Checks | Where-Object { $_.Title -eq 'Memory' -and $_.Status -eq 'warn' }).Count -gt 0

    $candidates = @()
    foreach ($d in Get-UpgDistroTable) {
        $k = ConvertTo-UpgVersion $d.Kernel
        if ($RequiredKernel -and $k -and $k -lt $RequiredKernel) { continue }
        if ($hasNvidia -and -not $d.NvidiaEasy) { continue }
        $candidates += $d
    }

    [pscustomobject]@{
        Distros   = @($candidates | Sort-Object -Property @{e={$_.Newcomer}} -Descending | Select-Object -First 3)
        HasNvidia = $hasNvidia
        LowRam    = $lowRam
        Excluded  = @(Get-UpgDistroTable | Where-Object {
                        $k = ConvertTo-UpgVersion $_.Kernel
                        $RequiredKernel -and $k -and $k -lt $RequiredKernel
                     })
    }
}

# =============================================================================
#  rendering
# =============================================================================

function Get-UpgStatusMark {
    param([string]$Status)
    switch ($Status) {
        'ok'      { '[ OK ]' }
        'warn'    { '[WARN]' }
        'fail'    { '[FAIL]' }
        'unknown' { '[ ?? ]' }
        default   { '[ -- ]' }
    }
}

function Get-UpgStatusColor {
    param([string]$Status)
    switch ($Status) {
        'ok'      { 'Green' }
        'warn'    { 'Yellow' }
        'fail'    { 'Red' }
        'unknown' { 'Magenta' }
        default   { 'Gray' }
    }
}

function Format-UpgWrap {
    param([string]$Text, [int]$Width = 68, [string]$Indent = '         ')
    if ([string]::IsNullOrWhiteSpace($Text)) { return @() }
    $out = @()
    foreach ($para in ($Text -split "`r?`n")) {
        if ($para.Trim() -eq '') { $out += ''; continue }
        # Pre-formatted blocks (leading spaces) pass through untouched.
        if ($para -match '^\s{2,}\S') { $out += ($Indent + $para); continue }
        $line = ''
        foreach ($word in ($para -split '\s+' | Where-Object { $_ })) {
            if (($line.Length + $word.Length + 1) -gt $Width) {
                $out += ($Indent + $line); $line = $word
            } else {
                $line = if ($line) { "$line $word" } else { $word }
            }
        }
        if ($line) { $out += ($Indent + $line) }
    }
    $out
}

function Write-UpgReport {
    param($Sys, $Verdict, $Rec, $RequiredKernel, [bool]$IsAdmin)

    $L = New-Object System.Collections.Generic.List[string]
    function Add-L { param([string]$s = '') $L.Add($s) }

    Add-L '==============================================================================='
    Add-L "  upgrade_  preflight  v$UpgVersion"
    Add-L "  $($Sys.Vendor) $($Sys.Model)"
    Add-L "  $(Get-Date -Format 'yyyy-MM-dd HH:mm')"
    Add-L '==============================================================================='
    Add-L ''
    Add-L "  $($Sys.CpuName)"
    Add-L "  $($Sys.RamGB) GB RAM   |   BIOS $($Sys.BiosVersion)   |   $($Sys.OsCaption)"
    if (-not $IsAdmin) {
        Add-L ''
        Add-L '  ! Running without Administrator rights. Encryption status could not be'
        Add-L '    checked - see the BitLocker line below.'
    }
    Add-L ''

    foreach ($section in @('Fundamentals','Storage','Hardware','Software','Context')) {
        $items = @($script:Checks | Where-Object { $_.Section -eq $section })
        if ($items.Count -eq 0) { continue }
        Add-L ''
        Add-L "-- $($section.ToUpper()) $('-' * (75 - $section.Length))"
        Add-L ''
        foreach ($c in $items) {
            Add-L ("  {0} {1,-24} {2}" -f (Get-UpgStatusMark $c.Status), $c.Title, $c.Detail)
            if ($c.MinKernel) { Add-L ("         needs kernel $($c.MinKernel) or newer") }
            if ($c.Note)   { Format-UpgWrap $c.Note   | ForEach-Object { Add-L $_ } }
            if ($c.Remedy) {
                Add-L ''
                Add-L '         WHAT TO DO:'
                Format-UpgWrap $c.Remedy -Indent '           ' | ForEach-Object { Add-L $_ }
            }
            Add-L ''
        }
    }

    Add-L ''
    Add-L '==============================================================================='
    Add-L "  VERDICT: $($Verdict.Level)"
    Add-L '==============================================================================='
    Add-L ''
    Format-UpgWrap $Verdict.Summary -Indent '  ' | ForEach-Object { Add-L $_ }
    Add-L ''
    foreach ($g in $Verdict.Groups) {
        Add-L "  $($g.Label)"
        foreach ($item in $g.Items) { Add-L "    - $item" }
        Add-L ''
    }

    if ($RequiredKernel) {
        Add-L '-- THE ONE NUMBER THAT MATTERS ------------------------------------------------'
        Add-L ''
        Add-L "  This machine needs Linux kernel $RequiredKernel or newer."
        Add-L ''
        Add-L '  A distribution older than that will not merely run slower - the specific'
        Add-L '  hardware listed above will not work at all. This is the mistake that sends'
        Add-L '  people back to Windows convinced Linux is broken.'
        Add-L ''
        if ($Rec.Excluded.Count -gt 0) {
            Add-L '  RULED OUT for shipping an older kernel by default:'
            foreach ($x in $Rec.Excluded) { Add-L ("    x {0}  (kernel {1})" -f $x.Name, $x.Kernel) }
            Add-L ''
            Add-L '  Those are popular, and they are the ones a first-time user is most'
            Add-L '  likely to be pointed at. On this machine they are the wrong answer.'
            Add-L ''
        }
    }

    if ($Verdict.Level -ne 'RED') {
        Add-L '-- RECOMMENDED ----------------------------------------------------------------'
        Add-L ''
        if ($Rec.Distros.Count -eq 0) {
            Add-L '  No distribution in our table ships a new enough kernel by default.'
            Add-L '  Use a rolling-release distribution, or Fedora, and expect to be on'
            Add-L '  recent-hardware territory.'
        } else {
            foreach ($d in $Rec.Distros) {
                Add-L ("  * {0}  (kernel {1})" -f $d.Name, $d.Kernel)
                Format-UpgWrap $d.Note -Indent '      ' | ForEach-Object { Add-L $_ }
                Add-L ''
            }
        }
        if ($Rec.HasNvidia) {
            Add-L '  NVIDIA present - only distributions that install the proprietary driver'
            Add-L '  for you are listed above.'
            Add-L ''
        }

        Add-L '-- BEFORE YOU DO ANYTHING -----------------------------------------------------'
        Add-L ''
        Add-L '   1. Save your BitLocker recovery key somewhere that is not this computer'
        Add-L '      - a phone note, another machine. An encrypted disk touched without it'
        Add-L '      is gone for good.'
        Add-L '   2. Get a USB stick big enough for your files - the converter puts them'
        Add-L '      there. You do NOT need an external hard drive. (Or keep Windows itself'
        Add-L '      as the fallback: see "Room to keep Windows" above.)'
        Add-L '   3. Write the ISO to a USB stick and boot it WITHOUT installing.'
        Add-L '      Live mode runs the whole desktop from the USB and changes nothing.'
        Add-L '   4. In that live session, test: Wi-Fi, sound through the SPEAKERS (not'
        Add-L '      just headphones), screen brightness, and suspend/resume.'
        Add-L '   5. Only then decide. Nothing is irreversible until you commit.'
        Add-L ''
    }

    $age = Get-UpgDistroTableAge
    if ($age -gt 120) {
        Add-L "  ! The distribution table in this scanner was last verified $age days ago."
        Add-L '    Kernel versions move; confirm against the distribution release notes.'
        Add-L ''
    }

    if ($script:Unmatched.Count -gt 0) {
        Add-L '-- HELP THE PROJECT -----------------------------------------------------------'
        Add-L ''
        Add-L '  These devices are not individually catalogued yet, so the advice above'
        Add-L '  fell back to a general rule for the vendor. Reporting them - and what'
        Add-L '  actually happened when you installed - makes the next report exact:'
        Add-L ''
        foreach ($u in ($script:Unmatched | Sort-Object -Unique)) { Add-L "    $u" }
        Add-L ''
    }

    Add-L '==============================================================================='
    Add-L '  This scanner made no changes to this computer and sent nothing anywhere.'
    Add-L '==============================================================================='

    $L
}

# =============================================================================
#  self-test
# =============================================================================
#  Exercises the verdict and recommendation logic against synthetic machines,
#  including the paths real hardware here cannot reach. Run this after editing
#  anything in data/.

function Invoke-UpgSelfTest {
    $cases = @(
        @{ Name = 'ARM laptop is an outright no'
           Checks = @(
             @{ Section='Fundamentals'; Title='CPU architecture'; Status='fail'; Detail='Snapdragon X (ARM64)' })
           Expect = 'RED'; ExpectKernel = $null }

        @{ Name = 'Intel VMD blocks the install'
           Checks = @(
             @{ Section='Storage'; Title='Storage controller mode'; Status='fail'; Detail='Intel RST / VMD active' })
           Expect = 'RED'; ExpectKernel = $null }

        @{ Name = 'Adobe is a software problem, not a hardware one'
           Checks = @(
             @{ Section='Hardware'; Title='Wi-Fi';    Status='ok';   Detail='Intel AX200' },
             @{ Section='Software'; Title='No Linux equivalent'; Status='fail'; Detail='Adobe Photoshop' })
           Expect = 'YELLOW'; ExpectKernel = $null }

        @{ Name = 'Broadcom Wi-Fi is a hardware blocker'
           Checks = @(
             @{ Section='Hardware'; Title='Wi-Fi'; Status='fail'; Detail='Broadcom BCM43142' })
           Expect = 'RED'; ExpectKernel = $null }

        @{ Name = 'Old clean ThinkPad passes'
           Checks = @(
             @{ Section='Fundamentals'; Title='CPU architecture'; Status='ok'; Detail='i5-6300U' },
             @{ Section='Hardware';     Title='Wi-Fi';            Status='ok'; Detail='Intel 8260' },
             @{ Section='Storage';      Title='Free space';       Status='ok'; Detail='200 GB free' })
           Expect = 'GREEN'; ExpectKernel = $null }

        @{ Name = 'Kernel requirement is the max across all checks'
           Checks = @(
             @{ Section='Hardware'; Title='Wi-Fi';    Status='warn'; Detail='MT7925'; MinKernel='6.7' },
             @{ Section='Hardware'; Title='Graphics'; Status='warn'; Detail='890M';   MinKernel='6.10' },
             @{ Section='Hardware'; Title='Audio';    Status='warn'; Detail='CS35L56';MinKernel='6.7' })
           Expect = 'YELLOW'; ExpectKernel = '6.10' }
    )

    $failed = 0
    Write-Host ''
    Write-Host '  upgrade_ self-test' -ForegroundColor Cyan
    Write-Host ''

    foreach ($case in $cases) {
        $script:Checks = @()
        foreach ($c in $case.Checks) {
            New-UpgCheck -Section $c.Section -Title $c.Title -Status $c.Status `
                         -Detail $c.Detail -MinKernel $c.MinKernel
        }

        $errors = @()
        $verdict = Get-UpgVerdict
        if ($verdict.Level -ne $case.Expect) {
            $errors += "verdict was $($verdict.Level), expected $($case.Expect)"
        }

        $kernel = Get-UpgRequiredKernel
        $kernelText = if ($kernel) { $kernel.ToString() } else { $null }
        if ($case.ExpectKernel) {
            # [version]'6.10' stringifies as '6.10'; compare as versions to be safe.
            if (-not $kernel -or $kernel -ne [version]$case.ExpectKernel) {
                $errors += "kernel was '$kernelText', expected '$($case.ExpectKernel)'"
            }
        }

        # A RED verdict must never recommend a distribution.
        if ($verdict.Level -eq 'RED') {
            $rec = Get-UpgRecommendation -RequiredKernel $kernel
            if ($verdict.Groups.Count -eq 0) { $errors += 'RED verdict listed no reasons' }
        }

        if ($errors.Count -eq 0) {
            Write-Host ("    PASS  " + $case.Name) -ForegroundColor Green
        } else {
            $failed++
            Write-Host ("    FAIL  " + $case.Name) -ForegroundColor Red
            foreach ($e in $errors) { Write-Host "          $e" -ForegroundColor Red }
        }
    }

    # Detection-level cases for the storage-mode check (R1/V5): fabricated PnP
    # entries through the real Test-UpgStorageMode. Still synthetic - the
    # real-RST-hardware confirmation these cannot provide is tracked as V5.
    $storageCases = @(
        @{ Name = 'storage: VMD PCI ID fires (Tiger Lake 9a0b)'
           Pnp = @([pscustomobject]@{ Name='Intel RST VMD Controller 9A0B'
                    DeviceID='PCI\VEN_8086&DEV_9A0B&SUBSYS_00000000&REV_00\3&0&0A'
                    PNPClass='SCSIAdapter'; Service='iaStorVD'
                    CompatibleID=@('PCI\CC_010400','PCI\CC_0104') })
           Expect = 'fail' }
        @{ Name = 'storage: VMD managed child fires (09ab)'
           Pnp = @([pscustomobject]@{ Name='Intel RST VMD Managed Controller 09AB'
                    DeviceID='PCI\VEN_8086&DEV_09AB&SUBSYS_00000000&REV_00\3&0&0B'
                    PNPClass='System'; Service=$null; CompatibleID=$null })
           Expect = 'fail' }
        @{ Name = 'storage: unknown VMD ID still fires via iaStorVD service'
           Pnp = @([pscustomobject]@{ Name='Intel RST VMD Controller FFFF'
                    DeviceID='PCI\VEN_8086&DEV_FFFF&SUBSYS_00000000&REV_00\3&0&0C'
                    PNPClass='SCSIAdapter'; Service='iaStorVD'
                    CompatibleID=@('PCI\CC_010400') })
           Expect = 'fail' }
        @{ Name = 'storage: pre-VMD RST RAID class fires via CC_0104 (iaStorAVC era)'
           Pnp = @([pscustomobject]@{ Name='Intel Chipset SATA/PCIe RST Premium Controller'
                    DeviceID='PCI\VEN_8086&DEV_282A&SUBSYS_00000000&REV_00\3&0&0D'
                    PNPClass='HDC'; Service='iaStorAVC'
                    CompatibleID=@('PCI\VEN_8086&CC_010400','PCI\CC_010400','PCI\CC_0104') })
           Expect = 'fail' }
        @{ Name = 'storage: RST driver on AHCI-class controller warns, not fails'
           Pnp = @([pscustomobject]@{ Name='Intel 300 Series Chipset Family SATA AHCI Controller'
                    DeviceID='PCI\VEN_8086&DEV_A103&SUBSYS_00000000&REV_00\3&0&0E'
                    PNPClass='HDC'; Service='iaStorA'
                    CompatibleID=@('PCI\VEN_8086&CC_010601','PCI\CC_010601','PCI\CC_0106') })
           Expect = 'warn' }
        @{ Name = 'storage: standard NVMe passes (the G16 as seen 2026-08-22)'
           Pnp = @([pscustomobject]@{ Name='Standard NVM Express Controller'
                    DeviceID='PCI\VEN_1344&DEV_5413&SUBSYS_21001344&REV_03\4&23CF4B8A&0&0011'
                    PNPClass='SCSIAdapter'; Service='stornvme'
                    CompatibleID=@('PCI\VEN_1344&CC_010802','PCI\CC_010802','PCI\CC_0108') })
           Expect = 'ok' }
    )

    foreach ($case in $storageCases) {
        $script:Checks = @()
        Test-UpgStorageMode -Pnp $case.Pnp
        $check = $script:Checks | Where-Object { $_.Title -eq 'Storage controller mode' } |
                 Select-Object -First 1
        $status = if ($check) { $check.Status } else { '(no check emitted)' }
        if ($status -eq $case.Expect) {
            Write-Host ("    PASS  " + $case.Name) -ForegroundColor Green
        } else {
            $failed++
            Write-Host ("    FAIL  " + $case.Name) -ForegroundColor Red
            Write-Host ("          status was '$status', expected '$($case.Expect)'") -ForegroundColor Red
        }
    }

    # Judgment-level cases for the checks that read the live OS. Their
    # collection halves (registry, Get-Disk, Get-BitLockerVolume) sit behind
    # seams; these cases feed the judgment halves fabricated inputs, including
    # the paths a healthy dev machine can never show. Expect maps check Title
    # to expected status(es); an empty array asserts the check must NOT fire.
    $gb = 1073741824
    $uefiSys = [pscustomobject]@{ Firmware = 'UEFI' }
    $seamCases = @(
        @{ Name = 'seam: Secure Boot enabled is ok with distro note'
           Run = { Test-UpgFirmware -Sys $uefiSys -SecureBoot 1 }
           Expect = @{ 'Secure Boot' = 'ok' } }
        @{ Name = 'seam: Secure Boot disabled is ok'
           Run = { Test-UpgFirmware -Sys $uefiSys -SecureBoot 0 }
           Expect = @{ 'Secure Boot' = 'ok' } }
        @{ Name = 'seam: Secure Boot unreadable is info, not a guess'
           Run = { Test-UpgFirmware -Sys $uefiSys -SecureBoot $null }
           Expect = @{ 'Secure Boot' = 'info' } }

        @{ Name = 'seam: disk with shrink headroom keeps Windows'
           Run = { Test-UpgDisk -IsAdmin $true -Facts ([pscustomobject]@{
                     Disks = @([pscustomobject]@{ Number=0; FriendlyName='Test NVMe'; Size=(1000*$gb); PartitionStyle='GPT'; BusType='NVMe' })
                     SysVolume = [pscustomobject]@{ Size=(1000*$gb); SizeRemaining=(500*$gb) }
                     ShrinkGB = 120.0; Disk0PartCount = 4 }) }
           Expect = @{ 'Room to keep Windows' = 'ok'; 'Disk in use' = 'info'; 'Partition table' = @() } }
        @{ Name = 'seam: too little shrink room warns toward clean slate'
           Run = { Test-UpgDisk -IsAdmin $true -Facts ([pscustomobject]@{
                     Disks = @([pscustomobject]@{ Number=0; FriendlyName='Test NVMe'; Size=(256*$gb); PartitionStyle='GPT'; BusType='NVMe' })
                     SysVolume = [pscustomobject]@{ Size=(256*$gb); SizeRemaining=(20*$gb) }
                     ShrinkGB = 10.0; Disk0PartCount = 3 }) }
           Expect = @{ 'Room to keep Windows' = 'warn' } }
        @{ Name = 'seam: unelevated shrink query says re-run as Administrator'
           Run = { Test-UpgDisk -IsAdmin $false -Facts ([pscustomobject]@{
                     Disks = @([pscustomobject]@{ Number=0; FriendlyName='Test NVMe'; Size=(500*$gb); PartitionStyle='GPT'; BusType='NVMe' })
                     SysVolume = [pscustomobject]@{ Size=(500*$gb); SizeRemaining=(200*$gb) }
                     ShrinkGB = $null; Disk0PartCount = 4 }) }
           Expect = @{ 'Room to keep Windows' = 'info' } }
        @{ Name = 'seam: elevated but unmeasurable shrink is info, not fail'
           Run = { Test-UpgDisk -IsAdmin $true -Facts ([pscustomobject]@{
                     Disks = @([pscustomobject]@{ Number=0; FriendlyName='Test NVMe'; Size=(500*$gb); PartitionStyle='GPT'; BusType='NVMe' })
                     SysVolume = [pscustomobject]@{ Size=(500*$gb); SizeRemaining=(200*$gb) }
                     ShrinkGB = $null; Disk0PartCount = 4 }) }
           Expect = @{ 'Room to keep Windows' = 'info' } }
        @{ Name = 'seam: unreadable C: is unknown and stops there'
           Run = { Test-UpgDisk -IsAdmin $true -Facts ([pscustomobject]@{
                     Disks = @(); SysVolume = $null; ShrinkGB = $null; Disk0PartCount = 0 }) }
           Expect = @{ 'Disk space' = 'unknown'; 'Room to keep Windows' = @() } }
        @{ Name = 'seam: MBR at the four-partition limit warns'
           Run = { Test-UpgDisk -IsAdmin $true -Facts ([pscustomobject]@{
                     Disks = @([pscustomobject]@{ Number=0; FriendlyName='Old SATA'; Size=(500*$gb); PartitionStyle='MBR'; BusType='SATA' })
                     SysVolume = [pscustomobject]@{ Size=(500*$gb); SizeRemaining=(100*$gb) }
                     ShrinkGB = 60.0; Disk0PartCount = 4 }) }
           Expect = @{ 'Partition table' = 'warn' } }

        @{ Name = 'seam: Fast Startup enabled warns'
           Run = { Test-UpgFastStartup -HiberbootEnabled 1 }
           Expect = @{ 'Fast Startup' = 'warn' } }
        @{ Name = 'seam: Fast Startup disabled is ok'
           Run = { Test-UpgFastStartup -HiberbootEnabled 0 }
           Expect = @{ 'Fast Startup' = 'ok' } }
        @{ Name = 'seam: Fast Startup unreadable stays silent'
           Run = { Test-UpgFastStartup -HiberbootEnabled $null }
           Expect = @{ 'Fast Startup' = @() } }

        @{ Name = 'seam: BitLocker without Administrator is unknown'
           Run = { Test-UpgBitLocker -IsAdmin $false -State $null }
           Expect = @{ 'BitLocker' = 'unknown' } }
        @{ Name = 'seam: encrypted volume warns with the recovery-key remedy'
           Run = { Test-UpgBitLocker -IsAdmin $true -State ([pscustomobject]@{ Succeeded=$true; EncryptedMounts=@('C:') }) }
           Expect = @{ 'BitLocker' = 'warn' } }
        @{ Name = 'seam: no encrypted volumes is ok'
           Run = { Test-UpgBitLocker -IsAdmin $true -State ([pscustomobject]@{ Succeeded=$true; EncryptedMounts=@() }) }
           Expect = @{ 'BitLocker' = 'ok' } }
        @{ Name = 'seam: failed BitLocker query is unknown - possibly encrypted'
           Run = { Test-UpgBitLocker -IsAdmin $true -State ([pscustomobject]@{ Succeeded=$false; EncryptedMounts=@() }) }
           Expect = @{ 'BitLocker' = 'unknown' } }

        @{ Name = 'seam: ESP with room and the boot entry on it is ok'
           Run = { Test-UpgEsp -IsAdmin $true -Facts ([pscustomobject]@{ Succeeded=$true; FreeBytes=(70*1MB); TotalBytes=(100*1MB); HasWindowsBootFiles=$true; BootmgrPointsAtEsp=$true; BcdDevice='Z:' }) }
           Expect = @{ 'Boot partition (ESP)' = 'ok' } }
        @{ Name = 'seam: full ESP warns toward clean slate (the R21 gate)'
           Run = { Test-UpgEsp -IsAdmin $true -Facts ([pscustomobject]@{ Succeeded=$true; FreeBytes=(10*1MB); TotalBytes=(100*1MB); HasWindowsBootFiles=$true; BootmgrPointsAtEsp=$true; BcdDevice='Z:' }) }
           Expect = @{ 'Boot partition (ESP)' = 'warn' } }
        @{ Name = 'seam: boot entry on a different volume warns'
           Run = { Test-UpgEsp -IsAdmin $true -Facts ([pscustomobject]@{ Succeeded=$true; FreeBytes=(70*1MB); TotalBytes=(100*1MB); HasWindowsBootFiles=$true; BootmgrPointsAtEsp=$false; BcdDevice='\Device\HarddiskVolume7' }) }
           Expect = @{ 'Boot partition (ESP)' = 'warn' } }
        @{ Name = 'seam: unresolvable Windows boot entry is unknown, not a guess'
           Run = { Test-UpgEsp -IsAdmin $true -Facts ([pscustomobject]@{ Succeeded=$true; FreeBytes=(70*1MB); TotalBytes=(100*1MB); HasWindowsBootFiles=$true; BootmgrPointsAtEsp=$null; BcdDevice=$null }) }
           Expect = @{ 'Boot partition (ESP)' = 'unknown' } }
        @{ Name = 'seam: unelevated ESP check says re-run as Administrator'
           Run = { Test-UpgEsp -IsAdmin $false -Facts $null }
           Expect = @{ 'Boot partition (ESP)' = 'info' } }
        @{ Name = 'seam: failed ESP query is unknown - promise nothing'
           Run = { Test-UpgEsp -IsAdmin $true -Facts ([pscustomobject]@{ Succeeded=$false; FreeBytes=$null; TotalBytes=$null; HasWindowsBootFiles=$null; BootmgrPointsAtEsp=$null; BcdDevice=$null }) }
           Expect = @{ 'Boot partition (ESP)' = 'unknown' } }

        @{ Name = 'seam: F4 regression - real VS Community install must match'
           Run = { Test-UpgApps -Apps @('Microsoft Visual Studio Community 2022') }
           Expect = @{ 'Works differently' = 'warn' } }
        @{ Name = 'seam: VS Code must NOT trip the Visual Studio rule'
           Run = { Test-UpgApps -Apps @('Visual Studio Code') }
           Expect = @{ 'Works differently' = @(); 'Programs installed' = 'info' } }
        @{ Name = 'seam: Adobe is a blocker, Steam is info'
           Run = { Test-UpgApps -Apps @('Adobe Photoshop 2024', 'Steam') }
           Expect = @{ 'No Linux equivalent' = 'fail'; 'Worth knowing' = 'info' } }
        @{ Name = 'seam: empty app enumeration is unknown'
           Run = { Test-UpgApps -Apps @() }
           Expect = @{ 'Installed software' = 'unknown' } }

        @{ Name = 'seam: Windows 10 build gets the end-of-support note'
           Run = { Test-UpgWin11Context -IsAdmin $true -Sys ([pscustomobject]@{ OsCaption='Microsoft Windows 10 Home'; OsBuild=19045 }) }
           Expect = @{ 'Current OS' = 'info' } }
    )

    foreach ($case in $seamCases) {
        $script:Checks = @(); $script:Unmatched = @()
        & $case.Run
        $caseErrors = @()
        foreach ($k in $case.Expect.Keys) {
            $want = @($case.Expect[$k] | ForEach-Object { "$_" }) | Sort-Object
            $got  = @($script:Checks | Where-Object { $_.Title -eq $k } |
                      ForEach-Object { $_.Status }) | Sort-Object
            if (($want -join ',') -ne ($got -join ',')) {
                $caseErrors += "'$k' was [$($got -join ',')], expected [$($want -join ',')]"
            }
        }
        if ($caseErrors.Count -eq 0) {
            Write-Host ("    PASS  " + $case.Name) -ForegroundColor Green
        } else {
            $failed++
            Write-Host ("    FAIL  " + $case.Name) -ForegroundColor Red
            foreach ($e in $caseErrors) { Write-Host "          $e" -ForegroundColor Red }
        }
    }

    # Corpus replay: every file in corpus/ is a capture from a real machine
    # (made with -DumpMachine), replayed through the pure detection checks -
    # the ones that are functions of the enumeration alone. Checks that call
    # the live OS mid-check (Firmware/SecureBoot, Disk, FastStartup,
    # BitLocker, Apps, Win11Context) are not replayable from a capture and are
    # deliberately absent here. A recording of a real machine is evidence; a
    # green replay proves the current code still reads that machine correctly.
    # The single-file dist build ships without corpus/ - skipping is normal.
    $corpusDir = Join-Path $PSScriptRoot 'corpus'
    if (Test-Path $corpusDir) {
        foreach ($f in @(Get-ChildItem $corpusDir -Filter '*.json' | Sort-Object Name)) {
            $cap = $null
            try { $cap = Get-Content $f.FullName -Raw | ConvertFrom-Json }
            catch {
                $failed++
                Write-Host ("    FAIL  corpus: $($f.Name) is not valid JSON") -ForegroundColor Red
                continue
            }
            $script:Checks = @(); $script:Unmatched = @()
            $pnp = @($cap.Pnp)
            Test-UpgArchitecture -Sys $cap.Sys
            Test-UpgMemory       -Sys $cap.Sys
            Test-UpgStorageMode  -Pnp $pnp
            Test-UpgWifi         -Pnp $pnp
            Test-UpgGpu          -Pnp $pnp
            Test-UpgAudio        -Pnp $pnp
            Test-UpgVendor       -Sys $cap.Sys

            $caseErrors = @()
            $expected = @($cap.Expected.PSObject.Properties)
            if ($expected.Count -eq 0) {
                $caseErrors += 'capture has an empty Expected block - curate it before it lands in corpus/'
            }
            foreach ($prop in $expected) {
                $want = @($prop.Value | ForEach-Object { "$_" }) | Sort-Object
                $got  = @($script:Checks | Where-Object { $_.Title -eq $prop.Name } |
                          ForEach-Object { $_.Status }) | Sort-Object
                if (($want -join ',') -ne ($got -join ',')) {
                    $caseErrors += "'$($prop.Name)' was [$($got -join ',')], expected [$($want -join ',')]"
                }
            }
            $label = "corpus: $($cap.Label) ($($f.Name))"
            if ($caseErrors.Count -eq 0) {
                Write-Host ("    PASS  " + $label) -ForegroundColor Green
            } else {
                $failed++
                Write-Host ("    FAIL  " + $label) -ForegroundColor Red
                foreach ($e in $caseErrors) { Write-Host "          $e" -ForegroundColor Red }
            }
        }
    }

    # The distro table must stay internally coherent.
    foreach ($d in Get-UpgDistroTable) {
        if ($d.Kernel -ne 'rolling' -and -not (ConvertTo-UpgVersion $d.Kernel)) {
            $failed++
            Write-Host ("    FAIL  distro table: $($d.Name) has unparseable kernel '$($d.Kernel)'") -ForegroundColor Red
        }
    }

    Write-Host ''
    if ($failed -eq 0) {
        Write-Host '  all checks passed' -ForegroundColor Green
        Write-Host ''
        exit 0
    }
    Write-Host "  $failed failed" -ForegroundColor Red
    Write-Host ''
    exit 1
}

# =============================================================================
#  main
# =============================================================================

if ($SelfTest) { Invoke-UpgSelfTest }

if ($DumpMachine) {
    Write-Host ''
    Write-Host '  capturing hardware enumeration...' -ForegroundColor DarkGray
    Export-UpgMachineCapture -Path $DumpMachine -Sys (Get-UpgSystem) -Pnp (Get-UpgPnp)
    exit 0
}

$isAdmin = Test-UpgAdmin

Write-Host ''
Write-Host '  scanning...' -ForegroundColor DarkGray

$sys = Get-UpgSystem
$pnp = Get-UpgPnp

Test-UpgArchitecture -Sys $sys
Test-UpgMemory       -Sys $sys
Test-UpgFirmware     -Sys $sys -SecureBoot (Get-UpgSecureBootState)
Test-UpgStorageMode  -Pnp $pnp
Test-UpgDisk         -Facts (Get-UpgDiskFacts) -IsAdmin $isAdmin
Test-UpgFastStartup  -HiberbootEnabled (Get-UpgFastStartupState)
Test-UpgBitLocker    -IsAdmin $isAdmin -State $(if ($isAdmin) { Get-UpgBitLockerState })
Test-UpgEsp          -IsAdmin $isAdmin -Facts $(if ($isAdmin) { Get-UpgEspFacts })
Test-UpgWifi         -Pnp $pnp
Test-UpgGpu          -Pnp $pnp
Test-UpgAudio        -Pnp $pnp
Test-UpgVendor       -Sys $sys
Test-UpgApps
Test-UpgWin11Context -Sys $sys -IsAdmin $isAdmin

$requiredKernel = Get-UpgRequiredKernel
$verdict        = Get-UpgVerdict
$rec            = Get-UpgRecommendation -RequiredKernel $requiredKernel

$lines = Write-UpgReport -Sys $sys -Verdict $verdict -Rec $rec `
                         -RequiredKernel $requiredKernel -IsAdmin $isAdmin

Write-Host ''
foreach ($line in $lines) {
    $color = 'Gray'
    if     ($line -match '^\s*\[FAIL\]')   { $color = 'Red' }
    elseif ($line -match '^\s*\[WARN\]')   { $color = 'Yellow' }
    elseif ($line -match '^\s*\[ OK \]')   { $color = 'Green' }
    elseif ($line -match '^\s*\[ \?\? \]') { $color = 'Magenta' }
    elseif ($line -match '^={10,}')        { $color = 'DarkGray' }
    elseif ($line -match '^--\s[A-Z]')     { $color = 'Cyan' }
    elseif ($line -match 'VERDICT: RED')    { $color = 'Red' }
    elseif ($line -match 'VERDICT: YELLOW') { $color = 'Yellow' }
    elseif ($line -match 'VERDICT: GREEN')  { $color = 'Green' }
    elseif ($line -match '^\s{9}WHAT TO DO:') { $color = 'White' }
    Write-Host $line -ForegroundColor $color
}

if (-not $NoFile) {
    if (-not $OutDir) { $OutDir = [Environment]::GetFolderPath('Desktop') }
    if (-not (Test-Path $OutDir)) { New-Item -ItemType Directory -Path $OutDir -Force | Out-Null }

    $stamp = Get-Date -Format 'yyyyMMdd-HHmm'
    $safeModel = ($sys.Model -replace '[^A-Za-z0-9]+', '-').Trim('-')
    $txtPath = Join-Path $OutDir "upgrade-report-$safeModel-$stamp.txt"
    $lines | Out-File -FilePath $txtPath -Encoding UTF8

    Write-Host ''
    Write-Host "  Report saved: $txtPath" -ForegroundColor Cyan

    if ($Json) {
        $jsonPath = Join-Path $OutDir "upgrade-report-$safeModel-$stamp.json"
        [pscustomobject]@{
            ScannerVersion = $UpgVersion
            ScannedUtc     = (Get-Date).ToUniversalTime().ToString('o')
            System         = $sys
            RanAsAdmin     = $isAdmin
            RequiredKernel = if ($requiredKernel) { $requiredKernel.ToString() } else { $null }
            Verdict        = $verdict
            Recommended    = @($rec.Distros | ForEach-Object { $_.Name })
            Checks         = $script:Checks
            UnmatchedIds   = @($script:Unmatched | Sort-Object -Unique)
        } | ConvertTo-Json -Depth 6 | Out-File -FilePath $jsonPath -Encoding UTF8
        Write-Host "  JSON saved:   $jsonPath" -ForegroundColor Cyan
    }
}

Write-Host ''
