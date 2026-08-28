@echo off
REM ============================================================
REM  upgrade_ scanner - one-click USB launcher
REM
REM  Pair this with the SINGLE-FILE scanner: copy both this file
REM  and dist/upgrade-scan.ps1 into the same folder on a USB
REM  stick (see README.md). It will NOT work next to the source
REM  evaluate/windows/upgrade-scan.ps1, which needs data/ beside it.
REM
REM  On the target machine: double-click this file, then click
REM  "Yes" on the User Account Control prompt. Nothing to type.
REM ============================================================

REM --- self-elevate to Administrator (storage-mode / shrink / BitLocker need it) ---
net session >nul 2>&1
if %errorlevel% neq 0 (
  echo Asking for administrator access - click "Yes" on the prompt...
  powershell -NoProfile -Command "Start-Process -FilePath '%~f0' -Verb RunAs"
  exit /b
)

cd /d "%~dp0"

if not exist "%~dp0upgrade-scan.ps1" (
  echo.
  echo   ERROR: upgrade-scan.ps1 is not next to this launcher.
  echo   Copy dist\upgrade-scan.ps1 into this folder and try again.
  echo.
  pause
  exit /b 1
)

echo.
echo ============================================================
echo   upgrade_ scanner  -  running as Administrator
echo ============================================================
echo.
powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0upgrade-scan.ps1" -OutDir "%~dp0."

echo.
echo === Saving machine capture for the corpus ===
powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0upgrade-scan.ps1" -DumpMachine "%~dp0machine-capture.json"

echo.
echo ============================================================
echo   Done. The report and machine-capture.json are saved onto
echo   this USB stick. Unplug it and bring it back.
echo ============================================================
echo.
pause
