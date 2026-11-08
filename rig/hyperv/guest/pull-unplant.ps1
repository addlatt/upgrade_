# Pull arm: take guest/pull-plant.ps1's tree off this Windows again (rig tooling).
# Unregisters the test sync root first (the Cloud Files filter can block
# deletes under a registered root), then removes the planted folders, the
# Chrome Bookmarks file only if Chrome is not installed here (so it was ours),
# and the Firefox test profile. Elevated, PowerShell Direct, Windows PowerShell 5.1.
$ErrorActionPreference = 'Continue'
$docs = [Environment]::GetFolderPath('MyDocuments')
$pics = [Environment]::GetFolderPath('MyPictures')
$root = Join-Path $docs 'pull-test\Cloud'
Add-Type -TypeDefinition @'
using System.Runtime.InteropServices;
public static class CfU { [DllImport("cldapi.dll", CharSet = CharSet.Unicode)] public static extern int CfUnregisterSyncRoot(string path); }
'@
if (Test-Path -LiteralPath $root) { Write-Host ('CfUnregisterSyncRoot hr=0x{0:X8}' -f [CfU]::CfUnregisterSyncRoot($root)) }
foreach ($d in @((Join-Path $docs 'pull-test'), (Join-Path $pics 'pull-test'))) {
    if (Test-Path -LiteralPath $d) { cmd /c "rmdir /s /q `"$d`""; Write-Host "removed $d : $(-not (Test-Path -LiteralPath $d))" }
}
$chromeInstalled = (Test-Path "$env:ProgramFiles\Google\Chrome\Application\chrome.exe") -or (Test-Path "${env:ProgramFiles(x86)}\Google\Chrome\Application\chrome.exe") -or (Test-Path "$env:LOCALAPPDATA\Google\Chrome\Application\chrome.exe")
Write-Host "Chrome installed: $chromeInstalled"
# only what the plant wrote, then the folders it left empty
function Remove-Empty($leaf, $stop) { $d = $leaf; while ($d -and ($d.Length -gt $stop.Length) -and (Test-Path -LiteralPath $d) -and -not (Get-ChildItem -LiteralPath $d -Force)) { Remove-Item -LiteralPath $d -Force; Write-Host "removed empty $d"; $d = Split-Path $d -Parent } }
$cd = Join-Path $env:LOCALAPPDATA 'Google\Chrome\User Data\Default'
if (-not $chromeInstalled -and (Test-Path (Join-Path $cd 'Bookmarks'))) { Remove-Item -LiteralPath (Join-Path $cd 'Bookmarks') -Force; Write-Host "removed $cd\Bookmarks"; Remove-Empty $cd $env:LOCALAPPDATA }
$ff = Join-Path $env:APPDATA 'Mozilla\Firefox\Profiles\pulltest.default-release'
if (Test-Path $ff) { Remove-Item -LiteralPath $ff -Recurse -Force; Write-Host "removed $ff"; Remove-Empty (Split-Path $ff -Parent) $env:APPDATA }
Write-Host 'full shutdown'; shutdown /s /t 5 /f
