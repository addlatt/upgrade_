@echo off
REM ============================================================
REM  upgrade_ - read-only Secure Boot revocation diagnostic
REM  Reads the firmware's SBAT level, the size of db and dbx,
REM  Windows' Secure Boot update events, and the SBAT data of
REM  this stick's boot files. Writes ONE file to this stick:
REM  upgrade_\report\secureboot-diag-<time>.txt
REM  Changes nothing on this computer.
REM ============================================================
net session >nul 2>&1
if %errorlevel% neq 0 (
  echo Asking for administrator access - click "Yes" on the prompt...
  powershell -NoProfile -Command "Start-Process -FilePath '%~f0' -Verb RunAs"
  exit /b
)
cd /d "%~dp0"
powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0Diag-SecureBoot.ps1"
echo.
echo   Done. Bring the stick back.
pause
