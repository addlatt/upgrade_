$ErrorActionPreference = 'Stop'
Write-Host "=== TPM ===" -ForegroundColor Cyan
$tpm = Get-Tpm
Write-Host ("TpmPresent={0} TpmReady={1} TpmEnabled={2}" -f $tpm.TpmPresent,$tpm.TpmReady,$tpm.TpmEnabled)
if ($tpm.TpmPresent -and -not $tpm.TpmReady) { Write-Host "initializing TPM..."; Initialize-Tpm -AllowClear -AllowPhysicalPresence | Out-Null }

Write-Host "=== enabling BitLocker on C: (TPM + recovery password, used-space only) ===" -ForegroundColor Cyan
try {
  Enable-BitLocker -MountPoint C: -TpmProtector -UsedSpaceOnly -SkipHardwareTest | Out-Null
} catch { Write-Host ("Enable-BitLocker (TPM) note: " + $_.Exception.Message) -ForegroundColor Yellow }
$rp = Add-BitLockerKeyProtector -MountPoint C: -RecoveryPasswordProtector
$key = ($rp.KeyProtector | Where-Object { $_.KeyProtectorType -eq 'RecoveryPassword' } | Select-Object -Last 1).RecoveryPassword
Write-Host ("RECOVERY_KEY= " + $key) -ForegroundColor Yellow

$v = Get-BitLockerVolume -MountPoint C:
Write-Host ("Volume C: ProtectionStatus={0} VolumeStatus={1} EncryptionPercentage={2}" -f $v.ProtectionStatus,$v.VolumeStatus,$v.EncryptionPercentage) -ForegroundColor Green
Write-Host ("Protectors: " + (($v.KeyProtector | ForEach-Object { $_.KeyProtectorType }) -join ', '))
