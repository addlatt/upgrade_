# V3 / R19 - Windows side (guest, PowerShell Direct, ELEVATED). Windows PowerShell 5.1.
# Puts a pristine (pre-BitLocker, pre-shrink) Windows disk into one V3 config in
# the PRODUCT's order: encrypt C: first (TPM + recovery-password protectors,
# the chosen method, used-space-only or full), wait until FullyEncrypted, THEN
# shrink C: by -ShrinkGiB (what the converter does to an already-encrypted
# volume). Prints RECOVERY_KEY= once - the host captures it to its key file;
# nothing else ever echoes it.
param(
    [ValidateSet('XtsAes128','XtsAes256','Aes128','Aes256')][string]$Method = 'XtsAes128',
    [switch]$UsedSpaceOnly,
    [int]$ShrinkGiB = 32,
    [switch]$NoShutdown
)
$ErrorActionPreference = 'Stop'
function Log($m) { Write-Host ('{0:HH:mm:ss} {1}' -f (Get-Date), $m) }
$tpm = Get-Tpm
Log ("TPM present={0} ready={1}" -f $tpm.TpmPresent, $tpm.TpmReady)
if ($tpm.TpmPresent -and -not $tpm.TpmReady) { Initialize-Tpm -AllowClear -AllowPhysicalPresence | Out-Null }
$v = Get-BitLockerVolume -MountPoint C:
Log ("before: status={0} method={1} protection={2}" -f $v.VolumeStatus, $v.EncryptionMethod, $v.ProtectionStatus)
if ($v.VolumeStatus -ne 'FullyDecrypted') { throw "C: is not FullyDecrypted - this script wants the pristine disk" }
$p = Get-Partition -DriveLetter C
Log ("before: partition {0:N0} B" -f $p.Size)

$args = @{ MountPoint = 'C:'; EncryptionMethod = $Method; TpmProtector = $true; SkipHardwareTest = $true }
if ($UsedSpaceOnly) { $args.UsedSpaceOnly = $true }
Log ("Enable-BitLocker {0} usedspaceonly={1}" -f $Method, [bool]$UsedSpaceOnly)
Enable-BitLocker @args | Out-Null
$rp = Add-BitLockerKeyProtector -MountPoint C: -RecoveryPasswordProtector
$key = ($rp.KeyProtector | Where-Object { $_.KeyProtectorType -eq 'RecoveryPassword' } | Select-Object -Last 1).RecoveryPassword
Write-Output ("RECOVERY_KEY= " + $key)
$t0 = Get-Date
while ($true) {
    $v = Get-BitLockerVolume -MountPoint C:
    if ($v.VolumeStatus -eq 'FullyEncrypted') { break }
    Log ("encrypting: {0} {1}%" -f $v.VolumeStatus, $v.EncryptionPercentage)
    Start-Sleep -Seconds 30
    if (((Get-Date) - $t0).TotalHours -gt 4) { throw "encryption did not finish in 4 h" }
}
$v = Get-BitLockerVolume -MountPoint C:
Log ("encrypted in {0:N0} s: status={1} method={2} protection={3} protectors={4}" -f ((Get-Date) - $t0).TotalSeconds, $v.VolumeStatus, $v.EncryptionMethod, $v.ProtectionStatus, (($v.KeyProtector | ForEach-Object { "$($_.KeyProtectorType)" }) -join '+'))
Log ((& manage-bde.exe -status C: | Select-String 'Conversion Status|Encryption Method|Percentage') -join ' | ')

# the converter's shrink, on the encrypted volume (Windows' own resize; BitLocker stays on)
$p = Get-Partition -DriveLetter C
$sup = Get-PartitionSupportedSize -DriveLetter C
$target = $p.Size - ($ShrinkGiB * 1GB)
Log ("shrink: size {0:N0} min {1:N0} target {2:N0}" -f $p.Size, $sup.SizeMin, $target)
if ($target -lt $sup.SizeMin) { throw ("cannot shrink by {0} GiB: minimum is {1:N0}" -f $ShrinkGiB, $sup.SizeMin) }
Resize-Partition -DriveLetter C -Size $target
$p = Get-Partition -DriveLetter C
$v = Get-BitLockerVolume -MountPoint C:
Log ("after shrink: partition {0:N0} B, BitLocker status={1} protection={2}" -f $p.Size, $v.VolumeStatus, $v.ProtectionStatus)
if (-not $NoShutdown) { Log "full shutdown in 5 s"; & shutdown.exe /s /f /t 5 }
