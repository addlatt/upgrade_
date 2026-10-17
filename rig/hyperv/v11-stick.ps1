# V11 step 3 on the rig: boot a "Go back to Windows" stick image in a new
# Generation 2 VM (UEFI, Secure Boot on with Microsoft's Windows template)
# with an empty target disk, to see Windows Setup start and install from the
# split install.swm. Windows PowerShell 5.1, elevated.
#   v11-stick.ps1 -StickVhdx C:\upgrade-rig\hv\vm\v11-stick.vhdx
# Re-running replaces the VM and its target disk (never the stick image).
# The walk-away way back (V11 step 4, R33; 2026-10-17): -KeepTarget keeps a target
# made beforehand (its DiskIdentifier names the drive in the job, v11w-root.py),
# -Tpm gives Windows 11 its security chip, -Switch adds a network adapter left
# DISCONNECTED (Setup runs offline; connect it later for the SSH server's download).
param(
    [Parameter(Mandatory=$true)][string]$StickVhdx,
    [string]$Name = 'UPGRIGV11',
    [switch]$KeepTarget,
    [switch]$Tpm,
    [string]$Switch = ''
)
$ErrorActionPreference = 'Stop'
$root = 'C:\upgrade-rig\hv'
$target = "$root\vm\$Name-target.vhdx"
if (Get-VM -Name $Name -ErrorAction SilentlyContinue) {
    Stop-VM -Name $Name -TurnOff -Force -ErrorAction SilentlyContinue
    Remove-VM -Name $Name -Force
}
if (-not ($KeepTarget -and (Test-Path $target))) {
    if (Test-Path $target) { Remove-Item $target -Force }
    New-VHD -Path $target -SizeBytes 64GB -Dynamic | Out-Null
}
$vm = New-VM -Name $Name -Generation 2 -MemoryStartupBytes 4GB -NoVHD
Set-VMProcessor -VMName $Name -Count 2
Set-VMMemory -VMName $Name -DynamicMemoryEnabled $false
Set-VMFirmware -VMName $Name -EnableSecureBoot On -SecureBootTemplate MicrosoftWindows
if ($Tpm) { Set-VMKeyProtector -VMName $Name -NewLocalKeyProtector; Enable-VMTPM -VMName $Name }
Get-VMNetworkAdapter -VMName $Name | Remove-VMNetworkAdapter
if ($Switch) { Add-VMNetworkAdapter -VMName $Name -Name net; Write-Host "network adapter added, not connected (Connect-VMNetworkAdapter -VMName $Name -SwitchName '$Switch')" }
Add-VMHardDiskDrive -VMName $Name -ControllerType SCSI -Path $StickVhdx
Add-VMHardDiskDrive -VMName $Name -ControllerType SCSI -Path $target
$stick = Get-VMHardDiskDrive -VMName $Name | Where-Object { $_.Path -eq $StickVhdx }
Set-VMFirmware -VMName $Name -FirstBootDevice $stick
# no network: Setup runs offline, as it would for someone at home without a cable
Write-Host "created ${Name}: stick $StickVhdx first, target $target ($((Get-VHD $target).Size) B, $((Get-VHD $target).DiskIdentifier)), Secure Boot on (MicrosoftWindows), TPM $([bool]$Tpm)"
