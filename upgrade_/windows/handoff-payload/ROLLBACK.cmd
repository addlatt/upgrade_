@echo off
REM ============================================================
REM  upgrade_ - ROLL BACK to Windows first
REM
REM  For a computer converted with Windows kept. Run this from
REM  Windows (pick it in the boot menu Linux shows). It puts
REM  Windows Boot Manager first in the firmware's boot order and
REM  puts Windows' own fallback boot file back from the copy this
REM  stick took before the conversion. It deletes NOTHING: Linux
REM  stays on the disk and in the firmware's boot menu; its space
REM  is only returned when you ask for that separately.
REM ============================================================

net session >nul 2>&1
if %errorlevel% neq 0 (
  echo Asking for administrator access - click "Yes" on the prompt...
  powershell -NoProfile -Command "Start-Process -FilePath '%~f0' -Verb RunAs"
  exit /b
)

cd /d "%~dp0"
if not exist "%~dp0Invoke-Rollback.ps1" (
  echo   ERROR: Invoke-Rollback.ps1 is not on this stick.
  pause
  exit /b 1
)
if not exist "%~dp0upgrade_\esp-snapshot\SHA256SUMS" (
  echo   ERROR: this stick holds no ESP snapshot - it was not the stick this computer was converted with.
  pause
  exit /b 1
)

echo.
echo ============================================================
echo   upgrade_ - roll back to Windows first   (stick: %~d0)
echo ============================================================
echo.
set WORD=
set /p WORD=  Type ROLLBACK (in capitals) to continue, anything else to stop:
if not "%WORD%"=="ROLLBACK" (
  echo.
  echo   Not confirmed. Nothing was changed.
  pause
  exit /b 1
)
powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0Invoke-Rollback.ps1" -StickDrive %~d0
if %errorlevel% neq 0 (
  echo.
  echo   Rollback did not complete - read the message above.
  pause
  exit /b 1
)
echo.
echo   Done. Restart the computer; it boots Windows directly.
pause
