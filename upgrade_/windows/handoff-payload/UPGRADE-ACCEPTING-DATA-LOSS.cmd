@echo off
REM ============================================================
REM  upgrade_ - the window, ACCEPTING DATA LOSS
REM
REM  This is the separate launcher rule #1 asks for (RISKS R23).
REM  It opens UPGRADE.exe on the data-loss path: for a computer the
REM  scanner refused because its DRIVE is failing or its Windows
REM  volume needs a repair. The window asks the sentence, exactly,
REM  before anything else, and says DATA LOSS ACCEPTED on every
REM  screen after it. It lifts that refusal and nothing else.
REM  Everything else is RUN-CONVERT-ACCEPTING-DATA-LOSS.cmd's flow.
REM ============================================================
if not exist "%~dp0UPGRADE.exe" (
  echo   ERROR: UPGRADE.exe is not on this stick - this is not a complete kit.
  pause
  exit /b 1
)
start "" "%~dp0UPGRADE.exe" --accepting-data-loss
