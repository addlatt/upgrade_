<#
.SYNOPSIS
    A real machine's evidence from Windows, for v2-verdict.py (V1b, RISKS R21).

.DESCRIPTION
    Run elevated, over SSH or by hand. Reads only: nothing on the internal drive
    is changed. Writes one new folder, <OutRoot>\<UTC>\ (give an OutRoot that is
    NOT on the drive under test: the second drive, or the stick's data partition):

      first-mib-system.raw   the system drive's first MiB (its partition table)
      esp.raw                the EFI System Partition, whole, read raw from the drive
      esp.txt                where it starts and how long it is
      partitions.txt         Get-Partition for the system drive
      bcd-firmware.txt       bcdedit /enum firmware
      boot-line.txt          the bench's windows-boot line for THIS boot (rig/hyperv/v2.sh),
                             with -Via saying how Windows was reached
      state.txt              build, Secure Boot, BitLocker, the volume's health
      SHA256SUMS

    rig/hyperv/physical/assemble-image.py turns first-mib + esp into the image
    the offline inspector (rig/vm/v1b-inspect.py) reads, unchanged.
    Windows PowerShell 5.1.
#>
param(
    [Parameter(Mandatory = $true)][string]$OutRoot,
    [ValidateSet('before-install', 'via-grub', 'direct')][string]$Via = 'before-install'
)
$ErrorActionPreference = 'Stop'
$t = (Get-Date).ToUniversalTime().ToString('yyyyMMddTHHmmssZ')
$o = Join-Path $OutRoot $t; New-Item -ItemType Directory -Force $o | Out-Null

$sysPart = Get-Partition -DriveLetter $env:SystemDrive.TrimEnd(':')
$disk = $sysPart | Get-Disk
$esp = Get-Partition -DiskNumber $disk.Number | Where-Object { $_.GptType -eq '{c12a7328-f81f-11d2-ba4b-00a0c93ec93b}' } | Select-Object -First 1
if (-not $esp) { throw "no EFI System Partition on disk $($disk.Number)" }
if ((Split-Path -Qualifier $o) -eq $env:SystemDrive) { Write-Warning "OutRoot is on the system drive: this writes to the drive under test" }

function Read-Raw([string]$Device, [long]$Offset, [long]$Length, [string]$OutFile) {
    $in = New-Object IO.FileStream($Device, [IO.FileMode]::Open, [IO.FileAccess]::Read, [IO.FileShare]::ReadWrite)
    try {
        $out = [IO.File]::Create($OutFile)
        try {
            [void]$in.Seek($Offset, [IO.SeekOrigin]::Begin)
            $buf = New-Object byte[] (1MB); $left = $Length
            while ($left -gt 0) {
                $want = [int][Math]::Min($buf.Length, $left)
                $n = $in.Read($buf, 0, $want)       # raw devices read in whole sectors: 1 MiB is a multiple
                if ($n -le 0) { throw "short read at $($Length - $left) of $Length" }
                $out.Write($buf, 0, $n); $left -= $n
            }
        } finally { $out.Close() }
    } finally { $in.Close() }
}
$dev = "\\.\PhysicalDrive$($disk.Number)"
Read-Raw $dev 0 1MB (Join-Path $o 'first-mib-system.raw')
Read-Raw $dev $esp.Offset $esp.Size (Join-Path $o 'esp.raw')
"$dev partition $($esp.PartitionNumber): $($esp.Size) bytes, starts at byte $($esp.Offset) (sector $($esp.Offset / 512))" | Set-Content (Join-Path $o 'esp.txt')
Get-Partition -DiskNumber $disk.Number | Format-Table PartitionNumber, DriveLetter, Offset, Size, Type, GptType -AutoSize | Out-String -Width 200 | Set-Content (Join-Path $o 'partitions.txt')
(bcdedit /enum firmware | Out-String) | Set-Content (Join-Path $o 'bcd-firmware.txt')

# the bench's windows-boot line (rig/hyperv/v2.sh "cycle windows")
$order = ((bcdedit /enum '{fwbootmgr}' | Select-String 'bootsequence|displayorder' | Select-Object -First 1) -replace '\s+', ' ')
$line = 'windows-boot,' + (Get-Date).ToUniversalTime().ToString('o') + ',' + $Via + ',BootCurrent=' + $order
$line | Set-Content (Join-Path $o 'boot-line.txt')

$os = Get-CimInstance Win32_OperatingSystem
$sb = try { Confirm-SecureBootUEFI } catch { "unknown: $_" }
$bl = try { (Get-BitLockerVolume -MountPoint $env:SystemDrive).ProtectionStatus } catch { 'unknown' }
@("$($os.Caption) $($os.BuildNumber)", "last boot $($os.LastBootUpTime.ToUniversalTime().ToString('o'))", "Secure Boot $sb", "BitLocker $bl",
  (Get-Volume -DriveLetter $env:SystemDrive.TrimEnd(':') | Format-List DriveLetter, FileSystem, HealthStatus, OperationalStatus, Size, SizeRemaining | Out-String)) | Set-Content (Join-Path $o 'state.txt')

Get-ChildItem $o -File | Where-Object Name -ne 'SHA256SUMS' | ForEach-Object { "$((Get-FileHash $_.FullName -Algorithm SHA256).Hash.ToLower())  $($_.Name)" } | Set-Content (Join-Path $o 'SHA256SUMS')
Write-Host "DONE $o"
