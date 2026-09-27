@echo off
REM ============================================================
REM  upgrade_ - ERASE THIS COMPUTER AND INSTALL FEDORA
REM
REM  Everything on this computer's internal drives is deleted:
REM  Windows, programs, settings and every file. Nothing is kept.
REM  Fedora Linux is installed in its place. (RISKS R27; decided
REM  2026-09-26. Carrying your files across is a later version.)
REM
REM  What it does, in order:
REM    1. scans this computer (changes nothing)
REM    2. asks you to type the erase sentence, exactly
REM    3. asks you to choose the password for your Linux account
REM    4. writes the job, naming every drive it will erase
REM    5. restarts into the installer. There, a 2-minute countdown
REM       comes first: press any key during it to cancel and come
REM       back to Windows, untouched. When it ends, the drives are
REM       erased and Fedora is installed. You can walk away.
REM ============================================================

net session >nul 2>&1
if %errorlevel% neq 0 (
  echo Asking for administrator access - click "Yes" on the prompt...
  powershell -NoProfile -Command "Start-Process -FilePath '%~f0' -Verb RunAs"
  exit /b
)

cd /d "%~dp0"
if not exist "%~dp0upgrade_" mkdir "%~dp0upgrade_"
echo.>> "%~dp0upgrade_\convert.log"
echo ======== %date% %time%  RUN-ERASE-AND-INSTALL.cmd on %COMPUTERNAME%  (stick %~d0)>> "%~dp0upgrade_\convert.log"
for %%f in (Invoke-Logged.ps1 Invoke-Prologue.ps1 upgrade-scan.ps1 New-Job.ps1 Harvest-UpgradeState.ps1 Read-Password.ps1 New-Kickstart.ps1 EFI\BOOT\BOOTX64.EFI images\install.img upgrade_\verify.sh upgrade_\outcome.sh upgrade_\LiveOS\kde.squashfs upgrade_\LiveOS\gnome.squashfs SHA256SUMS) do (
  if not exist "%~dp0%%f" (
    echo   ERROR: %%f is not on this stick - this is not a complete kit.
    pause
    exit /b 1
  )
)

echo.
echo ============================================================
echo   upgrade_ - ERASE THIS COMPUTER AND INSTALL FEDORA
echo   stick: %~d0
echo ============================================================
echo.
echo   READ THIS FIRST.
echo.
echo   This deletes EVERYTHING on this computer's drives: Windows, every
echo   program, every setting and every file. Nothing is kept and nothing
echo   is copied anywhere. Fedora Linux is installed in its place.
echo.
echo   Before you type anything, copy every file you want to keep OFF this
echo   computer.
echo.
echo   Nothing changes until the very end: the computer restarts into the
echo   installer, and a 2-minute countdown appears on the screen. Press any
echo   key during the countdown to cancel - Windows comes back untouched.
echo   When the countdown ends, the drives are erased. You can walk away.
echo.
echo   Step 1 of 6: scanning this computer (nothing is changed)...
echo.
if not exist "%~dp0upgrade_\reports" mkdir "%~dp0upgrade_\reports"
powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0Invoke-Logged.ps1" -Log "%~dp0upgrade_\convert.log" -Script "%~dp0upgrade-scan.ps1" -Json -OutDir "%~dp0upgrade_\reports"
powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0Invoke-Logged.ps1" -Log "%~dp0upgrade_\convert.log" -Script "%~dp0upgrade-scan.ps1" -DumpMachine "%~dp0machine-capture.json" >nul

:choose
echo.
echo   Step 2 of 6: what this computer shows when it starts.
echo.
echo     1  KDE Plasma desktop - looks and works most like Windows
echo     2  GNOME desktop - simpler, one clean workspace
echo     3  Text console only - for people who know Linux commands
echo.
set PICK=
set /p PICK=  Type 1, 2 or 3 and press Enter:
set DESKTOP=
set STARTAT=
if "%PICK%"=="1" set DESKTOP=kde
if "%PICK%"=="1" set STARTAT=desktop
if "%PICK%"=="2" set DESKTOP=gnome
if "%PICK%"=="2" set STARTAT=desktop
if "%PICK%"=="3" set DESKTOP=kde
if "%PICK%"=="3" set STARTAT=console
if not defined STARTAT (
  echo   Please type 1, 2 or 3.
  goto choose
)
echo %date% %time%  chose: %DESKTOP%, starts at the %STARTAT%>> "%~dp0upgrade_\convert.log"

echo.
echo   Step 3 of 6: your decision.
echo.
echo   To erase everything on this computer and install Fedora, type the
echo   following sentence EXACTLY, then press Enter. Anything else stops here.
echo.
echo       I confirm that everything on this computer will be deleted and nothing will be kept
echo.
set ERASE=
set /p ERASE=  ^>
if not "%ERASE%"=="I confirm that everything on this computer will be deleted and nothing will be kept" (
  echo.
  echo   That is not the sentence. Nothing was changed.
  echo %date% %time%  the erase sentence was not typed exactly; stopped>> "%~dp0upgrade_\convert.log"
  pause
  exit /b 1
)

echo.
echo   Step 4 of 6: the password for your new Linux account.
set PWFILE=%TEMP%\upgrade-pw-%RANDOM%%RANDOM%.txt
REM run directly, never through Invoke-Logged: nothing typed here is logged
powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0Read-Password.ps1" -OutFile "%PWFILE%"
if %errorlevel% neq 0 (
  if exist "%PWFILE%" del "%PWFILE%"
  echo.
  echo   No password was set. Nothing was changed.
  echo %date% %time%  no password was set; stopped>> "%~dp0upgrade_\convert.log"
  pause
  exit /b 1
)

echo.
echo   Step 5 of 6: writing the job - it names every drive that will be erased...
echo.
powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0Invoke-Logged.ps1" -Log "%~dp0upgrade_\convert.log" -Script "%~dp0New-Job.ps1" -StickDrive %~d0 -OutDir "%~dp0upgrade_" -ScanDir "%~dp0upgrade_\reports" -Desktop %DESKTOP% -StartAt %STARTAT% -IfCannotKeep stop -EraseEverything "%ERASE%" -PasswordHashFile "%PWFILE%"
set JOBERR=%errorlevel%
if exist "%PWFILE%" del "%PWFILE%"
if %JOBERR% neq 0 (
  echo.
  echo   No job was written - the reasons are above. Nothing was changed.
  pause
  exit /b 1
)
powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0Invoke-Logged.ps1" -Log "%~dp0upgrade_\convert.log" -Script "%~dp0New-Kickstart.ps1" -JobPath "%~dp0upgrade_\job.json" -OutFile "%~dp0upgrade_\ks.cfg" -StickLabel UPGV0 -Manifest "%~dp0SHA256SUMS"
if %errorlevel% neq 0 (
  echo.
  echo   The installer's instructions could not be generated. Nothing was changed.
  pause
  exit /b 1
)
if exist "%~dp0upgrade_\boot-verify" del "%~dp0upgrade_\boot-verify"
if exist "%~dp0upgrade_\boot-install" del "%~dp0upgrade_\boot-install"

echo.
echo   Step 6 of 6: restarting into the installer.
echo.
echo   Leave the USB stick in. After the restart a 2-minute countdown appears:
echo   press any key during it to cancel and come back to Windows, untouched.
echo   If Windows has an update waiting, the computer restarts first to let
echo   it finish, then carries on by itself.
echo.
powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0Invoke-Logged.ps1" -Log "%~dp0upgrade_\convert.log" -Script "%~dp0Invoke-Prologue.ps1" -Start -StickDrive %~d0 -EraseConsent "%ERASE%"
if %errorlevel% neq 0 (
  echo.
  echo   It stopped before anything was changed - read the message above.
  echo   The record is in upgrade_\outcome.json on this stick. Close this window.
  pause
  exit /b 1
)
