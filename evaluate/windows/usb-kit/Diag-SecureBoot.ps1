<#
.SYNOPSIS
    upgrade_ - read-only Secure Boot revocation diagnostic. Writes one text
    file to the stick. Changes nothing on the computer.

.DESCRIPTION
    Added 2026-10-03 after the Aspire's run 10: with Secure Boot on, the
    stick's shim started and then refused the stick's GRUB with
    "Verification failed: (0x1A) Security Violation". The suspected cause is
    a raised SBAT level (shim's revocation list, the UEFI variable SbatLevel)
    that the stick's GRUB (SBAT generation grub,3) no longer meets. This
    reads the facts that decide it:
      - SbatLevel and SbatLevelRT (shim's vendor GUID 605dab50-...), the
        revocation level the firmware holds now;
      - the dbx (Microsoft's list of revoked signatures): size and sha256;
      - the db: size and sha256;
      - Windows' Secure Boot servicing keys in the registry;
      - Windows' Secure Boot update events (TPM-WMI, Kernel-Boot), last 90 days;
      - the .sbat section of the stick's EFI\BOOT\grubx64.efi and BOOTX64.EFI.
    Reading a firmware variable needs SeSystemEnvironmentPrivilege, which an
    elevated administrator holds but must switch on; reading never writes.
#>
param([string]$OutFile)
$ErrorActionPreference = 'Continue'
$o = New-Object System.Collections.Generic.List[string]
function L([string]$s) { $o.Add($s) }
$root = Split-Path -Parent $MyInvocation.MyCommand.Path
if (-not $OutFile) {
    $dir = Join-Path $root 'upgrade_\report'
    New-Item -ItemType Directory -Path $dir -Force | Out-Null
    $OutFile = Join-Path $dir ('secureboot-diag-' + (Get-Date).ToUniversalTime().ToString('yyyyMMddTHHmmssZ') + '.txt')
}
L ('== upgrade_ Secure Boot diagnostic ' + (Get-Date).ToUniversalTime().ToString('o') + ' ' + $env:COMPUTERNAME)

Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public static class UpgFw {
    [StructLayout(LayoutKind.Sequential)] struct LUID { public uint Low; public int High; }
    [StructLayout(LayoutKind.Sequential)] struct TP { public int Count; public LUID Luid; public int Attr; }
    [DllImport("advapi32.dll", SetLastError = true)] static extern bool OpenProcessToken(IntPtr h, int access, out IntPtr tok);
    [DllImport("advapi32.dll", SetLastError = true, CharSet = CharSet.Unicode)] static extern bool LookupPrivilegeValue(string sys, string name, out LUID luid);
    [DllImport("advapi32.dll", SetLastError = true)] static extern bool AdjustTokenPrivileges(IntPtr tok, bool all, ref TP tp, int len, IntPtr prev, IntPtr rlen);
    [DllImport("kernel32.dll")] static extern IntPtr GetCurrentProcess();
    [DllImport("kernel32.dll", SetLastError = true, CharSet = CharSet.Unicode)] static extern uint GetFirmwareEnvironmentVariableExW(string name, string guid, byte[] buf, uint size, out uint attrs);
    public static string Enable() {
        IntPtr tok; LUID l;
        if (!OpenProcessToken(GetCurrentProcess(), 0x28, out tok)) return "OpenProcessToken " + Marshal.GetLastWin32Error();
        if (!LookupPrivilegeValue(null, "SeSystemEnvironmentPrivilege", out l)) return "LookupPrivilegeValue " + Marshal.GetLastWin32Error();
        TP tp = new TP(); tp.Count = 1; tp.Luid = l; tp.Attr = 2;
        AdjustTokenPrivileges(tok, false, ref tp, 0, IntPtr.Zero, IntPtr.Zero);
        int e = Marshal.GetLastWin32Error();
        return e == 0 ? "ok" : "AdjustTokenPrivileges " + e;
    }
    public static byte[] Read(string name, string guid, out string err) {
        byte[] b = new byte[65536]; uint a; err = null;
        uint n = GetFirmwareEnvironmentVariableExW(name, guid, b, (uint)b.Length, out a);
        if (n == 0) { err = "error " + Marshal.GetLastWin32Error() + " (203 = not present or not readable from Windows)"; return null; }
        byte[] r = new byte[n]; Array.Copy(b, r, n); err = "attributes 0x" + a.ToString("x"); return r;
    }
}
'@
L ('privilege: ' + [UpgFw]::Enable())

$shimGuid = '{605dab50-e046-4300-abb6-3dd810dd8b23}'
foreach ($n in 'SbatLevel', 'SbatLevelRT') {
    $err = $null
    $b = [UpgFw]::Read($n, $shimGuid, [ref]$err)
    L ''
    L ("== $n $shimGuid  $err")
    if ($b) { L ([Text.Encoding]::ASCII.GetString($b).TrimEnd([char]0)) }
}

$sha = [Security.Cryptography.SHA256]::Create()
foreach ($n in 'db', 'dbx') {
    L ''
    try {
        $v = Get-SecureBootUEFI -Name $n -ErrorAction Stop
        L ("== $n  $($v.Bytes.Length) bytes  sha256 " + (($sha.ComputeHash($v.Bytes) | ForEach-Object { $_.ToString('x2') }) -join ''))
    } catch { L ("== $n  not read: $_") }
}
L ''
try { L ('== Confirm-SecureBootUEFI: ' + (Confirm-SecureBootUEFI)) } catch { L "== Confirm-SecureBootUEFI: $_" }

L ''
L '== registry HKLM\SYSTEM\CurrentControlSet\Control\SecureBoot'
foreach ($k in @(Get-Item 'HKLM:\SYSTEM\CurrentControlSet\Control\SecureBoot' -ErrorAction SilentlyContinue) + @(Get-ChildItem 'HKLM:\SYSTEM\CurrentControlSet\Control\SecureBoot' -Recurse -ErrorAction SilentlyContinue)) {
    L ('  [' + ($k.Name -replace '^HKEY_LOCAL_MACHINE', 'HKLM') + ']')
    foreach ($p in $k.GetValueNames()) { L ("    $p = " + ($k.GetValue($p) -join ' ')) }
}

L ''
L '== Secure Boot update events, last 90 days (TPM-WMI, Kernel-Boot)'
$since = (Get-Date).AddDays(-90)
foreach ($prov in 'Microsoft-Windows-TPM-WMI', 'Microsoft-Windows-Kernel-Boot') {
    Get-WinEvent -FilterHashtable @{ LogName = 'System'; ProviderName = $prov; StartTime = $since } -ErrorAction SilentlyContinue |
        Where-Object { $_.Id -ge 1030 -and $_.Id -le 1050 -or $_.Id -ge 1795 -and $_.Id -le 1810 } |
        ForEach-Object { L ('  ' + $_.TimeCreated.ToUniversalTime().ToString('o') + "  $prov $($_.Id)  " + (($_.Message -replace '\s+', ' ').Trim())) }
}

foreach ($f in 'EFI\BOOT\grubx64.efi', 'EFI\BOOT\BOOTX64.EFI') {
    $p = Join-Path $root $f
    L ''
    if (-not (Test-Path $p)) { L "== $f  not on this stick"; continue }
    $bytes = [IO.File]::ReadAllBytes($p)
    L ("== $f  sha256 " + (($sha.ComputeHash($bytes) | ForEach-Object { $_.ToString('x2') }) -join ''))
    # PE section table: find .sbat and print its text
    $pe = [BitConverter]::ToInt32($bytes, 0x3c)
    $nsec = [BitConverter]::ToUInt16($bytes, $pe + 6)
    $opt = [BitConverter]::ToUInt16($bytes, $pe + 20)
    $tab = $pe + 24 + $opt
    for ($i = 0; $i -lt $nsec; $i++) {
        $s = $tab + 40 * $i
        $name = [Text.Encoding]::ASCII.GetString($bytes, $s, 8).TrimEnd([char]0)
        if ($name -eq '.sbat') {
            $size = [BitConverter]::ToInt32($bytes, $s + 16); $at = [BitConverter]::ToInt32($bytes, $s + 20)
            L ([Text.Encoding]::ASCII.GetString($bytes, $at, $size).TrimEnd([char]0))
        }
    }
}

$o | Set-Content -Path $OutFile -Encoding UTF8
$o | ForEach-Object { Write-Host $_ }
Write-Host ''
Write-Host "  written: $OutFile"
