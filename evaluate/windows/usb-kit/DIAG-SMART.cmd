@echo off
REM ============================================================
REM  upgrade_ - read-only SMART / drive diagnostic
REM  Reads every drive's health attributes and the disk error log.
REM  Writes ONE file to this stick: upgrade_\reports\smart-diag-<computer>.txt
REM  Changes nothing on this computer.
REM ============================================================
net session >nul 2>&1
if %errorlevel% neq 0 (
  echo Asking for administrator access - click "Yes" on the prompt...
  powershell -NoProfile -Command "Start-Process -FilePath '%~f0' -Verb RunAs"
  exit /b
)
cd /d "%~dp0"
powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0Diag-Smart.ps1"
echo.
echo   Done. Bring the stick back.
pause
