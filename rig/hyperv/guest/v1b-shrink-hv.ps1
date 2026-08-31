# V1b step 2 (guest side, ELEVATED) - Hyper-V rig variant of rig/vm/guest/
# v1b-shrink.ps1: shrink C: to free space for the alongside install and record
# everything the gate needs from the Windows side. Windows PowerShell 5.1.
#
# Differences from the QEMU variant, both transport-only (no SMB share here):
#   - output lands LOCALLY in C:\upgrade-rig-guest\v1b\{shrink.json,shrink.txt};
#     the host reads it back over PowerShell Direct (vm.ps1 ps "Get-Content ...")
#   - v1b-mark.ps1 must already be at C:\upgrade_\rig\v1b-mark.ps1
#     (host: vm.ps1 copy rig/vm/guest/v1b-mark.ps1 C:\upgrade_\rig\v1b-mark.ps1)
#   - records the BitLocker state of C: (this guest encrypts; the QEMU one
#     could not) - how BitLocker and the kept Windows interact with the
#     alongside install is product-real evidence (R21/R19)
$ErrorActionPreference = 'Continue'
$out = 'C:\upgrade-rig-guest\v1b'
New-Item -ItemType Directory -Force -Path $out | Out-Null
$log = Join-Path $out 'shrink.txt'
Remove-Item $log -ErrorAction SilentlyContinue
function Log($m) { $m | Tee-Object -FilePath $log -Append }
$r = [ordered]@{}
$r.timestamp = (Get-Date).ToUniversalTime().ToString('o')
$r.elevated = ([Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
Log "=== V1b shrink (hv) $($r.timestamp) elevated=$($r.elevated) ==="
if (-not $r.elevated) { Log "!! not elevated - Resize-Partition and mountvol need admin"; exit 1 }
if (-not (Test-Path 'C:\upgrade_\rig\v1b-mark.ps1')) { Log "!! C:\upgrade_\rig\v1b-mark.ps1 missing - Copy-VMFile it first"; exit 1 }

# --- BitLocker state (product-real: the kept Windows is encrypted) -----------
$blv = Get-BitLockerVolume -MountPoint C: -ErrorAction SilentlyContinue
if ($blv) {
    $r.bitlocker_volume_status = "$($blv.VolumeStatus)"
    $r.bitlocker_protection = "$($blv.ProtectionStatus)"
    $r.bitlocker_method = "$($blv.EncryptionMethod)"
    $r.bitlocker_protectors = ($blv.KeyProtector | ForEach-Object { "$($_.KeyProtectorType)" }) -join ','
} else {
    $r.bitlocker_volume_status = 'none'
}
Log ("BitLocker C:  : {0} protection={1} method={2} protectors={3}" -f $r.bitlocker_volume_status, $r.bitlocker_protection, $r.bitlocker_method, $r.bitlocker_protectors)

# --- before ------------------------------------------------------------------
$disk = Get-Disk -Number 0
$p = Get-Partition -DriveLetter C
$v = Get-Volume -DriveLetter C
$sz = Get-PartitionSupportedSize -DriveLetter C
$r.disk_size = $disk.Size
$r.c_size_before = $p.Size
$r.c_free_before = $v.SizeRemaining
$r.c_size_min = $sz.SizeMin
$r.c_size_max = $sz.SizeMax
$r.shrinkable_cold = $p.Size - $sz.SizeMin
$r.hiberfil_bytes = if (Test-Path C:\hiberfil.sys) { (Get-Item C:\hiberfil.sys -Force).Length } else { 0 }
$r.pagefile_bytes = if (Test-Path C:\pagefile.sys) { (Get-Item C:\pagefile.sys -Force).Length } else { 0 }
Log ("disk 0        : {0:N0} B" -f $r.disk_size)
Log ("C: size       : {0:N0} B  free {1:N0} B" -f $r.c_size_before, $r.c_free_before)
Log ("SizeMin/Max   : {0:N0} / {1:N0} B" -f $r.c_size_min, $r.c_size_max)
Log ("shrinkable    : {0:N0} B ({1:N1} GiB) cold, no mitigations" -f $r.shrinkable_cold, ($r.shrinkable_cold / 1GB))
Log ("hiberfil/page : {0:N0} / {1:N0} B" -f $r.hiberfil_bytes, $r.pagefile_bytes)
Log "partitions before:"
Get-Partition -DiskNumber 0 | ForEach-Object { Log ("  p{0} {1,-40} off {2,14:N0} size {3,14:N0} {4}" -f $_.PartitionNumber, $_.GptType, $_.Offset, $_.Size, $_.DriveLetter) }

# --- shrink ------------------------------------------------------------------
$want = 32GB
$target = $p.Size - $want
$r.capped_by_immovable = $false
if ($target -lt $sz.SizeMin) { $target = $sz.SizeMin; $r.capped_by_immovable = $true }
$r.c_size_target = $target
Log ("target C: size: {0:N0} B (freeing {1:N1} GiB){2}" -f $target, (($p.Size - $target) / 1GB), $(if ($r.capped_by_immovable) { '  CAPPED by SizeMin' } else { '' }))
try {
    Resize-Partition -DriveLetter C -Size $target -ErrorAction Stop
    $r.resize_ok = $true
} catch {
    $r.resize_ok = $false; $r.resize_error = $_.Exception.Message
    Log "!! Resize-Partition failed: $($_.Exception.Message)"
}
$p2 = Get-Partition -DriveLetter C
$r.c_size_after = $p2.Size
$r.freed_bytes = $r.c_size_before - $p2.Size
$disk2 = Get-Disk -Number 0
$r.largest_free_extent = $disk2.LargestFreeExtent
Log ("C: size after : {0:N0} B  freed {1:N1} GiB; largest free extent {2:N0} B" -f $r.c_size_after, ($r.freed_bytes / 1GB), $r.largest_free_extent)
Log "partitions after:"
Get-Partition -DiskNumber 0 | ForEach-Object { Log ("  p{0} {1,-40} off {2,14:N0} size {3,14:N0} {4}" -f $_.PartitionNumber, $_.GptType, $_.Offset, $_.Size, $_.DriveLetter) }

# --- ESP as Windows sees it --------------------------------------------------
$esp = Get-Partition -DiskNumber 0 | Where-Object { $_.GptType -eq '{c12a7328-f81f-11d2-ba4b-00a0c93ec93b}' } | Select-Object -First 1
$r.esp_size = $esp.Size
cmd /c "mountvol S: /S" 2>$null | Out-Null
$di = New-Object IO.DriveInfo('S:')
$r.esp_total_fs = $di.TotalSize
$r.esp_free_fs = $di.AvailableFreeSpace
$r.bootmgfw_sha256 = (Get-FileHash -Algorithm SHA256 'S:\EFI\Microsoft\Boot\bootmgfw.efi').Hash.ToLower()
$r.esp_efi_dirs = (Get-ChildItem 'S:\EFI' -Force | Select-Object -Expand Name) -join ','
cmd /c "mountvol S: /D" 2>$null | Out-Null
Log ("ESP           : partition {0:N0} B, FS {1:N0} B, free {2:N0} B, EFI\ = {3}" -f $r.esp_size, $r.esp_total_fs, $r.esp_free_fs, $r.esp_efi_dirs)
Log ("bootmgfw.efi  : sha256 {0}" -f $r.bootmgfw_sha256)
$r.fwbootmgr = (bcdedit /enum '{fwbootmgr}' 2>&1 | Out-String)
Log "bcdedit {fwbootmgr}:"; Log $r.fwbootmgr
$r.secureboot = try { Confirm-SecureBootUEFI } catch { "unknown: $($_.Exception.Message)" }
Log ("SecureBoot    : {0}" -f $r.secureboot)

# --- boot-marker task --------------------------------------------------------
$act = New-ScheduledTaskAction -Execute 'powershell.exe' -Argument '-NoProfile -ExecutionPolicy Bypass -File C:\upgrade_\rig\v1b-mark.ps1'
$trg = New-ScheduledTaskTrigger -AtLogOn -User 'rig'
Register-ScheduledTask -TaskName 'v1b-mark' -Action $act -Trigger $trg -User 'rig' -RunLevel Highest -Force | Out-Null
$r.mark_task = [bool](Get-ScheduledTask -TaskName 'v1b-mark' -ErrorAction SilentlyContinue)
$oem = Get-Volume | Where-Object { $_.FileSystemLabel -eq 'OEMDRV' } | Select-Object -First 1
$r.oemdrv_letter = if ($oem) { "$($oem.DriveLetter)" } else { '' }
Log ("mark task     : {0}; OEMDRV volume letter: '{1}'" -f $r.mark_task, $r.oemdrv_letter)

$r | ConvertTo-Json -Depth 3 | Set-Content -Path (Join-Path $out 'shrink.json') -Encoding UTF8
Log "=== shrink done; output in $out; shutdown when ready ==="
