<#
.SYNOPSIS
    Run one of the kit's scripts with everything it prints copied to a log on the stick.

.DESCRIPTION
    The launchers (RUN-CONVERT.cmd, RUN-CONVERT-ACCEPTING-DATA-LOSS.cmd,
    RUN-VERIFY.cmd) run each step through this. It starts the script in a
    child Windows PowerShell, echoes every line to the console as it comes,
    appends the same lines to -Log (UTF-8, with a stamped header per step),
    and exits with the script's own exit code so the launcher's error checks
    keep working. Nothing else changes: the scripts do not know they are
    wrapped.

    Why (rule 5, CLAUDE.md - every contact with a real machine leaves a
    capture): on the Acer Aspire, 2026-09-17, the acknowledged-data-loss
    launcher ended at the kickstart step with a message nobody captured,
    and the stick came back with no trace of why.

    Colours are lost in the child's output (its console is a pipe); the
    text is the evidence.

.EXAMPLE
    powershell -NoProfile -ExecutionPolicy Bypass -File Invoke-Logged.ps1 -Log D:\upgrade_\convert.log -Script D:\New-Job.ps1 -StickDrive D: -OutDir D:\upgrade_
#>
param(
    [Parameter(Mandatory = $true)][string]$Log,
    [Parameter(Mandatory = $true)][string]$Script,
    [Parameter(ValueFromRemainingArguments = $true)][string[]]$Rest
)
$ErrorActionPreference = 'Continue'
$dir = Split-Path -Parent $Log
if ($dir -and -not (Test-Path $dir)) { New-Item -ItemType Directory -Path $dir -Force | Out-Null }
function Add-Line([string]$Text) { try { Add-Content -Path $Log -Value $Text -Encoding UTF8 } catch { } }
$stamp = (Get-Date).ToUniversalTime().ToString('yyyy-MM-ddTHH:mm:ssZ')
Add-Line ''
Add-Line "==== $stamp  $(Split-Path -Leaf $Script) $(@($Rest) -join ' ')"
$code = 1
try {
    & powershell.exe -NoProfile -ExecutionPolicy Bypass -File $Script @Rest 2>&1 | ForEach-Object { $line = "$_"; Write-Host $line; Add-Line $line }
    $code = [int]$LASTEXITCODE
} catch {
    Write-Host "  $_" -ForegroundColor Red; Add-Line "wrapper: $_"; $code = 1
}
Add-Line "==== exit $code"
exit $code
