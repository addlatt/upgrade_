# Create the Gen 2 rig guest. Unelevated is fine once setup.ps1 has run.
#   powershell -ExecutionPolicy Bypass -File new-vm.ps1 [-Name UPGRIGHV] [-DiskGB 80] [-MemoryGB 8] [-Cpu 4]
#              [-SecureBootTemplate MicrosoftUEFICertificateAuthority|MicrosoftWindows] [-NoTpm]
# Attaches win10.iso + unattend-hv.iso and boots the DVD first; expect ONE
# keypress at "Press any key to boot from CD or DVD..." (vm.ps1 key 13 spammed
# across the window). Refuses to overwrite an existing VM of that name.
# Windows PowerShell 5.1.
param(
    [string]$Name = 'UPGRIGHV',
    [int]$DiskGB = 80,
    [int]$MemoryGB = 8,
    [int]$Cpu = 4,
    [ValidateSet('MicrosoftUEFICertificateAuthority','MicrosoftWindows')]
    [string]$SecureBootTemplate = 'MicrosoftUEFICertificateAuthority',
    [switch]$NoTpm,
    [string]$Switch = 'Default Switch'
)
$ErrorActionPreference = 'Stop'
$root = 'C:\upgrade-rig\hv'
$vhd  = "$root\vm\$Name.vhdx"
if (Get-VM -Name $Name -ErrorAction SilentlyContinue) { throw "VM '$Name' already exists. Remove-VM it (and delete $vhd) explicitly if you mean to reinstall." }
foreach ($iso in "$root\iso\win10.iso", "$root\iso\unattend-hv.iso") { if (-not (Test-Path $iso)) { throw "missing $iso" } }
if (Test-Path $vhd) { throw "$vhd exists - delete it explicitly if you mean to reinstall." }

$vm = New-VM -Name $Name -Generation 2 -MemoryStartupBytes ($MemoryGB * 1GB) -NewVHDPath $vhd -NewVHDSizeBytes ($DiskGB * 1GB) -SwitchName $Switch -Path "$root\vm"
Set-VM -VM $vm -ProcessorCount $Cpu -AutomaticCheckpointsEnabled $false -CheckpointType Standard -AutomaticStopAction ShutDown
Set-VMMemory -VM $vm -DynamicMemoryEnabled $false
# Two DVDs: install media + the autounattend volume (Setup scans every CD root)
Add-VMDvdDrive -VM $vm -Path "$root\iso\win10.iso"
Add-VMDvdDrive -VM $vm -Path "$root\iso\unattend-hv.iso"
$dvd = Get-VMDvdDrive -VM $vm | Select-Object -First 1
# Secure Boot ON from the first boot - the whole point of this leg. The
# template picks the db: MicrosoftWindows = Windows Production CA only;
# MicrosoftUEFICertificateAuthority = the Microsoft UEFI (third-party) CA that
# signs Fedora's shim. Which one also boots Windows is itself a V1b data point.
Set-VMFirmware -VM $vm -EnableSecureBoot On -SecureBootTemplate $SecureBootTemplate -FirstBootDevice $dvd
if (-not $NoTpm) {
    # vTPM needs a key protector; local mode is fine for a throwaway rig.
    Set-VMKeyProtector -VM $vm -NewLocalKeyProtector
    Enable-VMTPM -VM $vm
}
# Integration services: guest services (file copy) + PowerShell Direct come for free on Gen 2
Enable-VMIntegrationService -VM $vm -Name 'Guest Service Interface' -ErrorAction SilentlyContinue
$fw = Get-VMFirmware -VM $vm
Write-Host ("created {0}: gen2, {1} vCPU, {2} GB, disk {3}, SecureBoot={4} template={5}, TPM={6}" -f $Name, $Cpu, $MemoryGB, $vhd, $fw.SecureBoot, $fw.SecureBootTemplate, (-not $NoTpm))
Write-Host "next: .\vm.ps1 start; then .\vm.ps1 press-any-key  (spams Enter across Setup's 5 s window); watch with .\vm.ps1 shot"
