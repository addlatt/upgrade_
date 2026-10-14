@echo off
REM ============================================================
REM  upgrade_ - read-only volume diagnostic (RISKS R18)
REM
REM  For a machine whose C: keeps NTFS's dirty flag through restarts.
REM  Reads only: the flag, what autochk is set to do at boot, the last
REM  boot-time and online check results Windows logged, NTFS corruption
REM  events, found.000, the volume's own health. Writes ONE file to this
REM  stick: upgrade_\reports\volume-diag-<computer>.txt. Changes nothing.
REM  Runs `upgrade-scan diag volume` (its inline PowerShell until 2026-10-14).
REM ============================================================
net session >nul 2>&1
if %errorlevel% neq 0 (
  echo Asking for administrator access - click "Yes" on the prompt...
  powershell -NoProfile -Command "Start-Process -FilePath '%~f0' -Verb RunAs"
  exit /b
)
cd /d "%~dp0"
if not exist "%~dp0upgrade_\reports" mkdir "%~dp0upgrade_\reports"
if not exist "%~dp0upgrade-scan.exe" (
  echo   ERROR: upgrade-scan.exe is not on this stick - this is not a complete kit.
  pause
  exit /b 1
)
echo   writing %~dp0upgrade_\reports\volume-diag-%COMPUTERNAME%.txt ...
"%~dp0upgrade-scan.exe" diag volume
echo.
echo   Done. Nothing on this computer was changed. Bring the stick back.
pause
