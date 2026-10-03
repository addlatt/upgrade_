@echo off
REM ============================================================
REM  upgrade_ - CANCEL a conversion that has not crossed its line
REM
REM  For a computer that is back in Windows with a conversion
REM  still "in progress" (a launcher then says "a conversion is
REM  already in progress"). This is the safe direction: it only
REM  undoes what the prologue did on the Windows side. It removes
REM  the one-time boot entry to the stick, turns BitLocker
REM  protection back on if it was suspended, puts the pagefile and
REM  hibernation back, deletes the Wi-Fi passwords from the stick
REM  and moves the state aside (state-aborted.json). Nothing is
REM  erased. A shrink already made stays (Disk Management can
REM  extend C: again).
REM
REM  Before it changes anything, it copies the firmware's boot
REM  list and the prologue's state onto this stick (rule 5: every
REM  contact with a real machine leaves a capture). Added after
REM  the Aspire's run 10 (2026-10-03): the handoff did not fire and
REM  the only way out was a typed -Abort.
REM ============================================================

net session >nul 2>&1
if %errorlevel% neq 0 (
  echo Asking for administrator access - click "Yes" on the prompt...
  powershell -NoProfile -Command "Start-Process -FilePath '%~f0' -Verb RunAs"
  exit /b
)

cd /d "%~dp0"
if not exist "%~dp0Invoke-Prologue.ps1" (
  echo   ERROR: Invoke-Prologue.ps1 is not on this stick - this is not a complete kit.
  pause
  exit /b 1
)
if not exist "%~dp0upgrade_\report" mkdir "%~dp0upgrade_\report"

echo.>> "%~dp0upgrade_\convert.log"
echo ======== %date% %time%  CANCEL-CONVERSION.cmd on %COMPUTERNAME%  (stick %~d0)>> "%~dp0upgrade_\convert.log"

echo.
echo ============================================================
echo   upgrade_ - cancel the conversion   (stick: %~d0)
echo ============================================================
echo.
echo   Nothing is erased. Windows stays as it is.
echo.

REM the evidence first, read-only: what the firmware holds before -Abort removes our entry
powershell -NoProfile -ExecutionPolicy Bypass -Command "$o = Join-Path '%~dp0upgrade_\report' ('before-cancel-' + (Get-Date).ToUniversalTime().ToString('yyyyMMddTHHmmssZ') + '.txt'); & { '== utc ' + (Get-Date).ToUniversalTime().ToString('o'); '== bcdedit {fwbootmgr}'; bcdedit /enum '{fwbootmgr}'; '== bcdedit firmware'; bcdedit /enum firmware; '== resume task'; Get-ScheduledTask -TaskName 'upgrade_ prologue resume' -EA SilentlyContinue | Get-ScheduledTaskInfo | Format-List; '== state dir'; Get-ChildItem (Join-Path $env:ProgramData 'upgrade_') -Recurse -EA SilentlyContinue | Select-Object FullName,Length,LastWriteTime | Format-Table -AutoSize | Out-String -Width 250; '== state.json'; Get-Content (Join-Path $env:ProgramData 'upgrade_\prologue\state.json') -EA SilentlyContinue; '== prologue.log (C:)'; Get-Content (Join-Path $env:ProgramData 'upgrade_\prologue\prologue.log') -EA SilentlyContinue; '== boots, last 6 h'; Get-WinEvent -FilterHashtable @{LogName='System';Id=12,13,41,1074,6008;StartTime=(Get-Date).AddHours(-6)} -EA SilentlyContinue | Select-Object TimeCreated,Id,Message | Format-List } *> $o; Write-Host ('  saved the firmware boot list to ' + $o)"

powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0Invoke-Logged.ps1" -Log "%~dp0upgrade_\convert.log" -Script "%~dp0Invoke-Prologue.ps1" -Abort
if %errorlevel% neq 0 (
  echo.
  echo   The cancel did not finish - read the message above.
  echo   Nothing was erased. Bring this stick back so the log can be read.
  pause
  exit /b 1
)
echo.
echo   Cancelled. You can start a launcher again now.
echo.
pause
