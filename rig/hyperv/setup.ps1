# One-shot HOST prep for the Hyper-V Gen 2 rig. Run ONCE from an ELEVATED
# Windows PowerShell (right-click > Run as administrator):
#   powershell -ExecutionPolicy Bypass -File \\wsl.localhost\Ubuntu\home\addlatt\upgrade_\rig\hyperv\setup.ps1
# Then sign out and back in (and `wsl --shutdown`) so the WSL shell's token
# carries the new group. Everything after this runs unelevated from WSL.
# Windows PowerShell 5.1. Idempotent.
$ErrorActionPreference = 'Stop'
$me = [Security.Principal.WindowsIdentity]::GetCurrent().Name
$isAdmin = ([Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
if (-not $isAdmin) { throw "setup.ps1 must run elevated (it adds $me to 'Hyper-V Administrators')." }

$feat = Get-WindowsOptionalFeature -Online -FeatureName Microsoft-Hyper-V-All
if ($feat.State -ne 'Enabled') { throw "Hyper-V is not enabled ($($feat.State)). Enable-WindowsOptionalFeature -Online -FeatureName Microsoft-Hyper-V-All, reboot, re-run." }
Write-Host "Hyper-V: enabled"

$user = $me.Split('\')[-1]
$members = (Get-LocalGroupMember -Group 'Hyper-V Administrators' -ErrorAction SilentlyContinue | ForEach-Object { $_.Name })
if ($members -notcontains $me) {
    Add-LocalGroupMember -Group 'Hyper-V Administrators' -Member $me
    Write-Host "added $me to 'Hyper-V Administrators' - sign out/in for it to take effect" -ForegroundColor Yellow
} else { Write-Host "$me already in 'Hyper-V Administrators'" }

foreach ($d in 'C:\upgrade-rig\hv\iso', 'C:\upgrade-rig\hv\vm', 'C:\upgrade-rig\hv\shots') {
    New-Item -ItemType Directory -Force -Path $d | Out-Null
}
Write-Host "folders: C:\upgrade-rig\hv\{iso,vm,shots}"
$sw = Get-VMSwitch -Name 'Default Switch' -ErrorAction SilentlyContinue
if ($sw) { Write-Host "switch: 'Default Switch' present (NAT, gives the guest internet)" } else { Write-Host "!! no 'Default Switch' - create a NAT switch before new-vm.ps1" -ForegroundColor Yellow }
Write-Host "setup: done."
