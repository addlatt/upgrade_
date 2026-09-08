@echo off
REM ============================================================
REM  upgrade_ V0 handoff test - ONE CLICK
REM
REM  Double-click this, click "Yes" on the blue prompt, and let
REM  the computer restart. When Windows comes back and you sign
REM  in, the result appears by itself and is saved on this stick.
REM
REM  What it does: runs the scanner (report + machine capture on
REM  this stick), then arms the one-time boot test with the SIGNED
REM  payload (works with Secure Boot on), registers its own return
REM  check for the next sign-in, and restarts. Nothing on the
REM  internal disk is changed; the one boot entry it adds is
REM  removed by the return check whatever happens.
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
if not exist "%~dp0EFI\BOOT\BOOTX64.EFI" (
  echo   ERROR: no payload at %~dp0EFI\BOOT\BOOTX64.EFI - this is not the test stick.
  pause
  exit /b 1
)

echo.
echo ============================================================
echo   upgrade_ V0 handoff test   (stick: %~d0)
echo ============================================================
echo.
echo   Step 1 of 2: scanning this computer (nothing is changed)...
echo.
powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0upgrade-scan.ps1" -OutDir "%~dp0."
powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0upgrade-scan.ps1" -DumpMachine "%~dp0machine-capture.json" >nul

echo.
echo   Step 2 of 2: arming the boot test. The computer will restart.
echo.
powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0Test-Handoff.ps1" -Arm -Auto -Payload shim -PayloadDrive %~d0 -SuspendBitLocker
if %errorlevel% neq 0 (
  echo.
  echo   The test refused to arm - nothing was changed. Read the message above,
  echo   then close this window.
  pause
  exit /b 1
)
