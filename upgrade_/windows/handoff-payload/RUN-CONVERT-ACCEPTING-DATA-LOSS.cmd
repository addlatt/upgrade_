@echo off
REM ============================================================
REM  upgrade_ - CONVERT THIS COMPUTER, ACCEPTING DATA LOSS
REM
REM  This is NOT the normal launcher. Use RUN-CONVERT.cmd. This one
REM  exists for a computer the scanner has refused because its DRIVE
REM  is failing or its Windows volume needs a repair - and only for
REM  someone who has ALREADY COPIED THEIR FILES OFF IT and accepts
REM  that the conversion may destroy what is left. It lifts exactly
REM  those two refusals and nothing else: a wrong machine, a wrong
REM  stick, legacy BIOS, an unknown BitLocker state, a bad image or
REM  a failed boot-file snapshot still stop it.
REM
REM  You will be asked to type one sentence, exactly. Everything it
REM  writes afterwards says DATA LOSS ACCEPTED.
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
echo   upgrade_ - convert ACCEPTING DATA LOSS   (stick: %~d0)
echo ============================================================
echo.
echo   READ THIS FIRST.
echo.
echo   The scanner refuses computers whose drive is failing or whose
echo   Windows volume needs a repair, because converting them can
echo   silently lose files. This launcher lets you go ahead anyway.
echo.
echo   Before you type anything:
echo     - copy every file you care about OFF this computer, now;
echo     - assume anything still on it may be gone afterwards;
echo     - know that a failing drive can stop working at any point,
echo       including in the middle of the conversion.
echo.
echo   Step 1 of 5: scanning this computer (nothing is changed)...
echo.
if not exist "%~dp0upgrade_\reports" mkdir "%~dp0upgrade_\reports"
powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0upgrade-scan.ps1" -Json -OutDir "%~dp0upgrade_\reports"
powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0upgrade-scan.ps1" -DumpMachine "%~dp0machine-capture.json" >nul

echo.
echo   Step 2 of 5: your acknowledgement.
echo.
echo   Type the following sentence EXACTLY, then press Enter:
echo.
echo       I confirm that I understand the risks and could lose data
echo.
set ACK=
set /p ACK=  ^>
if not "%ACK%"=="I confirm that I understand the risks and could lose data" (
  echo.
  echo   That is not the sentence. Nothing was changed.
  pause
  exit /b 1
)

echo.
echo   Step 3 of 5: writing the job for this machine...
echo.
powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0New-Job.ps1" -StickDrive %~d0 -OutDir "%~dp0upgrade_" -ScanDir "%~dp0upgrade_\reports" -Desktop kde -IfCannotKeep stop -AcknowledgeDataLoss "%ACK%"
if %errorlevel% neq 0 (
  echo.
  echo   No job was written - the reasons are above. Nothing was changed.
  echo   (A refusal your acknowledgement cannot lift stays a refusal.)
  pause
  exit /b 1
)

echo.
echo   Step 4 of 5: generating the kickstart...
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
echo   Step 5 of 5: the prologue, DATA LOSS ACCEPTED.
echo.
set WORD=
set /p WORD=  Type CONVERT (in capitals) to continue, anything else to stop:
if not "%WORD%"=="CONVERT" (
  echo.
  echo   Not confirmed. Nothing was changed.
  pause
  exit /b 1
)
powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0Invoke-Prologue.ps1" -Start -StickDrive %~d0 -ConfirmWord %WORD% -AcknowledgeDataLoss "%ACK%"
if %errorlevel% neq 0 (
  echo.
  echo   The prologue stopped - read the message above. If it says STOPPED,
  echo   the record is in upgrade_\outcome.json on this stick. Close this window.
  pause
  exit /b 1
)
