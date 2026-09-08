@echo off
REM ============================================================
REM  upgrade_ V0 handoff test - CHECK (step 2 of 2)
REM
REM  Runs Test-Handoff.ps1 -Check after the reboot: classifies
REM  the result, removes the test boot entry, restores the BCD
REM  state, asks three questions, and appends ONE evidence row
REM  to v0-handoff.csv on this stick. Bring the stick back; the
REM  row is transported verbatim into the repo's CSV.
REM ============================================================

net session >nul 2>&1
if %errorlevel% neq 0 (
  echo Asking for administrator access - click "Yes" on the prompt...
  powershell -NoProfile -Command "Start-Process -FilePath '%~f0' -Verb RunAs"
  exit /b
)

cd /d "%~dp0"
if not exist "%~dp0Test-Handoff.ps1" (
  echo   ERROR: Test-Handoff.ps1 is not next to this launcher.
  pause
  exit /b 1
)

echo.
echo ============================================================
echo   upgrade_ V0 handoff test - CHECK   (stick: %~d0)
echo ============================================================
echo.
powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0Test-Handoff.ps1" -Check -ResultsCsv "%~dp0v0-handoff.csv"

echo.
echo   If the result was 'persisted' or 'reordered', or Windows did not come
echo   back on its own, run this once more with the BCD backup:
echo     Test-Handoff.ps1 -Check -RestoreBcd
echo   (the harness keeps the backup in %%ProgramData%%\upgrade_\v0).
echo.
pause
