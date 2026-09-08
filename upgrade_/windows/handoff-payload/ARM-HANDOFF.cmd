@echo off
REM ============================================================
REM  upgrade_ V0 handoff test - ARM (step 1 of 2)
REM
REM  Runs Test-Handoff.ps1 -Arm against THIS stick: the payload
REM  drive is the drive this launcher lives on (%~d0), so there
REM  is no drive letter to type and no wrong device to pick.
REM
REM  Double-click, click "Yes" on the User Account Control
REM  prompt, choose the row, then let it reboot. After Windows
REM  comes back, double-click CHECK-HANDOFF.cmd.
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
  echo   ERROR: no payload at %~dp0EFI\BOOT\BOOTX64.EFI - this is not a payload stick.
  pause
  exit /b 1
)

echo.
echo ============================================================
echo   upgrade_ V0 handoff test - ARM   (stick: %~d0)
echo ============================================================
echo.
echo   Before choosing: run the scanner (RUN-SCANNER.cmd) once and
echo   read its BitLocker line. If BitLocker is on, have the
echo   recovery key saved somewhere that is NOT this computer.
echo.
echo   1  baseline        (suspends BitLocker if on; the shipping default)
echo   2  NoFile          (entry points at a missing file; expect: ignored)
echo   3  SecureBootUnsigned  (Secure Boot ON + the Shell stick; expect: ignored)
echo   4  NoSuspend       (BitLocker ON, not suspended; records what happens)
echo.
choice /c 1234 /n /m "  Which row? [1-4] "
set ROW=%errorlevel%
set ARGS=-Arm -PayloadDrive %~d0
if %ROW%==1 set ARGS=%ARGS% -SuspendBitLocker
if %ROW%==2 set ARGS=%ARGS% -FailMode NoFile
if %ROW%==3 set ARGS=%ARGS% -FailMode SecureBootUnsigned
if %ROW%==4 set ARGS=%ARGS% -FailMode NoSuspend

echo.
powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0Test-Handoff.ps1" %ARGS%
if %errorlevel% neq 0 (
  echo.
  echo   The harness refused or failed - nothing was armed. Read the message above.
  pause
  exit /b 1
)

echo.
echo   Armed. Leave this stick plugged in. Watch the screen during the
echo   reboot and note whether any key had to be pressed.
echo.
choice /c YN /n /m "  Reboot now? [Y/N] "
if %errorlevel%==1 shutdown /r /t 5 /c "upgrade_ V0 handoff test"
