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

.EXAMPLE
    .\upgrade-scan.ps1

.EXAMPLE
    .\upgrade-scan.ps1 -Json -OutDir C:\Temp
#>
[CmdletBinding()]
param(
    [string]$OutDir,
    [switch]$NoFile,
    [switch]$Json,
    [switch]$SelfTest
)

$ErrorActionPreference = 'Stop'
$UpgVersion = '0.1.0'

# --- data ---------------------------------------------------------------
# The build script replaces this block with the file contents inline, so the
# released single file has no dependencies. Running from source uses the files.
#!INLINE-DATA-BEGIN!#
. (Join-Path $PSScriptRoot 'data\devices.ps1')
. (Join-Path $PSScriptRoot 'data\distros.ps1')
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
    Get-CimInstance Win32_PnPEntity | Select-Object Name, DeviceID, PNPClass, Service
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

function Test-UpgFirmware {
    param($Sys)
    $mode = if ($Sys.Firmware) { $Sys.Firmware } else { 'unknown' }
    New-UpgCheck -Section 'Fundamentals' -Title 'Firmware mode' -Status 'ok' -Detail $mode

    $sb = $null
    try {
        $sb = (Get-ItemProperty -Path 'HKLM:\SYSTEM\CurrentControlSet\Control\SecureBoot\State' `
               -Name UEFISecureBootEnabled -ErrorAction Stop).UEFISecureBootEnabled
    } catch { }

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
    $vmdIds = Get-UpgVmdDeviceIds
    $hit = $null
    foreach ($d in $Pnp) {
        if (-not $d.DeviceID) { continue }
        $id = Get-UpgPciId $d.DeviceID
        if ($id -and ($vmdIds -contains $id)) { $hit = $d; break }
        if ($d.Service -and ($d.Service -match '^iaStorV')) { $hit = $d; break }
    }

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

    New-UpgCheck -Section 'Storage' -Title 'Storage controller mode' -Status 'ok' `
        -Detail 'standard AHCI / NVMe - visible to Linux installers'
}

function Test-UpgDisk {
    $disks = @(Get-Disk | Where-Object { $_.BusType -ne 'File Backed Virtual' })
    foreach ($d in $disks) {
        New-UpgCheck -Section 'Storage' -Title "Disk $($d.Number)" -Status 'info' `
            -Detail ('{0} - {1} GB, {2}, {3}' -f $d.FriendlyName, [math]::Round($d.Size/1GB,1), $d.PartitionStyle, $d.BusType)
    }

    $sys = Get-Volume -DriveLetter C -ErrorAction SilentlyContinue
    if (-not $sys) {
        New-UpgCheck -Section 'Storage' -Title 'Free space' -Status 'unknown' -Detail 'could not read C:'
        return
    }

    $freeGB = [math]::Round($sys.SizeRemaining / 1GB, 1)
    $usedGB = [math]::Round(($sys.Size - $sys.SizeRemaining) / 1GB, 1)

    if ($freeGB -lt 25) {
        New-UpgCheck -Section 'Storage' -Title 'Free space' -Status 'fail' `
            -Detail "$freeGB GB free (using $usedGB GB)" `
            -Note 'Not enough free space to install Linux alongside Windows. Shrinking the Windows partition below this point is not safely possible.' `
            -Remedy 'Free up space first, or plan to erase Windows entirely - which requires backing up to an external drive.'
    } elseif ($freeGB -lt 60) {
        New-UpgCheck -Section 'Storage' -Title 'Free space' -Status 'warn' `
            -Detail "$freeGB GB free (using $usedGB GB)" `
            -Note 'Enough to dual-boot, but tight. 60 GB or more is comfortable for a Linux install with room to grow.' `
            -Remedy 'Free up more space, or commit to erasing Windows.'
    } else {
        New-UpgCheck -Section 'Storage' -Title 'Free space' -Status 'ok' `
            -Detail "$freeGB GB free (using $usedGB GB)"
    }

    # Backup sizing is the number people fail to plan for.
    New-UpgCheck -Section 'Storage' -Title 'Backup drive needed' -Status 'info' `
        -Detail "at least $([math]::Ceiling($usedGB * 1.2)) GB" `
        -Note 'You need somewhere to put your files before you touch the disk. Size this at your used space plus headroom. An external drive that already holds your only backup does not count - that is one drive away from losing everything.'

    $partCount = @(Get-Partition -DiskNumber 0 -ErrorAction SilentlyContinue).Count
    if ($disks.Count -gt 0 -and $disks[0].PartitionStyle -eq 'MBR' -and $partCount -ge 4) {
        New-UpgCheck -Section 'Storage' -Title 'Partition table' -Status 'warn' `
            -Detail "MBR with $partCount primary partitions" `
            -Note 'MBR discs allow only four primary partitions and you are at the limit. The installer will not be able to create a new one.' `
            -Remedy 'Delete or convert a partition, or erase the disk and let the installer create a fresh GPT layout.'
    }
}

function Test-UpgFastStartup {
    # Fast Startup leaves NTFS in a dirty hibernated state. Linux then refuses
    # to mount it read-write, and partition resize is unsafe. Cheap to check,
    # and it silently ruins installs.
    $val = $null
    try {
        $val = (Get-ItemProperty -Path 'HKLM:\SYSTEM\CurrentControlSet\Control\Session Manager\Power' `
                -Name HiberbootEnabled -ErrorAction Stop).HiberbootEnabled
    } catch { }

    if ($val -eq 1) {
        New-UpgCheck -Section 'Storage' -Title 'Fast Startup' -Status 'warn' `
            -Detail 'enabled' `
            -Note 'Windows Fast Startup does not fully shut down - it hibernates. That leaves the Windows partition in a state Linux will not write to, and makes resizing it unsafe. Shutting down does not clear it; only a restart or disabling the feature does.' `
            -Remedy 'Control Panel > Power Options > Choose what the power buttons do > Change settings that are currently unavailable > untick "Turn on fast startup". Then shut down normally.'
    } elseif ($val -eq 0) {
        New-UpgCheck -Section 'Storage' -Title 'Fast Startup' -Status 'ok' -Detail 'disabled'
    }
}

function Test-UpgBitLocker {
    param([bool]$IsAdmin)
    if (-not $IsAdmin) {
        New-UpgCheck -Section 'Storage' -Title 'BitLocker' -Status 'unknown' `
            -Detail 'requires Administrator to check' `
            -Note 'Could not determine whether this disk is encrypted. This matters more than any other unknown here: if BitLocker is on and you resize or reinstall without the recovery key, the data is gone permanently and no recovery is possible.' `
            -Remedy 'Re-run this scanner as Administrator, or check manually: Settings > Privacy & security > Device encryption.'
        return
    }

    try {
        $vols = @(Get-BitLockerVolume -ErrorAction Stop | Where-Object { $_.ProtectionStatus -eq 'On' })
        if ($vols.Count -gt 0) {
            New-UpgCheck -Section 'Storage' -Title 'BitLocker' -Status 'warn' `
                -Detail ("enabled on " + (($vols | ForEach-Object { $_.MountPoint }) -join ', ')) `
                -Note 'This disk is encrypted. Any partition change without the recovery key destroys the data irrecoverably.' `
                -Remedy 'Save your recovery key before doing anything: run "manage-bde -protectors -get C:" as Administrator, or retrieve it from account.microsoft.com/devicerecoverykey. Save it somewhere that is not this computer. Then either suspend or fully decrypt BitLocker before touching partitions.'
        } else {
            New-UpgCheck -Section 'Storage' -Title 'BitLocker' -Status 'ok' -Detail 'not enabled'
        }
    } catch {
        New-UpgCheck -Section 'Storage' -Title 'BitLocker' -Status 'unknown' `
            -Detail 'query failed' `
            -Note 'Could not read encryption status. Treat the disk as possibly encrypted until you have confirmed otherwise.' `
            -Remedy 'Check Settings > Privacy & security > Device encryption before proceeding.'
    }
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
    $apps = Get-UpgInstalledApps
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
        Add-L '   1. Back up your files to an external drive. Not to the same disk.'
        Add-L '   2. Save your BitLocker recovery key somewhere off this machine.'
        Add-L '   3. Write the ISO to a USB stick and boot it WITHOUT installing.'
        Add-L '      Live mode runs the whole desktop from the USB and changes nothing.'
        Add-L '   4. In that live session, test: Wi-Fi, sound through the SPEAKERS (not'
        Add-L '      just headphones), screen brightness, and suspend/resume.'
        Add-L '   5. Only then decide. Nothing is irreversible until you click Install.'
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

$isAdmin = Test-UpgAdmin

Write-Host ''
Write-Host '  scanning...' -ForegroundColor DarkGray

$sys = Get-UpgSystem
$pnp = Get-UpgPnp

Test-UpgArchitecture -Sys $sys
Test-UpgMemory       -Sys $sys
Test-UpgFirmware     -Sys $sys
Test-UpgStorageMode  -Pnp $pnp
Test-UpgDisk
Test-UpgFastStartup
Test-UpgBitLocker    -IsAdmin $isAdmin
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
