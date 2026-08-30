# Guest control for the Hyper-V rig - the QMP stand-in. Windows PowerShell 5.1.
#   vm.ps1 start | stop | kill | state
#   vm.ps1 shot [file.png]          thumbnail screenshot via WMI (default C:\upgrade-rig\hv\shots\<ts>.png)
#   vm.ps1 type "text"              type a string into the guest console (WMI Msvm_Keyboard)
#   vm.ps1 key <vk> [<vk>...]       press virtual-key codes (13=Enter, 27=Esc, 9=Tab, 32=Space, 37/38/39/40=arrows)
#   vm.ps1 press-any-key            spam Enter for 30 s (Windows Setup's "Press any key" window)
#   vm.ps1 fw                       Secure Boot state/template, boot order, TPM
#   vm.ps1 sb on|off [template]     toggle Secure Boot (VM must be off)
#   vm.ps1 dvd <iso>|none [n]       swap the n-th DVD's media
#   vm.ps1 disk add <vhdx> | list   attach a VHDX on SCSI (the 'stick' / OEMDRV stand-in)
#   vm.ps1 boot-first dvd|disk|net  Set-VMFirmware -FirstBootDevice
#   vm.ps1 ps "<command>"           run a command in the guest via PowerShell Direct (rig/rig)
#   vm.ps1 copy <host-file> <guest-path>   Copy-VMFile via Guest Service Interface
param(
    [Parameter(Position=0, Mandatory=$true)][string]$Action,
    [Parameter(Position=1, ValueFromRemainingArguments=$true)][string[]]$Rest,
    [string]$Name = 'UPGRIGHV'
)
$ErrorActionPreference = 'Stop'
$shots = 'C:\upgrade-rig\hv\shots'

function Get-VmWmi {
    Get-CimInstance -Namespace root\virtualization\v2 -ClassName Msvm_ComputerSystem -Filter "ElementName='$Name'"
}
function Get-Keyboard {
    $cs = Get-VmWmi
    if (-not $cs) { throw "VM '$Name' not found in WMI" }
    Get-CimAssociatedInstance -InputObject $cs -ResultClassName Msvm_Keyboard
}
function Save-Shot([string]$path) {
    Add-Type -AssemblyName System.Drawing
    $cs = Get-VmWmi
    $svc = Get-CimInstance -Namespace root\virtualization\v2 -ClassName Msvm_VirtualSystemManagementService
    $vssd = Get-CimAssociatedInstance -InputObject $cs -ResultClassName Msvm_VirtualSystemSettingData | Where-Object { $_.VirtualSystemType -eq 'Microsoft:Hyper-V:System:Realized' } | Select-Object -First 1
    $w = 1024; $h = 768
    $r = Invoke-CimMethod -InputObject $svc -MethodName GetVirtualSystemThumbnailImage -Arguments @{ TargetSystem = $vssd; WidthPixels = $w; HeightPixels = $h }
    if ($r.ReturnValue -ne 0) { throw "GetVirtualSystemThumbnailImage returned $($r.ReturnValue)" }
    $bytes = $r.ImageData
    $bmp = New-Object System.Drawing.Bitmap $w, $h, ([System.Drawing.Imaging.PixelFormat]::Format16bppRgb565)
    $rect = New-Object System.Drawing.Rectangle 0, 0, $w, $h
    $data = $bmp.LockBits($rect, [System.Drawing.Imaging.ImageLockMode]::WriteOnly, $bmp.PixelFormat)
    [System.Runtime.InteropServices.Marshal]::Copy($bytes, 0, $data.Scan0, $bytes.Length)
    $bmp.UnlockBits($data)
    $bmp.Save($path, [System.Drawing.Imaging.ImageFormat]::Png)
    $bmp.Dispose()
    Write-Host "shot: $path"
}

switch ($Action) {
    'start' { Start-VM -Name $Name; Write-Host "started $Name" }
    'stop'  { Stop-VM -Name $Name -Force; Write-Host "shutdown requested for $Name" }
    'kill'  { Stop-VM -Name $Name -TurnOff -Force; Write-Host "turned off $Name" }
    'state' { Get-VM -Name $Name | Select-Object Name, State, Uptime, Generation, Status | Format-List }
    'shot'  {
        New-Item -ItemType Directory -Force -Path $shots | Out-Null
        $p = if ($Rest) { $Rest[0] } else { Join-Path $shots ((Get-Date).ToString('yyyyMMdd-HHmmss') + '.png') }
        Save-Shot $p
    }
    'type'  { $kb = Get-Keyboard; Invoke-CimMethod -InputObject $kb -MethodName TypeText -Arguments @{ asciiText = ($Rest -join ' ') } | Out-Null }
    'key'   { $kb = Get-Keyboard; foreach ($k in $Rest) { Invoke-CimMethod -InputObject $kb -MethodName TypeKey -Arguments @{ keyCode = [uint32]$k } | Out-Null; Start-Sleep -Milliseconds 80 } }
    'press-any-key' {
        $kb = Get-Keyboard; $t = [DateTime]::Now
        while (([DateTime]::Now - $t).TotalSeconds -lt 30) { Invoke-CimMethod -InputObject $kb -MethodName TypeKey -Arguments @{ keyCode = [uint32]13 } | Out-Null; Start-Sleep -Milliseconds 250 }
        Write-Host "spammed Enter for 30 s"
    }
    'fw' {
        $fw = Get-VMFirmware -VMName $Name
        Write-Host ("SecureBoot={0} template={1} PreferredNetworkBootProtocol={2}" -f $fw.SecureBoot, $fw.SecureBootTemplate, $fw.PreferredNetworkBootProtocol)
        $i = 0; foreach ($b in $fw.BootOrder) { Write-Host ("  boot[{0}] {1} {2} {3}" -f $i, $b.BootType, $b.Device, $b.FirmwarePath); $i++ }
        $sec = Get-VMSecurity -VMName $Name
        Write-Host ("TPM={0} Shielded={1} EncryptStateAndVmMigrationTraffic={2}" -f $sec.TpmEnabled, $sec.Shielded, $sec.EncryptStateAndVmMigrationTraffic)
    }
    'sb' {
        $on = if ($Rest[0] -eq 'on') { 'On' } else { 'Off' }
        if ($Rest.Count -ge 2) { Set-VMFirmware -VMName $Name -EnableSecureBoot $on -SecureBootTemplate $Rest[1] } else { Set-VMFirmware -VMName $Name -EnableSecureBoot $on }
        Write-Host "SecureBoot -> $on"
    }
    'dvd' {
        $n = if ($Rest.Count -ge 2) { [int]$Rest[1] } else { 0 }
        $d = @(Get-VMDvdDrive -VMName $Name)[$n]
        if ($Rest[0] -eq 'none') { Set-VMDvdDrive -VMDvdDrive $d -Path $null } else { Set-VMDvdDrive -VMDvdDrive $d -Path $Rest[0] }
        Write-Host "dvd[$n] -> $($Rest[0])"
    }
    'disk' {
        if ($Rest[0] -eq 'add') { Add-VMHardDiskDrive -VMName $Name -ControllerType SCSI -Path $Rest[1]; Write-Host "attached $($Rest[1])" }
        else { Get-VMHardDiskDrive -VMName $Name | Select-Object ControllerType, ControllerNumber, ControllerLocation, Path | Format-Table -AutoSize }
    }
    'boot-first' {
        $dev = switch ($Rest[0]) {
            'dvd'  { Get-VMDvdDrive -VMName $Name | Select-Object -First 1 }
            'disk' { Get-VMHardDiskDrive -VMName $Name | Select-Object -First 1 }
            'net'  { Get-VMNetworkAdapter -VMName $Name | Select-Object -First 1 }
        }
        Set-VMFirmware -VMName $Name -FirstBootDevice $dev; Write-Host "first boot device -> $($Rest[0])"
    }
    'ps' {
        $sec = ConvertTo-SecureString 'rig' -AsPlainText -Force
        $cred = New-Object System.Management.Automation.PSCredential ('rig', $sec)
        $cmd = $Rest -join ' '
        Invoke-Command -VMName $Name -Credential $cred -ScriptBlock ([scriptblock]::Create($cmd))
    }
    'copy' { Copy-VMFile -Name $Name -SourcePath $Rest[0] -DestinationPath $Rest[1] -FileSource Host -CreateFullPath -Force; Write-Host "copied $($Rest[0]) -> guest $($Rest[1])" }
    default { throw "unknown action '$Action'" }
}
