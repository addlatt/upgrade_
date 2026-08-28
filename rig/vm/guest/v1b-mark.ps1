# V1b boot marker (Windows side). Registered by v1b-shrink.ps1 as a logon task
# running elevated; appends one row per Windows boot to boots.log on the
# OEMDRV volume, mirroring the Linux-side v1b-mark unit. Windows PowerShell 5.1.
$ErrorActionPreference = 'Continue'
$vol = Get-Volume | Where-Object { $_.FileSystemLabel -eq 'OEMDRV' } | Select-Object -First 1
if (-not $vol) { exit 0 }
if (-not $vol.DriveLetter) {
    $part = Get-Partition | Where-Object { $_.AccessPaths -contains $vol.Path } | Select-Object -First 1
    if ($part) { $part | Add-PartitionAccessPath -AssignDriveLetter | Out-Null }
    Start-Sleep -Seconds 2
    $vol = Get-Volume | Where-Object { $_.FileSystemLabel -eq 'OEMDRV' } | Select-Object -First 1
}
if (-not $vol.DriveLetter) { exit 0 }
$root = "$($vol.DriveLetter):\"

$sha = ''; $espFree = ''
cmd /c "mountvol S: /S" 2>$null | Out-Null
if (Test-Path 'S:\EFI\Microsoft\Boot\bootmgfw.efi') {
    $sha = (Get-FileHash -Algorithm SHA256 'S:\EFI\Microsoft\Boot\bootmgfw.efi').Hash.ToLower()
    $espFree = (New-Object IO.DriveInfo('S:')).AvailableFreeSpace
}
cmd /c "mountvol S: /D" 2>$null | Out-Null

# BootCurrent: which firmware Boot#### entry this session actually came from.
# If Windows was chainloaded from GRUB it is Fedora's entry, not Windows'.
$bootCurrent = ''
try {
    Add-Type -Namespace V1b -Name Fw -MemberDefinition @'
[DllImport("kernel32.dll", SetLastError=true, CharSet=CharSet.Unicode)]
public static extern uint GetFirmwareEnvironmentVariableW(string name, string guid, byte[] buf, uint size);
[DllImport("advapi32.dll", SetLastError=true)]
public static extern bool OpenProcessToken(IntPtr h, uint access, out IntPtr tok);
[DllImport("advapi32.dll", SetLastError=true, CharSet=CharSet.Unicode)]
public static extern bool LookupPrivilegeValueW(string sys, string name, out long luid);
[StructLayout(LayoutKind.Sequential)] public struct TP { public uint Count; public uint LuidLow; public int LuidHigh; public uint Attr; }
[DllImport("advapi32.dll", SetLastError=true)]
public static extern bool AdjustTokenPrivileges(IntPtr tok, bool dis, ref TP np, uint len, IntPtr prev, IntPtr ret);
'@
    $tok = [IntPtr]::Zero
    [V1b.Fw]::OpenProcessToken([Diagnostics.Process]::GetCurrentProcess().Handle, 0x28, [ref]$tok) | Out-Null
    $luid = 0L
    [V1b.Fw]::LookupPrivilegeValueW($null, 'SeSystemEnvironmentPrivilege', [ref]$luid) | Out-Null
    # TOKEN_PRIVILEGES is packed 4+8+4 = 16 bytes with the LUID at offset 4; a
    # 'long' field would be aligned to offset 8 and the call fails with 1314.
    $tp = New-Object V1b.Fw+TP; $tp.Count = 1
    $tp.LuidLow = [uint32]($luid -band 0xFFFFFFFF); $tp.LuidHigh = [int32]($luid -shr 32); $tp.Attr = 2
    $adj = [V1b.Fw]::AdjustTokenPrivileges($tok, $false, [ref]$tp, 16, [IntPtr]::Zero, [IntPtr]::Zero)
    $adjErr = [Runtime.InteropServices.Marshal]::GetLastWin32Error()
    $buf = New-Object byte[] 2
    $n = [V1b.Fw]::GetFirmwareEnvironmentVariableW('BootCurrent', '{8BE4DF61-93CA-11D2-AA0D-00E098032B8C}', $buf, 2)
    if ($n -eq 2) { $bootCurrent = ('{0:X4}' -f [BitConverter]::ToUInt16($buf, 0)) } else { $bootCurrent = 'err' + [Runtime.InteropServices.Marshal]::GetLastWin32Error() + '/adj' + $adj + '-' + $adjErr }
} catch { $bootCurrent = 'exc' }

$build = (Get-ItemProperty 'HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion').CurrentBuild
$fw = (bcdedit /enum '{fwbootmgr}' 2>&1 | Out-String)
$fwCount = ([regex]::Matches($fw, '\{[0-9a-f-]{36}\}')).Count
$line = 'windows-boot,{0},build={1},BootCurrent={5},bootmgfw_sha256={2},esp_free={3},fwbootmgr_entries={4}' -f `
    (Get-Date).ToUniversalTime().ToString('yyyy-MM-ddTHH:mm:ssZ'), $build, $sha, $espFree, $fwCount, $bootCurrent
Add-Content -Path (Join-Path $root 'boots.log') -Value $line -Encoding ASCII
# raw firmware view from Windows' side, one file per boot
$fw | Set-Content -Path (Join-Path $root ('bcdedit-fw-' + (Get-Date).ToUniversalTime().ToString('yyyyMMddTHHmmssZ') + '.txt')) -Encoding ASCII
if (Test-Path (Join-Path $root 'autoshutdown')) { shutdown /s /t 10 }
