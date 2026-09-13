@echo off
REM ============================================================
REM  upgrade_ - read-only volume diagnostic (RISKS R18)
REM
REM  For a machine whose C: keeps NTFS's dirty flag through restarts.
REM  Reads only: the flag, what autochk is set to do at boot, the last
REM  boot-time and online check results Windows logged, NTFS corruption
REM  events, found.000, the volume's own health. Writes ONE file to this
REM  stick: upgrade_\reports\volume-diag-<computer>.txt. Changes nothing.
REM ============================================================
net session >nul 2>&1
if %errorlevel% neq 0 (
  echo Asking for administrator access - click "Yes" on the prompt...
  powershell -NoProfile -Command "Start-Process -FilePath '%~f0' -Verb RunAs"
  exit /b
)
cd /d "%~dp0"
if not exist "%~dp0upgrade_\reports" mkdir "%~dp0upgrade_\reports"
set OUT=%~dp0upgrade_\reports\volume-diag-%COMPUTERNAME%.txt
echo   writing %OUT% ...
powershell -NoProfile -ExecutionPolicy Bypass -Command ^
  "$o = @(); $o += '== upgrade_ volume diagnostic ' + (Get-Date).ToUniversalTime().ToString('o') + ' ' + $env:COMPUTERNAME;" ^
  "$o += '-- fsutil dirty query C:'; $o += (fsutil dirty query C: 2>&1);" ^
  "$o += '-- chkntfs C:'; $o += (chkntfs C: 2>&1);" ^
  "$o += '-- BootExecute (what autochk runs at boot)'; $o += (Get-ItemProperty 'HKLM:\SYSTEM\CurrentControlSet\Control\Session Manager' -Name BootExecute -ErrorAction SilentlyContinue).BootExecute;" ^
  "$o += '-- found.*'; $o += (Get-ChildItem C:\ -Force -Directory -ErrorAction SilentlyContinue | Where-Object Name -match '^found\.\d{3}$' | ForEach-Object { $_.FullName + '  ' + $_.LastWriteTime + '  files=' + (Get-ChildItem $_.FullName -Recurse -File -Force -ErrorAction SilentlyContinue | Measure-Object).Count });" ^
  "$o += '-- Get-Volume C:'; $o += (Get-Volume -DriveLetter C | Format-List DriveLetter,FileSystem,HealthStatus,OperationalStatus,Size,SizeRemaining | Out-String);" ^
  "$o += '-- Repair-Volume -Scan (read-only)'; $o += ('' + (Repair-Volume -DriveLetter C -Scan -ErrorAction SilentlyContinue));" ^
  "$o += '-- fsutil fsinfo ntfsinfo C:'; $o += (fsutil fsinfo ntfsinfo C: 2>&1);" ^
  "$o += '-- Wininit 1001 (boot-time chkdsk output), last 30 days'; $o += (Get-WinEvent -FilterHashtable @{LogName='Application'; Id=1001; StartTime=(Get-Date).AddDays(-30)} -ErrorAction SilentlyContinue | Where-Object ProviderName -eq 'Microsoft-Windows-Wininit' | ForEach-Object { '[' + $_.TimeCreated + ']'; $_.Message });" ^
  "$o += '-- Chkdsk provider events (26212/26214/26226/26228 ...), last 30 days'; $o += (Get-WinEvent -FilterHashtable @{LogName='Application'; ProviderName='Chkdsk'; StartTime=(Get-Date).AddDays(-30)} -ErrorAction SilentlyContinue | ForEach-Object { '[' + $_.TimeCreated + '] id=' + $_.Id; $_.Message });" ^
  "$o += '-- Ntfs events (55 corruption, 98 mount, 130/131 ...), System log, last 30 days'; $o += (Get-WinEvent -FilterHashtable @{LogName='System'; ProviderName='Microsoft-Windows-Ntfs'; StartTime=(Get-Date).AddDays(-30)} -ErrorAction SilentlyContinue | Select-Object -First 40 | ForEach-Object { '[' + $_.TimeCreated + '] id=' + $_.Id + ' ' + (($_.Message -replace '\s+',' ').Substring(0, [Math]::Min(300, $_.Message.Length))) });" ^
  "$o += '-- disk events (System, provider disk/storahci/stornvme), last 30 days'; $o += (Get-WinEvent -FilterHashtable @{LogName='System'; StartTime=(Get-Date).AddDays(-30)} -ErrorAction SilentlyContinue | Where-Object { $_.ProviderName -in 'disk','storahci','stornvme','partmgr','volmgr' } | Select-Object -First 30 | ForEach-Object { '[' + $_.TimeCreated + '] ' + $_.ProviderName + ' id=' + $_.Id + ' ' + (($_.Message -replace '\s+',' ').Substring(0, [Math]::Min(200, $_.Message.Length))) });" ^
  "$o += '-- Get-PhysicalDisk'; $o += (Get-PhysicalDisk | Format-Table DeviceId,FriendlyName,MediaType,HealthStatus,OperationalStatus,Size -AutoSize | Out-String);" ^
  "$o += '-- Fast Startup (HiberbootEnabled) and hiberfil'; $o += ('HiberbootEnabled=' + (Get-ItemProperty 'HKLM:\SYSTEM\CurrentControlSet\Control\Session Manager\Power' -Name HiberbootEnabled -ErrorAction SilentlyContinue).HiberbootEnabled + '  hiberfil=' + (Test-Path C:\hiberfil.sys));" ^
  "$o += '-- last boot / uptime'; $o += ('LastBootUpTime=' + (Get-CimInstance Win32_OperatingSystem).LastBootUpTime);" ^
  "[IO.File]::WriteAllLines('%OUT%', ($o | ForEach-Object { \"$_\" }), (New-Object Text.UTF8Encoding($false)))"
echo.
echo   Done. Nothing on this computer was changed. Bring the stick back.
pause
