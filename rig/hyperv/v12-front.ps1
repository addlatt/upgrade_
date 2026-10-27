# V12 rig helper (2026-10-17): bring UPGRADE.exe's window to the front, maximized, so the
# screens arm's clicks land on fixed guest pixels. Runs inside the signed-in session (a task).
Add-Type @'
using System;
using System.Runtime.InteropServices;
public static class W {
    [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr h, int c);
    [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
}
'@
$p = Get-Process UPGRADE -ErrorAction SilentlyContinue | Where-Object { $_.MainWindowHandle -ne 0 } | Select-Object -First 1
if ($p) { [W]::ShowWindow($p.MainWindowHandle, 3) | Out-Null; [W]::SetForegroundWindow($p.MainWindowHandle) | Out-Null }
(New-Object -ComObject WScript.Shell).AppActivate('upgrade_') | Out-Null
