@echo off
REM ============================================================
REM  upgrade_ - WALK-AWAY PROBE  (read-only, one restart)
REM
REM  Tests one thing on this computer: that the conversion can
REM  continue after a restart with NOBODY signed in. It registers
REM  the same startup task the conversion uses, restarts, and on
REM  the way back records who ran it, whether anyone was signed
REM  in, and how long this USB stick took to appear. Then it
REM  removes the task. Nothing on the disk is changed.
REM
REM  When Windows comes back: DO NOT SIGN IN for two minutes.
REM  Leave it at the sign-in screen. Then sign in as usual - a
REM  window shows the result, and the row is on this stick
REM  (upgrade_\walkaway-probe.csv). Never edit that file.
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

echo.
echo ============================================================
echo   upgrade_ - walk-away probe   (stick: %~d0)
echo ============================================================
echo.
echo   This restarts the computer once. Nothing on the disk is changed.
echo   When Windows comes back, DO NOT SIGN IN for two minutes - leave
echo   it at the sign-in screen with the stick in. Then sign in.
echo.
pause
powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0Invoke-Prologue.ps1" -Probe -ProbeStickDrive %~d0
if %errorlevel% neq 0 (
  echo.
  echo   The probe did not start - read the message above. Nothing was changed.
  pause
  exit /b 1
)
