@echo off
REM ============================================================
REM  upgrade_ - STORAGE-MODE TEST  (V5 / R1: both SATA modes, one click)
REM
REM  Scans this computer in BOTH SATA modes so the scanner's
REM  "Storage controller mode" line is proven on real hardware:
REM  once as it is now, once after you change SATA Mode on the
REM  setup screen, then once more after you change it back. The
REM  computer does everything else itself: it restarts straight
REM  into the setup screen, boots Safe Mode once so Windows binds
REM  the driver for the re-enumerated controller, scans with
REM  nobody signed in, and undoes everything it armed.
REM
REM  YOUR PART, twice, on the setup screen (the computer opens it
REM  itself, or tap F2 repeatedly as the screen goes dark):
REM    Main tab -> SATA Mode -> the mode the window asked for
REM    -> F10 -> Yes.  (Acer: Ctrl+S on the Main tab if SATA
REM    Mode is hidden. If it is still missing, Esc, exit WITHOUT
REM    saving - the test stops by itself.)
REM  Then, at the black Safe Mode sign-in screen: power icon
REM  (bottom right) -> Restart. No password needed.
REM
REM  Your files are not touched. Windows is expected to boot in
REM  either mode. Leave this stick in the whole time. When it is
REM  done, sign in: a window shows the result. The record is in
REM  upgrade_\storage-mode\ on this stick. Never edit it.
REM ============================================================

net session >nul 2>&1
if %errorlevel% neq 0 (
  echo Asking for administrator access - click "Yes" on the prompt...
  powershell -NoProfile -Command "Start-Process -FilePath '%~f0' -Verb RunAs"
  exit /b
)

cd /d "%~dp0"
if not exist "%~dp0Test-StorageMode.ps1" (
  echo   ERROR: Test-StorageMode.ps1 is not on this stick - this is not a complete kit.
  pause
  exit /b 1
)
if not exist "%~dp0upgrade-scan.ps1" (
  echo   ERROR: upgrade-scan.ps1 is not on this stick - this is not a complete kit.
  pause
  exit /b 1
)

echo.
echo ============================================================
echo   upgrade_ - storage-mode test   (stick: %~d0)
echo ============================================================
echo.
echo   Three scans, two restarts into the setup screen where YOU
echo   change SATA Mode (the window that follows says which way).
echo   Nothing on the disk is changed; Windows keeps booting.
echo.
powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0Test-StorageMode.ps1" -Start -StickDrive %~d0
if %errorlevel% neq 0 (
  echo.
  echo   The test did not start - read the message above. Nothing was changed.
  pause
  exit /b 1
)
