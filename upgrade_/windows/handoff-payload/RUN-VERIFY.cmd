@echo off
REM ============================================================
REM  upgrade_ live-boot test (V1, reversible half) - ONE CLICK
REM
REM  Double-click this, click "Yes" on the blue prompt, and let
REM  the computer restart. It boots the Fedora installer from
REM  this stick, which checks that this is the right computer,
REM  checks the display and Wi-Fi hardware, reads the desktop
REM  image on this stick back, writes its report to the stick and
REM  restarts back into Windows. NOTHING IS INSTALLED and nothing
REM  on the internal disk is changed. When Windows comes back and
REM  you sign in, the result appears by itself.
REM
REM  What it does, in order:
REM    1. runs the scanner (report + JSON on this stick)
REM    2. writes job.json for this machine (refuses on RED,
REM       legacy BIOS, unknown BitLocker state, unmapped locale)
REM    3. generates the kickstart from the job
REM    4. arms the one-time boot handoff and restarts
REM  The one boot entry it adds is removed by the return check
REM  whatever happens. BitLocker is suspended for one restart.
REM ============================================================

net session >nul 2>&1
if %errorlevel% neq 0 (
  echo Asking for administrator access - click "Yes" on the prompt...
  powershell -NoProfile -Command "Start-Process -FilePath '%~f0' -Verb RunAs"
  exit /b
)

cd /d "%~dp0"
for %%f in (Test-Handoff.ps1 upgrade-scan.ps1 New-Job.ps1 New-Kickstart.ps1 EFI\BOOT\BOOTX64.EFI images\install.img upgrade_\verify.sh upgrade_\LiveOS\kde.squashfs SHA256SUMS) do (
  if not exist "%~dp0%%f" (
    echo   ERROR: %%f is not on this stick - this is not a complete kit.
    pause
    exit /b 1
  )
)

echo.
echo ============================================================
echo   upgrade_ live-boot test   (stick: %~d0)
echo ============================================================
echo.
echo   Step 1 of 4: scanning this computer (nothing is changed)...
echo.
if not exist "%~dp0upgrade_\reports" mkdir "%~dp0upgrade_\reports"
powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0upgrade-scan.ps1" -Json -OutDir "%~dp0upgrade_\reports"
powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0upgrade-scan.ps1" -DumpMachine "%~dp0machine-capture.json" >nul

echo.
echo   Step 2 of 4: writing the job for this machine...
echo.
powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0New-Job.ps1" -StickDrive %~d0 -OutDir "%~dp0upgrade_" -ScanDir "%~dp0upgrade_\reports" -Desktop kde
if %errorlevel% neq 0 (
  echo.
  echo   No job was written - the reasons are above. Nothing was changed.
  pause
  exit /b 1
)

echo.
echo   Step 3 of 4: generating the kickstart...
echo.
powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0New-Kickstart.ps1" -JobPath "%~dp0upgrade_\job.json" -OutFile "%~dp0upgrade_\ks.cfg" -StickLabel UPGV0 -Manifest "%~dp0SHA256SUMS"
if %errorlevel% neq 0 (
  echo.
  echo   The kickstart could not be generated. Nothing was changed.
  pause
  exit /b 1
)
echo v1> "%~dp0upgrade_\boot-verify"
if exist "%~dp0upgrade_\boot-install" del "%~dp0upgrade_\boot-install"
if exist "%~dp0upgrade_\report" rd /s /q "%~dp0upgrade_\report"

echo.
echo   Step 4 of 4: arming the boot handoff. The computer will restart.
echo.
powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0Test-Handoff.ps1" -Arm -Auto -Payload shim -PayloadDrive %~d0 -SuspendBitLocker
if %errorlevel% neq 0 (
  echo.
  echo   The handoff refused to arm - nothing was changed. Read the message above,
  echo   then close this window.
  pause
  exit /b 1
)
