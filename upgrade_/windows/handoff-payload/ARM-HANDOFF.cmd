@echo off
REM ============================================================
REM  upgrade_ V0 handoff test - ARM (matrix rows, step 1 of 2)
REM
REM  For running the full vendor matrix by hand. The one-click
REM  flow is RUN-TEST.cmd; use this to pick a specific row.
REM  The payload drive is the drive this launcher lives on (%~d0).
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
echo   upgrade_ V0 handoff test - ARM   (stick: %~d0)
echo ============================================================
echo.
echo   Run RUN-SCANNER.cmd first if you have not: its BitLocker line
echo   tells you whether a recovery key must be saved before this.
echo.
echo   1  signed payload, baseline       (Secure Boot ON or off; the product path)
echo   2  unsigned Shell, baseline       (Secure Boot must be OFF)
echo   3  unsigned Shell, Secure Boot ON (expect: ignored - the refusal row)
echo   4  NoFile                         (expect: ignored - fail-safe row)
echo   5  NoSuspend                      (BitLocker ON, not suspended; records what happens)
echo.
choice /c 12345 /n /m "  Which row? [1-5] "
set ROW=%errorlevel%
set ARGS=-Arm -PayloadDrive %~d0
if %ROW%==1 set ARGS=%ARGS% -Payload shim -SuspendBitLocker
if %ROW%==2 set ARGS=%ARGS% -Payload shell -SuspendBitLocker
if %ROW%==3 set ARGS=%ARGS% -Payload shell -FailMode SecureBootUnsigned -SuspendBitLocker
if %ROW%==4 set ARGS=%ARGS% -Payload shim -FailMode NoFile -SuspendBitLocker
if %ROW%==5 set ARGS=%ARGS% -Payload shim -FailMode NoSuspend

echo.
choice /c YN /n /m "  Let the return check run itself after the restart (Y), or run CHECK-HANDOFF.cmd by hand (N)? "
if %errorlevel%==1 set ARGS=%ARGS% -Auto

echo.
powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0Test-Handoff.ps1" %ARGS%
if %errorlevel% neq 0 (
  echo.
  echo   The harness refused or failed - nothing was armed. Read the message above.
  pause
  exit /b 1
)
echo %ARGS% | find "-Auto" >nul && exit /b 0

echo.
echo   Armed. Leave this stick plugged in. Watch the screen during the
echo   reboot and note whether any key had to be pressed.
echo.
choice /c YN /n /m "  Reboot now? [Y/N] "
if %errorlevel%==1 shutdown /r /t 5 /c "upgrade_ V0 handoff test"
