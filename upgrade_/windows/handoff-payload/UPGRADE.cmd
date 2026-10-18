@echo off
REM ============================================================
REM  upgrade_ - the window
REM
REM  Double-click this to start. UPGRADE.exe is the whole product on
REM  the Windows side: it scans this computer, says what it found,
REM  asks what you want, and does the conversion (or a cancel). Nothing
REM  else on this stick needs to be run by hand.
REM
REM  A computer the scanner refused for a failing drive or a Windows
REM  volume that needs a repair has its own launcher:
REM  UPGRADE-ACCEPTING-DATA-LOSS.cmd (RISKS R23).
REM ============================================================
if not exist "%~dp0UPGRADE.exe" (
  echo   ERROR: UPGRADE.exe is not on this stick - this is not a complete kit.
  pause
  exit /b 1
)
start "" "%~dp0UPGRADE.exe"
