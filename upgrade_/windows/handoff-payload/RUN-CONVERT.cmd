@echo off
REM ============================================================
REM  upgrade_ - CONVERT THIS COMPUTER (keep Windows, install
REM  Linux alongside)  -  ONE CLICK, ONE TYPED WORD
REM
REM  This is the converter. It CHANGES THE INTERNAL DISK: it may
REM  run Windows' own disk check (with a restart), it shrinks the
REM  Windows partition to make room, and it restarts into the
REM  Linux installer from this stick. Windows is kept and stays
REM  bootable from the boot menu until you choose to reclaim it
REM  later, in Linux.
REM
REM  What it does, in order:
REM    1. runs the scanner (report + JSON on this stick)
REM    2. writes job.json for this machine (refuses on RED, legacy
REM       BIOS, unknown BitLocker state, unmapped locale)
REM    3. generates the kickstart from the job
REM    4. asks you to type CONVERT
REM    5. the prologue: re-validates the job against this machine,
REM       runs the disk check if Windows flagged C: (restart), re-
REM       measures the room, shrinks C:, suspends BitLocker for one
REM       restart, arms the one-time boot handoff and restarts.
REM  Every refusal happens before anything is changed. A restart
REM  in the middle is normal: leave the stick in, sign in when
REM  Windows comes back, and it continues by itself.
REM ============================================================

net session >nul 2>&1
if %errorlevel% neq 0 (
  echo Asking for administrator access - click "Yes" on the prompt...
  powershell -NoProfile -Command "Start-Process -FilePath '%~f0' -Verb RunAs"
  exit /b
)

cd /d "%~dp0"
for %%f in (Invoke-Prologue.ps1 upgrade-scan.ps1 New-Job.ps1 New-Kickstart.ps1 EFI\BOOT\BOOTX64.EFI images\install.img upgrade_\verify.sh upgrade_\outcome.sh upgrade_\LiveOS\kde.squashfs SHA256SUMS) do (
  if not exist "%~dp0%%f" (
    echo   ERROR: %%f is not on this stick - this is not a complete kit.
    pause
    exit /b 1
  )
)

echo.
echo ============================================================
echo   upgrade_ - convert this computer   (stick: %~d0)
echo ============================================================
echo.
echo   Step 1 of 5: scanning this computer (nothing is changed)...
echo.
if not exist "%~dp0upgrade_\reports" mkdir "%~dp0upgrade_\reports"
powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0upgrade-scan.ps1" -Json -OutDir "%~dp0upgrade_\reports"
powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0upgrade-scan.ps1" -DumpMachine "%~dp0machine-capture.json" >nul

echo.
echo   Step 2 of 5: writing the job for this machine...
echo.
powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0New-Job.ps1" -StickDrive %~d0 -OutDir "%~dp0upgrade_" -ScanDir "%~dp0upgrade_\reports" -Desktop kde -IfCannotKeep stop
if %errorlevel% neq 0 (
  echo.
  echo   No job was written - the reasons are above. Nothing was changed.
  pause
  exit /b 1
)

echo.
echo   Step 3 of 5: generating the kickstart...
echo.
powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0New-Kickstart.ps1" -JobPath "%~dp0upgrade_\job.json" -OutFile "%~dp0upgrade_\ks.cfg" -StickLabel UPGV0 -Manifest "%~dp0SHA256SUMS"
if %errorlevel% neq 0 (
  echo.
  echo   The kickstart could not be generated. Nothing was changed.
  pause
  exit /b 1
)
if exist "%~dp0upgrade_\boot-verify" del "%~dp0upgrade_\boot-verify"
if exist "%~dp0upgrade_\boot-install" del "%~dp0upgrade_\boot-install"

echo.
echo   Step 4 of 5: your decision.
echo.
echo   This will change the internal disk of this computer: Windows' disk
echo   check may run (with a restart), the Windows partition will be shrunk,
echo   and Linux will be installed beside it. Windows stays bootable from
echo   the boot menu until you reclaim it later. If the disk check runs it
echo   may be slow - do not switch the computer off while it runs.
echo.
set WORD=
set /p WORD=  Type CONVERT (in capitals) to continue, anything else to stop:
if not "%WORD%"=="CONVERT" (
  echo.
  echo   Not confirmed. Nothing was changed.
  pause
  exit /b 1
)

echo.
echo   Step 5 of 5: the prologue.
echo.
powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0Invoke-Prologue.ps1" -Start -StickDrive %~d0 -ConfirmWord %WORD%
if %errorlevel% neq 0 (
  echo.
  echo   The prologue stopped - read the message above. If it says STOPPED,
  echo   Windows is as it was and the record is in upgrade_\outcome.json on
  echo   this stick. Close this window.
  pause
  exit /b 1
)
